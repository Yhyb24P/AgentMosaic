//! The SQLite implementation of the durable runtime agent registry.

use rusqlite::{params, Connection, Row};

use crate::schema::initialize_schema;

/// A persisted registration of one runtime agent (one `agent_registry` row).
#[derive(Debug, Clone)]
pub struct AgentRegistryRecord {
    pub id: String,
    pub name: String,
    /// One of `reasoner`, `worker`, or `utility`.
    pub tier: String,
    /// Raw persisted driver spelling, including retired values; decoding and
    /// runnable driver selection happen at the registry boundary.
    pub driver_kind: Option<String>,
    /// The external runtime executable, when configured.
    pub executable: Option<String>,
    /// Public runtime version observed by a probe, never a credential or endpoint.
    pub runtime_version: Option<String>,
    /// The driver argv as a JSON array string.
    pub driver_args_json: Option<String>,
    /// The per-agent concurrency limit; 0 means unlimited.
    pub max_concurrency: Option<i64>,
    /// The agent tags as a JSON array string.
    pub tags_json: Option<String>,
    /// Non-secret driver options as a JSON object string, or None.
    pub driver_config_json: Option<String>,
}

/// A durable runtime agent registry backed by a single SQLite connection.
pub struct SqliteAgentRegistry {
    conn: Connection,
}

impl SqliteAgentRegistry {
    /// Open the registry on `path`, initializing empty state or validating the current generation.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, rusqlite::Error> {
        let mut conn = Connection::open(path)?;
        initialize_schema(&mut conn)?;
        Ok(Self { conn })
    }

    /// The schema version of the underlying database.
    pub fn schema_version(&self) -> Result<i32, rusqlite::Error> {
        self.conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
    }

    /// Idempotently upsert a registration, keyed by agent id.
    pub fn upsert_agent(&self, record: &AgentRegistryRecord) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO agent_registry (
                     id, name, tier, driver_kind, executable, runtime_version,
                     driver_args_json, max_concurrency, tags_json, driver_config_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                     name = excluded.name,
                     tier = excluded.tier,
                     driver_kind = excluded.driver_kind,
                     executable = excluded.executable,
                     runtime_version = excluded.runtime_version,
                     driver_args_json = excluded.driver_args_json,
                     max_concurrency = excluded.max_concurrency,
                     tags_json = excluded.tags_json,
                     driver_config_json = excluded.driver_config_json",
            params![
                record.id,
                record.name,
                record.tier,
                record.driver_kind,
                record.executable,
                record.runtime_version,
                record.driver_args_json,
                record.max_concurrency,
                record.tags_json,
                record.driver_config_json
            ],
        )?;
        Ok(())
    }

    /// Read one registration by agent id.
    pub fn get_agent(&self, id: &str) -> Result<Option<AgentRegistryRecord>, rusqlite::Error> {
        let row = self.conn.query_row(
            "SELECT id, name, tier, driver_kind, executable, runtime_version, driver_args_json,
                    max_concurrency, tags_json, driver_config_json
             FROM agent_registry WHERE id = ?1",
            params![id],
            from_row,
        );
        match row {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Remove one registration, reporting whether a row was deleted.
    ///
    /// Deletion is deliberately narrow: it touches `agent_registry` only. The
    /// durable task board, attempts, results, artifacts, bindings and final
    /// references are history and outlive the Agent registration.
    pub fn delete_agent(&self, id: &str) -> Result<bool, rusqlite::Error> {
        let removed = self
            .conn
            .execute("DELETE FROM agent_registry WHERE id = ?1", params![id])?;
        Ok(removed > 0)
    }

    /// List all registrations in id order.
    pub fn list_agents(&self) -> Result<Vec<AgentRegistryRecord>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, tier, driver_kind, executable, runtime_version,
                        driver_args_json, max_concurrency, tags_json, driver_config_json
                 FROM agent_registry ORDER BY id",
        )?;
        let rows = stmt.query_map(params![], from_row)?;
        rows.collect()
    }
}

/// Reconstruct one registry row.
fn from_row(row: &Row) -> rusqlite::Result<AgentRegistryRecord> {
    Ok(AgentRegistryRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        tier: row.get(2)?,
        driver_kind: row.get(3)?,
        executable: row.get(4)?,
        runtime_version: row.get(5)?,
        driver_args_json: row.get(6)?,
        max_concurrency: row.get(7)?,
        tags_json: row.get(8)?,
        driver_config_json: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {

    use super::*;

    fn temp_db(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "agentmosaic_registry_{name}_{}.db",
            std::process::id()
        ))
    }

    fn record() -> AgentRegistryRecord {
        AgentRegistryRecord {
            id: "acp-worker".into(),
            name: "acp worker".into(),
            tier: "worker".into(),
            driver_kind: Some("acp".into()),
            executable: Some("codex".into()),
            runtime_version: Some("0.154.0".into()),
            driver_args_json: Some(r#"["-w", "--acp"]"#.into()),
            max_concurrency: Some(2),
            tags_json: Some(r#"["qwen"]"#.into()),
            driver_config_json: Some(r#"{"auth_method":"chatgpt"}"#.into()),
        }
    }

    /// Upsert, reopen and upsert once more: the row count stays one and the
    /// row carries the latest values.
    #[test]
    fn upsert_is_idempotent_across_reopen() {
        let path = temp_db("upsert");
        let _ = std::fs::remove_file(&path);
        {
            let registry = SqliteAgentRegistry::open(&path).expect("open registry");
            registry.upsert_agent(&record()).expect("upsert");
        }
        let registry = SqliteAgentRegistry::open(&path).expect("reopen registry");
        let mut updated = record();
        updated.name = "renamed".into();
        registry.upsert_agent(&updated).expect("upsert again");
        let agents = registry.list_agents().expect("list");
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, "acp-worker");
        assert_eq!(agents[0].name, "renamed");
        assert_eq!(agents[0].max_concurrency, Some(2));
        let _ = std::fs::remove_file(&path);
    }

    /// The raw JSON columns round-trip verbatim and NULL stays NULL.
    #[test]
    fn json_columns_roundtrip_verbatim() {
        let path = temp_db("json");
        let _ = std::fs::remove_file(&path);
        let registry = SqliteAgentRegistry::open(&path).expect("open registry");
        let mut upserted = record();
        upserted.id = "json-worker".into();
        upserted.driver_args_json = Some(r#"["a", "b"]"#.into());
        upserted.tags_json = None;
        registry.upsert_agent(&upserted).expect("upsert");
        let read = registry
            .get_agent("json-worker")
            .expect("get")
            .expect("exists");
        assert_eq!(read.driver_args_json.as_deref(), Some(r#"["a", "b"]"#));
        assert_eq!(read.tags_json.as_deref(), None);
        assert_eq!(read.runtime_version.as_deref(), Some("0.154.0"));
        let _ = std::fs::remove_file(&path);
    }

    /// Non-secret driver options round-trip through upsert/get/list, and a
    /// missing config stays None.
    #[test]
    fn driver_config_json_roundtrips_through_upsert_get_and_list() {
        let path = temp_db("driver-config");
        let _ = std::fs::remove_file(&path);
        let registry = SqliteAgentRegistry::open(&path).expect("open registry");
        let mut configured = record();
        configured.driver_config_json = Some(r#"{"timeout_ms":30000,"artifact_paths":[]}"#.into());
        registry
            .upsert_agent(&configured)
            .expect("upsert configured");
        let mut bare = record();
        bare.id = "bare-worker".into();
        bare.driver_config_json = None;
        registry.upsert_agent(&bare).expect("upsert bare");

        let read = registry
            .get_agent("acp-worker")
            .expect("get")
            .expect("exists");
        assert_eq!(
            read.driver_config_json.as_deref(),
            Some(r#"{"timeout_ms":30000,"artifact_paths":[]}"#)
        );
        assert_eq!(
            registry
                .get_agent("bare-worker")
                .expect("get bare")
                .expect("exists")
                .driver_config_json,
            None
        );
        let listed = registry.list_agents().expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(
            listed[0].driver_config_json.as_deref(),
            Some(r#"{"timeout_ms":30000,"artifact_paths":[]}"#)
        );
        assert_eq!(listed[1].driver_config_json, None);
        let _ = std::fs::remove_file(&path);
    }

    /// Changing the config replaces it; clearing it writes NULL.
    #[test]
    fn driver_config_json_upsert_replaces_and_clears() {
        let path = temp_db("driver-config-update");
        let _ = std::fs::remove_file(&path);
        let registry = SqliteAgentRegistry::open(&path).expect("open registry");
        registry.upsert_agent(&record()).expect("upsert initial");
        let mut updated = record();
        updated.driver_config_json = Some(r#"{"max_prompt_bytes":500}"#.into());
        registry.upsert_agent(&updated).expect("upsert update");
        assert_eq!(
            registry
                .get_agent("acp-worker")
                .expect("get")
                .expect("exists")
                .driver_config_json
                .as_deref(),
            Some(r#"{"max_prompt_bytes":500}"#)
        );
        updated.driver_config_json = None;
        registry.upsert_agent(&updated).expect("upsert clear");
        assert_eq!(
            registry
                .get_agent("acp-worker")
                .expect("get")
                .expect("exists")
                .driver_config_json,
            None
        );
        let _ = std::fs::remove_file(&path);
    }
}
