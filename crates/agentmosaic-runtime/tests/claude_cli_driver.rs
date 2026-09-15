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

/// The Claude executable used by live worker probes.
fn claude_binary() -> PathBuf {
    PathBuf::from(std::env::var("AM_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string()))
}

fn running_claude_task(database: &std::path::Path) -> u64 {
    let mut board = SqliteTaskBoard::open(Connection::open(database).unwrap()).unwrap();
    let task_id = board
        .create_task("Claude live task", None, TaskKind::Reasoning, None)
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
}

#[tokio::test]
#[cfg(unix)]
#[ignore = "requires an authenticated local Claude Code CLI; runs one bounded live worker turn"]
async fn live_worker_turn_binds_a_session_and_persists_only_visible_evidence() {
    let root = root();
    let database = root.join("board.db");
    let working_directory = root.join("repo");
    std::fs::create_dir_all(&working_directory).unwrap();
    let task_id = running_claude_task(&database);
    let driver = PersistedClaudeCliDriver::new(
        ClaudeCliDriverConfig {
            command: claude_binary(),
            args: Vec::new(),
            working_directory,
            timeout: Duration::from_secs(240),
            max_prompt_bytes: 2048,
            max_result_bytes: 4096,
            json_schema: None,
            artifact_paths: Vec::new(),
        },
        database.clone(),
        "claude",
    )
    .unwrap();
    let result = driver
        .run_task(AgentTask {
            id: task_id,
            objective:
                "Do not use tools. Reply with exactly this text and nothing else: claude worker ok"
                    .into(),
            kind: TaskKind::Reasoning,
            context: Vec::new(),
        })
        .await
        .expect("the live Claude worker turn completes");
    assert!(
        result.summary.contains("claude worker ok"),
        "unexpected live summary: {}",
        result.summary
    );

    let board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
    let binding = board.external_binding(task_id, 1).unwrap().unwrap();
    assert_eq!(binding.runtime_kind, "claude-cli");
    assert_eq!(binding.lifecycle_state, "completed");
    assert!(
        binding.native_thread_id.is_some(),
        "a live Claude worker turn must persist its foreign session"
    );
    let events = board.runtime_events(task_id, 1, 0, 64).unwrap();
    assert!(events
        .iter()
        .any(|event| event.record.event.kind() == "session_started"));
    // Only normalized, visible observations are durable: hidden reasoning and
    // raw frames never reach the board.
    for event in &events {
        let kind = event.record.event.kind();
        assert!(
            !kind.contains("thought") && !kind.contains("thinking"),
            "hidden Claude reasoning must not be durable: {kind}"
        );
    }

    // A second attempt that already carries the foreign session must resume the
    // same Claude session instead of starting a new one.
    let session = binding.native_thread_id.clone().unwrap();
    let resumed_task = running_claude_task(&database);
    {
        let board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
        board
            .upsert_external_binding(&agentmosaic_storage::ExternalRuntimeBinding {
                team_task_id: resumed_task,
                attempt: 1,
                agent_id: "claude".into(),
                runtime_kind: "claude-cli".into(),
                native_thread_id: Some(session.clone()),
                native_turn_id: None,
                lifecycle_state: "running".into(),
            })
            .unwrap();
    }
    let resumed = driver
        .run_task(AgentTask {
            id: resumed_task,
            objective:
                "Do not use tools. Reply with exactly this text and nothing else: claude resume ok"
                    .into(),
            kind: TaskKind::Reasoning,
            context: Vec::new(),
        })
        .await
        .expect("the live Claude resume turn completes");
    assert!(
        resumed.summary.contains("claude resume ok"),
        "unexpected live resume summary: {}",
        resumed.summary
    );
    let board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
    let resumed_binding = board.external_binding(resumed_task, 1).unwrap().unwrap();
    assert_eq!(
        resumed_binding.native_thread_id.as_deref(),
        Some(session.as_str())
    );
    let _ = std::fs::remove_dir_all(root);
}
