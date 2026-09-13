//! Migration coverage for the durable registry config column and the authentic
//! historical v8 fixture.
//!
//! The v8 fixture is immutable historical DDL (`tests/fixtures/schema_v8.sql`,
//! extracted from commit e7649230). It must never be rebuilt by mutating the
//! current `SCHEMA`; that trick is only acceptable for the previous-current
//! (v10) shape tested below.

use std::path::PathBuf;

use agentmosaic_storage::{
    AgentRegistryRecord, SqliteAgentRegistry, SqliteTaskBoard, SCHEMA, SCHEMA_VERSION,
};
use agentmosaic_team::{ArtifactMeta, SelectedArtifactRef, TaskBoard, TaskKind};
use rusqlite::Connection;

const V8_SCHEMA_VERSION: i32 = 8;

fn fixture_sql() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("schema_v8.sql");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn temp_db(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "agentmosaic_schema_migration_{name}_{}.db",
        std::process::id()
    ))
}

fn user_version(conn: &Connection) -> i32 {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version")
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    count > 0
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
            [table, column],
            |row| row.get(0),
        )
        .expect("query pragma_table_info");
    count > 0
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).expect(sql)
}

/// Seed representative v8 rows, using only columns the historical DDL has.
fn seed_v8(conn: &Connection) {
    conn.execute(
        "INSERT INTO team_tasks (objective, kind, status) VALUES ('preserved v8 task', 'bulk', 'pending')",
        [],
    )
    .expect("seed v8 task");
    conn.execute(
        "INSERT INTO team_task_runs (task_id, attempt, agent_id, status, result)
         VALUES (1, 1, 'worker-a', 'succeeded', 'v8 result')",
        [],
    )
    .expect("seed v8 run");
    conn.execute(
        "INSERT INTO artifacts (session_id, task_id, path, sha256)
         VALUES (NULL, 1, 'out.txt', 'v8-artifact-sha256')",
        [],
    )
    .expect("seed v8 artifact");
    // No `runtime_version` and no `driver_config_json`: neither column exists at v8.
    conn.execute(
        "INSERT INTO agent_registry
             (id, name, tier, driver_kind, executable, driver_args_json, max_concurrency, tags_json)
         VALUES ('v8-worker', 'v8 worker', 'worker', 'acp', 'codex', '[\"--acp\"]', 1, '[\"qwen\"]')",
        [],
    )
    .expect("seed v8 agent");
}

fn agent(id: &str) -> AgentRegistryRecord {
    AgentRegistryRecord {
        id: id.into(),
        name: "acp worker".into(),
        tier: "worker".into(),
        driver_kind: Some("acp".into()),
        executable: Some("codex".into()),
        runtime_version: Some("0.154.0".into()),
        driver_args_json: Some(r#"["--acp"]"#.into()),
        max_concurrency: Some(2),
        tags_json: Some(r#"["qwen"]"#.into()),
        driver_config_json: Some(r#"{"auth_method":"chatgpt","timeout_ms":30000}"#.into()),
    }
}

/// The authentic v8 database migrates all the way to the current version:
/// runtime metadata and non-secret driver options are added, the v9 final-ref
/// tables appear, and every seeded v8 row survives.
#[test]
fn authentic_v8_database_migrates_to_current_and_preserves_rows() {
    let path = temp_db("v8-to-current");
    let _ = std::fs::remove_file(&path);

    // Pre-migration assertions run on an independent connection that has not
    // been through the current migrate().
    {
        let conn = Connection::open(&path).expect("create v8 fixture");
        conn.execute_batch(&fixture_sql())
            .expect("apply authentic historical v8 DDL");
        conn.pragma_update(None, "user_version", V8_SCHEMA_VERSION)
            .expect("stamp v8");
        seed_v8(&conn);

        assert_eq!(user_version(&conn), V8_SCHEMA_VERSION);
        assert!(table_exists(&conn, "agent_registry"));
        assert!(!column_exists(&conn, "agent_registry", "runtime_version"));
        assert!(!column_exists(
            &conn,
            "agent_registry",
            "driver_config_json"
        ));
        assert!(!table_exists(&conn, "team_final_task_refs"));
        assert!(!table_exists(&conn, "team_final_artifact_refs"));
        // The authentic v8 DDL already contains these tables: ACC landed at v6
        // and the external runtime bindings at v7, both before v8. The fixture
        // is historical DDL and must not be "fixed" to drop them.
        for table in [
            "acc_tasks",
            "acc_dependencies",
            "acc_context_manifests",
            "acc_artifacts",
            "acc_events",
            "external_runtime_bindings",
            "runtime_collaboration_records",
        ] {
            assert!(
                table_exists(&conn, table),
                "{table} is part of the authentic v8 DDL"
            );
        }
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM team_tasks"),
            1,
            "representative v8 task"
        );
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM team_task_runs"),
            1,
            "representative v8 run"
        );
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM artifacts"),
            1,
            "representative v8 artifact"
        );
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM agent_registry"),
            1,
            "representative v8 agent"
        );
    }

    // Post-migration assertions.
    let mut board = SqliteTaskBoard::open(Connection::open(&path).expect("reopen"))
        .expect("migrate v8 to the current version");
    assert_eq!(
        board.schema_version().expect("schema version"),
        SCHEMA_VERSION
    );
    let task = board.task(1).expect("read task").expect("task preserved");
    assert_eq!(task.objective, "preserved v8 task");
    assert_eq!(task.kind, TaskKind::Bulk);
    let runs = board.attempts(1).expect("read attempts");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].agent_id, "worker-a");
    assert_eq!(runs[0].result.as_deref(), Some("v8 result"));
    let artifacts = board.artifacts(1).expect("read artifacts");
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].path, "out.txt");

    // The v9 final-ref tables exist and are writable through the typed API.
    let child = board
        .create_task("selected after migration", Some(1), TaskKind::Utility, None)
        .expect("create child");
    let selected = ArtifactMeta {
        path: "selected.txt".into(),
        sha256: "a".repeat(64),
    };
    board.record_artifact(child, &selected).expect("artifact");
    board
        .record_final_refs(
            1,
            &[child],
            &[SelectedArtifactRef {
                task_id: child,
                artifact: selected.clone(),
            }],
        )
        .expect("final refs writable");
    let (task_refs, artifact_refs) = board.final_refs(1).expect("read final refs");
    assert_eq!(task_refs, vec![child]);
    assert_eq!(artifact_refs.len(), 1);
    assert_eq!(artifact_refs[0].artifact, selected);
    drop(board);

    // The registry columns were added by the migration and are writable.
    {
        let registry = SqliteAgentRegistry::open(&path).expect("open migrated registry");
        assert_eq!(registry.schema_version().expect("version"), SCHEMA_VERSION);
        let migrated = registry
            .get_agent("v8-worker")
            .expect("get v8 agent")
            .expect("v8 agent preserved");
        assert_eq!(migrated.name, "v8 worker");
        assert_eq!(migrated.max_concurrency, Some(1));
        assert_eq!(migrated.runtime_version, None);
        assert_eq!(migrated.driver_config_json, None);
        registry
            .upsert_agent(&agent("v8-worker"))
            .expect("runtime_version and driver_config_json writable");
        let updated = registry
            .get_agent("v8-worker")
            .expect("get updated")
            .expect("exists");
        assert_eq!(updated.runtime_version.as_deref(), Some("0.154.0"));
        assert_eq!(
            updated.driver_config_json.as_deref(),
            Some(r#"{"auth_method":"chatgpt","timeout_ms":30000}"#)
        );
    }

    // Column presence proven independently of the registry API.
    {
        let conn = Connection::open(&path).expect("reopen raw");
        assert!(column_exists(&conn, "agent_registry", "runtime_version"));
        assert!(column_exists(&conn, "agent_registry", "driver_config_json"));
        assert!(table_exists(&conn, "team_final_task_refs"));
        assert!(table_exists(&conn, "team_final_artifact_refs"));
    }

    // Reopening again is idempotent: same version and no row loss.
    {
        let reopened = SqliteTaskBoard::open(Connection::open(&path).expect("reopen again"))
            .expect("idempotent reopen");
        assert_eq!(reopened.schema_version().expect("version"), SCHEMA_VERSION);
        drop(reopened);
        let conn = Connection::open(&path).expect("reopen for counts");
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM team_tasks"), 2);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM team_task_runs"), 1);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM artifacts"), 2);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM agent_registry"), 1);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM team_final_task_refs"), 1);
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM team_final_artifact_refs"),
            1
        );
        assert_eq!(user_version(&conn), SCHEMA_VERSION);
    }

    let _ = std::fs::remove_file(&path);
}

/// A v10-shaped database migrates to v11: `driver_config_json` is added and the
/// existing agent rows are preserved.
///
/// The v10 shape here is built by executing the current `SCHEMA` and dropping
/// the newest column. That is acceptable for the *previous-current* version:
/// unlike the historical v8 fixture, v10 is the immediately preceding shape of
/// the same DDL, so removing the one new column reconstructs it exactly. This
/// is not the S1-4 defect (which fabricated a v8 database from current DDL).
#[test]
fn v10_database_migrates_to_v11_adding_driver_config() {
    let path = temp_db("v10-to-v11");
    let _ = std::fs::remove_file(&path);
    {
        let conn = Connection::open(&path).expect("create v10 fixture");
        conn.execute_batch(SCHEMA).expect("apply current schema");
        conn.execute(
            "ALTER TABLE agent_registry DROP COLUMN driver_config_json",
            [],
        )
        .expect("drop the v11 column to reconstruct v10");
        conn.pragma_update(None, "user_version", 10)
            .expect("stamp v10");
        conn.execute(
            "INSERT INTO agent_registry
                 (id, name, tier, driver_kind, executable, runtime_version,
                  driver_args_json, max_concurrency, tags_json)
             VALUES ('old-worker', 'old worker', 'worker', 'acp', 'codex', '0.1.0',
                     '[\"--acp\"]', 4, '[\"qwen\"]')",
            [],
        )
        .expect("seed v10 agent");

        assert_eq!(user_version(&conn), 10);
        assert!(!column_exists(
            &conn,
            "agent_registry",
            "driver_config_json"
        ));
    }

    let registry = SqliteAgentRegistry::open(&path).expect("migrate v10 to v11");
    assert_eq!(registry.schema_version().expect("version"), SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 11);
    let old = registry
        .get_agent("old-worker")
        .expect("get old agent")
        .expect("old agent preserved");
    assert_eq!(old.name, "old worker");
    assert_eq!(old.max_concurrency, Some(4));
    assert_eq!(old.runtime_version.as_deref(), Some("0.1.0"));
    assert_eq!(old.driver_config_json, None);

    registry
        .upsert_agent(&agent("old-worker"))
        .expect("write driver config after migration");
    assert_eq!(
        registry
            .get_agent("old-worker")
            .expect("get")
            .expect("exists")
            .driver_config_json
            .as_deref(),
        Some(r#"{"auth_method":"chatgpt","timeout_ms":30000}"#)
    );

    drop(registry);
    let conn = Connection::open(&path).expect("reopen raw");
    assert!(column_exists(&conn, "agent_registry", "driver_config_json"));
    assert_eq!(user_version(&conn), 11);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM agent_registry"), 1);
    let _ = std::fs::remove_file(&path);
}
