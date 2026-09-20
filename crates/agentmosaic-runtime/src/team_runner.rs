//! The product team runner: one objective in, one durable team result out.
//!
//! This is the orchestration the product owns, not a test harness. One call:
//! reads the durable registry, builds one real driver per registered Agent,
//! resolves the Lead, creates the durable root task (with its Lead attempt
//! already recorded as `Running`), and drives the Lead's plan → delegate →
//! follow-up → synthesize loop through the scheduler. Worker results and
//! artifacts reach the Lead as durable board context, and the final answer plus
//! the exact selected refs are persisted on the root task.
//!
//! Nothing else orchestrates: no plan file is read, no task is created outside
//! the Lead and the scheduler, no driver runs outside the scheduler, and one
//! Agent's output never becomes another Agent's prompt.
//!
//! A returned error never leaves the root `Running`: the root's Lead attempt is
//! settled as `Failed` on a freshly reopened connection, so the durable state
//! always says what happened. `resume` is the recovery entry point — it returns
//! a succeeded root's persisted result idempotently, and otherwise closes
//! interrupted descendant attempts with the board's existing no-replay
//! recovery primitive before continuing the Lead.
//!
//! A run may also report its lifecycle to an optional [`RunEventSink`]. The
//! projection is notification only: the durable board stays the only truth, the
//! default sink is the no-op, and every event is emitted after the mutation it
//! reports is durable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry, SqliteTaskBoard};
use agentmosaic_team::{
    bounded_event_text, reconstruct_team_result, AgentConfig, AgentDriver, AgentRegistry,
    AgentTier, BoardError, DriverKind, Lead, LeadBrain, LeadBrainError, LeadError,
    NoopRunEventSink, RegistryError, RunEvent, RunEventSink, Scheduler, TaskAttempt, TaskBoard,
    TaskKind, TaskStatus, TeamResult,
};
use rusqlite::Connection;

use crate::driver_factory::{
    codex_option_values, launch_spec, parse_agent_options, DriverFactory, DriverFactoryError,
};
use crate::{CodexExecLeadBrain, CodexExecLeadConfig, CodexLeadBrain, CodexLeadConfig, LaunchSpec};

/// Default bound for one automatic team run.
pub const DEFAULT_MAX_ROUNDS: u32 = 8;
/// Default task budget for one automatic team run.
pub const DEFAULT_MAX_TASKS: usize = 32;
/// Default per-agent retry limit before the scheduler reassigns.
pub const DEFAULT_MAX_RETRIES: u32 = 2;
/// Default byte bound for one round of Lead context.
pub const DEFAULT_LEAD_MAX_PROMPT_BYTES: usize = 32768;
/// Default byte bound for the final answer.
pub const DEFAULT_LEAD_MAX_ANSWER_BYTES: usize = 16384;
/// Default event bound for one Lead turn.
pub const DEFAULT_LEAD_MAX_EVENTS: usize = 200;

/// The options one team run is bounded by.
#[derive(Debug, Clone)]
pub struct TeamRunOptions {
    /// The Lead agent id. `None` selects the only registered `reasoner`.
    pub lead_agent: Option<String>,
    pub max_rounds: u32,
    pub max_tasks: usize,
    pub max_retries: u32,
}

impl Default for TeamRunOptions {
    fn default() -> Self {
        Self {
            lead_agent: None,
            max_rounds: DEFAULT_MAX_ROUNDS,
            max_tasks: DEFAULT_MAX_TASKS,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }
}

/// The outcome of one team run: the durable root task and the final result that
/// was persisted on it.
#[derive(Debug, Clone)]
pub struct TeamRunOutcome {
    pub root_task_id: u64,
    pub lead_agent: String,
    pub result: TeamResult,
}

/// An error from the team runner.
#[derive(Debug, Clone)]
pub enum TeamRunnerError {
    /// The repository the run works in does not exist.
    RepoMissing(PathBuf),
    /// The run objective is empty.
    EmptyObjective,
    /// The durable registry could not be read.
    RegistryStorage(String),
    /// A registered Agent has a tier no run can route.
    UnsupportedTier { agent: String, tier: String },
    /// A registered Agent's tags/argv could not be read.
    InvalidAgentField { agent: String, detail: String },
    /// The registry itself rejected the agent set.
    Registry(RegistryError),
    /// The Lead could not be resolved unambiguously.
    LeadSelection(String),
    /// The Lead has no executable, so no Lead brain can be built.
    MissingLeadExecutable(String),
    /// The selected reasoner's runtime has no conforming Lead adapter.
    UnsupportedLeadRuntime { agent: String, kind: String },
    /// A driver could not be constructed.
    Driver(DriverFactoryError),
    /// The Lead brain could not be built or failed.
    LeadBrain(LeadBrainError),
    /// The Lead loop failed.
    Lead(LeadError),
    /// A board operation failed.
    Board(BoardError),
    /// The resumable root task does not exist.
    UnknownRoot(u64),
    /// The named root task is not a reasoning task.
    RootNotReasoning { root: u64, kind: TaskKind },
    /// The Lead failed and the root could not be settled as failed.
    LeadAndSettleFailed { cause: String, settle: String },
}

impl std::fmt::Display for TeamRunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepoMissing(path) => {
                write!(f, "repository {} is not an existing directory", path.display())
            }
            Self::EmptyObjective => write!(f, "the team objective must not be empty"),
            Self::RegistryStorage(detail) => write!(f, "reading the agent registry failed: {detail}"),
            Self::UnsupportedTier { agent, tier } => {
                write!(f, "agent `{agent}` has unknown tier `{tier}`")
            }
            Self::InvalidAgentField { agent, detail } => {
                write!(f, "agent `{agent}` has an invalid registry field: {detail}")
            }
            Self::Registry(error) => match error {
                RegistryError::MissingTier(tier) => write!(
                    f,
                    "no agent is registered for the {tier:?} tier; a team run needs at least one reasoner and one worker; utility agents are optional"
                ),
                RegistryError::ZeroConcurrency(agent) => write!(
                    f,
                    "agent `{agent}` has a max-concurrency of zero; every registered Agent needs a positive concurrency"
                ),
                other => write!(f, "the agent registry is not runnable: {other:?}"),
            },
            Self::LeadSelection(detail) => write!(f, "the team lead could not be resolved: {detail}"),
            Self::MissingLeadExecutable(agent) => {
                write!(f, "lead agent `{agent}` has no executable")
            }
            Self::UnsupportedLeadRuntime { agent, kind } => write!(
                f,
                "lead agent `{agent}` has runtime `{kind}`, which is not supported for the Lead role"
            ),
            Self::Driver(error) => write!(f, "{error}"),
            Self::LeadBrain(error) => write!(f, "{error}"),
            Self::Lead(error) => write!(f, "the lead run failed: {error}"),
            Self::Board(error) => write!(f, "a task board operation failed: {error}"),
            Self::UnknownRoot(root) => write!(f, "no task {root} exists to resume"),
            Self::RootNotReasoning { root, kind } => write!(
                f,
                "task {root} is a {} task, not the reasoning root of a team run",
                kind.as_str()
            ),
            Self::LeadAndSettleFailed { cause, settle } => write!(
                f,
                "the lead run failed ({cause}) and the root task could not be settled as failed: {settle}"
            ),
        }
    }
}

/// Constructs the selected reasoner's stateful Lead implementation.
///
/// The factory is a runtime composition concern: the team crate owns the
/// vendor-neutral [`LeadBrain`] contract, while durable registry rows select a
/// concrete runtime here. Implementations must validate without starting a
/// process so a rejected Lead never leaves a root task behind.
pub trait LeadBrainFactory: Send + Sync {
    fn build(
        &self,
        record: &AgentRegistryRecord,
        candidates: Vec<String>,
    ) -> Result<Box<dyn LeadBrain>, TeamRunnerError>;
}

/// The product Lead factory. More conforming Lead runtimes are added to this
/// dispatch table; role selection itself remains independent of any vendor.
pub struct DefaultLeadBrainFactory {
    repo: PathBuf,
    database: Option<PathBuf>,
}

impl DefaultLeadBrainFactory {
    pub fn new(repo: impl Into<PathBuf>) -> Self {
        Self {
            repo: repo.into(),
            database: None,
        }
    }

    pub fn with_database(mut self, database: impl Into<PathBuf>) -> Self {
        self.database = Some(database.into());
        self
    }
}

impl LeadBrainFactory for DefaultLeadBrainFactory {
    fn build(
        &self,
        record: &AgentRegistryRecord,
        candidates: Vec<String>,
    ) -> Result<Box<dyn LeadBrain>, TeamRunnerError> {
        ensure_supported_lead_runtime(record)?;
        let config = lead_config(record, &self.repo)?;
        match record.driver_kind.as_deref().and_then(DriverKind::restore) {
            Some(DriverKind::CodexAppServer) => {
                Ok(Box::new(CodexLeadBrain::new(config, candidates)?))
            }
            Some(DriverKind::CodexExec) => Ok(Box::new(CodexExecLeadBrain::new(
                CodexExecLeadConfig {
                    launch: config.launch,
                    working_directory: config.working_directory,
                    max_prompt_bytes: config.max_prompt_bytes,
                    max_answer_bytes: config.max_answer_bytes,
                    timeout: std::time::Duration::from_secs(300),
                    isolate: false,
                    binding_database: self.database.clone(),
                    binding_agent_id: self.database.as_ref().map(|_| record.id.clone()),
                },
                candidates,
            )?)),
            _ => unreachable!("lead runtime was validated"),
        }
    }
}

impl std::error::Error for TeamRunnerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Driver(error) => Some(error),
            Self::LeadBrain(error) => Some(error),
            _ => None,
        }
    }
}

impl From<BoardError> for TeamRunnerError {
    fn from(error: BoardError) -> Self {
        Self::Board(error)
    }
}

impl From<DriverFactoryError> for TeamRunnerError {
    fn from(error: DriverFactoryError) -> Self {
        Self::Driver(error)
    }
}

impl From<LeadBrainError> for TeamRunnerError {
    fn from(error: LeadBrainError) -> Self {
        Self::LeadBrain(error)
    }
}

/// One objective, one durable team run.
pub struct TeamRunner {
    database: PathBuf,
    repo: PathBuf,
    options: TeamRunOptions,
    bridge_host: Option<LaunchSpec>,
    lead_factory: Arc<dyn LeadBrainFactory>,
    sink: Arc<dyn RunEventSink>,
}

impl TeamRunner {
    pub fn new(
        database: impl Into<PathBuf>,
        repo: impl Into<PathBuf>,
        options: TeamRunOptions,
    ) -> Self {
        let repo = repo.into();
        let database = database.into();
        Self {
            database: database.clone(),
            lead_factory: Arc::new(
                DefaultLeadBrainFactory::new(repo.clone()).with_database(database),
            ),
            repo,
            options,
            bridge_host: None,
            sink: Arc::new(NoopRunEventSink),
        }
    }

    pub fn with_bridge_host(mut self, host: LaunchSpec) -> Self {
        self.bridge_host = Some(host);
        self
    }

    /// Override runtime construction while preserving the product's existing
    /// Lead loop and all of its durable scheduling semantics.
    pub fn with_lead_brain_factory(mut self, factory: Arc<dyn LeadBrainFactory>) -> Self {
        self.lead_factory = factory;
        self
    }

    /// Attach the presentation sink the whole run reports its lifecycle to. The
    /// default is the no-op, so a runner without a sink behaves exactly as it
    /// did before the projection existed. The sink is non-authoritative: the
    /// durable board stays the only truth.
    pub fn with_sink(mut self, sink: Arc<dyn RunEventSink>) -> Self {
        self.sink = sink;
        self
    }

    /// Run `objective` to a durable final result: create the root, drive the
    /// Lead through the scheduler, and return the result the Lead persisted.
    pub async fn run(&self, objective: &str) -> Result<TeamRunOutcome, TeamRunnerError> {
        if objective.trim().is_empty() {
            return Err(TeamRunnerError::EmptyObjective);
        }
        self.ensure_repo()?;
        let records = self.records()?;
        let registry = agent_registry(&records)?;
        let lead = resolve_lead(&records, self.options.lead_agent.as_deref())?;
        // The brain is built before the board is touched: constructing it starts
        // no process, and a configuration error must never leave a root behind.
        let brain = self.lead_factory.build(
            &lead,
            registry
                .agent_ids()
                .into_iter()
                .map(str::to_string)
                .collect(),
        )?;
        let drivers = self.drivers(&records)?;
        // The durable root: the Lead owns it, its attempt is observable as
        // Running before the external Lead turn can have any side effect, and
        // the Lead's `persist_final` settles exactly this row.
        let mut board = self.open_board()?;
        let root =
            board.create_task(objective, None, TaskKind::Reasoning, Some(lead.id.clone()))?;
        board.assign(root, &lead.id)?;
        board.record_attempt(&TaskAttempt {
            task_id: root,
            attempt: 1,
            agent_id: lead.id.clone(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })?;
        board.set_status(root, TaskStatus::Running)?;
        // The root is durable and Running: the run can now be observed.
        self.sink.emit(&RunEvent::RunStarted {
            root_task_id: root,
            lead_agent: lead.id.clone(),
        });

        let scheduler = Scheduler::new(registry, drivers, board, self.options.max_retries)
            .with_sink(Arc::clone(&self.sink));
        let mut lead_loop = Lead::new(
            brain,
            scheduler,
            self.options.max_rounds,
            self.options.max_tasks,
            lead.id.clone(),
        );
        self.drive(&mut lead_loop, root, objective, &lead.id).await
    }

    /// Resume a team run whose root task already exists.
    ///
    /// A root that already succeeded returns its persisted result unchanged —
    /// nothing is replayed and no driver, brain, or task is touched. Otherwise
    /// every interrupted descendant attempt is closed with the board's existing
    /// no-replay recovery primitive, and the Lead continues from the durable
    /// board state it left behind.
    pub async fn resume(&self, root_task_id: u64) -> Result<TeamRunOutcome, TeamRunnerError> {
        self.ensure_repo()?;
        let mut board = self.open_board()?;
        let root = board
            .task(root_task_id)?
            .ok_or(TeamRunnerError::UnknownRoot(root_task_id))?;
        if root.kind != TaskKind::Reasoning {
            return Err(TeamRunnerError::RootNotReasoning {
                root: root_task_id,
                kind: root.kind,
            });
        }
        if root.status == TaskStatus::Succeeded {
            // Idempotent, and deliberately independent of the current registry:
            // the durable result is the answer, so an idempotent resume must not
            // depend on drivers that will never run.
            let result = reconstruct_team_result(&board, root_task_id)?;
            let lead_agent = match root.assignee {
                Some(assignee) => assignee,
                None => resolve_lead(&self.records()?, self.options.lead_agent.as_deref())?.id,
            };
            return Ok(TeamRunOutcome {
                root_task_id,
                lead_agent,
                result,
            });
        }
        let objective = root.objective.clone();
        let records = self.records()?;
        let registry = agent_registry(&records)?;
        let lead = resolve_lead(&records, self.options.lead_agent.as_deref())?;
        // Every fallible configuration step happens before the board is
        // mutated, so a configuration error leaves the recoverable state for a
        // later resume instead of a half-driven run.
        let brain = self.lead_factory.build(
            &lead,
            registry
                .agent_ids()
                .into_iter()
                .map(str::to_string)
                .collect(),
        )?;
        let drivers = self.drivers(&records)?;
        // Close every descendant attempt a process interruption left Running.
        // Recovery never replays the external work; the Lead decides what to do
        // next from the durable state.
        for descendant in descendants(&board, root_task_id)? {
            board.recover_interrupted_attempt(descendant)?;
        }
        // Reopen the existing Lead attempt before an external turn restores
        // or persists its binding. A failed root otherwise remains Failed
        // while Codex Exec correctly requires a Running owner for that turn.
        // Root settlement has always reused attempt 1; retain its binding.
        let attempt = TaskAttempt {
            task_id: root_task_id,
            attempt: 1,
            agent_id: lead.id.clone(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        };
        if board
            .attempts(root_task_id)?
            .iter()
            .any(|row| row.attempt == 1)
        {
            board.complete_attempt(&attempt)?;
        } else {
            board.record_attempt(&attempt)?;
        }
        board.set_status(root_task_id, TaskStatus::Running)?;
        let scheduler = Scheduler::new(registry, drivers, board, self.options.max_retries)
            .with_sink(Arc::clone(&self.sink));
        let mut lead_loop = Lead::new(
            brain,
            scheduler,
            self.options.max_rounds,
            self.options.max_tasks,
            lead.id.clone(),
        );
        // Every interrupted descendant is closed and the Lead is about to
        // continue from the durable state, so the resumed run is observable.
        self.sink.emit(&RunEvent::RunResumed { root_task_id });
        self.drive(&mut lead_loop, root_task_id, &objective, &lead.id)
            .await
    }

    /// Drive the Lead loop on an already-created root and settle the run's
    /// outcome. A returned error never leaves the root observable as Running.
    async fn drive(
        &self,
        lead_loop: &mut Lead<SqliteTaskBoard>,
        root: u64,
        objective: &str,
        lead_id: &str,
    ) -> Result<TeamRunOutcome, TeamRunnerError> {
        match lead_loop.run_on_root(root, objective).await {
            Ok(result) => {
                // The Lead persisted the final answer, the exact refs, and the
                // root's succeeded status before returning.
                self.sink.emit(&RunEvent::RunCompleted {
                    root_task_id: root,
                    selected_task_ids: result.task_refs.clone(),
                    artifact_count: result.artifact_refs.len(),
                });
                Ok(TeamRunOutcome {
                    root_task_id: root,
                    lead_agent: lead_id.to_string(),
                    result,
                })
            }
            Err(error) => {
                let cause = lead_error_text(&error);
                let settled = self.settle_root_failed(root, lead_id, &cause);
                let failure = match settled {
                    Ok(()) => TeamRunnerError::Lead(error),
                    Err(settle) => TeamRunnerError::LeadAndSettleFailed {
                        cause: cause.clone(),
                        settle: settle.to_string(),
                    },
                };
                // Emitted only after the settle attempt has run, so a reported
                // failure never leaves the root observable as Running. This is
                // reached only from inside `drive`, where a root exists; a
                // pre-root configuration failure emits no run event at all.
                self.sink.emit(&RunEvent::RunFailed {
                    root_task_id: root,
                    error: bounded_event_text(&cause),
                });
                Err(failure)
            }
        }
    }

    /// Settle the root's Lead attempt as failed on a freshly reopened
    /// connection (the original board lives inside the scheduler). A root that
    /// is already succeeded is never rewritten.
    fn settle_root_failed(
        &self,
        root: u64,
        lead_id: &str,
        error: &str,
    ) -> Result<(), TeamRunnerError> {
        let mut board = self.open_board()?;
        if board.task(root)?.map(|record| record.status) == Some(TaskStatus::Succeeded) {
            return Ok(());
        }
        let attempt = TaskAttempt {
            task_id: root,
            attempt: 1,
            agent_id: lead_id.to_string(),
            status: TaskStatus::Failed,
            result: None,
            error: Some(bounded(error, 4096)),
        };
        if board.attempts(root)?.iter().any(|row| row.attempt == 1) {
            board.complete_attempt(&attempt)?;
        } else {
            board.record_attempt(&attempt)?;
        }
        board.set_status(root, TaskStatus::Failed)?;
        Ok(())
    }

    fn ensure_repo(&self) -> Result<(), TeamRunnerError> {
        if !self.repo.is_dir() {
            return Err(TeamRunnerError::RepoMissing(self.repo.clone()));
        }
        Ok(())
    }

    fn open_board(&self) -> Result<SqliteTaskBoard, TeamRunnerError> {
        let connection = Connection::open(&self.database)
            .map_err(|error| TeamRunnerError::RegistryStorage(error.to_string()))?;
        SqliteTaskBoard::open(connection)
            .map_err(|error| TeamRunnerError::RegistryStorage(error.to_string()))
    }

    fn records(&self) -> Result<Vec<AgentRegistryRecord>, TeamRunnerError> {
        let registry = SqliteAgentRegistry::open(&self.database)
            .map_err(|error| TeamRunnerError::RegistryStorage(error.to_string()))?;
        registry
            .list_agents()
            .map_err(|error| TeamRunnerError::RegistryStorage(error.to_string()))
    }

    fn drivers(
        &self,
        records: &[AgentRegistryRecord],
    ) -> Result<BTreeMap<String, Arc<dyn AgentDriver>>, TeamRunnerError> {
        let factory = DriverFactory::new(&self.database, &self.repo);
        let factory = match &self.bridge_host {
            Some(host) => factory.with_bridge_host(host.clone()),
            None => factory,
        };
        factory.build(records).map_err(TeamRunnerError::Driver)
    }
}

fn ensure_supported_lead_runtime(record: &AgentRegistryRecord) -> Result<(), TeamRunnerError> {
    let kind = record
        .driver_kind
        .as_deref()
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .unwrap_or("<missing>");
    match DriverKind::restore(kind) {
        Some(DriverKind::CodexAppServer | DriverKind::CodexExec) => Ok(()),
        _ => Err(TeamRunnerError::UnsupportedLeadRuntime {
            agent: record.id.clone(),
            kind: kind.to_string(),
        }),
    }
}

/// Build the Lead's effective configuration the way a run will. Starts no
/// process and opens no board.
///
/// This is the one construction path for a Lead's configuration:
/// [`TeamRunner::lead_brain`] builds its brain from exactly this value, so a
/// readiness verdict and a run can never disagree about the Lead's
/// configuration. The configuration is validated here too, because a
/// configuration a run would refuse is not one a run will use.
///
/// The typed form is what the run calls, so the run's error classes stay what
/// they were; [`validate_lead_config`] is the same verdict as one line of text.
fn lead_config(
    record: &AgentRegistryRecord,
    repo: &Path,
) -> Result<CodexLeadConfig, TeamRunnerError> {
    ensure_supported_lead_runtime(record)?;
    let options = parse_agent_options(&record.id, record.driver_config_json.as_deref())?;
    // Shared options are judged by the adapter's one validator before the
    // Lead-specific bounds, exactly as the matching team driver will judge
    // them later in construction.
    codex_option_values(&record.id, &options)?;
    let launch = launch_spec(record).map_err(|error| match error {
        DriverFactoryError::MissingExecutable(_) => {
            TeamRunnerError::MissingLeadExecutable(record.id.clone())
        }
        other => TeamRunnerError::Driver(other),
    })?;
    let config = CodexLeadConfig {
        launch,
        working_directory: repo.to_path_buf(),
        model: options.model.clone(),
        overrides: options.overrides.clone(),
        max_prompt_bytes: options
            .max_prompt_bytes
            .unwrap_or(DEFAULT_LEAD_MAX_PROMPT_BYTES),
        max_answer_bytes: options
            .max_answer_bytes
            .unwrap_or(DEFAULT_LEAD_MAX_ANSWER_BYTES),
        max_events: options.max_events.unwrap_or(DEFAULT_LEAD_MAX_EVENTS),
    };
    config
        .validate()
        .map_err(|detail| TeamRunnerError::LeadBrain(LeadBrainError::Unavailable(detail)))?;
    Ok(config)
}

/// The Lead's effective configuration, as the one-line verdict a readiness
/// check renders: `Ok` is exactly the configuration a run would build for
/// `record`, and `Err` is the message the run's own construction produces.
pub fn validate_lead_config(
    record: &AgentRegistryRecord,
    repo: &Path,
) -> Result<CodexLeadConfig, String> {
    lead_config(record, repo).map_err(|error| error.to_string())
}

/// Judge one registry row the way a run's registry construction does, without
/// spawning anything: the tier, the JSON string-array fields, and the
/// concurrency rule the registry enforces.
///
/// This reuses the run's own construction ([`agent_config`]) and the run's own
/// error type, so a readiness verdict and a run cannot drift apart in wording or
/// in which row they refuse.
pub fn validate_registry_row(record: &AgentRegistryRecord) -> Result<(), String> {
    let config = agent_config(record).map_err(|error| error.to_string())?;
    if config.max_concurrency == 0 {
        return Err(
            TeamRunnerError::Registry(RegistryError::ZeroConcurrency(record.id.clone()))
                .to_string(),
        );
    }
    Ok(())
}

/// Build the validated routing registry from the durable rows.
fn agent_registry(records: &[AgentRegistryRecord]) -> Result<AgentRegistry, TeamRunnerError> {
    let configs = records
        .iter()
        .map(agent_config)
        .collect::<Result<Vec<_>, _>>()?;
    AgentRegistry::new(configs).map_err(TeamRunnerError::Registry)
}

fn agent_config(record: &AgentRegistryRecord) -> Result<AgentConfig, TeamRunnerError> {
    let tier = match record.tier.as_str() {
        "reasoner" => AgentTier::Reasoner,
        "worker" => AgentTier::Worker,
        "utility" => AgentTier::Utility,
        other => {
            return Err(TeamRunnerError::UnsupportedTier {
                agent: record.id.clone(),
                tier: other.to_string(),
            })
        }
    };
    Ok(AgentConfig {
        id: record.id.clone(),
        name: record.name.clone(),
        tier,
        tags: json_string_array(&record.id, record.tags_json.as_deref())?,
        max_concurrency: match record.max_concurrency {
            None => 1,
            Some(value) => usize::try_from(value).unwrap_or(0),
        },
        driver_kind: record.driver_kind.as_deref().and_then(DriverKind::restore),
        executable: record.executable.clone(),
        driver_args: json_string_array(&record.id, record.driver_args_json.as_deref())?,
    })
}

fn json_string_array(agent: &str, raw: Option<&str>) -> Result<Vec<String>, TeamRunnerError> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Vec::new());
    };
    serde_json::from_str(raw).map_err(|error| TeamRunnerError::InvalidAgentField {
        agent: agent.to_string(),
        detail: format!("expected a JSON string array: {error}"),
    })
}

/// Resolve the run's Lead.
///
/// An explicit `--lead` must name a registered `reasoner`. Without one, exactly
/// one registered `reasoner` must exist: zero or several is an error, because an
/// ambiguous Lead must fail rather than be guessed.
fn resolve_lead(
    records: &[AgentRegistryRecord],
    requested: Option<&str>,
) -> Result<AgentRegistryRecord, TeamRunnerError> {
    if let Some(requested) = requested.map(str::trim).filter(|value| !value.is_empty()) {
        let found = records
            .iter()
            .find(|record| record.id == requested)
            .ok_or_else(|| {
                TeamRunnerError::LeadSelection(format!("agent `{requested}` is not registered"))
            })?;
        if found.tier != "reasoner" {
            return Err(TeamRunnerError::LeadSelection(format!(
                "agent `{requested}` is tier `{}`, and only a reasoner can lead",
                found.tier
            )));
        }
        return Ok(found.clone());
    }
    let reasoners: Vec<&AgentRegistryRecord> = records
        .iter()
        .filter(|record| record.tier == "reasoner")
        .collect();
    match reasoners.as_slice() {
        [] => Err(TeamRunnerError::LeadSelection(
            "no reasoner agent is registered; a team run needs exactly one lead".into(),
        )),
        [only] => Ok((*only).clone()),
        many => Err(TeamRunnerError::LeadSelection(format!(
            "{} reasoner agents are registered ({}); name the lead explicitly",
            many.len(),
            many.iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Every task whose `parent_task` chain reaches `root`, excluding the root.
fn descendants<B: TaskBoard>(board: &B, root: u64) -> Result<Vec<u64>, TeamRunnerError> {
    const MAX_HOPS: usize = 1024;
    let mut found = Vec::new();
    for id in board.task_ids()? {
        if id == root {
            continue;
        }
        let mut current = id;
        for _ in 0..MAX_HOPS {
            let Some(record) = board.task(current)? else {
                break;
            };
            match record.parent_task {
                Some(parent) if parent == root => {
                    found.push(id);
                    break;
                }
                Some(parent) => current = parent,
                None => break,
            }
        }
    }
    Ok(found)
}

/// Bound a diagnostic without splitting a UTF-8 character.
fn bounded(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// The Lead loop's failure as the text the durable failure row and the failure
/// event carry.
fn lead_error_text(error: &LeadError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, tier: &str) -> AgentRegistryRecord {
        AgentRegistryRecord {
            id: id.into(),
            name: id.into(),
            tier: tier.into(),
            driver_kind: Some("acp".into()),
            executable: Some("agent".into()),
            runtime_version: None,
            driver_args_json: Some("[]".into()),
            max_concurrency: Some(1),
            tags_json: None,
            driver_config_json: None,
        }
    }

    #[test]
    fn options_default_to_bounded_limits() {
        let options = TeamRunOptions::default();
        assert_eq!(options.lead_agent, None);
        assert_eq!(options.max_rounds, 8);
        assert_eq!(options.max_tasks, 32);
        assert_eq!(options.max_retries, 2);
    }

    #[test]
    fn a_single_reasoner_is_the_lead() {
        let records = [record("worker", "worker"), record("reasoner", "reasoner")];
        assert_eq!(
            resolve_lead(&records, None).unwrap().id,
            "reasoner".to_string()
        );
        assert_eq!(
            resolve_lead(&records, Some("reasoner")).unwrap().id,
            "reasoner".to_string()
        );
    }

    #[test]
    fn lead_ambiguity_and_absence_fail() {
        let two = [
            record("r-a", "reasoner"),
            record("r-b", "reasoner"),
            record("w", "worker"),
        ];
        let error = resolve_lead(&two, None).unwrap_err();
        assert!(
            error.to_string().contains("name the lead explicitly"),
            "{error}"
        );
        // An explicit lead resolves the ambiguity.
        assert_eq!(resolve_lead(&two, Some("r-b")).unwrap().id, "r-b");
        let none = [record("w", "worker"), record("u", "utility")];
        assert!(resolve_lead(&none, None).is_err());
        // An explicit lead must exist and must be a reasoner.
        assert!(resolve_lead(&two, Some("ghost")).is_err());
        assert!(resolve_lead(&two, Some("w")).is_err());
    }

    #[test]
    fn registry_rows_map_to_routable_agents() {
        let mut reasoner = record("lead", "reasoner");
        reasoner.max_concurrency = None;
        reasoner.driver_kind = Some("codex-app-server".into());
        reasoner.tags_json = Some(r#"["codex"]"#.into());
        let registry =
            agent_registry(&[reasoner, record("worker", "worker"), record("u", "utility")])
                .unwrap();
        assert_eq!(registry.agent_ids(), vec!["lead", "u", "worker"]);
        assert_eq!(registry.get("lead").unwrap().max_concurrency, 1);
        assert_eq!(
            registry.get("lead").unwrap().driver_kind,
            Some(DriverKind::CodexAppServer)
        );
        assert_eq!(registry.get("lead").unwrap().tags, vec!["codex"]);
        let error = agent_registry(&[record("ghost", "wizard")]).unwrap_err();
        assert!(matches!(
            error,
            TeamRunnerError::UnsupportedTier { ref tier, .. } if tier == "wizard"
        ));
    }

    /// The Lead's effective configuration is built by the one function a run
    /// calls, so a readiness verdict is the run's own verdict.
    #[test]
    fn a_lead_configuration_is_built_and_judged_the_way_a_run_does() {
        let repo = std::env::temp_dir();
        let lead_record = || AgentRegistryRecord {
            driver_kind: Some("codex-app-server".into()),
            ..record("lead", "reasoner")
        };
        let good = AgentRegistryRecord {
            driver_config_json: Some(
                r#"{"model":"gpt","overrides":["x=1"],"max_prompt_bytes":4096,"max_answer_bytes":2048,"max_events":4000}"#
                    .into(),
            ),
            ..lead_record()
        };
        let config = validate_lead_config(&good, &repo).unwrap();
        assert_eq!(config.model.as_deref(), Some("gpt"));
        assert_eq!(config.overrides, vec!["x=1"]);
        assert_eq!(config.max_prompt_bytes, 4096);
        assert_eq!(config.max_answer_bytes, 2048);
        assert_eq!(config.max_events, 4000);
        assert_eq!(config.working_directory, repo);
        assert_eq!(config.launch.program, std::path::Path::new("agent"));

        // An absent body is the documented defaults, exactly as a run reads it.
        let defaults = validate_lead_config(&lead_record(), &repo).unwrap();
        assert_eq!(defaults.model, None);
        assert!(defaults.overrides.is_empty());
        assert_eq!(defaults.max_prompt_bytes, DEFAULT_LEAD_MAX_PROMPT_BYTES);
        assert_eq!(defaults.max_answer_bytes, DEFAULT_LEAD_MAX_ANSWER_BYTES);
        assert_eq!(defaults.max_events, DEFAULT_LEAD_MAX_EVENTS);

        for (config, expected) in [
            ("not json", "is not valid JSON"),
            ("[1,2]", "must be a JSON object"),
            (r#"{"api_key":"x"}"#, "looks like a credential"),
            (
                r#"{"max_events":"not-a-number"}"#,
                "`max_events` must be a number",
            ),
            (
                r#"{"max_events":0}"#,
                "max_events must be greater than zero",
            ),
            (r#"{"max_prompt_bytes":16}"#, "at least 1024"),
            (
                r#"{"max_answer_bytes":0}"#,
                "max_answer_bytes must be positive",
            ),
            (r#"{"model":""}"#, "model must be non-empty"),
        ] {
            let row = AgentRegistryRecord {
                driver_config_json: Some(config.into()),
                ..lead_record()
            };
            let detail = validate_lead_config(&row, &repo).unwrap_err();
            assert!(detail.contains(expected), "{config}: {detail}");
        }

        // No executable: the run's own launch error, verbatim.
        let no_program = AgentRegistryRecord {
            executable: None,
            ..lead_record()
        };
        assert!(validate_lead_config(&no_program, &repo)
            .unwrap_err()
            .contains("has no executable"));
        // A repository that is not a directory is not one a run works in.
        let missing = repo.join("agentmosaic-lead-config-must-not-exist");
        assert!(validate_lead_config(&lead_record(), &missing)
            .unwrap_err()
            .contains("working directory must exist"));
    }

    #[test]
    fn the_default_factory_dispatches_by_runtime_and_rejects_unsupported_leads() {
        let repo = std::env::temp_dir();
        let factory = DefaultLeadBrainFactory::new(&repo);

        let unsupported = record("lead", "reasoner");
        let error = match factory.build(&unsupported, vec!["worker".into()]) {
            Ok(_) => panic!("ACP is not a conforming Lead runtime"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            TeamRunnerError::UnsupportedLeadRuntime {
                ref agent,
                ref kind
            } if agent == "lead" && kind == "acp"
        ));

        let missing = AgentRegistryRecord {
            driver_kind: None,
            ..record("lead", "reasoner")
        };
        let error = validate_lead_config(&missing, &repo).unwrap_err();
        assert!(error.contains("runtime `<missing>`"), "{error}");
    }

    /// One registry row is judged by the run's own construction, so the verdict
    /// and the wording come from the code a run calls rather than a copy of it.
    #[test]
    fn a_registry_row_is_judged_the_way_a_run_builds_it() {
        assert_eq!(validate_registry_row(&record("worker", "worker")), Ok(()));
        // An absent concurrency defaults to 1, which the registry accepts.
        let defaulted = AgentRegistryRecord {
            max_concurrency: None,
            ..record("worker", "worker")
        };
        assert_eq!(validate_registry_row(&defaulted), Ok(()));

        // An unknown tier is the run's own construction error, verbatim.
        let unknown_tier = record("ghost", "wizard");
        assert_eq!(
            validate_registry_row(&unknown_tier).unwrap_err(),
            agent_config(&unknown_tier).unwrap_err().to_string()
        );

        // A malformed JSON string-array field, likewise.
        let malformed = AgentRegistryRecord {
            tags_json: Some("not json".into()),
            ..record("worker", "worker")
        };
        let detail = validate_registry_row(&malformed).unwrap_err();
        assert_eq!(detail, agent_config(&malformed).unwrap_err().to_string());
        assert!(detail.contains("expected a JSON string array"), "{detail}");

        // Zero concurrency is refused by the registry, and the message is the
        // run's own error type formatted, so doctor and run are identical by
        // construction.
        let zero = AgentRegistryRecord {
            max_concurrency: Some(0),
            ..record("worker", "worker")
        };
        let error = validate_registry_row(&zero).unwrap_err();
        assert_eq!(
            error,
            TeamRunnerError::Registry(RegistryError::ZeroConcurrency("worker".into())).to_string()
        );
        assert!(error.contains("max-concurrency of zero"), "{error}");
    }
}
