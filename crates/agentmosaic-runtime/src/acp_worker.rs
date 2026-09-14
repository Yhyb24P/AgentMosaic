//! Bounded shared ACP worker driver for local coding-agent CLIs.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agent_client_protocol::schema::v1::{
    AuthMethodId, AuthenticateRequest, CancelNotification, ContentBlock, ContentChunk,
    InitializeRequest, InitializeResponse, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionNotification, SessionUpdate, StopReason, ToolCallStatus,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::util::MatchDispatch;
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Client, SessionMessage};
use agentmosaic_storage::{
    ExtendedExternalRuntimeBinding, ExternalRuntimeBinding, SqliteTaskBoard,
};
use agentmosaic_team::{
    AgentDriver, AgentTask, AgentTaskResult, ArtifactMeta, RuntimeEvent, RuntimeFileChangeKind,
    RuntimePermissionDecision, RuntimePermissionOption, RuntimePlanItem, TaskBoard, TaskStatus,
};
use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use tokio::sync::watch;

use crate::{
    NoopLiveRuntimeEventSink, NoopRuntimeEventSink, RuntimeAdapter, RuntimeBinding,
    RuntimeCapabilities, RuntimeCheckpoint, RuntimeDescriptor, RuntimeError,
    RuntimeEventDispatcher, RuntimeEventSink, RuntimeExecution, RuntimeExecutionRequest,
    RuntimeKind, RuntimeOutcome, SqliteRuntimeEventWriter,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpWorkerConfig {
    pub runtime_kind: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// Auth method id advertised by the agent's `initialize`; sent via
    /// `authenticate` before `session/new`. `None` skips authentication.
    pub auth_method: Option<String>,
    pub working_directory: PathBuf,
    pub timeout: Duration,
    pub max_prompt_bytes: usize,
    pub max_result_bytes: usize,
    /// Relative output paths returned with the worker result before the board
    /// may expose a successful task.
    pub artifact_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpWorkerError {
    InvalidConfig(String),
    Protocol(String),
    TimedOut,
    Cancelled,
    InvalidPeerResult(String),
}

impl std::fmt::Display for AcpWorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(s) => write!(f, "invalid ACP worker configuration: {s}"),
            Self::Protocol(s) => write!(f, "ACP protocol error: {s}"),
            Self::TimedOut => write!(f, "ACP worker timed out"),
            Self::Cancelled => write!(f, "ACP worker cancelled"),
            Self::InvalidPeerResult(s) => write!(f, "invalid ACP peer result: {s}"),
        }
    }
}
impl std::error::Error for AcpWorkerError {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AcpPermissionPolicy {
    /// Select an advertised rejection option, or cancel when none exists.
    #[default]
    Deny,
    /// Explicit opt-in for isolated, controlled local execution only.
    Allow,
}

#[derive(Clone)]
pub struct AcpWorkerDriver {
    config: AcpWorkerConfig,
    permission_policy: AcpPermissionPolicy,
    events: Arc<dyn RuntimeEventSink>,
}

/// Scheduler-facing ACP driver that persists only the foreign session binding
/// into the existing SQLite team board. The scheduler remains the sole owner
/// of task/attempt lifecycle and result commits.
#[derive(Clone)]
pub struct PersistedAcpWorkerDriver {
    driver: crate::RuntimeAgentDriver,
}

struct PersistedAcpRuntimeRun {
    adapter: AcpRuntimeAdapter,
    database: PathBuf,
    agent_id: String,
    events: Arc<dyn RuntimeEventSink>,
}

/// Stable-v1 ACP implementation of the vendor-neutral runtime adapter.
///
/// `start` waits until ACP has returned the *actual* session id, but holds the
/// first prompt behind a one-shot gate.  This lets the scheduler-facing bridge
/// persist that binding before any foreign side effect is released.
#[derive(Clone)]
pub struct AcpRuntimeAdapter {
    worker: AcpWorkerDriver,
}

struct AcpRuntimeExecution {
    binding: RuntimeBinding,
    release: Option<oneshot::Sender<()>>,
    cancellation: AcpCancellation,
    task: Option<tokio::task::JoinHandle<Result<AcpTaskExecution, AcpWorkerError>>>,
}

struct AcpResumedExecution {
    binding: RuntimeBinding,
    worker: AcpWorkerDriver,
    task: AgentTask,
    cancelled: bool,
}

impl std::fmt::Debug for AcpWorkerDriver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcpWorkerDriver")
            .field("config", &self.config)
            .field("permission_policy", &self.permission_policy)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for PersistedAcpWorkerDriver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PersistedAcpWorkerDriver").finish()
    }
}

impl std::fmt::Debug for AcpRuntimeAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcpRuntimeAdapter")
            .field("worker", &self.worker)
            .finish()
    }
}

/// A bounded, same-session ACP exchange. The native session id is an external
/// recovery reference; callers must persist it only through the team board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpConversationResult {
    pub external_session_id: String,
    pub first_summary: String,
    pub follow_up_summary: String,
}

/// A completed bounded task plus its foreign session reference. The reference
/// is deliberately not a task id and must be persisted only by the caller's
/// existing external-runtime binding path.
#[derive(Debug, Clone)]
pub struct AcpTaskExecution {
    pub external_session_id: String,
    pub result: AgentTaskResult,
}

/// A narrowly scoped lifecycle callback invoked after ACP `session/new` has
/// returned an external reference and before the first prompt is sent. The
/// reference remains foreign runtime metadata; callers keep canonical state in
/// their existing task board.
pub type AcpSessionStartedObserver = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// Internal extension of the legacy session callback. It carries only the
/// typed initialization snapshot, never raw wire frames or credentials.
type AcpSessionDescriptorObserver =
    Arc<dyn Fn(&str, &RuntimeDescriptor) -> Result<(), String> + Send + Sync>;

/// A caller-owned cancellation trigger for one ACP task execution.  It does
/// not carry a session id: the live driver derives the foreign reference from
/// the session it actually opened, so callers cannot redirect cancellation to
/// an arbitrary runtime session.
#[derive(Clone, Debug)]
pub struct AcpCancellation {
    sender: watch::Sender<bool>,
}

/// The driver-side half of an [`AcpCancellation`] pair.  It is intentionally
/// not serializable or durable; durable task state remains on the team board.
#[derive(Debug)]
pub struct AcpCancellationListener {
    receiver: watch::Receiver<bool>,
}

impl AcpCancellation {
    pub fn new() -> (Self, AcpCancellationListener) {
        let (sender, receiver) = watch::channel(false);
        (Self { sender }, AcpCancellationListener { receiver })
    }

    /// Request cancellation. Repeated requests are idempotent.
    pub fn cancel(&self) {
        self.sender.send_replace(true);
    }
}

impl AcpCancellationListener {
    async fn cancelled(&mut self) {
        if *self.receiver.borrow() {
            return;
        }
        let _ = self.receiver.changed().await;
    }
}

enum AcpRunOutcome {
    Completed(String, String),
    Cancelled,
}

struct RuntimeStartGate {
    binding: oneshot::Sender<RuntimeBinding>,
    release: oneshot::Receiver<()>,
}

#[derive(Clone)]
struct AcpEventIdentity {
    task_id: u64,
    attempt: u32,
    agent_id: String,
    runtime_name: Option<String>,
    events: Arc<dyn RuntimeEventSink>,
}

impl AcpEventIdentity {
    fn emit(
        &self,
        native_session_id: &str,
        event: RuntimeEvent,
    ) -> Result<(), agent_client_protocol::Error> {
        self.events
            .emit(agentmosaic_team::RuntimeEventRecord {
                task_id: self.task_id,
                attempt: self.attempt,
                agent_id: self.agent_id.clone(),
                runtime_name: self.runtime_name.clone(),
                native_session_id: Some(native_session_id.to_string()),
                event,
            })
            .map_err(|error| {
                agent_client_protocol::Error::internal_error()
                    .data(format!("emit normalized runtime event: {error}"))
            })
    }
}

impl AcpWorkerDriver {
    pub fn new(config: AcpWorkerConfig) -> Result<Self, AcpWorkerError> {
        if config.runtime_kind.trim().is_empty()
            || config.command.as_os_str().is_empty()
            || !config.working_directory.is_dir()
            || config.timeout.is_zero()
            || config.max_prompt_bytes == 0
            || config.max_result_bytes == 0
            || config.artifact_paths.iter().any(|path| {
                path.as_os_str().is_empty()
                    || path.is_absolute()
                    || path
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
            })
        {
            return Err(AcpWorkerError::InvalidConfig("runtime kind, command, existing working directory, bounded relative artifact paths, timeout, and limits are required".into()));
        }
        Ok(Self {
            config,
            permission_policy: AcpPermissionPolicy::Deny,
            events: Arc::new(NoopRuntimeEventSink),
        })
    }

    pub fn with_permission_policy(mut self, policy: AcpPermissionPolicy) -> Self {
        self.permission_policy = policy;
        self
    }

    pub fn with_event_sink(mut self, events: Arc<dyn RuntimeEventSink>) -> Self {
        self.events = events;
        self
    }

    /// Verify that an ACP runtime can initialize and open the smallest safe
    /// session.  This deliberately does not authenticate, send a prompt, or
    /// execute a user task: it is the bounded protocol check used by `am
    /// doctor`.
    pub async fn probe_readiness(&self) -> Result<(), AcpWorkerError> {
        self.probe_descriptor().await.map(|_| ())
    }

    /// Perform the stable-v1 handshake and return only typed, negotiated
    /// runtime facts. The client advertises no reverse filesystem or terminal
    /// capabilities, so a peer cannot route unrestricted host I/O through AM.
    pub async fn probe_descriptor(&self) -> Result<RuntimeDescriptor, AcpWorkerError> {
        let agent =
            AcpAgent::new(AcpAgentConfig::new(&self.config.command).args(self.config.args.clone()));
        let cwd = self.config.working_directory.clone();
        let run =
            Client
                .builder()
                .name("agentmosaic-doctor")
                .connect_with(agent, async move |cx| {
                    let initialized = initialize_v1(&cx).await?;
                    cx.build_session(&cwd)
                        .block_task()
                        .start_session()
                        .await
                        .map(|_session| descriptor_from_initialize(&initialized))
                });
        tokio::time::timeout(self.config.timeout, run)
            .await
            .map_err(|_| AcpWorkerError::TimedOut)?
            .map_err(|error| AcpWorkerError::Protocol(error.to_string()))
    }

    pub async fn run(&self, task: &AgentTask) -> Result<(String, String), AcpWorkerError> {
        let (_cancellation, mut listener) = AcpCancellation::new();
        self.run_with_cancellation(task, &mut listener).await
    }

    /// Run one bounded task and honor a caller-owned cancellation request on
    /// the exact live ACP session. A cancellation is confirmed only after the
    /// peer returns the stable-v1 `cancelled` stop reason.
    pub async fn run_with_cancellation(
        &self,
        task: &AgentTask,
        cancellation: &mut AcpCancellationListener,
    ) -> Result<(String, String), AcpWorkerError> {
        self.run_with_cancellation_observed(
            task,
            cancellation,
            None,
            None,
            None,
            1,
            self.config.runtime_kind.clone(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)] // lifecycle callbacks and the start gate are independently optional.
    async fn run_with_cancellation_observed(
        &self,
        task: &AgentTask,
        cancellation: &mut AcpCancellationListener,
        session_started: Option<AcpSessionStartedObserver>,
        descriptor_started: Option<AcpSessionDescriptorObserver>,
        start_gate: Option<RuntimeStartGate>,
        attempt: u32,
        agent_id: String,
    ) -> Result<(String, String), AcpWorkerError> {
        let prompt = bounded_prompt(task, self.config.max_prompt_bytes);
        let agent =
            AcpAgent::new(AcpAgentConfig::new(&self.config.command).args(self.config.args.clone()));
        let cwd = self.config.working_directory.clone();
        let max_result_bytes = self.config.max_result_bytes;
        let events = Arc::clone(&self.events);
        let permission_policy = self.permission_policy;
        let task_id = task.id;
        let run = Client
            .builder()
            .name("agentmosaic-r6")
            .connect_with(agent, async move |cx| {
                let initialized = initialize_v1(&cx).await?;
                if let Some(method) = &self.config.auth_method {
                    cx.send_request(AuthenticateRequest::new(AuthMethodId::new(method.as_str())))
                        .block_task()
                        .await?;
                }
                cx.build_session(&cwd)
                    .block_task()
                    .run_until(async |mut session| {
                        let session_id = session.session_id().to_string();
                        if let Some(observer) = session_started {
                            observer(&session_id).map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(format!("persist ACP session binding: {error}"))
                            })?;
                        }
                        let descriptor = descriptor_from_initialize(&initialized);
                        if let Some(observer) = descriptor_started {
                            observer(&session_id, &descriptor).map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(format!("persist ACP runtime descriptor: {error}"))
                            })?;
                        }
                        if let Some(start_gate) = start_gate {
                            let binding = RuntimeBinding {
                                task_id,
                                attempt,
                                agent_id: agent_id.clone(),
                                runtime_kind: RuntimeKind::Acp,
                                native_session_id: session_id.clone(),
                                descriptor: descriptor.clone(),
                            };
                            start_gate.binding.send(binding).map_err(|_| {
                                agent_client_protocol::Error::internal_error()
                                    .data("ACP runtime starter dropped before binding")
                            })?;
                            start_gate.release.await.map_err(|_| {
                                agent_client_protocol::Error::internal_error()
                                    .data("ACP runtime execution was dropped before release")
                            })?;
                        }
                        let identity = AcpEventIdentity {
                            task_id,
                            attempt,
                            agent_id,
                            runtime_name: descriptor.runtime_name,
                            events,
                        };
                        identity.emit(
                            &session_id,
                            RuntimeEvent::SessionStarted {
                                native_session_id: session_id.clone(),
                            },
                        )?;
                        session.send_prompt(&prompt)?;
                        let connection = session.connection().clone();
                        let native_session_id = session.session_id().clone();
                        tokio::select! {
                            response = read_acp_turn(&mut session, &identity, permission_policy) => {
                                let response = response?;
                                if parse_peer_result(&response, max_result_bytes).is_ok() {
                                    Ok(AcpRunOutcome::Completed(session_id, response))
                                } else {
                                    // A worker may finish its bounded filesystem work but add
                                    // prose around the required structured peer result. Ask once
                                    // on the *same* external session; the final response still
                                    // undergoes strict parsing below and is never accepted by
                                    // heuristic extraction.
                                    session.send_prompt("Your prior result did not satisfy the required peer-result contract. Return exactly one JSON object with only a non-empty string field named summary. Do not include prose, markdown, credentials, hidden reasoning, or other fields.")?;
                                    let repaired = tokio::select! {
                                        repaired = read_acp_turn(&mut session, &identity, permission_policy) => repaired?,
                                        _ = cancellation.cancelled() => {
                                            connection.send_notification(CancelNotification::new(native_session_id))?;
                                            loop {
                                                match session.read_update().await? {
                                                    SessionMessage::StopReason(StopReason::Cancelled) => break,
                                                    SessionMessage::StopReason(reason) => {
                                                        return Err(agent_client_protocol::Error::internal_error()
                                                            .data(format!("ACP cancel returned unexpected stop reason: {reason:?}")));
                                                    }
                                                    SessionMessage::SessionMessage(_) => {}
                                                    _ => {}
                                                }
                                            }
                                            return Ok(AcpRunOutcome::Cancelled);
                                        }
                                    };
                                    Ok(AcpRunOutcome::Completed(session_id, repaired))
                                }
                            }
                            _ = cancellation.cancelled() => {
                                connection.send_notification(CancelNotification::new(native_session_id))?;
                                loop {
                                    match session.read_update().await? {
                                        SessionMessage::StopReason(StopReason::Cancelled) => break,
                                        SessionMessage::StopReason(reason) => {
                                            return Err(agent_client_protocol::Error::internal_error()
                                                .data(format!("ACP cancel returned unexpected stop reason: {reason:?}")));
                                        }
                                        SessionMessage::SessionMessage(_) => {}
                                        _ => {}
                                    }
                                }
                                Ok(AcpRunOutcome::Cancelled)
                            }
                        }
                    })
                    .await
            });
        match tokio::time::timeout(self.config.timeout, run)
            .await
            .map_err(|_| AcpWorkerError::TimedOut)?
            .map_err(|e| AcpWorkerError::Protocol(e.to_string()))?
        {
            AcpRunOutcome::Completed(session_id, response) => Ok((session_id, response)),
            AcpRunOutcome::Cancelled => Err(AcpWorkerError::Cancelled),
        }
    }

    /// Sends the follow-up through the same ACP session, rather than opening
    /// another session or replaying a full task context.
    pub async fn run_with_follow_up(
        &self,
        task: &AgentTask,
        follow_up: &str,
    ) -> Result<AcpConversationResult, AcpWorkerError> {
        let first_prompt = bounded_prompt(task, self.config.max_prompt_bytes);
        let follow_up_prompt = bounded_follow_up(follow_up, self.config.max_prompt_bytes);
        let agent =
            AcpAgent::new(AcpAgentConfig::new(&self.config.command).args(self.config.args.clone()));
        let cwd = self.config.working_directory.clone();
        let run = Client
            .builder()
            .name("agentmosaic-r6")
            .connect_with(agent, async move |cx| {
                initialize_v1(&cx).await?;
                if let Some(method) = &self.config.auth_method {
                    cx.send_request(AuthenticateRequest::new(AuthMethodId::new(method.as_str())))
                        .block_task()
                        .await?;
                }
                cx.build_session(&cwd)
                    .block_task()
                    .run_until(async |mut session| {
                        let session_id = session.session_id().to_string();
                        session.send_prompt(first_prompt)?;
                        let first = session.read_to_string().await?;
                        session.send_prompt(follow_up_prompt)?;
                        let second = session.read_to_string().await?;
                        Ok((session_id, first, second))
                    })
                    .await
            });
        let (external_session_id, first, second) = tokio::time::timeout(self.config.timeout, run)
            .await
            .map_err(|_| AcpWorkerError::TimedOut)?
            .map_err(|e| AcpWorkerError::Protocol(e.to_string()))?;
        Ok(AcpConversationResult {
            external_session_id,
            first_summary: parse_peer_result(&first, self.config.max_result_bytes)?,
            follow_up_summary: parse_peer_result(&second, self.config.max_result_bytes)?,
        })
    }

    /// Resume a runtime-advertised stable-v1 session and send one bounded
    /// follow-up. The caller supplies the opaque external id recovered from
    /// the existing board binding; it is never treated as canonical task
    /// state and no prior task is replayed automatically.
    pub async fn resume_with_follow_up(
        &self,
        external_session_id: &str,
        follow_up: &str,
    ) -> Result<String, AcpWorkerError> {
        if external_session_id.trim().is_empty() {
            return Err(AcpWorkerError::InvalidConfig(
                "external session id is required for resume".into(),
            ));
        }
        let prompt = bounded_follow_up(follow_up, self.config.max_prompt_bytes);
        let agent =
            AcpAgent::new(AcpAgentConfig::new(&self.config.command).args(self.config.args.clone()));
        let cwd = self.config.working_directory.clone();
        let session_id = external_session_id.to_string();
        let run = Client
            .builder()
            .name("agentmosaic-r6")
            .connect_with(agent, async move |cx| {
                let initialized = initialize_v1(&cx).await?;
                if initialized
                    .agent_capabilities
                    .session_capabilities
                    .resume
                    .is_none()
                {
                    return Err(agent_client_protocol::Error::invalid_request()
                        .data("ACP agent did not advertise session/resume"));
                }
                if let Some(method) = &self.config.auth_method {
                    cx.send_request(AuthenticateRequest::new(AuthMethodId::new(method.as_str())))
                        .block_task()
                        .await?;
                }
                let restored = cx
                    .resume_session(session_id, &cwd)
                    .block_task()
                    .start_session()
                    .await?;
                let (mut session, _response) = restored.into_parts();
                session.send_prompt(prompt)?;
                session.read_to_string().await
            });
        let response = tokio::time::timeout(self.config.timeout, run)
            .await
            .map_err(|_| AcpWorkerError::TimedOut)?
            .map_err(|e| AcpWorkerError::Protocol(e.to_string()))?;
        parse_peer_result(&response, self.config.max_result_bytes)
    }

    pub async fn execute_task(&self, task: &AgentTask) -> Result<AcpTaskExecution, AcpWorkerError> {
        let (_cancellation, mut listener) = AcpCancellation::new();
        self.execute_task_with_cancellation(task, &mut listener)
            .await
    }

    /// Execute one task while allowing the caller to durably bind the exact
    /// external ACP session before prompt side effects begin.
    pub async fn execute_task_with_session_observer(
        &self,
        task: &AgentTask,
        session_started: AcpSessionStartedObserver,
    ) -> Result<AcpTaskExecution, AcpWorkerError> {
        let (_cancellation, mut listener) = AcpCancellation::new();
        self.execute_task_with_cancellation_and_session_observer(
            task,
            &mut listener,
            session_started,
        )
        .await
    }

    /// Execute a task while a caller both observes the newly-created external
    /// session and owns cancellation for that exact live session.  This keeps
    /// the durable board as the cross-process intent channel while preventing
    /// a separate process from naming or cancelling an arbitrary ACP session.
    pub async fn execute_task_with_cancellation_and_session_observer(
        &self,
        task: &AgentTask,
        cancellation: &mut AcpCancellationListener,
        session_started: AcpSessionStartedObserver,
    ) -> Result<AcpTaskExecution, AcpWorkerError> {
        self.execute_task_with_attempt_context(
            task,
            cancellation,
            session_started,
            None,
            None,
            1,
            self.config.runtime_kind.clone(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)] // preserves the compatibility entrypoints while carrying exact attempt context.
    async fn execute_task_with_attempt_context(
        &self,
        task: &AgentTask,
        cancellation: &mut AcpCancellationListener,
        session_started: AcpSessionStartedObserver,
        descriptor_started: Option<AcpSessionDescriptorObserver>,
        start_gate: Option<RuntimeStartGate>,
        attempt: u32,
        agent_id: String,
    ) -> Result<AcpTaskExecution, AcpWorkerError> {
        let (external_session_id, response) = self
            .run_with_cancellation_observed(
                task,
                cancellation,
                Some(session_started),
                descriptor_started,
                start_gate,
                attempt,
                agent_id,
            )
            .await?;
        let summary = parse_peer_result(&response, self.config.max_result_bytes)?;
        let artifacts = self.collect_artifacts()?;
        Ok(AcpTaskExecution {
            external_session_id,
            result: AgentTaskResult {
                task_id: task.id,
                summary,
                artifacts,
                message: None,
            },
        })
    }

    /// Execute one task with an in-process cancellation handle.  Successful
    /// results remain the existing team result shape; cancelled tasks never
    /// produce a result that a caller could commit as success.
    pub async fn execute_task_with_cancellation(
        &self,
        task: &AgentTask,
        cancellation: &mut AcpCancellationListener,
    ) -> Result<AcpTaskExecution, AcpWorkerError> {
        let (external_session_id, response) =
            self.run_with_cancellation(task, cancellation).await?;
        let summary = parse_peer_result(&response, self.config.max_result_bytes)?;
        let artifacts = self.collect_artifacts()?;
        Ok(AcpTaskExecution {
            external_session_id,
            result: AgentTaskResult {
                task_id: task.id,
                summary,
                artifacts,
                message: None,
            },
        })
    }

    fn collect_artifacts(&self) -> Result<Vec<ArtifactMeta>, AcpWorkerError> {
        self.config
            .artifact_paths
            .iter()
            .map(|relative| {
                let bytes =
                    std::fs::read(self.config.working_directory.join(relative)).map_err(|_| {
                        AcpWorkerError::InvalidPeerResult(format!(
                            "expected bounded artifact is missing: {}",
                            relative.display()
                        ))
                    })?;
                Ok(ArtifactMeta {
                    path: relative.to_string_lossy().into_owned(),
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                })
            })
            .collect()
    }
}

impl AcpRuntimeAdapter {
    pub fn new(config: AcpWorkerConfig) -> Result<Self, AcpWorkerError> {
        Ok(Self {
            worker: AcpWorkerDriver::new(config)?,
        })
    }

    pub fn from_worker(worker: AcpWorkerDriver) -> Self {
        Self { worker }
    }
}

fn runtime_error(error: AcpWorkerError) -> RuntimeError {
    match error {
        AcpWorkerError::InvalidConfig(detail) => RuntimeError::InvalidConfiguration(detail),
        AcpWorkerError::Protocol(detail) => RuntimeError::Protocol(detail),
        AcpWorkerError::TimedOut => RuntimeError::TimedOut,
        AcpWorkerError::Cancelled => RuntimeError::Cancelled,
        AcpWorkerError::InvalidPeerResult(detail) => RuntimeError::InvalidResult(detail),
    }
}

impl AcpRuntimeExecution {
    fn release(&mut self) -> Result<(), RuntimeError> {
        if let Some(release) = self.release.take() {
            release
                .send(())
                .map_err(|_| RuntimeError::Protocol("ACP execution gate was dropped".into()))?;
        }
        Ok(())
    }

    async fn finish(&mut self) -> Result<RuntimeOutcome, RuntimeError> {
        let task = self.task.take().ok_or(RuntimeError::AlreadyFinished)?;
        let execution = task
            .await
            .map_err(|error| RuntimeError::Protocol(format!("ACP execution task failed: {error}")))?
            .map_err(runtime_error)?;
        if execution.result.task_id != self.binding.task_id
            || execution.external_session_id != self.binding.native_session_id
        {
            return Err(RuntimeError::InvalidResult(
                "ACP completion did not match the bound task/session".into(),
            ));
        }
        Ok(RuntimeOutcome {
            binding: self.binding.clone(),
            result: execution.result,
        })
    }
}

#[async_trait]
impl RuntimeExecution for AcpRuntimeExecution {
    fn binding(&self) -> &RuntimeBinding {
        &self.binding
    }

    async fn wait(&mut self) -> Result<RuntimeOutcome, RuntimeError> {
        self.release()?;
        self.finish().await
    }

    async fn cancel(&mut self) -> Result<(), RuntimeError> {
        self.cancellation.cancel();
        self.release()?;
        match self.finish().await {
            Err(RuntimeError::Cancelled) => Ok(()),
            Ok(_) => Err(RuntimeError::Protocol(
                "ACP completed successfully after cancellation was requested".into(),
            )),
            Err(error) => Err(error),
        }
    }
}

#[async_trait]
impl RuntimeExecution for AcpResumedExecution {
    fn binding(&self) -> &RuntimeBinding {
        &self.binding
    }

    async fn wait(&mut self) -> Result<RuntimeOutcome, RuntimeError> {
        if self.cancelled {
            return Err(RuntimeError::Cancelled);
        }
        let summary = self
            .worker
            .resume_with_follow_up(&self.binding.native_session_id, &self.task.objective)
            .await
            .map_err(runtime_error)?;
        Ok(RuntimeOutcome {
            binding: self.binding.clone(),
            result: AgentTaskResult {
                task_id: self.task.id,
                summary,
                artifacts: Vec::new(),
                message: None,
            },
        })
    }

    async fn cancel(&mut self) -> Result<(), RuntimeError> {
        // A resume has no live session until `wait` performs the negotiated
        // request. Marking it cancelled prevents any replay/prompt side effect.
        self.cancelled = true;
        Ok(())
    }
}

#[async_trait]
impl RuntimeAdapter for AcpRuntimeAdapter {
    fn adapter_kind(&self) -> RuntimeKind {
        RuntimeKind::Acp
    }

    async fn probe(&self) -> Result<RuntimeDescriptor, RuntimeError> {
        self.worker.probe_descriptor().await.map_err(runtime_error)
    }

    async fn start(
        &self,
        request: RuntimeExecutionRequest,
        events: Arc<dyn RuntimeEventSink>,
    ) -> Result<Box<dyn RuntimeExecution>, RuntimeError> {
        let worker = self.worker.clone().with_event_sink(events);
        let timeout = worker.config.timeout;
        let (cancellation, mut listener) = AcpCancellation::new();
        let (binding_sender, binding_receiver) = oneshot::channel();
        let (release_sender, release_receiver) = oneshot::channel();
        let task = request.task.clone();
        let agent_id = request.agent_id.clone();
        let attempt = request.attempt;
        let observer: AcpSessionStartedObserver = Arc::new(|_| Ok(()));
        let join = tokio::spawn(async move {
            worker
                .execute_task_with_attempt_context(
                    &task,
                    &mut listener,
                    observer,
                    None,
                    Some(RuntimeStartGate {
                        binding: binding_sender,
                        release: release_receiver,
                    }),
                    attempt,
                    agent_id,
                )
                .await
        });
        let binding = match tokio::time::timeout(timeout, binding_receiver).await {
            Ok(Ok(binding)) => binding,
            Ok(Err(_)) => {
                let result = join.await.map_err(|error| {
                    RuntimeError::Protocol(format!("ACP starter failed: {error}"))
                })?;
                return Err(match result {
                    Ok(_) => RuntimeError::Protocol("ACP starter ended without a binding".into()),
                    Err(error) => runtime_error(error),
                });
            }
            Err(_) => {
                join.abort();
                return Err(RuntimeError::TimedOut);
            }
        };
        Ok(Box::new(AcpRuntimeExecution {
            binding,
            release: Some(release_sender),
            cancellation,
            task: Some(join),
        }))
    }

    async fn resume(
        &self,
        checkpoint: RuntimeCheckpoint,
        request: RuntimeExecutionRequest,
        events: Arc<dyn RuntimeEventSink>,
    ) -> Result<Box<dyn RuntimeExecution>, RuntimeError> {
        if checkpoint.native_session_id.trim().is_empty() {
            return Err(RuntimeError::InvalidConfiguration(
                "ACP resume requires a native session id".into(),
            ));
        }
        let descriptor = self.probe().await?;
        if !descriptor.capabilities.supports_resume {
            return Err(RuntimeError::UnsupportedCapability(
                "ACP peer did not advertise session/resume".into(),
            ));
        }
        let binding = RuntimeBinding {
            task_id: request.task.id,
            attempt: request.attempt,
            agent_id: request.agent_id,
            runtime_kind: RuntimeKind::Acp,
            native_session_id: checkpoint.native_session_id,
            descriptor,
        };
        events.emit(agentmosaic_team::RuntimeEventRecord {
            task_id: binding.task_id,
            attempt: binding.attempt,
            agent_id: binding.agent_id.clone(),
            runtime_name: binding.descriptor.runtime_name.clone(),
            native_session_id: Some(binding.native_session_id.clone()),
            event: RuntimeEvent::SessionResumed {
                native_session_id: binding.native_session_id.clone(),
            },
        })?;
        Ok(Box::new(AcpResumedExecution {
            binding,
            worker: self.worker.clone(),
            task: request.task,
            cancelled: false,
        }))
    }
}

async fn initialize_v1(
    connection: &agent_client_protocol::ConnectionTo<agent_client_protocol::Agent>,
) -> Result<InitializeResponse, agent_client_protocol::Error> {
    connection
        .send_request(InitializeRequest::new(ProtocolVersion::V1))
        .block_task()
        .await
}

fn descriptor_from_initialize(response: &InitializeResponse) -> RuntimeDescriptor {
    let capabilities = &response.agent_capabilities;
    let info = response.agent_info.as_ref();
    RuntimeDescriptor {
        runtime_name: info.map(|value| value.name.clone()),
        runtime_version: info.map(|value| value.version.clone()),
        protocol_kind: "acp".into(),
        protocol_version: "1".into(),
        capabilities: RuntimeCapabilities {
            supports_load: capabilities.load_session,
            supports_resume: capabilities.session_capabilities.resume.is_some(),
            supports_fork: false,
            supports_cancel: true,
            assistant_stream: true,
            plan_updates: true,
            tool_events: true,
            permission_requests: true,
            reverse_filesystem: false,
            reverse_terminal: false,
            subagent_events: false,
            mcp: capabilities.mcp_capabilities.http || capabilities.mcp_capabilities.sse,
        },
    }
}

async fn read_acp_turn(
    session: &mut agent_client_protocol::ActiveSession<'_, agent_client_protocol::Agent>,
    identity: &AcpEventIdentity,
    permission_policy: AcpPermissionPolicy,
) -> Result<String, agent_client_protocol::Error> {
    let mut output = String::new();
    loop {
        match session.read_update().await? {
            SessionMessage::SessionMessage(dispatch) => {
                MatchDispatch::new(dispatch)
                    .if_notification(async |notification: SessionNotification| {
                        let (delta, events) = map_acp_update(notification.update);
                        if let Some(delta) = delta {
                            output.push_str(&delta);
                        }
                        for event in events {
                            identity.emit(&notification.session_id.to_string(), event)?;
                        }
                        Ok(())
                    })
                    .await
                    .if_request(async |request: RequestPermissionRequest, responder| {
                        resolve_permission(identity, permission_policy, request, responder)
                    })
                    .await
                    .otherwise_ignore()?;
            }
            SessionMessage::StopReason(StopReason::Cancelled) => {
                return Err(agent_client_protocol::Error::internal_error()
                    .data("ACP prompt stopped as cancelled"));
            }
            SessionMessage::StopReason(_) => {
                identity.emit(
                    &session.session_id().to_string(),
                    RuntimeEvent::AssistantMessageCompleted {
                        text: output.clone(),
                    },
                )?;
                return Ok(output);
            }
            _ => {}
        }
    }
}

fn resolve_permission(
    identity: &AcpEventIdentity,
    policy: AcpPermissionPolicy,
    request: RequestPermissionRequest,
    responder: agent_client_protocol::Responder<RequestPermissionResponse>,
) -> Result<(), agent_client_protocol::Error> {
    let session_id = request.session_id.to_string();
    let request_id = request.tool_call.tool_call_id.to_string();
    let action = request
        .tool_call
        .fields
        .title
        .clone()
        .unwrap_or_else(|| request_id.clone());
    let normalized_options = request
        .options
        .iter()
        .map(|option| RuntimePermissionOption {
            option_id: option.option_id.to_string(),
            label: option.name.clone(),
        })
        .collect();
    identity.emit(
        &session_id,
        RuntimeEvent::PermissionRequested {
            request_id: request_id.clone(),
            action,
            options: normalized_options,
        },
    )?;

    let selected = request.options.iter().find(|option| match policy {
        AcpPermissionPolicy::Deny => matches!(
            option.kind,
            PermissionOptionKind::RejectOnce | PermissionOptionKind::RejectAlways
        ),
        AcpPermissionPolicy::Allow => matches!(
            option.kind,
            PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
        ),
    });
    let (outcome, decision) = match selected {
        Some(option) => {
            let option_id = option.option_id.clone();
            let decision = match policy {
                AcpPermissionPolicy::Deny => RuntimePermissionDecision::Denied,
                AcpPermissionPolicy::Allow => RuntimePermissionDecision::Allowed {
                    option_id: Some(option_id.to_string()),
                },
            };
            (
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
                decision,
            )
        }
        None => (
            RequestPermissionOutcome::Cancelled,
            RuntimePermissionDecision::Cancelled,
        ),
    };
    responder.respond(RequestPermissionResponse::new(outcome))?;
    identity.emit(
        &session_id,
        RuntimeEvent::PermissionResolved {
            request_id,
            decision,
        },
    )
}

/// Convert one typed stable-v1 update into public normalized observations.
/// Raw input/output and thought chunks are deliberately never copied.
fn map_acp_update(update: SessionUpdate) -> (Option<String>, Vec<RuntimeEvent>) {
    match update {
        SessionUpdate::AgentMessageChunk(ContentChunk {
            content: ContentBlock::Text(text),
            ..
        }) => {
            let delta = text.text;
            (
                Some(delta.clone()),
                vec![RuntimeEvent::AssistantMessageDelta { text: delta }],
            )
        }
        SessionUpdate::AgentThoughtChunk(_) => (None, Vec::new()),
        SessionUpdate::Plan(plan) => (
            None,
            vec![RuntimeEvent::PlanUpdated {
                items: plan
                    .entries
                    .into_iter()
                    .map(|entry| RuntimePlanItem {
                        text: entry.content,
                        status: Some(enum_name(&entry.status)),
                    })
                    .collect(),
            }],
        ),
        SessionUpdate::ToolCall(call) => {
            let id = call.tool_call_id.to_string();
            let mut events = vec![RuntimeEvent::ToolCallStarted {
                native_call_id: id.clone(),
                tool: call.title.clone(),
                input_summary: call.title,
            }];
            for location in call.locations {
                events.push(RuntimeEvent::FileChanged {
                    path: location.path.to_string_lossy().into_owned(),
                    change: RuntimeFileChangeKind::Modified,
                });
            }
            if matches!(
                call.status,
                ToolCallStatus::Completed | ToolCallStatus::Failed
            ) {
                events.push(RuntimeEvent::ToolCallCompleted {
                    native_call_id: id,
                    tool: "acp-tool".into(),
                    ok: call.status == ToolCallStatus::Completed,
                    output_summary: enum_name(&call.status),
                });
            }
            (None, events)
        }
        SessionUpdate::ToolCallUpdate(call) => {
            let id = call.tool_call_id.to_string();
            let status = call
                .fields
                .status
                .map(|value| enum_name(&value))
                .unwrap_or_else(|| "updated".into());
            let event = match call.fields.status {
                Some(ToolCallStatus::Completed | ToolCallStatus::Failed) => {
                    RuntimeEvent::ToolCallCompleted {
                        native_call_id: id,
                        tool: call.fields.title.unwrap_or_else(|| "acp-tool".into()),
                        ok: call.fields.status == Some(ToolCallStatus::Completed),
                        output_summary: status,
                    }
                }
                _ => RuntimeEvent::ToolCallUpdated {
                    native_call_id: id,
                    status,
                    output_summary: None,
                },
            };
            (None, vec![event])
        }
        SessionUpdate::UsageUpdate(usage) => (
            None,
            vec![RuntimeEvent::UsageUpdated {
                input_tokens: Some(usage.used),
                cached_input_tokens: None,
                output_tokens: None,
                reasoning_tokens: None,
                estimated_cost_usd: usage
                    .cost
                    .filter(|cost| cost.currency.eq_ignore_ascii_case("USD"))
                    .map(|cost| cost.amount),
            }],
        ),
        _ => (None, Vec::new()),
    }
}

fn enum_name(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "other".into())
}

impl PersistedAcpWorkerDriver {
    pub fn new(
        config: AcpWorkerConfig,
        database: PathBuf,
        agent_id: impl Into<String>,
    ) -> Result<Self, AcpWorkerError> {
        let writer = SqliteRuntimeEventWriter::new(database.clone())
            .map_err(AcpWorkerError::InvalidConfig)?;
        let events: Arc<dyn RuntimeEventSink> = Arc::new(RuntimeEventDispatcher::new(
            Arc::new(NoopLiveRuntimeEventSink),
            Arc::new(writer),
        ));
        let worker = AcpWorkerDriver::new(config)?.with_event_sink(Arc::clone(&events));
        let run = Arc::new(PersistedAcpRuntimeRun {
            adapter: AcpRuntimeAdapter::from_worker(worker),
            database,
            agent_id: agent_id.into(),
            events,
        });
        Ok(Self {
            driver: crate::RuntimeAgentDriver::new(run),
        })
    }
}

#[async_trait]
impl crate::RuntimeDriverRun for PersistedAcpRuntimeRun {
    async fn run(&self, task: AgentTask) -> Result<AgentTaskResult, RuntimeError> {
        let agent_id = self.agent_id.clone();
        let attempts = SqliteTaskBoard::open(
            rusqlite::Connection::open(&self.database)
                .map_err(|error| RuntimeError::Persistence(error.to_string()))?,
        )
        .map_err(|error| RuntimeError::Persistence(error.to_string()))?
        .attempts(task.id)
        .map_err(|error| RuntimeError::Persistence(format!("read scheduler attempt: {error:?}")))?;
        let running = attempts
            .iter()
            .filter(|row| row.status == TaskStatus::Running && row.agent_id == agent_id)
            .collect::<Vec<_>>();
        let [running] = running.as_slice() else {
            return Err(RuntimeError::Protocol(format!(
                "scheduler ACP driver requires exactly one running attempt for agent `{agent_id}`, found {}",
                running.len()
            )));
        };
        let attempt = running.attempt;
        let mut execution = self
            .adapter
            .start(
                RuntimeExecutionRequest {
                    task: task.clone(),
                    attempt,
                    agent_id: self.agent_id.clone(),
                },
                Arc::clone(&self.events),
            )
            .await?;
        let binding = execution.binding().clone();
        let board = SqliteTaskBoard::open(
            rusqlite::Connection::open(&self.database)
                .map_err(|error| RuntimeError::Persistence(error.to_string()))?,
        )
        .map_err(|error| RuntimeError::Persistence(error.to_string()))?;
        board
            .upsert_external_binding_extended(&ExtendedExternalRuntimeBinding {
                binding: ExternalRuntimeBinding {
                    team_task_id: binding.task_id,
                    attempt,
                    agent_id: self.agent_id.clone(),
                    runtime_kind: "acp".into(),
                    native_thread_id: Some(binding.native_session_id.clone()),
                    native_turn_id: None,
                    lifecycle_state: "running".into(),
                },
                runtime_name: binding.descriptor.runtime_name.clone(),
                runtime_version: binding.descriptor.runtime_version.clone(),
                protocol_kind: Some(binding.descriptor.protocol_kind.clone()),
                protocol_version: Some(binding.descriptor.protocol_version.clone()),
                capabilities_json: Some(
                    serde_json::to_string(&binding.descriptor.capabilities)
                        .map_err(|error| RuntimeError::Persistence(error.to_string()))?,
                ),
                started_at: Some(runtime_timestamp()),
                finished_at: None,
            })
            .map_err(|error| RuntimeError::Persistence(error.to_string()))?;
        let outcome = execution.wait().await?;
        let board = SqliteTaskBoard::open(
            rusqlite::Connection::open(&self.database)
                .map_err(|error| RuntimeError::Persistence(error.to_string()))?,
        )
        .map_err(|error| RuntimeError::Persistence(error.to_string()))?;
        board
            .upsert_external_binding_extended(&ExtendedExternalRuntimeBinding {
                binding: ExternalRuntimeBinding {
                    team_task_id: binding.task_id,
                    attempt,
                    agent_id: self.agent_id.clone(),
                    runtime_kind: "acp".into(),
                    native_thread_id: Some(binding.native_session_id),
                    native_turn_id: None,
                    lifecycle_state: "completed".into(),
                },
                runtime_name: binding.descriptor.runtime_name,
                runtime_version: binding.descriptor.runtime_version,
                protocol_kind: Some(binding.descriptor.protocol_kind),
                protocol_version: Some(binding.descriptor.protocol_version),
                capabilities_json: Some(
                    serde_json::to_string(&binding.descriptor.capabilities)
                        .map_err(|error| RuntimeError::Persistence(error.to_string()))?,
                ),
                started_at: None,
                finished_at: Some(runtime_timestamp()),
            })
            .map_err(|error| RuntimeError::Persistence(error.to_string()))?;
        Ok(outcome.result)
    }
}

#[async_trait]
impl AgentDriver for PersistedAcpWorkerDriver {
    async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
        self.driver.run_task(task).await
    }
}

fn runtime_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[async_trait]
impl AgentDriver for AcpWorkerDriver {
    async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
        Ok(self
            .execute_task(&task)
            .await
            .map_err(|error| error.to_string())?
            .result)
    }
}

fn bounded_prompt(task: &AgentTask, max: usize) -> String {
    let mut text = format!(
        "Task {}:\n{}\n\nBounded ACC context:\n",
        task.id, task.objective
    );
    for item in &task.context {
        if text.len() >= max {
            break;
        }
        text.push_str(
            &item
                .chars()
                .take(max.saturating_sub(text.len()))
                .collect::<String>(),
        );
        text.push('\n');
    }
    text.push_str("\nReturn exactly one JSON object with only a string field named summary. Do not include prose, markdown, credentials, hidden reasoning, or other fields.\n");
    text.chars().take(max).collect()
}

fn bounded_follow_up(follow_up: &str, max: usize) -> String {
    format!(
        "Follow-up instruction (same bounded session):\n{}\n\nReturn exactly one JSON object with only a string field named summary. Do not include prose, markdown, credentials, hidden reasoning, or other fields.\n",
        follow_up.chars().take(max / 2).collect::<String>()
    )
    .chars()
    .take(max)
    .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerResult {
    summary: String,
}

fn parse_peer_result(response: &str, max_bytes: usize) -> Result<String, AcpWorkerError> {
    if response.len() > max_bytes {
        return Err(AcpWorkerError::InvalidPeerResult(
            "response exceeds limit".into(),
        ));
    }
    let peer: PeerResult = serde_json::from_str(response.trim())
        .map_err(|_| AcpWorkerError::InvalidPeerResult("expected one strict JSON object".into()))?;
    if peer.summary.trim().is_empty() || peer.summary.len() > max_bytes {
        return Err(AcpWorkerError::InvalidPeerResult(
            "invalid summary length".into(),
        ));
    }
    Ok(peer.summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus, TextContent, ToolCall,
    };
    use agentmosaic_team::TaskKind;

    #[test]
    fn typed_acp_updates_map_without_exposing_thought_or_raw_input() {
        let (delta, events) = map_acp_update(SessionUpdate::AgentMessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new("hello")),
        )));
        assert_eq!(delta.as_deref(), Some("hello"));
        assert!(matches!(
            events.as_slice(),
            [RuntimeEvent::AssistantMessageDelta { text }] if text == "hello"
        ));

        let (_, events) = map_acp_update(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new("private reasoning")),
        )));
        assert!(events.is_empty());

        let call = ToolCall::new("call-1", "inspect repository")
            .raw_input(serde_json::json!({"token":"must-not-survive"}));
        let (_, events) = map_acp_update(SessionUpdate::ToolCall(call));
        let encoded = serde_json::to_string(&events).unwrap();
        assert!(encoded.contains("inspect repository"));
        assert!(!encoded.contains("must-not-survive"));
    }

    #[test]
    fn typed_acp_plan_maps_to_normalized_plan_items() {
        let plan = Plan::new(vec![PlanEntry::new(
            "verify the result",
            PlanEntryPriority::High,
            PlanEntryStatus::InProgress,
        )]);
        let (_, events) = map_acp_update(SessionUpdate::Plan(plan));
        assert!(matches!(
            events.as_slice(),
            [RuntimeEvent::PlanUpdated { items }]
                if items[0].text == "verify the result"
                    && items[0].status.as_deref() == Some("in_progress")
        ));
    }

    #[test]
    fn prompt_is_bounded() {
        let task = AgentTask {
            id: 1,
            objective: "x".repeat(100),
            kind: TaskKind::Bulk,
            context: vec!["y".repeat(100)],
        };
        assert!(bounded_prompt(&task, 32).len() <= 32);
    }

    #[test]
    fn peer_result_is_strict_and_bounded() {
        assert_eq!(
            parse_peer_result(r#"{"summary":"peer finding"}"#, 64).unwrap(),
            "peer finding"
        );
        assert!(parse_peer_result(r#"{"summary":"x","extra":true}"#, 64).is_err());
        assert!(parse_peer_result("not-json", 64).is_err());
    }

    #[test]
    fn follow_up_prompt_is_bounded_and_requires_structured_result() {
        let prompt = bounded_follow_up(&"x".repeat(100), 48);
        assert!(prompt.len() <= 48);
        assert!(prompt.contains("Follow-up"));
    }

    #[test]
    fn resume_rejects_an_empty_external_session_id() {
        let cwd = std::env::temp_dir();
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "test".into(),
            command: PathBuf::from("test-agent"),
            args: Vec::new(),
            auth_method: None,
            working_directory: cwd,
            timeout: Duration::from_secs(1),
            max_prompt_bytes: 64,
            max_result_bytes: 64,
            artifact_paths: Vec::new(),
        })
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let error = runtime
            .block_on(driver.resume_with_follow_up("", "continue"))
            .unwrap_err();
        assert!(matches!(error, AcpWorkerError::InvalidConfig(_)));
    }

    #[test]
    fn artifact_collection_is_relative_hashed_and_fail_closed() {
        let cwd = std::env::temp_dir().join(format!(
            "agentmosaic_acp_artifacts_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(cwd.join("result.txt"), "exact bytes\n").unwrap();
        let config = AcpWorkerConfig {
            runtime_kind: "test".into(),
            command: PathBuf::from("test-agent"),
            args: Vec::new(),
            auth_method: None,
            working_directory: cwd.clone(),
            timeout: Duration::from_secs(1),
            max_prompt_bytes: 64,
            max_result_bytes: 64,
            artifact_paths: vec![PathBuf::from("result.txt")],
        };
        let driver = AcpWorkerDriver::new(config).unwrap();
        let artifacts = driver.collect_artifacts().unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].path, "result.txt");
        assert_eq!(
            artifacts[0].sha256,
            "6a77ce4ad94636f6120bb985066c1d75ce65b73f264a35f9d5ac910e252f0355"
        );
        let missing = AcpWorkerDriver::new(AcpWorkerConfig {
            artifact_paths: vec![PathBuf::from("missing.txt")],
            ..driver.config.clone()
        })
        .unwrap();
        assert!(missing.collect_artifacts().is_err());
        assert!(AcpWorkerDriver::new(AcpWorkerConfig {
            artifact_paths: vec![PathBuf::from("../escape")],
            ..driver.config.clone()
        })
        .is_err());
        let _ = std::fs::remove_dir_all(cwd);
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code; asserts only the current no-credential path"]
    async fn qwen_acp_reports_auth_required_without_running_a_task() {
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "qwen-code".into(),
            command: PathBuf::from("qwen"),
            args: vec!["--acp".into(), "--bare".into()],
            auth_method: None,
            working_directory: std::env::temp_dir(),
            timeout: Duration::from_secs(15),
            max_prompt_bytes: 1024,
            max_result_bytes: 4096,
            artifact_paths: Vec::new(),
        })
        .expect("valid local Qwen profile");
        let error = driver
            .run(&AgentTask {
                id: 1,
                objective: "no task should be sent without authentication".into(),
                kind: TaskKind::Bulk,
                context: Vec::new(),
            })
            .await
            .expect_err("current Qwen environment requires authentication");
        assert!(error.to_string().contains("Authentication required"));
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn qwen_acp_authenticated_session_completes_a_bounded_task() {
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "qwen-code".into(),
            command: PathBuf::from("qwen"),
            args: vec!["--acp".into()],
            auth_method: Some("openai".into()),
            working_directory: std::env::temp_dir(),
            timeout: Duration::from_secs(180),
            max_prompt_bytes: 1024,
            max_result_bytes: 4096,
            artifact_paths: Vec::new(),
        })
        .expect("valid local Qwen profile");
        let result = driver
            .run_task(AgentTask {
                id: 1,
                objective: "Return the bounded peer finding pong.".into(),
                kind: TaskKind::Bulk,
                context: Vec::new(),
            })
            .await
            .expect("authenticated ACP task completes");
        assert!(!result.summary.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn qwen_acp_reuses_one_authenticated_session_for_follow_up() {
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "qwen-code".into(),
            command: PathBuf::from("qwen"),
            args: vec!["--acp".into()],
            auth_method: Some("openai".into()),
            working_directory: std::env::temp_dir(),
            timeout: Duration::from_secs(180),
            max_prompt_bytes: 1024,
            max_result_bytes: 4096,
            artifact_paths: Vec::new(),
        })
        .expect("valid local Qwen profile");
        let result =
            driver
                .run_with_follow_up(
                    &AgentTask {
                        id: 2,
                        objective:
                            "Return exactly this JSON peer result: {\"summary\":\"first-pass\"}."
                                .into(),
                        kind: TaskKind::Bulk,
                        context: Vec::new(),
                    },
                    "Return exactly this JSON peer result: {\"summary\":\"follow-up-pass\"}.",
                )
                .await
                .expect("same authenticated ACP session completes follow-up");
        assert!(!result.external_session_id.is_empty());
        assert!(!result.first_summary.is_empty());
        assert!(!result.follow_up_summary.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn qwen_acp_resumes_a_persisted_session_for_a_follow_up() {
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "qwen-code".into(),
            command: PathBuf::from("qwen"),
            args: vec!["--acp".into()],
            auth_method: Some("openai".into()),
            working_directory: std::env::temp_dir(),
            timeout: Duration::from_secs(180),
            max_prompt_bytes: 1024,
            max_result_bytes: 4096,
            artifact_paths: Vec::new(),
        })
        .expect("valid local Qwen profile");
        let (session_id, _) = driver
            .run(&AgentTask {
                id: 3,
                objective: "Return exactly this JSON peer result: {\"summary\":\"resume-seed\"}."
                    .into(),
                kind: TaskKind::Bulk,
                context: Vec::new(),
            })
            .await
            .expect("seed session completes");
        let summary = driver
            .resume_with_follow_up(
                &session_id,
                "Return exactly this JSON peer result: {\"summary\":\"resume-follow-up\"}.",
            )
            .await
            .expect("resumed session completes follow-up");
        assert_eq!(summary, "resume-follow-up");
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn qwen_acp_completes_a_bounded_isolated_coding_task() {
        let cwd = std::env::temp_dir().join(format!(
            "agentmosaic_qwen_coding_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&cwd).unwrap();
        let git_status = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&cwd)
            .status()
            .expect("git available for isolated fixture");
        assert!(git_status.success(), "initialize isolated git repository");
        std::fs::write(cwd.join("status.txt"), "status=broken\n").unwrap();
        std::fs::write(
            cwd.join("check.sh"),
            "#!/bin/sh\ntest \"$(cat status.txt)\" = \"status=fixed\"\n",
        )
        .unwrap();
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "qwen-code".into(),
            command: PathBuf::from("qwen"),
            args: vec!["--acp".into()],
            auth_method: Some("openai".into()),
            working_directory: cwd.clone(),
            // Contention-calibrated budget; see the live test note.
            timeout: Duration::from_secs(600),
            max_prompt_bytes: 2048,
            max_result_bytes: 4096,
            artifact_paths: vec![PathBuf::from("status.txt")],
        })
        .expect("valid local Qwen profile");
        let result = driver
            .run_task(AgentTask {
                id: 3,
                objective: "In this isolated Git repository, inspect status.txt, replace its exact contents with status=fixed followed by one newline, run `sh check.sh`, and then return exactly this JSON peer result: {\"summary\":\"status fixed and check passed\"}. Do not modify any other file.".into(),
                kind: TaskKind::Bulk,
                context: Vec::new(),
            })
            .await
            .expect("Qwen completes bounded coding task");
        assert_eq!(
            std::fs::read(cwd.join("status.txt")).unwrap(),
            b"status=fixed\n"
        );
        assert!(!result.summary.is_empty());
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].path, "status.txt");
        let _ = std::fs::remove_dir_all(cwd);
    }

    #[tokio::test]
    #[ignore = "requires locally configured Kimi Code ACP; sends one bounded no-tool prompt in an isolated directory"]
    async fn kimi_acp_completes_a_bounded_no_tool_turn() {
        let cwd = acp_m2_probe_cwd("kimi_bounded");
        std::fs::create_dir_all(&cwd).expect("Kimi probe work directory");
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "kimi-code".into(),
            command: PathBuf::from("kimi"),
            args: vec!["acp".into()],
            auth_method: None,
            working_directory: cwd.clone(),
            timeout: Duration::from_secs(180),
            max_prompt_bytes: 512,
            max_result_bytes: 1024,
            artifact_paths: Vec::new(),
        })
        .expect("valid local Kimi ACP profile");
        let result = driver
            .run(&AgentTask {
                id: 9010,
                objective: "Do not use tools, shell commands, network access, or filesystem writes. Return exactly this JSON peer result: {\"summary\":\"kimi bounded task complete\"}.".into(),
                kind: TaskKind::Reasoning,
                context: Vec::new(),
            })
            .await;
        let _ = std::fs::remove_dir_all(&cwd);
        let (session_id, response) = result.expect("Kimi bounded ACP turn completes");
        assert!(!session_id.is_empty(), "Kimi returns a session reference");
        assert!(!response.is_empty(), "Kimi returns a bounded turn response");
    }

    const ACP_M2_PROBE_TARGET: &str = "qwen";
    const ACP_M2_INSPECT_SHA256: &str =
        "4f9cb58fb7462cbc9d82112069421c479b28fc8bd74a2abaa7fdd899ce63914b";

    fn acp_m2_probe_emit(name: &str, bucket: &str, detail: &str) {
        println!(
            "ACPM2PROBE {} bucket={} detail={}",
            name,
            bucket,
            detail.chars().take(80).collect::<String>()
        );
    }

    fn acp_m2_probe_cwd(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "acp_m2_probe_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn acp_m2_probe_driver(
        timeout: Duration,
        working_directory: PathBuf,
        artifact_paths: Vec<PathBuf>,
    ) -> Result<AcpWorkerDriver, AcpWorkerError> {
        AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "qwen-code".into(),
            command: PathBuf::from(ACP_M2_PROBE_TARGET),
            args: vec!["--acp".into()],
            auth_method: Some("openai".into()),
            working_directory,
            timeout,
            max_prompt_bytes: 2048,
            max_result_bytes: 4096,
            artifact_paths,
        })
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn acp_m2_probe_follow_up_same_session() {
        let cwd = acp_m2_probe_cwd("follow_up");
        std::fs::create_dir_all(&cwd).expect("probe work directory");
        let driver = acp_m2_probe_driver(Duration::from_secs(180), cwd.clone(), Vec::new())
            .expect("valid probe driver");
        let task = AgentTask {
            id: 9001,
            objective: "Return exactly this JSON peer result: {\"summary\":\"first-pass\"}.".into(),
            kind: TaskKind::Bulk,
            context: Vec::new(),
        };
        let follow_up = "Return exactly this JSON peer result: {\"summary\":\"follow-up-pass\"}.";
        match driver.run_with_follow_up(&task, follow_up).await {
            Ok(conversation)
                if !conversation.external_session_id.is_empty()
                    && !conversation.first_summary.is_empty()
                    && !conversation.follow_up_summary.is_empty() =>
            {
                acp_m2_probe_emit(
                    "acp_m2_probe_follow_up_same_session",
                    "supported",
                    &format!("ssid_len={}", conversation.external_session_id.len()),
                );
            }
            Ok(_) => acp_m2_probe_emit(
                "acp_m2_probe_follow_up_same_session",
                "failed",
                "peer_invalid",
            ),
            Err(AcpWorkerError::TimedOut) => acp_m2_probe_emit(
                "acp_m2_probe_follow_up_same_session",
                "timed-out",
                "rt=180s",
            ),
            Err(AcpWorkerError::Cancelled) => acp_m2_probe_emit(
                "acp_m2_probe_follow_up_same_session",
                "failed",
                "unexpected_cancelled",
            ),
            Err(AcpWorkerError::InvalidPeerResult(_)) => acp_m2_probe_emit(
                "acp_m2_probe_follow_up_same_session",
                "failed",
                "peer_invalid",
            ),
            Err(AcpWorkerError::InvalidConfig(_)) => acp_m2_probe_emit(
                "acp_m2_probe_follow_up_same_session",
                "failed",
                "config_invalid",
            ),
            Err(AcpWorkerError::Protocol(_)) => {
                acp_m2_probe_emit("acp_m2_probe_follow_up_same_session", "failed", "protocol")
            }
        }
        let _ = std::fs::remove_dir_all(&cwd);
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn acp_m2_probe_cancel_active_session() {
        let cwd = acp_m2_probe_cwd("cancel");
        std::fs::create_dir_all(&cwd).expect("probe work directory");
        let driver = acp_m2_probe_driver(Duration::from_secs(180), cwd.clone(), Vec::new())
            .expect("valid probe driver");
        let (cancellation, mut listener) = AcpCancellation::new();
        let task = AgentTask {
            id: 9002,
            objective: "State that this probe turn will be cancelled imminently. Stop when the cancellation arrives.".into(),
            kind: TaskKind::Bulk,
            context: Vec::new(),
        };
        let run = tokio::spawn(async move {
            driver
                .execute_task_with_cancellation(&task, &mut listener)
                .await
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancellation.cancel();
        match tokio::time::timeout(Duration::from_secs(180), run).await {
            Err(_) => {
                acp_m2_probe_emit("acp_m2_probe_cancel_active_session", "timed-out", "rt=180s")
            }
            Ok(Err(_)) => acp_m2_probe_emit("acp_m2_probe_cancel_active_session", "failed", "join"),
            Ok(Ok(Err(AcpWorkerError::Cancelled))) => acp_m2_probe_emit(
                "acp_m2_probe_cancel_active_session",
                "supported",
                "peer_confirmed_cancel",
            ),
            Ok(Ok(Err(_))) | Ok(Ok(Ok(_))) => acp_m2_probe_emit(
                "acp_m2_probe_cancel_active_session",
                "failed",
                "cancel_not_confirmed",
            ),
        }
        let _ = std::fs::remove_dir_all(&cwd);
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn acp_m2_probe_process_exit_retry() {
        let cwd = acp_m2_probe_cwd("process_retry");
        std::fs::create_dir_all(&cwd).expect("probe work directory");
        let driver = acp_m2_probe_driver(Duration::from_millis(2000), cwd.clone(), Vec::new())
            .expect("valid probe driver");
        let task = AgentTask {
            id: 9003,
            objective: "Return exactly this JSON peer result: {\"summary\":\"retry-readiness\"}."
                .into(),
            kind: TaskKind::Bulk,
            context: Vec::new(),
        };
        let attempt1 = driver.execute_task(&task).await;
        let attempt2 = driver.execute_task(&task).await;
        match (attempt1, attempt2) {
            (Ok(_), _) => acp_m2_probe_emit(
                "acp_m2_probe_process_exit_retry",
                "supported",
                "attempt1_ok",
            ),
            (Err(_), Ok(_)) => {
                acp_m2_probe_emit("acp_m2_probe_process_exit_retry", "supported", "retry_ok")
            }
            (Err(first), Err(second))
                if matches!(first, AcpWorkerError::TimedOut)
                    && matches!(second, AcpWorkerError::TimedOut) =>
            {
                acp_m2_probe_emit(
                    "acp_m2_probe_process_exit_retry",
                    "timed-out",
                    "r2_both rt=2000",
                );
            }
            (Err(_), Err(_)) => acp_m2_probe_emit(
                "acp_m2_probe_process_exit_retry",
                "failed",
                "r2_both_failed",
            ),
        }
        let _ = std::fs::remove_dir_all(&cwd);
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn acp_m2_probe_load_verify() {
        let cwd = acp_m2_probe_cwd("load_verify");
        std::fs::create_dir_all(&cwd).expect("probe work directory");
        let driver = acp_m2_probe_driver(Duration::from_secs(600), cwd.clone(), Vec::new())
            .expect("valid probe driver");
        let task = AgentTask {
            id: 9004,
            objective: "Return exactly this JSON peer result: {\"summary\":\"load-verify\"}."
                .into(),
            kind: TaskKind::Bulk,
            context: Vec::new(),
        };
        match driver.execute_task(&task).await {
            Ok(execution)
                if !execution.external_session_id.is_empty()
                    && execution.result.task_id == task.id
                    && !execution.result.summary.is_empty() =>
            {
                acp_m2_probe_emit(
                    "acp_m2_probe_load_verify",
                    "supported",
                    &format!("ssid_len={}", execution.external_session_id.len()),
                );
            }
            Ok(_) => acp_m2_probe_emit("acp_m2_probe_load_verify", "failed", "protocol"),
            Err(AcpWorkerError::TimedOut) => {
                acp_m2_probe_emit("acp_m2_probe_load_verify", "timed-out", "rt=600s")
            }
            Err(AcpWorkerError::Cancelled) => {
                acp_m2_probe_emit("acp_m2_probe_load_verify", "failed", "unexpected_cancelled")
            }
            Err(AcpWorkerError::InvalidPeerResult(_)) => {
                acp_m2_probe_emit("acp_m2_probe_load_verify", "failed", "peer_invalid")
            }
            Err(AcpWorkerError::InvalidConfig(_)) => {
                acp_m2_probe_emit("acp_m2_probe_load_verify", "failed", "config_invalid")
            }
            Err(AcpWorkerError::Protocol(_)) => {
                acp_m2_probe_emit("acp_m2_probe_load_verify", "failed", "protocol")
            }
        }
        let _ = std::fs::remove_dir_all(&cwd);
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn acp_m2_probe_inspect() {
        let cwd = acp_m2_probe_cwd("inspect");
        std::fs::create_dir_all(&cwd).expect("probe work directory");
        let git_status = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&cwd)
            .status()
            .expect("git available for isolated fixture");
        assert!(git_status.success(), "initialize isolated git repository");
        std::fs::write(cwd.join("status.txt"), "status=broken\n").expect("probe fixture write");
        std::fs::write(
            cwd.join("check.sh"),
            "#!/bin/sh\ntest \"$(cat status.txt)\" = \"status=fixed\"\n",
        )
        .expect("probe fixture write");
        let driver = acp_m2_probe_driver(
            Duration::from_secs(600),
            cwd.clone(),
            vec![PathBuf::from("status.txt")],
        )
        .expect("valid probe driver");
        let task = AgentTask {
            id: 9005,
            objective:
                "In this isolated Git repository, inspect status.txt, replace its exact contents with status=fixed followed by one newline, run `sh check.sh`, and then return exactly this JSON peer result: {\"summary\":\"status fixed and check passed\"}. Do not modify any other file."
                    .into(),
            kind: TaskKind::Bulk,
            context: Vec::new(),
        };
        match driver.execute_task(&task).await {
            Ok(execution) => {
                let inspect_ok = execution.result.artifacts.iter().any(|artifact| {
                    artifact.path == "status.txt" && artifact.sha256 == ACP_M2_INSPECT_SHA256
                });
                if inspect_ok {
                    acp_m2_probe_emit("acp_m2_probe_inspect", "supported", "inspect_sha_ok");
                } else {
                    acp_m2_probe_emit("acp_m2_probe_inspect", "failed", "mismatch");
                }
            }
            Err(AcpWorkerError::TimedOut) => {
                acp_m2_probe_emit("acp_m2_probe_inspect", "timed-out", "rt=600s")
            }
            Err(AcpWorkerError::Cancelled) => {
                acp_m2_probe_emit("acp_m2_probe_inspect", "failed", "unexpected_cancelled")
            }
            Err(AcpWorkerError::InvalidPeerResult(_)) => {
                acp_m2_probe_emit("acp_m2_probe_inspect", "failed", "peer_invalid")
            }
            Err(AcpWorkerError::InvalidConfig(_)) => {
                acp_m2_probe_emit("acp_m2_probe_inspect", "failed", "config_invalid")
            }
            Err(AcpWorkerError::Protocol(_)) => {
                acp_m2_probe_emit("acp_m2_probe_inspect", "failed", "protocol")
            }
        }
        let _ = std::fs::remove_dir_all(&cwd);
    }
}
