//! The durable task board: tasks, attempts, messages, and artifacts.
//!
//! The board is the team's memory. Results, artifacts, and directed messages
//! are persisted here so they flow between Agents without a human copying
//! anything (T16). The trait is pure (no storage dependency); the SQLite
//! implementation lives in the storage crate, which avoids a dependency cycle.

use crate::registry::{AgentTaskResult, TaskKind};

/// The lifecycle state of a team task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Assigned,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskStatus {
    /// The durable string form.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Assigned => "assigned",
            TaskStatus::Running => "running",
            TaskStatus::Succeeded => "succeeded",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    /// Restore a status from its string form.
    pub fn restore(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => TaskStatus::Pending,
            "assigned" => TaskStatus::Assigned,
            "running" => TaskStatus::Running,
            "succeeded" => TaskStatus::Succeeded,
            "failed" => TaskStatus::Failed,
            "cancelled" => TaskStatus::Cancelled,
            _ => return None,
        })
    }
}

/// A durable team task.
#[derive(Debug, Clone)]
pub struct TaskRecord {
    pub id: u64,
    pub objective: String,
    pub parent_task: Option<u64>,
    pub kind: TaskKind,
    /// The explicit user target, if any (T13).
    pub target: Option<String>,
    /// The agent that was actually assigned.
    pub assignee: Option<String>,
    pub status: TaskStatus,
}

/// A single attempt/run of a task. Each attempt is persisted independently, so
/// a failure is never overwritten by a later retry (T11).
#[derive(Debug, Clone)]
pub struct TaskAttempt {
    pub task_id: u64,
    pub attempt: u32,
    pub agent_id: String,
    pub status: TaskStatus,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// A directed message between two Agents. It reaches only the target's
/// context (T09).
#[derive(Debug, Clone)]
pub struct AgentMessage {
    pub from_agent: String,
    pub to_agent: String,
    pub body: String,
}

/// Artifact metadata: a path and its content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactMeta {
    pub path: String,
    pub sha256: String,
}

/// One exact artifact selected by the Lead for a final team result. The task
/// remains canonical; path and digest prevent reconstruction from silently
/// substituting a newer or unrelated artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedArtifactRef {
    pub task_id: u64,
    pub artifact: ArtifactMeta,
}

/// An error from the task board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardError {
    /// An operation named a task that does not exist.
    UnknownTask(u64),
    /// A storage failure.
    Storage(String),
}

/// The board's failure as a sentence rather than as the enum's structure.
impl std::fmt::Display for BoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTask(task) => write!(f, "no task {task} exists on the board"),
            Self::Storage(detail) => write!(f, "the task board storage failed: {detail}"),
        }
    }
}

/// The durable task board.
///
/// Implementations persist tasks, attempts, messages, and artifacts so that a
/// worker's result, artifact, and directed message reach the right Agent's
/// next context (T07/T08/T09) and survive a restart.
pub trait TaskBoard {
    /// Create a task and return its id.
    fn create_task(
        &mut self,
        objective: &str,
        parent: Option<u64>,
        kind: TaskKind,
        target: Option<String>,
    ) -> Result<u64, BoardError>;
    /// Assign a task to `agent` (Pending -> Assigned).
    fn assign(&mut self, task: u64, agent: &str) -> Result<(), BoardError>;
    /// Move a task to a new lifecycle status.
    fn set_status(&mut self, task: u64, status: TaskStatus) -> Result<(), BoardError>;
    /// Persist one attempt (independently; failures are kept).
    fn record_attempt(&mut self, attempt: &TaskAttempt) -> Result<(), BoardError>;
    /// Move a running attempt to its terminal status, updating the row that
    /// was persisted as Running. This is what makes the lifecycle durable: the
    /// attempt is observable as Running before the driver runs, then settles to
    /// Succeeded/Failed without being overwritten by a later retry.
    fn complete_attempt(&mut self, attempt: &TaskAttempt) -> Result<(), BoardError>;
    /// Close an attempt left Running by a process interruption without
    /// replaying its driver. A caller may explicitly resume the task later.
    fn recover_interrupted_attempt(
        &mut self,
        task: u64,
    ) -> Result<Option<TaskAttempt>, BoardError> {
        let interrupted = self
            .attempts(task)?
            .into_iter()
            .rev()
            .find(|attempt| attempt.status == TaskStatus::Running);
        let Some(mut interrupted) = interrupted else {
            return Ok(None);
        };
        interrupted.status = TaskStatus::Failed;
        interrupted.error =
            Some("interrupted before terminal driver result; explicit resume required".into());
        self.complete_attempt(&interrupted)?;
        self.set_status(task, TaskStatus::Failed)?;
        Ok(Some(interrupted))
    }
    /// Atomically (where the backing store supports it) commit a successful
    /// worker result flow.  Messages and artifact references become durable
    /// before the task is observable as succeeded, so a restart cannot expose
    /// a terminal result without the data that grounds it.
    fn commit_successful_result(
        &mut self,
        attempt: &TaskAttempt,
        result: &AgentTaskResult,
    ) -> Result<(), BoardError> {
        if attempt.task_id != result.task_id || attempt.status != TaskStatus::Succeeded {
            return Err(BoardError::Storage(
                "successful result does not match succeeded attempt".into(),
            ));
        }
        if let Some(message) = &result.message {
            self.record_message(message)?;
        }
        for artifact in &result.artifacts {
            self.record_artifact(attempt.task_id, artifact)?;
        }
        self.complete_attempt(attempt)?;
        self.set_status(attempt.task_id, TaskStatus::Succeeded)
    }
    /// Persist a directed message.
    fn record_message(&mut self, message: &AgentMessage) -> Result<(), BoardError>;
    /// Persist artifact metadata for a task.
    fn record_artifact(&mut self, task: u64, artifact: &ArtifactMeta) -> Result<(), BoardError>;
    /// Persist the Lead's explicitly selected completed task and artifact
    /// references before the root task becomes observable as succeeded.
    fn record_final_refs(
        &mut self,
        root_task: u64,
        task_refs: &[u64],
        artifact_refs: &[SelectedArtifactRef],
    ) -> Result<(), BoardError>;
    /// Read exactly the final references selected for `root_task`, rather
    /// than inferring them from every successful descendant.
    fn final_refs(
        &self,
        root_task: u64,
    ) -> Result<(Vec<u64>, Vec<SelectedArtifactRef>), BoardError>;
    /// Read a task.
    fn task(&self, id: u64) -> Result<Option<TaskRecord>, BoardError>;
    /// Read all attempts for a task, in attempt order.
    fn attempts(&self, task: u64) -> Result<Vec<TaskAttempt>, BoardError>;
    /// Read the messages addressed to `agent` (T09).
    fn messages_to(&self, agent: &str) -> Result<Vec<AgentMessage>, BoardError>;
    /// Read normalized directed messages in durable insertion order.  This is
    /// intentionally a summary surface for the product UI, not a runtime
    /// transcript or hidden-reasoning channel.
    fn messages(&self) -> Result<Vec<AgentMessage>, BoardError>;
    /// Read the artifact metadata for a task (T08).
    fn artifacts(&self, task: u64) -> Result<Vec<ArtifactMeta>, BoardError>;
    /// List every task id, in creation order. Used to reconstruct the task
    /// tree (and thus the final result) from the durable board.
    fn task_ids(&self) -> Result<Vec<u64>, BoardError>;
}
