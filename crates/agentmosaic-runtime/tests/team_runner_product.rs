//! Deterministic product tests for `TeamRunner` and `DriverFactory`.
//!
//! No credentials and no live runtimes: the Lead is a real
//! `PersistedCodexTeamDriver`/`CodexLeadBrain` pointed at the scripted
//! `codex_bridge_mock` app-server, and the Worker/Utility agents are real
//! `PersistedAcpWorkerDriver`s pointed at `acp_m2_mock`. Everything else is the
//! product path: the durable registry, the SQLite board, the scheduler, and the
//! Lead loop.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use agentmosaic_runtime::{TeamRunOptions, TeamRunner, TeamRunnerError};
use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry, SqliteTaskBoard};
use agentmosaic_team::{reconstruct_team_result, TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};

const CODEX_MOCK: &str = env!("CARGO_BIN_EXE_codex_bridge_mock");
const ACP_MOCK: &str = env!("CARGO_BIN_EXE_acp_m2_mock");

const LEAD_ANSWER: &str = "lead synthesized final answer";
const WORKER_ARTIFACT: &str = "worker-result.txt";
const WORKER_ARTIFACT_BYTES: &[u8] = b"worker artifact\n";

/// One isolated board + repository + mock observation file.
struct Fixture {
    root: PathBuf,
    database: PathBuf,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "agentmosaic_team_runner_{name}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        Self {
            database: root.join("board.db"),
            state: root.join("codex-mock-state.json"),
            repo,
            root,
        }
    }

    /// Write the worker's artifact and return its exact digest, so the scripted
    /// Lead can select the artifact the worker driver actually produces.
    fn write_worker_artifact(&self) -> String {
        std::fs::write(self.repo.join(WORKER_ARTIFACT), WORKER_ARTIFACT_BYTES).unwrap();
        format!("{:x}", Sha256::digest(WORKER_ARTIFACT_BYTES))
    }

    fn register(&self, record: &AgentRegistryRecord) {
        SqliteAgentRegistry::open(&self.database)
            .expect("open registry")
            .upsert_agent(record)
            .expect("register agent");
    }

    fn open_board(&self) -> SqliteTaskBoard {
        SqliteTaskBoard::open(Connection::open(&self.database).unwrap()).expect("open board")
    }

    /// The number of turns the Lead's mock process actually served.
    fn lead_turns(&self) -> u64 {
        let raw = std::fs::read_to_string(&self.state).expect("mock wrote its state file");
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        value["turn_starts"].as_u64().expect("turn_starts counter")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn runner(fixture: &Fixture) -> TeamRunner {
    TeamRunner::new(&fixture.database, &fixture.repo, TeamRunOptions::default())
}

fn acp_agent(id: &str, tier: &str, artifact_paths: &[&str]) -> AgentRegistryRecord {
    AgentRegistryRecord {
        id: id.into(),
        name: id.into(),
        tier: tier.into(),
        driver_kind: Some("acp".into()),
        executable: Some(ACP_MOCK.into()),
        runtime_version: None,
        driver_args_json: Some("[]".into()),
        max_concurrency: Some(1),
        tags_json: Some("[]".into()),
        driver_config_json: Some(
            json!({
                "timeout_seconds": 60,
                "artifact_paths": artifact_paths,
            })
            .to_string(),
        ),
    }
}

fn codex_agent(id: &str, fixture: &Fixture, replies: &[String]) -> AgentRegistryRecord {
    let overrides = vec![
        format!(
            "codex_bridge_mock.replies={}",
            serde_json::to_string(replies).unwrap()
        ),
        format!("codex_bridge_mock.state={}", fixture.state.display()),
    ];
    AgentRegistryRecord {
        id: id.into(),
        name: id.into(),
        tier: "reasoner".into(),
        driver_kind: Some("codex-app-server".into()),
        executable: Some(CODEX_MOCK.into()),
        runtime_version: None,
        driver_args_json: Some("[]".into()),
        max_concurrency: Some(1),
        tags_json: Some("[]".into()),
        driver_config_json: Some(
            json!({
                "mcp_command": CODEX_MOCK,
                "overrides": overrides,
                "max_events": 64,
            })
            .to_string(),
        ),
    }
}

fn delegate_reply() -> String {
    json!({
        "action": "delegate",
        "tasks": [
            {"kind": "bulk", "target": "worker", "objective": "produce the worker artifact"},
            {"kind": "utility", "target": "utility", "objective": "produce the utility result"},
        ],
    })
    .to_string()
}

/// The Lead's final decision: it selects one exact task and one exact artifact,
/// never "every successful descendant".
fn complete_reply(selected_task: u64, artifact: Option<&str>, sha256: &str) -> String {
    let selected_artifacts: Vec<serde_json::Value> = artifact
        .map(|path| json!({"task_id": selected_task, "path": path, "sha256": sha256}))
        .into_iter()
        .collect();
    json!({
        "action": "complete",
        "answer": LEAD_ANSWER,
        "selected_task_ids": [selected_task],
        "selected_artifacts": selected_artifacts,
    })
    .to_string()
}

fn register_trio(fixture: &Fixture, replies: &[String], lead_id: &str) {
    fixture.register(&acp_agent("worker", "worker", &[WORKER_ARTIFACT]));
    fixture.register(&acp_agent("utility", "utility", &[]));
    fixture.register(&codex_agent(lead_id, fixture, replies));
}

fn register_lead_and_worker(fixture: &Fixture, replies: &[String], lead_id: &str) {
    fixture.register(&acp_agent("worker", "worker", &[WORKER_ARTIFACT]));
    fixture.register(&codex_agent(lead_id, fixture, replies));
}

fn delegate_two_worker_tasks_reply() -> String {
    json!({
        "action": "delegate",
        "tasks": [
            {"kind": "bulk", "target": "worker", "objective": "produce the worker artifact"},
            {"kind": "tool", "target": "worker", "objective": "perform the worker check"},
        ],
    })
    .to_string()
}

#[tokio::test]
async fn lead_and_worker_without_utility_reaches_the_team_runner() {
    let fixture = Fixture::new("lead-worker-only");
    let sha256 = fixture.write_worker_artifact();
    register_lead_and_worker(
        &fixture,
        &[
            delegate_two_worker_tasks_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
        ],
        "lead",
    );

    let outcome = runner(&fixture)
        .run("deliver the objective")
        .await
        .expect("a Lead and Worker are a runnable team");
    assert_eq!(outcome.root_task_id, 1);
    let board = fixture.open_board();
    assert_eq!(board.attempts(2).unwrap()[0].agent_id, "worker");
    assert_eq!(board.attempts(3).unwrap()[0].agent_id, "worker");
}

// The whole product path: one objective in, one durable team result out, with
// the delegated task executed by the registered worker driver and the Lead's
// exact selection persisted on the root.
#[tokio::test]
async fn one_objective_becomes_a_durable_team_result() {
    let fixture = Fixture::new("product");
    let sha256 = fixture.write_worker_artifact();
    register_trio(
        &fixture,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
        ],
        "lead",
    );

    let outcome = runner(&fixture)
        .run("deliver the objective")
        .await
        .expect("the team run completes");

    assert_eq!(outcome.root_task_id, 1);
    assert_eq!(outcome.lead_agent, "lead");
    assert_eq!(outcome.result.answer, LEAD_ANSWER);
    assert_eq!(outcome.result.task_refs, vec![2]);
    assert_eq!(outcome.result.artifact_refs.len(), 1);
    assert_eq!(
        outcome.result.artifact_refs[0].artifact.path,
        WORKER_ARTIFACT
    );
    assert_eq!(outcome.result.artifact_refs[0].artifact.sha256, sha256);

    let board = fixture.open_board();
    // Exactly one root task, of kind reasoning, owned by the Lead.
    let ids = board.task_ids().unwrap();
    assert_eq!(ids, vec![1, 2, 3]);
    let root = board.task(1).unwrap().unwrap();
    assert_eq!(root.kind, TaskKind::Reasoning);
    assert_eq!(root.parent_task, None);
    assert_eq!(root.assignee.as_deref(), Some("lead"));
    assert_eq!(root.status, TaskStatus::Succeeded);
    assert_eq!(root.objective, "deliver the objective");
    assert_eq!(
        ids.iter()
            .filter(|id| board.task(**id).unwrap().unwrap().parent_task.is_none())
            .count(),
        1
    );
    // The delegated task is a child of the root and ran on the worker driver.
    let child = board.task(2).unwrap().unwrap();
    assert_eq!(child.parent_task, Some(1));
    assert_eq!(child.kind, TaskKind::Bulk);
    let child_attempts = board.attempts(2).unwrap();
    assert_eq!(child_attempts.len(), 1);
    assert_eq!(child_attempts[0].agent_id, "worker");
    assert_eq!(child_attempts[0].status, TaskStatus::Succeeded);
    // The utility task ran on the utility agent.
    assert_eq!(board.task(3).unwrap().unwrap().parent_task, Some(1));
    assert_eq!(board.attempts(3).unwrap()[0].agent_id, "utility");
    // The answer stored on the root is exactly what the Lead returned.
    let root_attempts = board.attempts(1).unwrap();
    assert_eq!(root_attempts.len(), 1);
    assert_eq!(
        root_attempts[0].result.as_deref(),
        Some(LEAD_ANSWER),
        "the root's attempt carries the Lead's own answer"
    );
    assert_eq!(root_attempts[0].status, TaskStatus::Succeeded);
    // The refs are the exact selection, not every successful descendant.
    let (task_refs, artifact_refs) = board.final_refs(1).unwrap();
    assert_eq!(task_refs, vec![2]);
    assert!(!task_refs.contains(&3));
    assert_eq!(artifact_refs.len(), 1);
    assert_eq!(artifact_refs[0].artifact.sha256, sha256);
    drop(board);

    // Reopening the database reproduces the answer and the exact refs.
    let reopened = fixture.open_board();
    let reconstructed = reconstruct_team_result(&reopened, 1).expect("reconstruct");
    assert_eq!(reconstructed.answer, LEAD_ANSWER);
    assert_eq!(reconstructed.task_refs, vec![2]);
    assert_eq!(reconstructed.artifact_refs, artifact_refs);
}

// A succeeded root resumes idempotently: same result, and neither the Lead's
// external thread nor any child task is touched again.
#[tokio::test]
async fn resume_on_a_succeeded_root_is_idempotent_and_replays_nothing() {
    let fixture = Fixture::new("resume-idempotent");
    let sha256 = fixture.write_worker_artifact();
    register_trio(
        &fixture,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
        ],
        "lead",
    );
    let runner = runner(&fixture);
    let first = runner
        .run("deliver the objective")
        .await
        .expect("first run");
    assert_eq!(
        fixture.lead_turns(),
        2,
        "one delegate turn and one completion"
    );

    let before: BTreeMap<u64, usize> = {
        let board = fixture.open_board();
        board
            .task_ids()
            .unwrap()
            .into_iter()
            .map(|id| (id, board.attempts(id).unwrap().len()))
            .collect()
    };

    // An idempotent resume must not depend on a runnable registry: remove the
    // worker entirely (no worker tier is left) and it still returns the
    // durable result instead of rebuilding drivers that will never run.
    Connection::open(&fixture.database)
        .unwrap()
        .execute("DELETE FROM agent_registry WHERE id = 'worker'", [])
        .unwrap();

    let resumed = runner
        .resume(first.root_task_id)
        .await
        .expect("resuming a succeeded root succeeds");

    assert_eq!(resumed.root_task_id, first.root_task_id);
    assert_eq!(resumed.lead_agent, first.lead_agent);
    assert_eq!(resumed.result.answer, first.result.answer);
    assert_eq!(resumed.result.task_refs, first.result.task_refs);
    assert_eq!(resumed.result.artifact_refs, first.result.artifact_refs);
    // The Lead's external thread was not re-driven.
    assert_eq!(fixture.lead_turns(), 2);
    let after: BTreeMap<u64, usize> = {
        let board = fixture.open_board();
        board
            .task_ids()
            .unwrap()
            .into_iter()
            .map(|id| (id, board.attempts(id).unwrap().len()))
            .collect()
    };
    assert_eq!(after, before, "resume created no task and no attempt");
}

// Resume recovers an interrupted descendant with the board's no-replay
// primitive, then continues the Lead from the durable state.
#[tokio::test]
async fn resume_recovers_interrupted_descendants_without_replaying_them() {
    let fixture = Fixture::new("resume-recovery");
    let sha256 = fixture.write_worker_artifact();
    // The new worker task created by the resumed run is 3: the root and the
    // interrupted child already occupy 1 and 2.
    register_trio(
        &fixture,
        &[
            delegate_reply(),
            complete_reply(3, Some(WORKER_ARTIFACT), &sha256),
        ],
        "lead",
    );
    {
        let mut board = fixture.open_board();
        let root = board
            .create_task(
                "recover the objective",
                None,
                TaskKind::Reasoning,
                Some("lead".into()),
            )
            .unwrap();
        board.assign(root, "lead").unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: root,
                attempt: 1,
                agent_id: "lead".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(root, TaskStatus::Running).unwrap();
        let child = board
            .create_task(
                "interrupted bulk",
                Some(root),
                TaskKind::Bulk,
                Some("worker".into()),
            )
            .unwrap();
        board.assign(child, "worker").unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: child,
                attempt: 1,
                agent_id: "worker".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(child, TaskStatus::Running).unwrap();
        assert_eq!((root, child), (1, 2));
    }

    let outcome = runner(&fixture).resume(1).await.expect("resume completes");

    assert_eq!(outcome.root_task_id, 1);
    assert_eq!(outcome.lead_agent, "lead");
    assert_eq!(outcome.result.answer, LEAD_ANSWER);
    assert_eq!(outcome.result.task_refs, vec![3]);
    let board = fixture.open_board();
    // The interrupted child was closed, never replayed.
    let child_attempts = board.attempts(2).unwrap();
    assert_eq!(child_attempts.len(), 1, "no new attempt was appended");
    assert_eq!(child_attempts[0].status, TaskStatus::Failed);
    assert!(child_attempts[0]
        .error
        .as_deref()
        .unwrap()
        .contains("interrupted before terminal driver result"));
    assert_eq!(board.task(2).unwrap().unwrap().status, TaskStatus::Failed);
    assert!(board.artifacts(2).unwrap().is_empty());
    // The root's own pre-existing attempt was settled in place, not duplicated.
    assert_eq!(board.attempts(1).unwrap().len(), 1);
    assert_eq!(
        board.attempts(1).unwrap()[0].result.as_deref(),
        Some(LEAD_ANSWER)
    );
    assert_eq!(
        board.task(1).unwrap().unwrap().status,
        TaskStatus::Succeeded
    );
    // The resumed run created its own children under the same root.
    assert_eq!(board.task(3).unwrap().unwrap().parent_task, Some(1));
    assert_eq!(board.task(4).unwrap().unwrap().parent_task, Some(1));
}

// An ambiguous Lead fails instead of guessing, and an explicit `--lead`
// resolves it.
#[tokio::test]
async fn an_ambiguous_lead_fails_and_an_explicit_lead_resolves_it() {
    let fixture = Fixture::new("ambiguous");
    let sha256 = fixture.write_worker_artifact();
    let replies = [
        delegate_reply(),
        complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
    ];
    fixture.register(&acp_agent("worker", "worker", &[WORKER_ARTIFACT]));
    fixture.register(&acp_agent("utility", "utility", &[]));
    fixture.register(&codex_agent("lead-a", &fixture, &replies));
    fixture.register(&codex_agent("lead-b", &fixture, &replies));

    let error = runner(&fixture)
        .run("deliver the objective")
        .await
        .expect_err("two reasoners are ambiguous");
    let text = error.to_string();
    assert!(text.contains("reasoner agents are registered"), "{text}");
    assert!(text.contains("lead-a") && text.contains("lead-b"), "{text}");
    assert!(
        fixture.open_board().task_ids().unwrap().is_empty(),
        "an ambiguous lead must not create a root"
    );

    let explicit = TeamRunner::new(
        &fixture.database,
        &fixture.repo,
        TeamRunOptions {
            lead_agent: Some("lead-b".into()),
            ..TeamRunOptions::default()
        },
    );
    let outcome = explicit.run("deliver the objective").await.expect("run");
    assert_eq!(outcome.lead_agent, "lead-b");
    assert_eq!(outcome.result.answer, LEAD_ANSWER);
}

// A registered Agent whose kind cannot drive an automatic run is a hard error:
// routing must never claim an agent that cannot actually run.
#[tokio::test]
async fn an_unsupported_driver_kind_fails_the_run() {
    let fixture = Fixture::new("unsupported-kind");
    let sha256 = fixture.write_worker_artifact();
    let mut native = acp_agent("worker", "worker", &[WORKER_ARTIFACT]);
    native.driver_kind = Some("native".into());
    fixture.register(&native);
    fixture.register(&acp_agent("utility", "utility", &[]));
    fixture.register(&codex_agent(
        "lead",
        &fixture,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
        ],
    ));
    let error = runner(&fixture)
        .run("deliver the objective")
        .await
        .expect_err("a native driver cannot run a team task");
    assert!(
        matches!(error, TeamRunnerError::Driver(_)),
        "unexpected error: {error}"
    );
    assert!(error
        .to_string()
        .contains("cannot drive an automatic team run"));

    // A missing driver kind is equally fatal.
    let missing = Fixture::new("missing-kind");
    let mut bare = acp_agent("worker", "worker", &[]);
    bare.driver_kind = None;
    missing.register(&bare);
    missing.register(&acp_agent("utility", "utility", &[]));
    missing.register(&codex_agent("lead", &missing, &[delegate_reply()]));
    let error = runner(&missing)
        .run("deliver the objective")
        .await
        .expect_err("a missing driver kind cannot run a team task");
    assert!(error.to_string().contains("has no driver kind"), "{error}");
}

// A Lead that never satisfies the decision contract fails the run and leaves
// the durable root observably not succeeded.
#[tokio::test]
async fn an_invalid_lead_decision_leaves_the_root_not_succeeded() {
    let fixture = Fixture::new("invalid-lead");
    fixture.write_worker_artifact();
    register_trio(
        &fixture,
        &["this is not a JSON decision".to_string()],
        "lead",
    );

    let error = runner(&fixture)
        .run("deliver the objective")
        .await
        .expect_err("an invalid decision fails the run");
    assert!(
        matches!(error, TeamRunnerError::Lead(_)),
        "unexpected error: {error}"
    );

    let board = fixture.open_board();
    let root = board.task(1).unwrap().unwrap();
    assert_eq!(root.status, TaskStatus::Failed);
    assert_ne!(root.status, TaskStatus::Succeeded);
    assert_eq!(board.task_ids().unwrap(), vec![1], "no subtask was created");
    let attempts = board.attempts(1).unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status, TaskStatus::Failed);
    assert!(attempts[0].error.is_some());
    assert!(board.final_refs(1).unwrap().0.is_empty());
}

// A Lead configuration the brain rejects fails before a root exists: a
// returned error must never leave a Running root behind.
#[tokio::test]
async fn a_configuration_error_never_leaves_a_root_behind() {
    let fixture = Fixture::new("bad-config");
    fixture.write_worker_artifact();
    fixture.register(&acp_agent("worker", "worker", &[WORKER_ARTIFACT]));
    fixture.register(&acp_agent("utility", "utility", &[]));
    let mut lead = codex_agent("lead", &fixture, &[delegate_reply()]);
    // The Lead thread cannot be driven with a prompt budget that cannot carry
    // the decision contract.
    lead.driver_config_json = Some(
        json!({
            "mcp_command": CODEX_MOCK,
            "max_prompt_bytes": 16,
        })
        .to_string(),
    );
    fixture.register(&lead);

    let error = runner(&fixture)
        .run("deliver the objective")
        .await
        .expect_err("a brain that cannot be configured fails the run");
    assert!(
        matches!(error, TeamRunnerError::LeadBrain(_)),
        "unexpected error: {error}"
    );
    assert!(
        fixture.open_board().task_ids().unwrap().is_empty(),
        "a pre-drive failure must not create a root"
    );
}

// A failed root is resumable: once the registry is corrected, the Lead is
// driven again and its pre-existing attempt row is settled in place.
#[tokio::test]
async fn resume_re_drives_a_failed_root() {
    let fixture = Fixture::new("resume-after-failure");
    let sha256 = fixture.write_worker_artifact();
    register_trio(&fixture, &["not a decision".to_string()], "lead");
    let runner = runner(&fixture);
    runner
        .run("deliver the objective")
        .await
        .expect_err("the first run fails");
    assert_eq!(
        fixture.open_board().task(1).unwrap().unwrap().status,
        TaskStatus::Failed
    );

    // Correct the Lead's configuration and resume the same durable root.
    fixture.register(&codex_agent(
        "lead",
        &fixture,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
        ],
    ));
    let outcome = runner.resume(1).await.expect("the resumed run completes");

    assert_eq!(outcome.root_task_id, 1);
    assert_eq!(outcome.result.answer, LEAD_ANSWER);
    assert_eq!(outcome.result.task_refs, vec![2]);
    let board = fixture.open_board();
    let attempts = board.attempts(1).unwrap();
    assert_eq!(
        attempts.len(),
        1,
        "the failed attempt row is settled in place"
    );
    assert_eq!(attempts[0].status, TaskStatus::Succeeded);
    assert_eq!(attempts[0].result.as_deref(), Some(LEAD_ANSWER));
    assert_eq!(
        board.task(1).unwrap().unwrap().status,
        TaskStatus::Succeeded
    );
}

// Resume refuses a root it cannot reason about.
#[tokio::test]
async fn resume_rejects_an_unknown_or_non_reasoning_root() {
    let fixture = Fixture::new("resume-rejects");
    fixture.register(&acp_agent("worker", "worker", &[]));
    fixture.register(&acp_agent("utility", "utility", &[]));
    fixture.register(&codex_agent("lead", &fixture, &[delegate_reply()]));
    let runner = runner(&fixture);
    let error = runner.resume(99).await.expect_err("unknown root");
    assert!(matches!(error, TeamRunnerError::UnknownRoot(99)));
    let bulk = {
        let mut board = fixture.open_board();
        board
            .create_task("a bulk task", None, TaskKind::Bulk, None)
            .unwrap()
    };
    let error = runner.resume(bulk).await.expect_err("not a reasoning root");
    assert!(matches!(
        error,
        TeamRunnerError::RootNotReasoning { root, .. } if root == bulk
    ));
}
