/// Product database generation. Historical releases are read only through explicit import.
pub const SCHEMA_VERSION: i32 = 14;

/// The current durable product truth: tasks, attempts, artifacts, bindings, events,
/// registry, and the two explicit final selection relations.
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS team_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    objective TEXT NOT NULL,
    parent_task INTEGER,
    kind TEXT NOT NULL,
    target TEXT,
    assignee TEXT,
    status TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS team_task_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES team_tasks(id),
    attempt INTEGER NOT NULL,
    agent_id TEXT NOT NULL,
    status TEXT NOT NULL,
    result TEXT,
    error TEXT
);
CREATE TABLE IF NOT EXISTS artifacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES team_tasks(id),
    path TEXT NOT NULL,
    sha256 TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS external_runtime_bindings (
    team_task_id INTEGER NOT NULL REFERENCES team_tasks(id),
    attempt INTEGER NOT NULL,
    agent_id TEXT NOT NULL,
    runtime_kind TEXT NOT NULL,
    native_thread_id TEXT,
    native_turn_id TEXT,
    lifecycle_state TEXT NOT NULL,
    runtime_name TEXT,
    runtime_version TEXT,
    protocol_kind TEXT,
    protocol_version TEXT,
    capabilities_json TEXT,
    started_at TEXT,
    finished_at TEXT,
    PRIMARY KEY (team_task_id, attempt)
);
CREATE TABLE IF NOT EXISTS runtime_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_task_id INTEGER NOT NULL REFERENCES team_tasks(id),
    attempt INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    event_kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(team_task_id, attempt, sequence)
);
CREATE TABLE IF NOT EXISTS agent_registry (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    tier TEXT NOT NULL,
    driver_kind TEXT,
    executable TEXT,
    runtime_version TEXT,
    driver_args_json TEXT,
    max_concurrency INTEGER NOT NULL,
    tags_json TEXT,
    driver_config_json TEXT
);
CREATE TABLE IF NOT EXISTS team_final_task_refs (
    root_task_id INTEGER NOT NULL REFERENCES team_tasks(id),
    selected_task_id INTEGER NOT NULL REFERENCES team_tasks(id),
    PRIMARY KEY (root_task_id, selected_task_id)
);
CREATE TABLE IF NOT EXISTS team_final_artifact_refs (
    root_task_id INTEGER NOT NULL REFERENCES team_tasks(id),
    task_id INTEGER NOT NULL REFERENCES team_tasks(id),
    path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    PRIMARY KEY (root_task_id, task_id, path, sha256)
);
"#;

/// Initialize an empty database or open generation 14. Older databases must be
/// imported into a separate file, never changed by ordinary product startup.
pub fn initialize_schema(conn: &mut rusqlite::Connection) -> Result<(), rusqlite::Error> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let version: i32 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let tables: i64 = tx.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get(0),
    )?;
    if version == SCHEMA_VERSION {
        let names = tx.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        let expected = [
            "agent_registry",
            "artifacts",
            "external_runtime_bindings",
            "runtime_events",
            "team_final_artifact_refs",
            "team_final_task_refs",
            "team_task_runs",
            "team_tasks",
        ];
        if names.iter().map(String::as_str).collect::<Vec<_>>() != expected {
            return Err(rusqlite::Error::InvalidParameterName(
                "generation 14 product table set is invalid".into(),
            ));
        }
        return tx.commit();
    }
    if version != 0 || tables != 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!("database schema {version} cannot be opened as generation 14; use am import <source-database> to create a separate product database")));
    }
    tx.execute_batch(SCHEMA)?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()
}
