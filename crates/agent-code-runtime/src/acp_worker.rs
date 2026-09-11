//! Bounded shared ACP worker driver for local coding-agent CLIs.

use std::path::PathBuf;
use std::time::Duration;

use agent_client_protocol::schema::v1::{AuthMethodId, AuthenticateRequest};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Client};
use agent_code_team::{AgentDriver, AgentTask, AgentTaskResult, ArtifactMeta};
use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpWorkerConfig {
    pub runtime_kind: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// Auth method id advertised by the agent's `initialize`; sent via
    /// `authenticate` before `session/new`. `None` skips authentication.
    pub auth_method: Option<String>,
    pub working_directory: PathBuf,
    pub timeout: Duration,
    pub max_prompt_bytes: usize,
    pub max_result_bytes: usize,
    /// Relative output paths returned with the worker result before the board
    /// may expose a successful task.
    pub artifact_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpWorkerError {
    InvalidConfig(String),
    Protocol(String),
    TimedOut,
    InvalidPeerResult(String),
}

impl std::fmt::Display for AcpWorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(s) => write!(f, "invalid ACP worker configuration: {s}"),
            Self::Protocol(s) => write!(f, "ACP protocol error: {s}"),
            Self::TimedOut => write!(f, "ACP worker timed out"),
            Self::InvalidPeerResult(s) => write!(f, "invalid ACP peer result: {s}"),
        }
    }
}
impl std::error::Error for AcpWorkerError {}

#[derive(Debug, Clone)]
pub struct AcpWorkerDriver {
    config: AcpWorkerConfig,
}

/// A bounded, same-session ACP exchange. The native session id is an external
/// recovery reference; callers must persist it only through the team board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpConversationResult {
    pub external_session_id: String,
    pub first_summary: String,
    pub follow_up_summary: String,
}

/// A completed bounded task plus its foreign session reference. The reference
/// is deliberately not a task id and must be persisted only by the caller's
/// existing external-runtime binding path.
#[derive(Debug, Clone)]
pub struct AcpTaskExecution {
    pub external_session_id: String,
    pub result: AgentTaskResult,
}

impl AcpWorkerDriver {
    pub fn new(config: AcpWorkerConfig) -> Result<Self, AcpWorkerError> {
        if config.runtime_kind.trim().is_empty()
            || config.command.as_os_str().is_empty()
            || !config.working_directory.is_dir()
            || config.timeout.is_zero()
            || config.max_prompt_bytes == 0
            || config.max_result_bytes == 0
            || config.artifact_paths.iter().any(|path| {
                path.as_os_str().is_empty()
                    || path.is_absolute()
                    || path
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
            })
        {
            return Err(AcpWorkerError::InvalidConfig("runtime kind, command, existing working directory, bounded relative artifact paths, timeout, and limits are required".into()));
        }
        Ok(Self { config })
    }
    pub async fn run(&self, task: &AgentTask) -> Result<(String, String), AcpWorkerError> {
        let prompt = bounded_prompt(task, self.config.max_prompt_bytes);
        let agent =
            AcpAgent::new(AcpAgentConfig::new(&self.config.command).args(self.config.args.clone()));
        let cwd = self.config.working_directory.clone();
        let run = Client
            .builder()
            .name("agent-code-r6")
            .connect_with(agent, async move |cx| {
                if let Some(method) = &self.config.auth_method {
                    cx.send_request(AuthenticateRequest::new(AuthMethodId::new(method.as_str())))
                        .block_task()
                        .await?;
                }
                cx.build_session(&cwd)
                    .block_task()
                    .run_until(async |mut session| {
                        let session_id = session.session_id().to_string();
                        session.send_prompt(prompt)?;
                        Ok((session_id, session.read_to_string().await?))
                    })
                    .await
            });
        tokio::time::timeout(self.config.timeout, run)
            .await
            .map_err(|_| AcpWorkerError::TimedOut)?
            .map_err(|e| AcpWorkerError::Protocol(e.to_string()))
    }

    /// Sends the follow-up through the same ACP session, rather than opening
    /// another session or replaying a full task context.
    pub async fn run_with_follow_up(
        &self,
        task: &AgentTask,
        follow_up: &str,
    ) -> Result<AcpConversationResult, AcpWorkerError> {
        let first_prompt = bounded_prompt(task, self.config.max_prompt_bytes);
        let follow_up_prompt = bounded_follow_up(follow_up, self.config.max_prompt_bytes);
        let agent =
            AcpAgent::new(AcpAgentConfig::new(&self.config.command).args(self.config.args.clone()));
        let cwd = self.config.working_directory.clone();
        let run = Client
            .builder()
            .name("agent-code-r6")
            .connect_with(agent, async move |cx| {
                if let Some(method) = &self.config.auth_method {
                    cx.send_request(AuthenticateRequest::new(AuthMethodId::new(method.as_str())))
                        .block_task()
                        .await?;
                }
                cx.build_session(&cwd)
                    .block_task()
                    .run_until(async |mut session| {
                        let session_id = session.session_id().to_string();
                        session.send_prompt(first_prompt)?;
                        let first = session.read_to_string().await?;
                        session.send_prompt(follow_up_prompt)?;
                        let second = session.read_to_string().await?;
                        Ok((session_id, first, second))
                    })
                    .await
            });
        let (external_session_id, first, second) = tokio::time::timeout(self.config.timeout, run)
            .await
            .map_err(|_| AcpWorkerError::TimedOut)?
            .map_err(|e| AcpWorkerError::Protocol(e.to_string()))?;
        Ok(AcpConversationResult {
            external_session_id,
            first_summary: parse_peer_result(&first, self.config.max_result_bytes)?,
            follow_up_summary: parse_peer_result(&second, self.config.max_result_bytes)?,
        })
    }

    pub async fn execute_task(&self, task: &AgentTask) -> Result<AcpTaskExecution, AcpWorkerError> {
        let (external_session_id, response) = self.run(task).await?;
        let summary = parse_peer_result(&response, self.config.max_result_bytes)?;
        let artifacts = self.collect_artifacts()?;
        Ok(AcpTaskExecution {
            external_session_id,
            result: AgentTaskResult {
                task_id: task.id,
                summary,
                artifacts,
                message: None,
            },
        })
    }

    fn collect_artifacts(&self) -> Result<Vec<ArtifactMeta>, AcpWorkerError> {
        self.config
            .artifact_paths
            .iter()
            .map(|relative| {
                let bytes =
                    std::fs::read(self.config.working_directory.join(relative)).map_err(|_| {
                        AcpWorkerError::InvalidPeerResult(format!(
                            "expected bounded artifact is missing: {}",
                            relative.display()
                        ))
                    })?;
                Ok(ArtifactMeta {
                    path: relative.to_string_lossy().into_owned(),
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                })
            })
            .collect()
    }
}

#[async_trait]
impl AgentDriver for AcpWorkerDriver {
    async fn run_task(&self, task: AgentTask) -> Result<AgentTaskResult, String> {
        Ok(self
            .execute_task(&task)
            .await
            .map_err(|error| error.to_string())?
            .result)
    }
}

fn bounded_prompt(task: &AgentTask, max: usize) -> String {
    let mut text = format!(
        "Task {}:\n{}\n\nBounded ACC context:\n",
        task.id, task.objective
    );
    for item in &task.context {
        if text.len() >= max {
            break;
        }
        text.push_str(
            &item
                .chars()
                .take(max.saturating_sub(text.len()))
                .collect::<String>(),
        );
        text.push('\n');
    }
    text.push_str("\nReturn exactly one JSON object with only a string field named summary. Do not include prose, markdown, credentials, hidden reasoning, or other fields.\n");
    text.chars().take(max).collect()
}

fn bounded_follow_up(follow_up: &str, max: usize) -> String {
    format!(
        "Follow-up instruction (same bounded session):\n{}\n\nReturn exactly one JSON object with only a string field named summary. Do not include prose, markdown, credentials, hidden reasoning, or other fields.\n",
        follow_up.chars().take(max / 2).collect::<String>()
    )
    .chars()
    .take(max)
    .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerResult {
    summary: String,
}

fn parse_peer_result(response: &str, max_bytes: usize) -> Result<String, AcpWorkerError> {
    if response.len() > max_bytes {
        return Err(AcpWorkerError::InvalidPeerResult(
            "response exceeds limit".into(),
        ));
    }
    let peer: PeerResult = serde_json::from_str(response.trim())
        .map_err(|_| AcpWorkerError::InvalidPeerResult("expected one strict JSON object".into()))?;
    if peer.summary.trim().is_empty() || peer.summary.len() > max_bytes {
        return Err(AcpWorkerError::InvalidPeerResult(
            "invalid summary length".into(),
        ));
    }
    Ok(peer.summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_code_team::TaskKind;
    #[test]
    fn prompt_is_bounded() {
        let task = AgentTask {
            id: 1,
            objective: "x".repeat(100),
            kind: TaskKind::Bulk,
            context: vec!["y".repeat(100)],
        };
        assert!(bounded_prompt(&task, 32).len() <= 32);
    }

    #[test]
    fn peer_result_is_strict_and_bounded() {
        assert_eq!(
            parse_peer_result(r#"{"summary":"peer finding"}"#, 64).unwrap(),
            "peer finding"
        );
        assert!(parse_peer_result(r#"{"summary":"x","extra":true}"#, 64).is_err());
        assert!(parse_peer_result("not-json", 64).is_err());
    }

    #[test]
    fn follow_up_prompt_is_bounded_and_requires_structured_result() {
        let prompt = bounded_follow_up(&"x".repeat(100), 48);
        assert!(prompt.len() <= 48);
        assert!(prompt.contains("Follow-up"));
    }

    #[test]
    fn artifact_collection_is_relative_hashed_and_fail_closed() {
        let cwd = std::env::temp_dir().join(format!(
            "agent_code_acp_artifacts_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(cwd.join("result.txt"), "exact bytes\n").unwrap();
        let config = AcpWorkerConfig {
            runtime_kind: "test".into(),
            command: PathBuf::from("test-agent"),
            args: Vec::new(),
            auth_method: None,
            working_directory: cwd.clone(),
            timeout: Duration::from_secs(1),
            max_prompt_bytes: 64,
            max_result_bytes: 64,
            artifact_paths: vec![PathBuf::from("result.txt")],
        };
        let driver = AcpWorkerDriver::new(config).unwrap();
        let artifacts = driver.collect_artifacts().unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].path, "result.txt");
        assert_eq!(
            artifacts[0].sha256,
            "6a77ce4ad94636f6120bb985066c1d75ce65b73f264a35f9d5ac910e252f0355"
        );
        let missing = AcpWorkerDriver::new(AcpWorkerConfig {
            artifact_paths: vec![PathBuf::from("missing.txt")],
            ..driver.config.clone()
        })
        .unwrap();
        assert!(missing.collect_artifacts().is_err());
        assert!(AcpWorkerDriver::new(AcpWorkerConfig {
            artifact_paths: vec![PathBuf::from("../escape")],
            ..driver.config.clone()
        })
        .is_err());
        let _ = std::fs::remove_dir_all(cwd);
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code; asserts only the current no-credential path"]
    async fn qwen_acp_reports_auth_required_without_running_a_task() {
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "qwen-code".into(),
            command: PathBuf::from("qwen"),
            args: vec!["--acp".into(), "--bare".into()],
            auth_method: None,
            working_directory: std::env::temp_dir(),
            timeout: Duration::from_secs(15),
            max_prompt_bytes: 1024,
            max_result_bytes: 4096,
            artifact_paths: Vec::new(),
        })
        .expect("valid local Qwen profile");
        let error = driver
            .run(&AgentTask {
                id: 1,
                objective: "no task should be sent without authentication".into(),
                kind: TaskKind::Bulk,
                context: Vec::new(),
            })
            .await
            .expect_err("current Qwen environment requires authentication");
        assert!(error.to_string().contains("Authentication required"));
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn qwen_acp_authenticated_session_completes_a_bounded_task() {
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "qwen-code".into(),
            command: PathBuf::from("qwen"),
            args: vec!["--acp".into()],
            auth_method: Some("openai".into()),
            working_directory: std::env::temp_dir(),
            timeout: Duration::from_secs(180),
            max_prompt_bytes: 1024,
            max_result_bytes: 4096,
            artifact_paths: Vec::new(),
        })
        .expect("valid local Qwen profile");
        let result = driver
            .run_task(AgentTask {
                id: 1,
                objective: "Return the bounded peer finding pong.".into(),
                kind: TaskKind::Bulk,
                context: Vec::new(),
            })
            .await
            .expect("authenticated ACP task completes");
        assert!(!result.summary.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn qwen_acp_reuses_one_authenticated_session_for_follow_up() {
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "qwen-code".into(),
            command: PathBuf::from("qwen"),
            args: vec!["--acp".into()],
            auth_method: Some("openai".into()),
            working_directory: std::env::temp_dir(),
            timeout: Duration::from_secs(180),
            max_prompt_bytes: 1024,
            max_result_bytes: 4096,
            artifact_paths: Vec::new(),
        })
        .expect("valid local Qwen profile");
        let result =
            driver
                .run_with_follow_up(
                    &AgentTask {
                        id: 2,
                        objective:
                            "Return exactly this JSON peer result: {\"summary\":\"first-pass\"}."
                                .into(),
                        kind: TaskKind::Bulk,
                        context: Vec::new(),
                    },
                    "Return exactly this JSON peer result: {\"summary\":\"follow-up-pass\"}.",
                )
                .await
                .expect("same authenticated ACP session completes follow-up");
        assert!(!result.external_session_id.is_empty());
        assert!(!result.first_summary.is_empty());
        assert!(!result.follow_up_summary.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires local Qwen Code with an authenticated openai provider and a live local vLLM endpoint"]
    async fn qwen_acp_completes_a_bounded_isolated_coding_task() {
        let cwd = std::env::temp_dir().join(format!(
            "agent_code_qwen_coding_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&cwd).unwrap();
        let git_status = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&cwd)
            .status()
            .expect("git available for isolated fixture");
        assert!(git_status.success(), "initialize isolated git repository");
        std::fs::write(cwd.join("status.txt"), "status=broken\n").unwrap();
        std::fs::write(
            cwd.join("check.sh"),
            "#!/bin/sh\ntest \"$(cat status.txt)\" = \"status=fixed\"\n",
        )
        .unwrap();
        let driver = AcpWorkerDriver::new(AcpWorkerConfig {
            runtime_kind: "qwen-code".into(),
            command: PathBuf::from("qwen"),
            args: vec!["--acp".into()],
            auth_method: Some("openai".into()),
            working_directory: cwd.clone(),
            // Contention-calibrated budget; see the live test note.
            timeout: Duration::from_secs(600),
            max_prompt_bytes: 2048,
            max_result_bytes: 4096,
            artifact_paths: vec![PathBuf::from("status.txt")],
        })
        .expect("valid local Qwen profile");
        let result = driver
            .run_task(AgentTask {
                id: 3,
                objective: "In this isolated Git repository, inspect status.txt, replace its exact contents with status=fixed followed by one newline, run `sh check.sh`, and then return exactly this JSON peer result: {\"summary\":\"status fixed and check passed\"}. Do not modify any other file.".into(),
                kind: TaskKind::Bulk,
                context: Vec::new(),
            })
            .await
            .expect("Qwen completes bounded coding task");
        assert_eq!(
            std::fs::read(cwd.join("status.txt")).unwrap(),
            b"status=fixed\n"
        );
        assert!(!result.summary.is_empty());
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].path, "status.txt");
        let _ = std::fs::remove_dir_all(cwd);
    }
}
