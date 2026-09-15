//! External Agent runtime adapters and the durable team run path.
//!
//! This crate is the composition root for the supported product: it turns
//! durable registry rows into live external runtime drivers (ACP, Codex exec,
//! Codex app-server, Claude CLI), runs the Lead loop through the scheduler and
//! task board, and persists normalized runtime observations. It adds no second
//! tool loop and no control plane.

mod acp_worker;
mod claude_cli;
mod claude_cli_driver;
mod codex_app_server;
#[path = "bin/am-codex-mcp.rs"]
mod codex_bridge;
mod codex_exec;
mod codex_exec_driver;
mod codex_exec_lead;
mod codex_lead;
mod codex_team_driver;
mod driver_factory;
mod launch;
mod runtime_adapter;
mod runtime_event;
mod team_runner;

pub use acp_worker::{
    AcpCancellation, AcpCancellationListener, AcpPermissionPolicy, AcpRuntimeAdapter,
    AcpSessionStartedObserver, AcpWorkerConfig, AcpWorkerDriver, AcpWorkerError,
    PersistedAcpWorkerDriver,
};
pub use claude_cli::{
    normalize_stream_event as normalize_claude_stream_event,
    run_invocation as run_claude_cli_invocation, ClaudeCliInvocation, ClaudeCliResult,
};
pub use claude_cli_driver::{ClaudeCliDriverConfig, PersistedClaudeCliDriver};
pub use codex_app_server::{
    select_final_agent_message, CodexAppServer, CodexBridgeError, CodexBridgeEvent,
    DEFAULT_FINAL_MESSAGE_MAX_BYTES,
};
pub use codex_bridge::run_codex_mcp_bridge;
pub use codex_exec::{
    normalize_event as normalize_codex_exec_event, run_invocation as run_codex_exec_invocation,
    CodexExecInvocation, CodexExecResult,
};
pub use codex_exec_driver::{CodexExecDriverConfig, PersistedCodexExecDriver};
pub use codex_exec_lead::{CodexExecLeadBrain, CodexExecLeadConfig};
pub use codex_lead::{CodexLeadBrain, CodexLeadConfig};
pub use codex_team_driver::{CodexTeamDriverConfig, PersistedCodexTeamDriver};
pub use driver_factory::{
    validate_driver_config, DriverFactory, DriverFactoryError, DEFAULT_ACP_MAX_PROMPT_BYTES,
    DEFAULT_ACP_MAX_RESULT_BYTES, DEFAULT_ACP_TIMEOUT_SECONDS, DEFAULT_CODEX_MAX_EVENTS,
};
pub use launch::LaunchSpec;
pub use runtime_adapter::{
    NoopRuntimeEventSink, RuntimeAdapter, RuntimeAgentDriver, RuntimeBinding, RuntimeCapabilities,
    RuntimeCheckpoint, RuntimeDescriptor, RuntimeDriverRun, RuntimeError, RuntimeEventSink,
    RuntimeExecution, RuntimeExecutionRequest, RuntimeKind, RuntimeOutcome,
};
pub use runtime_event::{
    DurableRuntimeEventWriter, LiveRuntimeEventSink, NoopLiveRuntimeEventSink,
    RuntimeEventDispatcher, SqliteRuntimeEventWriter,
};
pub use team_runner::{
    validate_lead_config, validate_registry_row, DefaultLeadBrainFactory, LeadBrainFactory,
    TeamRunOptions, TeamRunOutcome, TeamRunner, TeamRunnerError, DEFAULT_LEAD_MAX_ANSWER_BYTES,
    DEFAULT_LEAD_MAX_EVENTS, DEFAULT_LEAD_MAX_PROMPT_BYTES, DEFAULT_MAX_RETRIES,
    DEFAULT_MAX_ROUNDS, DEFAULT_MAX_TASKS,
};
