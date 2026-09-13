//! Small Rust normal-path CLI for the durable team board.

use std::path::{Path, PathBuf};
use std::time::Duration;

use agent_code_runtime::{AcpWorkerConfig, AcpWorkerDriver};
use agent_code_storage::{
    AgentRegistryRecord, ExternalRuntimeBinding, SqliteAgentRegistry, SqliteTaskBoard,
};
use agent_code_team::{AgentTask, DriverKind, TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

fn usage() -> &'static str {
    "usage: agent-code-cli <register|registry|run-acp|submit|status|cancel|override|resume|artifact|final> <database> [fields]"
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
    if fields.len() != 8 {
        return Err(
            "register requires exactly 8 fields: agent-id name tier driver-kind executable driver-args max-concurrency tags"
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
                "id={} name={} tier={} driver_kind={} executable={} args={} concurrency={} tags={}",
                agent.id,
                agent.name,
                agent.tier,
                agent.driver_kind.as_deref().unwrap_or("-"),
                agent.executable.as_deref().unwrap_or("-"),
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
    let execution = runtime.block_on(driver.execute_task(&AgentTask {
        id: task_id,
        objective: task.objective,
        kind: task.kind,
        context: Vec::new(),
    }));
    match execution {
        Ok(execution) => {
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
            Err(format!("run-acp: {text}"))
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
                board
                    .task(id)
                    .map_err(|e| format!("status: {e:?}"))?
                    .map(|task| {
                        format!(
                            "task={} status={} assignee={} objective={}",
                            task.id,
                            task.status.as_str(),
                            task.assignee.unwrap_or_else(|| "-".into()),
                            task.objective
                        )
                    })
                    .ok_or_else(|| format!("status: missing task {id}"))
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
        _ => Err(usage().into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
}
