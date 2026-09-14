//! The compatibility/diagnostic surface.
//!
//! These commands keep their current top-level spellings and their established
//! field-level grammar: clap consumes the `database` positional and the rest of
//! the invocation is handed to the extracted parser that already owned it. The
//! `run-team`/`resume-team` bounds are deliberately *not* clap flags, so a
//! malformed bound keeps failing through `parse_team_invocation` with the same
//! message and exit code as before.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agentmosaic_runtime::{
    AcpCancellation, AcpSessionStartedObserver, AcpWorkerConfig, AcpWorkerDriver, AcpWorkerError,
    LaunchSpec, TeamRunOptions, TeamRunner,
};
use agentmosaic_storage::{AgentRegistryRecord, ExternalRuntimeBinding, SqliteAgentRegistry};
use agentmosaic_team::{AgentTask, DriverKind, TaskAttempt, TaskBoard, TaskKind, TaskStatus};

use crate::{output, project};

/// `am advanced` itself: the compatibility commands, named honestly.
pub fn text() -> &'static str {
    "compatibility and low-level commands (not part of the normal onboarding path):\n\
     \x20 register      <database> <id> <name> <tier> <driver_kind> <executable> [argv...] <concurrency> <tags> <driver_config>\n\
     \x20 registry      <database> [limit]\n\
     \x20 run-acp       <database> <task-id> <agent-id> <working-directory> <auth-method|-> <timeout-seconds> [artifact-paths]\n\
     \x20 continue-acp  <database> <task-id> <agent-id> <source-task-id> <working-directory> <auth-method|-> <timeout-seconds>\n\
     \x20 run-team      <database> <repo> \"<objective>\" [--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]\n\
     \x20 resume-team   <database> <repo> <root-task-id> [--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]\n\
     \x20 submit        <database> <kind> \"<objective>\"\n\
     \x20 cancel        <database> <task>\n\
     \x20 override      <database> <task> <agent>\n\
     \x20 recover       <database> <task>\n\
     \x20 recover-all   <database>\n\
     \x20 resume        <database> <task>\n\
     \x20 binding       <database> <task> [attempt]\n\
     \n\
     the normal path is: am init, am agent add, am doctor, am run, am status, am final"
}

pub fn register(database: &str, fields: &[String]) -> Result<String, String> {
    register_agent(database, fields)
}

pub fn registry(database: &str, limit: Option<&str>) -> Result<String, String> {
    output::registry_list(database, limit)
}

pub fn submit(database: &str, fields: &[String]) -> Result<String, String> {
    let mut board = project::open(database)?;
    let kind = parse_kind(fields.first())?;
    let objective = fields.get(1..).unwrap_or_default().join(" ");
    if objective.trim().is_empty() {
        return Err("missing objective".into());
    }
    let id = board
        .create_task(&objective, None, kind, None)
        .map_err(|e| format!("submit: {e:?}"))?;
    Ok(format!("submitted task={id}"))
}

pub fn cancel(database: &str, fields: &[String]) -> Result<String, String> {
    let mut board = project::open(database)?;
    let task = super::parse_task(fields.first().ok_or("missing task id")?)?;
    board
        .set_status(task, TaskStatus::Cancelled)
        .map_err(|e| format!("cancel: {e:?}"))?;
    Ok(format!("cancelled task={task}"))
}

pub fn override_task(database: &str, fields: &[String]) -> Result<String, String> {
    let mut board = project::open(database)?;
    let task = super::parse_task(fields.first().ok_or("missing task id")?)?;
    let agent = fields
        .get(1)
        .ok_or_else(|| "missing agent id".to_string())?;
    board
        .assign(task, agent)
        .map_err(|e| format!("override: {e:?}"))?;
    Ok(format!("overrode task={task} agent={agent}"))
}

pub fn recover(database: &str, fields: &[String]) -> Result<String, String> {
    let mut board = project::open(database)?;
    let task = super::parse_task(fields.first().ok_or("missing task id")?)?;
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

pub fn recover_all(database: &str) -> Result<String, String> {
    let mut board = project::open(database)?;
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

pub fn resume(database: &str, fields: &[String]) -> Result<String, String> {
    let mut board = project::open(database)?;
    let task = super::parse_task(fields.first().ok_or("missing task id")?)?;
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

pub fn binding(database: &str, fields: &[String]) -> Result<String, String> {
    let board = project::open(database)?;
    let task = super::parse_task(fields.first().ok_or("missing task id")?)?;
    let attempt = fields
        .get(1)
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
        .ok_or_else(|| "binding: no external runtime binding for task attempt".to_string())?;
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

pub fn run_acp(database: &str, fields: &[String]) -> Result<String, String> {
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
    let mut board = project::open(database)?;
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
        let binding_board = project::open(&binding_database)
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
                let cancelled = project::open(&cancellation_database)
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
pub fn continue_acp(database: &str, fields: &[String]) -> Result<String, String> {
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
    let mut board = project::open(database)?;
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

/// The product entry point: one objective, one durable team result. This
/// surface only parses argv; all orchestration lives in `TeamRunner`.
pub fn run_team(database: &str, fields: &[String]) -> Result<String, String> {
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
    let runtime = super::team_runtime()?;
    let outcome = runtime
        .block_on(runner.run(&objective))
        .map_err(|error| format!("run-team: {error}"))?;
    Ok(output::render_team_outcome(&outcome))
}

/// Resume a team run whose root task already exists. Never replays finished
/// work: a succeeded root returns its persisted result.
pub fn resume_team(database: &str, fields: &[String]) -> Result<String, String> {
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
    let runtime = super::team_runtime()?;
    let outcome = runtime
        .block_on(runner.resume(root))
        .map_err(|error| format!("resume-team: {error}"))?;
    Ok(output::render_team_outcome(&outcome))
}
