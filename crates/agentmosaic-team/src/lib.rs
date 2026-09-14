//! Heterogeneous Agent team layer: registry, task board, scheduling, and the
//! Lead loop.
//!
//! R1 defined the driver contract and Agent shapes. R5 adds the registry and
//! deterministic routing (commit 1), the durable task board and result flow
//! (commit 2), concurrent scheduling with retry/reassignment (commit 3), and
//! the Lead plan-follow-up-synthesis loop (commit 4).

mod acc;
mod board;
mod lead;
mod registry;
mod run_event;
mod runtime_event;
mod scheduler;

#[cfg(test)]
mod testutil;

pub use acc::*;
pub use board::{
    AgentMessage, ArtifactMeta, BoardError, SelectedArtifactRef, TaskAttempt, TaskBoard,
    TaskRecord, TaskStatus,
};
pub use lead::{
    reconstruct_team_result, Lead, LeadBrain, LeadBrainError, LeadContext, LeadDecision, LeadError,
    TeamResult,
};
pub use registry::{
    AgentConfig, AgentDriver, AgentRegistry, AgentTask, AgentTaskResult, AgentTier, DriverKind,
    RegistryError, TaskKind,
};
pub use run_event::{
    bounded_event_text, LeadPhase, NoopRunEventSink, RunEvent, RunEventSink, MAX_EVENT_TEXT_BYTES,
};
pub use runtime_event::{
    PrivateRuntimeInput, RuntimeEvent, RuntimeEventPolicy, RuntimeEventRecord,
    RuntimeFileChangeKind, RuntimePermissionDecision, RuntimePermissionOption, RuntimePlanItem,
    MAX_ASSISTANT_MESSAGE_BYTES, MAX_COMMAND_BYTES, MAX_DURABLE_RUNTIME_PAYLOAD_BYTES,
    MAX_PERMISSION_OPTIONS, MAX_PLAN_ITEMS, MAX_PLAN_ITEM_BYTES, MAX_RUNTIME_ID_BYTES,
    MAX_RUNTIME_SUMMARY_BYTES,
};
pub use scheduler::{ScheduleError, ScheduledResult, Scheduler, TaskSpec};
