use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agentmosaic_runtime::{ClaudeCliDriverConfig, PersistedClaudeCliDriver};
use agentmosaic_storage::SqliteTaskBoard;
use agentmosaic_team::{AgentDriver, AgentTask, TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_claude_cli_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[tokio::test]
#[cfg(unix)]
async fn persists_session_binding_and_normalized_events() {
    let root = root();
    let database = root.join("board.db");
    let task_id = {
        let mut board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
        let task_id = board
            .create_task("Claude task", None, TaskKind::Reasoning, None)
            .unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id,
                attempt: 1,
                agent_id: "claude".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(task_id, TaskStatus::Running).unwrap();
        task_id
    };
    let driver = PersistedClaudeCliDriver::new(
        ClaudeCliDriverConfig {
            command: PathBuf::from("sh"),
            // `cat` consumes the adapter-owned stdin prompt before emitting a
            // deterministic Claude stream fixture; later Claude flags become
            // shell positional parameters and are not interpreted as code.
            args: vec!["-c".into(), "cat >/dev/null; printf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"session-1\"}' '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"finished\"}]}}'".into()],
            working_directory: root.clone(), timeout: Duration::from_secs(1), max_prompt_bytes: 1024, max_result_bytes: 1024, json_schema: None, artifact_paths: Vec::new(),
        }, database.clone(), "claude"
    ).unwrap();
    let result = driver
        .run_task(AgentTask {
            id: task_id,
            objective: "complete fixture".into(),
            kind: TaskKind::Reasoning,
            context: Vec::new(),
        })
        .await
        .unwrap();
    assert_eq!(result.summary, "finished");
    let board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
    let binding = board.external_binding(task_id, 1).unwrap().unwrap();
    assert_eq!(binding.runtime_kind, "claude-cli");
    assert_eq!(binding.native_thread_id.as_deref(), Some("session-1"));
    assert_eq!(binding.lifecycle_state, "completed");
    let events = board.runtime_events(task_id, 1, 0, 16).unwrap();
    assert!(events
        .iter()
        .any(|event| event.record.event.kind() == "session_started"));
    assert!(events
        .iter()
        .any(|event| event.record.event.kind() == "assistant_message_completed"));
    let _ = std::fs::remove_dir_all(root);
}
