//! Compatibility/experimental resident Codex app-server Lead brain.
//!
//! One Lead objective is one Codex thread. The brain spawns the local
//! `codex app-server` once, starts a single `read-only` / `never` thread whose
//! developer instructions state the strict decision contract, and then drives
//! one bounded turn per Lead round — planning, follow-up, and final synthesis
//! all happen in that one conversation. The thread holds no task state: every
//! round's turn input is a freshly rendered, bounded view of the durable board
//! ([`LeadContext`]).
//!
//! The model's reply is not trusted prose. After trimming ASCII whitespace only
//! it must be exactly one JSON object matching the decision contract; anything
//! else is rejected and re-asked exactly once, then reported as
//! [`LeadBrainError::InvalidDecision`]. A decision naming an agent the
//! scheduler cannot route to, an ungrounded completion, or a malformed artifact
//! digest never reaches the Lead loop.

use std::path::PathBuf;

use agentmosaic_team::{
    ArtifactMeta, LeadBrain, LeadBrainError, LeadContext, LeadDecision, SelectedArtifactRef,
    TaskKind, TaskSpec, TeamResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{CodexAppServer, CodexBridgeError, CodexBridgeEvent, LaunchSpec};

/// The Lead only reasons: its thread may not write the workspace and may never
/// raise an approval prompt.
const LEAD_SANDBOX: &str = "read-only";
const LEAD_APPROVAL_POLICY: &str = "never";

/// Reply bounds of the decision contract.
const MAX_DELEGATED_TASKS: usize = 32;
const MAX_SELECTED_IDS: usize = 256;
const MAX_SELECTED_ARTIFACTS: usize = 256;
const SHA256_HEX_LEN: usize = 64;

/// The smallest prompt budget that still carries the whole reply instruction.
const MIN_PROMPT_BYTES: usize = 1024;

const PROMPT_PREFIX: &str = "Current lead context (compact JSON):\n";

const PROMPT_SUFFIX: &str = "Reply with exactly one JSON object and nothing else: no prose, no markdown fences. Use action delegate, follow_up, or complete, exactly as your developer instructions specify.";

/// The thread-level contract. It is stated once, when the resident thread
/// starts, so each round's turn only has to carry the bounded context.
const DEVELOPER_INSTRUCTIONS: &str = concat!(
    "You are the Lead of a heterogeneous coding agent team. You plan, delegate, ",
    "and synthesize. You never run shell commands, never edit files, and never ",
    "read repository content yourself: your workers do the work, and their ",
    "results and artifacts reach you as context.\n\n",
    "Reply to every turn with exactly one JSON object and nothing else: no prose ",
    "before or after it, no markdown fences, no comments. Choose exactly one of:\n",
    "1. Delegate work, 1 to 32 tasks (the first round):\n",
    "{\"action\":\"delegate\",\"tasks\":[{\"kind\":\"bulk\",\"target\":\"<agent id>\",\"objective\":\"...\"}]}\n",
    "2. Follow up with exactly one further task, grounded in the results you were given:\n",
    "{\"action\":\"follow_up\",\"task\":{\"kind\":\"reasoning\",\"target\":null,\"objective\":\"...\"}}\n",
    "3. Complete the objective:\n",
    "{\"action\":\"complete\",\"answer\":\"...\",\"selected_task_ids\":[2],\"selected_artifacts\":[{\"task_id\":2,\"path\":\"result.txt\",\"sha256\":\"<64 lowercase hex>\"}]}\n\n",
    "Rules:\n",
    "- Every task object has exactly the three keys kind, target, objective. ",
    "kind is one of bulk, tool, review, reasoning, utility.\n",
    "- target is either null (the scheduler chooses) or exactly one candidate ",
    "agent id from the context. Never invent an agent id.\n",
    "- objective is a non-empty string stating the work to do.\n",
    "- complete needs a non-empty answer and at least one selected_task_ids ",
    "entry; each id must be a task that succeeded in the context you were given. ",
    "Each selected_artifacts entry must name one of those ids and an exact path ",
    "and sha256 from the context. Never invent results, paths, or digests.\n",
    "- Unknown fields, missing fields, extra text, empty answers, and any other ",
    "shape are rejected.",
);

/// Configuration for the resident Codex Lead brain.
#[derive(Debug, Clone)]
pub struct CodexLeadConfig {
    /// The configured runtime launch; the adapter appends `app-server --stdio`.
    pub launch: LaunchSpec,
    /// The directory the Lead thread runs in.
    pub working_directory: PathBuf,
    /// An optional model override (`-c model="..."`).
    pub model: Option<String>,
    /// Extra `codex -c` overrides.
    pub overrides: Vec<String>,
    /// Byte bound for one round's rendered context.
    pub max_prompt_bytes: usize,
    /// Byte bound for a final answer.
    pub max_answer_bytes: usize,
    /// Event bound for one turn's event pump.
    pub max_events: usize,
}

impl CodexLeadConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.launch.validate()?;
        if !self.working_directory.is_dir() {
            return Err("Codex lead working directory must exist".into());
        }
        if let Some(model) = &self.model {
            if model.trim().is_empty() {
                return Err("Codex lead model must be non-empty when set".into());
            }
        }
        if self.max_prompt_bytes < MIN_PROMPT_BYTES {
            return Err(format!(
                "Codex lead max_prompt_bytes must be at least {MIN_PROMPT_BYTES}"
            ));
        }
        if self.max_answer_bytes == 0 {
            return Err("Codex lead max_answer_bytes must be positive".into());
        }
        if self.max_events == 0 {
            return Err("Codex lead max_events must be positive".into());
        }
        Ok(())
    }
}

/// A Codex-backed Lead brain with one resident thread.
pub struct CodexLeadBrain {
    config: CodexLeadConfig,
    /// The agent ids the scheduler can route to; a decision naming any other
    /// target is rejected.
    candidates: Vec<String>,
    server: Option<CodexAppServer>,
    thread_id: Option<String>,
}

impl CodexLeadBrain {
    /// Build a brain for `candidates`. The app server is spawned lazily on the
    /// first `decide`, so constructing a brain starts no process.
    pub fn new(config: CodexLeadConfig, candidates: Vec<String>) -> Result<Self, LeadBrainError> {
        config.validate().map_err(LeadBrainError::Unavailable)?;
        Ok(Self {
            config,
            candidates,
            server: None,
            thread_id: None,
        })
    }

    /// Spawn the app server and the resident thread on the first call, and
    /// return the thread id every later round reuses.
    fn ensure_thread(&mut self) -> Result<String, LeadBrainError> {
        if let Some(thread_id) = &self.thread_id {
            return Ok(thread_id.clone());
        }
        let mut overrides = self.config.overrides.clone();
        if let Some(model) = &self.config.model {
            overrides.push(format!("model={model:?}"));
        }
        let working_directory = self
            .config
            .working_directory
            .to_str()
            .ok_or_else(|| {
                LeadBrainError::Unavailable("Codex lead working directory is not UTF-8".into())
            })?
            .to_string();
        let mut server = CodexAppServer::spawn_launch(self.config.launch.clone(), &overrides)
            .map_err(|e| unavailable("spawn the codex app-server", e))?;
        let started = server
            .initialize("agentmosaic-codex-lead", "0.1")
            .and_then(|_| {
                server.start_thread_with_options(
                    &working_directory,
                    Some(DEVELOPER_INSTRUCTIONS),
                    LEAD_SANDBOX,
                    LEAD_APPROVAL_POLICY,
                )
            });
        let thread_id = match started {
            Ok(thread_id) => thread_id,
            Err(error) => {
                let _ = server.close();
                return Err(unavailable("start the resident codex lead thread", error));
            }
        };
        self.server = Some(server);
        self.thread_id = Some(thread_id.clone());
        Ok(thread_id)
    }

    /// Start one turn on the resident thread and return its bounded visible
    /// reply.
    fn run_turn(&mut self, prompt: &str) -> Result<String, LeadBrainError> {
        let thread_id = self.ensure_thread()?;
        let max_events = self.config.max_events;
        let server = self.server.as_mut().ok_or_else(|| {
            LeadBrainError::Unavailable("the codex lead app-server is not running".into())
        })?;
        let turn_id = server
            .start_turn(&thread_id, prompt)
            .map_err(|e| unavailable("start a codex lead turn", e))?;
        pump_turn(server, &thread_id, &turn_id, max_events)
    }

    /// One Lead round: render the bounded context, run the turn, and parse the
    /// reply strictly. A rejected reply earns exactly one correction turn on
    /// the same thread; a second rejection is final.
    fn decide_turn(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
        let prompt = self.render_prompt(ctx);
        let reply = self.run_turn(&prompt)?;
        match self.parse_reply(&reply) {
            Ok(decision) => Ok(decision),
            Err(reason) => {
                let correction = self.correction_prompt(&reason);
                let second = self.run_turn(&correction)?;
                self.parse_reply(&second).map_err(|second_reason| {
                    LeadBrainError::InvalidDecision(format!(
                        "codex lead reply rejected: {reason}; correction reply rejected: {second_reason}"
                    ))
                })
            }
        }
    }

    /// The single correction turn's input: why the reply was rejected, then the
    /// contract again. The reason is bounded so this prompt also stays within
    /// `max_prompt_bytes`.
    pub(crate) fn correction_prompt(&self, reason: &str) -> String {
        let head_budget = self
            .config
            .max_prompt_bytes
            .saturating_sub(PROMPT_SUFFIX.len() + 2);
        let head = bound_utf8(
            &format!("Your previous reply was rejected: {reason}"),
            head_budget,
        );
        format!("{head}\n{PROMPT_SUFFIX}")
    }

    /// Render one bounded turn input: the compact board context and the reply
    /// instruction. Only durable board facts are rendered (task ids, bounded
    /// result summaries, artifact digests, bounded errors, messages) — never
    /// hidden model reasoning.
    pub(crate) fn render_prompt(&self, ctx: &LeadContext) -> String {
        let budget = self
            .config
            .max_prompt_bytes
            .saturating_sub(PROMPT_PREFIX.len() + PROMPT_SUFFIX.len() + 2);
        let context = self.render_context(ctx, budget);
        format!("{PROMPT_PREFIX}{context}\n{PROMPT_SUFFIX}")
    }

    /// `codex app-server` receives this contract once as developer
    /// instructions, but stateless `codex exec` has no equivalent thread
    /// configuration. Include it in every Exec Lead turn so the rendered
    /// context never refers to instructions that were not actually sent.
    pub(crate) fn render_exec_prompt(&self, ctx: &LeadContext) -> String {
        format!("{DEVELOPER_INSTRUCTIONS}\n\n{}", self.render_prompt(ctx))
    }

    fn render_context(&self, ctx: &LeadContext, budget: usize) -> String {
        let entries =
            ctx.results.len() + ctx.artifacts.len() + ctx.failures.len() + ctx.messages.len() + 1;
        let per_text = (budget / entries.max(1)).clamp(64, 4096);
        let results: Vec<Value> = ctx
            .results
            .iter()
            .map(|(task_id, result)| {
                json!({
                    "task_id": task_id,
                    "summary": bound_utf8(&result.summary, per_text),
                })
            })
            .collect();
        let artifacts: Vec<Value> = ctx
            .artifacts
            .iter()
            .map(|artifact| {
                json!({
                    "path": bound_utf8(&artifact.path, per_text),
                    "sha256": artifact.sha256,
                })
            })
            .collect();
        let failures: Vec<Value> = ctx
            .failures
            .iter()
            .map(|(task_id, error)| {
                json!({
                    "task_id": task_id,
                    "error": bound_utf8(error, per_text),
                })
            })
            .collect();
        let messages: Vec<Value> = ctx
            .messages
            .iter()
            .map(|message| {
                json!({
                    "from": message.from_agent,
                    "to": message.to_agent,
                    "body": bound_utf8(&message.body, per_text),
                })
            })
            .collect();
        let payload = json!({
            "root_task_id": ctx.root_task_id,
            "objective": bound_utf8(&ctx.objective, per_text),
            "round": ctx.round,
            "candidates": ctx.candidates,
            "results": results,
            "artifacts": artifacts,
            "failures": failures,
            "messages": messages,
        });
        bound_utf8(&payload.to_string(), budget)
    }

    /// Parse the reply strictly: after trimming ASCII whitespace only it must
    /// be exactly one JSON object in the contract, and every field must survive
    /// the contract's validation. No fence stripping, no prose scanning, no
    /// substring extraction.
    pub(crate) fn parse_reply(&self, reply: &str) -> Result<LeadDecision, String> {
        let trimmed = reply.trim_matches(|c: char| c.is_ascii_whitespace());
        if trimmed.is_empty() {
            return Err("the reply was empty".into());
        }
        let wire: DecisionWire = serde_json::from_str(trimmed)
            .map_err(|e| format!("the reply is not exactly one JSON decision object: {e}"))?;
        match wire {
            DecisionWire::Delegate { tasks } => {
                if tasks.is_empty() || tasks.len() > MAX_DELEGATED_TASKS {
                    return Err(format!(
                        "action delegate needs 1 to {MAX_DELEGATED_TASKS} tasks, got {}",
                        tasks.len()
                    ));
                }
                let specs = tasks
                    .iter()
                    .map(|task| self.task_spec(task))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(LeadDecision::Delegate(specs))
            }
            DecisionWire::FollowUp { task } => {
                Ok(LeadDecision::FollowUp(vec![self.task_spec(&task)?]))
            }
            DecisionWire::Complete {
                answer,
                selected_task_ids,
                selected_artifacts,
            } => self
                .team_result(answer, selected_task_ids, selected_artifacts)
                .map(LeadDecision::Complete),
        }
    }

    fn task_spec(&self, task: &TaskWire) -> Result<TaskSpec, String> {
        let objective = task.objective.trim();
        if objective.is_empty() {
            return Err("every task objective must be non-empty".into());
        }
        let target = match &task.target {
            None => None,
            Some(target) => {
                if !self.candidates.iter().any(|candidate| candidate == target) {
                    return Err(format!(
                        "target `{target}` is not one of the routable candidates"
                    ));
                }
                Some(target.clone())
            }
        };
        Ok(TaskSpec {
            objective: objective.to_string(),
            kind: task.kind.to_task_kind(),
            target,
            // The Lead loop forces the root as parent; the model never names it.
            parent: None,
            context: Vec::new(),
        })
    }

    fn team_result(
        &self,
        answer: String,
        selected_task_ids: Vec<u64>,
        selected_artifacts: Vec<ArtifactWire>,
    ) -> Result<TeamResult, String> {
        let answer = answer.trim();
        if answer.is_empty() {
            return Err("a complete answer must be non-empty".into());
        }
        if answer.len() > self.config.max_answer_bytes {
            return Err(format!(
                "the complete answer is {} bytes, over the {}-byte bound",
                answer.len(),
                self.config.max_answer_bytes
            ));
        }
        if selected_task_ids.is_empty() {
            // A final result that references no completed task cannot be
            // grounded; `Lead::verify_completion` refuses it too, so refuse it
            // here while the model can still correct itself.
            return Err("complete requires at least one selected_task_ids entry".into());
        }
        if selected_task_ids.len() > MAX_SELECTED_IDS {
            return Err(format!(
                "complete selects {} task ids, over the {MAX_SELECTED_IDS}-id bound",
                selected_task_ids.len()
            ));
        }
        for (index, task_id) in selected_task_ids.iter().enumerate() {
            if selected_task_ids[..index].contains(task_id) {
                return Err(format!("selected_task_ids repeats task {task_id}"));
            }
        }
        if selected_artifacts.len() > MAX_SELECTED_ARTIFACTS {
            return Err(format!(
                "complete selects {} artifacts, over the {MAX_SELECTED_ARTIFACTS}-artifact bound",
                selected_artifacts.len()
            ));
        }
        let mut artifact_refs = Vec::with_capacity(selected_artifacts.len());
        for artifact in selected_artifacts {
            let path = artifact.path.trim();
            if path.is_empty() {
                return Err("every selected artifact needs a non-empty path".into());
            }
            if !selected_task_ids.contains(&artifact.task_id) {
                return Err(format!(
                    "selected artifact `{path}` names task {}, which is not in selected_task_ids",
                    artifact.task_id
                ));
            }
            if !is_sha256_hex(&artifact.sha256) {
                return Err(format!(
                    "selected artifact `{path}` has a malformed sha256: expected {SHA256_HEX_LEN} lowercase hex characters"
                ));
            }
            artifact_refs.push(SelectedArtifactRef {
                task_id: artifact.task_id,
                artifact: ArtifactMeta {
                    path: path.to_string(),
                    sha256: artifact.sha256,
                },
            });
        }
        Ok(TeamResult {
            answer: answer.to_string(),
            task_refs: selected_task_ids,
            artifact_refs,
        })
    }
}

#[async_trait]
impl LeadBrain for CodexLeadBrain {
    async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
        // The app-server client is a synchronous stdio conversation: a Lead
        // turn is one request/response exchange, and no other work may touch
        // the thread while it is in flight. `&mut self` cannot be moved into
        // `spawn_blocking`, and the product runner drives the Lead from a
        // current-thread runtime where nothing else needs this thread, so
        // blocking directly here is the honest choice.
        self.decide_turn(ctx)
    }
}

impl Drop for CodexLeadBrain {
    fn drop(&mut self) {
        // `close` consumes the client, so take it out and kill the child: a
        // dropped brain must not leave a Codex process behind.
        if let Some(server) = self.server.take() {
            let _ = server.close();
        }
    }
}

/// Pump one turn's events to completion and return its bounded visible reply.
///
/// The loop is bounded by `max_events` and fails closed when the bound is
/// reached, so a wedged turn can never hang the Lead. Elicitations are answered
/// through the single allowlisted RAS bridge and tool calls are refused: the
/// Lead only reasons, and an unanswered request would hang the turn.
fn pump_turn(
    server: &mut CodexAppServer,
    thread_id: &str,
    turn_id: &str,
    max_events: usize,
) -> Result<String, LeadBrainError> {
    for _ in 0..max_events {
        match server
            .next_event()
            .map_err(|e| unavailable("read a codex lead event", e))?
        {
            CodexBridgeEvent::TurnCompleted {
                thread_id: completed_thread,
                turn_id: completed_turn,
            } => {
                // The app-server races events against correlated requests, so a
                // queued completion of an earlier turn can surface first. Only
                // the completion of the turn this round started carries this
                // round's reply; anything else is skipped.
                if !completed_turn.is_empty() && completed_turn != turn_id {
                    continue;
                }
                if !completed_thread.is_empty() && completed_thread != thread_id {
                    continue;
                }
                return server
                    .final_agent_message(&completed_thread, &completed_turn)
                    .map_err(|e| unavailable("read the completed codex lead turn", e));
            }
            CodexBridgeEvent::McpElicitation {
                request_id,
                server_name,
            } => {
                server
                    .respond_ras_elicitation(request_id, &server_name)
                    .map_err(|e| unavailable("answer a codex lead elicitation", e))?;
            }
            CodexBridgeEvent::ToolCall {
                request_id, tool, ..
            } => {
                let refusal = format!("tool `{tool}` is not available to the lead");
                server
                    .respond_tool(request_id, false, &refusal)
                    .map_err(|e| unavailable("refuse a codex lead tool call", e))?;
            }
            CodexBridgeEvent::Notification(_) => {}
        }
    }
    Err(LeadBrainError::Unavailable(format!(
        "the codex lead turn did not complete within {max_events} events"
    )))
}

fn unavailable(action: &str, error: CodexBridgeError) -> LeadBrainError {
    LeadBrainError::Unavailable(format!("codex lead failed to {action}: {error}"))
}

/// The strict decision wire. `deny_unknown_fields` plus a required field for
/// every contract field makes both extra keys and missing keys a rejection.
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum DecisionWire {
    Delegate {
        tasks: Vec<TaskWire>,
    },
    FollowUp {
        task: TaskWire,
    },
    Complete {
        answer: String,
        selected_task_ids: Vec<u64>,
        selected_artifacts: Vec<ArtifactWire>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskWire {
    kind: TaskKindWire,
    /// Required, but `null` means "let the scheduler choose the agent".
    #[serde(deserialize_with = "nullable_string")]
    target: Option<String>,
    objective: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TaskKindWire {
    Reasoning,
    Review,
    Bulk,
    Tool,
    Utility,
}

impl TaskKindWire {
    fn to_task_kind(self) -> TaskKind {
        match self {
            TaskKindWire::Reasoning => TaskKind::Reasoning,
            TaskKindWire::Review => TaskKind::Review,
            TaskKindWire::Bulk => TaskKind::Bulk,
            TaskKindWire::Tool => TaskKind::Tool,
            TaskKindWire::Utility => TaskKind::Utility,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactWire {
    task_id: u64,
    path: String,
    sha256: String,
}

/// A required field that may be `null`. Deserializing through this function
/// keeps a missing `target` key an error while `"target":null` is accepted.
fn nullable_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == SHA256_HEX_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Truncate to at most `max_bytes` without splitting a UTF-8 character.
fn bound_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use agentmosaic_team::{AgentMessage, AgentTaskResult, ArtifactMeta, LeadContext};

    use super::{CodexLeadBrain, CodexLeadConfig};
    use crate::LaunchSpec;

    fn config(max_prompt_bytes: usize) -> CodexLeadConfig {
        CodexLeadConfig {
            launch: LaunchSpec::new("codex", Vec::new()).unwrap(),
            working_directory: PathBuf::from("."),
            model: None,
            overrides: Vec::new(),
            max_prompt_bytes,
            max_answer_bytes: 4096,
            max_events: 16,
        }
    }

    fn brain() -> CodexLeadBrain {
        CodexLeadBrain::new(config(4096), vec!["worker-a".into(), "reasoner-a".into()]).unwrap()
    }

    fn context() -> LeadContext {
        LeadContext {
            root_task_id: 1,
            objective: "analyze the dataset".into(),
            round: 1,
            candidates: vec!["worker-a".into(), "reasoner-a".into()],
            results: (0..2)
                .map(|index| {
                    (
                        index + 2,
                        AgentTaskResult {
                            task_id: index + 2,
                            summary: format!("summary {index}"),
                            artifacts: Vec::new(),
                            message: None,
                        },
                    )
                })
                .collect(),
            artifacts: vec![ArtifactMeta {
                path: "result.txt".into(),
                sha256: "a".repeat(64),
            }],
            failures: vec![(99, "boom".into())],
            messages: vec![AgentMessage {
                from_agent: "worker-a".into(),
                to_agent: "lead".into(),
                body: "done".into(),
            }],
        }
    }

    /// A context whose long summaries and many entries overflow any sane
    /// prompt budget, so the render bound is exercised.
    fn huge_context() -> LeadContext {
        LeadContext {
            results: (0..40)
                .map(|index| {
                    (
                        index + 2,
                        AgentTaskResult {
                            task_id: index + 2,
                            summary: "s".repeat(4000),
                            artifacts: Vec::new(),
                            message: None,
                        },
                    )
                })
                .collect(),
            failures: vec![(99, "boom".repeat(4000))],
            messages: vec![AgentMessage {
                from_agent: "worker-a".into(),
                to_agent: "lead".into(),
                body: "m".repeat(4000),
            }],
            ..context()
        }
    }

    #[test]
    fn config_fails_closed_before_any_process_starts() {
        let mut broken = config(4096);
        broken.launch = LaunchSpec {
            program: "  ".into(),
            args: Vec::new(),
        };
        assert!(broken.validate().is_err());
        broken = config(4096);
        broken.working_directory = PathBuf::from("definitely-not-a-directory");
        assert!(broken.validate().is_err());
        broken = config(4096);
        broken.max_events = 0;
        assert!(broken.validate().is_err());
        broken = config(4096);
        broken.max_answer_bytes = 0;
        assert!(broken.validate().is_err());
        broken = config(4096);
        broken.model = Some("  ".into());
        assert!(broken.validate().is_err());
        assert!(config(4096).validate().is_ok());
        // A budget too small to carry the contract is refused up front.
        assert!(brain_config_error(16));
    }

    fn brain_config_error(max_prompt_bytes: usize) -> bool {
        CodexLeadBrain::new(config(max_prompt_bytes), vec!["worker-a".into()]).is_err()
    }

    #[test]
    fn prompt_is_bounded_and_carries_only_board_facts() {
        let brain = brain();
        let prompt = brain.render_prompt(&context());
        assert!(prompt.len() <= 4096, "prompt was {} bytes", prompt.len());
        assert!(prompt.starts_with("Current lead context (compact JSON):"));
        assert!(prompt.contains("\"candidates\""));
        assert!(prompt.contains("\"results\""));
        assert!(prompt.contains("\"artifacts\""));
        assert!(prompt.contains("\"failures\""));
        assert!(prompt.contains("\"messages\""));
        assert!(prompt
            .trim_end()
            .ends_with("developer instructions specify."));
    }

    #[test]
    fn tiny_budget_still_bounds_the_prompt() {
        let brain = CodexLeadBrain::new(config(1024), vec!["worker-a".into()]).unwrap();
        let prompt = brain.render_prompt(&huge_context());
        assert!(prompt.len() <= 1024, "prompt was {} bytes", prompt.len());
        let correction = brain.correction_prompt(&"bad".repeat(4096));
        assert!(correction.len() <= 1024);
        assert!(correction.ends_with("developer instructions specify."));
    }

    #[test]
    fn an_overflowing_context_is_truncated_within_the_bound() {
        let brain = brain();
        let prompt = brain.render_prompt(&huge_context());
        assert!(prompt.len() <= 4096, "prompt was {} bytes", prompt.len());
        // Per-summary truncation happens before the whole-prompt bound, so no
        // single result can crowd out every other entry.
        assert!(prompt.contains("\"candidates\""));
    }

    #[test]
    fn delegate_reply_maps_to_specs() {
        let brain = brain();
        let decision = brain
            .parse_reply(
                r#"{"action":"delegate","tasks":[{"kind":"bulk","target":"worker-a","objective":"  scrape  "},{"kind":"utility","target":null,"objective":"fetch"}]}"#,
            )
            .unwrap();
        match decision {
            agentmosaic_team::LeadDecision::Delegate(specs) => {
                assert_eq!(specs.len(), 2);
                assert_eq!(specs[0].objective, "scrape");
                assert_eq!(specs[0].kind, agentmosaic_team::TaskKind::Bulk);
                assert_eq!(specs[0].target.as_deref(), Some("worker-a"));
                assert!(specs[0].parent.is_none());
                assert!(specs[0].context.is_empty());
                assert_eq!(specs[1].kind, agentmosaic_team::TaskKind::Utility);
                assert!(specs[1].target.is_none());
            }
            other => panic!("expected delegate, got {other:?}"),
        }
    }

    #[test]
    fn single_task_delegate_reply_is_accepted() {
        let brain = brain();
        let decision = brain
            .parse_reply(
                r#"{"action":"delegate","tasks":[{"kind":"bulk","target":"worker-a","objective":"scrape"}]}"#,
            )
            .expect("one delegated task is the contract's lower bound");
        match decision {
            agentmosaic_team::LeadDecision::Delegate(specs) => {
                assert_eq!(specs.len(), 1);
                assert_eq!(specs[0].objective, "scrape");
                assert_eq!(specs[0].kind, agentmosaic_team::TaskKind::Bulk);
                assert_eq!(specs[0].target.as_deref(), Some("worker-a"));
            }
            other => panic!("expected delegate, got {other:?}"),
        }
    }

    #[test]
    fn every_contract_violation_is_rejected() {
        let brain = brain();
        let cases = [
            ("unknown top-level field", r#"{"action":"delegate","tasks":[{"kind":"bulk","target":null,"objective":"o"}],"note":"x"}"#),
            ("unknown task field", r#"{"action":"delegate","tasks":[{"kind":"bulk","target":null,"objective":"o","parent":2}]}"#),
            ("missing task field", r#"{"action":"delegate","tasks":[{"kind":"bulk","objective":"o"}]}"#),
            ("missing complete field", r#"{"action":"complete","answer":"a","selected_task_ids":[2]}"#),
            ("prose around the object", "here you go: {\"action\":\"delegate\",\"tasks\":[{\"kind\":\"bulk\",\"target\":null,\"objective\":\"o\"}]}"),
            ("markdown fence", "```json\n{\"action\":\"delegate\",\"tasks\":[{\"kind\":\"bulk\",\"target\":null,\"objective\":\"o\"}]}\n```"),
            ("empty objective", r#"{"action":"delegate","tasks":[{"kind":"bulk","target":null,"objective":"   "}]}"#),
            ("empty task list", r#"{"action":"delegate","tasks":[]}"#),
            ("bad kind", r#"{"action":"delegate","tasks":[{"kind":"wizardry","target":null,"objective":"o"}]}"#),
            ("unknown action", r#"{"action":"ponder","tasks":[]}"#),
            ("target not a candidate", r#"{"action":"delegate","tasks":[{"kind":"bulk","target":"rogue","objective":"o"}]}"#),
            ("follow-up target not a candidate", r#"{"action":"follow_up","task":{"kind":"bulk","target":"rogue","objective":"o"}}"#),
            ("empty answer", r#"{"action":"complete","answer":"  ","selected_task_ids":[2],"selected_artifacts":[]}"#),
            ("no selected ids", r#"{"action":"complete","answer":"a","selected_task_ids":[],"selected_artifacts":[]}"#),
            ("repeated id", r#"{"action":"complete","answer":"a","selected_task_ids":[2,2],"selected_artifacts":[]}"#),
            ("malformed sha256", r#"{"action":"complete","answer":"a","selected_task_ids":[2],"selected_artifacts":[{"task_id":2,"path":"r.txt","sha256":"abc"}]}"#),
            ("uppercase sha256", r#"{"action":"complete","answer":"a","selected_task_ids":[2],"selected_artifacts":[{"task_id":2,"path":"r.txt","sha256":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}]}"#),
            ("artifact for an unselected task", r#"{"action":"complete","answer":"a","selected_task_ids":[2],"selected_artifacts":[{"task_id":3,"path":"r.txt","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#),
            ("empty reply", "   "),
        ];
        for (name, reply) in cases {
            assert!(brain.parse_reply(reply).is_err(), "{name} was accepted");
        }
    }

    #[test]
    fn too_many_tasks_is_rejected() {
        let brain = brain();
        let tasks: Vec<String> = (0..33)
            .map(|index| format!(r#"{{"kind":"bulk","target":null,"objective":"o{index}"}}"#))
            .collect();
        let reply = format!(r#"{{"action":"delegate","tasks":[{}]}}"#, tasks.join(","));
        let error = brain.parse_reply(&reply).unwrap_err();
        assert!(error.contains("1 to 32"), "{error}");
    }

    #[test]
    fn complete_reply_maps_to_a_team_result() {
        let brain = brain();
        let sha = "b".repeat(64);
        let reply = format!(
            r#"{{"action":"complete","answer":" done ","selected_task_ids":[2],"selected_artifacts":[{{"task_id":2,"path":"result.txt","sha256":"{sha}"}}]}}"#
        );
        match brain.parse_reply(&reply).unwrap() {
            agentmosaic_team::LeadDecision::Complete(result) => {
                assert_eq!(result.answer, "done");
                assert_eq!(result.task_refs, vec![2]);
                assert_eq!(result.artifact_refs.len(), 1);
                assert_eq!(result.artifact_refs[0].task_id, 2);
                assert_eq!(result.artifact_refs[0].artifact.path, "result.txt");
                assert_eq!(result.artifact_refs[0].artifact.sha256, sha);
            }
            other => panic!("expected complete, got {other:?}"),
        }
    }

    #[test]
    fn exec_prompt_carries_the_contract_that_app_server_gets_at_thread_start() {
        let prompt = brain().render_exec_prompt(&LeadContext {
            root_task_id: 9,
            objective: "delegate safely".into(),
            round: 0,
            candidates: vec!["worker-a".into()],
            results: Vec::new(),
            artifacts: Vec::new(),
            failures: Vec::new(),
            messages: Vec::new(),
        });
        assert!(prompt.contains("You are the Lead of a heterogeneous coding agent team"));
        assert!(prompt.contains("exactly one JSON object"));
        assert!(prompt.contains("\"root_task_id\":9"));
    }

    #[test]
    fn answer_over_the_bound_is_rejected() {
        let mut config = config(4096);
        config.max_answer_bytes = 8;
        let brain = CodexLeadBrain::new(config, vec!["worker-a".into()]).unwrap();
        let reply = r#"{"action":"complete","answer":"0123456789","selected_task_ids":[2],"selected_artifacts":[]}"#;
        let error = brain.parse_reply(reply).unwrap_err();
        assert!(error.contains("over the 8-byte bound"), "{error}");
    }
}
