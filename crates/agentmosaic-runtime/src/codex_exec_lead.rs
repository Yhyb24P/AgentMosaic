//! `codex exec --json` transport for the existing Lead decision contract.

use std::path::PathBuf;
use std::time::Duration;

use agentmosaic_team::{LeadBrain, LeadBrainError, LeadContext, LeadDecision};
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

    fn run_turn(&mut self, prompt: &str) -> Result<String, LeadBrainError> {
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
            |_| Ok(()),
        )
        .map_err(|error| LeadBrainError::Unavailable(error.to_string()))?;
        self.thread_id = Some(result.thread_id);
        Ok(result.final_message)
    }
}

#[async_trait]
impl LeadBrain for CodexExecLeadBrain {
    async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
        let reply = self.run_turn(&self.contract.render_prompt(ctx))?;
        match self.contract.parse_reply(&reply) {
            Ok(decision) => Ok(decision),
            Err(first) => {
                let correction = self.contract.correction_prompt(&first);
                let second = self.run_turn(&correction)?;
                self.contract.parse_reply(&second).map_err(|second_reason| {
                    LeadBrainError::InvalidDecision(format!(
                        "codex exec lead reply rejected: {first}; correction reply rejected: {second_reason}"
                    ))
                })
            }
        }
    }
}
