//! The SQLite implementation of the team's durable task board.

use agentmosaic_team::{
    AgentMessage, ArtifactMeta, BoardError, RuntimeEventPolicy, RuntimeEventRecord,
    SelectedArtifactRef, TaskAttempt, TaskBoard, TaskKind, TaskRecord, TaskStatus,
    MAX_DURABLE_RUNTIME_PAYLOAD_BYTES,
};
use rusqlite::{params, Connection, Row, TransactionBehavior};
use std::time::Duration;

use crate::schema::{migrate, SCHEMA};

/// Seconds since the Unix epoch, the durable timestamp form this board writes.
fn now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// Product reads are always bounded even when a caller supplies a larger
/// value. Follow-mode callers page with `after_sequence`.
pub const MAX_RUNTIME_EVENT_QUERY: usize = 1_000;

/// A durable task board backed by a SQLite connection.
pub struct SqliteTaskBoard {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRuntimeBinding {
    pub team_task_id: u64,
    pub attempt: u32,
    pub agent_id: String,
    pub runtime_kind: String,
    pub native_thread_id: Option<String>,
    pub native_turn_id: Option<String>,
    pub lifecycle_state: String,
}

/// v12 metadata added to the existing per-attempt binding. The legacy binding
/// API remains source-compatible and does not clear these fields when it
/// updates lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendedExternalRuntimeBinding {
    pub binding: ExternalRuntimeBinding,
    pub runtime_name: Option<String>,
    pub runtime_version: Option<String>,
    pub protocol_kind: Option<String>,
    pub protocol_version: Option<String>,
    pub capabilities_json: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredRuntimeEvent {
    pub sequence: u64,
    pub created_at: String,
    pub record: RuntimeEventRecord,
}

#[derive(Debug)]
pub enum RuntimeEventStoreError {
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    LiveOnly(String),
    MissingAttempt { task_id: u64, attempt: u32 },
    Corrupt(String),
    PayloadTooLarge(usize),
}

impl std::fmt::Display for RuntimeEventStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "runtime event storage: {error}"),
            Self::Json(error) => write!(formatter, "runtime event JSON: {error}"),
            Self::LiveOnly(kind) => write!(formatter, "runtime event `{kind}` is live-only"),
            Self::MissingAttempt { task_id, attempt } => {
                write!(formatter, "task {task_id} has no attempt {attempt}")
            }
            Self::Corrupt(detail) => write!(formatter, "corrupt runtime event: {detail}"),
            Self::PayloadTooLarge(bytes) => write!(
                formatter,
                "runtime event payload is {bytes} bytes; maximum is {MAX_DURABLE_RUNTIME_PAYLOAD_BYTES}"
            ),
        }
    }
}

impl std::error::Error for RuntimeEventStoreError {}

impl From<rusqlite::Error> for RuntimeEventStoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<serde_json::Error> for RuntimeEventStoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCollaborationRecord {
    pub team_task_id: u64,
    pub attempt: u32,
    pub runtime_kind: String,
    pub native_call_id: String,
    pub kind: String,
    pub payload_summary: String,
    pub response_summary: Option<String>,
}

impl SqliteTaskBoard {
    /// Open a board on a connection, applying the schema and migrating.
    pub fn open(mut conn: Connection) -> Result<Self, rusqlite::Error> {
        // Scheduler drivers bind external runtimes from independently opened
        // connections while another task may settle on the canonical board.
        // Wait briefly for SQLite's short writer lock rather than converting a
        // benign contention race into a spurious runtime failure.
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Open a board on an in-memory database.
    pub fn in_memory() -> Result<Self, rusqlite::Error> {
        Self::open(Connection::open_in_memory()?)
    }

    /// The database's schema version.
    pub fn schema_version(&self) -> Result<i32, rusqlite::Error> {
        self.conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
    }

    pub fn upsert_external_binding(
        &self,
        binding: &ExternalRuntimeBinding,
    ) -> Result<(), rusqlite::Error> {
        self.conn.execute("INSERT INTO external_runtime_bindings (team_task_id, attempt, agent_id, runtime_kind, native_thread_id, native_turn_id, lifecycle_state) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(team_task_id,attempt) DO UPDATE SET native_thread_id=excluded.native_thread_id,native_turn_id=excluded.native_turn_id,lifecycle_state=excluded.lifecycle_state", params![binding.team_task_id as i64,binding.attempt as i64,binding.agent_id,binding.runtime_kind,binding.native_thread_id,binding.native_turn_id,binding.lifecycle_state])?;
        Ok(())
    }

    pub fn upsert_external_binding_extended(
        &self,
        extended: &ExtendedExternalRuntimeBinding,
    ) -> Result<(), rusqlite::Error> {
        let binding = &extended.binding;
        self.conn.execute(
            "INSERT INTO external_runtime_bindings (
                team_task_id,attempt,agent_id,runtime_kind,native_thread_id,native_turn_id,
                lifecycle_state,runtime_name,runtime_version,protocol_kind,protocol_version,
                capabilities_json,started_at,finished_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(team_task_id,attempt) DO UPDATE SET
                agent_id=excluded.agent_id,
                runtime_kind=excluded.runtime_kind,
                native_thread_id=COALESCE(excluded.native_thread_id,native_thread_id),
                native_turn_id=COALESCE(excluded.native_turn_id,native_turn_id),
                lifecycle_state=excluded.lifecycle_state,
                runtime_name=COALESCE(excluded.runtime_name,runtime_name),
                runtime_version=COALESCE(excluded.runtime_version,runtime_version),
                protocol_kind=COALESCE(excluded.protocol_kind,protocol_kind),
                protocol_version=COALESCE(excluded.protocol_version,protocol_version),
                capabilities_json=COALESCE(excluded.capabilities_json,capabilities_json),
                started_at=COALESCE(excluded.started_at,started_at),
                finished_at=COALESCE(excluded.finished_at,finished_at)",
            params![
                binding.team_task_id as i64,
                binding.attempt as i64,
                binding.agent_id,
                binding.runtime_kind,
                binding.native_thread_id,
                binding.native_turn_id,
                binding.lifecycle_state,
                extended.runtime_name,
                extended.runtime_version,
                extended.protocol_kind,
                extended.protocol_version,
                extended.capabilities_json,
                extended.started_at,
                extended.finished_at,
            ],
        )?;
        Ok(())
    }

    pub fn external_binding(
        &self,
        task: u64,
        attempt: u32,
    ) -> Result<Option<ExternalRuntimeBinding>, rusqlite::Error> {
        let row = self.conn.query_row("SELECT team_task_id,attempt,agent_id,runtime_kind,native_thread_id,native_turn_id,lifecycle_state FROM external_runtime_bindings WHERE team_task_id=?1 AND attempt=?2", params![task as i64,attempt as i64], |r| Ok(ExternalRuntimeBinding { team_task_id:r.get::<_,i64>(0)? as u64,attempt:r.get::<_,i64>(1)? as u32,agent_id:r.get(2)?,runtime_kind:r.get(3)?,native_thread_id:r.get(4)?,native_turn_id:r.get(5)?,lifecycle_state:r.get(6)? }));
        match row {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn external_binding_extended(
        &self,
        task: u64,
        attempt: u32,
    ) -> Result<Option<ExtendedExternalRuntimeBinding>, rusqlite::Error> {
        let row = self.conn.query_row(
            "SELECT team_task_id,attempt,agent_id,runtime_kind,native_thread_id,native_turn_id,
                    lifecycle_state,runtime_name,runtime_version,protocol_kind,protocol_version,
                    capabilities_json,started_at,finished_at
             FROM external_runtime_bindings WHERE team_task_id=?1 AND attempt=?2",
            params![task as i64, attempt as i64],
            |row| {
                Ok(ExtendedExternalRuntimeBinding {
                    binding: ExternalRuntimeBinding {
                        team_task_id: row.get::<_, i64>(0)? as u64,
                        attempt: row.get::<_, i64>(1)? as u32,
                        agent_id: row.get(2)?,
                        runtime_kind: row.get(3)?,
                        native_thread_id: row.get(4)?,
                        native_turn_id: row.get(5)?,
                        lifecycle_state: row.get(6)?,
                    },
                    runtime_name: row.get(7)?,
                    runtime_version: row.get(8)?,
                    protocol_kind: row.get(9)?,
                    protocol_version: row.get(10)?,
                    capabilities_json: row.get(11)?,
                    started_at: row.get(12)?,
                    finished_at: row.get(13)?,
                })
            },
        );
        match row {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Persist one normalized durable observation. Sequence allocation and
    /// insertion share an IMMEDIATE transaction, so overlapping writers for
    /// the same attempt cannot allocate the same sequence.
    pub fn append_runtime_event(
        &mut self,
        record: RuntimeEventRecord,
    ) -> Result<StoredRuntimeEvent, RuntimeEventStoreError> {
        let record = record.bounded();
        if record.event.policy() != RuntimeEventPolicy::Durable {
            return Err(RuntimeEventStoreError::LiveOnly(
                record.event.kind().to_string(),
            ));
        }
        let payload = serde_json::to_string(&record)?;
        if payload.len() > MAX_DURABLE_RUNTIME_PAYLOAD_BYTES {
            return Err(RuntimeEventStoreError::PayloadTooLarge(payload.len()));
        }
        let created_at = now();
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let attempt_exists: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM team_task_runs WHERE task_id=?1 AND attempt=?2",
            params![record.task_id as i64, record.attempt as i64],
            |row| row.get(0),
        )?;
        if attempt_exists != 1 {
            return Err(RuntimeEventStoreError::MissingAttempt {
                task_id: record.task_id,
                attempt: record.attempt,
            });
        }
        let sequence: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(sequence),0)+1 FROM runtime_events
             WHERE team_task_id=?1 AND attempt=?2",
            params![record.task_id as i64, record.attempt as i64],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO runtime_events
                (team_task_id,attempt,sequence,event_kind,payload_json,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                record.task_id as i64,
                record.attempt as i64,
                sequence,
                record.event.kind(),
                payload,
                created_at,
            ],
        )?;
        transaction.commit()?;
        Ok(StoredRuntimeEvent {
            sequence: sequence as u64,
            created_at,
            record,
        })
    }

    pub fn runtime_events(
        &self,
        task: u64,
        attempt: u32,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<StoredRuntimeEvent>, RuntimeEventStoreError> {
        let limit = limit.min(MAX_RUNTIME_EVENT_QUERY);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut statement = self.conn.prepare(
            "SELECT sequence,event_kind,payload_json,created_at FROM runtime_events
             WHERE team_task_id=?1 AND attempt=?2 AND sequence>?3
             ORDER BY sequence LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![
                task as i64,
                attempt as i64,
                after_sequence as i64,
                limit as i64
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        rows.map(|row| {
            let (sequence, event_kind, payload, created_at) = row?;
            decode_stored_runtime_event(task, attempt, sequence, &event_kind, &payload, created_at)
        })
        .collect()
    }

    pub fn latest_runtime_events(
        &self,
        task: u64,
        limit: usize,
    ) -> Result<Vec<StoredRuntimeEvent>, RuntimeEventStoreError> {
        let limit = limit.min(MAX_RUNTIME_EVENT_QUERY);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut statement = self.conn.prepare(
            "SELECT attempt,sequence,event_kind,payload_json,created_at FROM runtime_events
             WHERE team_task_id=?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![task as i64, limit as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut events = rows
            .map(|row| {
                let (attempt, sequence, event_kind, payload, created_at) = row?;
                decode_stored_runtime_event(
                    task,
                    attempt as u32,
                    sequence,
                    &event_kind,
                    &payload,
                    created_at,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        events.reverse();
        Ok(events)
    }

    /// Idempotently persists a bounded external tool request and its response.
    pub fn record_runtime_collaboration(
        &self,
        record: &RuntimeCollaborationRecord,
    ) -> Result<bool, rusqlite::Error> {
        Ok(self.conn.execute("INSERT INTO runtime_collaboration_records (team_task_id,attempt,runtime_kind,native_call_id,kind,payload_summary,response_summary) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(runtime_kind,native_call_id) DO NOTHING", params![record.team_task_id as i64,record.attempt as i64,record.runtime_kind,record.native_call_id,record.kind,record.payload_summary,record.response_summary])? == 1)
    }

    pub fn runtime_collaboration(
        &self,
        task: u64,
        attempt: u32,
    ) -> Result<Vec<RuntimeCollaborationRecord>, rusqlite::Error> {
        let mut s=self.conn.prepare("SELECT team_task_id,attempt,runtime_kind,native_call_id,kind,payload_summary,response_summary FROM runtime_collaboration_records WHERE team_task_id=?1 AND attempt=?2 ORDER BY id")?;
        let rows = s.query_map(params![task as i64, attempt as i64], |r| {
            Ok(RuntimeCollaborationRecord {
                team_task_id: r.get::<_, i64>(0)? as u64,
                attempt: r.get::<_, i64>(1)? as u32,
                runtime_kind: r.get(2)?,
                native_call_id: r.get(3)?,
                kind: r.get(4)?,
                payload_summary: r.get(5)?,
                response_summary: r.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Every user-visible run: a root `reasoning` task, ordered by id.
    ///
    /// These are inherent methods, not `TaskBoard` members: they read the
    /// durable row shapes the SQLite board actually stores, and adding them to
    /// the trait would force every in-memory board to reproduce them.
    pub fn root_tasks(&self) -> Result<Vec<TaskRecord>, BoardError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, objective, parent_task, kind, target, assignee, status
                 FROM team_tasks WHERE parent_task IS NULL AND kind = ?1 ORDER BY id",
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let rows = statement
            .query_map(params![TaskKind::Reasoning.as_str()], row_to_task)
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        rows.map(|row| row.map_err(|e| BoardError::Storage(e.to_string())))
            .collect()
    }

    /// The newest run, or `None` when this board has none.
    pub fn latest_root_task(&self) -> Result<Option<TaskRecord>, BoardError> {
        Ok(self.root_tasks()?.pop())
    }

    /// Every task whose `parent_task` chain reaches `root`, excluding `root`.
    ///
    /// The walk is bounded, so a malformed chain cannot spin forever: after
    /// [`MAX_DESCENDANT_HOPS`] the candidate is dropped.
    pub fn descendants_of(&self, root: u64) -> Result<Vec<u64>, BoardError> {
        let mut found = Vec::new();
        for id in self.task_ids()? {
            if id == root {
                continue;
            }
            let mut current = id;
            for _ in 0..MAX_DESCENDANT_HOPS {
                let Some(record) = self.task(current)? else {
                    break;
                };
                match record.parent_task {
                    Some(parent) if parent == root => {
                        found.push(id);
                        break;
                    }
                    Some(parent) => current = parent,
                    None => break,
                }
            }
        }
        Ok(found)
    }

    /// The underlying connection, for direct queries in tests.
    #[cfg(test)]
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// The bounded number of `parent_task` hops the descendant walk follows.
pub const MAX_DESCENDANT_HOPS: usize = 1024;

fn decode_stored_runtime_event(
    task: u64,
    attempt: u32,
    sequence: i64,
    event_kind: &str,
    payload: &str,
    created_at: String,
) -> Result<StoredRuntimeEvent, RuntimeEventStoreError> {
    let record = serde_json::from_str::<RuntimeEventRecord>(payload)?;
    if record.task_id != task || record.attempt != attempt {
        return Err(RuntimeEventStoreError::Corrupt(
            "payload task/attempt does not match row".into(),
        ));
    }
    if record.event.kind() != event_kind {
        return Err(RuntimeEventStoreError::Corrupt(format!(
            "row kind `{event_kind}` does not match payload kind `{}`",
            record.event.kind()
        )));
    }
    Ok(StoredRuntimeEvent {
        sequence: sequence as u64,
        created_at,
        record,
    })
}

/// Replace the Lead's explicit final selection for `root_task` inside an open
/// transaction. Shared by the standalone write and the root-final commit, so
/// both persist exactly the same rows.
fn write_final_refs(
    tx: &rusqlite::Transaction<'_>,
    root_task: u64,
    task_refs: &[u64],
    artifact_refs: &[SelectedArtifactRef],
) -> Result<(), BoardError> {
    let root_exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM team_tasks WHERE id = ?1)",
            params![root_task as i64],
            |row| row.get(0),
        )
        .map_err(|e| BoardError::Storage(e.to_string()))?;
    if !root_exists {
        return Err(BoardError::UnknownTask(root_task));
    }
    tx.execute(
        "DELETE FROM team_final_task_refs WHERE root_task_id = ?1",
        params![root_task as i64],
    )
    .map_err(|e| BoardError::Storage(e.to_string()))?;
    tx.execute(
        "DELETE FROM team_final_artifact_refs WHERE root_task_id = ?1",
        params![root_task as i64],
    )
    .map_err(|e| BoardError::Storage(e.to_string()))?;
    for task_id in task_refs {
        tx.execute(
            "INSERT INTO team_final_task_refs (root_task_id, selected_task_id) VALUES (?1, ?2)",
            params![root_task as i64, *task_id as i64],
        )
        .map_err(|e| BoardError::Storage(e.to_string()))?;
    }
    for selected in artifact_refs {
        tx.execute(
            "INSERT INTO team_final_artifact_refs (root_task_id, task_id, path, sha256)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                root_task as i64,
                selected.task_id as i64,
                selected.artifact.path,
                selected.artifact.sha256,
            ],
        )
        .map_err(|e| BoardError::Storage(e.to_string()))?;
    }
    Ok(())
}

impl TaskBoard for SqliteTaskBoard {
    fn create_task(
        &mut self,
        objective: &str,
        parent: Option<u64>,
        kind: TaskKind,
        target: Option<String>,
    ) -> Result<u64, BoardError> {
        self.conn
            .execute(
                "INSERT INTO team_tasks (objective, parent_task, kind, target, status)
                 VALUES (?1, ?2, ?3, ?4, 'pending')",
                params![objective, parent.map(|p| p as i64), kind.as_str(), target],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        Ok(self.conn.last_insert_rowid() as u64)
    }

    fn assign(&mut self, task: u64, agent: &str) -> Result<(), BoardError> {
        let n = self
            .conn
            .execute(
                "UPDATE team_tasks SET assignee = ?2, status = 'assigned' WHERE id = ?1",
                params![task as i64, agent],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if n == 0 {
            return Err(BoardError::UnknownTask(task));
        }
        Ok(())
    }

    fn set_status(&mut self, task: u64, status: TaskStatus) -> Result<(), BoardError> {
        let n = self
            .conn
            .execute(
                "UPDATE team_tasks SET status = ?2 WHERE id = ?1",
                params![task as i64, status.as_str()],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if n == 0 {
            return Err(BoardError::UnknownTask(task));
        }
        Ok(())
    }

    fn record_attempt(&mut self, attempt: &TaskAttempt) -> Result<(), BoardError> {
        self.conn
            .execute(
                "INSERT INTO team_task_runs (task_id, attempt, agent_id, status, result, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    attempt.task_id as i64,
                    attempt.attempt as i64,
                    attempt.agent_id,
                    attempt.status.as_str(),
                    attempt.result,
                    attempt.error,
                ],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        Ok(())
    }

    fn complete_attempt(&mut self, attempt: &TaskAttempt) -> Result<(), BoardError> {
        let n = self
            .conn
            .execute(
                "UPDATE team_task_runs
                 SET status = ?3, result = ?4, error = ?5
                 WHERE task_id = ?1 AND attempt = ?2",
                params![
                    attempt.task_id as i64,
                    attempt.attempt as i64,
                    attempt.status.as_str(),
                    attempt.result,
                    attempt.error,
                ],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if n == 0 {
            return Err(BoardError::UnknownTask(attempt.task_id));
        }
        Ok(())
    }

    fn recover_interrupted_attempt(
        &mut self,
        task: u64,
    ) -> Result<Option<TaskAttempt>, BoardError> {
        let interrupted = self
            .attempts(task)?
            .into_iter()
            .rev()
            .find(|attempt| attempt.status == TaskStatus::Running);
        let Some(mut interrupted) = interrupted else {
            return Ok(None);
        };
        let tx = self
            .conn
            .transaction()
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        interrupted.status = TaskStatus::Failed;
        interrupted.error =
            Some("interrupted before terminal driver result; explicit resume required".into());
        let changed = tx.execute(
            "UPDATE team_task_runs SET status = 'failed', result = NULL, error = ?3 WHERE task_id = ?1 AND attempt = ?2 AND status = 'running'",
            params![task as i64, interrupted.attempt as i64, interrupted.error],
        ).map_err(|e| BoardError::Storage(e.to_string()))?;
        if changed == 0 {
            return Ok(None);
        }
        tx.execute(
            "UPDATE team_tasks SET status = 'failed' WHERE id = ?1",
            params![task as i64],
        )
        .map_err(|e| BoardError::Storage(e.to_string()))?;
        tx.execute(
            "UPDATE external_runtime_bindings SET lifecycle_state = 'interrupted' WHERE team_task_id = ?1 AND attempt = ?2 AND lifecycle_state IN ('starting', 'running')",
            params![task as i64, interrupted.attempt as i64],
        ).map_err(|e| BoardError::Storage(e.to_string()))?;
        tx.commit()
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        Ok(Some(interrupted))
    }

    fn commit_successful_result(
        &mut self,
        attempt: &TaskAttempt,
        result: &agentmosaic_team::AgentTaskResult,
    ) -> Result<(), BoardError> {
        if attempt.task_id != result.task_id || attempt.status != TaskStatus::Succeeded {
            return Err(BoardError::Storage(
                "successful result does not match succeeded attempt".into(),
            ));
        }
        let tx = self
            .conn
            .transaction()
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if let Some(message) = &result.message {
            tx.execute(
                "INSERT INTO messages (from_agent, to_agent, body) VALUES (?1, ?2, ?3)",
                params![message.from_agent, message.to_agent, message.body],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        }
        for artifact in &result.artifacts {
            tx.execute(
                "INSERT INTO artifacts (task_id, path, sha256) VALUES (?1, ?2, ?3)",
                params![attempt.task_id as i64, artifact.path, artifact.sha256],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        }
        let changed = tx
            .execute(
                "UPDATE team_task_runs SET status = ?3, result = ?4, error = ?5
                 WHERE task_id = ?1 AND attempt = ?2",
                params![
                    attempt.task_id as i64,
                    attempt.attempt as i64,
                    attempt.status.as_str(),
                    attempt.result,
                    attempt.error,
                ],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if changed == 0 {
            return Err(BoardError::UnknownTask(attempt.task_id));
        }
        let changed = tx
            .execute(
                "UPDATE team_tasks SET status = 'succeeded' WHERE id = ?1",
                params![attempt.task_id as i64],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if changed == 0 {
            return Err(BoardError::UnknownTask(attempt.task_id));
        }
        tx.commit().map_err(|e| BoardError::Storage(e.to_string()))
    }

    fn record_message(&mut self, message: &AgentMessage) -> Result<(), BoardError> {
        self.conn
            .execute(
                "INSERT INTO messages (from_agent, to_agent, body) VALUES (?1, ?2, ?3)",
                params![message.from_agent, message.to_agent, message.body],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        Ok(())
    }

    fn record_artifact(&mut self, task: u64, artifact: &ArtifactMeta) -> Result<(), BoardError> {
        self.conn
            .execute(
                "INSERT INTO artifacts (task_id, path, sha256) VALUES (?1, ?2, ?3)",
                params![task as i64, artifact.path, artifact.sha256],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        Ok(())
    }

    fn record_final_refs(
        &mut self,
        root_task: u64,
        task_refs: &[u64],
        artifact_refs: &[SelectedArtifactRef],
    ) -> Result<(), BoardError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        write_final_refs(&tx, root_task, task_refs, artifact_refs)?;
        tx.commit().map_err(|e| BoardError::Storage(e.to_string()))
    }

    fn claim_root_attempt(
        &mut self,
        root: u64,
        lead_agent: &str,
    ) -> Result<Option<u32>, BoardError> {
        // IMMEDIATE begins the write transaction up front, so two concurrent
        // resumes serialize here: the loser re-reads the status the winner
        // committed and its compare-and-swap changes no row.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let row = tx.query_row(
            "SELECT status, assignee FROM team_tasks WHERE id = ?1",
            params![root as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        );
        let (status, assignee) = match row {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Err(BoardError::UnknownTask(root)),
            Err(e) => return Err(BoardError::Storage(e.to_string())),
        };
        if !matches!(status.as_str(), "failed" | "cancelled") {
            return Ok(None);
        }
        if let Some(assignee) = assignee.as_deref() {
            if assignee != lead_agent {
                return Ok(None);
            }
        }
        let running: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM team_task_runs WHERE task_id = ?1 AND status = 'running'",
                params![root as i64],
                |row| row.get(0),
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if running > 0 {
            return Ok(None);
        }
        let changed = tx
            .execute(
                "UPDATE team_tasks SET status = 'running', assignee = ?2
                 WHERE id = ?1 AND status IN ('failed', 'cancelled')
                   AND (assignee IS NULL OR assignee = ?2)",
                params![root as i64, lead_agent],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if changed == 0 {
            return Ok(None);
        }
        let next: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(attempt), 0) + 1 FROM team_task_runs WHERE task_id = ?1",
                params![root as i64],
                |row| row.get(0),
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        tx.execute(
            "INSERT INTO team_task_runs (task_id, attempt, agent_id, status, result, error)
             VALUES (?1, ?2, ?3, 'running', NULL, NULL)",
            params![root as i64, next, lead_agent],
        )
        .map_err(|e| BoardError::Storage(e.to_string()))?;
        tx.commit()
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        Ok(Some(next as u32))
    }

    fn commit_root_final(
        &mut self,
        attempt: &TaskAttempt,
        task_refs: &[u64],
        artifact_refs: &[SelectedArtifactRef],
    ) -> Result<(), BoardError> {
        if attempt.status != TaskStatus::Succeeded {
            return Err(BoardError::Storage(
                "a root final commit needs a succeeded attempt".into(),
            ));
        }
        let tx = self
            .conn
            .transaction()
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        write_final_refs(&tx, attempt.task_id, task_refs, artifact_refs)?;
        let changed = tx
            .execute(
                "UPDATE team_task_runs SET status = 'succeeded', result = ?3, error = NULL
                 WHERE task_id = ?1 AND attempt = ?2",
                params![
                    attempt.task_id as i64,
                    attempt.attempt as i64,
                    attempt.result
                ],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if changed == 0 {
            return Err(BoardError::UnknownTask(attempt.task_id));
        }
        tx.execute(
            "UPDATE external_runtime_bindings SET lifecycle_state = 'completed'
             WHERE team_task_id = ?1 AND attempt = ?2",
            params![attempt.task_id as i64, attempt.attempt as i64],
        )
        .map_err(|e| BoardError::Storage(e.to_string()))?;
        let changed = tx
            .execute(
                "UPDATE team_tasks SET status = 'succeeded' WHERE id = ?1",
                params![attempt.task_id as i64],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if changed == 0 {
            return Err(BoardError::UnknownTask(attempt.task_id));
        }
        tx.commit().map_err(|e| BoardError::Storage(e.to_string()))
    }

    fn commit_root_failure(&mut self, attempt: &TaskAttempt) -> Result<(), BoardError> {
        if attempt.status != TaskStatus::Failed {
            return Err(BoardError::Storage(
                "a root failure commit needs a failed attempt".into(),
            ));
        }
        let tx = self
            .conn
            .transaction()
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let changed = tx
            .execute(
                "UPDATE team_task_runs SET status = 'failed', result = NULL, error = ?3
                 WHERE task_id = ?1 AND attempt = ?2",
                params![
                    attempt.task_id as i64,
                    attempt.attempt as i64,
                    attempt.error
                ],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if changed == 0 {
            return Err(BoardError::UnknownTask(attempt.task_id));
        }
        tx.execute(
            "UPDATE external_runtime_bindings SET lifecycle_state = 'failed'
             WHERE team_task_id = ?1 AND attempt = ?2",
            params![attempt.task_id as i64, attempt.attempt as i64],
        )
        .map_err(|e| BoardError::Storage(e.to_string()))?;
        let changed = tx
            .execute(
                "UPDATE team_tasks SET status = 'failed' WHERE id = ?1",
                params![attempt.task_id as i64],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if changed == 0 {
            return Err(BoardError::UnknownTask(attempt.task_id));
        }
        tx.commit().map_err(|e| BoardError::Storage(e.to_string()))
    }

    fn reconcile_root_final(&mut self, root: u64, attempt: u32) -> Result<(), BoardError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let changed = tx
            .execute(
                "UPDATE team_tasks SET status = 'succeeded'
                 WHERE id = ?1 AND status <> 'succeeded'",
                params![root as i64],
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        if changed == 0 {
            let exists: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM team_tasks WHERE id = ?1",
                    params![root as i64],
                    |row| row.get(0),
                )
                .map_err(|e| BoardError::Storage(e.to_string()))?;
            if exists == 0 {
                return Err(BoardError::UnknownTask(root));
            }
        }
        tx.execute(
            "UPDATE external_runtime_bindings SET lifecycle_state = 'completed'
             WHERE team_task_id = ?1 AND attempt = ?2 AND lifecycle_state <> 'completed'",
            params![root as i64, attempt as i64],
        )
        .map_err(|e| BoardError::Storage(e.to_string()))?;
        tx.commit().map_err(|e| BoardError::Storage(e.to_string()))
    }

    fn final_refs(
        &self,
        root_task: u64,
    ) -> Result<(Vec<u64>, Vec<SelectedArtifactRef>), BoardError> {
        let mut tasks = self
            .conn
            .prepare(
                "SELECT selected_task_id FROM team_final_task_refs
                 WHERE root_task_id = ?1 ORDER BY selected_task_id",
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let task_refs = tasks
            .query_map(params![root_task as i64], |row| row.get::<_, i64>(0))
            .map_err(|e| BoardError::Storage(e.to_string()))?
            .map(|row| {
                row.map(|id| id as u64)
                    .map_err(|e| BoardError::Storage(e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut artifacts = self
            .conn
            .prepare(
                "SELECT task_id, path, sha256 FROM team_final_artifact_refs
                 WHERE root_task_id = ?1 ORDER BY task_id, path, sha256",
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let artifact_refs = artifacts
            .query_map(params![root_task as i64], |row| {
                Ok(SelectedArtifactRef {
                    task_id: row.get::<_, i64>(0)? as u64,
                    artifact: ArtifactMeta {
                        path: row.get(1)?,
                        sha256: row.get(2)?,
                    },
                })
            })
            .map_err(|e| BoardError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        Ok((task_refs, artifact_refs))
    }

    fn task(&self, id: u64) -> Result<Option<TaskRecord>, BoardError> {
        let row = self.conn.query_row(
            "SELECT id, objective, parent_task, kind, target, assignee, status
             FROM team_tasks WHERE id = ?1",
            params![id as i64],
            row_to_task,
        );
        match row {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(BoardError::Storage(e.to_string())),
        }
    }

    fn attempts(&self, task: u64) -> Result<Vec<TaskAttempt>, BoardError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT task_id, attempt, agent_id, status, result, error
                 FROM team_task_runs WHERE task_id = ?1 ORDER BY attempt",
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![task as i64], row_to_attempt)
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        rows.map(|r| r.map_err(|e| BoardError::Storage(e.to_string())))
            .collect()
    }

    fn messages_to(&self, agent: &str) -> Result<Vec<AgentMessage>, BoardError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT from_agent, to_agent, body FROM messages WHERE to_agent = ?1 ORDER BY id",
            )
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![agent], |r| {
                Ok(AgentMessage {
                    from_agent: r.get(0)?,
                    to_agent: r.get(1)?,
                    body: r.get(2)?,
                })
            })
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        rows.map(|r| r.map_err(|e| BoardError::Storage(e.to_string())))
            .collect()
    }

    fn messages(&self) -> Result<Vec<AgentMessage>, BoardError> {
        let mut stmt = self
            .conn
            .prepare("SELECT from_agent, to_agent, body FROM messages ORDER BY id")
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(AgentMessage {
                    from_agent: r.get(0)?,
                    to_agent: r.get(1)?,
                    body: r.get(2)?,
                })
            })
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        rows.map(|r| r.map_err(|e| BoardError::Storage(e.to_string())))
            .collect()
    }

    fn artifacts(&self, task: u64) -> Result<Vec<ArtifactMeta>, BoardError> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, sha256 FROM artifacts WHERE task_id = ?1 ORDER BY id")
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![task as i64], |r| {
                Ok(ArtifactMeta {
                    path: r.get(0)?,
                    sha256: r.get(1)?,
                })
            })
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        rows.map(|r| r.map_err(|e| BoardError::Storage(e.to_string())))
            .collect()
    }

    fn task_ids(&self) -> Result<Vec<u64>, BoardError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM team_tasks ORDER BY id")
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, i64>(0))
            .map_err(|e| BoardError::Storage(e.to_string()))?;
        rows.map(|r| {
            r.map(|v| v as u64)
                .map_err(|e| BoardError::Storage(e.to_string()))
        })
        .collect()
    }
}

fn row_to_task(r: &Row) -> rusqlite::Result<TaskRecord> {
    let kind = r.get::<_, String>(3)?;
    let status = r.get::<_, String>(6)?;
    Ok(TaskRecord {
        id: r.get::<_, i64>(0)? as u64,
        objective: r.get(1)?,
        parent_task: r.get::<_, Option<i64>>(2)?.map(|p| p as u64),
        kind: TaskKind::restore(&kind).ok_or(rusqlite::Error::InvalidQuery)?,
        target: r.get(4)?,
        assignee: r.get(5)?,
        status: TaskStatus::restore(&status).ok_or(rusqlite::Error::InvalidQuery)?,
    })
}

fn row_to_attempt(r: &Row) -> rusqlite::Result<TaskAttempt> {
    let status = r.get::<_, String>(3)?;
    Ok(TaskAttempt {
        task_id: r.get::<_, i64>(0)? as u64,
        attempt: r.get::<_, i64>(1)? as u32,
        agent_id: r.get(2)?,
        status: TaskStatus::restore(&status).ok_or(rusqlite::Error::InvalidQuery)?,
        result: r.get(4)?,
        error: r.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentmosaic_team::{AgentTaskResult, TaskBoard};

    #[test]
    fn rejected_artifact_rolls_back_the_entire_successful_result_flow() {
        let mut board = SqliteTaskBoard::in_memory().expect("board");
        let task = board
            .create_task("bounded worker task", None, TaskKind::Bulk, None)
            .expect("task");
        board.assign(task, "worker").expect("assign");
        board
            .record_attempt(&TaskAttempt {
                task_id: task,
                attempt: 1,
                agent_id: "worker".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .expect("running attempt");
        board
            .set_status(task, TaskStatus::Running)
            .expect("running task");
        board
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_bad_artifact BEFORE INSERT ON artifacts
                 WHEN NEW.path = 'reject-me'
                 BEGIN SELECT RAISE(ABORT, 'artifact rejected'); END;",
            )
            .expect("test trigger");

        let error = board
            .commit_successful_result(
                &TaskAttempt {
                    task_id: task,
                    attempt: 1,
                    agent_id: "worker".into(),
                    status: TaskStatus::Succeeded,
                    result: Some("finished".into()),
                    error: None,
                },
                &AgentTaskResult {
                    task_id: task,
                    summary: "finished".into(),
                    artifacts: vec![ArtifactMeta {
                        path: "reject-me".into(),
                        sha256: "bad".into(),
                    }],
                    message: Some(AgentMessage {
                        from_agent: "worker".into(),
                        to_agent: "lead".into(),
                        body: "must not persist".into(),
                    }),
                },
            )
            .expect_err("artifact rejection must abort result flow");
        assert!(matches!(error, BoardError::Storage(_)));
        assert_eq!(
            board.task(task).expect("task").expect("exists").status,
            TaskStatus::Running
        );
        assert_eq!(
            board.attempts(task).expect("attempts")[0].status,
            TaskStatus::Running
        );
        assert!(board.messages_to("lead").expect("messages").is_empty());
        assert!(board.artifacts(task).expect("artifacts").is_empty());
    }

    #[test]
    fn a_rejected_root_final_commit_rolls_back_entirely() {
        let mut board = SqliteTaskBoard::in_memory().expect("board");
        let root = board
            .create_task("team objective", None, TaskKind::Reasoning, None)
            .expect("root");
        let child = board
            .create_task("selected worker", Some(root), TaskKind::Bulk, None)
            .expect("child");
        board
            .record_attempt(&TaskAttempt {
                task_id: root,
                attempt: 1,
                agent_id: "lead".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .expect("running attempt");
        board
            .set_status(root, TaskStatus::Running)
            .expect("running root");
        // The fault rejects the commit's last step, so the refs and the attempt
        // written before it must roll back with it.
        board
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_root_success BEFORE UPDATE ON team_tasks
                 WHEN NEW.status = 'succeeded'
                 BEGIN SELECT RAISE(ABORT, 'root success rejected'); END;",
            )
            .expect("test trigger");

        let error = board
            .commit_root_final(
                &TaskAttempt {
                    task_id: root,
                    attempt: 1,
                    agent_id: "lead".into(),
                    status: TaskStatus::Succeeded,
                    result: Some("answer".into()),
                    error: None,
                },
                &[child],
                &[SelectedArtifactRef {
                    task_id: child,
                    artifact: ArtifactMeta {
                        path: "selected.txt".into(),
                        sha256: "a".repeat(64),
                    },
                }],
            )
            .expect_err("the injected rejection must abort the commit");
        assert!(matches!(error, BoardError::Storage(_)));
        let attempts = board.attempts(root).expect("attempts");
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].status, TaskStatus::Running);
        assert!(attempts[0].result.is_none());
        assert_eq!(
            board.task(root).expect("task").expect("exists").status,
            TaskStatus::Running
        );
        let (task_refs, artifact_refs) = board.final_refs(root).expect("refs");
        assert!(task_refs.is_empty());
        assert!(artifact_refs.is_empty());
    }

    #[test]
    fn final_refs_preserve_explicit_selection_not_all_successful_tasks() {
        let mut board = SqliteTaskBoard::in_memory().expect("board");
        let root = board
            .create_task("team objective", None, TaskKind::Reasoning, None)
            .expect("root");
        let selected = board
            .create_task("selected worker", Some(root), TaskKind::Bulk, None)
            .expect("selected");
        let unselected = board
            .create_task(
                "other completed worker",
                Some(root),
                TaskKind::Utility,
                None,
            )
            .expect("unselected");
        let selected_artifact = ArtifactMeta {
            path: "selected.txt".into(),
            sha256: "a".repeat(64),
        };
        board
            .record_artifact(selected, &selected_artifact)
            .expect("selected artifact");
        board
            .record_final_refs(
                root,
                &[selected],
                &[SelectedArtifactRef {
                    task_id: selected,
                    artifact: selected_artifact.clone(),
                }],
            )
            .expect("persist selection");

        let (task_refs, artifact_refs) = board.final_refs(root).expect("read selection");
        assert_eq!(task_refs, vec![selected]);
        assert!(!task_refs.contains(&unselected));
        assert_eq!(artifact_refs.len(), 1);
        assert_eq!(artifact_refs[0].task_id, selected);
        assert_eq!(artifact_refs[0].artifact, selected_artifact);
    }
}
