//! Deterministic mock-backed tests for `PersistedCodexTeamDriver`.
//!
//! The mock app-server returns a fixed final agent message for its turn, so a
//! successful scheduler result proves the driver extracts the real turn text
//! instead of a placeholder.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use agent_code_runtime::{CodexTeamDriverConfig, PersistedCodexTeamDriver};
use agent_code_storage::SqliteTaskBoard;
use agent_code_team::{AgentDriver, AgentTask, TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

const MOCK: &str = env!("CARGO_BIN_EXE_codex_bridge_mock");
const MOCK_FINAL_TEXT: &str = "mock final answer";

fn unique_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ras_codex_bridge_{}_{}_{}",
        name,
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn running_codex_task(board: &mut SqliteTaskBoard) -> u64 {
    let task = board
        .create_task("mock codex task", None, TaskKind::Reasoning, None)
        .unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id: task,
            attempt: 1,
            agent_id: "codex".into(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })
        .unwrap();
    board.set_status(task, TaskStatus::Running).unwrap();
    task
}

#[tokio::test]
async fn scheduler_result_is_the_extracted_final_agent_message() {
    let root = unique_root("driver");
    let database = root.join("board.db");
    let working_directory = root.join("repo");
    std::fs::create_dir_all(&working_directory).unwrap();

    let task = {
        let mut board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
        running_codex_task(&mut board)
    };

    let driver = PersistedCodexTeamDriver::new(
        CodexTeamDriverConfig {
            command: MOCK.into(),
            working_directory: working_directory.clone(),
            // The mock ignores `-c` overrides, so any existing file satisfies
            // the configuration guard.
            mcp_command: PathBuf::from(MOCK),
            artifact_paths: Vec::new(),
            max_events: 16,
            overrides: Vec::new(),
        },
        database.clone(),
        "codex",
    )
    .unwrap();

    let result = driver
        .run_task(AgentTask {
            id: task,
            objective: "exercise the bounded Codex driver".into(),
            kind: TaskKind::Reasoning,
            context: Vec::new(),
        })
        .await
        .unwrap();

    assert_eq!(result.summary, MOCK_FINAL_TEXT);
    assert_ne!(
        result.summary,
        concat!("Codex scheduler task", " completed"),
        "driver must not fall back to a placeholder"
    );
    assert!(result.artifacts.is_empty());

    let reopened = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
    let binding = reopened.external_binding(task, 1).unwrap().unwrap();
    assert_eq!(binding.lifecycle_state, "completed");
    assert_eq!(binding.native_thread_id.as_deref(), Some("mock-thread"));
    assert_eq!(binding.native_turn_id.as_deref(), Some("mock-turn"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn driver_source_contains_no_completion_placeholder() {
    let placeholder = concat!("Codex scheduler task", " completed");
    assert!(!include_str!("../src/codex_team_driver.rs").contains(placeholder));
}
