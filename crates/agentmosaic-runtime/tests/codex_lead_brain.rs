//! Deterministic mock-backed tests for the resident Codex Lead brain.
//!
//! No real Codex runs on this path: each test points the brain at the
//! scriptable mock app-server, whose reply script and observation file are
//! passed as `codex_bridge_mock.*` overrides. Nothing is written to the
//! process environment, so tests stay parallel and credential-free, and the
//! mock's observation file makes the brain's thread and turn behaviour
//! directly assertable.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use agentmosaic_runtime::{CodexLeadBrain, CodexLeadConfig};
use agentmosaic_team::{
    AgentMessage, AgentTaskResult, ArtifactMeta, LeadBrain, LeadBrainError, LeadContext,
    LeadDecision, TaskKind,
};
use serde_json::Value;

const MOCK: &str = env!("CARGO_BIN_EXE_codex_bridge_mock");

/// A valid artifact digest shared by the fixtures.
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const DELEGATE_REPLY: &str = concat!(
    r#"{"action":"delegate","tasks":["#,
    r#"{"kind":"bulk","target":"worker-a","objective":"scrape the data"},"#,
    r#"{"kind":"utility","target":null,"objective":"fetch the utility fact"}]}"#,
);

fn complete_reply() -> String {
    format!(
        concat!(
            r#"{{"action":"complete","answer":"combined answer","selected_task_ids":[2,3],"#,
            r#""selected_artifacts":[{{"task_id":2,"path":"result.txt","sha256":"{0}"}}]}}"#,
        ),
        SHA
    )
}

fn follow_up_reply() -> &'static str {
    r#"{"action":"follow_up","task":{"kind":"reasoning","target":"reasoner-a","objective":"refine it"}}"#
}

struct Harness {
    brain: Option<CodexLeadBrain>,
    root: PathBuf,
    state: PathBuf,
}

impl Harness {
    fn new(replies: &[&str]) -> Self {
        Self::scripted(replies, &[])
    }

    /// Build a brain whose mock app-server serves `replies`. `extra` adds mock
    /// scripting (elicitation, tool call, ...) as `codex_bridge_mock.*` overrides.
    fn scripted(replies: &[&str], extra: &[(&str, &str)]) -> Self {
        let root = unique_root();
        let state = root.join("state.json");
        let mut overrides = vec![
            format!(
                "codex_bridge_mock.replies={}",
                serde_json::to_string(replies).unwrap()
            ),
            format!("codex_bridge_mock.state={}", state.display()),
        ];
        overrides.extend(
            extra
                .iter()
                .map(|(key, value)| format!("codex_bridge_mock.{key}={value}")),
        );
        let brain = CodexLeadBrain::new(
            CodexLeadConfig {
                command: MOCK.to_string(),
                working_directory: root.clone(),
                model: None,
                overrides,
                max_prompt_bytes: 8192,
                max_answer_bytes: 4096,
                max_events: 64,
            },
            vec![
                "worker-a".to_string(),
                "worker-b".to_string(),
                "reasoner-a".to_string(),
            ],
        )
        .unwrap();
        Self {
            brain: Some(brain),
            root,
            state,
        }
    }

    /// A brain pointed at a Codex executable that does not exist.
    fn unavailable() -> Self {
        let root = unique_root();
        let brain = CodexLeadBrain::new(
            CodexLeadConfig {
                command: root.join("missing-codex").display().to_string(),
                working_directory: root.clone(),
                model: None,
                overrides: Vec::new(),
                max_prompt_bytes: 8192,
                max_answer_bytes: 4096,
                max_events: 64,
            },
            vec!["worker-a".to_string()],
        )
        .unwrap();
        Self {
            brain: Some(brain),
            root: root.clone(),
            state: root.join("state.json"),
        }
    }

    async fn decide(&mut self, ctx: &LeadContext) -> Result<LeadDecision, LeadBrainError> {
        self.brain.as_mut().unwrap().decide(ctx).await
    }

    fn state(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.state).unwrap_or_else(|error| {
            panic!("mock wrote no state to {}: {error}", self.state.display())
        }))
        .unwrap()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Kill the mock before the directory it runs in goes away.
        self.brain.take();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn unique_root() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "agentmosaic_codex_lead_{}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn context(round: u32) -> LeadContext {
    LeadContext {
        root_task_id: 1,
        objective: "analyze the dataset".into(),
        round,
        candidates: vec!["worker-a".into(), "worker-b".into(), "reasoner-a".into()],
        results: vec![
            (
                2,
                AgentTaskResult {
                    task_id: 2,
                    summary: "data summary".into(),
                    artifacts: vec![ArtifactMeta {
                        path: "result.txt".into(),
                        sha256: SHA.into(),
                    }],
                    message: None,
                },
            ),
            (
                3,
                AgentTaskResult {
                    task_id: 3,
                    summary: "utility output".into(),
                    artifacts: Vec::new(),
                    message: None,
                },
            ),
        ],
        artifacts: vec![ArtifactMeta {
            path: "result.txt".into(),
            sha256: SHA.into(),
        }],
        failures: Vec::new(),
        messages: vec![AgentMessage {
            from_agent: "worker-a".into(),
            to_agent: "lead".into(),
            body: "bulk done".into(),
        }],
    }
}

/// A first bad reply, then a good one: the only path that may recover.
async fn reject_only(reply: &str) {
    let mut harness = Harness::new(&[reply]);
    let error = harness
        .decide(&context(0))
        .await
        .expect_err("the reply must be rejected");
    assert!(
        matches!(error, LeadBrainError::InvalidDecision(_)),
        "expected InvalidDecision, got {error:?}"
    );
}

#[test]
fn the_brain_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<CodexLeadBrain>();
}

#[tokio::test]
async fn delegate_reply_maps_to_a_delegate_decision() {
    let mut harness = Harness::new(&[DELEGATE_REPLY]);
    match harness.decide(&context(0)).await.unwrap() {
        LeadDecision::Delegate(specs) => {
            assert_eq!(specs.len(), 2);
            assert_eq!(specs[0].objective, "scrape the data");
            assert_eq!(specs[0].kind, TaskKind::Bulk);
            assert_eq!(specs[0].target.as_deref(), Some("worker-a"));
            assert!(specs[0].parent.is_none());
            assert!(specs[0].context.is_empty());
            assert_eq!(specs[1].objective, "fetch the utility fact");
            assert_eq!(specs[1].kind, TaskKind::Utility);
            assert_eq!(specs[1].target, None);
        }
        other => panic!("expected a delegate decision, got {other:?}"),
    }
    // One thread, one turn, no correction.
    assert_eq!(harness.state()["thread_starts"], 1);
    assert_eq!(harness.state()["turn_starts"], 1);
}

#[tokio::test]
async fn follow_up_reply_maps_to_a_single_follow_up_task() {
    let mut harness = Harness::new(&[follow_up_reply()]);
    match harness.decide(&context(1)).await.unwrap() {
        LeadDecision::FollowUp(specs) => {
            assert_eq!(specs.len(), 1);
            assert_eq!(specs[0].objective, "refine it");
            assert_eq!(specs[0].kind, TaskKind::Reasoning);
            assert_eq!(specs[0].target.as_deref(), Some("reasoner-a"));
        }
        other => panic!("expected a follow-up decision, got {other:?}"),
    }
}

#[tokio::test]
async fn complete_reply_maps_to_exact_refs() {
    let mut harness = Harness::new(&[&complete_reply()]);
    match harness.decide(&context(1)).await.unwrap() {
        LeadDecision::Complete(result) => {
            assert_eq!(result.answer, "combined answer");
            assert_eq!(result.task_refs, vec![2, 3]);
            assert_eq!(result.artifact_refs.len(), 1);
            let selected = &result.artifact_refs[0];
            // The task id lives on the ref; the artifact carries only path/digest.
            assert_eq!(selected.task_id, 2);
            assert_eq!(selected.artifact.path, "result.txt");
            assert_eq!(selected.artifact.sha256, SHA);
        }
        other => panic!("expected a complete decision, got {other:?}"),
    }
}

#[tokio::test]
async fn unknown_field_is_rejected() {
    reject_only(
        r#"{"action":"delegate","tasks":[{"kind":"bulk","target":"worker-a","objective":"o"}],"note":"x"}"#,
    )
    .await;
}

#[tokio::test]
async fn missing_field_is_rejected() {
    reject_only(r#"{"action":"delegate","tasks":[{"kind":"bulk","objective":"o"}]}"#).await;
    reject_only(r#"{"action":"complete","answer":"a","selected_task_ids":[2]}"#).await;
}

#[tokio::test]
async fn prose_around_the_json_is_rejected() {
    let prose = format!("Sure, here is my plan:\n{DELEGATE_REPLY}\nLet me know if that works.");
    reject_only(&prose).await;
    let fenced = format!("```json\n{DELEGATE_REPLY}\n```");
    reject_only(&fenced).await;
}

#[tokio::test]
async fn empty_objective_is_rejected() {
    reject_only(
        r#"{"action":"delegate","tasks":[{"kind":"bulk","target":null,"objective":"   "}]}"#,
    )
    .await;
}

#[tokio::test]
async fn bad_kind_is_rejected() {
    reject_only(
        r#"{"action":"delegate","tasks":[{"kind":"wizardry","target":null,"objective":"o"}]}"#,
    )
    .await;
}

#[tokio::test]
async fn unknown_target_is_rejected() {
    reject_only(
        r#"{"action":"delegate","tasks":[{"kind":"bulk","target":"rogue-agent","objective":"o"}]}"#,
    )
    .await;
}

#[tokio::test]
async fn malformed_sha256_is_rejected() {
    let short = r#"{"action":"complete","answer":"a","selected_task_ids":[2],"selected_artifacts":[{"task_id":2,"path":"r.txt","sha256":"abc"}]}"#;
    reject_only(short).await;
    let uppercase = format!(
        r#"{{"action":"complete","answer":"a","selected_task_ids":[2],"selected_artifacts":[{{"task_id":2,"path":"r.txt","sha256":"{}"}}]}}"#,
        SHA.to_uppercase()
    );
    reject_only(&uppercase).await;
}

#[tokio::test]
async fn task_count_over_the_bound_is_rejected() {
    let tasks: Vec<String> = (0..33)
        .map(|index| format!(r#"{{"kind":"bulk","target":null,"objective":"o{index}"}}"#))
        .collect();
    let reply = format!(r#"{{"action":"delegate","tasks":[{}]}}"#, tasks.join(","));
    reject_only(&reply).await;
}

#[tokio::test]
async fn a_bad_reply_earns_exactly_one_correction_turn() {
    let mut harness = Harness::new(&["this is not JSON at all", DELEGATE_REPLY]);
    harness
        .decide(&context(0))
        .await
        .expect("the corrected reply is accepted");
    let state = harness.state();
    assert_eq!(state["thread_starts"], 1);
    assert_eq!(state["turn_starts"], 2);
    assert_eq!(state["replies_consumed"], 2);
    // The second turn is the correction: it carries the rejection reason.
    let correction = state["last_prompt"].as_str().unwrap();
    assert!(
        correction.contains("Your previous reply was rejected"),
        "correction prompt was {correction}"
    );
}

#[tokio::test]
async fn two_bad_replies_return_invalid_decision_after_exactly_two_turns() {
    let mut harness = Harness::new(&["nope", "still not JSON"]);
    let error = harness
        .decide(&context(0))
        .await
        .expect_err("a second rejection is final");
    match error {
        LeadBrainError::InvalidDecision(reason) => {
            assert!(reason.contains("codex lead reply rejected"), "{reason}");
            assert!(reason.contains("correction reply rejected"), "{reason}");
        }
        other => panic!("expected InvalidDecision, got {other:?}"),
    }
    // Exactly two turns were asked of the mock: the reply and its correction.
    let state = harness.state();
    assert_eq!(state["thread_starts"], 1);
    assert_eq!(state["turn_starts"], 2);
    assert_eq!(state["replies_consumed"], 2);
}

#[tokio::test]
async fn the_thread_is_resident_across_rounds() {
    let mut harness = Harness::new(&[DELEGATE_REPLY, &complete_reply()]);
    assert!(matches!(
        harness.decide(&context(0)).await.unwrap(),
        LeadDecision::Delegate(_)
    ));
    match harness.decide(&context(1)).await.unwrap() {
        LeadDecision::Complete(result) => assert_eq!(result.task_refs, vec![2, 3]),
        other => panic!("expected a complete decision, got {other:?}"),
    }
    // Two rounds, two turns, one thread.
    let state = harness.state();
    assert_eq!(state["thread_starts"], 1);
    assert_eq!(state["turn_starts"], 2);
    assert_eq!(
        state["turns"],
        serde_json::json!(["mock-turn", "mock-turn-2"])
    );
    // A third round reuses the same thread and repeats the exhausted reply.
    match harness.decide(&context(2)).await.unwrap() {
        LeadDecision::Complete(result) => assert_eq!(result.task_refs, vec![2, 3]),
        other => panic!("expected the repeated complete decision, got {other:?}"),
    }
    let state = harness.state();
    assert_eq!(state["thread_starts"], 1);
    assert_eq!(state["turn_starts"], 3);
    assert_eq!(state["replies"][1], state["replies"][2]);
}

#[tokio::test]
async fn the_event_pump_answers_an_elicitation() {
    let mut harness = Harness::scripted(&[DELEGATE_REPLY], &[("elicitation", "agentmosaic")]);
    harness
        .decide(&context(0))
        .await
        .expect("the turn completes");
    assert_eq!(harness.state()["elicitation_responses"], 1);
}

#[tokio::test]
async fn the_event_pump_refuses_a_tool_call() {
    let mut harness = Harness::scripted(&[DELEGATE_REPLY], &[("tool_call", "shell")]);
    harness
        .decide(&context(0))
        .await
        .expect("the turn completes");
    // The Lead only reasons: the tool call is answered with a refusal instead
    // of being left to hang the turn.
    assert_eq!(harness.state()["tool_responses"], 1);
}

#[tokio::test]
async fn a_missing_codex_executable_is_unavailable() {
    let mut harness = Harness::unavailable();
    let error = harness.decide(&context(0)).await.expect_err("spawn fails");
    assert!(
        matches!(error, LeadBrainError::Unavailable(_)),
        "expected Unavailable, got {error:?}"
    );
}

/// The mock's documented environment channel: a caller that cannot pass
/// `-c` overrides scripts it with `CODEX_BRIDGE_MOCK_*` instead. The variables
/// are set on the child only, so this test stays independent of its neighbours.
#[test]
fn the_mock_script_can_come_from_the_environment() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let root = unique_root();
    let state = root.join("state.json");
    let mut child = Command::new(MOCK)
        .args(["app-server", "--stdio"])
        .env("CODEX_BRIDGE_MOCK_REPLIES", r#"["env reply"]"#)
        .env("CODEX_BRIDGE_MOCK_STATE", &state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let requests = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"thread/start","params":{"cwd":"/tmp"}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"turn/start","params":{"threadId":"mock-thread","input":[{"type":"text","text":"hi"}]}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"thread/read","params":{"threadId":"mock-thread","includeTurns":true}}"#,
    ];
    for request in requests {
        writeln!(stdin, "{request}").unwrap();
    }
    stdin.flush().unwrap();
    let mut reply = String::new();
    for _ in 0..32 {
        let mut line = String::new();
        if stdout.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let value: Value = serde_json::from_str(&line).unwrap();
        if value.get("id") == Some(&serde_json::json!(4)) {
            reply = value
                .pointer("/result/thread/turns/0/items/0/text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            break;
        }
    }
    assert_eq!(reply, "env reply");
    let recorded: Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    assert_eq!(recorded["thread_starts"], 1);
    assert_eq!(recorded["turn_starts"], 1);
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&root);
}
