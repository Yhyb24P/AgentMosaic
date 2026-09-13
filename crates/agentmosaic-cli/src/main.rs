//! Small Rust normal-path CLI for the durable team board.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agentmosaic_runtime::{
    run_codex_mcp_bridge, AcpCancellation, AcpSessionStartedObserver, AcpWorkerConfig,
    AcpWorkerDriver, AcpWorkerError, CodexAppServer, LaunchSpec, TeamRunOptions, TeamRunOutcome,
    TeamRunner,
};
use agentmosaic_storage::{
    AgentRegistryRecord, ExternalRuntimeBinding, SqliteAgentRegistry, SqliteTaskBoard,
};
use agentmosaic_team::{AgentTask, DriverKind, TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

const PROJECT_DIR: &str = ".agentmosaic";
const PROJECT_DB: &str = "state.db";

fn usage() -> &'static str {
    "usage: am <init|agent|doctor|run|register|registry|run-acp|continue-acp|run-team|resume-team|submit|status|cancel|override|recover|recover-all|resume|artifact|binding|final|tui> [fields]\n\
     \x20      am init [PATH]\n\
     \x20      am agent add <id> --role <reasoner|worker|utility> --adapter <acp|codex-app-server> [--name NAME] [--concurrency N] [--tag TAG] [--artifact RELPATH] -- <program> [arg ...]\n\
     \x20      am agent list\n\
     \x20      am doctor\n\
     \x20      am run \"<objective...>\"\n\
     \x20      am run-team <database> <repo> \"<objective...>\" [--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]\n\
     \x20      am resume-team <database> <repo> <root-task-id> [--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]\n\
     \x20      am tui <database>"
}

fn state_path(root: &Path) -> PathBuf {
    root.join(PROJECT_DIR).join(PROJECT_DB)
}

fn discover_project(start: &Path) -> Result<(PathBuf, PathBuf), String> {
    let start = start
        .canonicalize()
        .map_err(|e| format!("cannot resolve current directory: {e}"))?;
    for directory in start.ancestors() {
        let database = state_path(directory);
        if database.is_file() {
            return Ok((directory.to_path_buf(), database));
        }
    }
    Err("no initialized AgentMosaic project found; run `am init` first".into())
}

fn init_project(path: Option<&str>) -> Result<String, String> {
    let requested = path
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let requested = requested
        .canonicalize()
        .map_err(|e| format!("init path is not available: {e}"))?;
    if !requested.is_dir() {
        return Err("init path must be a directory".into());
    }
    let root = std::process::Command::new("git")
        .arg("-C")
        .arg(&requested)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|result| result.status.success())
        .and_then(|result| String::from_utf8(result.stdout).ok())
        .map(|value| PathBuf::from(value.trim()))
        .unwrap_or(requested);
    let directory = root.join(PROJECT_DIR);
    std::fs::create_dir_all(&directory)
        .map_err(|e| format!("create project state directory: {e}"))?;
    let database = state_path(&root);
    open(database.to_str().ok_or("project state path is not UTF-8")?)?;
    let ignore = root.join(".gitignore");
    if root.join(".git").exists() {
        let existing = std::fs::read_to_string(&ignore).unwrap_or_default();
        if !existing.lines().any(|line| line.trim() == "/.agentmosaic/") {
            let suffix = if existing.is_empty() || existing.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            std::fs::write(&ignore, format!("{existing}{suffix}/.agentmosaic/\n"))
                .map_err(|e| format!("update .gitignore: {e}"))?;
        }
    }
    Ok(format!("initialized AgentMosaic\nproject: {}\nstate:   {}\n\nnext:\n  am agent add ...\n  am doctor", root.display(), database.display()))
}

fn project_database() -> Result<(PathBuf, PathBuf), String> {
    discover_project(&std::env::current_dir().map_err(|e| e.to_string())?)
}

fn agent_add(fields: &[String]) -> Result<String, String> {
    let id = fields.first().ok_or("agent add requires an id")?.clone();
    let mut role = None;
    let mut adapter = None;
    let mut name = None;
    let mut concurrency = 1_i64;
    let mut tags = Vec::new();
    let mut artifacts = Vec::new();
    let mut index = 1;
    while index < fields.len() {
        match fields[index].as_str() {
            "--" => {
                index += 1;
                break;
            }
            "--role" => {
                index += 1;
                role = fields.get(index).cloned();
            }
            "--adapter" => {
                index += 1;
                adapter = fields.get(index).cloned();
            }
            "--name" => {
                index += 1;
                name = fields.get(index).cloned();
            }
            "--concurrency" => {
                index += 1;
                concurrency =
                    parse_concurrency(fields.get(index).ok_or("--concurrency requires a value")?)?;
            }
            "--tag" => {
                index += 1;
                tags.push(fields.get(index).ok_or("--tag requires a value")?.clone());
            }
            "--artifact" => {
                index += 1;
                artifacts.push(
                    fields
                        .get(index)
                        .ok_or("--artifact requires a value")?
                        .clone(),
                );
            }
            option => return Err(format!("unknown agent add option {option}")),
        };
        index += 1;
    }
    let role = role.ok_or("agent add requires --role")?;
    if !matches!(role.as_str(), "reasoner" | "worker" | "utility") {
        return Err("--role must be reasoner, worker, or utility".into());
    }
    let adapter = adapter.ok_or("agent add requires --adapter")?;
    if !matches!(adapter.as_str(), "acp" | "codex-app-server") {
        return Err("--adapter must be acp or codex-app-server".into());
    }
    let program = fields
        .get(index)
        .ok_or("agent add requires a launch command after --")?
        .clone();
    index += 1;
    let args = fields[index..].to_vec();
    let config = if artifacts.is_empty() {
        None
    } else {
        Some(serde_json::json!({"artifact_paths": artifacts}).to_string())
    };
    let (_, database) = project_database()?;
    SqliteAgentRegistry::open(&database)
        .map_err(|e| e.to_string())?
        .upsert_agent(&AgentRegistryRecord {
            id: id.clone(),
            name: name.unwrap_or_else(|| id.clone()),
            tier: role,
            driver_kind: Some(adapter),
            executable: Some(program),
            driver_args_json: Some(serde_json::to_string(&args).map_err(|e| e.to_string())?),
            max_concurrency: Some(concurrency),
            tags_json: Some(serde_json::to_string(&tags).map_err(|e| e.to_string())?),
            runtime_version: None,
            driver_config_json: config,
        })
        .map_err(|e| format!("agent add: {e}"))?;
    Ok(format!("added agent={id}"))
}

fn doctor() -> Result<String, String> {
    let (root, database) = project_database()?;
    let registry = SqliteAgentRegistry::open(&database).map_err(|e| format!("doctor: {e}"))?;
    let agents = registry.list_agents().map_err(|e| format!("doctor: {e}"))?;
    let mut lines = vec![
        format!("project   READY {}", root.display()),
        "state     READY schema=11".into(),
    ];
    let mut tiers = [0usize; 3];
    for agent in agents {
        match agent.tier.as_str() {
            "reasoner" => tiers[0] += 1,
            "worker" => tiers[1] += 1,
            "utility" => tiers[2] += 1,
            _ => {}
        }
        let ready = agent.executable.as_deref().is_some_and(program_on_path);
        let launch = agent
            .executable
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            && agent
                .driver_args_json
                .as_deref()
                .map(|raw| serde_json::from_str::<Vec<String>>(raw).is_ok())
                .unwrap_or(true);
        let stages = if !ready {
            "PROGRAM_NOT_FOUND".to_string()
        } else if !launch {
            "PROGRAM_FOUND LAUNCHSPEC_INVALID".to_string()
        } else {
            format!(
                "PROGRAM_FOUND LAUNCHSPEC_VALID {}",
                doctor_probe(&agent, &root)
            )
        };
        lines.push(format!(
            "{}      {}    {}",
            agent.id,
            agent.driver_kind.as_deref().unwrap_or("-"),
            stages
        ));
    }
    lines.push(format!(
        "team      reasoner={} worker={} utility={}",
        tiers[0], tiers[1], tiers[2]
    ));
    if tiers[0] != 1 {
        lines.push("lead      LEAD_SELECTION_AMBIGUOUS_OR_MISSING".into());
    }
    Ok(lines.join("\n"))
}

/// Probe the configured adapter without a prompt, model selection, or login.
/// The adapter owns the protocol details; the CLI reports only a bounded
/// readiness classification and never exposes protocol transcripts.
fn doctor_probe(agent: &AgentRegistryRecord, root: &Path) -> String {
    let Some(program) = agent.executable.as_deref() else {
        return "LAUNCHSPEC_INVALID".into();
    };
    let args = match agent.driver_args_json.as_deref() {
        Some(raw) => match serde_json::from_str::<Vec<String>>(raw) {
            Ok(args) => args,
            Err(_) => return "LAUNCHSPEC_INVALID".into(),
        },
        None => Vec::new(),
    };
    let launch = match LaunchSpec::new(program, args.clone()) {
        Ok(launch) => launch,
        Err(_) => return "LAUNCHSPEC_INVALID".into(),
    };
    match agent.driver_kind.as_deref() {
        Some("acp") => {
            let driver = match AcpWorkerDriver::new(AcpWorkerConfig {
                runtime_kind: "acp".into(),
                command: launch.program,
                args,
                auth_method: None,
                working_directory: root.to_path_buf(),
                timeout: Duration::from_secs(5),
                max_prompt_bytes: 1,
                max_result_bytes: 1,
                artifact_paths: Vec::new(),
            }) {
                Ok(driver) => driver,
                Err(_) => return "SPAWN_FAILED".into(),
            };
            match team_runtime().and_then(|runtime| {
                runtime
                    .block_on(driver.probe_readiness())
                    .map_err(|error| error.to_string())
            }) {
                Ok(()) => "SPAWN_OK PROTOCOL_OK SESSION_OK READY".into(),
                Err(error) if error.to_ascii_lowercase().contains("auth") => {
                    "RUNTIME_PREPARATION_REQUIRED".into()
                }
                Err(error) if error.contains("timed out") => "PROTOCOL_UNAVAILABLE".into(),
                Err(error) if error.contains("No such file") => "SPAWN_FAILED".into(),
                Err(_) => "PROTOCOL_UNAVAILABLE".into(),
            }
        }
        Some("codex-app-server") => match CodexAppServer::spawn_launch(launch, &[]) {
            Err(_) => "SPAWN_FAILED".into(),
            Ok(mut server) => match server
                .initialize("agentmosaic-doctor", "0.2")
                .and_then(|_| {
                    server.start_thread_with_options(
                        &root.display().to_string(),
                        None,
                        "read-only",
                        "never",
                    )
                }) {
                Ok(_) => "SPAWN_OK PROTOCOL_OK SESSION_OK READY".into(),
                Err(error) if error.to_string().to_ascii_lowercase().contains("auth") => {
                    "RUNTIME_PREPARATION_REQUIRED".into()
                }
                Err(_) => "PROTOCOL_UNAVAILABLE".into(),
            },
        },
        _ => "PROTOCOL_UNAVAILABLE".into(),
    }
}

/// A doctor program check is intentionally non-invasive: it never starts an
/// external Agent or attempts authentication. Protocol readiness is reported
/// only by adapters that can perform a safe handshake.
fn program_on_path(program: &str) -> bool {
    let candidate = Path::new(program);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        return candidate.is_file();
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(program).is_file())
    })
}

fn open(path: &str) -> Result<SqliteTaskBoard, String> {
    SqliteTaskBoard::open(Connection::open(Path::new(path)).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn parse_task(value: Option<&String>) -> Result<u64, String> {
    value
        .ok_or_else(|| "missing task id".to_string())?
        .parse()
        .map_err(|_| "invalid task id".to_string())
}

fn parse_kind(value: Option<&String>) -> Result<TaskKind, String> {
    match value.map(String::as_str) {
        Some("reasoning") => Ok(TaskKind::Reasoning),
        Some("bulk") => Ok(TaskKind::Bulk),
        Some("tool") => Ok(TaskKind::Tool),
        Some("utility") => Ok(TaskKind::Utility),
        Some("review") => Ok(TaskKind::Review),
        _ => Err("task kind must be reasoning, review, bulk, tool, or utility".into()),
    }
}

fn register_agent(database: &str, fields: &[String]) -> Result<String, String> {
    if !(8..=10).contains(&fields.len()) {
        return Err(
            "register requires 8 to 10 fields: agent-id name tier driver-kind executable driver-args max-concurrency tags [runtime-version-or--] [driver-config-json-or--]"
                .to_string(),
        );
    }
    let (id, name, tier, driver_kind, executable, driver_args, max_concurrency, tags) = (
        &fields[0], &fields[1], &fields[2], &fields[3], &fields[4], &fields[5], &fields[6],
        &fields[7],
    );
    if !matches!(tier.as_str(), "reasoner" | "worker" | "utility") {
        return Err(format!("invalid tier {tier}"));
    }
    let driver = parse_driver_kind(driver_kind.as_str())?;
    let executable = parse_optional_field(executable.as_str())?;
    let concurrency = parse_concurrency(max_concurrency.as_str())?;
    let record = AgentRegistryRecord {
        id: id.clone(),
        name: name.clone(),
        tier: tier.clone(),
        driver_kind: driver,
        executable,
        driver_args_json: Some(csv_json(driver_args)?),
        max_concurrency: Some(concurrency),
        tags_json: Some(csv_json(tags)?),
        runtime_version: fields
            .get(8)
            .map(|value| parse_optional_field(value))
            .transpose()?
            .flatten(),
        driver_config_json: fields
            .get(9)
            .map(|value| parse_driver_config(value))
            .transpose()?
            .flatten(),
    };
    let registry = SqliteAgentRegistry::open(database).map_err(|e| format!("register: {e}"))?;
    registry
        .upsert_agent(&record)
        .map_err(|e| format!("register: {e}"))?;
    Ok(format!("registered agent={id}"))
}

fn parse_driver_kind(value: &str) -> Result<Option<String>, String> {
    match value {
        "-" => Ok(None),
        kind => DriverKind::restore(kind)
            .map(|kind| Some(kind.as_str().to_string()))
            .ok_or_else(|| {
                "driver kind must be native, acp, cli, codex-app-server, or -".to_string()
            }),
    }
}

/// A driver config is durable, shareable text: it must be one JSON object (or
/// `-` for none), and the driver factory refuses secret-looking keys later.
fn parse_driver_config(value: &str) -> Result<Option<String>, String> {
    if value == "-" {
        return Ok(None);
    }
    let parsed: serde_json::Value = serde_json::from_str(value)
        .map_err(|_| "driver config must be one JSON object or -".to_string())?;
    if !parsed.is_object() {
        return Err("driver config must be one JSON object or -".to_string());
    }
    Ok(Some(value.to_string()))
}

fn parse_optional_field(value: &str) -> Result<Option<String>, String> {
    if value == "-" {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

fn parse_concurrency(value: &str) -> Result<i64, String> {
    let limit: i64 = value
        .parse()
        .map_err(|_| "invalid max-concurrency".to_string())?;
    if limit <= 0 {
        return Err("max-concurrency must be greater than zero".to_string());
    }
    Ok(limit)
}

fn csv_json(value: &str) -> Result<String, String> {
    let list = csv_list(value);
    serde_json::to_string(&list).map_err(|e| e.to_string())
}

fn csv_list(value: &str) -> Vec<String> {
    if value == "-" {
        return Vec::new();
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn registry_list(database: &str, limit: Option<&str>) -> Result<String, String> {
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
fn bounded_config_note(raw: &str) -> String {
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

fn run_acp(database: &str, fields: &[String]) -> Result<String, String> {
    if !(5..=6).contains(&fields.len()) {
        return Err("run-acp requires task-id agent-id working-directory auth-method-or-- timeout-seconds [artifact-paths-csv]".into());
    }
    let task_id = fields[0]
        .parse::<u64>()
        .map_err(|_| "invalid task id".to_string())?;
    let agent_id = &fields[1];
    let working_directory = PathBuf::from(&fields[2]);
    let auth_method = parse_optional_field(&fields[3])?;
    let timeout_seconds = fields[4]
        .parse::<u64>()
        .map_err(|_| "invalid timeout seconds".to_string())?;
    if timeout_seconds == 0 {
        return Err("timeout seconds must be greater than zero".into());
    }
    let artifact_paths = fields
        .get(5)
        .map(|value| csv_list(value).into_iter().map(PathBuf::from).collect())
        .unwrap_or_default();
    let registry = SqliteAgentRegistry::open(database).map_err(|e| format!("run-acp: {e}"))?;
    let agent = registry
        .get_agent(agent_id)
        .map_err(|e| format!("run-acp: {e}"))?
        .ok_or_else(|| format!("run-acp: unknown registered agent {agent_id}"))?;
    if agent.driver_kind.as_deref() != Some("acp") {
        return Err("run-acp requires an agent registered with driver-kind acp".into());
    }
    let executable = agent
        .executable
        .ok_or_else(|| "run-acp: registered ACP agent has no executable".to_string())?;
    let driver_args: Vec<String> = serde_json::from_str(
        agent
            .driver_args_json
            .as_deref()
            .ok_or_else(|| "run-acp: registered ACP agent has no args".to_string())?,
    )
    .map_err(|_| "run-acp: stored driver args are not a JSON string array".to_string())?;
    let mut board = open(database)?;
    let task = board
        .task(task_id)
        .map_err(|e| format!("run-acp: {e:?}"))?
        .ok_or_else(|| format!("run-acp: missing task {task_id}"))?;
    if matches!(task.status, TaskStatus::Succeeded | TaskStatus::Running) {
        return Err("run-acp requires a pending, assigned, failed, or cancelled task".into());
    }
    let attempt = board
        .attempts(task_id)
        .map_err(|e| format!("run-acp: {e:?}"))?
        .len() as u32
        + 1;
    board
        .assign(task_id, agent_id)
        .map_err(|e| format!("run-acp: {e:?}"))?;
    board
        .record_attempt(&TaskAttempt {
            task_id,
            attempt,
            agent_id: agent_id.clone(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })
        .map_err(|e| format!("run-acp: {e:?}"))?;
    board
        .set_status(task_id, TaskStatus::Running)
        .map_err(|e| format!("run-acp: {e:?}"))?;
    let driver_config = AcpWorkerConfig {
        runtime_kind: format!("registered-acp:{agent_id}"),
        command: PathBuf::from(executable),
        args: driver_args,
        auth_method,
        working_directory,
        timeout: Duration::from_secs(timeout_seconds),
        max_prompt_bytes: 4096,
        max_result_bytes: 4096,
        artifact_paths,
    };
    let driver = match AcpWorkerDriver::new(driver_config) {
        Ok(driver) => driver,
        Err(error) => {
            let text = error.to_string();
            board
                .complete_attempt(&TaskAttempt {
                    task_id,
                    attempt,
                    agent_id: agent_id.clone(),
                    status: TaskStatus::Failed,
                    result: None,
                    error: Some(text.clone()),
                })
                .and_then(|_| board.set_status(task_id, TaskStatus::Failed))
                .map_err(|e| format!("run-acp: {e:?}"))?;
            return Err(format!("run-acp: {text}"));
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|e| format!("run-acp: {e}"))?;
    let binding_database = database.to_string();
    let binding_agent = agent_id.clone();
    let binding_runtime_kind = "acp".to_string();
    let session_started: AcpSessionStartedObserver = Arc::new(move |session_id| {
        let binding_board = open(&binding_database)
            .map_err(|error| format!("open board for ACP binding: {error}"))?;
        binding_board
            .upsert_external_binding(&ExternalRuntimeBinding {
                team_task_id: task_id,
                attempt,
                agent_id: binding_agent.clone(),
                runtime_kind: binding_runtime_kind.clone(),
                native_thread_id: Some(session_id.to_string()),
                native_turn_id: None,
                lifecycle_state: "running".into(),
            })
            .map_err(|error| format!("persist ACP binding: {error}"))
    });
    let (cancellation, mut cancellation_listener) = AcpCancellation::new();
    let cancellation_database = database.to_string();
    let cancellation_request = cancellation.clone();
    let execution = runtime.block_on(async {
        let monitor = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let cancelled = open(&cancellation_database)
                    .ok()
                    .and_then(|board| board.task(task_id).ok().flatten())
                    .is_some_and(|task| task.status == TaskStatus::Cancelled);
                if cancelled {
                    cancellation_request.cancel();
                    break;
                }
            }
        });
        let result = driver
            .execute_task_with_cancellation_and_session_observer(
                &AgentTask {
                    id: task_id,
                    objective: task.objective,
                    kind: task.kind,
                    context: Vec::new(),
                },
                &mut cancellation_listener,
                session_started,
            )
            .await;
        monitor.abort();
        result
    });
    match execution {
        Ok(execution) => {
            // A response that races a durable user cancellation is never
            // committed as success. The running process may only accept a
            // result while the authoritative task remains non-cancelled.
            if board
                .task(task_id)
                .map_err(|e| format!("run-acp: {e:?}"))?
                .is_some_and(|task| task.status == TaskStatus::Cancelled)
            {
                board
                    .upsert_external_binding(&ExternalRuntimeBinding {
                        team_task_id: task_id,
                        attempt,
                        agent_id: agent_id.clone(),
                        runtime_kind: "acp".into(),
                        native_thread_id: Some(execution.external_session_id),
                        native_turn_id: None,
                        lifecycle_state: "cancelled".into(),
                    })
                    .map_err(|e| format!("run-acp: {e:?}"))?;
                board
                    .complete_attempt(&TaskAttempt {
                        task_id,
                        attempt,
                        agent_id: agent_id.clone(),
                        status: TaskStatus::Cancelled,
                        result: None,
                        error: Some("user cancellation requested before result commit".into()),
                    })
                    .map_err(|e| format!("run-acp: {e:?}"))?;
                return Err(format!("run-acp: task {task_id} was cancelled"));
            }
            board
                .upsert_external_binding(&ExternalRuntimeBinding {
                    team_task_id: task_id,
                    attempt,
                    agent_id: agent_id.clone(),
                    runtime_kind: "acp".into(),
                    native_thread_id: Some(execution.external_session_id),
                    native_turn_id: None,
                    lifecycle_state: "completed".into(),
                })
                .map_err(|e| format!("run-acp: {e}"))?;
            board
                .commit_successful_result(
                    &TaskAttempt {
                        task_id,
                        attempt,
                        agent_id: agent_id.clone(),
                        status: TaskStatus::Succeeded,
                        result: Some(execution.result.summary.clone()),
                        error: None,
                    },
                    &execution.result,
                )
                .map_err(|e| format!("run-acp: {e:?}"))?;
            Ok(format!("completed task={task_id} agent={agent_id}"))
        }
        Err(error) => {
            let text = error.to_string();
            let cancelled = matches!(error, AcpWorkerError::Cancelled)
                && board
                    .task(task_id)
                    .map_err(|e| format!("run-acp: {e:?}"))?
                    .is_some_and(|task| task.status == TaskStatus::Cancelled);
            if cancelled {
                if let Some(binding) = board
                    .external_binding(task_id, attempt)
                    .map_err(|e| format!("run-acp: {e}"))?
                {
                    board
                        .upsert_external_binding(&ExternalRuntimeBinding {
                            lifecycle_state: "cancelled".into(),
                            ..binding
                        })
                        .map_err(|e| format!("run-acp: {e}"))?;
                }
            }
            board
                .complete_attempt(&TaskAttempt {
                    task_id,
                    attempt,
                    agent_id: agent_id.clone(),
                    status: if cancelled {
                        TaskStatus::Cancelled
                    } else {
                        TaskStatus::Failed
                    },
                    result: None,
                    error: Some(text.clone()),
                })
                .and_then(|_| {
                    if cancelled {
                        Ok(())
                    } else {
                        board.set_status(task_id, TaskStatus::Failed)
                    }
                })
                .map_err(|e| format!("run-acp: {e:?}"))?;
            Err(format!("run-acp: {text}"))
        }
    }
}

/// Continue a *new* pending team task through the foreign ACP session recorded
/// on a completed source task. The source task is never replayed and its
/// foreign ID never becomes canonical task identity.
fn continue_acp(database: &str, fields: &[String]) -> Result<String, String> {
    if fields.len() != 6 {
        return Err("continue-acp requires task-id agent-id source-task-id working-directory auth-method-or-- timeout-seconds".into());
    }
    let task_id = fields[0]
        .parse::<u64>()
        .map_err(|_| "invalid task id".to_string())?;
    let agent_id = &fields[1];
    let source_task = fields[2]
        .parse::<u64>()
        .map_err(|_| "invalid source task id".to_string())?;
    let working_directory = PathBuf::from(&fields[3]);
    let auth_method = parse_optional_field(&fields[4])?;
    let timeout_seconds = fields[5]
        .parse::<u64>()
        .map_err(|_| "invalid timeout seconds".to_string())?;
    if timeout_seconds == 0 {
        return Err("timeout seconds must be greater than zero".into());
    }
    let registry = SqliteAgentRegistry::open(database).map_err(|e| format!("continue-acp: {e}"))?;
    let agent = registry
        .get_agent(agent_id)
        .map_err(|e| format!("continue-acp: {e}"))?
        .ok_or_else(|| format!("continue-acp: unknown registered agent {agent_id}"))?;
    if agent.driver_kind.as_deref() != Some("acp") {
        return Err("continue-acp requires an agent registered with driver-kind acp".into());
    }
    let executable = agent
        .executable
        .ok_or_else(|| "continue-acp: registered ACP agent has no executable".to_string())?;
    let driver_args: Vec<String> = serde_json::from_str(
        agent
            .driver_args_json
            .as_deref()
            .ok_or_else(|| "continue-acp: registered ACP agent has no args".to_string())?,
    )
    .map_err(|_| "continue-acp: stored driver args are not a JSON string array".to_string())?;
    let mut board = open(database)?;
    let task = board
        .task(task_id)
        .map_err(|e| format!("continue-acp: {e:?}"))?
        .ok_or_else(|| format!("continue-acp: missing task {task_id}"))?;
    if !matches!(
        task.status,
        TaskStatus::Pending | TaskStatus::Assigned | TaskStatus::Failed | TaskStatus::Cancelled
    ) {
        return Err("continue-acp requires a non-running, non-succeeded task".into());
    }
    let source_attempt = board
        .attempts(source_task)
        .map_err(|e| format!("continue-acp: {e:?}"))?
        .len() as u32;
    let source_binding = board
        .external_binding(source_task, source_attempt)
        .map_err(|e| format!("continue-acp: {e}"))?
        .ok_or_else(|| "continue-acp: source task has no external binding".to_string())?;
    if source_binding.agent_id != *agent_id
        || source_binding.runtime_kind != "acp"
        || source_binding.lifecycle_state != "completed"
    {
        return Err(
            "continue-acp: source binding is not a completed binding for this ACP agent".into(),
        );
    }
    let session_id = source_binding.native_thread_id.ok_or_else(|| {
        "continue-acp: source binding has no external session reference".to_string()
    })?;
    let attempt = board
        .attempts(task_id)
        .map_err(|e| format!("continue-acp: {e:?}"))?
        .len() as u32
        + 1;
    board
        .assign(task_id, agent_id)
        .map_err(|e| format!("continue-acp: {e:?}"))?;
    board
        .record_attempt(&TaskAttempt {
            task_id,
            attempt,
            agent_id: agent_id.clone(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })
        .map_err(|e| format!("continue-acp: {e:?}"))?;
    board
        .set_status(task_id, TaskStatus::Running)
        .map_err(|e| format!("continue-acp: {e:?}"))?;
    board
        .upsert_external_binding(&ExternalRuntimeBinding {
            team_task_id: task_id,
            attempt,
            agent_id: agent_id.clone(),
            runtime_kind: "acp".into(),
            native_thread_id: Some(session_id.clone()),
            native_turn_id: None,
            lifecycle_state: "running".into(),
        })
        .map_err(|e| format!("continue-acp: {e}"))?;
    let driver = AcpWorkerDriver::new(AcpWorkerConfig {
        runtime_kind: format!("registered-acp:{agent_id}"),
        command: PathBuf::from(executable),
        args: driver_args,
        auth_method,
        working_directory,
        timeout: Duration::from_secs(timeout_seconds),
        max_prompt_bytes: 4096,
        max_result_bytes: 4096,
        artifact_paths: Vec::new(),
    })
    .map_err(|e| format!("continue-acp: {e}"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|e| format!("continue-acp: {e}"))?;
    let outcome = runtime.block_on(driver.resume_with_follow_up(&session_id, &task.objective));
    match outcome {
        Ok(summary) => {
            let result = AgentTask {
                id: task_id,
                objective: String::new(),
                kind: task.kind,
                context: Vec::new(),
            };
            let team_result = agentmosaic_team::AgentTaskResult {
                task_id: result.id,
                summary: summary.clone(),
                artifacts: Vec::new(),
                message: None,
            };
            board
                .upsert_external_binding(&ExternalRuntimeBinding {
                    team_task_id: task_id,
                    attempt,
                    agent_id: agent_id.clone(),
                    runtime_kind: "acp".into(),
                    native_thread_id: Some(session_id),
                    native_turn_id: None,
                    lifecycle_state: "completed".into(),
                })
                .map_err(|e| format!("continue-acp: {e}"))?;
            board
                .commit_successful_result(
                    &TaskAttempt {
                        task_id,
                        attempt,
                        agent_id: agent_id.clone(),
                        status: TaskStatus::Succeeded,
                        result: Some(summary),
                        error: None,
                    },
                    &team_result,
                )
                .map_err(|e| format!("continue-acp: {e:?}"))?;
            Ok(format!(
                "continued task={task_id} agent={agent_id} source_task={source_task}"
            ))
        }
        Err(error) => {
            let text = error.to_string();
            board
                .complete_attempt(&TaskAttempt {
                    task_id,
                    attempt,
                    agent_id: agent_id.clone(),
                    status: TaskStatus::Failed,
                    result: None,
                    error: Some(text.clone()),
                })
                .and_then(|_| board.set_status(task_id, TaskStatus::Failed))
                .map_err(|e| format!("continue-acp: {e:?}"))?;
            Err(format!("continue-acp: {text}"))
        }
    }
}

/// A parsed `run-team` / `resume-team` invocation: the positional arguments and
/// the run bounds. Flags may appear anywhere among the trailing arguments.
struct TeamInvocation {
    positional: Vec<String>,
    options: TeamRunOptions,
}

const TEAM_FLAGS: &str = "[--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]";

fn parse_team_invocation(fields: &[String]) -> Result<TeamInvocation, String> {
    let mut positional = Vec::new();
    let mut options = TeamRunOptions::default();
    let mut lead = None;
    let mut index = 0;
    while index < fields.len() {
        match fields[index].as_str() {
            "--lead" => lead = Some(team_flag_value(fields, &mut index, "--lead")?.to_string()),
            "--max-rounds" => {
                options.max_rounds = parse_positive_u32(
                    team_flag_value(fields, &mut index, "--max-rounds")?,
                    "--max-rounds",
                )?
            }
            "--max-tasks" => {
                options.max_tasks = parse_positive_u32(
                    team_flag_value(fields, &mut index, "--max-tasks")?,
                    "--max-tasks",
                )? as usize
            }
            "--max-retries" => {
                options.max_retries = parse_positive_u32(
                    team_flag_value(fields, &mut index, "--max-retries")?,
                    "--max-retries",
                )?
            }
            token if token.starts_with("--") => {
                return Err(format!("unknown option {token}; expected {TEAM_FLAGS}"))
            }
            token => positional.push(token.to_string()),
        }
        index += 1;
    }
    options.lead_agent = lead;
    Ok(TeamInvocation {
        positional,
        options,
    })
}

fn team_flag_value<'a>(
    fields: &'a [String],
    index: &mut usize,
    flag: &str,
) -> Result<&'a str, String> {
    *index += 1;
    fields
        .get(*index)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_positive_u32(value: &str, flag: &str) -> Result<u32, String> {
    let parsed: u32 = value
        .parse()
        .map_err(|_| format!("{flag} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{flag} must be greater than zero"));
    }
    Ok(parsed)
}

fn team_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|error| format!("team run runtime: {error}"))
}

/// The bounded, human-readable summary of one team run.
fn render_team_outcome(outcome: &TeamRunOutcome) -> String {
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

/// The product entry point: one objective, one durable team result. This
/// surface only parses argv; all orchestration lives in `TeamRunner`.
fn run_team(database: &str, fields: &[String]) -> Result<String, String> {
    let invocation = parse_team_invocation(fields)?;
    let repo = invocation.positional.first().ok_or_else(|| {
        format!("run-team requires <database> <repo> \"<objective>\" {TEAM_FLAGS}")
    })?;
    let objective = invocation.positional[1..].join(" ");
    if objective.trim().is_empty() {
        return Err(format!(
            "run-team requires an objective: <database> <repo> \"<objective>\" {TEAM_FLAGS}"
        ));
    }
    let host = LaunchSpec::new(
        std::env::current_exe().map_err(|e| format!("locate am executable: {e}"))?,
        Vec::new(),
    )?;
    let runner = TeamRunner::new(database, repo, invocation.options).with_bridge_host(host);
    let runtime = team_runtime()?;
    let outcome = runtime
        .block_on(runner.run(&objective))
        .map_err(|error| format!("run-team: {error}"))?;
    Ok(render_team_outcome(&outcome))
}

/// Resume a team run whose root task already exists. Never replays finished
/// work: a succeeded root returns its persisted result.
fn resume_team(database: &str, fields: &[String]) -> Result<String, String> {
    let invocation = parse_team_invocation(fields)?;
    let root = match invocation.positional.as_slice() {
        [_, _] => invocation.positional[1]
            .parse::<u64>()
            .map_err(|_| "invalid root task id".to_string())?,
        _ => {
            return Err(format!(
                "resume-team requires <database> <repo> <root-task-id> {TEAM_FLAGS}"
            ))
        }
    };
    let repo = invocation.positional[0].clone();
    let host = LaunchSpec::new(
        std::env::current_exe().map_err(|e| format!("locate am executable: {e}"))?,
        Vec::new(),
    )?;
    let runner = TeamRunner::new(database, repo, invocation.options).with_bridge_host(host);
    let runtime = team_runtime()?;
    let outcome = runtime
        .block_on(runner.resume(root))
        .map_err(|error| format!("resume-team: {error}"))?;
    Ok(render_team_outcome(&outcome))
}

fn run(args: &[String]) -> Result<String, String> {
    let command = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| usage().to_string())?;
    match command {
        "init" => return init_project(args.get(1).map(String::as_str)),
        "agent" => match args.get(1).map(String::as_str) {
            Some("add") => return agent_add(&args[2..]),
            Some("list") => {
                let (_, database) = project_database()?;
                return registry_list(
                    database.to_str().ok_or("project state path is not UTF-8")?,
                    None,
                );
            }
            _ => return Err("usage: am agent <add|list>".into()),
        },
        "doctor" => return doctor(),
        "run" => {
            let (root, database) = project_database()?;
            let objective = args.get(1..).unwrap_or_default().join(" ");
            if objective.trim().is_empty() {
                return Err("am run requires an objective".into());
            }
            return run_team(
                database.to_str().ok_or("project state path is not UTF-8")?,
                &[root.display().to_string(), objective],
            );
        }
        _ => {}
    }
    let database = args.get(1).ok_or_else(|| usage().to_string())?;
    let mut board = open(database)?;
    match command {
        "submit" => {
            let kind = parse_kind(args.get(2))?;
            let objective = args.get(3..).unwrap_or_default().join(" ");
            if objective.trim().is_empty() {
                return Err("missing objective".into());
            }
            let id = board
                .create_task(&objective, None, kind, None)
                .map_err(|e| format!("submit: {e:?}"))?;
            Ok(format!("submitted task={id}"))
        }
        "status" => board
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
            .map(|lines| lines.join("\n")),
        "cancel" => {
            let task = parse_task(args.get(2))?;
            board
                .set_status(task, TaskStatus::Cancelled)
                .map_err(|e| format!("cancel: {e:?}"))?;
            Ok(format!("cancelled task={task}"))
        }
        "override" => {
            let task = parse_task(args.get(2))?;
            let agent = args.get(3).ok_or_else(|| "missing agent id".to_string())?;
            board
                .assign(task, agent)
                .map_err(|e| format!("override: {e:?}"))?;
            Ok(format!("overrode task={task} agent={agent}"))
        }
        "recover" => {
            let task = parse_task(args.get(2))?;
            match board
                .recover_interrupted_attempt(task)
                .map_err(|e| format!("recover: {e:?}"))?
            {
                Some(attempt) => Ok(format!(
                    "recovered task={task} interrupted_attempt={}",
                    attempt.attempt
                )),
                None => Ok(format!(
                    "recover found no interrupted running attempt task={task}"
                )),
            }
        }
        "recover-all" => {
            let ids = board
                .task_ids()
                .map_err(|e| format!("recover-all: {e:?}"))?;
            let mut recovered = Vec::new();
            for task in ids {
                if let Some(attempt) = board
                    .recover_interrupted_attempt(task)
                    .map_err(|e| format!("recover-all: {e:?}"))?
                {
                    recovered.push(format!("{task}:{}", attempt.attempt));
                }
            }
            if recovered.is_empty() {
                Ok("recover-all found no interrupted running attempts".into())
            } else {
                Ok(format!(
                    "recovered interrupted attempts={}",
                    recovered.join(",")
                ))
            }
        }
        "resume" => {
            let task = parse_task(args.get(2))?;
            let record = board
                .task(task)
                .map_err(|e| format!("resume: {e:?}"))?
                .ok_or_else(|| format!("resume: missing task {task}"))?;
            if !matches!(record.status, TaskStatus::Failed | TaskStatus::Cancelled) {
                return Err("resume requires failed or cancelled task".into());
            }
            board
                .set_status(task, TaskStatus::Pending)
                .map_err(|e| format!("resume: {e:?}"))?;
            Ok(format!("resumed task={task}"))
        }
        "artifact" => {
            let task = parse_task(args.get(2))?;
            board
                .artifacts(task)
                .map_err(|e| format!("artifact: {e:?}"))
                .map(|items| {
                    items
                        .into_iter()
                        .map(|item| {
                            format!("task={task} path={} sha256={}", item.path, item.sha256)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
        }
        "binding" => {
            let task = parse_task(args.get(2))?;
            let attempt = args
                .get(3)
                .map(|value| {
                    value
                        .parse::<u32>()
                        .map_err(|_| "invalid attempt".to_string())
                })
                .transpose()?
                .unwrap_or_else(|| {
                    board
                        .attempts(task)
                        .ok()
                        .map(|items| items.len() as u32)
                        .unwrap_or(0)
                });
            let binding = board
                .external_binding(task, attempt)
                .map_err(|e| format!("binding: {e}"))?
                .ok_or_else(|| {
                    "binding: no external runtime binding for task attempt".to_string()
                })?;
            Ok(format!(
                "task={} attempt={} agent={} runtime_kind={} lifecycle_state={} external_reference_present={}",
                binding.team_task_id,
                binding.attempt,
                binding.agent_id,
                binding.runtime_kind,
                binding.lifecycle_state,
                binding.native_thread_id.is_some(),
            ))
        }
        "final" => {
            let task = parse_task(args.get(2))?;
            let result = board
                .attempts(task)
                .map_err(|e| format!("final: {e:?}"))?
                .into_iter()
                .rev()
                .find(|attempt| attempt.status == TaskStatus::Succeeded)
                .and_then(|attempt| attempt.result)
                .ok_or_else(|| "no successful result for task".to_string())?;
            Ok(result)
        }
        "register" => register_agent(database, &args[2..]),
        "registry" => registry_list(database, args.get(2).map(String::as_str)),
        "run-acp" => run_acp(database, &args[2..]),
        "continue-acp" => continue_acp(database, &args[2..]),
        "run-team" => run_team(database, &args[2..]),
        "resume-team" => resume_team(database, &args[2..]),
        "tui" => {
            agentmosaic_tui::run(database)?;
            Ok(String::new())
        }
        _ => Err(usage().into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.as_slice(), [flag] if flag == "--help" || flag == "-h") {
        println!("{}", usage());
        return;
    }
    if matches!(args.as_slice(), [flag] if flag == "--version" || flag == "-V") {
        println!("am {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if matches!(args.as_slice(), [internal, bridge] if internal == "__internal" && bridge == "codex-mcp")
    {
        run_codex_mcp_bridge();
        return;
    }
    match run(&args) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{run, usage};

    #[test]
    fn rejects_unknown_command() {
        assert!(run(&["unknown".into(), ":memory:".into()]).is_err());
    }

    #[test]
    fn normal_usage_does_not_advertise_the_internal_bridge() {
        assert!(!usage().contains("__internal"));
        assert!(!usage().contains("codex-mcp"));
    }

    #[test]
    fn registry_rejects_zero_concurrency_before_persisting_it() {
        let result = run(&[
            "register".into(),
            ":memory:".into(),
            "worker".into(),
            "worker".into(),
            "worker".into(),
            "acp".into(),
            "qwen".into(),
            "--acp".into(),
            "0".into(),
            "-".into(),
        ]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("greater than zero"));
    }

    #[test]
    fn run_acp_persists_invalid_configuration_as_failed() {
        let database = std::env::temp_dir().join(format!(
            "agentmosaic_cli_invalid_acp_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = database.to_string_lossy().into_owned();
        run(&[
            "register".into(),
            db.clone(),
            "worker".into(),
            "worker".into(),
            "worker".into(),
            "acp".into(),
            "qwen".into(),
            "--acp".into(),
            "1".into(),
            "-".into(),
        ])
        .unwrap();
        let submitted =
            run(&["submit".into(), db.clone(), "bulk".into(), "bounded".into()]).unwrap();
        let task = submitted.strip_prefix("submitted task=").unwrap();
        let result = run(&[
            "run-acp".into(),
            db.clone(),
            task.into(),
            "worker".into(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            "-".into(),
            "30".into(),
            "../outside".into(),
        ]);
        assert!(result.is_err());
        let status = run(&["status".into(), db]).unwrap();
        assert!(status.contains("status=failed"));
        let _ = std::fs::remove_file(database);
    }

    #[test]
    fn continue_acp_rejects_source_without_a_completed_binding() {
        let database = std::env::temp_dir().join(format!(
            "agentmosaic_cli_continue_reject_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = database.to_string_lossy().into_owned();
        run(&[
            "register".into(),
            db.clone(),
            "worker".into(),
            "worker".into(),
            "worker".into(),
            "acp".into(),
            "qwen".into(),
            "--acp".into(),
            "1".into(),
            "-".into(),
        ])
        .unwrap();
        let source = run(&["submit".into(), db.clone(), "bulk".into(), "source".into()]).unwrap();
        let next = run(&["submit".into(), db.clone(), "bulk".into(), "next".into()]).unwrap();
        let source_id = source.strip_prefix("submitted task=").unwrap();
        let next_id = next.strip_prefix("submitted task=").unwrap();
        let result = run(&[
            "continue-acp".into(),
            db.clone(),
            next_id.into(),
            "worker".into(),
            source_id.into(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            "-".into(),
            "30".into(),
        ]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no external binding"));
        let status = run(&["status".into(), db]).unwrap();
        assert!(status.contains(&format!("task={next_id} status=pending")));
        let _ = std::fs::remove_file(database);
    }
}
