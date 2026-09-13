//! Deterministic mock Codex app-server for transport regression tests.
//!
//! Speaks newline-delimited JSON-RPC on stdin/stdout like `codex app-server
//! --stdio`. It exists only to exercise the bounded event queue: `thread/read`
//! first emits a `turn/completed` notification and then the correlated
//! response, so the notification arrives while a request is pending.
//!
//! The mock is scriptable, so a resident Lead thread can be driven
//! deterministically. Every setting can be given either as a `-c` codex-style
//! override argument (`-c codex_bridge_mock.replies=[...]`, the channel a
//! caller that only configures `overrides` has) or through the environment:
//!
//! - replies: a JSON array of reply strings. Every `turn/start` consumes the
//!   next one; the last reply repeats (`CODEX_BRIDGE_MOCK_REPLIES`).
//! - state: a file path. The mock rewrites it with its call counters after
//!   every handled frame, so a test can observe how many threads and turns the
//!   client actually started (`CODEX_BRIDGE_MOCK_STATE`).
//! - elicitation: an MCP server name. Every turn first emits an
//!   `mcpServer/elicitation/request` for it (`CODEX_BRIDGE_MOCK_ELICITATION`).
//! - tool_call: a tool name. Every turn first emits an `item/tool/call` request
//!   for it (`CODEX_BRIDGE_MOCK_TOOL_CALL`).

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

const THREAD_ID: &str = "mock-thread";
const FINAL_TEXT: &str = "mock final answer";

fn main() {
    let mut mock = Mock::from_script();
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        mock.handle(&value);
    }
}

struct Mock {
    replies: Vec<String>,
    reply_index: usize,
    /// Every completed turn, in order: (turn id, reply).
    turns: Vec<(String, String)>,
    thread_starts: usize,
    turn_starts: usize,
    elicitation_responses: usize,
    tool_responses: usize,
    elicitation_server: Option<String>,
    tool_call: Option<String>,
    state_path: Option<PathBuf>,
    /// The text input of the most recent `turn/start`.
    last_prompt: Option<String>,
}

impl Mock {
    /// Read the script from `-c codex_bridge_mock.<key>=<value>` arguments,
    /// falling back to the `CODEX_BRIDGE_MOCK_*` environment variables.
    fn from_script() -> Self {
        let mut arguments: Vec<(String, String)> = Vec::new();
        for argument in std::env::args().skip(1) {
            let Some(setting) = argument.strip_prefix("codex_bridge_mock.") else {
                continue;
            };
            let Some((key, value)) = setting.split_once('=') else {
                continue;
            };
            arguments.push((key.to_string(), value.to_string()));
        }
        let setting = |key: &str, variable: &str| -> Option<String> {
            arguments
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
                .or_else(|| std::env::var(variable).ok())
        };
        let replies = match setting("replies", "CODEX_BRIDGE_MOCK_REPLIES") {
            Some(raw) => serde_json::from_str::<Vec<String>>(&raw).unwrap_or_else(|error| {
                eprintln!("codex_bridge_mock: mock replies are not a JSON string array: {error}");
                std::process::exit(2);
            }),
            None => vec![FINAL_TEXT.to_string()],
        };
        let replies = if replies.is_empty() {
            vec![FINAL_TEXT.to_string()]
        } else {
            replies
        };
        Self {
            replies,
            reply_index: 0,
            turns: Vec::new(),
            thread_starts: 0,
            turn_starts: 0,
            elicitation_responses: 0,
            tool_responses: 0,
            elicitation_server: setting("elicitation", "CODEX_BRIDGE_MOCK_ELICITATION"),
            tool_call: setting("tool_call", "CODEX_BRIDGE_MOCK_TOOL_CALL"),
            state_path: setting("state", "CODEX_BRIDGE_MOCK_STATE").map(PathBuf::from),
            last_prompt: None,
        }
    }

    fn handle(&mut self, value: &Value) {
        let id = value.get("id").cloned();
        match value.get("method").and_then(Value::as_str) {
            Some("initialize") => respond(&id, json!({"userAgent": "codex_bridge_mock"})),
            Some("initialized") => {}
            Some("thread/start") | Some("thread/resume") => {
                self.thread_starts += 1;
                respond(&id, json!({"thread": {"id": THREAD_ID}}));
            }
            Some("turn/start") => {
                self.last_prompt = value
                    .pointer("/params/input/0/text")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let turn_id = self.start_turn();
                respond(&id, json!({"turn": {"id": turn_id}}));
                // A real app-server initiates its own requests (elicitation,
                // tool calls) during a turn; emit them before completing so a
                // client's event pump must answer them.
                self.emit_requests();
                write_notification(
                    "turn/completed",
                    json!({"threadId": THREAD_ID, "turnId": turn_id}),
                );
            }
            Some("thread/read") => {
                // The regression fixture: the notification racing a pending
                // request must be queued, not dropped. The request carries no
                // turn id, so the notification names the most recent turn.
                let selected = self
                    .turns
                    .last()
                    .map(|(turn_id, _)| turn_id.clone())
                    .unwrap_or_else(|| "mock-turn".to_string());
                write_notification(
                    "turn/completed",
                    json!({"threadId": THREAD_ID, "turnId": selected}),
                );
                let turns: Vec<Value> = self
                    .turns
                    .iter()
                    .map(|(turn_id, reply)| {
                        json!({
                            "id": turn_id,
                            "status": "completed",
                            "items": [{
                                "type": "agentMessage",
                                "id": format!("message-{turn_id}"),
                                "text": reply,
                                "phase": null,
                            }],
                        })
                    })
                    .collect();
                respond(&id, json!({"thread": {"id": THREAD_ID, "turns": turns}}));
            }
            Some("turn/interrupt") => respond(&id, json!({})),
            Some("mcpServerStatus/list") => respond(&id, json!({"data": []})),
            Some(other) => {
                if id.is_some() {
                    write_frame(&json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32601, "message": format!("unknown mock method: {other}")},
                    }));
                }
            }
            None => {
                // A client response to a server-initiated request (elicitation
                // or tool call) carries no method; it is not a malformed
                // request. Counting it proves the client answered instead of
                // hanging the turn.
                if value.get("result").is_some() || value.get("error").is_some() {
                    match id.as_ref().and_then(Value::as_str) {
                        Some(response_id) if response_id.starts_with("mock-elicitation") => {
                            self.elicitation_responses += 1;
                        }
                        Some(response_id) if response_id.starts_with("mock-tool") => {
                            self.tool_responses += 1;
                        }
                        _ => {}
                    }
                } else if id.is_some() {
                    write_frame(&json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32600, "message": "malformed JSON-RPC request"},
                    }));
                }
            }
        }
        self.record();
    }

    /// Consume the next scripted reply and name the turn. The first turn keeps
    /// the fixed id the historical fixtures assert; later turns get distinct
    /// ids so a resident thread's turns stay distinguishable.
    fn start_turn(&mut self) -> String {
        let index = self.reply_index.min(self.replies.len() - 1);
        self.reply_index += 1;
        let reply = self.replies[index].clone();
        self.turn_starts += 1;
        let turn_id = if self.turn_starts == 1 {
            "mock-turn".to_string()
        } else {
            format!("mock-turn-{}", self.turn_starts)
        };
        self.turns.push((turn_id.clone(), reply));
        turn_id
    }

    fn emit_requests(&mut self) {
        if let Some(server_name) = self.elicitation_server.clone() {
            write_frame(&json!({
                "jsonrpc": "2.0",
                "id": format!("mock-elicitation-{}", self.turn_starts),
                "method": "mcpServer/elicitation/request",
                "params": {"serverName": server_name},
            }));
        }
        if let Some(tool) = self.tool_call.clone() {
            write_frame(&json!({
                "jsonrpc": "2.0",
                "id": format!("mock-tool-{}", self.turn_starts),
                "method": "item/tool/call",
                "params": {
                    "callId": format!("mock-call-{}", self.turn_starts),
                    "tool": tool,
                    "arguments": {},
                },
            }));
        }
    }

    /// Rewrite the observation file. It is written after every handled frame so
    /// a test that waits for a response always sees a state at least as new as
    /// that response; the write goes through a rename so a reader can never
    /// observe a half-written file.
    fn record(&self) {
        let Some(path) = &self.state_path else { return };
        let state = json!({
            "thread_starts": self.thread_starts,
            "turn_starts": self.turn_starts,
            "replies_consumed": self.reply_index,
            "elicitation_responses": self.elicitation_responses,
            "tool_responses": self.tool_responses,
            "turns": self.turns.iter().map(|(turn_id, _)| turn_id).collect::<Vec<_>>(),
            "replies": self.turns.iter().map(|(_, reply)| reply).collect::<Vec<_>>(),
            "last_prompt": self.last_prompt.as_deref(),
        });
        let temporary = path.with_extension("json.tmp");
        if std::fs::write(&temporary, state.to_string()).is_ok() {
            let _ = std::fs::rename(&temporary, path);
        }
    }
}

fn respond(id: &Option<Value>, result: Value) {
    write_frame(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
}

fn write_notification(method: &str, params: Value) {
    write_frame(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
}

fn write_frame(frame: &Value) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{frame}");
    let _ = stdout.flush();
}
