use agentmosaic_storage::{import_database, SqliteTaskBoard, SCHEMA_VERSION};
use agentmosaic_team::{TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn path(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "agentmosaic_import_{label}_{}_{}.db",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ))
}
fn published(label: &str) -> PathBuf {
    let source = path(label);
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0_3_0_state.db"),
        &source,
    )
    .unwrap();
    source
}
#[test]
fn fresh_product_has_exactly_the_eight_product_tables() {
    let file = path("fresh");
    let board = SqliteTaskBoard::open(Connection::open(&file).unwrap()).unwrap();
    assert_eq!(board.schema_version().unwrap(), 14);
    let c = Connection::open(&file).unwrap();
    let names = c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap().query_map([], |r| r.get::<_,String>(0)).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    assert_eq!(
        names,
        [
            "agent_registry",
            "artifacts",
            "external_runtime_bindings",
            "runtime_events",
            "team_final_artifact_refs",
            "team_final_task_refs",
            "team_task_runs",
            "team_tasks"
        ]
    );
    std::fs::remove_file(file).unwrap();
}
#[test]
fn every_unsupported_schema_is_rejected_before_destination_creation_or_source_write() {
    for version in [0, 1, 8, 10, 13, 14, 15, 999] {
        let source = published("unsupported");
        let target = path("unused");
        Connection::open(&source)
            .unwrap()
            .pragma_update(None, "user_version", version)
            .unwrap();
        let before = std::fs::read(&source).unwrap();
        assert!(import_database(&source, &target).is_err());
        assert!(!target.exists());
        assert_eq!(std::fs::read(&source).unwrap(), before);
        std::fs::remove_file(source).unwrap();
    }
}
#[test]
fn normal_open_rejects_historical_and_future_databases_without_writing() {
    for version in [11, 12, 13, 15] {
        let source = published("normal_open");
        Connection::open(&source)
            .unwrap()
            .pragma_update(None, "user_version", version)
            .unwrap();
        let before = std::fs::read(&source).unwrap();
        assert!(SqliteTaskBoard::open(Connection::open(&source).unwrap()).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), before);
        std::fs::remove_file(source).unwrap();
    }
}
#[test]
fn import_never_overwrites_destination_or_translates_retired_driver_strings() {
    let source = published("retired");
    Connection::open(&source)
        .unwrap()
        .execute("UPDATE agent_registry SET driver_kind='native'", [])
        .unwrap();
    let target = path("retired_target");
    std::fs::write(&target, b"existing destination").unwrap();
    assert!(import_database(&source, &target).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"existing destination");
    std::fs::remove_file(&target).unwrap();
    import_database(&source, &target).unwrap();
    let kinds = Connection::open(&target)
        .unwrap()
        .prepare("SELECT DISTINCT driver_kind FROM agent_registry")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(kinds, vec!["native"]);
    std::fs::remove_file(source).unwrap();
    std::fs::remove_file(target).unwrap();
}
#[test]
fn broken_reference_and_session_only_artifact_abort_without_destination() {
    for sql in [
        "UPDATE team_task_runs SET task_id=999",
        "UPDATE artifacts SET task_id=NULL",
        "UPDATE team_final_artifact_refs SET sha256='missing'",
        "UPDATE team_tasks SET parent_task=2 WHERE id=1",
        "UPDATE team_task_runs SET attempt=4294967296",
        "UPDATE team_final_task_refs SET selected_task_id=1",
    ] {
        let source = published("broken");
        let target = path("broken_target");
        let c = Connection::open(&source).unwrap();
        c.pragma_update(None, "foreign_keys", false).unwrap();
        c.execute(sql, []).unwrap();
        drop(c);
        let before = std::fs::read(&source).unwrap();
        assert!(import_database(&source, &target).is_err());
        assert!(!target.exists());
        assert_eq!(std::fs::read(&source).unwrap(), before);
        std::fs::remove_file(source).unwrap();
    }
}
#[test]
fn development_import_preserves_events_bindings_and_interrupted_recovery_without_replay() {
    let source = path("development");
    let target = path("development_target");
    let mut board = SqliteTaskBoard::open(Connection::open(&source).unwrap()).unwrap();
    let task = board
        .create_task("interrupted worker", None, TaskKind::Bulk, None)
        .unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id: task,
            attempt: 1,
            agent_id: "worker".into(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })
        .unwrap();
    board.set_status(task, TaskStatus::Running).unwrap();
    board
        .append_runtime_event(agentmosaic_team::RuntimeEventRecord {
            task_id: task,
            attempt: 1,
            agent_id: "worker".into(),
            runtime_name: Some("Peer".into()),
            native_session_id: None,
            event: agentmosaic_team::RuntimeEvent::AssistantMessageCompleted {
                text: "observation".into(),
            },
        })
        .unwrap();
    drop(board);
    let c = Connection::open(&source).unwrap();
    c.execute("INSERT INTO external_runtime_bindings(team_task_id,attempt,agent_id,runtime_kind,lifecycle_state,runtime_name) VALUES (1,1,'worker','acp','running','Peer')",[]).unwrap();
    c.pragma_update(None, "user_version", 12).unwrap();
    drop(c);
    let before = std::fs::read(&source).unwrap();
    import_database(&source, &target).unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), before);
    let mut board = SqliteTaskBoard::open(Connection::open(&target).unwrap()).unwrap();
    assert_eq!(board.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(
        board
            .external_binding_extended(task, 1)
            .unwrap()
            .unwrap()
            .runtime_name
            .as_deref(),
        Some("Peer")
    );
    assert_eq!(
        board.task(task).unwrap().unwrap().status,
        TaskStatus::Running
    );
    let c = Connection::open(&target).unwrap();
    let event: (String, String) = c
        .query_row(
            "SELECT event_kind,payload_json FROM runtime_events",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(event.0, "assistant_message_completed");
    assert_eq!(
        board.runtime_events(task, 1, 0, 10).unwrap()[0]
            .record
            .event,
        agentmosaic_team::RuntimeEvent::AssistantMessageCompleted {
            text: "observation".into()
        }
    );
    assert!(board.recover_interrupted_attempt(task).unwrap().is_some());
    assert!(board.recover_interrupted_attempt(task).unwrap().is_none());
    assert_eq!(board.attempts(task).unwrap().len(), 1);
    assert_eq!(board.attempts(task).unwrap()[0].status, TaskStatus::Failed);
    std::fs::remove_file(source).unwrap();
    std::fs::remove_file(target).unwrap();
}

#[test]
fn import_preserves_deleted_identifier_high_water_marks() {
    let source = published("sequence");
    let target = path("sequence_target");
    Connection::open(&source)
        .unwrap()
        .execute(
            "UPDATE sqlite_sequence SET seq=100 WHERE name='team_tasks'",
            [],
        )
        .unwrap();
    import_database(&source, &target).unwrap();
    let mut board = SqliteTaskBoard::open(Connection::open(&target).unwrap()).unwrap();
    assert_eq!(
        board
            .create_task("new work", None, TaskKind::Bulk, None)
            .unwrap(),
        101
    );
    std::fs::remove_file(source).unwrap();
    std::fs::remove_file(target).unwrap();
}
