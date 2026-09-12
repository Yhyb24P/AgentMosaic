//! Small Rust normal-path CLI for the durable team board.

use std::path::Path;

use agent_code_storage::{AgentRegistryRecord, SqliteAgentRegistry, SqliteTaskBoard};
use agent_code_team::{DriverKind, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

fn usage() -> &'static str {
    "usage: agent-code-cli <register|registry|submit|status|cancel|override|resume|artifact|final> <database> [fields]"
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
    if limit < 0 {
        return Err("negative max-concurrency".to_string());
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
}
