//! Scheduler-facing bounded Codex app-server driver.
//!
//! The scheduler owns canonical task/attempt state. This driver binds only an
//! external Codex thread/turn to that existing attempt and returns a normal
//! team result for the scheduler to commit.

use std::path::{Component, Path, PathBuf};

use agent_code_storage::{ExternalRuntimeBinding, SqliteTaskBoard};
use agent_code_team::{
    AgentDriver, AgentTask, AgentTaskResult, ArtifactMeta, TaskBoard, TaskStatus,
};
use async_trait::async_trait;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::{CodexAppServer, CodexBridgeEvent};

#[derive(Debug, Clone)]
pub struct CodexTeamDriverConfig {
    pub command: String,
    pub working_directory: PathBuf,
    pub mcp_command: PathBuf,
    pub artifact_paths: Vec<String>,
    pub max_events: usize,
    pub overrides: Vec<String>,
}

impl CodexTeamDriverConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.command.trim().is_empty() {
            return Err("Codex executable is required".into());
        }
        if !self.working_directory.is_dir() {
            return Err("Codex working directory must exist".into());
        }
        if !self.mcp_command.is_file() {
            return Err("RAS MCP executable must exist".into());
        }
        if self.max_events == 0 {
            return Err("Codex max_events must be positive".into());
        }
        for path in &self.artifact_paths {
            validate_relative_path(path)?;
        }
        Ok(())
    }
}

pub struct PersistedCodexTeamDriver {
    config: CodexTeamDriverConfig,
    database: PathBuf,
    agent_id: String,
}

impl PersistedCodexTeamDriver {
    pub fn new(
        config: CodexTeamDriverConfig,
        database: PathBuf,
        agent_id: impl Into<String>,
    ) -> Result<Self, String> {
        config.validate()?;
        if database.as_os_str().is_empty() {
            return Err("SQLite database path is required".into());
        }
        let agent_id = agent_id.into();
        if agent_id.trim().is_empty() {
            return Err("agent id is required".into());
        }
        Ok(Self {
            config,
            database,
            agent_id,
        })
    }

    fn open_board(&self) -> Result<SqliteTaskBoard, String> {
        SqliteTaskBoard::open(Connection::open(&self.database).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    fn current_attempt(&self, task_id: u64) -> Result<u32, String> {
        self.open_board()?
            .attempts(task_id)
            .map_err(|e| format!("{e:?}"))?
            .into_iter()
            .rev()
            .find(|attempt| {
                attempt.agent_id == self.agent_id && attempt.status == TaskStatus::Running
            })
            .map(|attempt| attempt.attempt)
            .ok_or_else(|| "scheduler must persist a running Codex attempt before launch".into())
    }

    fn upsert_binding(
        &self,
        task_id: u64,
        attempt: u32,
        thread: Option<String>,
        turn: Option<String>,
        lifecycle_state: &str,
    ) -> Result<(), String> {
        self.open_board()?
            .upsert_external_binding(&ExternalRuntimeBinding {
                team_task_id: task_id,
                attempt,
                agent_id: self.agent_id.clone(),
                runtime_kind: "codex-app-server".into(),
                native_thread_id: thread,
                native_turn_id: turn,
                lifecycle_state: lifecycle_state.into(),
            })
            .map_err(|e| e.to_string())
    }

    fn collect_artifacts(&self) -> Result<Vec<ArtifactMeta>, String> {
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
        let attempt = self.current_attempt(task.id)?;
        let bridge_log = std::env::temp_dir().join(format!(
            "ras_codex_team_{}_{}_{}.log",
            std::process::id(),
            task.id,
            attempt
        ));
        let _ = std::fs::remove_file(&bridge_log);
        let mut overrides = self.config.overrides.clone();
        overrides.extend([
            format!(
                "mcp_servers.ras.command={:?}",
                self.config.mcp_command.display().to_string()
            ),
            format!(
                "mcp_servers.ras.env.RAS_DB={:?}",
                self.database.display().to_string()
            ),
            format!(
                "mcp_servers.ras.env.RAS_BRIDGE_LOG={:?}",
                bridge_log.display().to_string()
            ),
            format!("mcp_servers.ras.env.RAS_TASK_ID=\"{}\"", task.id),
            format!("mcp_servers.ras.env.RAS_ATTEMPT=\"{attempt}\""),
        ]);
        let mut client = CodexAppServer::spawn_with_overrides(&self.config.command, &overrides)
            .map_err(|e| e.to_string())?;
        let outcome = (|| {
            client
                .initialize("ras-scheduler-codex", "0.1")
                .map_err(|e| e.to_string())?;
            let thread = client
                .start_thread_with_developer_instructions(
                    self.config
                        .working_directory
                        .to_str()
                        .ok_or("non-UTF8 working directory")?,
                    Some("For this bounded team task, use ras_request_context for teammate results. Tool arguments cannot select task or runtime identity."),
                )
                .map_err(|e| e.to_string())?;
            self.upsert_binding(task.id, attempt, Some(thread.clone()), None, "starting")?;
            let turn = client
                .start_turn(
                    &thread,
                    &format!(
                        "Complete this bounded scheduler task: {}\n\nScheduler context:\n{}\n\nBefore relying on teammate results, invoke ras_request_context exactly once. Return a concise completion response after performing only the requested work.",
                        task.objective,
                        task.context.join("\n")
                    ),
                )
                .map_err(|e| e.to_string())?;
            self.upsert_binding(task.id, attempt, Some(thread), Some(turn), "running")?;
            for _ in 0..self.config.max_events {
                match client.next_event().map_err(|e| e.to_string())? {
                    CodexBridgeEvent::TurnCompleted { thread_id, turn_id } => {
                        let summary = client
                            .final_agent_message(&thread_id, &turn_id)
                            .map_err(|e| e.to_string())?;
                        return Ok(AgentTaskResult {
                            task_id: task.id,
                            summary,
                            artifacts: self.collect_artifacts()?,
                            message: None,
                        });
                    }
                    CodexBridgeEvent::McpElicitation {
                        request_id,
                        server_name,
                    } => client
                        .respond_ras_elicitation(request_id, &server_name)
                        .map_err(|e| e.to_string())?,
                    CodexBridgeEvent::Notification(_) | CodexBridgeEvent::ToolCall { .. } => {}
                }
            }
            Err("Codex event limit reached before turn completion".into())
        })();
        let lifecycle = if outcome.is_ok() {
            "completed"
        } else {
            "failed"
        };
        if let Ok(Some(binding)) = self.open_board()?.external_binding(task.id, attempt) {
            self.upsert_binding(
                task.id,
                attempt,
                binding.native_thread_id,
                binding.native_turn_id,
                lifecycle,
            )?;
        }
        let _ = client.close();
        let _ = std::fs::remove_file(bridge_log);
        outcome
    }
}

#[async_trait]
impl AgentDriver for PersistedCodexTeamDriver {
    async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
        self.run_blocking(task)
    }
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("artifact path must be non-empty and relative-contained".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{validate_relative_path, CodexTeamDriverConfig};

    #[test]
    fn artifact_paths_fail_closed_before_runtime_launch() {
        assert!(validate_relative_path("result.txt").is_ok());
        assert!(validate_relative_path("nested/result.txt").is_ok());
        assert!(validate_relative_path("../result.txt").is_err());
        assert!(validate_relative_path("/result.txt").is_err());
        assert!(validate_relative_path("").is_err());
    }

    #[test]
    fn invalid_driver_config_rejects_before_runtime_launch() {
        let config = CodexTeamDriverConfig {
            command: String::new(),
            working_directory: PathBuf::from("."),
            mcp_command: PathBuf::from("missing"),
            artifact_paths: vec!["../escape".into()],
            max_events: 0,
            overrides: Vec::new(),
        };
        assert!(config.validate().is_err());
    }
}
