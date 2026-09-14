//! Scheduler-facing durable driver for `codex exec --json`.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agentmosaic_storage::{ExternalRuntimeBinding, SqliteTaskBoard};
use agentmosaic_team::{
    AgentDriver, AgentTask, AgentTaskResult, ArtifactMeta, RuntimeEvent, RuntimeEventRecord,
    TaskBoard, TaskStatus,
};
use async_trait::async_trait;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::{
    run_codex_exec_invocation, CodexExecInvocation, NoopLiveRuntimeEventSink, RuntimeError,
    RuntimeEventDispatcher, SqliteRuntimeEventWriter,
};

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
        if let Some(schema) = &self.output_schema {
            if !std::fs::metadata(schema)
                .map_err(|error| error.to_string())?
                .is_file()
            {
                return Err("Codex exec output_schema must name a regular file".into());
            }
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

    fn run_blocking(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
        let attempt = self.current_attempt(task.id)?;
        let existing = self
            .board()?
            .external_binding(task.id, attempt)
            .map_err(|error| error.to_string())?;
        let launch = crate::LaunchSpec::new(self.config.command.clone(), self.config.args.clone())?;
        let invocation = match existing.and_then(|binding| binding.native_thread_id) {
            Some(thread_id) => CodexExecInvocation::resume(
                launch,
                &thread_id,
                self.config.output_schema.as_deref(),
                self.config.isolate,
            ),
            None => Ok(CodexExecInvocation::start(
                launch,
                self.config.output_schema.as_deref(),
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
                    summary: result.final_message,
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
