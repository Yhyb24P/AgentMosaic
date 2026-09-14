//! Concurrent task scheduling with per-agent concurrency, retry, and
//! deterministic reassignment.
//!
//! Each agent has its own concurrency quota (`max_concurrency`), shared across
//! every scheduled task, so two independent tasks on different agents truly
//! overlap while two tasks on the same agent are capped. A failed task retries
//! the same agent up to a limit, then excludes that agent and reassigns to the
//! next deterministic candidate.
//!
//! The lifecycle is durable: each attempt is persisted as Running *before* the
//! driver runs, then settled to Succeeded/Failed, so a crash always leaves a
//! recoverable state rather than a task stuck at Pending.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use tokio::sync::Semaphore;

use crate::board::{BoardError, TaskAttempt, TaskBoard, TaskStatus};
use crate::registry::{
    AgentDriver, AgentRegistry, AgentTask, AgentTaskResult, AgentTier, TaskKind,
};

/// A structured subtask to create and run.
#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub objective: String,
    pub kind: TaskKind,
    /// An explicit user target (T13); it applies to not-yet-started and
    /// rescheduled tasks.
    pub target: Option<String>,
    /// The parent task, when this is a follow-up.
    pub parent: Option<u64>,
    /// Context handed to the driver.
    pub context: Vec<String>,
}

/// The outcome of scheduling one task.
#[derive(Debug, Clone)]
pub struct ScheduledResult {
    pub task_id: u64,
    pub result: Result<AgentTaskResult, String>,
    /// Every attempt, in order. Failures are kept, never overwritten.
    pub attempts: Vec<TaskAttempt>,
}

/// An error from the scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// A task-board operation failed.
    Board(BoardError),
    /// A spawned task panicked or was cancelled.
    JoinFailed,
}

impl From<BoardError> for ScheduleError {
    fn from(e: BoardError) -> Self {
        Self::Board(e)
    }
}

/// Schedules tasks across a team, honoring each agent's concurrency quota,
/// retrying failures, and reassigning deterministically.
///
/// The board and the per-agent semaphores live for the scheduler's whole
/// lifetime, so every `schedule()` call and every task shares them: the
/// concurrency quota is actually enforced, and the board is written to
/// concurrently as tasks progress.
pub struct Scheduler<B: TaskBoard + Send + 'static> {
    registry: AgentRegistry,
    drivers: BTreeMap<String, Arc<dyn AgentDriver>>,
    board: Arc<Mutex<B>>,
    max_retries: u32,
    semaphores: BTreeMap<String, Arc<Semaphore>>,
}

impl<B: TaskBoard + Send + 'static> Scheduler<B> {
    /// Build a scheduler. `drivers` maps agent id to its driver; `max_retries`
    /// is the per-agent retry limit before reassignment. The per-agent
    /// semaphores are sized by each agent's `max_concurrency` and shared by
    /// every task this scheduler runs.
    pub fn new(
        registry: AgentRegistry,
        drivers: BTreeMap<String, Arc<dyn AgentDriver>>,
        board: B,
        max_retries: u32,
    ) -> Self {
        let semaphores = build_semaphores(&registry);
        Self {
            registry,
            drivers,
            board: Arc::new(Mutex::new(board)),
            max_retries,
            semaphores,
        }
    }

    /// The durable board this scheduler persists to, shared and synchronized
    /// so tasks can write their lifecycle while running.
    pub fn board(&self) -> &Mutex<B> {
        &self.board
    }

    /// The registry this scheduler routes through, in stable agent-id order.
    pub fn registry(&self) -> &AgentRegistry {
        &self.registry
    }

    /// Create and run `specs` concurrently. Each task is bounded by its agent's
    /// shared concurrency quota; on failure it retries the same agent up to
    /// `max_retries`, then reassigns to the next candidate. Each attempt is
    /// persisted to the board as Running before the driver runs, then settled.
    pub async fn schedule(
        &mut self,
        specs: &[TaskSpec],
    ) -> Result<Vec<ScheduledResult>, ScheduleError> {
        // 1. Create the tasks on the board (durable, T02).
        let mut task_ids = Vec::new();
        {
            let mut board = self.board.lock().unwrap();
            for spec in specs {
                let id = board.create_task(
                    &spec.objective,
                    spec.parent,
                    spec.kind,
                    spec.target.clone(),
                )?;
                task_ids.push(id);
            }
        }

        self.run_existing(&task_ids, specs).await
    }

    /// Run one task that already exists on the board after an explicit caller
    /// has moved it back to Pending. This never creates a replacement task;
    /// a resumed external operation is recorded as a new attempt on the same
    /// canonical task.
    pub async fn resume_existing(
        &mut self,
        task_id: u64,
    ) -> Result<ScheduledResult, ScheduleError> {
        let record = self
            .board
            .lock()
            .unwrap()
            .task(task_id)?
            .ok_or(BoardError::UnknownTask(task_id))?;
        if record.status != TaskStatus::Pending {
            return Err(ScheduleError::Board(BoardError::Storage(
                "resume_existing requires an explicitly resumed pending task".into(),
            )));
        }
        let specs = [TaskSpec {
            objective: record.objective,
            kind: record.kind,
            target: record.target,
            parent: record.parent_task,
            context: Vec::new(),
        }];
        let mut results = self.run_existing(&[task_id], &specs).await?;
        Ok(results.remove(0))
    }

    async fn run_existing(
        &self,
        task_ids: &[u64],
        specs: &[TaskSpec],
    ) -> Result<Vec<ScheduledResult>, ScheduleError> {
        // Run them concurrently. Each task persists its own lifecycle
        //    (Assigned -> Running -> terminal) on the shared board, so the
        //    state is durable at every point, not only at the end.
        let mut handles = Vec::new();
        for (i, spec) in specs.iter().enumerate() {
            // A follow-up task's context includes its parent's result, so a
            // worker's result flows into the parent's subsequent context.
            let mut context = spec.context.clone();
            if let Some(parent) = spec.parent {
                let summary = {
                    let board = self.board.lock().unwrap();
                    parent_summary(&*board, parent)
                };
                if let Ok(Some(s)) = summary {
                    context.push(format!("parent[{parent}]: {s}"));
                }
            }
            let task = AgentTask {
                id: task_ids[i],
                objective: spec.objective.clone(),
                kind: spec.kind,
                context,
            };
            let candidates = self.candidate_order(spec.kind, spec.target.as_deref());
            let drivers = self.drivers.clone();
            let semaphores = self.semaphores.clone();
            let board = self.board.clone();
            let max_retries = self.max_retries;
            handles.push(tokio::spawn(async move {
                run_one(&drivers, &semaphores, &board, max_retries, task, candidates).await
            }));
        }

        // 3. Collect results.  `run_one` commits a successful result's message
        //    and artifacts before marking the attempt/task successful, so this
        //    collection phase is read-only with respect to result flow.
        let mut out = Vec::new();
        for (i, handle) in handles.into_iter().enumerate() {
            let (attempts, result) = handle.await.map_err(|_| ScheduleError::JoinFailed)??;
            let task_id = task_ids[i];
            out.push(ScheduledResult {
                task_id,
                result,
                attempts,
            });
        }
        Ok(out)
    }

    /// The candidate agent order for a task: the explicit target first (if it
    /// names a registered agent), then the task tier's agents in stable id
    /// order. Utility work falls back to Workers when no Utility is
    /// registered; it never implicitly targets a Reasoner. Deterministic and
    /// independent of transient completion order.
    fn candidate_order(&self, kind: TaskKind, target: Option<&str>) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        if let Some(t) = target {
            if self.registry.get(t).is_some() {
                order.push(t.to_string());
            }
        }
        let tier = kind.tier();
        for id in self.registry.agent_ids() {
            if self
                .registry
                .get(id)
                .map(|c| c.tier == tier)
                .unwrap_or(false)
            {
                order.push(id.to_string());
            }
        }
        if kind == TaskKind::Utility && order.is_empty() {
            for id in self.registry.agent_ids() {
                if self
                    .registry
                    .get(id)
                    .map(|c| c.tier == AgentTier::Worker)
                    .unwrap_or(false)
                {
                    order.push(id.to_string());
                }
            }
        }
        let mut seen = BTreeSet::new();
        order.retain(|id| seen.insert(id.clone()));
        order
    }
}

/// One shared concurrency semaphore per agent, sized by its
/// `max_concurrency`. Built once for the scheduler's lifetime so the quota is
/// honored across every task and every `schedule()` call.
fn build_semaphores(registry: &AgentRegistry) -> BTreeMap<String, Arc<Semaphore>> {
    let mut m = BTreeMap::new();
    for id in registry.agent_ids() {
        let mc = registry.get(id).map(|c| c.max_concurrency).unwrap_or(1);
        m.insert(id.to_string(), Arc::new(Semaphore::new(mc)));
    }
    m
}

/// The summary of a parent task's last successful attempt, if it has one.
fn parent_summary<B: TaskBoard>(board: &B, parent: u64) -> Result<Option<String>, BoardError> {
    let attempts = board.attempts(parent)?;
    let summary = attempts
        .iter()
        .rev()
        .find(|a| a.status == TaskStatus::Succeeded)
        .and_then(|a| a.result.clone());
    Ok(summary)
}

/// Run one task: walk the candidate agents, retrying each up to `max_retries`
/// under its shared concurrency quota, then reassigning deterministically. Each
/// attempt is persisted as Running before the driver runs and settled after, so
/// the board is durable throughout. Returns the attempts and the final result.
async fn run_one<B: TaskBoard + Send + 'static>(
    drivers: &BTreeMap<String, Arc<dyn AgentDriver>>,
    semaphores: &BTreeMap<String, Arc<Semaphore>>,
    board: &Arc<Mutex<B>>,
    max_retries: u32,
    task: AgentTask,
    candidates: Vec<String>,
) -> Result<(Vec<TaskAttempt>, Result<AgentTaskResult, String>), BoardError> {
    let mut attempts = Vec::new();
    let mut last_result: Option<AgentTaskResult> = None;
    // A recovered task resumes on the same canonical id; prior attempts must
    // remain immutable and the next run gets the next sequence number.
    let mut attempt_seq = board.lock().unwrap().attempts(task.id)?.len() as u32;
    for agent in &candidates {
        let driver = match drivers.get(agent) {
            Some(d) => d,
            None => continue,
        };
        let sem = match semaphores.get(agent) {
            Some(s) => s,
            None => continue,
        };
        // Reflect (re)assignment to this agent durably before any attempt.
        {
            let mut b = board.lock().unwrap();
            b.assign(task.id, agent)?;
        }
        let mut succeeded = false;
        for _ in 0..max_retries {
            attempt_seq += 1;
            // A capacity-queued task has not started its driver yet and must
            // remain Assigned. Acquire the shared permit first, then persist
            // Running immediately before the external driver side effect.
            let permit = sem.acquire().await;
            {
                let mut b = board.lock().unwrap();
                b.record_attempt(&TaskAttempt {
                    task_id: task.id,
                    attempt: attempt_seq,
                    agent_id: agent.clone(),
                    status: TaskStatus::Running,
                    result: None,
                    error: None,
                })?;
                b.set_status(task.id, TaskStatus::Running)?;
            }
            // Inject the directed messages addressed to this agent so they
            // actually reach its context (T09).
            let mut ctx_task = task.clone();
            {
                let b = board.lock().unwrap();
                for m in b.messages_to(agent)? {
                    ctx_task
                        .context
                        .push(format!("[{}] {}", m.from_agent, m.body));
                }
            }
            // Run the driver under the shared concurrency quota. The durable
            // Running attempt now exactly brackets possible side effects.
            let outcome = driver.run_task(ctx_task).await;
            drop(permit);
            // Persist the terminal attempt and settle the task status.
            let terminal = match &outcome {
                Ok(r) => TaskAttempt {
                    task_id: task.id,
                    attempt: attempt_seq,
                    agent_id: agent.clone(),
                    status: TaskStatus::Succeeded,
                    result: Some(r.summary.clone()),
                    error: None,
                },
                Err(e) => TaskAttempt {
                    task_id: task.id,
                    attempt: attempt_seq,
                    agent_id: agent.clone(),
                    status: TaskStatus::Failed,
                    result: None,
                    error: Some(e.clone()),
                },
            };
            {
                let mut b = board.lock().unwrap();
                match &outcome {
                    Ok(result) => b.commit_successful_result(&terminal, result)?,
                    Err(_) => {
                        b.complete_attempt(&terminal)?;
                    }
                }
            }
            match outcome {
                Ok(result) => {
                    attempts.push(terminal);
                    last_result = Some(result);
                    succeeded = true;
                    break;
                }
                Err(_) => {
                    attempts.push(terminal);
                }
            }
        }
        if succeeded {
            break;
        }
        // Exhausted this agent's retries: it is excluded and the next
        // candidate is tried (deterministic reassignment).
    }
    match last_result {
        Some(r) => Ok((attempts, Ok(r))),
        None => {
            // Every candidate failed: settle the task as failed durably.
            {
                let mut b = board.lock().unwrap();
                b.set_status(task.id, TaskStatus::Failed)?;
            }
            let last_err = attempts
                .last()
                .and_then(|a| a.error.clone())
                .unwrap_or_else(|| "no candidate agent".to_string());
            Ok((attempts, Err(last_err)))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;
    use tokio::sync::Notify;

    use super::*;
    use crate::registry::{AgentConfig, AgentTier};
    use crate::testutil::{err_driver, ok_driver, trio_registry, MemBoard};

    /// Records the peak number of in-flight tasks and only proceeds once two
    /// have been in-flight together, proving real overlap.
    struct OverlapDriver {
        in_flight: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
        summary: String,
    }

    #[async_trait]
    impl AgentDriver for OverlapDriver {
        async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
            let cur = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(cur, Ordering::SeqCst);
            while self.peak.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(AgentTaskResult {
                task_id: task.id,
                summary: self.summary.clone(),
                artifacts: Vec::new(),
                message: None,
            })
        }
    }

    /// Yields (without waiting for overlap) so a second task on the same agent
    /// gets scheduled and blocks on the shared semaphore.
    struct YieldDriver {
        in_flight: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
        summary: String,
    }

    struct BlockingDriver {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait]
    impl AgentDriver for BlockingDriver {
        async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(AgentTaskResult {
                task_id: task.id,
                summary: "released".into(),
                artifacts: Vec::new(),
                message: None,
            })
        }
    }

    #[async_trait]
    impl AgentDriver for YieldDriver {
        async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
            let cur = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(cur, Ordering::SeqCst);
            for _ in 0..8 {
                tokio::task::yield_now().await;
            }
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(AgentTaskResult {
                task_id: task.id,
                summary: self.summary.clone(),
                artifacts: Vec::new(),
                message: None,
            })
        }
    }

    // T06: two independent tasks on two agents truly overlap.
    #[tokio::test]
    async fn independent_tasks_run_concurrently() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let drivers = BTreeMap::from([
            (
                "worker-a".to_string(),
                Arc::new(OverlapDriver {
                    in_flight: in_flight.clone(),
                    peak: peak.clone(),
                    summary: "a".into(),
                }) as Arc<dyn AgentDriver>,
            ),
            (
                "worker-b".to_string(),
                Arc::new(OverlapDriver {
                    in_flight: in_flight.clone(),
                    peak: peak.clone(),
                    summary: "b".into(),
                }) as Arc<dyn AgentDriver>,
            ),
        ]);
        let mut sched = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1);
        let specs = vec![
            TaskSpec {
                objective: "t1".into(),
                kind: TaskKind::Bulk,
                target: Some("worker-a".into()),
                parent: None,
                context: Vec::new(),
            },
            TaskSpec {
                objective: "t2".into(),
                kind: TaskKind::Bulk,
                target: Some("worker-b".into()),
                parent: None,
                context: Vec::new(),
            },
        ];
        let results = sched.schedule(&specs).await.expect("schedule");
        assert!(results[0].result.is_ok());
        assert!(results[1].result.is_ok());
        // Both tasks were in-flight at the same time.
        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }

    // T06 (single-agent bound): with max_concurrency=1, two tasks on the same
    // agent never overlap. The shared semaphore enforces the quota.
    #[tokio::test]
    async fn same_agent_respects_max_concurrency() {
        let registry = AgentRegistry::new(vec![
            AgentConfig {
                id: "reasoner-a".into(),
                name: "reasoner-a".into(),
                tier: AgentTier::Reasoner,
                tags: Vec::new(),
                max_concurrency: 1,
                driver_kind: None,
                executable: None,
                driver_args: Vec::new(),
            },
            AgentConfig {
                id: "worker-a".into(),
                name: "worker-a".into(),
                tier: AgentTier::Worker,
                tags: Vec::new(),
                max_concurrency: 1,
                driver_kind: None,
                executable: None,
                driver_args: Vec::new(),
            },
            AgentConfig {
                id: "utility-a".into(),
                name: "utility-a".into(),
                tier: AgentTier::Utility,
                tags: Vec::new(),
                max_concurrency: 1,
                driver_kind: None,
                executable: None,
                driver_args: Vec::new(),
            },
        ])
        .expect("a full tier set is valid");
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let drivers = BTreeMap::from([(
            "worker-a".to_string(),
            Arc::new(YieldDriver {
                in_flight: in_flight.clone(),
                peak: peak.clone(),
                summary: "a".into(),
            }) as Arc<dyn AgentDriver>,
        )]);
        let mut sched = Scheduler::new(registry, drivers, MemBoard::default(), 1);
        let specs = vec![
            TaskSpec {
                objective: "t1".into(),
                kind: TaskKind::Bulk,
                target: Some("worker-a".into()),
                parent: None,
                context: Vec::new(),
            },
            TaskSpec {
                objective: "t2".into(),
                kind: TaskKind::Bulk,
                target: Some("worker-a".into()),
                parent: None,
                context: Vec::new(),
            },
        ];
        let results = sched.schedule(&specs).await.expect("schedule");
        assert!(results[0].result.is_ok());
        assert!(results[1].result.is_ok());
        // Both target worker-a (max_concurrency=1): they never overlap.
        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn capacity_queued_task_stays_assigned_until_driver_can_start() {
        let registry = AgentRegistry::new(vec![
            AgentConfig {
                id: "reasoner-a".into(),
                name: "reasoner-a".into(),
                tier: AgentTier::Reasoner,
                tags: Vec::new(),
                max_concurrency: 1,
                driver_kind: None,
                executable: None,
                driver_args: Vec::new(),
            },
            AgentConfig {
                id: "worker-a".into(),
                name: "worker-a".into(),
                tier: AgentTier::Worker,
                tags: Vec::new(),
                max_concurrency: 1,
                driver_kind: None,
                executable: None,
                driver_args: Vec::new(),
            },
            AgentConfig {
                id: "utility-a".into(),
                name: "utility-a".into(),
                tier: AgentTier::Utility,
                tags: Vec::new(),
                max_concurrency: 1,
                driver_kind: None,
                executable: None,
                driver_args: Vec::new(),
            },
        ])
        .expect("full registry");
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let drivers = BTreeMap::from([(
            "worker-a".to_string(),
            Arc::new(BlockingDriver {
                entered: entered.clone(),
                release: release.clone(),
            }) as Arc<dyn AgentDriver>,
        )]);
        let mut scheduler = Scheduler::new(registry, drivers, MemBoard::default(), 1);
        let board = scheduler.board.clone();
        let task = tokio::spawn(async move {
            scheduler
                .schedule(&[
                    TaskSpec {
                        objective: "first".into(),
                        kind: TaskKind::Bulk,
                        target: Some("worker-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                    TaskSpec {
                        objective: "second".into(),
                        kind: TaskKind::Bulk,
                        target: Some("worker-a".into()),
                        parent: None,
                        context: Vec::new(),
                    },
                ])
                .await
        });
        entered.notified().await;
        {
            let board = board.lock().unwrap();
            assert_eq!(board.task(1).unwrap().unwrap().status, TaskStatus::Running);
            assert_eq!(board.task(2).unwrap().unwrap().status, TaskStatus::Assigned);
        }
        release.notify_waiters();
        // The second driver reaches the same barrier only after the first is released.
        entered.notified().await;
        release.notify_waiters();
        let results = task.await.expect("join").expect("schedule");
        assert!(results.iter().all(|result| result.result.is_ok()));
    }

    // T11/T12: a task that fails on worker-a retries, then reassigns to
    // worker-b. The failure history is kept.
    #[tokio::test]
    async fn failed_task_retries_then_reassigns() {
        let drivers = BTreeMap::from([
            ("worker-a".to_string(), err_driver("worker-a is down")),
            ("worker-b".to_string(), ok_driver("worker-b")),
        ]);
        let mut sched = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 2);
        let specs = vec![TaskSpec {
            objective: "job".into(),
            kind: TaskKind::Bulk,
            target: None,
            parent: None,
            context: Vec::new(),
        }];
        let results = sched.schedule(&specs).await.expect("schedule");
        let r = &results[0];
        assert!(r.result.is_ok());
        assert_eq!(r.attempts.len(), 3);
        assert_eq!(r.attempts[0].agent_id, "worker-a");
        assert_eq!(r.attempts[0].status, TaskStatus::Failed);
        assert_eq!(r.attempts[1].agent_id, "worker-a");
        assert_eq!(r.attempts[1].status, TaskStatus::Failed);
        assert_eq!(r.attempts[2].agent_id, "worker-b");
        assert_eq!(r.attempts[2].status, TaskStatus::Succeeded);
    }

    #[tokio::test]
    async fn explicit_resume_reuses_task_and_appends_a_new_attempt() {
        let mut board = MemBoard::default();
        let task_id = board
            .create_task(
                "interrupted job",
                None,
                TaskKind::Bulk,
                Some("worker-b".into()),
            )
            .unwrap();
        board.assign(task_id, "worker-a").unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id,
                attempt: 1,
                agent_id: "worker-a".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.recover_interrupted_attempt(task_id).unwrap();
        board.set_status(task_id, TaskStatus::Pending).unwrap();
        let drivers = BTreeMap::from([("worker-b".to_string(), ok_driver("resumed"))]);
        let mut scheduler = Scheduler::new(trio_registry(), drivers, board, 1);
        let result = scheduler.resume_existing(task_id).await.unwrap();
        assert!(result.result.is_ok());
        assert_eq!(result.task_id, task_id);
        assert_eq!(result.attempts.len(), 1);
        assert_eq!(result.attempts[0].attempt, 2);
        let attempts = scheduler.board().lock().unwrap().attempts(task_id).unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].status, TaskStatus::Failed);
        assert_eq!(attempts[1].status, TaskStatus::Succeeded);
    }

    // T13: an explicit target overrides the tier routing.
    #[tokio::test]
    async fn user_override_targets_a_specific_agent() {
        let drivers = BTreeMap::from([
            ("worker-a".to_string(), ok_driver("worker-a")),
            ("worker-b".to_string(), ok_driver("worker-b")),
        ]);
        let mut sched = Scheduler::new(trio_registry(), drivers, MemBoard::default(), 1);
        let specs = vec![TaskSpec {
            objective: "job".into(),
            kind: TaskKind::Bulk,
            target: Some("worker-b".into()),
            parent: None,
            context: Vec::new(),
        }];
        let results = sched.schedule(&specs).await.expect("schedule");
        // The override wins: the first (and only) attempt is on worker-b.
        assert_eq!(results[0].attempts[0].agent_id, "worker-b");
        assert_eq!(results[0].attempts[0].status, TaskStatus::Succeeded);
    }

    #[test]
    fn utility_prefers_utility_when_present() {
        let scheduler = Scheduler::new(trio_registry(), BTreeMap::new(), MemBoard::default(), 1);
        assert_eq!(
            scheduler.candidate_order(TaskKind::Utility, None),
            vec!["utility-a"]
        );
    }

    #[test]
    fn utility_falls_back_to_worker_when_no_utility() {
        let registry = AgentRegistry::new(vec![
            AgentConfig {
                id: "reasoner-a".into(),
                name: "reasoner-a".into(),
                tier: AgentTier::Reasoner,
                tags: Vec::new(),
                max_concurrency: 1,
                driver_kind: None,
                executable: None,
                driver_args: Vec::new(),
            },
            AgentConfig {
                id: "worker-a".into(),
                name: "worker-a".into(),
                tier: AgentTier::Worker,
                tags: Vec::new(),
                max_concurrency: 1,
                driver_kind: None,
                executable: None,
                driver_args: Vec::new(),
            },
        ])
        .unwrap();
        let scheduler = Scheduler::new(registry, BTreeMap::new(), MemBoard::default(), 1);
        assert_eq!(
            scheduler.candidate_order(TaskKind::Utility, None),
            vec!["worker-a"]
        );
    }

    #[test]
    fn utility_never_implicitly_falls_back_to_reasoner() {
        let registry = AgentRegistry::new(vec![
            AgentConfig {
                id: "reasoner-a".into(),
                name: "reasoner-a".into(),
                tier: AgentTier::Reasoner,
                tags: Vec::new(),
                max_concurrency: 1,
                driver_kind: None,
                executable: None,
                driver_args: Vec::new(),
            },
            AgentConfig {
                id: "worker-a".into(),
                name: "worker-a".into(),
                tier: AgentTier::Worker,
                tags: Vec::new(),
                max_concurrency: 1,
                driver_kind: None,
                executable: None,
                driver_args: Vec::new(),
            },
        ])
        .unwrap();
        let scheduler = Scheduler::new(registry, BTreeMap::new(), MemBoard::default(), 1);
        assert_eq!(
            scheduler.candidate_order(TaskKind::Utility, None),
            vec!["worker-a"]
        );
    }
}
