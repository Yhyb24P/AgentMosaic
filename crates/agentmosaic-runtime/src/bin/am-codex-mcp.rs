//! Narrow stdio MCP bridge used only by the Phase 2.3 Codex live harness.
//! It persists bounded collaboration requests through the existing team board.

use std::io::{self, BufRead, Write};

use agentmosaic_storage::{RuntimeCollaborationRecord, SqliteTaskBoard};
use agentmosaic_team::{TaskBoard, TaskStatus};
use serde_json::{json, Value};

/// Run the fixed stdio bridge. This is intentionally not a general command
/// dispatcher; `am __internal codex-mcp` is its only product entrypoint.
pub fn run_codex_mcp_bridge() {
    let db = std::env::var("AGENTMOSAIC_DB").expect("AGENTMOSAIC_DB required");
    let task = std::env::var("AGENTMOSAIC_TASK_ID")
        .expect("AGENTMOSAIC_TASK_ID")
        .parse()
        .expect("task id");
    let attempt = std::env::var("AGENTMOSAIC_ATTEMPT")
        .expect("AGENTMOSAIC_ATTEMPT")
        .parse()
        .expect("attempt");
    let board =
        SqliteTaskBoard::open(rusqlite::Connection::open(db).expect("open board")).expect("board");
    audit("bridge-started");
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = msg.get("id").cloned();
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        audit(&format!("rpc-{method}"));
        let result = match method {
            "initialize" => {
                json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{},"resources":{},"prompts":{}},"serverInfo":{"name":"agentmosaic-codex-bridge","version":"0.1"}})
            }
            "tools/list" => json!({"tools":[
                {"name":"agentmosaic_request_context","description":"Request bounded ACC/team context","inputSchema":{"type":"object","properties":{"purpose":{"type":"string"}},"required":["purpose"]}},
                {"name":"agentmosaic_request_help","description":"Request bounded team help","inputSchema":{"type":"object","properties":{"question":{"type":"string"}},"required":["question"]}}
            ]}),
            "resources/list" => json!({"resources":[]}),
            "resources/templates/list" => json!({"resourceTemplates":[]}),
            "prompts/list" => json!({"prompts":[]}),
            "tools/call" => {
                audit("tools-call-received");
                let p = msg.get("params").unwrap_or(&Value::Null);
                let name = p.get("name").and_then(Value::as_str).unwrap_or_default();
                // MCP JSON-RPC request ids are supplied by the live client and
                // are the only native-call identity accepted by this bridge.
                // Tool arguments are deliberately not trusted for identity.
                let call_id = id
                    .as_ref()
                    .map(Value::to_string)
                    .unwrap_or_else(|| format!("unidentified-{name}"));
                let summary = p
                    .get("arguments")
                    .and_then(|a| a.get("purpose").or_else(|| a.get("question")))
                    .and_then(Value::as_str)
                    .unwrap_or("bounded request");
                let allowed = matches!(
                    name,
                    "agentmosaic_request_context" | "agentmosaic_request_help"
                );
                let persisted = if allowed {
                    board.record_runtime_collaboration(&RuntimeCollaborationRecord {
                        team_task_id: task,
                        attempt,
                        runtime_kind: "codex-app-server".into(),
                        native_call_id: call_id,
                        kind: name.into(),
                        payload_summary: summary.chars().take(512).collect(),
                        response_summary: Some("bounded team context delivered".into()),
                    })
                } else {
                    Ok(false)
                };
                match persisted {
                    Ok(inserted) if allowed => {
                        audit(if inserted {
                            "collaboration-persisted"
                        } else {
                            "collaboration-duplicate"
                        });
                        let text = if name == "agentmosaic_request_context" {
                            bounded_team_context(&board, "codex")
                        } else if inserted {
                            "bounded team help request persisted".into()
                        } else {
                            "duplicate bounded request".into()
                        };
                        json!({"content":[{"type":"text","text":text}],"isError":false})
                    }
                    Ok(_) => {
                        json!({"content":[{"type":"text","text":"unsupported tool"}],"isError":true})
                    }
                    Err(error) => {
                        audit("collaboration-persistence-error");
                        json!({"content":[{"type":"text","text":format!("bridge persistence failed: {error}")}],"isError":true})
                    }
                }
            }
            _ => json!({}),
        };
        if let Some(id) = id {
            let _ = writeln!(
                io::stdout(),
                "{}",
                json!({"jsonrpc":"2.0","id":id,"result":result})
            );
            let _ = io::stdout().flush();
        }
    }
}

/// Build the small, persisted team projection that is safe to send to the
/// active Codex turn. This is intentionally sourced from the board rather than
/// caller-supplied tool arguments or a raw runtime transcript. Successful
/// task summaries and artifact hashes make a worker result available even
/// where a worker had no directed free-form message to the Lead.
fn bounded_team_context(board: &SqliteTaskBoard, target: &str) -> String {
    const MAX_CONTEXT_CHARS: usize = 1024;
    let Ok(messages) = board.messages_to(target) else {
        return "bounded team context unavailable".into();
    };
    let mut text = String::from("bounded persisted team context:\n");
    for message in messages.into_iter().rev().take(4).rev() {
        let line = format!("from={}: {}\n", message.from_agent, message.body);
        if text.chars().count().saturating_add(line.chars().count()) > MAX_CONTEXT_CHARS {
            break;
        }
        text.push_str(&line);
    }
    if let Ok(task_ids) = board.task_ids() {
        for task_id in task_ids.into_iter().take(8) {
            let Ok(Some(task)) = board.task(task_id) else {
                continue;
            };
            if task.status != TaskStatus::Succeeded {
                continue;
            }
            let summary = board
                .attempts(task_id)
                .ok()
                .and_then(|attempts| {
                    attempts
                        .into_iter()
                        .rev()
                        .find(|attempt| attempt.status == TaskStatus::Succeeded)
                        .and_then(|attempt| attempt.result)
                })
                .unwrap_or_else(|| "completed".into());
            let artifacts = board
                .artifacts(task_id)
                .unwrap_or_default()
                .into_iter()
                .take(2)
                .map(|artifact| format!(" {}#{}", artifact.path, artifact.sha256))
                .collect::<String>();
            let line = format!(
                "task={task_id} from={} result={} artifacts={}\n",
                task.assignee.unwrap_or_else(|| "unassigned".into()),
                summary.chars().take(256).collect::<String>(),
                artifacts.chars().take(256).collect::<String>(),
            );
            if text.chars().count().saturating_add(line.chars().count()) > MAX_CONTEXT_CHARS {
                break;
            }
            text.push_str(&line);
        }
    }
    if text == "bounded persisted team context:\n" {
        text.push_str("no directed team message available\n");
    }
    text
}

/// Optional operational breadcrumbs. They never include tool arguments,
/// model text, credentials, prompts, or protocol bodies.
fn audit(event: &str) {
    let Ok(path) = std::env::var("AGENTMOSAIC_BRIDGE_LOG") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = writeln!(file, "{event}");
}

#[cfg(test)]
mod tests {
    use super::bounded_team_context;
    use agentmosaic_storage::SqliteTaskBoard;
    use agentmosaic_team::{
        AgentMessage, AgentTaskResult, ArtifactMeta, TaskAttempt, TaskBoard, TaskKind, TaskStatus,
    };

    #[test]
    fn context_is_bounded_and_sourced_from_directed_board_messages() {
        let mut board = SqliteTaskBoard::in_memory().expect("board");
        let task = board
            .create_task("worker result", None, TaskKind::Bulk, None)
            .expect("task");
        board
            .record_message(&AgentMessage {
                from_agent: "qwen".into(),
                to_agent: "codex".into(),
                body: "bounded worker finding".into(),
            })
            .expect("message");
        board
            .record_message(&AgentMessage {
                from_agent: "other".into(),
                to_agent: "lead".into(),
                body: "not for codex".into(),
            })
            .expect("other message");
        board
            .record_attempt(&TaskAttempt {
                task_id: task,
                attempt: 1,
                agent_id: "qwen".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .expect("running attempt");
        board
            .set_status(task, TaskStatus::Running)
            .expect("running task");
        board
            .commit_successful_result(
                &TaskAttempt {
                    task_id: task,
                    attempt: 1,
                    agent_id: "qwen".into(),
                    status: TaskStatus::Succeeded,
                    result: Some("bounded worker summary".into()),
                    error: None,
                },
                &AgentTaskResult {
                    task_id: task,
                    summary: "bounded worker summary".into(),
                    artifacts: vec![ArtifactMeta {
                        path: "worker.txt".into(),
                        sha256: "a".repeat(64),
                    }],
                    message: None,
                },
            )
            .expect("result");
        assert_eq!(task, 1);

        let context = bounded_team_context(&board, "codex");
        assert!(context.contains("from=qwen: bounded worker finding"));
        assert!(context.contains("result=bounded worker summary"));
        assert!(context.contains("worker.txt#"));
        assert!(!context.contains("not for codex"));
        assert!(context.chars().count() <= 1024);
    }
}
