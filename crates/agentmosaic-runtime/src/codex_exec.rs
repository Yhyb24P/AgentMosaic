//! Stable Codex `exec --json` event normalization.
//!
//! This module deliberately stores only identifiers and bounded, vendor-neutral
//! summaries.  In particular, JSONL command arguments and tool output are not
//! copied into AgentMosaic's durable runtime-event log.

use agentmosaic_team::{RuntimeEvent, RuntimeFileChangeKind, RuntimePlanItem};
use serde_json::Value;

use crate::RuntimeError;

/// Decode one JSONL event without retaining raw command input/output.
pub fn normalize_event(value: &Value) -> Result<Vec<RuntimeEvent>, RuntimeError> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| RuntimeError::Protocol("Codex exec event missing type".into()))?;
    if matches!(kind, "error" | "turn.failed") {
        return Err(RuntimeError::Protocol(
            value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Codex exec failed")
                .into(),
        ));
    }
    let event = match kind {
        "thread.started" => {
            value
                .get("thread_id")
                .and_then(Value::as_str)
                .map(|id| RuntimeEvent::SessionStarted {
                    native_session_id: id.into(),
                })
        }
        "item.completed"
            if value.pointer("/item/type").and_then(Value::as_str) == Some("agent_message") =>
        {
            value
                .pointer("/item/text")
                .and_then(Value::as_str)
                .map(|text| RuntimeEvent::AssistantMessageCompleted { text: text.into() })
        }
        "item.completed"
            if value.pointer("/item/type").and_then(Value::as_str) == Some("reasoning") =>
        {
            value
                .pointer("/item/text")
                .and_then(Value::as_str)
                .map(|text| RuntimeEvent::ReasoningSummary { text: text.into() })
        }
        "item.started"
            if value.pointer("/item/type").and_then(Value::as_str) == Some("command_execution") =>
        {
            value.pointer("/item/id").and_then(Value::as_str).map(|id| {
                RuntimeEvent::CommandStarted {
                    native_call_id: id.into(),
                    command: "command started".into(),
                }
            })
        }
        "item.completed"
            if value.pointer("/item/type").and_then(Value::as_str) == Some("command_execution") =>
        {
            value.pointer("/item/id").and_then(Value::as_str).map(|id| {
                RuntimeEvent::CommandCompleted {
                    native_call_id: id.into(),
                    exit_code: value
                        .pointer("/item/exit_code")
                        .and_then(Value::as_i64)
                        .and_then(|code| i32::try_from(code).ok()),
                    output_summary: "command completed".into(),
                }
            })
        }
        "item.completed"
            if value.pointer("/item/type").and_then(Value::as_str) == Some("file_change") =>
        {
            file_changes(value)
        }
        "item.started"
            if matches!(item_type(value), Some("mcp_tool_call") | Some("web_search")) =>
        {
            tool_started(value)
        }
        "item.completed"
            if matches!(item_type(value), Some("mcp_tool_call") | Some("web_search")) =>
        {
            tool_completed(value)
        }
        "item.started" if item_type(value) == Some("collab_tool_call") => value
            .pointer("/item/id")
            .and_then(Value::as_str)
            .map(|id| RuntimeEvent::SubagentStarted {
                native_id: id.into(),
                parent_native_id: value
                    .pointer("/item/parent_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            }),
        "item.completed" if item_type(value) == Some("collab_tool_call") => value
            .pointer("/item/id")
            .and_then(Value::as_str)
            .map(|id| RuntimeEvent::SubagentCompleted {
                native_id: id.into(),
                status: item_status(value).unwrap_or("completed").into(),
            }),
        "item.updated"
            if value.pointer("/item/type").and_then(Value::as_str) == Some("todo_list") =>
        {
            value
                .pointer("/item/items")
                .and_then(Value::as_array)
                .map(|items| RuntimeEvent::PlanUpdated {
                    items: items
                        .iter()
                        .filter_map(|item| {
                            Some(RuntimePlanItem {
                                text: item.get("text")?.as_str()?.into(),
                                status: item
                                    .get("status")
                                    .and_then(Value::as_str)
                                    .map(str::to_owned),
                            })
                        })
                        .collect(),
                })
        }
        "turn.completed" => Some(RuntimeEvent::UsageUpdated {
            input_tokens: value.pointer("/usage/input_tokens").and_then(Value::as_u64),
            cached_input_tokens: value
                .pointer("/usage/cached_input_tokens")
                .and_then(Value::as_u64),
            output_tokens: value
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64),
            reasoning_tokens: None,
            estimated_cost_usd: None,
        }),
        _ => None,
    };
    Ok(event.into_iter().collect())
}

fn item_type(value: &Value) -> Option<&str> {
    value.pointer("/item/type").and_then(Value::as_str)
}

fn item_status(value: &Value) -> Option<&str> {
    value.pointer("/item/status").and_then(Value::as_str)
}

fn tool_started(value: &Value) -> Option<RuntimeEvent> {
    value
        .pointer("/item/id")
        .and_then(Value::as_str)
        .map(|id| RuntimeEvent::ToolCallStarted {
            native_call_id: id.into(),
            tool: tool_name(value),
            input_summary: "tool started".into(),
        })
}

fn tool_completed(value: &Value) -> Option<RuntimeEvent> {
    value
        .pointer("/item/id")
        .and_then(Value::as_str)
        .map(|id| RuntimeEvent::ToolCallCompleted {
            native_call_id: id.into(),
            tool: tool_name(value),
            ok: !matches!(item_status(value), Some("failed") | Some("error")),
            output_summary: "tool completed".into(),
        })
}

fn tool_name(value: &Value) -> String {
    if item_type(value) == Some("web_search") {
        return "web_search".into();
    }
    value
        .pointer("/item/tool")
        .or_else(|| value.pointer("/item/name"))
        .and_then(Value::as_str)
        .unwrap_or("mcp")
        .to_owned()
}

fn file_changes(value: &Value) -> Option<RuntimeEvent> {
    let item = value.get("item").unwrap_or(value);
    let changes = item
        .get("changes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_else(|| std::slice::from_ref(item));
    changes.iter().find_map(|change| {
        Some(RuntimeEvent::FileChanged {
            path: change.get("path")?.as_str()?.to_owned(),
            change: match change
                .get("kind")
                .or_else(|| change.get("change_type"))
                .and_then(Value::as_str)
            {
                Some("create") | Some("created") => RuntimeFileChangeKind::Created,
                Some("delete") | Some("deleted") => RuntimeFileChangeKind::Deleted,
                Some("rename") | Some("renamed") => RuntimeFileChangeKind::Renamed,
                Some("modify") | Some("modified") | None => RuntimeFileChangeKind::Modified,
                Some(_) => RuntimeFileChangeKind::Other,
            },
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_thread_message_and_never_exposes_command_text() {
        assert!(matches!(
            normalize_event(&json!({"type":"thread.started","thread_id":"t"})).unwrap()[0],
            RuntimeEvent::SessionStarted { .. }
        ));
        assert!(matches!(
            normalize_event(
                &json!({"type":"item.completed","item":{"type":"agent_message","text":"done"}})
            )
            .unwrap()[0],
            RuntimeEvent::AssistantMessageCompleted { .. }
        ));
        let command = normalize_event(&json!({"type":"item.started","item":{"type":"command_execution","id":"c","command":"secret"}})).unwrap();
        assert!(
            matches!(&command[0], RuntimeEvent::CommandStarted { command, .. } if command == "command started")
        );
    }

    #[test]
    fn maps_pinned_exec_tool_collaboration_plan_and_usage_surface() {
        let mcp = normalize_event(&json!({
            "type":"item.completed",
            "item":{"type":"mcp_tool_call","id":"m1","tool":"git","status":"failed","arguments":"secret"}
        }))
        .unwrap();
        assert!(matches!(
            &mcp[0],
            RuntimeEvent::ToolCallCompleted { native_call_id, tool, ok, output_summary }
                if native_call_id == "m1" && tool == "git" && !ok && output_summary == "tool completed"
        ));

        let web = normalize_event(&json!({
            "type":"item.started",
            "item":{"type":"web_search","id":"w1","query":"private query"}
        }))
        .unwrap();
        assert!(matches!(
            &web[0],
            RuntimeEvent::ToolCallStarted { tool, input_summary, .. }
                if tool == "web_search" && input_summary == "tool started"
        ));

        let collaboration = normalize_event(&json!({
            "type":"item.started",
            "item":{"type":"collab_tool_call","id":"sub1","parent_id":"root"}
        }))
        .unwrap();
        assert!(matches!(
            &collaboration[0],
            RuntimeEvent::SubagentStarted { native_id, parent_native_id }
                if native_id == "sub1" && parent_native_id.as_deref() == Some("root")
        ));

        let plan = normalize_event(&json!({
            "type":"item.updated",
            "item":{"type":"todo_list","items":[{"text":"test","status":"in_progress"}]}
        }))
        .unwrap();
        assert!(matches!(
            &plan[0],
            RuntimeEvent::PlanUpdated { items }
                if items.len() == 1 && items[0].text == "test" && items[0].status.as_deref() == Some("in_progress")
        ));

        let usage = normalize_event(&json!({
            "type":"turn.completed",
            "usage":{"input_tokens":1,"cached_input_tokens":2,"output_tokens":3}
        }))
        .unwrap();
        assert!(matches!(
            &usage[0],
            RuntimeEvent::UsageUpdated {
                input_tokens: Some(1),
                cached_input_tokens: Some(2),
                output_tokens: Some(3),
                ..
            }
        ));
    }

    #[test]
    fn maps_file_change_and_fails_closed_for_terminal_errors() {
        let changed = normalize_event(&json!({
            "type":"item.completed",
            "item":{"type":"file_change","changes":[{"path":"src/lib.rs","kind":"create"}]}
        }))
        .unwrap();
        assert!(matches!(
            &changed[0],
            RuntimeEvent::FileChanged { path, change: RuntimeFileChangeKind::Created }
                if path == "src/lib.rs"
        ));
        let error = normalize_event(&json!({"type":"turn.failed","message":"provider failed"}));
        assert!(
            matches!(error, Err(RuntimeError::Protocol(message)) if message == "provider failed")
        );
    }
}
