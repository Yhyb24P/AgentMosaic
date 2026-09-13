//! Creates a minimal pre-v9 SQLite team-board fixture for release smoke tests.

use agent_code_storage::SCHEMA;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: make_v8_fixture <database>");
        std::process::exit(2);
    };
    let conn = rusqlite::Connection::open(path).expect("open fixture database");
    conn.execute_batch(SCHEMA).expect("create v8 base schema");
    conn.execute_batch(
        "DROP TABLE team_final_artifact_refs;
         DROP TABLE team_final_task_refs;",
    )
    .expect("remove v9 tables");
    conn.pragma_update(None, "user_version", 8)
        .expect("set v8 schema version");
    conn.execute(
        "INSERT INTO team_tasks (objective, kind, status) VALUES ('preserved v8 release task', 'bulk', 'pending')",
        [],
    )
    .expect("seed v8 task");
}
