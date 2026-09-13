-- Historical agentmosaic-storage schema v8, kept as an immutable migration fixture.
--
-- Provenance:
--   commit:         e7649230af388aa61fb851f1c4631e679b08e49b
--   path:           crates/agentmosaic-storage/src/schema.rs
--   blob sha:       21d10ff9c45e444f29b37f9082d1fd99b6333b56
--   schema version: 8 (SCHEMA_VERSION = 8)
--
-- The SQL below is the verbatim body of the `SCHEMA` const at that commit,
-- written as SQL. It is immutable historical DDL: never regenerate it from the
-- current `SCHEMA`, and never drop or "fix" parts of it. This file deliberately
-- does not set `user_version`; the fixture generator and tests stamp 8.

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    state TEXT NOT NULL,
    active_call INTEGER,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS agent_turns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    decision TEXT NOT NULL,
    error TEXT
);
CREATE TABLE IF NOT EXISTS tool_calls (
    session_id TEXT NOT NULL,
    call_id INTEGER NOT NULL,
    state TEXT NOT NULL,
    request TEXT,
    PRIMARY KEY (session_id, call_id)
);
CREATE TABLE IF NOT EXISTS checkpoints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    git_head TEXT NOT NULL
);
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
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_agent TEXT NOT NULL,
    to_agent TEXT NOT NULL,
    body TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS artifacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT,
    task_id INTEGER,
    path TEXT NOT NULL,
    sha256 TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS transitions (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    from_state TEXT NOT NULL,
    to_state TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS acc_tasks (
    task_id TEXT PRIMARY KEY,
    contract_json TEXT NOT NULL,
    state TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS acc_dependencies (
    predecessor TEXT NOT NULL REFERENCES acc_tasks(task_id),
    successor TEXT NOT NULL REFERENCES acc_tasks(task_id),
    kind TEXT NOT NULL,
    PRIMARY KEY (predecessor, successor, kind)
);
CREATE TABLE IF NOT EXISTS acc_context_manifests (
    manifest_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES acc_tasks(task_id),
    manifest_json TEXT NOT NULL,
    manifest_sha256 TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS acc_artifacts (
    artifact_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES acc_tasks(task_id),
    sha256 TEXT NOT NULL,
    version INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS acc_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    task_id TEXT NOT NULL REFERENCES acc_tasks(task_id),
    event_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS external_runtime_bindings (
    team_task_id INTEGER NOT NULL REFERENCES team_tasks(id),
    attempt INTEGER NOT NULL,
    agent_id TEXT NOT NULL,
    runtime_kind TEXT NOT NULL,
    native_thread_id TEXT,
    native_turn_id TEXT,
    lifecycle_state TEXT NOT NULL,
    PRIMARY KEY (team_task_id, attempt)
);
CREATE TABLE IF NOT EXISTS runtime_collaboration_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_task_id INTEGER NOT NULL REFERENCES team_tasks(id),
    attempt INTEGER NOT NULL,
    runtime_kind TEXT NOT NULL,
    native_call_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload_summary TEXT NOT NULL,
    response_summary TEXT,
    UNIQUE (runtime_kind, native_call_id)
);
CREATE TABLE IF NOT EXISTS agent_registry (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    tier TEXT NOT NULL,
    driver_kind TEXT,
    executable TEXT,
    driver_args_json TEXT,
    max_concurrency INTEGER NOT NULL,
    tags_json TEXT
);
