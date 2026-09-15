//! Scheduler-facing durable driver for `codex exec --json`.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use agentmosaic_storage::{ExternalRuntimeBinding, SqliteTaskBoard};
use agentmosaic_team::{
    AgentDriver, AgentTask, AgentTaskResult, ArtifactMeta, RuntimeEvent, RuntimeEventRecord,
    TaskBoard, TaskStatus,
};
use async_trait::async_trait;
use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    run_codex_exec_invocation, CodexExecInvocation, NoopLiveRuntimeEventSink, RuntimeError,
    RuntimeEventDispatcher, SqliteRuntimeEventWriter,
};

static SCHEMA_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct CodexExecDriverConfig {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
    pub timeout: Duration,
    pub max_prompt_bytes: usize,
    pub max_result_bytes: usize,
    pub output_schema: Option<String>,
    pub artifact_paths: Vec<String>,
    /// Only opt in when the registered auth/model setup does not need config.
    pub isolate: bool,
}

impl CodexExecDriverConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.command.as_os_str().is_empty() || !self.working_directory.is_dir() {
            return Err("Codex exec command and working directory are required".into());
        }
        if self.timeout.is_zero() || self.max_prompt_bytes == 0 || self.max_result_bytes == 0 {
            return Err("Codex exec bounds must be positive".into());
        }
        if self.output_schema.is_some() {
            return Err(
                "Codex exec output_schema is managed by the runtime and must not be configured"
                    .into(),
            );
        }
        for relative in &self.artifact_paths {
            let path = Path::new(relative);
            if path.as_os_str().is_empty()
                || path.is_absolute()
                || path.components().any(|part| {
                    matches!(
                        part,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
            {
                return Err("Codex exec artifact paths must remain inside the repository".into());
            }
        }
        Ok(())
    }
}

/// A private, one-invocation schema. It makes the CLI enforce the same result
/// contract that `structured_summary` validates, while Drop removes it even on
/// protocol failure or timeout.
struct WorkerResultSchema(PathBuf);

impl WorkerResultSchema {
    fn create(directory: &Path, task: u64, attempt: u32, max_bytes: usize) -> Result<Self, String> {
        let nonce = SCHEMA_NONCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".agentmosaic-codex-worker-schema-{}-{}-{}-{}.json",
            std::process::id(),
            task,
            attempt,
            nonce
        ));
        let body = format!(
            r#"{{"type":"object","additionalProperties":false,"required":["summary"],"properties":{{"summary":{{"type":"string","minLength":1,"maxLength":{max_bytes}}}}}}}"#
        );
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("create Codex worker schema: {error}"))?;
        file.write_all(body.as_bytes())
            .map_err(|error| format!("write Codex worker schema: {error}"))?;
        Ok(Self(path))
    }

    fn path(&self) -> Result<&str, String> {
        self.0
            .to_str()
            .ok_or_else(|| "Codex worker schema path is not UTF-8".into())
    }
}

impl Drop for WorkerResultSchema {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub struct PersistedCodexExecDriver {
    config: CodexExecDriverConfig,
    database: PathBuf,
    agent_id: String,
}

impl PersistedCodexExecDriver {
    pub fn new(
        config: CodexExecDriverConfig,
        database: PathBuf,
        agent_id: impl Into<String>,
    ) -> Result<Self, String> {
        config.validate()?;
        let agent_id = agent_id.into();
        if database.as_os_str().is_empty() || agent_id.trim().is_empty() {
            return Err("Codex exec database and agent id are required".into());
        }
        Ok(Self {
            config,
            database,
            agent_id,
        })
    }

    fn board(&self) -> Result<SqliteTaskBoard, String> {
        SqliteTaskBoard::open(Connection::open(&self.database).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())
    }

    fn current_attempt(&self, task_id: u64) -> Result<u32, String> {
        self.board()?
            .attempts(task_id)
            .map_err(|error| format!("{error:?}"))?
            .into_iter()
            .rev()
            .find(|attempt| {
                attempt.agent_id == self.agent_id && attempt.status == TaskStatus::Running
            })
            .map(|attempt| attempt.attempt)
            .ok_or_else(|| {
                "scheduler must persist a running Codex exec attempt before launch".into()
            })
    }

    fn binding(
        &self,
        task_id: u64,
        attempt: u32,
        thread: String,
        state: &str,
    ) -> Result<(), String> {
        self.board()?
            .upsert_external_binding(&ExternalRuntimeBinding {
                team_task_id: task_id,
                attempt,
                agent_id: self.agent_id.clone(),
                runtime_kind: "codex-exec".into(),
                native_thread_id: Some(thread),
                native_turn_id: None,
                lifecycle_state: state.into(),
            })
            .map_err(|error| error.to_string())
    }

    fn prompt(&self, task: &AgentTask) -> String {
        let mut prompt = format!(
            "Complete this bounded scheduler task:\n{}\n\nContext:\n",
            task.objective
        );
        for item in &task.context {
            if prompt.len() >= self.config.max_prompt_bytes {
                break;
            }
            prompt.push_str(
                &item
                    .chars()
                    .take(self.config.max_prompt_bytes - prompt.len())
                    .collect::<String>(),
            );
            prompt.push('\n');
        }
        prompt.chars().take(self.config.max_prompt_bytes).collect()
    }

    fn collect_artifacts(&self) -> Result<Vec<ArtifactMeta>, String> {
        self.config
            .artifact_paths
            .iter()
            .map(|relative| {
                let path = self.config.working_directory.join(relative);
                if !std::fs::metadata(&path)
                    .map_err(|error| error.to_string())?
                    .is_file()
                {
                    return Err(format!("artifact is not a regular file: {relative}"));
                }
                let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
                Ok(ArtifactMeta {
                    path: relative.clone(),
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                })
            })
            .collect()
    }

    /// The worker result is intentionally narrower than a free-form model
    /// reply.  `--output-schema` asks Codex for this shape, while this parser
    /// is the authoritative backstop: a non-conforming peer can never become
    /// a successful scheduler result merely because it emitted valid JSONL.
    fn structured_summary(message: &str) -> Result<String, String> {
        let value: Value = serde_json::from_str(message)
            .map_err(|error| format!("Codex exec final result must be JSON: {error}"))?;
        let Value::Object(fields) = value else {
            return Err("Codex exec final result must be a JSON object".into());
        };
        if fields.len() != 1 || !fields.contains_key("summary") {
            return Err("Codex exec final result must contain exactly `summary`".into());
        }
        let summary = fields
            .get("summary")
            .and_then(Value::as_str)
            .ok_or_else(|| "Codex exec final result `summary` must be a string".to_string())?;
        if summary.trim().is_empty() {
            return Err("Codex exec final result `summary` must not be empty".into());
        }
        Ok(summary.into())
    }

    fn run_blocking(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
        let attempt = self.current_attempt(task.id)?;
        let existing = self
            .board()?
            .external_binding(task.id, attempt)
            .map_err(|error| error.to_string())?;
        let schema = WorkerResultSchema::create(
            &self.config.working_directory,
            task.id,
            attempt,
            self.config.max_result_bytes,
        )?;
        let schema_path = schema.path()?;
        let launch = crate::LaunchSpec::new(self.config.command.clone(), self.config.args.clone())?;
        let invocation = match existing.and_then(|binding| binding.native_thread_id) {
            Some(thread_id) => CodexExecInvocation::resume(
                launch,
                &thread_id,
                Some(schema_path),
                self.config.isolate,
            ),
            None => Ok(CodexExecInvocation::start(
                launch,
                Some(schema_path),
                self.config.isolate,
            )),
        }
        .map_err(|error| error.to_string())?;
        let events = RuntimeEventDispatcher::new(
            Arc::new(NoopLiveRuntimeEventSink),
            Arc::new(SqliteRuntimeEventWriter::new(self.database.clone())?),
        );
        let run = run_codex_exec_invocation(
            &invocation,
            &self.config.working_directory,
            &self.prompt(&task),
            self.config.timeout,
            self.config.max_result_bytes,
            |event| {
                if let RuntimeEvent::SessionStarted { native_session_id } = &event {
                    self.binding(task.id, attempt, native_session_id.clone(), "running")
                        .map_err(RuntimeError::Persistence)?;
                }
                events
                    .emit(RuntimeEventRecord {
                        task_id: task.id,
                        attempt,
                        agent_id: self.agent_id.clone(),
                        runtime_name: Some("codex".into()),
                        native_session_id: match &event {
                            RuntimeEvent::SessionStarted { native_session_id } => {
                                Some(native_session_id.clone())
                            }
                            _ => None,
                        },
                        event,
                    })
                    .map_err(RuntimeError::Persistence)
            },
        );
        match run {
            Ok(result) => {
                self.binding(task.id, attempt, result.thread_id, "completed")?;
                Ok(AgentTaskResult {
                    task_id: task.id,
                    summary: Self::structured_summary(&result.final_message)?,
                    artifacts: self.collect_artifacts()?,
                    message: None,
                })
            }
            Err(error) => {
                if let Ok(Some(binding)) = self.board()?.external_binding(task.id, attempt) {
                    if let Some(thread) = binding.native_thread_id {
                        let _ = self.binding(task.id, attempt, thread, "failed");
                    }
                }
                Err(error.to_string())
            }
        }
    }
}

#[async_trait]
impl AgentDriver for PersistedCodexExecDriver {
    async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
        self.run_blocking(task)
    }
}

#[cfg(test)]
mod tests {
    use super::PersistedCodexExecDriver;

    #[test]
    fn worker_final_result_is_exactly_one_nonempty_summary() {
        assert_eq!(
            PersistedCodexExecDriver::structured_summary(r#"{"summary":"done"}"#).unwrap(),
            "done"
        );
        for invalid in [
            "done",
            "[]",
            r#"{"summary":""}"#,
            r#"{"summary":1}"#,
            r#"{"summary":"done","extra":true}"#,
        ] {
            assert!(
                PersistedCodexExecDriver::structured_summary(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }
}
