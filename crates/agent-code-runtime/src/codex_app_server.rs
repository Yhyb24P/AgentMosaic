//! Minimal stdio JSON-RPC transport for the locally probed Codex app-server.
//! Native IDs stay external; callers persist them through the team board.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

/// Upper bound on notifications retained while a correlated request is pending.
const EVENT_QUEUE_CAPACITY: usize = 256;

/// Bounded byte limit for a turn's extracted final visible agent message.
pub const DEFAULT_FINAL_MESSAGE_MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexBridgeEvent {
    Notification(String),
    ToolCall {
        request_id: Value,
        call_id: String,
        tool: String,
    },
    TurnCompleted {
        thread_id: String,
        turn_id: String,
    },
    McpElicitation {
        request_id: Value,
        server_name: String,
    },
}

#[derive(Debug)]
pub enum CodexBridgeError {
    Io(String),
    Protocol(String),
    Closed,
}

impl std::fmt::Display for CodexBridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for CodexBridgeError {}

/// A bounded client for `codex app-server --stdio`. It deliberately only
/// understands transport facts and the two allowlisted collaboration tools.
pub struct CodexAppServer {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    /// Notifications observed while a correlated request was pending. They are
    /// replayed by `next_event` so a racing `turn/completed` is never lost.
    pending_events: VecDeque<Value>,
}

impl CodexAppServer {
    pub fn spawn(executable: &str) -> Result<Self, CodexBridgeError> {
        Self::spawn_with_overrides(executable, &[])
    }

    pub fn spawn_with_overrides(
        executable: &str,
        overrides: &[String],
    ) -> Result<Self, CodexBridgeError> {
        let mut command = Command::new(executable);
        command.args(["app-server", "--stdio"]);
        for override_value in overrides {
            command.args(["-c", override_value]);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CodexBridgeError::Io(e.to_string()))?;
        Ok(Self {
            stdin: child.stdin.take().ok_or(CodexBridgeError::Closed)?,
            stdout: BufReader::new(child.stdout.take().ok_or(CodexBridgeError::Closed)?),
            child,
            next_id: 1,
            pending_events: VecDeque::new(),
        })
    }

    pub fn initialize(&mut self, name: &str, version: &str) -> Result<Value, CodexBridgeError> {
        let result = self.request("initialize", json!({"clientInfo":{"name":name,"version":version},"capabilities":{"experimentalApi":true}}))?;
        self.notify("initialized", json!({}))?;
        Ok(result)
    }

    pub fn start_thread(&mut self, cwd: &str) -> Result<String, CodexBridgeError> {
        self.start_thread_with_developer_instructions(cwd, None)
    }

    pub fn start_thread_with_developer_instructions(
        &mut self,
        cwd: &str,
        developer_instructions: Option<&str>,
    ) -> Result<String, CodexBridgeError> {
        self.start_thread_with_options(cwd, developer_instructions, "workspace-write", "on-request")
    }

    /// Start a thread with explicit sandbox and approval policy. A thread that
    /// only reasons (the team Lead) uses `read-only` + `never`; a thread that
    /// edits the workspace uses `workspace-write` + `on-request`.
    pub fn start_thread_with_options(
        &mut self,
        cwd: &str,
        developer_instructions: Option<&str>,
        sandbox: &str,
        approval_policy: &str,
    ) -> Result<String, CodexBridgeError> {
        let result = self.request(
            "thread/start",
            json!({
                "cwd":cwd,
                "sandbox":sandbox,
                "approvalPolicy":approval_policy,
                "developerInstructions":developer_instructions,
            }),
        )?;
        result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                CodexBridgeError::Protocol("thread/start response missing thread.id".into())
            })
    }

    pub fn start_turn(&mut self, thread_id: &str, text: &str) -> Result<String, CodexBridgeError> {
        let result = self.request(
            "turn/start",
            json!({"threadId":thread_id,"input":[{"type":"text","text":text}]}),
        )?;
        result
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| CodexBridgeError::Protocol("turn/start response missing turn.id".into()))
    }

    /// Rejoin an existing external Codex thread after an app-server restart.
    /// The caller must still reconcile the returned native reference against
    /// the canonical team task/run binding; a Codex thread is never task state.
    pub fn resume_thread(&mut self, thread_id: &str) -> Result<String, CodexBridgeError> {
        let result = self.request("thread/resume", json!({"threadId":thread_id}))?;
        result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                CodexBridgeError::Protocol("thread/resume response missing thread.id".into())
            })
    }

    pub fn mcp_status(&mut self, thread_id: &str) -> Result<Value, CodexBridgeError> {
        self.request(
            "mcpServerStatus/list",
            json!({"threadId":thread_id,"detail":"full"}),
        )
    }

    /// Returns protocol items for audit classification only. Callers must not
    /// persist raw item content into ACC state.
    pub fn thread_items(
        &mut self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<Value, CodexBridgeError> {
        self.request(
            "thread/items/list",
            json!({"threadId":thread_id,"turnId":turn_id,"limit":100}),
        )
    }

    /// Read the exact turn snapshot and return its bounded final visible agent
    /// text. Fails closed when the turn is not completed or carries no visible
    /// agent message.
    pub fn final_agent_message(
        &mut self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<String, CodexBridgeError> {
        let snapshot = self.request(
            "thread/read",
            json!({"threadId": thread_id, "includeTurns": true}),
        )?;
        select_final_agent_message(&snapshot, turn_id, DEFAULT_FINAL_MESSAGE_MAX_BYTES)
    }

    pub fn next_event(&mut self) -> Result<CodexBridgeEvent, CodexBridgeError> {
        if let Some(value) = self.pending_events.pop_front() {
            return Self::interpret_event(value);
        }
        let value = self.read_value()?;
        Self::interpret_event(value)
    }

    fn interpret_event(value: Value) -> Result<CodexBridgeEvent, CodexBridgeError> {
        if let Some(method) = value.get("method").and_then(Value::as_str) {
            if method == "mcpServer/elicitation/request" {
                let p = value.get("params").ok_or_else(|| {
                    CodexBridgeError::Protocol("elicitation missing params".into())
                })?;
                return Ok(CodexBridgeEvent::McpElicitation {
                    request_id: value.get("id").cloned().ok_or_else(|| {
                        CodexBridgeError::Protocol("elicitation missing request id".into())
                    })?,
                    server_name: p
                        .get("serverName")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                });
            }
            if method == "item/tool/call" {
                let p = value
                    .get("params")
                    .ok_or_else(|| CodexBridgeError::Protocol("tool call missing params".into()))?;
                let call_id = p
                    .get("callId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CodexBridgeError::Protocol("tool call missing callId".into()))?
                    .to_owned();
                let tool = p
                    .get("tool")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CodexBridgeError::Protocol("tool call missing tool".into()))?
                    .to_owned();
                return Ok(CodexBridgeEvent::ToolCall {
                    request_id: value.get("id").cloned().ok_or_else(|| {
                        CodexBridgeError::Protocol("tool call missing request id".into())
                    })?,
                    call_id,
                    tool,
                });
            }
            if method == "turn/completed" {
                let p = value
                    .get("params")
                    .ok_or_else(|| CodexBridgeError::Protocol("completed missing params".into()))?;
                return Ok(CodexBridgeEvent::TurnCompleted {
                    thread_id: p
                        .get("threadId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    turn_id: completed_turn_id(p),
                });
            }
            return Ok(CodexBridgeEvent::Notification(method.to_owned()));
        }
        Err(CodexBridgeError::Protocol(
            "unexpected response while awaiting event".into(),
        ))
    }

    pub fn respond_tool(
        &mut self,
        request_id: Value,
        success: bool,
        text: &str,
    ) -> Result<(), CodexBridgeError> {
        self.write_value(&json!({"jsonrpc":"2.0","id":request_id,"result":{"success":success,"contentItems":[{"type":"inputText","text":text}]}}))
    }

    /// Responds only to the configured, bounded RAS MCP bridge. This is not
    /// a shell, admin, or general approval path.
    pub fn respond_ras_elicitation(
        &mut self,
        request_id: Value,
        server_name: &str,
    ) -> Result<(), CodexBridgeError> {
        if server_name != "ras" {
            return Err(CodexBridgeError::Protocol(
                "refusing elicitation for a non-RAS MCP server".into(),
            ));
        }
        self.write_value(&json!({"jsonrpc":"2.0","id":request_id,"result":{"action":"accept"}}))
    }

    pub fn interrupt(&mut self, thread_id: &str, turn_id: &str) -> Result<(), CodexBridgeError> {
        // Local schema declares an empty object response. The observed
        // app-server response is an acknowledgement with no material result,
        // so this path validates correlation/error rather than requiring a
        // payload that the operation does not define.
        self.request_ack(
            "turn/interrupt",
            json!({"threadId":thread_id,"turnId":turn_id}),
        )?;
        Ok(())
    }

    pub fn close(mut self) -> Result<(), CodexBridgeError> {
        let _ = self.child.kill();
        self.child
            .wait()
            .map_err(|e| CodexBridgeError::Io(e.to_string()))?;
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, CodexBridgeError> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_value(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        loop {
            let value = self.read_value()?;
            if value.get("id") == Some(&json!(id)) {
                return value.get("result").cloned().ok_or_else(|| {
                    CodexBridgeError::Protocol(format!("{method} returned no result"))
                });
            }
            // Notifications before a correlated response carry no response
            // payload, but they may carry a durable event (e.g. a racing
            // `turn/completed`). Queue them for `next_event` instead of
            // dropping them.
            if value.get("method").is_some() {
                self.queue_notification(value);
            }
        }
    }

    /// Send a request whose local wire schema has no meaningful result body.
    fn request_ack(&mut self, method: &str, params: Value) -> Result<(), CodexBridgeError> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_value(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        loop {
            let value = self.read_value()?;
            if value.get("id") == Some(&json!(id)) {
                if let Some(error) = value.get("error") {
                    return Err(CodexBridgeError::Protocol(format!(
                        "{method} returned error {error}"
                    )));
                }
                return Ok(());
            }
            // As in `request`: retain notifications so a correlated
            // acknowledgement cannot swallow a durable event.
            if value.get("method").is_some() {
                self.queue_notification(value);
            }
        }
    }

    /// Retain a notification frame for `next_event`. The queue is bounded; the
    /// oldest notification is dropped on overflow because notifications carry
    /// no durable payload of their own.
    fn queue_notification(&mut self, value: Value) {
        if self.pending_events.len() >= EVENT_QUEUE_CAPACITY {
            self.pending_events.pop_front();
        }
        self.pending_events.push_back(value);
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), CodexBridgeError> {
        self.write_value(&json!({"jsonrpc":"2.0","method":method,"params":params}))
    }
    fn write_value(&mut self, value: &Value) -> Result<(), CodexBridgeError> {
        writeln!(self.stdin, "{value}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| CodexBridgeError::Io(e.to_string()))
    }
    fn read_value(&mut self) -> Result<Value, CodexBridgeError> {
        let mut line = String::new();
        if self
            .stdout
            .read_line(&mut line)
            .map_err(|e| CodexBridgeError::Io(e.to_string()))?
            == 0
        {
            return Err(CodexBridgeError::Closed);
        }
        serde_json::from_str(&line)
            .map_err(|_| CodexBridgeError::Protocol("malformed JSON-RPC message".into()))
    }
}

/// The turn id carried by a `turn/completed` notification.
///
/// The pinned upstream protocol (`codex-cli 0.154.0`) sends a full `Turn`
/// object: `params.turn.id`. A flat `params.turnId` is still accepted so a
/// simplified peer stays readable, but the upstream shape wins.
fn completed_turn_id(params: &Value) -> String {
    params
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .or_else(|| params.get("turnId").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

/// Select a turn's bounded final visible agent text from a `thread/read`
/// response value. Pure so the selection rule is unit-testable without a live
/// runtime; every failure path is fail-closed.
pub fn select_final_agent_message(
    snapshot: &Value,
    turn_id: &str,
    max_bytes: usize,
) -> Result<String, CodexBridgeError> {
    let turns = snapshot
        .pointer("/thread/turns")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            CodexBridgeError::Protocol("thread/read response missing thread.turns".into())
        })?;
    let turn = turns
        .iter()
        .find(|turn| turn.get("id").and_then(Value::as_str) == Some(turn_id))
        .ok_or_else(|| {
            CodexBridgeError::Protocol(format!("thread/read response has no turn {turn_id}"))
        })?;
    let status = turn.get("status").and_then(Value::as_str).ok_or_else(|| {
        CodexBridgeError::Protocol(format!("thread/read turn {turn_id} is missing status"))
    })?;
    if status != "completed" {
        return Err(CodexBridgeError::Protocol(format!(
            "thread/read turn {turn_id} is not completed (status={status})"
        )));
    }
    let items = turn.get("items").and_then(Value::as_array).ok_or_else(|| {
        CodexBridgeError::Protocol(format!("thread/read turn {turn_id} is missing items"))
    })?;
    let candidates: Vec<(&str, bool)> = items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("agentMessage"))
        .filter(|item| item.get("delivery").and_then(Value::as_str) != Some("async"))
        .filter_map(|item| {
            let text = item.get("text").and_then(Value::as_str)?;
            if text.is_empty() {
                return None;
            }
            let is_final = item.get("phase").and_then(Value::as_str) == Some("finalAnswer");
            Some((text, is_final))
        })
        .collect();
    let selected = candidates
        .iter()
        .rev()
        .find(|(_, is_final)| *is_final)
        .or_else(|| candidates.last())
        .ok_or_else(|| {
            CodexBridgeError::Protocol(format!(
                "thread/read turn {turn_id} has no visible agent message"
            ))
        })?;
    Ok(bound_utf8(selected.0, max_bytes))
}

/// Truncate to at most `max_bytes` without splitting a UTF-8 character.
fn bound_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str, text: &str, phase: Option<&str>, delivery: Option<&str>) -> Value {
        json!({
            "type": "agentMessage",
            "id": id,
            "text": text,
            "phase": phase,
            "delivery": delivery,
        })
    }

    fn snapshot(turns: Value) -> Value {
        json!({"thread": {"id": "thread-1", "turns": turns}})
    }

    fn completed_turn(id: &str, items: Value) -> Value {
        json!({"id": id, "status": "completed", "items": items})
    }

    #[test]
    fn turn_completed_reads_the_upstream_turn_object() {
        // The pinned codex 0.154.0 shape: `params.turn` is a full Turn object.
        let upstream = json!({
            "method": "turn/completed",
            "params": {
                "threadId": "thread-1",
                "turn": {"id": "turn-7", "status": "completed", "items": []},
            },
        });
        match CodexAppServer::interpret_event(upstream).unwrap() {
            CodexBridgeEvent::TurnCompleted { thread_id, turn_id } => {
                assert_eq!(thread_id, "thread-1");
                assert_eq!(turn_id, "turn-7");
            }
            other => panic!("expected TurnCompleted, got {other:?}"),
        }
        // A flat `turnId` is still accepted for a simplified peer.
        let flat = json!({
            "method": "turn/completed",
            "params": {"threadId": "thread-1", "turnId": "turn-8"},
        });
        match CodexAppServer::interpret_event(flat).unwrap() {
            CodexBridgeEvent::TurnCompleted { turn_id, .. } => assert_eq!(turn_id, "turn-8"),
            other => panic!("expected TurnCompleted, got {other:?}"),
        }
        // The upstream object wins when both are present.
        let both = json!({
            "method": "turn/completed",
            "params": {
                "threadId": "thread-1",
                "turnId": "stale",
                "turn": {"id": "turn-9", "status": "completed", "items": []},
            },
        });
        match CodexAppServer::interpret_event(both).unwrap() {
            CodexBridgeEvent::TurnCompleted { turn_id, .. } => assert_eq!(turn_id, "turn-9"),
            other => panic!("expected TurnCompleted, got {other:?}"),
        }
    }

    #[test]
    fn selects_exact_turn_and_ignores_earlier_turns() {
        let snapshot = snapshot(json!([
            completed_turn(
                "turn-0",
                json!([agent("m0", "old", Some("finalAnswer"), None)])
            ),
            completed_turn(
                "turn-1",
                json!([agent("m1", "new", Some("finalAnswer"), None)])
            ),
        ]));
        assert_eq!(
            select_final_agent_message(&snapshot, "turn-1", 1024).unwrap(),
            "new"
        );
    }

    #[test]
    fn ignores_reasoning_items() {
        let snapshot = snapshot(json!([completed_turn(
            "turn-1",
            json!([
                {"type": "reasoning", "id": "r1", "text": "hidden chain"},
                agent("m1", "visible", Some("finalAnswer"), None),
            ])
        )]));
        assert_eq!(
            select_final_agent_message(&snapshot, "turn-1", 1024).unwrap(),
            "visible"
        );
    }

    #[test]
    fn prefers_final_answer_over_later_commentary() {
        let snapshot = snapshot(json!([completed_turn(
            "turn-1",
            json!([
                agent("m1", "answer", Some("finalAnswer"), None),
                agent("m2", "later note", Some("commentary"), None),
            ])
        )]));
        assert_eq!(
            select_final_agent_message(&snapshot, "turn-1", 1024).unwrap(),
            "answer"
        );
    }

    #[test]
    fn falls_back_to_last_non_empty_message_without_final_answer() {
        let snapshot = snapshot(json!([completed_turn(
            "turn-1",
            json!([
                agent("m1", "first", Some("commentary"), None),
                agent("m2", "", Some("commentary"), None),
                agent("m3", "last", None, None),
            ])
        )]));
        assert_eq!(
            select_final_agent_message(&snapshot, "turn-1", 1024).unwrap(),
            "last"
        );
    }

    #[test]
    fn skips_async_delivery_messages() {
        let snapshot = snapshot(json!([completed_turn(
            "turn-1",
            json!([
                agent("m1", "async noise", Some("finalAnswer"), Some("async")),
                agent("m2", "sync answer", Some("commentary"), None),
            ])
        )]));
        assert_eq!(
            select_final_agent_message(&snapshot, "turn-1", 1024).unwrap(),
            "sync answer"
        );
    }

    #[test]
    fn non_completed_turn_fails_closed() {
        for status in ["interrupted", "failed", "inProgress"] {
            let snapshot = snapshot(json!([{
                "id": "turn-1",
                "status": status,
                "items": [agent("m1", "text", Some("finalAnswer"), None)],
            }]));
            assert!(select_final_agent_message(&snapshot, "turn-1", 1024).is_err());
        }
    }

    #[test]
    fn unknown_turn_id_fails_closed() {
        let snapshot = snapshot(json!([completed_turn(
            "turn-1",
            json!([agent("m1", "text", Some("finalAnswer"), None)])
        )]));
        assert!(select_final_agent_message(&snapshot, "turn-2", 1024).is_err());
    }

    #[test]
    fn turn_without_agent_message_fails_closed() {
        let snapshot = snapshot(json!([completed_turn(
            "turn-1",
            json!([{"type": "reasoning", "id": "r1", "text": "hidden"}])
        )]));
        assert!(select_final_agent_message(&snapshot, "turn-1", 1024).is_err());
    }

    #[test]
    fn missing_turns_fails_closed() {
        let snapshot = json!({"thread": {"id": "thread-1"}});
        assert!(select_final_agent_message(&snapshot, "turn-1", 1024).is_err());
    }

    #[test]
    fn long_text_is_bounded_on_a_char_boundary() {
        let text = "é".repeat(10); // two bytes per character
        let snapshot = snapshot(json!([completed_turn(
            "turn-1",
            json!([agent("m1", &text, Some("finalAnswer"), None)])
        )]));
        let bounded = select_final_agent_message(&snapshot, "turn-1", 5).unwrap();
        assert_eq!(bounded, "éé");
        assert_eq!(bounded.len(), 4);
        assert!(text.starts_with(&bounded));
        assert_eq!(
            select_final_agent_message(&snapshot, "turn-1", 1024).unwrap(),
            text
        );
    }
}
