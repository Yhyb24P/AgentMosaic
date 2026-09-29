use agentmosaic_storage::{ExternalRuntimeBinding, SqliteTaskBoard};
use agentmosaic_team::{TaskBoard, TaskKind};
use rusqlite::Connection;

#[test]
fn external_binding_survives_reopen() {
    let path = std::env::temp_dir().join(format!("agentmosaic_phase21_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let mut board = SqliteTaskBoard::open(Connection::open(&path).unwrap()).unwrap();
        let task = board
            .create_task("codex work", None, TaskKind::Reasoning, None)
            .unwrap();
        board
            .upsert_external_binding(&ExternalRuntimeBinding {
                team_task_id: task,
                attempt: 1,
                agent_id: "codex".into(),
                runtime_kind: "codex-exec".into(),
                native_thread_id: Some("thread-external".into()),
                native_turn_id: Some("turn-external".into()),
                lifecycle_state: "running".into(),
            })
            .unwrap();
    }
    let board = SqliteTaskBoard::open(Connection::open(&path).unwrap()).unwrap();
    let binding = board.external_binding(1, 1).unwrap().unwrap();
    assert_eq!(binding.native_thread_id.as_deref(), Some("thread-external"));
    let _ = std::fs::remove_file(path);
}
