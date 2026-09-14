use std::path::PathBuf;

use agentmosaic_storage::{SqliteAgentRegistry, SqliteTaskBoard, SCHEMA_VERSION};
use agentmosaic_team::{TaskBoard, TaskStatus};
use rusqlite::Connection;
use sha2::Digest;

const FIXTURE_SHA256: &str = "e7b430ad17b8c2be3704f540ca921c77b2ba063c9661300411fbda51334adc3b";

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0_3_0_state.db")
}

fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn published_v030_database_migrates_additively_to_v12() {
    let bytes = std::fs::read(fixture()).expect("read frozen published-version fixture");
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(&bytes)),
        FIXTURE_SHA256
    );
    let path = std::env::temp_dir().join(format!(
        "agentmosaic_published_v03_migration_{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, bytes).expect("copy fixture without mutating source");

    let before = Connection::open(&path).expect("open v0.3 database");
    assert_eq!(
        before
            .pragma_query_value::<i32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        11
    );
    let table_counts = [
        ("agent_registry", 2),
        ("team_tasks", 2),
        ("team_task_runs", 2),
        ("artifacts", 1),
        ("messages", 0),
        ("team_final_task_refs", 1),
        ("team_final_artifact_refs", 1),
        ("external_runtime_bindings", 1),
    ];
    for (table, expected) in table_counts {
        assert_eq!(count(&before, table), expected, "pre-migration {table}");
    }
    drop(before);

    let board = SqliteTaskBoard::open(Connection::open(&path).unwrap()).unwrap();
    assert_eq!(board.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 12);
    assert_eq!(
        board.task(1).unwrap().unwrap().status,
        TaskStatus::Succeeded
    );
    assert_eq!(
        board.task(2).unwrap().unwrap().status,
        TaskStatus::Succeeded
    );
    assert_eq!(
        board.attempts(2).unwrap()[0].result.as_deref(),
        Some("mock-ok-0")
    );
    assert_eq!(board.artifacts(2).unwrap().len(), 1);
    assert!(board.messages().unwrap().is_empty());
    let (tasks, artifacts) = board.final_refs(1).unwrap();
    assert_eq!(tasks, vec![2]);
    assert_eq!(artifacts.len(), 1);
    let binding = board.external_binding_extended(2, 1).unwrap().unwrap();
    assert_eq!(binding.binding.runtime_kind, "acp");
    assert_eq!(binding.binding.lifecycle_state, "completed");
    assert_eq!(binding.runtime_name, None);
    assert!(board.runtime_events(2, 1, 0, 10).unwrap().is_empty());
    drop(board);

    let registry = SqliteAgentRegistry::open(&path).unwrap();
    assert_eq!(registry.list_agents().unwrap().len(), 2);
    drop(registry);
    let after = Connection::open(&path).unwrap();
    for (table, expected) in table_counts {
        assert_eq!(count(&after, table), expected, "post-migration {table}");
    }
    assert_eq!(count(&after, "runtime_events"), 0);
    let _ = std::fs::remove_file(path);
}
