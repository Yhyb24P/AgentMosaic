//! M2-B3 mock ACP coding-agent for reproduction-critical lifecycle paths.
//!
//! Speaks ACP v1 JSON-RPC over NDJSON on stdin/stdout so that `AcpWorkerDriver`
//! can be exercised without credentials, live runtimes, or anything heavier.
//! It is intentionally not production functional: it only exists to exercise
//! the bounded-lifecycle paths the real runtime uses, including a liveness
//! verification path.

use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};

const MOCK_SESSION: &str = "acp-m2-mock-session";
const HANG_FOREVER: u64 = 3600;
const SLOW_CHUNK_DELAY_MS: u64 = 2000;

static PROMPT_TURNS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
enum Mode {
    Sync,
    Slow,
    SlowDrip,
    Hang,
    CancelWait,
    Crash,
    Repair,
    Permission,
    Resume,
}

fn parse_mode(args: &[String]) -> Result<Mode, String> {
    let mut i = 0;
    let mut mode = Mode::Sync;
    let mut pid_file: Option<String> = None;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                i += 1;
                let value = args.get(i).ok_or("--mode requires a value")?;
                mode = match value.as_str() {
                    "sync" => Mode::Sync,
                    "slow" => Mode::Slow,
                    "slow-drip" => Mode::SlowDrip,
                    "hang" => Mode::Hang,
                    "cancel-wait" => Mode::CancelWait,
                    "crash" => Mode::Crash,
                    "repair" => Mode::Repair,
                    "permission" => Mode::Permission,
                    "resume" => Mode::Resume,
                    other => return Err(format!("unknown mode: {other}")),
                };
            }
            "--pid-file" => {
                i += 1;
                pid_file = Some(args.get(i).ok_or("--pid-file requires a value")?.clone());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }
    if let Some(path) = pid_file {
        std::fs::write(path, std::process::id().to_string().as_bytes())
            .map_err(|e| format!("write pid file: {e}"))?;
    }
    Ok(mode)
}

fn main() {
    let mode = match parse_mode(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("acp_m2_mock: {message}");
            std::process::exit(2);
        }
    };

    let stdin = io::stdin();

    let mut pending_prompt: Option<Value> = None;
    for line in stdin.lock().lines() {
        match line {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match handle_line(line, mode, &mut pending_prompt) {
                    LineOutcome::Done => {}
                    LineOutcome::Crash => {
                        std::process::exit(1);
                    }
                }
            }
            Err(_) => break,
        }
    }
}

enum LineOutcome {
    Done,
    Crash,
}

fn handle_line(line: &str, mode: Mode, pending_prompt: &mut Option<Value>) -> LineOutcome {
    let value = match serde_json::from_str::<Value>(line) {
        Ok(value) => value,
        Err(_) => return LineOutcome::Done,
    };
    let method = value.get("method").and_then(Value::as_str);
    let id = value.get("id").cloned();

    match method {
        Some("session/new") => {
            write_response(&id, json!({ "sessionId": MOCK_SESSION }));
            if matches!(mode, Mode::Crash) {
                return LineOutcome::Crash;
            }
        }
        Some("session/resume") if matches!(mode, Mode::Resume) => {
            write_response(&id, json!({}));
        }
        Some("session/prompt") => {
            if matches!(mode, Mode::Permission) {
                *pending_prompt = id;
                write_permission_request();
                return LineOutcome::Done;
            }
            let turn = PROMPT_TURNS.fetch_add(1, Ordering::SeqCst);
            let text = if matches!(mode, Mode::Repair) && turn == 0 {
                "not-a-peer-result".into()
            } else {
                format!(r#"{{"summary":"mock-ok-{turn}"}}"#)
            };
            if matches!(mode, Mode::SlowDrip) {
                for chunk in ["{\"summary\":\"", "slow", "-drip", "\"}"] {
                    write_notification(&json!({
                        "sessionId": MOCK_SESSION,
                        "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": chunk } },
                    }));
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
                write_response(&id, json!({ "stopReason": "end_turn" }));
                return LineOutcome::Done;
            }
            write_notification(&json!({
                "sessionId": MOCK_SESSION,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {
                        "type": "text",
                        "text": text,
                    },
                },
            }));
            match mode {
                Mode::Hang => {
                    std::thread::sleep(std::time::Duration::from_secs(HANG_FOREVER));
                }
                Mode::Slow => {
                    std::thread::sleep(std::time::Duration::from_millis(SLOW_CHUNK_DELAY_MS));
                }
                Mode::CancelWait => {
                    *pending_prompt = id;
                    return LineOutcome::Done;
                }
                Mode::Permission
                | Mode::Sync
                | Mode::Crash
                | Mode::Repair
                | Mode::Resume
                | Mode::SlowDrip => {}
            }
            if !matches!(mode, Mode::Hang) {
                write_response(&id, json!({ "stopReason": "end_turn" }));
            }
        }
        Some("initialize") => {
            write_response(
                &id,
                json!({
                    "protocolVersion": 1,
                    "agentCapabilities": if matches!(mode, Mode::Resume) {
                        json!({ "sessionCapabilities": { "resume": {} } })
                    } else { json!({}) },
                    "authMethods": [],
                    "agentInfo": { "name": "acp-m2-mock", "version": "1.0" },
                }),
            );
        }
        Some("session/cancel") => {
            if let Some(prompt_id) = pending_prompt.take() {
                write_response(&Some(prompt_id), json!({ "stopReason": "cancelled" }));
            }
        }
        Some("authenticate") => {
            write_response(&id, json!({}));
        }
        Some(unknown) => {
            if id.is_some() {
                write_error(&id, -32601, &format!("unknown mock method: {unknown}"));
            }
        }
        None => {
            if matches!(mode, Mode::Permission)
                && id.as_ref().and_then(Value::as_str) == Some("mock-permission")
            {
                let selected = value
                    .get("result")
                    .and_then(|result| result.get("outcome"))
                    .and_then(|outcome| outcome.get("optionId"))
                    .and_then(Value::as_str);
                if selected != Some("reject") {
                    write_error(&pending_prompt.take(), -32000, "mock requires deny policy");
                    return LineOutcome::Done;
                }
                write_notification(&json!({
                    "sessionId": MOCK_SESSION,
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": r#"{"summary":"permission-denied"}"# },
                    },
                }));
                write_response(&pending_prompt.take(), json!({ "stopReason": "end_turn" }));
                return LineOutcome::Done;
            }
            if id.is_some() {
                write_error(&id, -32600, "malformed JSON-RPC request");
            }
        }
    }
    LineOutcome::Done
}

fn write_permission_request() {
    write_frame(&json!({
        "jsonrpc": "2.0",
        "id": "mock-permission",
        "method": "session/request_permission",
        "params": {
            "sessionId": MOCK_SESSION,
            "toolCall": {
                "toolCallId": "mock-tool-call",
                "title": "write guarded file",
            },
            "options": [
                { "optionId": "allow", "name": "Allow once", "kind": "allow_once" },
                { "optionId": "reject", "name": "Deny once", "kind": "reject_once" },
            ],
        },
    }));
}

fn write_response(id: &Option<Value>, result: Value) {
    let frame = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    write_frame(&frame);
}

fn write_error(id: &Option<Value>, code: i64, message: &str) {
    let frame = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    });
    write_frame(&frame);
}

fn write_notification(params: &Value) {
    let frame = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": params,
    });
    write_frame(&frame);
}

fn write_frame(frame: &Value) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{frame}");
    let _ = stdout.flush();
}
