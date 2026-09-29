//! Explicit projection of published schema 11 or development schema 12 into
//! a new product database. The source is opened read only and left intact.
use rusqlite::{types::Value, Connection, OpenFlags, OptionalExtension};
use std::path::Path;

#[derive(Debug)]
pub struct ImportError(String);
impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ImportError {}
impl From<rusqlite::Error> for ImportError {
    fn from(error: rusqlite::Error) -> Self {
        Self(format!("database import: {error}"))
    }
}
impl From<std::io::Error> for ImportError {
    fn from(error: std::io::Error) -> Self {
        Self(format!("database import: {error}"))
    }
}
fn reject(message: impl Into<String>) -> ImportError {
    ImportError(message.into())
}

/// Create a separate generation 14 database. Existing destinations, unpublished
/// schemas other than 12, malformed references, and session-only artifacts are
/// rejected. No driver kind is translated into a runnable replacement.
pub fn import_database(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<(), ImportError> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    let mut input = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let snapshot = input.transaction()?;
    let version: i32 = snapshot.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if !matches!(version, 11 | 12) {
        return Err(reject(format!("schema {version} is not supported for import; only published schema 11 and development schema 12 are supported; experimental schema 13 and unknown/future schemas must remain separate")));
    }
    let integrity: String = snapshot.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(reject(format!(
            "source integrity check failed: {integrity}"
        )));
    }
    let session_artifacts: i64 = snapshot.query_row(
        "SELECT COUNT(*) FROM artifacts WHERE task_id IS NULL",
        [],
        |row| row.get(0),
    )?;
    if session_artifacts != 0 {
        return Err(reject(format!("source contains {session_artifacts} session-only artifacts; retain the source database and export those files separately before importing a task-only copy")));
    }
    let checks = [
        ("task states or kinds", "SELECT COUNT(*) FROM team_tasks WHERE id <= 0 OR kind IS NULL OR kind NOT IN ('reasoning','review','bulk','tool','utility') OR status IS NULL OR status NOT IN ('pending','assigned','running','succeeded','failed','cancelled')"),
        ("attempt states", "SELECT COUNT(*) FROM team_task_runs WHERE status IS NULL OR status NOT IN ('pending','assigned','running','succeeded','failed','cancelled')"),
        ("task parents", "SELECT COUNT(*) FROM team_tasks t WHERE t.parent_task IS NOT NULL AND NOT EXISTS(SELECT 1 FROM team_tasks p WHERE p.id=t.parent_task)"),
        ("attempts", "SELECT COUNT(*) FROM team_task_runs r WHERE r.attempt <= 0 OR r.attempt > 4294967295 OR NOT EXISTS(SELECT 1 FROM team_tasks t WHERE t.id=r.task_id)"),
        ("artifacts", "SELECT COUNT(*) FROM artifacts a WHERE NOT EXISTS(SELECT 1 FROM team_tasks t WHERE t.id=a.task_id)"),
        ("bindings", "SELECT COUNT(*) FROM external_runtime_bindings b WHERE NOT EXISTS(SELECT 1 FROM team_task_runs r WHERE r.task_id=b.team_task_id AND r.attempt=b.attempt)"),
        ("final task refs", "SELECT COUNT(*) FROM team_final_task_refs f WHERE NOT EXISTS(SELECT 1 FROM team_tasks t WHERE t.id=f.root_task_id) OR NOT EXISTS(SELECT 1 FROM team_tasks t WHERE t.id=f.selected_task_id)"),
        ("final artifact refs", "SELECT COUNT(*) FROM team_final_artifact_refs f WHERE NOT EXISTS(SELECT 1 FROM team_tasks t WHERE t.id=f.root_task_id) OR NOT EXISTS(SELECT 1 FROM artifacts a WHERE a.task_id=f.task_id AND a.path=f.path AND a.sha256=f.sha256)"),
        ("duplicate attempts", "SELECT COUNT(*) FROM (SELECT task_id,attempt FROM team_task_runs GROUP BY task_id,attempt HAVING COUNT(*) > 1)"),
    ];
    for (label, sql) in checks {
        let count: i64 = snapshot.query_row(sql, [], |row| row.get(0))?;
        if count != 0 {
            return Err(reject(format!(
                "source contains {count} invalid {label}; repair a copy before import"
            )));
        }
    }
    let parents = snapshot
        .prepare("SELECT id,parent_task FROM team_tasks")?
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
        })?
        .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
    for &id in parents.keys() {
        let mut visited = std::collections::HashSet::new();
        let mut current = Some(id);
        while let Some(task) = current {
            if !visited.insert(task) || visited.len() > 1024 {
                return Err(reject(
                    "source task hierarchy contains a cycle or exceeds 1024 parent hops",
                ));
            }
            current = parents[&task];
        }
    }
    let final_refs = snapshot
        .prepare("SELECT root_task_id, selected_task_id FROM team_final_task_refs")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (root, selected) in final_refs {
        let mut ancestor = parents[&selected];
        while ancestor.is_some() && ancestor != Some(root) {
            ancestor = parents[&ancestor.unwrap()];
        }
        let completed: bool = snapshot.query_row(
            "SELECT status='succeeded' FROM team_tasks WHERE id=?1",
            [selected],
            |r| r.get(0),
        )?;
        let valid_root: bool = snapshot.query_row(
            "SELECT parent_task IS NULL AND kind='reasoning' FROM team_tasks WHERE id=?1",
            [root],
            |r| r.get(0),
        )?;
        if root == selected || ancestor != Some(root) || !completed || !valid_root {
            return Err(reject("source final task references must select succeeded descendants of a reasoning root"));
        }
    }
    let missing_selection: i64 = snapshot.query_row("SELECT COUNT(*) FROM team_final_artifact_refs a WHERE NOT EXISTS(SELECT 1 FROM team_final_task_refs t WHERE t.root_task_id=a.root_task_id AND t.selected_task_id=a.task_id)",[],|r|r.get(0))?;
    if missing_selection != 0 {
        return Err(reject(
            "source final artifacts must belong to explicitly selected final tasks",
        ));
    }
    if version == 12 {
        let count: i64 = snapshot.query_row("SELECT COUNT(*) FROM runtime_events e WHERE NOT EXISTS(SELECT 1 FROM team_task_runs r WHERE r.task_id=e.team_task_id AND r.attempt=e.attempt)", [], |row| row.get(0))?;
        if count != 0 {
            return Err(reject(
                "source contains runtime events without matching attempts",
            ));
        }
    }
    if version == 12 {
        let mut statement = snapshot.prepare("SELECT team_task_id, attempt, sequence, event_kind, payload_json, created_at FROM runtime_events")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let task: i64 = row.get(0)?;
            let attempt: i64 = row.get(1)?;
            let sequence: i64 = row.get(2)?;
            if task <= 0 || attempt <= 0 || attempt > u32::MAX as i64 || sequence <= 0 {
                return Err(reject(
                    "source runtime event has invalid task/attempt/sequence",
                ));
            }
            crate::board::decode_stored_runtime_event(
                task as u64,
                attempt as u32,
                sequence,
                &row.get::<_, String>(3)?,
                &row.get::<_, String>(4)?,
                row.get(5)?,
            )
            .map_err(|error| reject(format!("source runtime event is invalid: {error}")))?;
        }
    }
    // Keep the destination absent until the entire projection commits. Publishing
    // a hard link is exclusive and atomic, including racing import attempts.
    if destination.symlink_metadata().is_ok() {
        return Err(reject(
            "destination already exists; import never overwrites a database",
        ));
    }
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| reject(error.to_string()))?
        .as_nanos();
    let mut temporary_name = destination.as_os_str().to_os_string();
    temporary_name.push(format!(".import-{}-{suffix}", std::process::id()));
    let temporary = std::path::PathBuf::from(temporary_name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let reserved = options.open(&temporary)?;
    drop(reserved);
    let result = (|| -> Result<(), ImportError> {
        let mut output = Connection::open(&temporary)?;
        crate::initialize_schema(&mut output)?;
        let tx = output.transaction()?;
        for table in [
            "team_tasks",
            "team_task_runs",
            "artifacts",
            "agent_registry",
            "external_runtime_bindings",
            "team_final_task_refs",
            "team_final_artifact_refs",
            "runtime_events",
        ] {
            if table == "runtime_events" && version == 11 {
                continue;
            }
            let target_cols = tx
                .prepare(&format!("PRAGMA table_info({table})"))?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            let source_cols = snapshot
                .prepare(&format!("PRAGMA table_info({table})"))?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            if source_cols.is_empty() {
                return Err(reject(format!("source is missing required table {table}")));
            }
            for column in &target_cols {
                let added_in_v12 = table == "external_runtime_bindings"
                    && [
                        "runtime_name",
                        "runtime_version",
                        "protocol_kind",
                        "protocol_version",
                        "capabilities_json",
                        "started_at",
                        "finished_at",
                    ]
                    .contains(&column.as_str());
                if !(source_cols.contains(column) || version == 11 && added_in_v12) {
                    return Err(reject(format!(
                        "source schema {version} is missing required column {table}.{column}"
                    )));
                }
            }
            let cols = target_cols
                .into_iter()
                .filter(|col| source_cols.contains(col))
                .collect::<Vec<_>>();
            let list = cols.join(",");
            let mut read = snapshot.prepare(&format!("SELECT {list} FROM {table}"))?;
            let rows = read.query_map([], |row| {
                (0..cols.len())
                    .map(|i| row.get::<_, Value>(i))
                    .collect::<Result<Vec<_>, _>>()
            })?;
            let marks = vec!["?"; cols.len()].join(",");
            let mut write =
                tx.prepare(&format!("INSERT INTO {table} ({list}) VALUES ({marks})"))?;
            for row in rows {
                write.execute(rusqlite::params_from_iter(row?))?;
            }
        }
        // Preserve AUTOINCREMENT high-water marks even when the last issued
        // identifiers were deleted. Reusing those IDs could alias old references.
        for table in [
            "team_tasks",
            "team_task_runs",
            "artifacts",
            "runtime_events",
        ] {
            if table == "runtime_events" && version == 11 {
                continue;
            }
            let source_sequence: Option<i64> = snapshot
                .query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name=?1",
                    [table],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(sequence) = source_sequence {
                let copied_sequence: Option<i64> = tx
                    .query_row(
                        "SELECT seq FROM sqlite_sequence WHERE name=?1",
                        [table],
                        |r| r.get(0),
                    )
                    .optional()?;
                tx.execute("DELETE FROM sqlite_sequence WHERE name=?1", [table])?;
                tx.execute(
                    "INSERT INTO sqlite_sequence(name,seq) VALUES(?1,?2)",
                    rusqlite::params![table, sequence.max(copied_sequence.unwrap_or(0))],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    })();
    let result = result.and_then(|()| {
        std::fs::File::open(&temporary)?.sync_all()?;
        std::fs::hard_link(&temporary, destination)?;
        Ok(())
    });
    let _ = std::fs::remove_file(&temporary);
    result
}
