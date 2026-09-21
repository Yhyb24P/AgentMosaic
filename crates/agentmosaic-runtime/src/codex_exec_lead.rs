//! `codex exec --json` transport for the existing Lead decision contract.

use std::path::PathBuf;
use std::time::Duration;

use agentmosaic_storage::{ExternalRuntimeBinding, SqliteTaskBoard};
use agentmosaic_team::{LeadBrain, LeadBrainError, LeadContext, LeadDecision};
use agentmosaic_team::{TaskBoard, TaskStatus};
use async_trait::async_trait;

use crate::{
    run_codex_exec_invocation, CodexExecInvocation, CodexLeadBrain, CodexLeadConfig, LaunchSpec,
};

#[derive(Debug, Clone)]
pub struct CodexExecLeadConfig {
    pub launch: LaunchSpec,
    pub working_directory: PathBuf,
    pub max_prompt_bytes: usize,
    pub max_answer_bytes: usize,
    pub timeout: Duration,
    pub isolate: bool,
    pub binding_database: Option<PathBuf>,
    pub binding_agent_id: Option<String>,
}

impl CodexExecLeadConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.launch.validate()?;
        if !self.working_directory.is_dir() || self.timeout.is_zero() {
            return Err("Codex exec lead working directory and timeout are required".into());
        }
        if self.max_prompt_bytes < 1024 || self.max_answer_bytes == 0 {
            return Err("Codex exec lead prompt and answer bounds are invalid".into());
        }
        if self.binding_database.is_some() != self.binding_agent_id.is_some() {
            return Err("Codex exec lead binding database and agent id must be paired".into());
        }
        Ok(())
    }
}

/// A stateful Lead backend; rounds resume the foreign Codex thread while the
/// parser stays shared with the app-server Lead implementation.
pub struct CodexExecLeadBrain {
    config: CodexExecLeadConfig,
    contract: CodexLeadBrain,
    thread_id: Option<String>,
}

impl CodexExecLeadBrain {
    pub fn new(
        config: CodexExecLeadConfig,
        candidates: Vec<String>,
    ) -> Result<Self, LeadBrainError> {
        config.validate().map_err(LeadBrainError::Unavailable)?;
        let contract = CodexLeadBrain::new(
            CodexLeadConfig {
                launch: config.launch.clone(),
                working_directory: config.working_directory.clone(),
                model: None,
                overrides: Vec::new(),
                max_prompt_bytes: config.max_prompt_bytes,
                max_answer_bytes: config.max_answer_bytes,
                max_events: 1,
            },
            candidates,
        )?;
        Ok(Self {
            config,
            contract,
            thread_id: None,
        })
    }

    fn persist_binding(&self, root: u64, thread: &str) -> Result<(), LeadBrainError> {
        let (Some(database), Some(agent_id)) =
            (&self.config.binding_database, &self.config.binding_agent_id)
        else {
            return Ok(());
        };
        let board = SqliteTaskBoard::open(
            rusqlite::Connection::open(database)
                .map_err(|error| LeadBrainError::Unavailable(error.to_string()))?,
        )
        .map_err(|error| LeadBrainError::Unavailable(error.to_string()))?;
        let attempt = board
            .attempts(root)
            .map_err(|error| LeadBrainError::Unavailable(format!("{error:?}")))?
            .into_iter()
            .rev()
            .find(|attempt| attempt.agent_id == *agent_id && attempt.status == TaskStatus::Running)
            .ok_or_else(|| {
                LeadBrainError::Unavailable(
                    "root Lead attempt is not running before Codex exec binding".into(),
                )
            })?;
        board
            .upsert_external_binding(&ExternalRuntimeBinding {
                team_task_id: root,
                attempt: attempt.attempt,
                agent_id: agent_id.clone(),
                runtime_kind: "codex-exec".into(),
                native_thread_id: Some(thread.into()),
                native_turn_id: None,
                lifecycle_state: "running".into(),
            })
            .map_err(|error| LeadBrainError::Unavailable(error.to_string()))
    }

    fn restore_binding(&mut self, root: u64) -> Result<(), LeadBrainError> {
        if self.thread_id.is_some() {
            return Ok(());
        }
        let (Some(database), Some(agent_id)) =
            (&self.config.binding_database, &self.config.binding_agent_id)
        else {
            return Ok(());
        };
        let board = SqliteTaskBoard::open(
            rusqlite::Connection::open(database)
                .map_err(|error| LeadBrainError::Unavailable(error.to_string()))?,
        )
        .map_err(|error| LeadBrainError::Unavailable(error.to_string()))?;
        let attempts = board
            .attempts(root)
            .map_err(|error| LeadBrainError::Unavailable(format!("{error:?}")))?;
        let attempt = attempts
            .iter()
            .rev()
            .find(|attempt| attempt.agent_id == *agent_id && attempt.status == TaskStatus::Running)
            .ok_or_else(|| {
                LeadBrainError::Unavailable(
                    "root Lead attempt is not running before Codex exec resume".into(),
                )
            })?;
        if let Some(binding) = board
            .external_binding(root, attempt.attempt)
            .map_err(|error| LeadBrainError::Unavailable(error.to_string()))?
            .filter(|binding| binding.runtime_kind == "codex-exec")
        {
            self.thread_id = binding.native_thread_id;
            return Ok(());
        }
        // Explicit inheritance: this attempt is new, so it has no binding row
        // yet. A resumed Codex Lead continues the native thread its own earlier
        // attempt recorded, and the new `(root, attempt)` row is written when
        // that thread reports back. The earlier row stays exactly as it was —
        // never re-opened — and a thread is never inherited across Leads.
        self.thread_id = attempts
            .iter()
            .rev()
            .filter(|row| row.attempt < attempt.attempt && row.agent_id == *agent_id)
            .filter_map(|row| board.external_binding(root, row.attempt).ok().flatten())
            .find(|binding| binding.runtime_kind == "codex-exec")
            .and_then(|binding| binding.native_thread_id);
        Ok(())
    }

    fn run_turn(&mut self, root: u64, prompt: &str) -> Result<String, LeadBrainError> {
        self.restore_binding(root)?;
        let invocation = match &self.thread_id {
            Some(thread) => CodexExecInvocation::resume(
                self.config.launch.clone(),
                thread,
                None,
                self.config.isolate,
            ),
            None => Ok(CodexExecInvocation::start(
                self.config.launch.clone(),
                None,
                self.config.isolate,
            )),
        }
        .map_err(|error| LeadBrainError::Unavailable(error.to_string()))?;
        let result = run_codex_exec_invocation(
            &invocation,
            &self.config.working_directory,
            prompt,
            self.config.timeout,
            self.config.max_answer_bytes,
            |event| {
                if let agentmosaic_team::RuntimeEvent::SessionStarted { native_session_id } = event
                {
                    self.persist_binding(root, &native_session_id)
                        .map_err(|error| crate::RuntimeError::Persistence(error.to_string()))?;
                }
                Ok(())
            },
        )
        .map_err(|error| LeadBrainError::Unavailable(error.to_string()))?;
        self.thread_id = Some(result.thread_id);
        Ok(result.final_message)
    }
}

#[async_trait]
impl LeadBrain for CodexExecLeadBrain {
    async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
        let prompt = self.contract.render_exec_prompt(ctx)?;
        let reply = self.run_turn(ctx.root_task_id, &prompt)?;
        match self.contract.parse_reply(&reply) {
            Ok(decision) => Ok(decision),
            Err(first) => {
                let correction = self.contract.correction_prompt(&first);
                let second = self.run_turn(ctx.root_task_id, &correction)?;
                self.contract.parse_reply(&second).map_err(|second_reason| {
                    LeadBrainError::InvalidDecision(format!(
                        "codex exec lead reply rejected: {first}; correction reply rejected: {second_reason}"
                    ))
                })
            }
        }
    }
}
