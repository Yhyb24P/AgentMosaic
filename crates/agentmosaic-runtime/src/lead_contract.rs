//! Bounded Lead prompt and strict decision contract, independent of transport.
use agentmosaic_team::{
    ArtifactMeta, LeadBrainError, LeadContext, LeadDecision, SelectedArtifactRef, TaskKind,
    TaskSpec, TeamResult,
};
use serde::Deserialize;
use serde_json::{json, Value};
const MAX_DELEGATED_TASKS: usize = 32;
const MAX_SELECTED_IDS: usize = 256;
const MAX_SELECTED_ARTIFACTS: usize = 256;
const SHA256_HEX_LEN: usize = 64;

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

pub(crate) struct LeadContract {
    max_prompt_bytes: usize,
    max_answer_bytes: usize,
    candidates: Vec<String>,
}
impl LeadContract {
    pub(crate) fn new(
        max_prompt_bytes: usize,
        max_answer_bytes: usize,
        candidates: Vec<String>,
    ) -> Self {
        Self {
            max_prompt_bytes,
            max_answer_bytes,
            candidates,
        }
    }
    /// The single correction turn's input: why the reply was rejected, then the
    /// contract again. The reason is bounded so this prompt also stays within
    /// `max_prompt_bytes`.
    pub(crate) fn correction_prompt(&self, reason: &str) -> String {
        let head_budget = self
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
    /// result summaries, artifact digests, bounded errors) — never
    /// hidden model reasoning.
    pub(crate) fn render_prompt(&self, ctx: &LeadContext) -> Result<String, LeadBrainError> {
        let budget = self
            .max_prompt_bytes
            .saturating_sub(PROMPT_PREFIX.len() + PROMPT_SUFFIX.len() + 2);
        let context = self.render_context(ctx, budget)?;
        Ok(format!("{PROMPT_PREFIX}{context}\n{PROMPT_SUFFIX}"))
    }

    /// Include the fixed decision contract with every Exec Lead turn.
    pub(crate) fn render_exec_prompt(&self, ctx: &LeadContext) -> Result<String, LeadBrainError> {
        // The context bound is independent of the fixed contract size.
        Ok(format!(
            "{DEVELOPER_INSTRUCTIONS}\n\n{}",
            self.render_prompt(ctx)?
        ))
    }

    fn render_context(&self, ctx: &LeadContext, budget: usize) -> Result<String, LeadBrainError> {
        // Account for JSON escaping and all metadata using actual serialized
        // bytes. Never cut identifiers, references, or the serialized document.
        // Keep at least a small excerpt of each text; refuse a context whose
        // complete references and minimum excerpts cannot fit.
        let mut low = 64;
        let mut high = 4096;
        let mut best = Self::context_payload(ctx, low);
        if best.len() > budget {
            return Err(LeadBrainError::Rejected(format!(
                "Lead context capacity exceeded: complete references and minimum text need {} bytes; budget is {budget}. Reduce the team, task or artifact count, shorten agent identifiers, or increase max_prompt_bytes.",
                best.len()
            )));
        }
        while low < high {
            let mid = low + (high - low).div_ceil(2);
            let rendered = Self::context_payload(ctx, mid);
            if rendered.len() <= budget {
                low = mid;
                best = rendered;
            } else {
                high = mid - 1;
            }
        }
        Ok(best)
    }

    fn context_payload(ctx: &LeadContext, per_text: usize) -> String {
        let results: Vec<Value> = ctx
            .results
            .iter()
            .map(|(task_id, result)| {
                json!({
                    "task_id": task_id,
                    "summary": excerpt(&result.summary, per_text),
                })
            })
            .collect();
        let artifacts: Vec<Value> = ctx
            .artifacts
            .iter()
            .map(|artifact| {
                json!({
                    "task_id": artifact.task_id,
                    "path": artifact.artifact.path,
                    "sha256": artifact.artifact.sha256,
                })
            })
            .collect();
        let failures: Vec<Value> = ctx
            .failures
            .iter()
            .map(|(task_id, error)| {
                json!({
                    "task_id": task_id,
                    "error": excerpt(error, per_text),
                })
            })
            .collect();
        let payload = json!({
            "root_task_id": ctx.root_task_id,
            "objective": excerpt(&ctx.objective, per_text),
            "round": ctx.round,
            "candidates": ctx.candidates,
            "results": results,
            "artifacts": artifacts,
            "failures": failures,
        });
        payload.to_string()
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
        if answer.len() > self.max_answer_bytes {
            return Err(format!(
                "the complete answer is {} bytes, over the {}-byte bound",
                answer.len(),
                self.max_answer_bytes
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

/// Mark omitted text explicitly. The cap applies to source text bytes; JSON
/// escaping is accounted for by the caller's serialized-size search.
fn excerpt(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    const MARKER: &str = " [truncated]";
    format!(
        "{}{MARKER}",
        bound_utf8(text, max_bytes.saturating_sub(MARKER.len()))
    )
}

#[cfg(test)]
mod tests {

    use agentmosaic_team::{AgentTaskResult, ArtifactMeta, LeadContext, SelectedArtifactRef};

    use super::LeadContract;

    fn brain() -> LeadContract {
        LeadContract::new(4096, 4096, vec!["worker-a".into(), "reasoner-a".into()])
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
                        },
                    )
                })
                .collect(),
            artifacts: vec![SelectedArtifactRef {
                task_id: 2,
                artifact: ArtifactMeta {
                    path: "result.txt".into(),
                    sha256: "a".repeat(64),
                },
            }],
            failures: vec![(99, "boom".into())],
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
                        },
                    )
                })
                .collect(),
            failures: vec![(99, "boom".repeat(4000))],
            ..context()
        }
    }

    #[test]
    fn prompt_is_bounded_and_carries_only_board_facts() {
        let brain = brain();
        let prompt = brain.render_prompt(&context()).unwrap();
        assert!(prompt.len() <= 4096, "prompt was {} bytes", prompt.len());
        assert!(prompt.starts_with("Current lead context (compact JSON):"));
        assert!(prompt.contains("\"candidates\""));
        assert!(prompt.contains("\"results\""));
        assert!(prompt.contains("\"artifacts\""));
        assert!(prompt.contains("\"failures\""));
        assert!(prompt
            .trim_end()
            .ends_with("developer instructions specify."));
    }

    #[test]
    fn tiny_budget_refuses_context_that_cannot_preserve_all_references() {
        let brain = LeadContract::new(1024, 4096, vec!["worker-a".into()]);
        let error = brain.render_prompt(&huge_context()).unwrap_err();
        assert!(error.to_string().contains("context capacity exceeded"));
        let correction = brain.correction_prompt(&"bad".repeat(4096));
        assert!(correction.len() <= 1024);
        assert!(correction.ends_with("developer instructions specify."));
    }

    #[test]
    fn an_overflowing_context_is_truncated_within_the_bound() {
        let brain = LeadContract::new(8192, 4096, vec!["worker-a".into()]);
        let prompt = brain.render_prompt(&huge_context()).unwrap();
        assert!(prompt.len() <= 8192, "prompt was {} bytes", prompt.len());
        let json = prompt
            .strip_prefix(super::PROMPT_PREFIX)
            .unwrap()
            .strip_suffix(&format!("\n{}", super::PROMPT_SUFFIX))
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(payload["results"].as_array().unwrap().len(), 40);
        assert_eq!(payload["artifacts"][0]["task_id"], 2);
        assert!(payload["results"][0]["summary"]
            .as_str()
            .unwrap()
            .ends_with(" [truncated]"));
    }

    #[test]
    fn default_task_budget_preserves_json_and_all_result_ids() {
        let brain = LeadContract::new(32768, 4096, vec!["worker-a".into()]);
        for count in [8, 31, 32] {
            for text in ["x", "\"\\\n\t", "中文🦀"] {
                let mut ctx = huge_context();
                ctx.results.truncate(count);
                for (_, result) in &mut ctx.results {
                    result.summary = text.repeat(4096);
                }
                let prompt = brain.render_prompt(&ctx).unwrap();
                assert!(prompt.len() <= 32768);
                let json = prompt
                    .strip_prefix(super::PROMPT_PREFIX)
                    .unwrap()
                    .strip_suffix(&format!("\n{}", super::PROMPT_SUFFIX))
                    .unwrap();
                let payload: serde_json::Value = serde_json::from_str(json).unwrap();
                assert_eq!(payload["results"].as_array().unwrap().len(), count);
                for (index, result) in payload["results"].as_array().unwrap().iter().enumerate() {
                    assert_eq!(result["task_id"], index + 2);
                    assert!(!result["summary"].as_str().unwrap().is_empty());
                }
                assert_eq!(payload["candidates"], serde_json::json!(ctx.candidates));
            }
        }
    }

    #[test]
    fn artifact_identity_and_ownership_survive_text_reduction() {
        let brain = LeadContract::new(8192, 4096, vec!["worker-a".into()]);
        let mut ctx = huge_context();
        let path = format!("{}result.json", "目录/".repeat(60));
        ctx.artifacts = vec![
            SelectedArtifactRef {
                task_id: 2,
                artifact: ArtifactMeta {
                    path: path.clone(),
                    sha256: "a".repeat(64),
                },
            },
            SelectedArtifactRef {
                task_id: 3,
                artifact: ArtifactMeta {
                    path: path.clone(),
                    sha256: "b".repeat(64),
                },
            },
        ];
        let payload: serde_json::Value =
            serde_json::from_str(&brain.render_context(&ctx, 7978).unwrap()).unwrap();
        for (index, artifact) in payload["artifacts"].as_array().unwrap().iter().enumerate() {
            assert_eq!(artifact["path"], path);
            assert_eq!(artifact["task_id"], index + 2);
            assert_eq!(artifact["sha256"], ctx.artifacts[index].artifact.sha256);
        }
    }

    #[test]
    fn excessive_metadata_fails_before_spawning_a_lead() {
        let brain = brain();
        let mut ctx = context();
        ctx.candidates.push("w".repeat(33000));
        assert!(brain
            .render_prompt(&ctx)
            .unwrap_err()
            .to_string()
            .contains("context capacity exceeded"));
        assert!(brain.render_exec_prompt(&ctx).is_err());

        ctx = context();
        ctx.artifacts = vec![ctx.artifacts[0].clone(); 300];
        assert!(brain.render_prompt(&ctx).is_err());
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
            (
                "unknown top-level field",
                r#"{"action":"delegate","tasks":[{"kind":"bulk","target":null,"objective":"o"}],"note":"x"}"#,
            ),
            (
                "unknown task field",
                r#"{"action":"delegate","tasks":[{"kind":"bulk","target":null,"objective":"o","parent":2}]}"#,
            ),
            (
                "missing task field",
                r#"{"action":"delegate","tasks":[{"kind":"bulk","objective":"o"}]}"#,
            ),
            (
                "missing complete field",
                r#"{"action":"complete","answer":"a","selected_task_ids":[2]}"#,
            ),
            (
                "prose around the object",
                "here you go: {\"action\":\"delegate\",\"tasks\":[{\"kind\":\"bulk\",\"target\":null,\"objective\":\"o\"}]}",
            ),
            (
                "markdown fence",
                "```json\n{\"action\":\"delegate\",\"tasks\":[{\"kind\":\"bulk\",\"target\":null,\"objective\":\"o\"}]}\n```",
            ),
            (
                "empty objective",
                r#"{"action":"delegate","tasks":[{"kind":"bulk","target":null,"objective":"   "}]}"#,
            ),
            ("empty task list", r#"{"action":"delegate","tasks":[]}"#),
            (
                "bad kind",
                r#"{"action":"delegate","tasks":[{"kind":"wizardry","target":null,"objective":"o"}]}"#,
            ),
            ("unknown action", r#"{"action":"ponder","tasks":[]}"#),
            (
                "target not a candidate",
                r#"{"action":"delegate","tasks":[{"kind":"bulk","target":"rogue","objective":"o"}]}"#,
            ),
            (
                "follow-up target not a candidate",
                r#"{"action":"follow_up","task":{"kind":"bulk","target":"rogue","objective":"o"}}"#,
            ),
            (
                "empty answer",
                r#"{"action":"complete","answer":"  ","selected_task_ids":[2],"selected_artifacts":[]}"#,
            ),
            (
                "no selected ids",
                r#"{"action":"complete","answer":"a","selected_task_ids":[],"selected_artifacts":[]}"#,
            ),
            (
                "repeated id",
                r#"{"action":"complete","answer":"a","selected_task_ids":[2,2],"selected_artifacts":[]}"#,
            ),
            (
                "malformed sha256",
                r#"{"action":"complete","answer":"a","selected_task_ids":[2],"selected_artifacts":[{"task_id":2,"path":"r.txt","sha256":"abc"}]}"#,
            ),
            (
                "uppercase sha256",
                r#"{"action":"complete","answer":"a","selected_task_ids":[2],"selected_artifacts":[{"task_id":2,"path":"r.txt","sha256":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}]}"#,
            ),
            (
                "artifact for an unselected task",
                r#"{"action":"complete","answer":"a","selected_task_ids":[2],"selected_artifacts":[{"task_id":3,"path":"r.txt","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#,
            ),
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
    fn exec_prompt_carries_the_strict_decision_contract() {
        let prompt = brain()
            .render_exec_prompt(&LeadContext {
                root_task_id: 9,
                objective: "delegate safely".into(),
                round: 0,
                candidates: vec!["worker-a".into()],
                results: Vec::new(),
                artifacts: Vec::new(),
                failures: Vec::new(),
            })
            .unwrap();
        assert!(prompt.contains("You are the Lead of a heterogeneous coding agent team"));
        assert!(prompt.contains("exactly one JSON object"));
        assert!(prompt.contains("\"root_task_id\":9"));
    }

    #[test]
    fn answer_over_the_bound_is_rejected() {
        let max_answer_bytes = 8;
        let brain = LeadContract::new(4096, max_answer_bytes, vec!["worker-a".into()]);
        let reply = r#"{"action":"complete","answer":"0123456789","selected_task_ids":[2],"selected_artifacts":[]}"#;
        let error = brain.parse_reply(reply).unwrap_err();
        assert!(error.contains("over the 8-byte bound"), "{error}");
    }
}
