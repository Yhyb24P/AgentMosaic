//! Creates the authentic schema-v8 SQLite fixture for migration tests and
//! release smoke runs: the historical v8 DDL plus representative old rows.

/// The historical fixture is kept as immutable SQL next to the storage tests.
const V8_SCHEMA: &str = include_str!("../../tests/fixtures/schema_v8.sql");

/// The version the historical fixture is stamped with. Kept as a literal: the
/// fixture represents commit e7649230, not whatever the crate writes today.
const V8_SCHEMA_VERSION: i32 = 8;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: make_v8_fixture <database>");
        std::process::exit(2);
    };
    let conn = rusqlite::Connection::open(path).expect("open fixture database");
    conn.execute_batch(V8_SCHEMA)
        .expect("apply historical v8 schema");
    conn.pragma_update(None, "user_version", V8_SCHEMA_VERSION)
        .expect("stamp v8 schema version");
    // Representative v8 rows, seeded only with columns the historical DDL has.
    conn.execute(
        "INSERT INTO team_tasks (objective, kind, status) VALUES ('preserved v8 release task', 'bulk', 'pending')",
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
    // `runtime_version` and `driver_config_json` do not exist at v8.
    conn.execute(
        "INSERT INTO agent_registry
             (id, name, tier, driver_kind, executable, driver_args_json, max_concurrency, tags_json)
         VALUES ('v8-worker', 'v8 worker', 'worker', 'acp', 'codex', '[\"--acp\"]', 1, '[\"qwen\"]')",
        [],
    )
    .expect("seed v8 agent");
}
