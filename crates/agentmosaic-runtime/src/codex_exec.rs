//! Stable Codex `exec --json` event normalization.

use agentmosaic_team::{RuntimeEvent, RuntimeFileChangeKind};
use serde_json::Value;

use crate::RuntimeError;

/// Decode one JSONL event without retaining raw command input/output.
pub fn normalize_event(value: &Value) -> Result<Vec<RuntimeEvent>, RuntimeError> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| RuntimeError::Protocol("Codex exec event missing type".into()))?;
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
                RuntimeEvent::ToolCallStarted {
                    native_call_id: id.into(),
                    tool: "command".into(),
                    input_summary: "command started".into(),
                }
            })
        }
        "item.completed"
            if value.pointer("/item/type").and_then(Value::as_str) == Some("command_execution") =>
        {
            value.pointer("/item/id").and_then(Value::as_str).map(|id| {
                RuntimeEvent::ToolCallCompleted {
                    native_call_id: id.into(),
                    tool: "command".into(),
                    ok: value.pointer("/item/exit_code").and_then(Value::as_i64) == Some(0),
                    output_summary: "command completed".into(),
                }
            })
        }
        "item.completed"
            if value.pointer("/item/type").and_then(Value::as_str) == Some("file_change") =>
        {
            value
                .pointer("/item/path")
                .and_then(Value::as_str)
                .map(|path| RuntimeEvent::FileChanged {
                    path: path.into(),
                    change: RuntimeFileChangeKind::Modified,
                })
        }
        _ => None,
    };
    Ok(event.into_iter().collect())
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
            matches!(&command[0], RuntimeEvent::ToolCallStarted { input_summary, .. } if input_summary == "command started")
        );
    }
}
