//! Upgrade a pre-ACC journal through the real SQLite versioning path.
//!
//! The ACC *implementation* is retired: nothing in the current product reads or
//! writes the ACC tables. What remains is a representation contract, so this
//! test drives the real migration and asserts the preserved representation
//! through the current board, never through the retired store or graph API.

use agentmosaic_storage::{
    ExternalRuntimeBinding, RuntimeCollaborationRecord, SqliteTaskBoard, SCHEMA_VERSION,
};
use agentmosaic_team::{TaskBoard, TaskKind};
use rusqlite::Connection;

const PRE_ACC_V4: &str = r#"
CREATE TABLE sessions (id TEXT PRIMARY KEY, state TEXT NOT NULL, active_call INTEGER, created_at TEXT NOT NULL);
CREATE TABLE agent_turns (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL, decision TEXT NOT NULL, error TEXT);
CREATE TABLE tool_calls (session_id TEXT NOT NULL, call_id INTEGER NOT NULL, state TEXT NOT NULL, request TEXT, PRIMARY KEY (session_id, call_id));
CREATE TABLE checkpoints (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL, git_head TEXT NOT NULL);
CREATE TABLE team_tasks (id INTEGER PRIMARY KEY AUTOINCREMENT, objective TEXT NOT NULL, parent_task INTEGER, kind TEXT NOT NULL, target TEXT, assignee TEXT, status TEXT NOT NULL);
CREATE TABLE team_task_runs (id INTEGER PRIMARY KEY AUTOINCREMENT, task_id INTEGER NOT NULL, attempt INTEGER NOT NULL, agent_id TEXT NOT NULL, status TEXT NOT NULL, result TEXT, error TEXT);
CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, from_agent TEXT NOT NULL, to_agent TEXT NOT NULL, body TEXT NOT NULL);
CREATE TABLE artifacts (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT, task_id INTEGER, path TEXT NOT NULL, sha256 TEXT NOT NULL);
CREATE TABLE transitions (seq INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL, from_state TEXT NOT NULL, to_state TEXT NOT NULL);
CREATE TABLE observations (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL, kind TEXT NOT NULL, payload TEXT NOT NULL, created_at TEXT NOT NULL);
"#;

/// The ACC tables a v4 database gains when it is upgraded.
const ACC_TABLES: [&str; 5] = [
    "acc_tasks",
    "acc_dependencies",
    "acc_context_manifests",
    "acc_artifacts",
    "acc_events",
];

#[test]
fn pre_acc_journal_migrates_preserving_rows_and_acc_tables() {
    let path = std::env::temp_dir().join(format!("agentmosaic_pre_acc_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let conn = Connection::open(&path).expect("create old journal");
        conn.execute_batch(PRE_ACC_V4).expect("install v4 schema");
        conn.pragma_update(None, "user_version", 4)
            .expect("mark v4");
        conn.execute("INSERT INTO sessions (id, state, active_call, created_at) VALUES ('legacy-session', 'Observing', NULL, 'now')", []).expect("seed legacy session");
        conn.execute("INSERT INTO team_tasks (objective, kind, status) VALUES ('legacy task', 'bulk', 'succeeded')", []).expect("seed legacy task");
    }

    // Opening at the current version runs the real migration.
    let board = SqliteTaskBoard::open(Connection::open(&path).expect("open upgrade target"))
        .expect("migrate v4 to current");
    assert_eq!(
        board.schema_version().expect("schema version"),
        SCHEMA_VERSION
    );

    // The pre-ACC rows survive, and the current board is usable on the result.
    let legacy = board
        .task(1)
        .expect("read legacy task")
        .expect("legacy task exists");
    assert_eq!(legacy.objective, "legacy task");
    assert_eq!(legacy.kind, TaskKind::Bulk);
    board
        .upsert_external_binding(&ExternalRuntimeBinding {
            team_task_id: 1,
            attempt: 1,
            agent_id: "codex".into(),
            runtime_kind: "codex-app-server".into(),
            native_thread_id: Some("external-thread".into()),
            native_turn_id: None,
            lifecycle_state: "reconcile_pending".into(),
        })
        .expect("v7 binding usable after v4 migration");
    board
        .record_runtime_collaboration(&RuntimeCollaborationRecord {
            team_task_id: 1,
            attempt: 1,
            runtime_kind: "codex-app-server".into(),
            native_call_id: "call".into(),
            kind: "request_context".into(),
            payload_summary: "bounded".into(),
            response_summary: Some("context returned".into()),
        })
        .expect("v7 collaboration usable after v4 migration");

    // The ACC tables are part of the upgraded representation even though no
    // current implementation reads or writes them.
    let conn = Connection::open(&path).expect("reopen migrated database for SQL assertions");
    let legacy_session: String = conn
        .query_row(
            "SELECT state FROM sessions WHERE id = 'legacy-session'",
            [],
            |row| row.get(0),
        )
        .expect("legacy session row preserved");
    assert_eq!(legacy_session, "Observing");
    for table in ACC_TABLES {
        let present: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("inspect the migrated schema");
        assert_eq!(present, 1, "{table} must still exist after migration");
    }

    drop(board);
    let _ = std::fs::remove_file(&path);
}
