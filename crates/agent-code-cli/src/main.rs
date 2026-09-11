//! Small Rust normal-path CLI for the durable team board.

use std::path::Path;

use agent_code_storage::SqliteTaskBoard;
use agent_code_team::{TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

fn usage() -> &'static str {
    "usage: agent-code-cli <submit|status|cancel|override|resume|artifact|final> <database> [arguments]"
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
