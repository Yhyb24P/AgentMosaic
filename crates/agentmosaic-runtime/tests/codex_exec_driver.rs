use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agentmosaic_runtime::{CodexExecDriverConfig, PersistedCodexExecDriver};
use agentmosaic_storage::SqliteTaskBoard;
use agentmosaic_team::{AgentDriver, AgentTask, TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_codex_exec_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// The Codex executable used by the live worker probe.
fn codex_binary() -> PathBuf {
    PathBuf::from(std::env::var("AM_CODEX_BIN").unwrap_or_else(|_| "codex".to_string()))
}

/// One running task, the way the scheduler leaves it before a driver runs.
fn running_codex_task(database: &std::path::Path) -> u64 {
    let mut board = SqliteTaskBoard::open(Connection::open(database).unwrap()).unwrap();
    let task_id = board
        .create_task("Codex exec live task", None, TaskKind::Reasoning, None)
        .unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id,
            attempt: 1,
            agent_id: "codex".into(),
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
#[ignore = "requires an authenticated local Codex CLI; runs one bounded live codex exec turn"]
async fn live_exec_turn_binds_a_thread_and_returns_a_bounded_result() {
    let root = root();
    let database = root.join("board.db");
    let working_directory = root.join("repo");
    std::fs::create_dir_all(&working_directory).unwrap();
    // The real CLI refuses to run outside a trusted directory, so the probe
    // work directory is a real repository, exactly as a project run is.
    std::fs::write(working_directory.join("README.md"), "# codex exec probe\n").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "README.md"],
        vec![
            "-c",
            "user.email=probe@example.invalid",
            "-c",
            "user.name=probe",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    ] {
        let status = std::process::Command::new("git")
            .args(&args)
            .current_dir(&working_directory)
            .status()
            .expect("git is runnable");
        assert!(status.success(), "git {args:?} failed");
    }
    let task_id = running_codex_task(&database);
    let driver = PersistedCodexExecDriver::new(
        CodexExecDriverConfig {
            command: codex_binary(),
            args: Vec::new(),
            working_directory,
            timeout: Duration::from_secs(240),
            max_prompt_bytes: 2048,
            max_result_bytes: 4096,
            output_schema: None,
            artifact_paths: Vec::new(),
            isolate: false,
        },
        database.clone(),
        "codex",
    )
    .unwrap();
    let result = driver
        .run_task(AgentTask {
            id: task_id,
            objective: "Do not use tools or read files. Reply with exactly this JSON peer result and nothing else: {\"summary\":\"codex exec live ok\"}".into(),
            kind: TaskKind::Reasoning,
            context: Vec::new(),
        })
        .await
        .expect("the live Codex exec worker turn completes");
    assert!(
        result.summary.contains("codex exec live ok"),
        "unexpected live summary: {}",
        result.summary
    );

    let board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
    let binding = board.external_binding(task_id, 1).unwrap().unwrap();
    assert_eq!(binding.runtime_kind, "codex-exec");
    assert_eq!(binding.lifecycle_state, "completed");
    assert!(
        binding.native_thread_id.is_some(),
        "a live Codex exec turn must persist its foreign thread"
    );
    let events = board.runtime_events(task_id, 1, 0, 64).unwrap();
    assert!(events
        .iter()
        .any(|event| event.record.event.kind() == "session_started"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
#[cfg(unix)]
async fn persists_thread_binding_and_normalized_events() {
    let root = root();
    let database = root.join("board.db");
    let task_id = {
        let mut board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
        let task_id = board
            .create_task("Codex exec task", None, TaskKind::Reasoning, None)
            .unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id,
                attempt: 1,
                agent_id: "codex".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(task_id, TaskStatus::Running).unwrap();
        task_id
    };
    let driver = PersistedCodexExecDriver::new(
        CodexExecDriverConfig {
            command: PathBuf::from("sh"),
            // The shell script reads the stdin prompt exactly like the real
            // supervisor contract, and intentionally ignores adapter-owned
            // trailing `exec --json` argv, making this a deterministic JSONL
            // fixture rather than a timing-dependent one.
            args: vec![
                "-c".into(),
                "prompt=$(cat); [ -n \"$prompt\" ] || exit 8; [ \"$2\" = --output-schema ] && [ -f \"$3\" ] || exit 9; printf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"thread-1\"}' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"summary\\\":\\\"finished\\\"}\"}}'"
                    .into(),
            ],
            working_directory: root.clone(),
            timeout: Duration::from_secs(1),
            max_prompt_bytes: 1024,
            max_result_bytes: 1024,
            output_schema: None,
            artifact_paths: Vec::new(),
            isolate: false,
        },
        database.clone(),
        "codex",
    )
    .unwrap();
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
    assert_eq!(binding.runtime_kind, "codex-exec");
    assert_eq!(binding.native_thread_id.as_deref(), Some("thread-1"));
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
