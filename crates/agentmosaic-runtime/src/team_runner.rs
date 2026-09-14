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

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry, SqliteTaskBoard};
use agentmosaic_team::{
    reconstruct_team_result, AgentConfig, AgentDriver, AgentRegistry, AgentTier, BoardError,
    DriverKind, Lead, LeadBrainError, LeadError, RegistryError, Scheduler, TaskAttempt, TaskBoard,
    TaskKind, TaskStatus, TeamResult,
};
use rusqlite::Connection;

use crate::driver_factory::{launch_spec, parse_agent_options, DriverFactory, DriverFactoryError};
use crate::{CodexLeadBrain, CodexLeadConfig, LaunchSpec};

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
                other => write!(f, "the agent registry is not runnable: {other:?}"),
            },
            Self::LeadSelection(detail) => write!(f, "the team lead could not be resolved: {detail}"),
            Self::MissingLeadExecutable(agent) => {
                write!(f, "lead agent `{agent}` has no executable")
            }
            Self::Driver(error) => write!(f, "{error}"),
            Self::LeadBrain(error) => write!(f, "{error}"),
            Self::Lead(error) => write!(f, "the lead run failed: {error:?}"),
            Self::Board(error) => write!(f, "a task board operation failed: {error:?}"),
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
}

impl TeamRunner {
    pub fn new(
        database: impl Into<PathBuf>,
        repo: impl Into<PathBuf>,
        options: TeamRunOptions,
    ) -> Self {
        Self {
            database: database.into(),
            repo: repo.into(),
            options,
            bridge_host: None,
        }
    }

    pub fn with_bridge_host(mut self, host: LaunchSpec) -> Self {
        self.bridge_host = Some(host);
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
        let drivers = self.drivers(&records)?;
        // The brain is built before the board is touched: constructing it starts
        // no process, and a configuration error must never leave a root behind.
        let brain = self.lead_brain(&lead, registry.agent_ids())?;
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

        let scheduler = Scheduler::new(registry, drivers, board, self.options.max_retries);
        let mut lead_loop = Lead::new(
            Box::new(brain),
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
        let drivers = self.drivers(&records)?;
        let brain = self.lead_brain(&lead, registry.agent_ids())?;
        // Close every descendant attempt a process interruption left Running.
        // Recovery never replays the external work; the Lead decides what to do
        // next from the durable state.
        for descendant in descendants(&board, root_task_id)? {
            board.recover_interrupted_attempt(descendant)?;
        }
        let scheduler = Scheduler::new(registry, drivers, board, self.options.max_retries);
        let mut lead_loop = Lead::new(
            Box::new(brain),
            scheduler,
            self.options.max_rounds,
            self.options.max_tasks,
            lead.id.clone(),
        );
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
            Ok(result) => Ok(TeamRunOutcome {
                root_task_id: root,
                lead_agent: lead_id.to_string(),
                result,
            }),
            Err(error) => match self.settle_root_failed(root, lead_id, &lead_error_text(&error)) {
                Ok(()) => Err(TeamRunnerError::Lead(error)),
                Err(settle) => Err(TeamRunnerError::LeadAndSettleFailed {
                    cause: lead_error_text(&error),
                    settle: settle.to_string(),
                }),
            },
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

    /// The resident Lead brain for `lead`, configured from the Lead's own
    /// non-secret driver config. The Lead never runs a shell command and never
    /// edits the workspace, so it only needs the app-server command, the model,
    /// and the byte/event bounds.
    fn lead_brain(
        &self,
        lead: &AgentRegistryRecord,
        candidates: Vec<&str>,
    ) -> Result<CodexLeadBrain, TeamRunnerError> {
        let options = parse_agent_options(&lead.id, lead.driver_config_json.as_deref())?;
        let launch = launch_spec(lead).map_err(|error| match error {
            DriverFactoryError::MissingExecutable(_) => {
                TeamRunnerError::MissingLeadExecutable(lead.id.clone())
            }
            other => TeamRunnerError::Driver(other),
        })?;
        let config = CodexLeadConfig {
            launch,
            working_directory: self.repo.clone(),
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
        Ok(CodexLeadBrain::new(
            config,
            candidates.into_iter().map(str::to_string).collect(),
        )?)
    }
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

/// The Lead loop's error is a typed enum without a `Display`; render it for the
/// durable failure row and the diagnostic message.
fn lead_error_text(error: &LeadError) -> String {
    format!("{error:?}")
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
}
