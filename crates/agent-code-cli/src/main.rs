//! Small Rust normal-path CLI for the durable team board.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agent_code_runtime::{
    AcpCancellation, AcpSessionStartedObserver, AcpWorkerConfig, AcpWorkerDriver, AcpWorkerError,
};
use agent_code_storage::{
    AgentRegistryRecord, ExternalRuntimeBinding, SqliteAgentRegistry, SqliteTaskBoard,
};
use agent_code_team::{AgentTask, DriverKind, TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

fn usage() -> &'static str {
    "usage: agent-code-cli <register|registry|run-acp|continue-acp|submit|status|cancel|override|recover|recover-all|resume|artifact|binding|final> <database> [fields]"
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
    if !(8..=9).contains(&fields.len()) {
        return Err(
            "register requires 8 or 9 fields: agent-id name tier driver-kind executable driver-args max-concurrency tags [runtime-version-or--]"
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
            .ok_or_else(|| "driver kind must be native, acp, cli, or -".to_string()),
    }
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
                    "id={} name={} tier={} driver_kind={} executable={} version={} args={} concurrency={} tags={}",
                agent.id,
                agent.name,
                agent.tier,
                agent.driver_kind.as_deref().unwrap_or("-"),
                agent.executable.as_deref().unwrap_or("-"),
                agent.runtime_version.as_deref().unwrap_or("-"),
                agent.driver_args_json.as_deref().unwrap_or("-"),
                agent.max_concurrency.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                agent.tags_json.as_deref().unwrap_or("-"),
            )
            })
            .collect::<Vec<_>>()
            .join("\n");
    Ok(lines)
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
            let team_result = agent_code_team::AgentTaskResult {
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

fn run(args: &[String]) -> Result<String, String> {
    let command = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| usage().to_string())?;
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
                    "task={} status={} assignee={} attempts={} objective={}",
                    task.id,
                    task.status.as_str(),
                    task.assignee.unwrap_or_else(|| "-".into()),
                    attempts.len(),
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
        println!("agent-code-cli {}", env!("CARGO_PKG_VERSION"));
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
    use super::run;

    #[test]
    fn rejects_unknown_command() {
        assert!(run(&["unknown".into(), ":memory:".into()]).is_err());
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
            "agent_code_cli_invalid_acp_{}_{}.db",
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
            "agent_code_cli_continue_reject_{}_{}.db",
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
