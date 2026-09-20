//! The Lead plan-follow-up-synthesis loop.
//!
//! The Lead plans (delegating one or more structured subtasks), follows up
//! on worker results, and synthesizes a final answer grounded in the actual
//! results of completed tasks. It is bounded by a maximum number of rounds,
//! tasks, and (via the scheduler) attempts, so it cannot loop without bound.
//!
//! The objective and the final result are durable: the Lead records the
//! objective as a root task and persists the final answer as that root task's
//! successful result, so both can be reconstructed from the board alone.
//!
//! The brain is asynchronous, fallible, and stateful (`&mut self`), because a
//! real Lead may hold a resident Codex thread/process. A brain failure
//! propagates out of the loop instead of being swallowed.

use async_trait::async_trait;

use crate::board::{
    running_attempt, AgentMessage, BoardError, SelectedArtifactRef, TaskAttempt, TaskBoard,
    TaskStatus,
};
use crate::registry::{AgentTaskResult, TaskKind};
use crate::run_event::{bounded_event_text, LeadPhase, RunEvent};
use crate::scheduler::{ScheduleError, Scheduler, TaskSpec};

/// The Lead's structured output boundary.
#[derive(Debug, Clone)]
pub enum LeadDecision {
    /// Create structured subtasks (the first round).
    Delegate(Vec<TaskSpec>),
    /// Create follow-up tasks based on worker results.
    FollowUp(Vec<TaskSpec>),
    /// Finish with a final result grounded in completed tasks.
    Complete(TeamResult),
}

/// The final team result.
#[derive(Debug, Clone)]
pub struct TeamResult {
    pub answer: String,
    /// The completed tasks whose actual results ground this answer.
    pub task_refs: Vec<u64>,
    /// Exact artifact digests selected by the Lead for this answer.
    pub artifact_refs: Vec<SelectedArtifactRef>,
}

/// The context handed to the Lead brain each round.
///
/// It carries bounded, board-derived facts a real model brain needs — the
/// scheduler's routable agents, succeeded results, artifacts, failed attempts,
/// and messages addressed to the Lead — and nothing more (no hidden reasoning,
/// no raw transcripts).
#[derive(Debug, Clone)]
pub struct LeadContext {
    pub root_task_id: u64,
    pub objective: String,
    pub round: u32,
    /// Agent ids that the scheduler can actually route work to, in scheduler order.
    pub candidates: Vec<String>,
    /// Succeeded task results: (task_id, result summary).
    pub results: Vec<(u64, AgentTaskResult)>,
    /// Artifacts recorded on the Lead's tasks, retaining their owning task.
    pub artifacts: Vec<SelectedArtifactRef>,
    /// Failed task attempts: (task_id, bounded error text). No retry loops here.
    pub failures: Vec<(u64, String)>,
    /// Durable messages addressed to the Lead.
    pub messages: Vec<AgentMessage>,
}

/// An error from the Lead brain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeadBrainError {
    /// The brain's external runtime failed.
    Unavailable(String),
    /// The model's output violated the strict contract.
    InvalidDecision(String),
    /// The brain refused to decide.
    Rejected(String),
}

impl std::fmt::Display for LeadBrainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeadBrainError::Unavailable(m) => write!(f, "lead brain unavailable: {m}"),
            LeadBrainError::InvalidDecision(m) => {
                write!(f, "lead brain produced an invalid decision: {m}")
            }
            LeadBrainError::Rejected(m) => write!(f, "lead brain rejected the decision: {m}"),
        }
    }
}

impl std::error::Error for LeadBrainError {}

/// Produces the Lead's next decision from the current context.
///
/// `&mut self` is intentional: a real Lead may hold a resident Codex
/// thread/process. The brain is `Send` (not `Send + Sync`), because it is only
/// ever accessed through `&mut`.
#[async_trait]
pub trait LeadBrain: Send {
    /// Decide the next step given `ctx`.
    async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError>;
}

/// An error from the Lead loop.
#[derive(Debug, Clone)]
pub enum LeadError {
    /// A scheduling operation failed.
    Schedule(ScheduleError),
    /// A task-board operation failed.
    Board(BoardError),
    /// The Lead brain failed or refused to decide.
    Brain(LeadBrainError),
    /// The maximum number of rounds was reached without completion.
    MaxRounds,
    /// The maximum number of tasks was exceeded.
    TooManyTasks,
    /// A follow-up was requested before any worker result or failure existed.
    FollowUpWithoutResult,
    /// The final result does not reference completed tasks.
    CompletionNotGrounded,
}

/// The Lead's failure as a sentence a person can act on, rather than as the
/// enum's structure: the durable failure row and every user-facing surface
/// carry this text.
impl std::fmt::Display for LeadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schedule(ScheduleError::Board(error)) => {
                write!(f, "the lead could not schedule its work: {error}")
            }
            Self::Schedule(ScheduleError::JoinFailed) => {
                write!(f, "a scheduled task did not finish cleanly")
            }
            Self::Board(error) => write!(f, "a task board operation failed: {error}"),
            Self::Brain(error) => write!(f, "{error}"),
            Self::MaxRounds => write!(f, "the lead reached its round limit without a final answer"),
            Self::TooManyTasks => {
                write!(f, "the lead reached its task limit without a final answer")
            }
            Self::FollowUpWithoutResult => write!(
                f,
                "the lead asked for a follow-up before any worker result or failure existed"
            ),
            Self::CompletionNotGrounded => write!(
                f,
                "the lead completed without grounding the answer in completed tasks"
            ),
        }
    }
}

impl From<ScheduleError> for LeadError {
    fn from(e: ScheduleError) -> Self {
        Self::Schedule(e)
    }
}

impl From<BoardError> for LeadError {
    fn from(e: BoardError) -> Self {
        Self::Board(e)
    }
}

impl From<LeadBrainError> for LeadError {
    fn from(e: LeadBrainError) -> Self {
        Self::Brain(e)
    }
}

/// The Lead: plans, follows up on worker results, and synthesizes the final
/// answer. Bounded by a maximum number of rounds and tasks, and (through the
/// scheduler) a maximum number of attempts per task.
pub struct Lead<B: TaskBoard + Send + 'static> {
    brain: Box<dyn LeadBrain>,
    scheduler: Scheduler<B>,
    max_rounds: u32,
    max_tasks: usize,
    /// The agent id that addresses this Lead: the id directed messages are
    /// read from and the id persisted on the root's settling attempt.
    lead_agent: String,
    task_ids: Vec<u64>,
    root_id: Option<u64>,
}

impl<B: TaskBoard + Send + 'static> Lead<B> {
    /// Build a Lead. The scheduler's `max_retries` is the per-task attempt cap.
    /// `lead_agent` is the Lead's own registry id, which is configurable.
    pub fn new(
        brain: Box<dyn LeadBrain>,
        scheduler: Scheduler<B>,
        max_rounds: u32,
        max_tasks: usize,
        lead_agent: impl Into<String>,
    ) -> Self {
        Self {
            brain,
            scheduler,
            max_rounds,
            max_tasks,
            lead_agent: lead_agent.into(),
            task_ids: Vec::new(),
            root_id: None,
        }
    }

    /// All task ids created so far (the task tree, in creation order).
    pub fn task_ids(&self) -> &[u64] {
        &self.task_ids
    }

    /// The root task that holds the objective, if the loop has started.
    pub fn root_task_id(&self) -> Option<u64> {
        self.root_id
    }

    /// Run the plan-follow-up-synthesis loop for `objective`.
    pub async fn run(&mut self, objective: &str) -> Result<TeamResult, LeadError> {
        // Record the objective as a durable root task (T02/T16); the delegated
        // subtasks hang off it, and the final answer is persisted on it. The
        // root's own Lead attempt is recorded Running first, so this execution
        // owns an independent attempt row exactly like a product run does.
        let root_id = {
            let mut board = self.scheduler.board().lock().unwrap();
            let root = board
                .create_task(objective, None, TaskKind::Reasoning, None)
                .map_err(LeadError::Board)?;
            board
                .assign(root, &self.lead_agent)
                .map_err(LeadError::Board)?;
            board
                .record_attempt(&TaskAttempt {
                    task_id: root,
                    attempt: 1,
                    agent_id: self.lead_agent.clone(),
                    status: TaskStatus::Running,
                    result: None,
                    error: None,
                })
                .map_err(LeadError::Board)?;
            board
                .set_status(root, TaskStatus::Running)
                .map_err(LeadError::Board)?;
            root
        };
        self.root_id = Some(root_id);
        self.run_rounds(objective).await
    }

    /// Run the Lead loop for an objective whose root task already exists on the
    /// board (created by the product runner, with its Lead attempt already
    /// recorded).
    ///
    /// The existing descendants of `root_id` are seeded into the task list, so
    /// a resumed run's context carries the work that already happened (their
    /// results and failures), the task budget counts them, and they are never
    /// scheduled again. A fresh root has no descendants, so this is a no-op.
    pub async fn run_on_root(
        &mut self,
        root_id: u64,
        objective: &str,
    ) -> Result<TeamResult, LeadError> {
        {
            let board = self.scheduler.board().lock().unwrap();
            if board.task(root_id).map_err(LeadError::Board)?.is_none() {
                return Err(LeadError::Board(BoardError::UnknownTask(root_id)));
            }
            self.task_ids = descendants_of(&*board, root_id)?;
        }
        self.root_id = Some(root_id);
        self.run_rounds(objective).await
    }

    /// The shared plan-follow-up-synthesis round loop. The root task must
    /// already be recorded and stored in `self.root_id`.
    async fn run_rounds(&mut self, objective: &str) -> Result<TeamResult, LeadError> {
        let root_id = self.root_id.ok_or_else(|| {
            LeadError::Board(BoardError::Storage(
                "lead loop started without a root".into(),
            ))
        })?;
        let mut round = 0u32;
        loop {
            if round >= self.max_rounds {
                return Err(LeadError::MaxRounds);
            }
            let ctx = self.build_context(objective, round)?;
            // The one event allowed to be ephemeral: the round is about to
            // start and nothing about it is durable until the decision has been
            // acted on. It is emitted outside any board lock.
            self.scheduler.sink().emit(&RunEvent::LeadRoundStarted {
                root_task_id: root_id,
                round,
                phase: LeadPhase::for_round(round),
            });
            match self.brain.decide(&ctx).await? {
                LeadDecision::Delegate(specs) => {
                    self.extend_tasks(&specs, root_id).await?;
                    round += 1;
                }
                LeadDecision::FollowUp(specs) => {
                    if ctx.results.is_empty() && ctx.failures.is_empty() {
                        return Err(LeadError::FollowUpWithoutResult);
                    }
                    self.extend_tasks(&specs, root_id).await?;
                    round += 1;
                }
                LeadDecision::Complete(result) => {
                    // A resumed run starts at local round zero but can already
                    // have completed descendants. Grounding is a board fact,
                    // not a property of this process's round counter.
                    self.verify_completion(&result)?;
                    self.persist_final(root_id, &result)?;
                    return Ok(result);
                }
            }
        }
    }

    /// Build the Lead's context from the durable board: the scheduler's
    /// routable agents, completed task results, artifacts, failed attempts, and
    /// messages addressed to the Lead. This is how a worker's result, artifact,
    /// and message reach the Lead's next round (T07/T08/T09) without a human
    /// copying anything. Board read errors are propagated, never swallowed.
    fn build_context(&self, objective: &str, round: u32) -> Result<LeadContext, LeadError> {
        let root_task_id = self.root_id.ok_or_else(|| {
            LeadError::Board(BoardError::Storage("lead context without a root".into()))
        })?;
        let board = self.scheduler.board().lock().unwrap();
        let mut results = Vec::new();
        let mut artifacts = Vec::new();
        let mut failures = Vec::new();
        for &id in &self.task_ids {
            let record = board.task(id).map_err(LeadError::Board)?;
            if let Some(record) = record {
                match record.status {
                    TaskStatus::Succeeded => {
                        let attempts = board.attempts(id).map_err(LeadError::Board)?;
                        if let Some(last) = attempts
                            .iter()
                            .rev()
                            .find(|a| a.status == TaskStatus::Succeeded)
                        {
                            if let Some(summary) = &last.result {
                                results.push((
                                    id,
                                    AgentTaskResult {
                                        task_id: id,
                                        summary: summary.clone(),
                                        artifacts: Vec::new(),
                                        message: None,
                                    },
                                ));
                            }
                        }
                    }
                    TaskStatus::Failed => {
                        let attempts = board.attempts(id).map_err(LeadError::Board)?;
                        if let Some(last) = attempts
                            .iter()
                            .rev()
                            .find(|a| a.status == TaskStatus::Failed)
                        {
                            let text = last
                                .error
                                .clone()
                                .unwrap_or_else(|| "task failed without an error".into());
                            failures.push((id, bounded_error(&text)));
                        }
                    }
                    _ => {}
                }
                let arts = board.artifacts(id).map_err(LeadError::Board)?;
                artifacts.extend(arts.into_iter().map(|artifact| SelectedArtifactRef {
                    task_id: id,
                    artifact,
                }));
            }
        }
        let messages = board
            .messages_to(&self.lead_agent)
            .map_err(LeadError::Board)?;
        let candidates = self
            .scheduler
            .registry()
            .agent_ids()
            .into_iter()
            .map(str::to_string)
            .collect();
        Ok(LeadContext {
            root_task_id,
            objective: objective.to_string(),
            round,
            candidates,
            results,
            artifacts,
            failures,
            messages,
        })
    }

    /// Schedule a batch of subtasks and remember their ids. Both delegated and
    /// follow-up tasks are forced to hang off `root_id`: the model may not name
    /// its own parent through a `TaskSpec`, so it cannot create a nested task
    /// tree. Only product code may create a nested parent (by scheduling a spec
    /// directly).
    async fn extend_tasks(&mut self, specs: &[TaskSpec], root_id: u64) -> Result<(), LeadError> {
        if self.task_ids.len() + specs.len() > self.max_tasks {
            return Err(LeadError::TooManyTasks);
        }
        let modified: Vec<TaskSpec> = specs
            .iter()
            .map(|s| TaskSpec {
                objective: s.objective.clone(),
                kind: s.kind,
                target: s.target.clone(),
                parent: Some(root_id),
                context: s.context.clone(),
            })
            .collect();
        let results = self.scheduler.schedule(&modified).await?;
        self.task_ids.extend(results.iter().map(|r| r.task_id));
        Ok(())
    }

    /// The final result must reference real, completed tasks that are
    /// descendants of the current root (T16): no test fixture may splice in an
    /// ungrounded answer, and the model may not select a task outside the tree
    /// it was asked to solve. Board read errors are propagated, not conflated
    /// with an ungrounded completion.
    fn verify_completion(&self, result: &TeamResult) -> Result<(), LeadError> {
        if result.task_refs.is_empty() {
            return Err(LeadError::CompletionNotGrounded);
        }
        let root_id = self.root_id.ok_or_else(|| {
            LeadError::Board(BoardError::Storage("completion without a root".into()))
        })?;
        let board = self.scheduler.board().lock().unwrap();
        for &task_id in &result.task_refs {
            // The root itself is not an acceptable selection.
            if task_id == root_id {
                return Err(LeadError::CompletionNotGrounded);
            }
            let record = board
                .task(task_id)
                .map_err(LeadError::Board)?
                .ok_or(LeadError::CompletionNotGrounded)?;
            if record.status != TaskStatus::Succeeded {
                return Err(LeadError::CompletionNotGrounded);
            }
            if !descends_from(&*board, task_id, root_id)? {
                return Err(LeadError::CompletionNotGrounded);
            }
        }
        for selected in &result.artifact_refs {
            if !result.task_refs.contains(&selected.task_id) {
                return Err(LeadError::CompletionNotGrounded);
            }
            let artifacts = board
                .artifacts(selected.task_id)
                .map_err(LeadError::Board)?;
            if !artifacts
                .iter()
                .any(|artifact| artifact == &selected.artifact)
            {
                return Err(LeadError::CompletionNotGrounded);
            }
        }
        Ok(())
    }

    /// Persist the final answer and settle the root task as succeeded, so the
    /// objective and the final result are both reconstructable from the board.
    ///
    /// Every execution records its own root attempt as `Running` before the
    /// external Lead turn (the product runner and a resume both do), so the row
    /// this settles is this execution's attempt rather than a reused number.
    /// The refs, the attempt, its binding, and the root status are one atomic
    /// commit: a crash cannot expose a succeeded attempt next to a root that
    /// still looks resumable.
    fn persist_final(&self, root_id: u64, result: &TeamResult) -> Result<(), LeadError> {
        let mut board = self.scheduler.board().lock().unwrap();
        let attempts = board.attempts(root_id).map_err(LeadError::Board)?;
        let attempt = running_attempt(&attempts, &self.lead_agent)
            .map(|running| running.attempt)
            .ok_or_else(|| {
                LeadError::Board(BoardError::Storage(
                    "the root has no running Lead attempt to settle".into(),
                ))
            })?;
        let attempt = TaskAttempt {
            task_id: root_id,
            attempt,
            agent_id: self.lead_agent.clone(),
            status: TaskStatus::Succeeded,
            result: Some(result.answer.clone()),
            error: None,
        };
        board
            .commit_root_final(&attempt, &result.task_refs, &result.artifact_refs)
            .map_err(LeadError::Board)?;
        Ok(())
    }
}

/// Whether `task_id` has `root_id` as an ancestor by walking `parent_task`
/// upward. `task_id == root_id` is not a descendant. A bounded hop count breaks
/// cycles and pathological parent chains.
fn descends_from<B: TaskBoard>(board: &B, task_id: u64, root_id: u64) -> Result<bool, LeadError> {
    const MAX_HOPS: usize = 1024;
    let mut current = task_id;
    for _ in 0..MAX_HOPS {
        let record = board
            .task(current)
            .map_err(LeadError::Board)?
            .ok_or(LeadError::CompletionNotGrounded)?;
        match record.parent_task {
            Some(parent) if parent == root_id => return Ok(true),
            Some(parent) => current = parent,
            None => return Ok(false),
        }
    }
    Ok(false)
}

/// Every existing descendant of `root_id`, in creation order, excluding the
/// root itself. Used to seed a resumed run's task list from the durable board.
/// A dangling parent link ends that chain instead of failing the run.
fn descendants_of<B: TaskBoard>(board: &B, root_id: u64) -> Result<Vec<u64>, LeadError> {
    const MAX_HOPS: usize = 1024;
    let mut descendants = Vec::new();
    for id in board.task_ids().map_err(LeadError::Board)? {
        if id == root_id {
            continue;
        }
        let mut current = id;
        for _ in 0..MAX_HOPS {
            let Some(record) = board.task(current).map_err(LeadError::Board)? else {
                break;
            };
            match record.parent_task {
                Some(parent) if parent == root_id => {
                    descendants.push(id);
                    break;
                }
                Some(parent) => current = parent,
                None => break,
            }
        }
    }
    Ok(descendants)
}

/// Bound a diagnostic string without splitting a UTF-8 character. It is the
/// same bound every run event carries (`run_event::bounded_event_text`), so the
/// Lead's context text and the projection's text can never drift apart.
fn bounded_error(text: &str) -> String {
    bounded_event_text(text)
}

/// Reconstruct the final team result from the durable board: the root task's
/// successful result (the answer) and the exact refs selected by the Lead.
/// This deliberately does not infer selections from every successful
/// descendant, because such inference silently changes a final result.
pub fn reconstruct_team_result<B: TaskBoard>(
    board: &B,
    root_id: u64,
) -> Result<TeamResult, BoardError> {
    let attempts = board.attempts(root_id)?;
    let answer = attempts
        .iter()
        .rev()
        .find(|a| a.status == TaskStatus::Succeeded)
        .and_then(|a| a.result.clone())
        .ok_or(BoardError::Storage("no successful root result".into()))?;
    let (task_refs, artifact_refs) = board.final_refs(root_id)?;
    Ok(TeamResult {
        answer,
        task_refs,
        artifact_refs,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use crate::board::{AgentMessage, TaskAttempt, TaskBoard, TaskStatus};
    use crate::lead::{
        reconstruct_team_result, Lead, LeadBrain, LeadBrainError, LeadContext, LeadDecision,
        LeadError, TeamResult,
    };
    use crate::registry::TaskKind;
    use crate::scheduler::{Scheduler, TaskSpec};
    use crate::testutil::{err_driver, ok_driver, trio_registry, MemBoard};

    /// A deterministic Lead brain: delegate two tasks, follow up on a result,
    /// then complete with an answer grounded in the actual results.
    struct ScriptedBrain;

    #[async_trait]
    impl LeadBrain for ScriptedBrain {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            Ok(match ctx.round {
                0 => LeadDecision::Delegate(vec![
                    TaskSpec {
                        objective: "summarize data".into(),
                        kind: TaskKind::Bulk,
                        target: Some("worker-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                    TaskSpec {
                        objective: "fetch utility".into(),
                        kind: TaskKind::Utility,
                        target: Some("utility-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                ]),
                1 => {
                    // T10: a follow-up grounded in a worker result.
                    let parent = ctx.results.first().map(|(id, _)| *id);
                    LeadDecision::FollowUp(vec![TaskSpec {
                        objective: "refine the summary".into(),
                        kind: TaskKind::Reasoning,
                        target: Some("reasoner-a".into()),
                        parent,
                        context: Vec::new(),
                    }])
                }
                _ => {
                    // T16: the answer is synthesized from the actual results.
                    let task_refs: Vec<u64> = ctx.results.iter().map(|(id, _)| *id).collect();
                    let answer = ctx
                        .results
                        .iter()
                        .map(|(_, r)| r.summary.clone())
                        .collect::<Vec<_>>()
                        .join("; ");
                    LeadDecision::Complete(TeamResult {
                        answer,
                        task_refs,
                        artifact_refs: Vec::new(),
                    })
                }
            })
        }
    }

    fn lead() -> Lead<MemBoard> {
        let drivers = BTreeMap::from([
            ("worker-a".to_string(), ok_driver("data summary")),
            ("worker-b".to_string(), ok_driver("worker-b")),
            ("utility-a".to_string(), ok_driver("utility output")),
            ("reasoner-a".to_string(), ok_driver("refined insight")),
        ]);
        let sched = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1);
        Lead::new(Box::new(ScriptedBrain), sched, 5, 10, "lead")
    }

    fn full_drivers() -> BTreeMap<String, Arc<dyn crate::registry::AgentDriver>> {
        BTreeMap::from([
            ("worker-a".to_string(), ok_driver("data summary")),
            ("worker-b".to_string(), ok_driver("worker-b")),
            ("utility-a".to_string(), ok_driver("utility output")),
            ("reasoner-a".to_string(), ok_driver("refined insight")),
        ])
    }

    // T03/T07/T08/T10/T16: this scripted Lead delegates two tasks, follows up
    // on a result, and synthesizes an answer grounded in the actual results.
    #[tokio::test]
    async fn lead_delegates_follows_up_and_synthesizes() {
        let mut lead = lead();
        let result = lead
            .run("analyze the dataset")
            .await
            .expect("lead completes");
        // The answer is grounded in the actual worker results, not a fixture.
        assert!(result.answer.contains("data summary"));
        assert!(result.answer.contains("utility output"));
        assert!(result.task_refs.len() >= 2);
    }

    // T16: the objective and final result are durable; the TeamResult can be
    // reconstructed from the board alone.
    #[tokio::test]
    async fn final_result_is_reconstructable_from_board() {
        let mut lead = lead();
        let result = lead
            .run("analyze the dataset")
            .await
            .expect("lead completes");
        let root_id = lead.root_task_id().expect("root task");
        let board = lead.scheduler.board().lock().unwrap();
        let reconstructed = reconstruct_team_result(&*board, root_id).expect("reconstruct");
        assert!(reconstructed.answer.contains("data summary"));
        assert!(reconstructed.answer.contains("utility output"));
        assert_eq!(reconstructed.task_refs, result.task_refs);
    }

    /// A brain that delegates exactly one subtask in the first round, then
    /// completes from that result.
    struct OneTaskBrain;

    #[async_trait]
    impl LeadBrain for OneTaskBrain {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            Ok(match ctx.round {
                0 => LeadDecision::Delegate(vec![TaskSpec {
                    objective: "only one".into(),
                    kind: TaskKind::Bulk,
                    target: Some("worker-a".into()),
                    parent: None,
                    context: Vec::new(),
                }]),
                _ => LeadDecision::Complete(TeamResult {
                    answer: ctx
                        .results
                        .iter()
                        .map(|(_, result)| result.summary.clone())
                        .collect::<Vec<_>>()
                        .join("; "),
                    task_refs: ctx.results.iter().map(|(id, _)| *id).collect(),
                    artifact_refs: Vec::new(),
                }),
            })
        }
    }

    // The first round may delegate a single subtask: the contract's lower bound
    // is one task, not two. The run must still reach a durable, grounded final
    // result.
    #[tokio::test]
    async fn first_round_accepts_a_single_subtask() {
        let drivers = BTreeMap::from([("worker-a".to_string(), ok_driver("only result"))]);
        let sched = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1);
        let mut lead = Lead::new(Box::new(OneTaskBrain), sched, 5, 10, "lead");
        let result = lead.run("x").await.expect("one subtask is accepted");
        assert_eq!(lead.task_ids().len(), 1);
        assert_eq!(result.task_refs, lead.task_ids());
        assert_eq!(result.answer, "only result");
        // The returned result is the durable one, not just an in-memory value.
        let root_id = lead.root_task_id().expect("root task");
        let board = lead.scheduler.board().lock().unwrap();
        let reconstructed = reconstruct_team_result(&*board, root_id).expect("reconstruct");
        assert_eq!(reconstructed.answer, result.answer);
        assert_eq!(reconstructed.task_refs, result.task_refs);
    }

    // A completion that references no completed task is rejected.
    #[tokio::test]
    async fn ungrounded_completion_is_rejected() {
        struct UngroundedBrain;
        #[async_trait]
        impl LeadBrain for UngroundedBrain {
            async fn decide(&mut self, _ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
                Ok(LeadDecision::Complete(TeamResult {
                    answer: "made up".into(),
                    task_refs: Vec::new(),
                    artifact_refs: Vec::new(),
                }))
            }
        }
        let drivers = BTreeMap::from([("worker-a".to_string(), err_driver("down"))]);
        let sched = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1);
        let mut lead = Lead::new(Box::new(UngroundedBrain), sched, 5, 10, "lead");
        let err = lead.run("x").await.expect_err("ungrounded");
        assert!(matches!(err, LeadError::CompletionNotGrounded));
    }

    /// A brain whose external runtime is unavailable.
    struct UnavailableBrain;

    #[async_trait]
    impl LeadBrain for UnavailableBrain {
        async fn decide(&mut self, _ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            Err(LeadBrainError::Unavailable(
                "resident codex thread died".into(),
            ))
        }
    }

    // A brain failure propagates out of `Lead::run`; the root must not be
    // settled as succeeded.
    #[tokio::test]
    async fn brain_error_propagates_and_root_is_not_succeeded() {
        let drivers = BTreeMap::from([("worker-a".to_string(), ok_driver("ok"))]);
        let sched = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1);
        let mut lead = Lead::new(Box::new(UnavailableBrain), sched, 5, 10, "lead");
        let err = lead.run("x").await.expect_err("brain error propagates");
        assert!(matches!(
            err,
            LeadError::Brain(LeadBrainError::Unavailable(_))
        ));
        let root_id = lead.root_task_id().expect("root exists");
        let board = lead.scheduler.board().lock().unwrap();
        let status = board.task(root_id).unwrap().unwrap().status;
        assert_ne!(status, TaskStatus::Succeeded);
    }

    /// A brain that always delegates two tasks and never completes.
    struct AlwaysDelegateBrain;

    #[async_trait]
    impl LeadBrain for AlwaysDelegateBrain {
        async fn decide(&mut self, _ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            Ok(LeadDecision::Delegate(vec![
                TaskSpec {
                    objective: "a".into(),
                    kind: TaskKind::Bulk,
                    target: Some("worker-a".into()),
                    parent: None,
                    context: Vec::new(),
                },
                TaskSpec {
                    objective: "b".into(),
                    kind: TaskKind::Utility,
                    target: Some("utility-a".into()),
                    parent: None,
                    context: Vec::new(),
                },
            ]))
        }
    }

    #[tokio::test]
    async fn max_rounds_is_enforced() {
        let drivers = BTreeMap::from([
            ("worker-a".to_string(), ok_driver("a")),
            ("utility-a".to_string(), ok_driver("u")),
        ]);
        let sched = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1);
        let mut lead = Lead::new(Box::new(AlwaysDelegateBrain), sched, 1, 10, "lead");
        let err = lead.run("x").await.expect_err("round cap");
        assert!(matches!(err, LeadError::MaxRounds));
    }

    /// A brain that delegates with a hostile `parent` and then completes.
    struct HostileDelegateBrain;

    #[async_trait]
    impl LeadBrain for HostileDelegateBrain {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            Ok(match ctx.round {
                0 => LeadDecision::Delegate(vec![
                    TaskSpec {
                        objective: "a".into(),
                        kind: TaskKind::Bulk,
                        target: Some("worker-a".into()),
                        parent: Some(999),
                        context: Vec::new(),
                    },
                    TaskSpec {
                        objective: "b".into(),
                        kind: TaskKind::Utility,
                        target: Some("utility-a".into()),
                        parent: Some(999),
                        context: Vec::new(),
                    },
                ]),
                _ => LeadDecision::Complete(TeamResult {
                    answer: "done".into(),
                    task_refs: ctx.results.iter().map(|(id, _)| *id).collect(),
                    artifact_refs: Vec::new(),
                }),
            })
        }
    }

    // The model cannot name a parent: a delegated task with a hostile parent
    // still hangs off the root.
    #[tokio::test]
    async fn delegate_hostile_parent_is_contained_by_root() {
        let sched = Scheduler::new(trio_registry(), full_drivers(), MemBoard::default(), 1);
        let mut lead = Lead::new(Box::new(HostileDelegateBrain), sched, 5, 10, "lead");
        lead.run("x").await.expect("completes");
        let root_id = lead.root_task_id().expect("root");
        let task_ids = lead.task_ids().to_vec();
        assert_eq!(task_ids.len(), 2);
        let board = lead.scheduler.board().lock().unwrap();
        for id in task_ids {
            assert_eq!(board.task(id).unwrap().unwrap().parent_task, Some(root_id));
        }
    }

    /// A brain that follows up with a hostile `parent` and then completes.
    struct HostileFollowUpBrain;

    #[async_trait]
    impl LeadBrain for HostileFollowUpBrain {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            Ok(match ctx.round {
                0 => LeadDecision::Delegate(vec![
                    TaskSpec {
                        objective: "a".into(),
                        kind: TaskKind::Bulk,
                        target: Some("worker-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                    TaskSpec {
                        objective: "b".into(),
                        kind: TaskKind::Utility,
                        target: Some("utility-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                ]),
                1 => LeadDecision::FollowUp(vec![TaskSpec {
                    objective: "refine".into(),
                    kind: TaskKind::Reasoning,
                    target: Some("reasoner-a".into()),
                    parent: Some(999),
                    context: Vec::new(),
                }]),
                _ => LeadDecision::Complete(TeamResult {
                    answer: "done".into(),
                    task_refs: ctx.results.iter().map(|(id, _)| *id).collect(),
                    artifact_refs: Vec::new(),
                }),
            })
        }
    }

    // A follow-up with a hostile parent still hangs off the root.
    #[tokio::test]
    async fn followup_hostile_parent_is_contained_by_root() {
        let sched = Scheduler::new(trio_registry(), full_drivers(), MemBoard::default(), 1);
        let mut lead = Lead::new(Box::new(HostileFollowUpBrain), sched, 5, 10, "lead");
        lead.run("x").await.expect("completes");
        let root_id = lead.root_task_id().expect("root");
        let task_ids = lead.task_ids().to_vec();
        assert_eq!(task_ids.len(), 3);
        let board = lead.scheduler.board().lock().unwrap();
        for id in task_ids {
            assert_eq!(board.task(id).unwrap().unwrap().parent_task, Some(root_id));
        }
    }

    /// A brain that completes by selecting a rogue, non-descendant task.
    struct RogueSelectingBrain {
        rogue: u64,
    }

    #[async_trait]
    impl LeadBrain for RogueSelectingBrain {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            Ok(match ctx.round {
                0 => LeadDecision::Delegate(vec![
                    TaskSpec {
                        objective: "a".into(),
                        kind: TaskKind::Bulk,
                        target: Some("worker-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                    TaskSpec {
                        objective: "b".into(),
                        kind: TaskKind::Utility,
                        target: Some("utility-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                ]),
                _ => {
                    let mut refs: Vec<u64> = ctx.results.iter().map(|(id, _)| *id).collect();
                    refs.push(self.rogue);
                    LeadDecision::Complete(TeamResult {
                        answer: "done".into(),
                        task_refs: refs,
                        artifact_refs: Vec::new(),
                    })
                }
            })
        }
    }

    // A selected task that is succeeded but not a descendant of the root is
    // rejected, even when the other refs are valid.
    #[tokio::test]
    async fn completion_selecting_non_descendant_is_rejected() {
        let mut board = MemBoard::default();
        let rogue = board
            .create_task("rogue", None, TaskKind::Bulk, Some("worker-a".into()))
            .unwrap();
        board.assign(rogue, "worker-a").unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: rogue,
                attempt: 1,
                agent_id: "worker-a".into(),
                status: TaskStatus::Succeeded,
                result: Some("rogue done".into()),
                error: None,
            })
            .unwrap();
        board.set_status(rogue, TaskStatus::Succeeded).unwrap();
        let sched = Scheduler::new(trio_registry(), full_drivers(), board, 1);
        let mut lead = Lead::new(
            Box::new(RogueSelectingBrain { rogue }),
            sched,
            5,
            10,
            "lead",
        );
        let err = lead.run("x").await.expect_err("non-descendant rejected");
        assert!(matches!(err, LeadError::CompletionNotGrounded));
    }

    /// A brain that completes by selecting the root itself.
    struct RootSelectingBrain;

    #[async_trait]
    impl LeadBrain for RootSelectingBrain {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            Ok(match ctx.round {
                0 => LeadDecision::Delegate(vec![
                    TaskSpec {
                        objective: "a".into(),
                        kind: TaskKind::Bulk,
                        target: Some("worker-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                    TaskSpec {
                        objective: "b".into(),
                        kind: TaskKind::Utility,
                        target: Some("utility-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                ]),
                _ => LeadDecision::Complete(TeamResult {
                    answer: "done".into(),
                    task_refs: vec![ctx.root_task_id],
                    artifact_refs: Vec::new(),
                }),
            })
        }
    }

    #[tokio::test]
    async fn completion_selecting_root_is_rejected() {
        let sched = Scheduler::new(trio_registry(), full_drivers(), MemBoard::default(), 1);
        let mut lead = Lead::new(Box::new(RootSelectingBrain), sched, 5, 10, "lead");
        let err = lead.run("x").await.expect_err("root rejected");
        assert!(matches!(err, LeadError::CompletionNotGrounded));
    }

    /// `run_on_root` settles a root the product runner already created and
    /// whose Lead attempt row already exists.
    #[tokio::test]
    async fn run_on_root_settles_precreated_root() {
        let mut board = MemBoard::default();
        let root_id = board
            .create_task("analyze", None, TaskKind::Reasoning, None)
            .unwrap();
        // The product runner records the Lead attempt before the loop starts.
        board
            .record_attempt(&TaskAttempt {
                task_id: root_id,
                attempt: 1,
                agent_id: "lead".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(root_id, TaskStatus::Running).unwrap();
        let sched = Scheduler::new(trio_registry(), full_drivers(), board, 1);
        let mut lead = Lead::new(Box::new(ScriptedBrain), sched, 5, 10, "lead");
        let result = lead
            .run_on_root(root_id, "analyze")
            .await
            .expect("completes");
        assert_eq!(lead.root_task_id(), Some(root_id));
        let board = lead.scheduler.board().lock().unwrap();
        // The existing attempt row is settled in place, not duplicated.
        let attempts = board.attempts(root_id).unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].status, TaskStatus::Succeeded);
        assert_eq!(attempts[0].result.as_deref(), Some(result.answer.as_str()));
        let record = board.task(root_id).unwrap().unwrap();
        assert_eq!(record.status, TaskStatus::Succeeded);
        let (refs, _) = board.final_refs(root_id).unwrap();
        assert_eq!(refs, result.task_refs);
    }

    /// A root whose Lead attempt is recorded and whose child already succeeded:
    /// the shape a process interruption leaves behind.
    fn resumed_board() -> (MemBoard, u64, u64) {
        let mut board = MemBoard::default();
        let root_id = board
            .create_task("analyze", None, TaskKind::Reasoning, None)
            .unwrap();
        let child_id = board
            .create_task("earlier bulk", Some(root_id), TaskKind::Bulk, None)
            .unwrap();
        board.assign(child_id, "worker-a").unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: child_id,
                attempt: 1,
                agent_id: "worker-a".into(),
                status: TaskStatus::Succeeded,
                result: Some("earlier result".into()),
                error: None,
            })
            .unwrap();
        board.set_status(child_id, TaskStatus::Succeeded).unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: root_id,
                attempt: 1,
                agent_id: "lead".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(root_id, TaskStatus::Running).unwrap();
        (board, root_id, child_id)
    }

    // A resumed run seeds the root's existing descendants: the Lead's next
    // context carries the earlier child's result and the child is never
    // scheduled again.
    #[tokio::test]
    async fn run_on_root_seeds_existing_descendants() {
        let (board, root_id, child_id) = resumed_board();
        let sched = Scheduler::new(trio_registry(), full_drivers(), board, 1);
        let mut lead = Lead::new(Box::new(ScriptedBrain), sched, 5, 4, "lead");
        let result = lead
            .run_on_root(root_id, "analyze")
            .await
            .expect("completes");
        assert!(result.answer.contains("earlier result"));
        assert_eq!(lead.task_ids()[0], child_id);
        assert_eq!(lead.task_ids().len(), 4);
        let board = lead.scheduler.board().lock().unwrap();
        // The seeded child keeps its single attempt: it was not re-run.
        assert_eq!(board.attempts(child_id).unwrap().len(), 1);
    }

    struct CompleteExisting;

    #[async_trait]
    impl LeadBrain for CompleteExisting {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            assert_eq!(ctx.round, 0);
            Ok(LeadDecision::Complete(TeamResult {
                answer: "existing work is sufficient".into(),
                task_refs: ctx.results.iter().map(|(id, _)| *id).collect(),
                artifact_refs: ctx.artifacts.clone(),
            }))
        }
    }

    #[tokio::test]
    async fn resumed_root_can_complete_immediately_without_new_tasks_or_attempts() {
        let (board, root_id, child_id) = resumed_board();
        let sched = Scheduler::new(trio_registry(), full_drivers(), board, 1);
        // No spare task budget: any unnecessary delegation must fail.
        let mut lead = Lead::new(Box::new(CompleteExisting), sched, 1, 1, "lead");
        let result = lead.run_on_root(root_id, "analyze").await.unwrap();
        assert_eq!(result.task_refs, vec![child_id]);
        let board = lead.scheduler.board().lock().unwrap();
        assert_eq!(board.task_ids().unwrap(), vec![root_id, child_id]);
        assert_eq!(board.attempts(child_id).unwrap().len(), 1);
        assert_eq!(
            board.task(root_id).unwrap().unwrap().status,
            TaskStatus::Succeeded
        );
    }

    struct RecoverFailure;

    #[async_trait]
    impl LeadBrain for RecoverFailure {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            if ctx.results.is_empty() {
                let (target, kind, objective) = if ctx.failures.is_empty() {
                    ("worker-a", TaskKind::Bulk, "primary")
                } else {
                    assert!(ctx.failures[0].1.contains("primary unavailable"));
                    (
                        "utility-a",
                        TaskKind::Utility,
                        "recover from primary unavailable",
                    )
                };
                let tasks = vec![TaskSpec {
                    objective: objective.into(),
                    kind,
                    target: Some(target.into()),
                    parent: None,
                    context: Vec::new(),
                }];
                return Ok(if ctx.round == 0 {
                    LeadDecision::Delegate(tasks)
                } else {
                    LeadDecision::FollowUp(tasks)
                });
            }
            Ok(LeadDecision::Complete(TeamResult {
                answer: ctx.results[0].1.summary.clone(),
                task_refs: ctx.results.iter().map(|(id, _)| *id).collect(),
                artifact_refs: Vec::new(),
            }))
        }
    }

    #[tokio::test]
    async fn failure_only_context_can_drive_a_successful_follow_up() {
        let drivers = BTreeMap::from([
            ("worker-a".to_string(), err_driver("primary unavailable")),
            ("utility-a".to_string(), ok_driver("recovered")),
        ]);
        let sched = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1);
        let mut lead = Lead::new(Box::new(RecoverFailure), sched, 3, 2, "lead");
        let result = lead.run("recover").await.unwrap();
        assert_eq!(result.answer, "recovered");
        assert_eq!(result.task_refs, vec![3]);
        let board = lead.scheduler.board().lock().unwrap();
        assert_eq!(board.task(2).unwrap().unwrap().status, TaskStatus::Failed);
        assert_eq!(
            board.task(3).unwrap().unwrap().status,
            TaskStatus::Succeeded
        );
    }

    #[tokio::test]
    async fn follow_up_without_any_terminal_evidence_is_rejected() {
        struct Premature;
        #[async_trait]
        impl LeadBrain for Premature {
            async fn decide(&mut self, _: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
                Ok(LeadDecision::FollowUp(Vec::new()))
            }
        }
        let sched = Scheduler::new(trio_registry(), full_drivers(), MemBoard::default(), 1);
        let mut lead = Lead::new(Box::new(Premature), sched, 3, 2, "lead");
        assert!(matches!(
            lead.run("x").await,
            Err(LeadError::FollowUpWithoutResult)
        ));
    }

    // Seeded descendants count against `max_tasks`, so a resume cannot exceed
    // the run's task budget by ignoring the work that already happened.
    #[tokio::test]
    async fn run_on_root_counts_seeded_descendants_against_the_task_budget() {
        let (board, root_id, _) = resumed_board();
        let sched = Scheduler::new(trio_registry(), full_drivers(), board, 1);
        // Without the seeded child this budget would be enough for the
        // delegate (2) and the follow-up (1).
        let mut lead = Lead::new(Box::new(ScriptedBrain), sched, 5, 3, "lead");
        let err = lead
            .run_on_root(root_id, "analyze")
            .await
            .expect_err("budget counts the seeded child");
        assert!(matches!(err, LeadError::TooManyTasks));
    }

    /// A brain that captures the candidate list it was handed.
    struct CapturingBrain {
        seen: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl LeadBrain for CapturingBrain {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            *self.seen.lock().unwrap() = ctx.candidates.clone();
            Ok(match ctx.round {
                0 => LeadDecision::Delegate(vec![
                    TaskSpec {
                        objective: "a".into(),
                        kind: TaskKind::Bulk,
                        target: Some("worker-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                    TaskSpec {
                        objective: "b".into(),
                        kind: TaskKind::Utility,
                        target: Some("utility-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                ]),
                _ => LeadDecision::Complete(TeamResult {
                    answer: "done".into(),
                    task_refs: ctx.results.iter().map(|(id, _)| *id).collect(),
                    artifact_refs: Vec::new(),
                }),
            })
        }
    }

    #[tokio::test]
    async fn context_candidates_are_the_registered_agent_ids() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sched = Scheduler::new(trio_registry(), full_drivers(), MemBoard::default(), 1);
        let mut lead = Lead::new(
            Box::new(CapturingBrain { seen: seen.clone() }),
            sched,
            5,
            10,
            "lead",
        );
        lead.run("x").await.expect("completes");
        let captured = seen.lock().unwrap().clone();
        assert_eq!(
            captured,
            vec!["reasoner-a", "utility-a", "worker-a", "worker-b"]
        );
    }

    /// A brain that captures the failures it was handed.
    struct FailureCapturingBrain {
        seen: Arc<Mutex<Vec<(u64, String)>>>,
    }

    #[async_trait]
    impl LeadBrain for FailureCapturingBrain {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            Ok(match ctx.round {
                0 => LeadDecision::Delegate(vec![
                    TaskSpec {
                        objective: "u".into(),
                        kind: TaskKind::Utility,
                        target: Some("utility-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                    TaskSpec {
                        objective: "b".into(),
                        kind: TaskKind::Bulk,
                        target: Some("worker-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                ]),
                _ => {
                    *self.seen.lock().unwrap() = ctx.failures.clone();
                    LeadDecision::Complete(TeamResult {
                        answer: "done".into(),
                        task_refs: ctx.results.iter().map(|(id, _)| *id).collect(),
                        artifact_refs: Vec::new(),
                    })
                }
            })
        }
    }

    #[tokio::test]
    async fn context_failures_are_populated_for_failed_child() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        // worker-a always fails and worker-b has no driver, so the bulk task
        // exhausts its candidates and ends Failed.
        let drivers = BTreeMap::from([
            ("worker-a".to_string(), err_driver("worker-a is down")),
            ("utility-a".to_string(), ok_driver("utility output")),
        ]);
        let sched = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1);
        let mut lead = Lead::new(
            Box::new(FailureCapturingBrain { seen: seen.clone() }),
            sched,
            5,
            10,
            "lead",
        );
        lead.run("x").await.expect("completes");
        let captured = seen.lock().unwrap().clone();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, lead.task_ids()[1]);
        assert_eq!(captured[0].1, "worker-a is down");
    }

    /// A brain that captures the directed messages it was handed, then
    /// delegates one task and completes from its result.
    struct MessageCapturingBrain {
        seen: Arc<Mutex<Vec<AgentMessage>>>,
    }

    #[async_trait]
    impl LeadBrain for MessageCapturingBrain {
        async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
            *self.seen.lock().unwrap() = ctx.messages.clone();
            Ok(match ctx.round {
                0 => LeadDecision::Delegate(vec![TaskSpec {
                    objective: "b".into(),
                    kind: TaskKind::Bulk,
                    target: Some("worker-a".into()),
                    parent: None,
                    context: Vec::new(),
                }]),
                _ => LeadDecision::Complete(TeamResult {
                    answer: "done".into(),
                    task_refs: ctx.results.iter().map(|(id, _)| *id).collect(),
                    artifact_refs: Vec::new(),
                }),
            })
        }
    }

    // Directed messages follow the configured Lead id, not the literal "lead".
    #[tokio::test]
    async fn directed_messages_follow_the_configured_lead_id() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut board = MemBoard::default();
        board
            .record_message(&AgentMessage {
                from_agent: "worker-a".into(),
                to_agent: "reasoner-a".into(),
                body: "addressed to the reasoner".into(),
            })
            .unwrap();
        board
            .record_message(&AgentMessage {
                from_agent: "worker-a".into(),
                to_agent: "lead".into(),
                body: "decoy for the literal id".into(),
            })
            .unwrap();
        let sched = Scheduler::new(trio_registry(), full_drivers(), board, 1);
        let mut lead = Lead::new(
            Box::new(MessageCapturingBrain { seen: seen.clone() }),
            sched,
            5,
            10,
            "reasoner-a",
        );
        lead.run("x").await.expect("completes");
        let captured = seen.lock().unwrap().clone();
        let bodies: Vec<&str> = captured.iter().map(|m| m.body.as_str()).collect();
        assert!(
            bodies.contains(&"addressed to the reasoner"),
            "{captured:?}"
        );
        assert!(
            !bodies.contains(&"decoy for the literal id"),
            "{captured:?}"
        );
    }

    // `run` persists the resolved Lead id on the root's settling attempt row.
    #[tokio::test]
    async fn final_attempt_records_the_resolved_lead_id() {
        let sched = Scheduler::new(trio_registry(), full_drivers(), MemBoard::default(), 1);
        let mut lead = Lead::new(Box::new(ScriptedBrain), sched, 5, 10, "reasoner-a");
        lead.run("x").await.expect("completes");
        let root_id = lead.root_task_id().expect("root");
        let board = lead.scheduler.board().lock().unwrap();
        let attempts = board.attempts(root_id).unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].agent_id, "reasoner-a");
        assert_eq!(attempts[0].status, TaskStatus::Succeeded);
    }
}
