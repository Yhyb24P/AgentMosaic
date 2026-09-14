//! Vendor-neutral runtime execution contracts.
//!
//! Runtime adapters observe foreign processes. They do not own task state:
//! [`agentmosaic_team::Scheduler`] still creates and settles attempts, while a
//! [`RuntimeAgentDriver`] resolves the one running attempt and bridges the
//! adapter outcome back into the existing [`agentmosaic_team::AgentDriver`]
//! result contract.

use std::sync::Arc;

use agentmosaic_team::{AgentTask, AgentTaskResult, RuntimeEventRecord};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A machine protocol implemented by an external Agent runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    Acp,
    CodexExec,
    ClaudeCli,
}

/// Capability facts captured from the runtime handshake or probe.
///
/// Filesystem and terminal reverse RPC stay false until AM supplies bounded
/// handlers; an adapter must never advertise host access it cannot enforce.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub supports_load: bool,
    pub supports_resume: bool,
    pub supports_fork: bool,
    pub supports_cancel: bool,
    pub assistant_stream: bool,
    pub plan_updates: bool,
    pub tool_events: bool,
    pub permission_requests: bool,
    pub reverse_filesystem: bool,
    pub reverse_terminal: bool,
    pub subagent_events: bool,
    pub mcp: bool,
}

/// A probed runtime and the protocol contract it negotiated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDescriptor {
    pub runtime_name: Option<String>,
    pub runtime_version: Option<String>,
    pub protocol_kind: String,
    pub protocol_version: String,
    pub capabilities: RuntimeCapabilities,
}

/// The canonical attempt context passed into an adapter.
#[derive(Debug, Clone)]
pub struct RuntimeExecutionRequest {
    pub task: AgentTask,
    pub attempt: u32,
    pub agent_id: String,
}

/// The only durable foreign reference from which an adapter may resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCheckpoint {
    pub native_session_id: String,
}

/// Foreign runtime identity attached to one canonical task attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBinding {
    pub task_id: u64,
    pub attempt: u32,
    pub agent_id: String,
    pub runtime_kind: RuntimeKind,
    pub native_session_id: String,
    pub descriptor: RuntimeDescriptor,
}

#[derive(Debug, Clone)]
pub struct RuntimeOutcome {
    pub binding: RuntimeBinding,
    pub result: AgentTaskResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    InvalidConfiguration(String),
    UnsupportedCapability(String),
    Protocol(String),
    TimedOut,
    Cancelled,
    InvalidResult(String),
    Persistence(String),
    AlreadyFinished,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration(detail) => {
                write!(formatter, "invalid runtime configuration: {detail}")
            }
            Self::UnsupportedCapability(detail) => {
                write!(formatter, "runtime capability is unavailable: {detail}")
            }
            Self::Protocol(detail) => write!(formatter, "runtime protocol error: {detail}"),
            Self::TimedOut => write!(formatter, "runtime execution timed out"),
            Self::Cancelled => write!(formatter, "runtime execution cancelled"),
            Self::InvalidResult(detail) => write!(formatter, "invalid runtime result: {detail}"),
            Self::Persistence(detail) => write!(formatter, "runtime persistence failed: {detail}"),
            Self::AlreadyFinished => write!(formatter, "runtime execution is already finished"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Event boundary supplied to every runtime adapter. Presentation-only sinks
/// may hide errors internally; a required durable boundary returns an error.
pub trait RuntimeEventSink: Send + Sync {
    fn emit(&self, record: RuntimeEventRecord) -> Result<(), RuntimeError>;
}

#[derive(Debug, Default)]
pub struct NoopRuntimeEventSink;

impl RuntimeEventSink for NoopRuntimeEventSink {
    fn emit(&self, _record: RuntimeEventRecord) -> Result<(), RuntimeError> {
        Ok(())
    }
}

#[async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn adapter_kind(&self) -> RuntimeKind;

    async fn probe(&self) -> Result<RuntimeDescriptor, RuntimeError>;

    async fn start(
        &self,
        request: RuntimeExecutionRequest,
        events: Arc<dyn RuntimeEventSink>,
    ) -> Result<Box<dyn RuntimeExecution>, RuntimeError>;

    async fn resume(
        &self,
        checkpoint: RuntimeCheckpoint,
        request: RuntimeExecutionRequest,
        events: Arc<dyn RuntimeEventSink>,
    ) -> Result<Box<dyn RuntimeExecution>, RuntimeError>;
}

#[async_trait]
pub trait RuntimeExecution: Send {
    fn binding(&self) -> &RuntimeBinding;

    async fn wait(&mut self) -> Result<RuntimeOutcome, RuntimeError>;

    async fn cancel(&mut self) -> Result<(), RuntimeError>;
}

/// Scheduler bridge for adapters that implement the runtime contracts above.
/// The concrete persistence and attempt-resolution policy is supplied by the
/// adapter-specific constructor so the public team interface remains stable.
#[derive(Clone)]
pub struct RuntimeAgentDriver {
    run: Arc<dyn RuntimeDriverRun>,
}

impl RuntimeAgentDriver {
    pub fn new(run: Arc<dyn RuntimeDriverRun>) -> Self {
        Self { run }
    }
}

#[async_trait]
pub trait RuntimeDriverRun: Send + Sync {
    async fn run(&self, task: AgentTask) -> Result<AgentTaskResult, RuntimeError>;
}

#[async_trait]
impl agentmosaic_team::AgentDriver for RuntimeAgentDriver {
    async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
        self.run.run(task).await.map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_host_capabilities_default_to_false() {
        let capabilities = RuntimeCapabilities::default();
        assert!(!capabilities.reverse_filesystem);
        assert!(!capabilities.reverse_terminal);
        assert!(!capabilities.permission_requests);
    }

    #[test]
    fn capability_snapshots_have_a_stable_json_shape() {
        let value = serde_json::to_value(RuntimeCapabilities {
            supports_load: true,
            supports_resume: true,
            supports_cancel: true,
            assistant_stream: true,
            plan_updates: true,
            tool_events: true,
            permission_requests: true,
            mcp: true,
            ..RuntimeCapabilities::default()
        })
        .unwrap();
        assert_eq!(value["supports_load"], true);
        assert_eq!(value["reverse_filesystem"], false);
        assert_eq!(value["reverse_terminal"], false);
    }
}
