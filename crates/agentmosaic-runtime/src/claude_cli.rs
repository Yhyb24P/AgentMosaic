//! Verified non-interactive Claude Code invocation construction.

use agentmosaic_team::RuntimeEvent;
use serde_json::Value;

use crate::{LaunchSpec, RuntimeError};

/// Normalize one supported Claude `stream-json` object without retaining raw
/// thinking, tool input, or tool result output.
pub fn normalize_stream_event(value: &Value) -> Result<Vec<RuntimeEvent>, RuntimeError> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| RuntimeError::Protocol("Claude stream event missing type".into()))?;
    if matches!(kind, "error" | "error_during_execution") {
        return Err(RuntimeError::Protocol(
            value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Claude execution failed")
                .into(),
        ));
    }
    if kind == "result" {
        let mut events = Vec::new();
        if let Some(id) = value.get("session_id").and_then(Value::as_str) {
            events.push(RuntimeEvent::SessionStarted {
                native_session_id: id.into(),
            });
        }
        if value.get("usage").is_some() || value.get("total_cost_usd").is_some() {
            events.push(RuntimeEvent::UsageUpdated {
                input_tokens: value.pointer("/usage/input_tokens").and_then(Value::as_u64),
                cached_input_tokens: value
                    .pointer("/usage/cache_read_input_tokens")
                    .and_then(Value::as_u64),
                output_tokens: value
                    .pointer("/usage/output_tokens")
                    .and_then(Value::as_u64),
                reasoning_tokens: None,
                estimated_cost_usd: value.get("total_cost_usd").and_then(Value::as_f64),
            });
        }
        return Ok(events);
    }
    let event = match kind {
        "retry" | "warning" => Some(RuntimeEvent::RuntimeWarning {
            code: value
                .get("subtype")
                .or_else(|| value.get("code"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Claude runtime warning")
                .into(),
        }),
        "assistant" => value
            .pointer("/message/content")
            .and_then(Value::as_array)
            .and_then(|content| {
                content
                    .iter()
                    .find_map(|block| match block.get("type").and_then(Value::as_str) {
                        Some("text") => block.get("text").and_then(Value::as_str).map(|text| {
                            RuntimeEvent::AssistantMessageCompleted { text: text.into() }
                        }),
                        Some("tool_use") => block.get("id").and_then(Value::as_str).map(|id| {
                            RuntimeEvent::ToolCallStarted {
                                native_call_id: id.into(),
                                tool: block
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or("tool")
                                    .into(),
                                input_summary: "tool started".into(),
                            }
                        }),
                        _ => None,
                    })
            }),
        "user" => value
            .pointer("/message/content")
            .and_then(Value::as_array)
            .and_then(|content| {
                content.iter().find_map(|block| {
                    (block.get("type").and_then(Value::as_str) == Some("tool_result")).then(|| {
                        RuntimeEvent::ToolCallCompleted {
                            native_call_id: block
                                .get("tool_use_id")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown")
                                .into(),
                            tool: "tool".into(),
                            ok: !block
                                .get("is_error")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            output_summary: "tool completed".into(),
                        }
                    })
                })
            }),
        "system" if value.get("subtype").and_then(Value::as_str) == Some("init") => value
            .get("session_id")
            .and_then(Value::as_str)
            .map(|id| RuntimeEvent::SessionStarted {
                native_session_id: id.into(),
            }),
        _ => None,
    };
    Ok(event.into_iter().collect())
}

/// Shell-free argv for Claude Code 2.1.268's supported stream-json surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCliInvocation {
    pub launch: LaunchSpec,
    pub args: Vec<String>,
}

impl ClaudeCliInvocation {
    /// Apply the already-tokenized invocation to a process command. This is
    /// the sole shell-free bridge used by the future supervisor.
    pub fn apply_to(&self, command: &mut std::process::Command) {
        command.args(&self.args);
    }

    /// Create a fresh isolated non-interactive session. Prompt text is passed
    /// over stdin, never in argv.
    pub fn start(launch: LaunchSpec, json_schema: Option<&str>) -> Self {
        let mut args = base_args(&launch);
        append_schema(&mut args, json_schema);
        Self { launch, args }
    }

    /// Resume one exact foreign session with the same security boundary.
    pub fn resume(
        launch: LaunchSpec,
        session_id: &str,
        json_schema: Option<&str>,
    ) -> Result<Self, RuntimeError> {
        if session_id.trim().is_empty() {
            return Err(RuntimeError::Protocol(
                "Claude resume requires a non-empty session id".into(),
            ));
        }
        let mut args = base_args(&launch);
        args.extend(["--resume".into(), session_id.into()]);
        append_schema(&mut args, json_schema);
        Ok(Self { launch, args })
    }
}

fn base_args(launch: &LaunchSpec) -> Vec<String> {
    let mut args = launch.args.clone();
    args.extend([
        "--bare".into(),
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
        "--permission-mode".into(),
        "dontAsk".into(),
        "--permission-prompts".into(),
        "none".into(),
    ]);
    args
}

fn append_schema(args: &mut Vec<String>, schema: Option<&str>) {
    if let Some(schema) = schema.filter(|schema| !schema.trim().is_empty()) {
        args.extend(["--json-schema".into(), schema.into()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn invocation_uses_only_verified_machine_flags() {
        let launch = LaunchSpec::new("claude", vec!["--model".into(), "sonnet".into()]).unwrap();
        let invocation = ClaudeCliInvocation::resume(launch, "session-1", Some("{}")).unwrap();
        assert_eq!(invocation.args[0..2], ["--model", "sonnet"]);
        assert!(invocation
            .args
            .windows(2)
            .any(|part| part == ["--output-format", "stream-json"]));
        assert!(invocation
            .args
            .windows(2)
            .any(|part| part == ["--resume", "session-1"]));
        assert!(invocation.args.contains(&"--bare".into()));
        let mut command = std::process::Command::new(&invocation.launch.program);
        invocation.apply_to(&mut command);
        assert_eq!(command.get_args().count(), invocation.args.len());
        assert!(ClaudeCliInvocation::resume(
            LaunchSpec::new("claude", Vec::new()).unwrap(),
            " ",
            None
        )
        .is_err());
    }

    #[test]
    fn stream_json_drops_thinking_and_raw_tool_data() {
        assert!(matches!(
            normalize_stream_event(&json!({"type":"system","subtype":"init","session_id":"s"}))
                .unwrap()[0],
            RuntimeEvent::SessionStarted { .. }
        ));
        let tool = normalize_stream_event(&json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"call","name":"Bash","input":{"secret":"no"}}]}})).unwrap();
        assert!(
            matches!(&tool[0], RuntimeEvent::ToolCallStarted { input_summary, .. } if input_summary == "tool started")
        );
        assert!(normalize_stream_event(&json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":"private"}]}})).unwrap().is_empty());
        let result = normalize_stream_event(&json!({
            "type":"result",
            "session_id":"s",
            "usage":{"input_tokens":1,"cache_read_input_tokens":2,"output_tokens":3},
            "total_cost_usd":0.1
        }))
        .unwrap();
        assert!(matches!(result[0], RuntimeEvent::SessionStarted { .. }));
        assert!(matches!(
            result[1],
            RuntimeEvent::UsageUpdated {
                input_tokens: Some(1),
                cached_input_tokens: Some(2),
                output_tokens: Some(3),
                estimated_cost_usd: Some(cost),
                ..
            } if (cost - 0.1).abs() < f64::EPSILON
        ));
        assert!(matches!(
            normalize_stream_event(&json!({"type":"warning","code":"retrying","message":"try again"})).unwrap()[0],
            RuntimeEvent::RuntimeWarning { code: Some(ref code), .. } if code == "retrying"
        ));
    }
}
