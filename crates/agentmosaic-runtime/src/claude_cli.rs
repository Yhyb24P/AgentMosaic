//! Verified non-interactive Claude Code invocation and supervision.

use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use agentmosaic_team::{RuntimeEvent, RuntimePermissionDecision, RuntimePermissionOption};
use serde_json::Value;

use crate::{codex_exec, LaunchSpec, RuntimeError};

/// Result of one bounded Claude `stream-json` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCliResult {
    /// Foreign Claude session id observed from its structured event stream.
    pub session_id: String,
    /// The last visible assistant message, bounded by the caller's limit.
    pub final_message: String,
}

/// Run one Claude `stream-json` invocation under an absolute deadline.
///
/// The spawned process owns a Unix group so an error, callback failure, or
/// timeout also reaps any descendant helpers. Raw stream objects, tool input,
/// and tool output are deliberately never sent to the event callback.
pub fn run_invocation<F>(
    invocation: &ClaudeCliInvocation,
    working_directory: &std::path::Path,
    prompt: &str,
    timeout: Duration,
    max_final_message_bytes: usize,
    mut emit: F,
) -> Result<ClaudeCliResult, RuntimeError>
where
    F: FnMut(RuntimeEvent) -> Result<(), RuntimeError>,
{
    if timeout.is_zero() || max_final_message_bytes == 0 {
        return Err(RuntimeError::InvalidConfiguration(
            "Claude timeout and final-message limit must be positive".into(),
        ));
    }
    let deadline = Instant::now() + timeout;
    let mut command = invocation.supervised_command();
    command.current_dir(working_directory);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| RuntimeError::Protocol(format!("spawn Claude: {error}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RuntimeError::Protocol("Claude stdout unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| RuntimeError::Protocol("Claude stderr unavailable".into()))?;
    let stderr_reader = std::thread::spawn(move || codex_exec::bounded_stderr(stderr));
    let (sender, receiver) = mpsc::sync_channel(256);
    let stdout_reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender
                .send(line.map_err(|error| error.to_string()))
                .is_err()
            {
                break;
            }
        }
    });
    let write_result = child
        .stdin
        .take()
        .ok_or_else(|| RuntimeError::Protocol("Claude stdin unavailable".into()))
        .and_then(|mut stdin| {
            stdin
                .write_all(prompt.as_bytes())
                .map_err(|error| RuntimeError::Protocol(format!("write Claude prompt: {error}")))
        });
    if let Err(error) = write_result {
        codex_exec::terminate_group(&mut child);
        let _ = stdout_reader.join();
        let _ = stderr_reader.join();
        return Err(error);
    }

    let mut session_id = None;
    let mut final_message = String::new();
    let outcome = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break Err(RuntimeError::TimedOut);
        }
        match receiver.recv_timeout(remaining) {
            Ok(Ok(line)) => {
                let value: Value = match serde_json::from_str(&line) {
                    Ok(value) => value,
                    Err(error) => {
                        break Err(RuntimeError::Protocol(format!(
                            "decode Claude JSONL: {error}"
                        )))
                    }
                };
                let events = match normalize_stream_event(&value) {
                    Ok(events) => events,
                    Err(error) => break Err(error),
                };
                let mut emission_error = None;
                for event in events {
                    if let RuntimeEvent::SessionStarted { native_session_id } = &event {
                        session_id = Some(native_session_id.clone());
                    }
                    if let RuntimeEvent::AssistantMessageCompleted { text } = &event {
                        final_message = codex_exec::truncate_utf8(text, max_final_message_bytes);
                    }
                    if let Err(error) = emit(event) {
                        emission_error = Some(error);
                        break;
                    }
                }
                if let Some(error) = emission_error {
                    break Err(error);
                }
            }
            Ok(Err(error)) => {
                break Err(RuntimeError::Protocol(format!(
                    "read Claude JSONL: {error}"
                )))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break Err(RuntimeError::TimedOut),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = child
                    .wait()
                    .map_err(|error| RuntimeError::Protocol(format!("wait Claude: {error}")))?;
                if status.success() {
                    break session_id
                        .map(|session_id| ClaudeCliResult {
                            session_id,
                            final_message,
                        })
                        .ok_or_else(|| {
                            RuntimeError::Protocol("Claude ended without session id".into())
                        });
                }
                break Err(RuntimeError::Protocol(
                    "Claude exited unsuccessfully".into(),
                ));
            }
        }
    };
    if outcome.is_err() {
        codex_exec::terminate_group(&mut child);
    }
    let _ = stdout_reader.join();
    let diagnostics = stderr_reader.join().unwrap_or_default();
    outcome.map_err(|error| match error {
        RuntimeError::Protocol(message) if !diagnostics.is_empty() => {
            RuntimeError::Protocol(format!("{message}; Claude stderr: {diagnostics}"))
        }
        error => error,
    })
}

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
    if kind == "stream_event" {
        // Claude wraps partial Anthropic message frames in `stream_event`.
        // Only visible text deltas cross the runtime boundary: thinking and
        // signatures remain private even when the CLI elects to stream them.
        return Ok(value
            .pointer("/event/delta")
            .filter(|delta| delta.get("type").and_then(Value::as_str) == Some("text_delta"))
            .and_then(|delta| delta.get("text").and_then(Value::as_str))
            .map(|text| RuntimeEvent::AssistantMessageDelta { text: text.into() })
            .into_iter()
            .collect());
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
        "permission_request" => value
            .get("request_id")
            .or_else(|| value.get("tool_use_id"))
            .and_then(Value::as_str)
            .map(|request_id| RuntimeEvent::PermissionRequested {
                request_id: request_id.into(),
                action: value
                    .get("tool_name")
                    .or_else(|| value.get("action"))
                    .and_then(Value::as_str)
                    .unwrap_or("runtime action")
                    .into(),
                options: value
                    .get("options")
                    .and_then(Value::as_array)
                    .map(|options| {
                        options
                            .iter()
                            .filter_map(|option| {
                                option.get("id").and_then(Value::as_str).map(|id| {
                                    RuntimePermissionOption {
                                        option_id: id.into(),
                                        label: option
                                            .get("label")
                                            .and_then(Value::as_str)
                                            .unwrap_or("option")
                                            .into(),
                                    }
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }),
        "permission_denied" => value
            .get("request_id")
            .or_else(|| value.get("tool_use_id"))
            .and_then(Value::as_str)
            .map(|request_id| RuntimeEvent::PermissionResolved {
                request_id: request_id.into(),
                decision: RuntimePermissionDecision::Denied,
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
    let mut events = event.into_iter().collect::<Vec<_>>();
    if let Some(RuntimeEvent::ToolCallStarted { native_call_id, .. }) = events.first() {
        if let Some(parent_native_id) = value
            .pointer("/message/content/0/parent_tool_use_id")
            .and_then(Value::as_str)
        {
            events.push(RuntimeEvent::SubagentStarted {
                native_id: native_call_id.clone(),
                parent_native_id: Some(parent_native_id.into()),
            });
        }
    }
    Ok(events)
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

    /// Create the constrained process command consumed by the supervisor.
    pub fn supervised_command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.launch.program);
        self.apply_to(&mut command);
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        command
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
        let supervised = invocation.supervised_command();
        assert_eq!(supervised.get_args().count(), invocation.args.len());
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

    #[test]
    fn stream_json_maps_permission_and_subagent_topology_without_payloads() {
        let permission = normalize_stream_event(&json!({
            "type":"permission_request", "request_id":"permit-1", "tool_name":"Bash",
            "options":[{"id":"deny","label":"Deny"}]
        }))
        .unwrap();
        assert!(
            matches!(&permission[0], RuntimeEvent::PermissionRequested { request_id, action, options } if request_id == "permit-1" && action == "Bash" && options.len() == 1)
        );
        assert!(matches!(
            normalize_stream_event(&json!({"type":"permission_denied","tool_use_id":"call-1"}))
                .unwrap()[0],
            RuntimeEvent::PermissionResolved {
                decision: RuntimePermissionDecision::Denied,
                ..
            }
        ));
        let topology = normalize_stream_event(&json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"child","name":"Task","parent_tool_use_id":"parent","input":{"secret":"drop"}}]}})).unwrap();
        assert!(
            matches!(&topology[0], RuntimeEvent::ToolCallStarted { input_summary, .. } if input_summary == "tool started")
        );
        assert!(
            matches!(&topology[1], RuntimeEvent::SubagentStarted { native_id, parent_native_id: Some(parent) } if native_id == "child" && parent == "parent")
        );
    }

    #[test]
    fn nested_stream_events_forward_only_visible_text_deltas() {
        let text = normalize_stream_event(&json!({
            "type":"stream_event",
            "event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"visible"}}
        }))
        .unwrap();
        assert!(
            matches!(&text[0], RuntimeEvent::AssistantMessageDelta { text } if text == "visible")
        );
        assert!(normalize_stream_event(&json!({
            "type":"stream_event",
            "event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"private"}}
        }))
        .unwrap()
        .is_empty());
    }

    #[test]
    fn supervisor_collects_stream_events_without_a_shell() {
        let invocation = ClaudeCliInvocation {
            launch: LaunchSpec::new("sh", Vec::new()).unwrap(),
            args: vec![
                "-c".into(),
                "cat >/dev/null; printf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"session-1\"}' '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}'"
                    .into(),
            ],
        };
        let mut events = Vec::new();
        let result = run_invocation(
            &invocation,
            std::path::Path::new("."),
            "ignored",
            Duration::from_secs(1),
            128,
            |event| {
                events.push(event);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result.session_id, "session-1");
        assert_eq!(result.final_message, "done");
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn supervisor_enforces_an_absolute_deadline() {
        let invocation = ClaudeCliInvocation {
            launch: LaunchSpec::new("sh", Vec::new()).unwrap(),
            args: vec!["-c".into(), "sleep 2".into()],
        };
        let error = run_invocation(
            &invocation,
            std::path::Path::new("."),
            "ignored",
            Duration::from_millis(30),
            128,
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(matches!(error, RuntimeError::TimedOut));
    }

    #[test]
    #[cfg(unix)]
    fn deadline_reaps_the_claude_process_group_and_its_grandchild() {
        // The Claude worker has no protocol-level cancel, so its bounded
        // termination is the process group it owns: a deadline must kill the
        // CLI *and* the helpers it spawned, leaving no orphan behind.
        let directory = std::env::temp_dir().join(format!("am_claude_reap_{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let pid_file = directory.join("grandchild.pid");
        // The wrapper returns on its own and the grandchild keeps no inherited
        // pipe, so the only way the grandchild can disappear inside the window
        // below is the supervisor terminating the whole process group.
        let script = format!(
            "sleep 300 >/dev/null 2>&1 & echo $! > '{}'; sleep 2",
            pid_file.display()
        );
        let invocation = ClaudeCliInvocation {
            launch: LaunchSpec::new("sh", Vec::new()).unwrap(),
            args: vec!["-c".into(), script],
        };
        let error = run_invocation(
            &invocation,
            &directory,
            "ignored",
            Duration::from_millis(300),
            128,
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(matches!(error, RuntimeError::TimedOut));

        let grandchild =
            std::fs::read_to_string(&pid_file).expect("the wrapper recorded its grandchild pid");
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while process_is_alive(grandchild.trim()) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            !process_is_alive(grandchild.trim()),
            "Claude timeout left grandchild {} alive",
            grandchild.trim()
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    fn process_is_alive(pid: &str) -> bool {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}
