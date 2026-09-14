//! Stable Codex `exec --json` event normalization.
//!
//! This module deliberately stores only identifiers and bounded, vendor-neutral
//! summaries.  In particular, JSONL command arguments and tool output are not
//! copied into AgentMosaic's durable runtime-event log.

use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use agentmosaic_team::{RuntimeEvent, RuntimeFileChangeKind, RuntimePlanItem};
use serde_json::Value;

use crate::{LaunchSpec, RuntimeError};

/// Shell-free argv for the stable `codex exec --json` machine interface.
///
/// A prompt is supplied on stdin by the supervising driver. Keeping it out of
/// argv prevents it leaking into process listings and makes the verified
/// `exec resume [OPTIONS] SESSION_ID [PROMPT]` ordering explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExecInvocation {
    pub launch: LaunchSpec,
    pub args: Vec<String>,
}

/// Result of one bounded `codex exec --json` process invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExecResult {
    /// Foreign Codex thread id returned before the first item event.
    pub thread_id: String,
    /// The last visible assistant message, bounded by the caller's limit.
    pub final_message: String,
}

/// Run a Codex JSONL invocation under one absolute deadline.
///
/// The caller persists `thread_id` as soon as this function returns it through
/// its event callback.  The process owns a Unix group, so timeout and terminal
/// protocol failure also terminate descendants rather than leaving helpers.
pub fn run_invocation<F>(
    invocation: &CodexExecInvocation,
    working_directory: &std::path::Path,
    prompt: &str,
    timeout: Duration,
    max_final_message_bytes: usize,
    mut emit: F,
) -> Result<CodexExecResult, RuntimeError>
where
    F: FnMut(RuntimeEvent) -> Result<(), RuntimeError>,
{
    if timeout.is_zero() || max_final_message_bytes == 0 {
        return Err(RuntimeError::InvalidConfiguration(
            "Codex exec timeout and final-message limit must be positive".into(),
        ));
    }
    let deadline = Instant::now() + timeout;
    let mut command = Command::new(&invocation.launch.program);
    command
        .args(&invocation.args)
        .current_dir(working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| RuntimeError::Protocol(format!("spawn Codex exec: {error}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RuntimeError::Protocol("Codex exec stdout unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| RuntimeError::Protocol("Codex exec stderr unavailable".into()))?;
    let stderr_reader = std::thread::spawn(move || bounded_stderr(stderr));
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
        .ok_or_else(|| RuntimeError::Protocol("Codex exec stdin unavailable".into()))
        .and_then(|mut stdin| {
            stdin
                .write_all(prompt.as_bytes())
                .map_err(|error| RuntimeError::Protocol(format!("write Codex prompt: {error}")))
        });
    if let Err(error) = write_result {
        terminate_group(&mut child);
        let _ = stdout_reader.join();
        let _ = stderr_reader.join();
        return Err(error);
    }

    let mut thread_id = None;
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
                            "decode Codex JSONL: {error}"
                        )))
                    }
                };
                let events = match normalize_event(&value) {
                    Ok(events) => events,
                    Err(error) => break Err(error),
                };
                let mut emission_error = None;
                for event in events {
                    if let RuntimeEvent::SessionStarted { native_session_id } = &event {
                        thread_id = Some(native_session_id.clone());
                    }
                    if let RuntimeEvent::AssistantMessageCompleted { text } = &event {
                        final_message = truncate_utf8(text, max_final_message_bytes);
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
                break Err(RuntimeError::Protocol(format!("read Codex JSONL: {error}")))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break Err(RuntimeError::TimedOut),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = child
                    .wait()
                    .map_err(|error| RuntimeError::Protocol(format!("wait Codex exec: {error}")))?;
                if status.success() {
                    break thread_id
                        .map(|thread_id| CodexExecResult {
                            thread_id,
                            final_message,
                        })
                        .ok_or_else(|| {
                            RuntimeError::Protocol("Codex exec ended without thread.started".into())
                        });
                }
                break Err(RuntimeError::Protocol(
                    "Codex exec exited unsuccessfully".into(),
                ));
            }
        }
    };
    if outcome.is_err() {
        terminate_group(&mut child);
    }
    let _ = stdout_reader.join();
    let diagnostics = stderr_reader.join().unwrap_or_default();
    outcome.map_err(|error| match error {
        RuntimeError::Protocol(message) if !diagnostics.is_empty() => {
            RuntimeError::Protocol(format!("{message}; Codex stderr: {diagnostics}"))
        }
        error => error,
    })
}

fn bounded_stderr(mut stderr: impl Read) -> String {
    let mut bytes = Vec::new();
    let _ = stderr.by_ref().take(16 * 1024).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).trim().to_owned()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    value
        .char_indices()
        .take_while(|(index, character)| index + character.len_utf8() <= max_bytes)
        .map(|(_, character)| character)
        .collect()
}

fn terminate_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::killpg(child.id() as i32, libc::SIGKILL);
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let _ = child.wait();
}

impl CodexExecInvocation {
    /// Start a fresh Codex thread. Isolation is opt-in because it suppresses
    /// user configuration that can be necessary for the user's authentication
    /// and model provider.
    pub fn start(launch: LaunchSpec, output_schema: Option<&str>, isolate: bool) -> Self {
        let mut args = launch.args.clone();
        args.extend(["exec".into(), "--json".into()]);
        append_options(&mut args, output_schema, isolate);
        Self { launch, args }
    }

    /// Resume an existing Codex thread using the argument order accepted by
    /// the locally verified 0.154 CLI: `exec resume [OPTIONS] SESSION_ID`.
    pub fn resume(
        launch: LaunchSpec,
        thread_id: &str,
        output_schema: Option<&str>,
        isolate: bool,
    ) -> Result<Self, RuntimeError> {
        if thread_id.trim().is_empty() {
            return Err(RuntimeError::Protocol(
                "Codex exec resume requires a non-empty thread id".into(),
            ));
        }
        let mut args = launch.args.clone();
        args.extend(["exec".into(), "resume".into(), "--json".into()]);
        append_options(&mut args, output_schema, isolate);
        args.push(thread_id.into());
        Ok(Self { launch, args })
    }
}

fn append_options(args: &mut Vec<String>, output_schema: Option<&str>, isolate: bool) {
    if isolate {
        args.extend(["--ignore-user-config".into(), "--ignore-rules".into()]);
    }
    if let Some(schema) = output_schema.filter(|schema| !schema.trim().is_empty()) {
        args.extend(["--output-schema".into(), schema.into()]);
    }
}

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

    #[test]
    fn constructs_verified_exec_and_resume_argv_without_prompt() {
        let launch = LaunchSpec::new("codex", vec!["-p".into(), "brain".into()]).unwrap();
        let fresh = CodexExecInvocation::start(launch.clone(), Some("/tmp/schema.json"), false);
        assert_eq!(
            fresh.args,
            [
                "-p",
                "brain",
                "exec",
                "--json",
                "--output-schema",
                "/tmp/schema.json"
            ]
        );
        let resumed = CodexExecInvocation::resume(launch, "thread-1", None, true).unwrap();
        assert_eq!(
            resumed.args,
            [
                "-p",
                "brain",
                "exec",
                "resume",
                "--json",
                "--ignore-user-config",
                "--ignore-rules",
                "thread-1"
            ]
        );
        assert!(CodexExecInvocation::resume(
            LaunchSpec::new("codex", Vec::new()).unwrap(),
            " ",
            None,
            false
        )
        .is_err());
    }

    #[test]
    #[cfg(unix)]
    fn supervised_jsonl_invocation_binds_thread_and_forwards_normalized_events() {
        let invocation = CodexExecInvocation {
            launch: LaunchSpec::new("sh", Vec::new()).unwrap(),
            args: vec![
                "-c".into(),
                "printf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"thread-1\"}' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"done\"}}'"
                    .into(),
            ],
        };
        let mut events = Vec::new();
        let result = run_invocation(
            &invocation,
            std::path::Path::new("."),
            "ignored",
            Duration::from_secs(1),
            32,
            |event| {
                events.push(event);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result.thread_id, "thread-1");
        assert_eq!(result.final_message, "done");
        assert_eq!(events.len(), 2);
    }

    #[test]
    #[cfg(unix)]
    fn supervised_invocation_enforces_an_absolute_deadline() {
        let invocation = CodexExecInvocation {
            launch: LaunchSpec::new("sh", Vec::new()).unwrap(),
            args: vec!["-c".into(), "sleep 5".into()],
        };
        let started = std::time::Instant::now();
        let error = run_invocation(
            &invocation,
            std::path::Path::new("."),
            "ignored",
            Duration::from_millis(50),
            32,
            |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(error, RuntimeError::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
