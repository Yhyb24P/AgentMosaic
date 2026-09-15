//! Scheduler-facing durable driver for Claude Code `stream-json`.

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
    run_claude_cli_invocation, ClaudeCliInvocation, NoopLiveRuntimeEventSink, RuntimeError,
    RuntimeEventDispatcher, SqliteRuntimeEventWriter,
};

#[derive(Debug, Clone)]
pub struct ClaudeCliDriverConfig {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
    pub timeout: Duration,
    pub max_prompt_bytes: usize,
    pub max_result_bytes: usize,
    pub json_schema: Option<String>,
    pub artifact_paths: Vec<String>,
}

impl ClaudeCliDriverConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.command.as_os_str().is_empty() || !self.working_directory.is_dir() {
            return Err("Claude command and working directory are required".into());
        }
        if self.timeout.is_zero() || self.max_prompt_bytes == 0 || self.max_result_bytes == 0 {
            return Err("Claude bounds must be positive".into());
        }
        if let Some(schema) = &self.json_schema {
            if !std::fs::metadata(schema)
                .map_err(|e| e.to_string())?
                .is_file()
            {
                return Err("Claude json_schema must name a regular file".into());
            }
        }
        for path in &self.artifact_paths {
            let path = Path::new(path);
            if path.as_os_str().is_empty()
                || path.is_absolute()
                || path.components().any(|c| {
                    matches!(
                        c,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
            {
                return Err("Claude artifact paths must remain inside the repository".into());
            }
        }
        Ok(())
    }
}

pub struct PersistedClaudeCliDriver {
    config: ClaudeCliDriverConfig,
    database: PathBuf,
    agent_id: String,
}

impl PersistedClaudeCliDriver {
    pub fn new(
        config: ClaudeCliDriverConfig,
        database: PathBuf,
        agent_id: impl Into<String>,
    ) -> Result<Self, String> {
        config.validate()?;
        let agent_id = agent_id.into();
        if database.as_os_str().is_empty() || agent_id.trim().is_empty() {
            return Err("Claude database and agent id are required".into());
        }
        Ok(Self {
            config,
            database,
            agent_id,
        })
    }
    fn board(&self) -> Result<SqliteTaskBoard, String> {
        SqliteTaskBoard::open(Connection::open(&self.database).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }
    fn attempt(&self, task_id: u64) -> Result<u32, String> {
        self.board()?
            .attempts(task_id)
            .map_err(|e| format!("{e:?}"))?
            .into_iter()
            .rev()
            .find(|a| a.agent_id == self.agent_id && a.status == TaskStatus::Running)
            .map(|a| a.attempt)
            .ok_or_else(|| "scheduler must persist a running Claude attempt before launch".into())
    }
    fn bind(&self, task_id: u64, attempt: u32, session: String, state: &str) -> Result<(), String> {
        self.board()?
            .upsert_external_binding(&ExternalRuntimeBinding {
                team_task_id: task_id,
                attempt,
                agent_id: self.agent_id.clone(),
                runtime_kind: "claude-cli".into(),
                native_thread_id: Some(session),
                native_turn_id: None,
                lifecycle_state: state.into(),
            })
            .map_err(|e| e.to_string())
    }
    fn prompt(&self, task: &AgentTask) -> String {
        let mut value = format!(
            "Complete this bounded scheduler task:\n{}\n\nContext:\n",
            task.objective
        );
        for item in &task.context {
            if value.len() >= self.config.max_prompt_bytes {
                break;
            }
            value.push_str(
                &item
                    .chars()
                    .take(self.config.max_prompt_bytes - value.len())
                    .collect::<String>(),
            );
            value.push('\n');
        }
        value.chars().take(self.config.max_prompt_bytes).collect()
    }
    fn artifacts(&self) -> Result<Vec<ArtifactMeta>, String> {
        self.config
            .artifact_paths
            .iter()
            .map(|relative| {
                let path = self.config.working_directory.join(relative);
                if !std::fs::metadata(&path)
                    .map_err(|e| e.to_string())?
                    .is_file()
                {
                    return Err(format!("artifact is not a regular file: {relative}"));
                }
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                Ok(ArtifactMeta {
                    path: relative.clone(),
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                })
            })
            .collect()
    }
    fn run_blocking(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
        let attempt = self.attempt(task.id)?;
        let launch = crate::LaunchSpec::new(self.config.command.clone(), self.config.args.clone())?;
        let existing = self
            .board()?
            .external_binding(task.id, attempt)
            .map_err(|e| e.to_string())?;
        let invocation = match existing.and_then(|b| b.native_thread_id) {
            Some(id) => {
                ClaudeCliInvocation::resume(launch, &id, self.config.json_schema.as_deref())
            }
            None => Ok(ClaudeCliInvocation::start(
                launch,
                self.config.json_schema.as_deref(),
            )),
        }
        .map_err(|e| e.to_string())?;
        let events = RuntimeEventDispatcher::new(
            Arc::new(NoopLiveRuntimeEventSink),
            Arc::new(SqliteRuntimeEventWriter::new(self.database.clone())?),
        );
        let run = run_claude_cli_invocation(
            &invocation,
            &self.config.working_directory,
            &self.prompt(&task),
            self.config.timeout,
            self.config.max_result_bytes,
            |event| {
                if let RuntimeEvent::SessionStarted { native_session_id } = &event {
                    self.bind(task.id, attempt, native_session_id.clone(), "running")
                        .map_err(RuntimeError::Persistence)?;
                }
                events
                    .emit(RuntimeEventRecord {
                        task_id: task.id,
                        attempt,
                        agent_id: self.agent_id.clone(),
                        runtime_name: Some("claude".into()),
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
                self.bind(task.id, attempt, result.session_id, "completed")?;
                Ok(AgentTaskResult {
                    task_id: task.id,
                    summary: result.final_message,
                    artifacts: self.artifacts()?,
                    message: None,
                })
            }
            Err(error) => {
                if let Ok(Some(binding)) = self.board()?.external_binding(task.id, attempt) {
                    if let Some(session) = binding.native_thread_id {
                        let _ = self.bind(task.id, attempt, session, "failed");
                    }
                }
                Err(error.to_string())
            }
        }
    }
}

#[async_trait]
impl AgentDriver for PersistedClaudeCliDriver {
    async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
        self.run_blocking(task)
    }
}
