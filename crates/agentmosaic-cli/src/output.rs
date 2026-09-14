//! Presentation helpers: the bounded, human-readable renderings the CLI
//! prints. No command logic lives here.

use agentmosaic_runtime::TeamRunOutcome;
use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry, SqliteTaskBoard};
use agentmosaic_team::{TaskBoard, TaskRecord};

/// The longest objective a run rendering prints; longer text is cut on a
/// character boundary and marked.
const MAX_OBJECTIVE_BYTES: usize = 72;

/// The longest argv rendering a human surface prints. A registered launch argv
/// is opaque and unbounded, so the arguments are cut on a character boundary
/// and marked; the launch program itself is always shown whole. `codex -qw`
/// fits whole and stays distinguishable, and a wall of argv never reaches the
/// terminal.
const MAX_LAUNCH_BYTES: usize = 48;

/// The longest configuration detail one Agent's report line carries. The
/// `Reason` section below it always carries the detail whole.
const MAX_AGENT_DETAIL_BYTES: usize = 96;

/// Argument spellings that make an argv credential-looking. A durable,
/// shareable launch rendering never echoes one; the CLI has no business
/// printing a token it does not own.
const CREDENTIAL_MARKERS: [&str; 8] = [
    "token",
    "secret",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "bearer",
    "authorization",
];

/// The marker printed in place of a credential-looking argument.
const REDACTED: &str = "<redacted>";

/// The bounded, human-readable summary of one team run.
pub fn render_team_outcome(outcome: &TeamRunOutcome) -> String {
    let task_refs = if outcome.result.task_refs.is_empty() {
        "-".to_string()
    } else {
        outcome
            .result
            .task_refs
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut lines = vec![
        format!("root={} lead={}", outcome.root_task_id, outcome.lead_agent),
        format!("answer: {}", outcome.result.answer),
        format!("task_refs: {task_refs}"),
    ];
    if outcome.result.artifact_refs.is_empty() {
        lines.push("artifact_refs: -".into());
    } else {
        for selected in &outcome.result.artifact_refs {
            lines.push(format!(
                "artifact_refs: task={} path={} sha256={}",
                selected.task_id, selected.artifact.path, selected.artifact.sha256
            ));
        }
    }
    lines.join("\n")
}

/// One line per task: the durable board as the operator sees it.
///
/// This is the legacy `status <database>` renderer, kept byte-for-byte: every
/// line is a `task=...` record and nothing else is printed.
pub fn render_status(board: &SqliteTaskBoard) -> Result<String, String> {
    board
        .task_ids()
        .map_err(|e| format!("status: {e:?}"))?
        .into_iter()
        .map(|id| {
            let task = board
                .task(id)
                .map_err(|e| format!("status: {e:?}"))?
                .ok_or_else(|| format!("status: missing task {id}"))?;
            let attempts = board
                .attempts(task.id)
                .map_err(|e| format!("status: {e:?}"))?;
            Ok(format!(
                "task={} status={} assignee={} attempts={} parent={} objective={}",
                task.id,
                task.status.as_str(),
                task.assignee.as_deref().unwrap_or("-"),
                attempts.len(),
                task.parent_task
                    .map(|parent| parent.to_string())
                    .unwrap_or_else(|| "-".into()),
                task.objective
            ))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|lines| lines.join("\n"))
}

/// One run, as the operator sees it: the root task, the tasks below it, and
/// the artifacts recorded anywhere in that subtree.
///
/// Only board data is printed, never a storage path.
pub fn render_run_status(board: &SqliteTaskBoard, run: &TaskRecord) -> Result<String, String> {
    let ids = subtree_ids(board, run)?;
    let mut lines = vec![
        format!("run #{}  {}", run.id, run.status.as_str()),
        format!("objective  {}", bounded_objective(&run.objective)),
        format!("lead       {}", run.assignee.as_deref().unwrap_or("-")),
        String::new(),
        "tasks".to_string(),
    ];
    for id in &ids {
        let task = task_at(board, *id)?;
        lines.push(format!(
            "  #{}  {}  {}  {}",
            task.id,
            task.assignee.as_deref().unwrap_or("-"),
            task.status.as_str(),
            bounded_objective(&task.objective)
        ));
    }
    lines.push(String::new());
    lines.push("artifacts".to_string());
    for id in &ids {
        for artifact in board.artifacts(*id).map_err(|e| format!("{e:?}"))? {
            lines.push(format!("  #{}  {}", id, artifact.path));
        }
    }
    Ok(lines.join("\n"))
}

/// One concise line per run, newest first.
pub fn render_run_list(board: &SqliteTaskBoard) -> Result<String, String> {
    let mut runs = board.root_tasks().map_err(|e| format!("{e:?}"))?;
    runs.reverse();
    Ok(runs
        .iter()
        .map(|run| {
            format!(
                "run #{}  {}  {}",
                run.id,
                run.status.as_str(),
                bounded_objective(&run.objective)
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// The root task and every task below it, in id order.
fn subtree_ids(board: &SqliteTaskBoard, run: &TaskRecord) -> Result<Vec<u64>, String> {
    let mut ids = vec![run.id];
    ids.extend(board.descendants_of(run.id).map_err(|e| format!("{e:?}"))?);
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

fn task_at(board: &SqliteTaskBoard, id: u64) -> Result<TaskRecord, String> {
    board
        .task(id)
        .map_err(|e| format!("{e:?}"))?
        .ok_or_else(|| format!("missing task {id}"))
}

/// A single-line, bounded objective.
fn bounded_objective(objective: &str) -> String {
    bounded(
        &objective.split_whitespace().collect::<Vec<_>>().join(" "),
        MAX_OBJECTIVE_BYTES,
    )
}

/// One line of text, cut to `max` bytes on a character boundary and marked.
fn bounded(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

pub fn registry_list(database: &str, limit: Option<&str>) -> Result<String, String> {
    let cap = limit
        .map(|value| {
            value
                .parse()
                .map_err(|_| "invalid registry limit".to_string())
        })
        .transpose()?
        .unwrap_or(usize::MAX);
    let registry = SqliteAgentRegistry::open(database).map_err(|e| format!("registry: {e}"))?;
    let lines =
        registry
            .list_agents()
            .map_err(|e| format!("registry: {e}"))?
            .into_iter()
            .take(cap)
            .map(|agent| {
                format!(
                "id={} name={} tier={} driver_kind={} executable={} version={} args={} concurrency={} tags={} driver_config={}",
                agent.id,
                agent.name,
                agent.tier,
                agent.driver_kind.as_deref().unwrap_or("-"),
                agent.executable.as_deref().unwrap_or("-"),
                agent.runtime_version.as_deref().unwrap_or("-"),
                agent.driver_args_json.as_deref().unwrap_or("-"),
                agent.max_concurrency.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                agent.tags_json.as_deref().unwrap_or("-"),
                agent
                    .driver_config_json
                    .as_deref()
                    .map(bounded_config_note)
                    .unwrap_or_else(|| "-".into()),
            )
            })
            .collect::<Vec<_>>()
            .join("\n");
    Ok(lines)
}

/// A short rendering of a driver config for the list surface. A long body is
/// never printed whole; only its bounded head and its size are shown.
pub fn bounded_config_note(raw: &str) -> String {
    const MAX: usize = 80;
    if raw.len() <= MAX {
        return raw.to_string();
    }
    let mut end = MAX;
    while end > 0 && !raw.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... ({} bytes)", &raw[..end], raw.len())
}

/// The bounded launch rendering of one registered Agent: its launch program,
/// then its registered argv.
///
/// The program is shown whole; the arguments are capped at [`MAX_LAUNCH_BYTES`]
/// on a character boundary and marked. A credential-looking argument is
/// replaced before anything is cut, so the rendering can never echo one. An
/// unparsable argv is not printed at all: the durable row is the authority, and
/// no surface echoes raw JSON.
pub fn bounded_launch(agent: &AgentRegistryRecord) -> String {
    let mut rendered = agent.executable.as_deref().unwrap_or("-").to_string();
    if let Some(raw) = agent.driver_args_json.as_deref() {
        if let Ok(args) = serde_json::from_str::<Vec<String>>(raw) {
            let argv = args
                .iter()
                .map(|arg| redact_argument(arg))
                .collect::<Vec<_>>()
                .join(" ");
            if !argv.is_empty() {
                rendered.push(' ');
                rendered.push_str(&bounded(&argv, MAX_LAUNCH_BYTES));
            }
        }
    }
    rendered
}

/// One argument, or the redaction marker when it looks like a credential.
fn redact_argument(argument: &str) -> String {
    let lowered = argument.to_ascii_lowercase();
    if CREDENTIAL_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        REDACTED.to_string()
    } else {
        argument.to_string()
    }
}

/// `am agent list`: role-first, easy to scan, and never a runtime probe.
///
/// Rows are grouped by role — reasoner, worker, utility — because whether a
/// project can run is a role question before it is an identity one, and ids
/// order the rows inside a role. Only durable registry rows are read: listing
/// an Agent whose runtime is not installed must still succeed.
pub fn render_agent_table(agents: &[AgentRegistryRecord]) -> String {
    let mut rows = agents
        .iter()
        .map(|agent| {
            [
                agent.id.clone(),
                agent.tier.clone(),
                agent.driver_kind.clone().unwrap_or_else(|| "-".into()),
                bounded_launch(agent),
            ]
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        role_rank(&left[1])
            .cmp(&role_rank(&right[1]))
            .then_with(|| left[0].cmp(&right[0]))
    });
    let id_width = column_width("ID", 8, rows.iter().map(|row| row[0].as_str()));
    let role_width = column_width("ROLE", 10, rows.iter().map(|row| row[1].as_str()));
    let adapter_width = column_width("ADAPTER", 20, rows.iter().map(|row| row[2].as_str()));
    let mut lines = vec![format!(
        "{:<id_width$}{:<role_width$}{:<adapter_width$}LAUNCH",
        "ID", "ROLE", "ADAPTER"
    )];
    lines.extend(rows.iter().map(|row| {
        format!(
            "{:<id_width$}{:<role_width$}{:<adapter_width$}{}",
            row[0], row[1], row[2], row[3]
        )
    }));
    if rows.is_empty() {
        lines.push(String::new());
        lines.push("(no agents registered; add one with `am agent add`)".into());
    }
    lines.join("\n")
}

/// The width of one table column: the configured minimum, or the widest cell
/// plus a two-space gap.
fn column_width<'a>(header: &str, minimum: usize, cells: impl Iterator<Item = &'a str>) -> usize {
    cells
        .map(|cell| cell.chars().count() + 2)
        .fold(minimum.max(header.chars().count() + 2), usize::max)
}

/// Role-first ordering: the Lead, then the Workers, then the optional
/// utility Agents.
fn role_rank(role: &str) -> usize {
    match role {
        "reasoner" => 0,
        "worker" => 1,
        "utility" => 2,
        _ => 3,
    }
}

/// `am agent add`: what happened to the row, the three fields an operator
/// checks, and the next step.
pub fn render_agent_registration(
    id: &str,
    created: bool,
    role: &str,
    adapter: &str,
    launch: &str,
) -> String {
    let verb = if created { "registered" } else { "updated" };
    [
        format!("{verb} agent `{id}`"),
        format!("role     {role}"),
        format!("adapter  {adapter}"),
        format!("launch   {launch}"),
        String::new(),
        "Next: am doctor".to_string(),
    ]
    .join("\n")
}

/// `am agent remove`: the confirmation, and — when the removal leaves the team
/// unable to run — the decision the operator has to make next.
pub fn render_agent_removed(id: &str, team_runnable: bool) -> String {
    let mut lines = vec![format!("removed agent `{id}`")];
    if !team_runnable {
        lines.push(String::new());
        lines.push("team is no longer runnable".into());
        lines.push("  am doctor will fail until another required Agent is added".into());
    }
    lines.join("\n")
}

/// The bounded readiness classification of one registered Agent's runtime.
///
/// These are the only runtime facts `am doctor` reports. A classification is
/// produced without a prompt, a model, a login or a credential: the CLI proves
/// that a configured runtime starts, and — where the driver can do it safely —
/// that it answers a handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessStage {
    Ready,
    ProgramMissing,
    LaunchSpecInvalid,
    /// The configuration a run would read from this Agent's registry row is not
    /// one a run would accept.
    ConfigInvalid,
    SpawnFailed,
    ProtocolUnavailable,
    RuntimePreparationRequired,
}

impl ReadinessStage {
    /// Whether the runtime answered its bounded readiness check.
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// The classification as one stable machine token. The human surface keeps
    /// its own wording; a consumer keys on this.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::ProgramMissing => "program_missing",
            Self::LaunchSpecInvalid => "launch_spec_invalid",
            Self::ConfigInvalid => "config_invalid",
            Self::SpawnFailed => "spawn_failed",
            Self::ProtocolUnavailable => "protocol_unavailable",
            Self::RuntimePreparationRequired => "runtime_preparation_required",
        }
    }
}

/// One Agent's line in the doctor report: what is registered, and whether its
/// runtime answered.
#[derive(Debug, Clone)]
pub struct DoctorAgent {
    pub id: String,
    pub role: String,
    /// The launch program as registered, for remediation text.
    pub program: String,
    /// The bounded launch rendering, as `am agent list` prints it.
    pub launch: String,
    pub stage: ReadinessStage,
    /// The bounded diagnostic stages, printed only by `am doctor --verbose`.
    pub stages: String,
    /// The validator's own one-line detail when the configuration is not one a
    /// run would accept. It is rendered on the Agent's line and, whole, as the
    /// reason.
    pub detail: Option<String>,
}

/// Everything `am doctor` decided: the project, its registered Agents, and the
/// team rule the product enforces.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub project_root: String,
    pub schema_version: i32,
    pub agents: Vec<DoctorAgent>,
}

impl DoctorReport {
    /// The registered Agents per role: reasoner, worker, utility.
    pub fn tiers(&self) -> [usize; 3] {
        let mut tiers = [0usize; 3];
        for agent in &self.agents {
            match agent.role.as_str() {
                "reasoner" => tiers[0] += 1,
                "worker" => tiers[1] += 1,
                "utility" => tiers[2] += 1,
                _ => {}
            }
        }
        tiers
    }

    /// The team rule `am run` enforces: exactly one Lead, at least one Worker.
    /// Utility Agents are optional, so zero of them is a valid team.
    pub fn composition_ready(&self) -> bool {
        let tiers = self.tiers();
        tiers[0] == 1 && tiers[1] >= 1
    }

    /// Every registered runtime answered its bounded readiness check.
    pub fn runtimes_ready(&self) -> bool {
        self.agents.iter().all(|agent| agent.stage.is_ready())
    }

    /// The decision: can this project run?
    pub fn ready(&self) -> bool {
        self.composition_ready() && self.runtimes_ready()
    }

    /// The report as a human reads it.
    ///
    /// The default surface is the decision and, when it is not ready, the
    /// `Reason` and `Fix` that follow from it. `--verbose` adds the bounded
    /// stage codes and the project and schema lines.
    pub fn render(&self, verbose: bool) -> String {
        let width = self.label_width();
        let tiers = self.tiers();
        let mut lines = Vec::new();
        if verbose {
            lines.push(format!("{:<width$}ready  {}", "project", self.project_root));
            lines.push(format!(
                "{:<width$}ready  schema={}",
                "state", self.schema_version
            ));
        } else {
            lines.push(format!("{:<width$}ready", "project"));
        }
        for agent in &self.agents {
            let state = if agent.stage.is_ready() {
                "ready"
            } else {
                "not ready"
            };
            // A configuration failure is the Agent's own fact, so its line
            // carries it: the decision names the Agent that has to be fixed.
            let detail = agent
                .detail
                .as_deref()
                .map(|detail| format!("  {}", bounded(detail, MAX_AGENT_DETAIL_BYTES)))
                .unwrap_or_default();
            lines.push(if verbose {
                format!(
                    "{:<width$}{state}  {}  {}{detail}",
                    agent.id, agent.launch, agent.stages
                )
            } else {
                format!("{:<width$}{state}  {}{detail}", agent.id, agent.launch)
            });
        }
        lines.push(format!(
            "{:<width$}{}  {} lead · {} worker",
            "team",
            if self.ready() { "ready" } else { "not ready" },
            tiers[0],
            tiers[1],
        ));
        match self.remediation() {
            Some(remediation) => {
                lines.push(String::new());
                lines.push("Reason".into());
                lines.push(format!("  {}", remediation.reason));
                lines.push(String::new());
                lines.push("Fix".into());
                lines.extend(remediation.fix.iter().map(|line| format!("  {line}")));
            }
            None => {
                lines.push(String::new());
                lines.push("Ready to run.".into());
                lines.push("  am run \"<objective>\"".into());
            }
        }
        lines.join("\n")
    }

    /// Every label column fits the fixed words and the longest Agent id.
    fn label_width(&self) -> usize {
        self.agents
            .iter()
            .map(|agent| agent.id.chars().count() + 2)
            .fold(10, usize::max)
    }

    /// The one decision the operator has to make next, or None when the team
    /// is ready. Composition comes first: an incomplete team has to be fixed
    /// before a runtime detail matters.
    fn remediation(&self) -> Option<Remediation> {
        let tiers = self.tiers();
        if tiers[0] != 1 {
            return Some(lead_remediation(tiers[0]));
        }
        if tiers[1] == 0 {
            return Some(Remediation {
                reason: "the team has no worker; `am run` needs at least one worker Agent".into(),
                fix: vec!["add a worker Agent, then run:".into(), "am doctor".into()],
            });
        }
        self.agents.iter().find_map(DoctorAgent::remediation)
    }

    /// [`DoctorReport::remediation`] as data: the reason and its fix lines, for
    /// the machine surface. `None` means the team is ready.
    pub fn decision(&self) -> Option<(String, Vec<String>)> {
        self.remediation()
            .map(|remediation| (remediation.reason, remediation.fix))
    }
}

impl DoctorAgent {
    /// The concise remediation for this Agent, or None when it is ready.
    fn remediation(&self) -> Option<Remediation> {
        let role = self.role.as_str();
        let remediation = match self.stage {
            ReadinessStage::Ready => return None,
            ReadinessStage::ProgramMissing => Remediation {
                reason: format!("program `{}` was not found on PATH", self.program),
                fix: recheck(format!(
                    "install or configure the {role} runtime, then run:"
                )),
            },
            ReadinessStage::LaunchSpecInvalid => Remediation {
                reason: format!("the launch specification of `{}` is not valid", self.id),
                fix: recheck(
                    "re-register the Agent with a launch program and a JSON argv, then run:"
                        .to_string(),
                ),
            },
            ReadinessStage::ConfigInvalid => Remediation {
                reason: match self.detail.as_deref() {
                    Some(detail) => detail.to_string(),
                    None => format!("the configuration of `{}` is not valid", self.id),
                },
                fix: recheck(format!(
                    "fix the driver configuration of `{id}` or re-register the Agent, then run:",
                    id = self.id
                )),
            },
            ReadinessStage::SpawnFailed => Remediation {
                reason: format!("the {role} runtime `{}` could not be started", self.program),
                fix: recheck(format!("check the {role} runtime configuration, then run:")),
            },
            ReadinessStage::ProtocolUnavailable => Remediation {
                reason: format!(
                    "the {role} runtime `{}` did not answer the readiness handshake",
                    self.program
                ),
                fix: recheck(format!("check the {role} runtime configuration, then run:")),
            },
            ReadinessStage::RuntimePreparationRequired => Remediation {
                reason: format!(
                    "the {role} runtime `{}` needs local preparation or authentication",
                    self.program
                ),
                fix: recheck(format!(
                    "prepare the {role} runtime through your own provider configuration, then run:"
                )),
            },
        };
        Some(remediation)
    }
}

/// The decision a not-ready report hands back: why, and what to do about it.
struct Remediation {
    reason: String,
    fix: Vec<String>,
}

/// The remediation tail every runtime class shares: what to do, and the command
/// that re-checks it.
fn recheck(instruction: String) -> Vec<String> {
    vec![instruction, "am doctor".to_string()]
}

/// The Lead is the team's single reasoner; anything else has to be fixed first.
fn lead_remediation(reasoners: usize) -> Remediation {
    if reasoners == 0 {
        return Remediation {
            reason: "the team has no lead; `am run` needs exactly one reasoner Agent".into(),
            fix: recheck("add a lead reasoner Agent, then run:".to_string()),
        };
    }
    Remediation {
        reason: format!("the team has {reasoners} reasoner Agents; `am run` needs exactly one"),
        fix: recheck("leave exactly one reasoner registered, then run:".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str, role: &str, program: &str, stage: ReadinessStage) -> DoctorAgent {
        DoctorAgent {
            id: id.into(),
            role: role.into(),
            program: program.into(),
            launch: program.into(),
            stage,
            stages: format!("PROGRAM_FOUND LAUNCHSPEC_VALID {}", stage_code(stage)),
            detail: None,
        }
    }

    fn stage_code(stage: ReadinessStage) -> &'static str {
        match stage {
            ReadinessStage::Ready => "SPAWN_OK PROTOCOL_OK SESSION_OK READY",
            ReadinessStage::ProgramMissing => "PROGRAM_NOT_FOUND",
            ReadinessStage::LaunchSpecInvalid => "LAUNCHSPEC_INVALID",
            ReadinessStage::ConfigInvalid => "CONFIG_INVALID",
            ReadinessStage::SpawnFailed => "SPAWN_FAILED",
            ReadinessStage::ProtocolUnavailable => "PROTOCOL_UNAVAILABLE",
            ReadinessStage::RuntimePreparationRequired => "RUNTIME_PREPARATION_REQUIRED",
        }
    }

    fn report(agents: Vec<DoctorAgent>) -> DoctorReport {
        DoctorReport {
            project_root: "/tmp/project".into(),
            schema_version: 11,
            agents,
        }
    }

    fn ready_pair() -> Vec<DoctorAgent> {
        vec![
            agent("lead", "reasoner", "codex", ReadinessStage::Ready),
            agent("worker", "worker", "qwen", ReadinessStage::Ready),
        ]
    }

    fn record(
        id: &str,
        role: &str,
        adapter: &str,
        program: &str,
        args: Option<&str>,
    ) -> AgentRegistryRecord {
        AgentRegistryRecord {
            id: id.into(),
            name: id.into(),
            tier: role.into(),
            driver_kind: Some(adapter.into()),
            executable: Some(program.into()),
            driver_args_json: args.map(str::to_string),
            max_concurrency: Some(1),
            tags_json: None,
            runtime_version: None,
            driver_config_json: None,
        }
    }

    /// A ready team is ready without any utility Agent: utility is optional,
    /// and zero of them is a valid team.
    #[test]
    fn doctor_is_ready_without_any_utility_agent() {
        let report = report(ready_pair());
        assert_eq!(report.tiers(), [1, 1, 0]);
        assert!(report.ready());
        let text = report.render(false);
        assert!(text.contains("project   ready"), "{text}");
        assert!(text.contains("lead      ready  codex"), "{text}");
        assert!(text.contains("worker    ready  qwen"), "{text}");
        assert!(
            text.contains("team      ready  1 lead · 1 worker"),
            "{text}"
        );
        assert!(text.contains("Ready to run."), "{text}");
        assert!(text.contains("am run \"<objective>\""), "{text}");
        assert!(!text.contains("Reason"), "{text}");
        assert!(!text.contains("PROGRAM_FOUND"), "{text}");
    }

    /// A registered utility Agent whose runtime is ready keeps the team ready.
    #[test]
    fn doctor_is_ready_with_a_utility_agent() {
        let mut agents = ready_pair();
        agents.push(agent("utility", "utility", "qwen", ReadinessStage::Ready));
        let report = report(agents);
        assert_eq!(report.tiers(), [1, 1, 1]);
        assert!(report.ready());
    }

    /// `utility` is optional in the team's composition, not in its readiness:
    /// zero utility Agents is a valid team, but a registered utility runtime
    /// that does not answer is still a real problem.
    #[test]
    fn doctor_reports_an_unready_utility_runtime_as_not_ready() {
        let mut agents = ready_pair();
        agents.push(agent(
            "utility",
            "utility",
            "qwen",
            ReadinessStage::SpawnFailed,
        ));
        let report = report(agents);
        assert!(report.composition_ready());
        assert!(!report.ready());
        let text = report.render(false);
        assert!(text.contains("utility   not ready  qwen"), "{text}");
        assert!(text.contains("could not be started"), "{text}");
    }

    /// Every not-ready class renders the same decision shape: the reason, and
    /// the fix — never a provider-specific login command.
    #[test]
    fn doctor_reports_each_not_ready_class_with_a_reason_and_a_fix() {
        let cases = [
            (
                ReadinessStage::ProgramMissing,
                "program `qwen` was not found on PATH",
            ),
            (
                ReadinessStage::LaunchSpecInvalid,
                "the launch specification of `worker` is not valid",
            ),
            (
                ReadinessStage::SpawnFailed,
                "the worker runtime `qwen` could not be started",
            ),
            (
                ReadinessStage::ProtocolUnavailable,
                "the worker runtime `qwen` did not answer the readiness handshake",
            ),
            (
                ReadinessStage::RuntimePreparationRequired,
                "needs local preparation or authentication",
            ),
        ];
        for (stage, expected) in cases {
            let mut agents = ready_pair();
            agents[1] = agent("worker", "worker", "qwen", stage);
            let report = report(agents);
            assert!(!report.ready(), "{stage:?} must not be ready");
            let text = report.render(false);
            assert!(text.contains("worker    not ready  qwen"), "{text}");
            assert!(
                text.contains("team      not ready  1 lead · 1 worker"),
                "{text}"
            );
            assert!(text.contains("\nReason\n  "), "{text}");
            assert!(text.contains(expected), "{stage:?}: {text}");
            assert!(text.contains("\nFix\n  "), "{text}");
            assert!(text.contains("\n  am doctor"), "{text}");
            assert!(!text.contains("login"), "{text}");
            assert!(!text.contains("Ready to run."), "{text}");
        }
    }

    /// A configuration the run would refuse is its own readiness class: the
    /// agent's line carries the detail, and the reason carries it whole.
    #[test]
    fn doctor_reports_an_invalid_configuration_with_its_detail() {
        let detail = "agent `lead` has an invalid driver config: `max_events` must be a number";
        let mut agents = ready_pair();
        agents[0] = DoctorAgent {
            detail: Some(detail.into()),
            ..agent("lead", "reasoner", "codex", ReadinessStage::ConfigInvalid)
        };
        let verdict = report(agents);
        assert!(!verdict.ready());
        let text = verdict.render(false);
        assert!(text.contains("lead      not ready  codex  "), "{text}");
        assert!(text.contains(detail), "{text}");
        assert!(text.contains(&format!("\nReason\n  {detail}\n")), "{text}");
        assert!(
            text.contains("fix the driver configuration of `lead`"),
            "{text}"
        );
        assert!(text.contains("\n  am doctor"), "{text}");
        assert!(!text.contains("Ready to run."), "{text}");

        // A detail longer than one line carries is bounded on the line, and
        // never cut in the reason.
        let long = "x".repeat(MAX_AGENT_DETAIL_BYTES + 40);
        let mut agents = ready_pair();
        agents[0] = DoctorAgent {
            detail: Some(long.clone()),
            ..agent("lead", "reasoner", "codex", ReadinessStage::ConfigInvalid)
        };
        let text = report(agents).render(false);
        let line = text
            .lines()
            .find(|line| line.starts_with("lead"))
            .expect("the lead's line");
        assert!(line.ends_with("..."), "the line is bounded: {line}");
        assert!(line.len() < MAX_AGENT_DETAIL_BYTES + 64, "{line}");
        assert!(
            text.contains(&format!("\nReason\n  {long}\n")),
            "the reason carries the detail whole:\n{text}"
        );
    }

    /// An incomplete team is not ready even when every registered runtime is,
    /// and the decision names the missing role.
    #[test]
    fn doctor_requires_exactly_one_lead_and_at_least_one_worker() {
        let missing_worker = report(vec![agent(
            "lead",
            "reasoner",
            "codex",
            ReadinessStage::Ready,
        )]);
        assert!(!missing_worker.composition_ready());
        assert!(missing_worker
            .render(false)
            .contains("the team has no worker"));

        let missing_lead = report(vec![agent(
            "worker",
            "worker",
            "qwen",
            ReadinessStage::Ready,
        )]);
        assert!(!missing_lead.composition_ready());
        assert!(missing_lead.render(false).contains("the team has no lead"));

        let ambiguous = report(vec![
            agent("lead-a", "reasoner", "codex", ReadinessStage::Ready),
            agent("lead-b", "reasoner", "codex", ReadinessStage::Ready),
            agent("worker", "worker", "qwen", ReadinessStage::Ready),
        ]);
        assert!(!ambiguous.composition_ready());
        assert!(ambiguous.render(false).contains("2 reasoner Agents"));
    }

    /// `--verbose` is the only surface that prints the bounded stage codes.
    #[test]
    fn doctor_verbose_keeps_the_bounded_stage_codes() {
        let mut agents = ready_pair();
        agents[1] = agent("worker", "worker", "qwen", ReadinessStage::ProgramMissing);
        let report = report(agents);
        let verbose = report.render(true);
        assert!(
            verbose
                .contains("PROGRAM_FOUND LAUNCHSPEC_VALID SPAWN_OK PROTOCOL_OK SESSION_OK READY"),
            "{verbose}"
        );
        assert!(verbose.contains("PROGRAM_NOT_FOUND"), "{verbose}");
        assert!(verbose.contains("schema=11"), "{verbose}");
        assert!(verbose.contains("/tmp/project"), "{verbose}");
        assert!(!report.render(false).contains("PROGRAM_FOUND"));
    }

    /// The listing is role-first and its LAUNCH column is bounded.
    #[test]
    fn agent_table_is_role_first_and_bounds_the_launch_column() {
        let long_argv = format!(r#"["{}"]"#, "x".repeat(400));
        let text = render_agent_table(&[
            record("a-worker", "worker", "acp", "qwen", Some("[\"--acp\"]")),
            record("z-lead", "reasoner", "codex-app-server", "codex", None),
            record("m-utility", "utility", "acp", "qwen", Some(&long_argv)),
        ]);
        assert!(text.starts_with("ID"), "{text}");
        let rows = text.lines().skip(1).collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].starts_with("z-lead"), "{rows:?}");
        assert!(rows[1].starts_with("a-worker"), "{rows:?}");
        assert!(rows[2].starts_with("m-utility"), "{rows:?}");
        assert!(rows[0].contains("codex-app-server"), "{rows:?}");
        assert!(rows[1].ends_with("qwen --acp"), "{rows:?}");
        assert!(rows[2].ends_with("..."), "{rows:?}");
        assert!(
            !rows[2].contains(&"x".repeat(MAX_LAUNCH_BYTES + 1)),
            "the opaque argv must be bounded: {rows:?}"
        );
    }

    /// A long launch program is never cut: the program is the part an operator
    /// has to recognize.
    #[test]
    fn launch_rendering_keeps_the_program_whole() {
        let program = format!("/opt/{}/qwen", "x".repeat(120));
        let rendered = bounded_launch(&record("w", "worker", "acp", &program, None));
        assert_eq!(rendered, program);
    }

    #[test]
    fn agent_table_hints_when_the_registry_is_empty() {
        let text = render_agent_table(&[]);
        assert!(text.starts_with("ID"), "{text}");
        assert!(text.contains("no agents registered"), "{text}");
    }

    /// A credential-looking argument never reaches a rendered launch.
    #[test]
    fn launch_rendering_never_echoes_a_credential() {
        let rendered = bounded_launch(&record(
            "worker",
            "worker",
            "acp",
            "qwen",
            Some(r#"["--acp","--token=super-secret","-ds"]"#),
        ));
        assert!(!rendered.contains("super-secret"), "{rendered}");
        assert!(rendered.contains(REDACTED), "{rendered}");
        assert!(rendered.ends_with("-ds"), "{rendered}");
    }

    /// A launch argv survives whole when it is short, and raw argv JSON is
    /// never echoed.
    #[test]
    fn launch_rendering_keeps_a_short_argv_and_drops_raw_json() {
        assert_eq!(
            bounded_launch(&record("w", "worker", "acp", "codex", Some(r#"["-qw"]"#))),
            "codex -qw"
        );
        assert_eq!(
            bounded_launch(&record("w", "worker", "acp", "codex", Some("not json"))),
            "codex"
        );
    }

    #[test]
    fn registration_and_removal_render_their_decision() {
        let registered =
            render_agent_registration("lead", true, "reasoner", "codex-app-server", "codex");
        assert_eq!(
            registered,
            "registered agent `lead`\nrole     reasoner\nadapter  codex-app-server\nlaunch   codex\n\nNext: am doctor"
        );
        let updated =
            render_agent_registration("lead", false, "reasoner", "codex-app-server", "codex");
        assert!(updated.starts_with("updated agent `lead`"), "{updated}");
        assert!(!updated.contains("registered agent"), "{updated}");

        assert_eq!(
            render_agent_removed("utility", true),
            "removed agent `utility`"
        );
        let broken = render_agent_removed("worker", false);
        assert!(broken.contains("team is no longer runnable"), "{broken}");
        assert!(broken.contains("am doctor will fail"), "{broken}");
    }
}
