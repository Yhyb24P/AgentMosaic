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
use agentmosaic_team::{
    reconstruct_team_result, ArtifactMeta, SelectedArtifactRef, TaskAttempt, TaskBoard, TaskKind,
    TaskStatus,
};
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

/// The body of a JSON string literal for `text`, without its quotes: an exec
/// Lead reports its decision as the JSON-string `text` of one event.
#[cfg(unix)]
fn json_string_body(text: &str) -> String {
    let encoded = serde_json::to_string(text).unwrap();
    encoded[1..encoded.len() - 1].to_string()
}

/// A real Codex Exec Lead whose process is a shell script. It counts every
/// launch, continues the native thread it is asked to resume (and invents a
/// fresh one otherwise), and answers with the launch's own scripted reply.
#[cfg(unix)]
struct ExecLeadScript {
    executable: PathBuf,
    launches: PathBuf,
}

#[cfg(unix)]
impl ExecLeadScript {
    fn new(fixture: &Fixture, name: &str, replies: &[String], delay_seconds: u32) -> Self {
        let dir = fixture.root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let launches = dir.join("launches");
        let counter = dir.join("threads");
        let replies_file = dir.join("replies");
        let executable = dir.join("lead.sh");
        std::fs::write(
            &replies_file,
            replies
                .iter()
                .map(|reply| json_string_body(reply))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let script = [
            "#!/bin/sh".to_string(),
            "prompt=$(cat)".to_string(),
            "[ -n \"$prompt\" ] || exit 9".to_string(),
            format!("printf 'x' >> '{}'", launches.display()),
            format!(
                "n=0; [ -f '{}' ] && n=$(cat '{}'); n=$((n+1)); printf '%s' \"$n\" > '{}'",
                counter.display(),
                counter.display(),
                counter.display()
            ),
            format!("body=$(sed -n \"${{n}}p\" '{}')", replies_file.display()),
            "resumed=no".to_string(),
            "for a in \"$@\"; do [ \"$a\" = resume ] && resumed=yes; last=$a; done".to_string(),
            "if [ \"$resumed\" = yes ]; then tid=$last; else tid=thread-$n; fi".to_string(),
            match delay_seconds {
                0 => String::new(),
                seconds => format!("sleep {seconds}"),
            },
            "printf '%s\\n' \"{\\\"type\\\":\\\"thread.started\\\",\\\"thread_id\\\":\\\"$tid\\\"}\" \"{\\\"type\\\":\\\"item.completed\\\",\\\"item\\\":{\\\"type\\\":\\\"agent_message\\\",\\\"text\\\":\\\"$body\\\"}}\"".to_string(),
        ]
        .join("\n");
        std::fs::write(&executable, script).unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        Self {
            executable,
            launches,
        }
    }

    /// The durable registry row that drives this script as a Codex Exec Lead.
    fn record(&self, id: &str) -> AgentRegistryRecord {
        AgentRegistryRecord {
            id: id.into(),
            name: id.into(),
            tier: "reasoner".into(),
            driver_kind: Some("codex-exec".into()),
            executable: Some(self.executable.display().to_string()),
            runtime_version: None,
            driver_args_json: Some("[]".into()),
            max_concurrency: Some(1),
            tags_json: Some("[]".into()),
            driver_config_json: None,
        }
    }

    /// How many Lead processes this script actually served.
    fn launches(&self) -> usize {
        std::fs::metadata(&self.launches)
            .map(|metadata| metadata.len() as usize)
            .unwrap_or(0)
    }
}

/// Every durable row a resume must leave untouched, plus the exact board file
/// bytes. A refused resume is proven by comparing this before and after.
fn board_state(fixture: &Fixture) -> String {
    let mut dump = format!(
        "{:x}",
        Sha256::digest(std::fs::read(&fixture.database).unwrap())
    );
    let connection = Connection::open(&fixture.database).unwrap();
    for table in [
        "team_tasks",
        "team_task_runs",
        "external_runtime_bindings",
        "team_final_task_refs",
        "team_final_artifact_refs",
    ] {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM {table}"))
            .unwrap();
        let columns = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            dump.push('\n');
            for index in 0..columns {
                dump.push_str(&format!(
                    "{:?}|",
                    row.get::<_, rusqlite::types::Value>(index).unwrap()
                ));
            }
        }
    }
    dump
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

    // Inspect the input actually delivered to the external Lead, not merely
    // its scripted answer: artifact ownership survives board -> context -> JSON.
    let observation: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&fixture.state).unwrap()).unwrap();
    let prompt = observation["last_prompt"].as_str().unwrap();
    let segment = prompt
        .split_once("Current lead context (compact JSON):\n")
        .unwrap()
        .1
        .split_once("\nReply with exactly")
        .unwrap()
        .0;
    let context: serde_json::Value = serde_json::from_str(segment).unwrap();
    assert_eq!(
        context["artifacts"],
        json!([{
            "task_id": 2, "path": WORKER_ARTIFACT, "sha256": sha256,
        }])
    );

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

// A root left Running is never reclaimed implicitly: two live resumes must not
// both enter the Lead runtime. The explicit recovery primitive closes the
// interrupted attempt, and the next resume appends its own.
#[tokio::test]
async fn resume_refuses_a_running_root_until_recovery_closes_it() {
    let fixture = Fixture::new("resume-running-root");
    let sha256 = fixture.write_worker_artifact();
    register_trio(
        &fixture,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
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
    }

    let runner = runner(&fixture);
    let error = runner
        .resume(1)
        .await
        .expect_err("a running root is not reclaimed");
    assert!(
        matches!(
            error,
            TeamRunnerError::RootNotResumable {
                root: 1,
                status: TaskStatus::Running
            }
        ),
        "unexpected error: {error}"
    );
    assert!(error.to_string().contains("am recover"), "{error}");
    let board = fixture.open_board();
    assert_eq!(
        board.attempts(1).unwrap().len(),
        1,
        "no attempt was appended"
    );
    drop(board);
    assert!(!fixture.state.exists(), "no lead runtime was entered");

    // What `am recover <database> <root>` does, then resume.
    let mut board = fixture.open_board();
    board.recover_interrupted_attempt(1).unwrap();
    drop(board);
    let outcome = runner.resume(1).await.expect("resume completes");

    assert_eq!(outcome.root_task_id, 1);
    assert_eq!(outcome.lead_agent, "lead");
    assert_eq!(outcome.result.answer, LEAD_ANSWER);
    let board = fixture.open_board();
    let root_attempts = board.attempts(1).unwrap();
    assert_eq!(root_attempts.len(), 2);
    assert_eq!(root_attempts[0].status, TaskStatus::Failed);
    assert!(root_attempts[0]
        .error
        .as_deref()
        .unwrap()
        .contains("interrupted before terminal driver result"));
    assert_eq!(root_attempts[1].status, TaskStatus::Succeeded);
    assert_eq!(root_attempts[1].result.as_deref(), Some(LEAD_ANSWER));
    assert_eq!(
        board.task(1).unwrap().unwrap().status,
        TaskStatus::Succeeded
    );
}

// Resume closes an interrupted descendant with the board's no-replay
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
        // The interrupted Lead attempt was already closed by the explicit
        // recovery primitive; only the descendant is still Running.
        board
            .record_attempt(&TaskAttempt {
                task_id: root,
                attempt: 1,
                agent_id: "lead".into(),
                status: TaskStatus::Failed,
                result: None,
                error: Some("interrupted synthesis".into()),
            })
            .unwrap();
        board.set_status(root, TaskStatus::Failed).unwrap();
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
    // The root's own failed attempt is preserved; the resume appended its own.
    let root_attempts = board.attempts(1).unwrap();
    assert_eq!(root_attempts.len(), 2);
    assert_eq!(root_attempts[0].status, TaskStatus::Failed);
    assert_eq!(
        root_attempts[0].error.as_deref(),
        Some("interrupted synthesis")
    );
    assert_eq!(root_attempts[1].attempt, 2);
    assert_eq!(root_attempts[1].status, TaskStatus::Succeeded);
    assert_eq!(root_attempts[1].result.as_deref(), Some(LEAD_ANSWER));
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

// Selecting a reasoner no longer implies Codex app-server construction. The
// Lead factory dispatches on the durable runtime kind and rejects runtimes that
// have not passed the Lead contract before the root task can be created.
#[tokio::test]
async fn an_unsupported_reasoner_runtime_fails_before_root_creation() {
    let fixture = Fixture::new("unsupported-lead-runtime");
    fixture.register(&acp_agent("worker", "worker", &[]));
    fixture.register(&acp_agent("lead", "reasoner", &[]));

    let error = runner(&fixture)
        .run("deliver the objective")
        .await
        .expect_err("ACP is supported for workers, not for the Lead role");
    assert!(
        matches!(
            error,
            TeamRunnerError::UnsupportedLeadRuntime {
                ref agent,
                ref kind
            } if agent == "lead" && kind == "acp"
        ),
        "unexpected error: {error}"
    );
    assert!(
        fixture.open_board().task_ids().unwrap().is_empty(),
        "an unsupported reasoner must fail before root creation"
    );
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

/// Gate A: a failed root resumes by appending a new attempt. Attempt history
/// is append-only, so the failed attempt keeps its own error and is never
/// rewritten into a success.
#[tokio::test]
async fn failed_root_resume_appends_new_attempt() {
    let fixture = Fixture::new("resume-after-failure");
    let sha256 = fixture.write_worker_artifact();
    register_trio(&fixture, &["not a decision".to_string()], "lead");
    let runner = runner(&fixture);
    runner
        .run("deliver the objective")
        .await
        .expect_err("the first run fails");
    let board = fixture.open_board();
    assert_eq!(board.task(1).unwrap().unwrap().status, TaskStatus::Failed);
    let failed = board.attempts(1).unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].status, TaskStatus::Failed);
    let first_error = failed[0]
        .error
        .clone()
        .expect("the failed attempt kept its error");
    drop(board);

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
    assert_eq!(attempts.len(), 2, "resume appends a new attempt");
    assert_eq!(attempts[0].attempt, 1);
    assert_eq!(attempts[0].status, TaskStatus::Failed);
    assert_eq!(attempts[0].error.as_deref(), Some(first_error.as_str()));
    assert_eq!(attempts[0].result, None);
    assert_eq!(attempts[1].attempt, 2);
    assert_eq!(attempts[1].status, TaskStatus::Succeeded);
    assert_eq!(attempts[1].result.as_deref(), Some(LEAD_ANSWER));
    assert_eq!(attempts[1].agent_id, "lead");
    assert_eq!(
        board.task(1).unwrap().unwrap().status,
        TaskStatus::Succeeded
    );
}

/// Gate G: a Codex Exec root's bindings are per attempt. The failed attempt's
/// binding is preserved terminal, the resumed attempt owns its own row, and the
/// native thread is inherited from the same Lead's earlier attempt.
#[tokio::test]
#[cfg(unix)]
async fn root_exec_binding_history_is_one_row_per_attempt() {
    let fixture = Fixture::new("binding-history");
    let script = ExecLeadScript::new(
        &fixture,
        "lead",
        &[
            "this is not a JSON decision".to_string(),
            "this is not a JSON decision".to_string(),
            complete_reply(2, None, ""),
        ],
        0,
    );
    fixture.register(&acp_agent("worker", "worker", &[]));
    fixture.register(&script.record("lead"));

    // Attempt 1 really runs the external Lead and fails on its invalid
    // decision, so its binding row is written before the failure settles.
    let runner = runner(&fixture);
    runner
        .run("finish existing work")
        .await
        .expect_err("the first attempt fails on the invalid decision");

    let board = fixture.open_board();
    let first = board
        .external_binding(1, 1)
        .unwrap()
        .expect("attempt 1 binding");
    assert_eq!(first.agent_id, "lead");
    assert_eq!(first.lifecycle_state, "failed");
    assert_eq!(first.native_thread_id.as_deref(), Some("thread-1"));
    assert_eq!(board.attempts(1).unwrap()[0].status, TaskStatus::Failed);
    drop(board);

    // Work that already succeeded before the interruption. It is never re-run.
    {
        let mut board = fixture.open_board();
        let child = board
            .create_task(
                "already done",
                Some(1),
                TaskKind::Bulk,
                Some("worker".into()),
            )
            .unwrap();
        assert_eq!(child, 2);
        board.assign(child, "worker").unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: child,
                attempt: 1,
                agent_id: "worker".into(),
                status: TaskStatus::Succeeded,
                result: Some("durable work".into()),
                error: None,
            })
            .unwrap();
        board.set_status(child, TaskStatus::Succeeded).unwrap();
    }

    let outcome = runner
        .resume(1)
        .await
        .expect("the resumed attempt completes");
    assert_eq!(outcome.lead_agent, "lead");
    assert_eq!(outcome.result.task_refs, vec![2]);
    let board = fixture.open_board();
    // Attempt 1 is untouched, and attempt 2 owns the inherited native thread.
    let first_after = board.external_binding(1, 1).unwrap().unwrap();
    assert_eq!(
        first_after, first,
        "the earlier attempt's binding is preserved"
    );
    let second = board
        .external_binding(1, 2)
        .unwrap()
        .expect("attempt 2 binding");
    assert_eq!(second.agent_id, "lead");
    assert_eq!(second.lifecycle_state, "completed");
    assert_eq!(second.native_thread_id, first_after.native_thread_id);
    let attempts = board.attempts(1).unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].status, TaskStatus::Failed);
    assert_eq!(attempts[1].status, TaskStatus::Succeeded);
    // The succeeded child was never re-scheduled: one attempt, still one.
    assert_eq!(board.attempts(2).unwrap().len(), 1);
    assert_eq!(
        script.launches(),
        3,
        "one failing resolve plus its correction, then one resumed turn"
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

/// The durable shape an interruption leaves behind: a root whose Lead attempt
/// already failed, and one child whose work succeeded.
fn failed_root_with_succeeded_child(fixture: &Fixture, lead: &str) -> u64 {
    let mut board = fixture.open_board();
    let root = board
        .create_task(
            "finish existing work",
            None,
            TaskKind::Reasoning,
            Some(lead.into()),
        )
        .unwrap();
    board.assign(root, lead).unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id: root,
            attempt: 1,
            agent_id: lead.into(),
            status: TaskStatus::Failed,
            result: None,
            error: Some("interrupted synthesis".into()),
        })
        .unwrap();
    board.set_status(root, TaskStatus::Failed).unwrap();
    let child = board
        .create_task(
            "already done",
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
            status: TaskStatus::Succeeded,
            result: Some("durable work".into()),
            error: None,
        })
        .unwrap();
    board.set_status(child, TaskStatus::Succeeded).unwrap();
    assert_eq!(child, root + 1);
    root
}

/// Gate B: the durable root assignee is the canonical Lead. A registry that has
/// since become ambiguous must not change which agent a resume continues as.
#[tokio::test]
async fn resume_uses_durable_root_assignee() {
    let fixture = Fixture::new("resume-durable-assignee");
    fixture.register(&acp_agent("worker", "worker", &[]));
    fixture.register(&codex_agent(
        "lead-a",
        &fixture,
        &[complete_reply(2, None, "")],
    ));
    // A second reasoner makes a registry-only resolution ambiguous: only the
    // root's own assignee can decide which Lead resumes.
    fixture.register(&codex_agent(
        "lead-b",
        &fixture,
        &[complete_reply(2, None, "")],
    ));
    let root = failed_root_with_succeeded_child(&fixture, "lead-a");

    let outcome = runner(&fixture)
        .resume(root)
        .await
        .expect("resume continues the durable lead");

    assert_eq!(outcome.lead_agent, "lead-a");
    assert_eq!(outcome.result.task_refs, vec![2]);
    let board = fixture.open_board();
    assert_eq!(
        board.task(root).unwrap().unwrap().assignee.as_deref(),
        Some("lead-a")
    );
    let attempts = board.attempts(root).unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[1].agent_id, "lead-a");
    assert_eq!(attempts[1].status, TaskStatus::Succeeded);
}

/// The legacy branch of the canonical-Lead rule: a root with no durable
/// assignee resolves a Lead once, and the claim persists that choice.
#[tokio::test]
async fn resume_resolves_and_persists_a_legacy_root_lead() {
    let fixture = Fixture::new("resume-legacy-assignee");
    fixture.register(&acp_agent("worker", "worker", &[]));
    fixture.register(&codex_agent(
        "lead",
        &fixture,
        &[complete_reply(2, None, "")],
    ));
    let root = failed_root_with_succeeded_child(&fixture, "lead");
    Connection::open(&fixture.database)
        .unwrap()
        .execute(
            "UPDATE team_tasks SET assignee = NULL WHERE id = ?1",
            rusqlite::params![root as i64],
        )
        .unwrap();

    let outcome = runner(&fixture)
        .resume(root)
        .await
        .expect("resume resolves the legacy lead");

    assert_eq!(outcome.lead_agent, "lead");
    let board = fixture.open_board();
    assert_eq!(
        board.task(root).unwrap().unwrap().assignee.as_deref(),
        Some("lead"),
        "the resolved lead is persisted by the claim"
    );
    let attempts = board.attempts(root).unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[1].agent_id, "lead");
}

/// Gate C: an explicit `--lead` naming another agent fails before any durable
/// mutation or runtime launch. A resume never takes a root over.
#[tokio::test]
async fn resume_rejects_cross_lead_takeover_before_mutation() {
    let fixture = Fixture::new("resume-cross-lead");
    fixture.register(&acp_agent("worker", "worker", &[]));
    fixture.register(&codex_agent(
        "lead-a",
        &fixture,
        &[complete_reply(2, None, "")],
    ));
    fixture.register(&codex_agent(
        "lead-b",
        &fixture,
        &[complete_reply(2, None, "")],
    ));
    let root = failed_root_with_succeeded_child(&fixture, "lead-a");
    let before = board_state(&fixture);

    let runner = TeamRunner::new(
        &fixture.database,
        &fixture.repo,
        TeamRunOptions {
            lead_agent: Some("lead-b".into()),
            ..TeamRunOptions::default()
        },
    );
    let error = runner
        .resume(root)
        .await
        .expect_err("a resume never replaces the durable lead");
    assert!(
        matches!(error, TeamRunnerError::CrossLeadTakeover { root: 1, .. }),
        "unexpected error: {error}"
    );
    assert!(
        error.to_string().contains("lead-a") && error.to_string().contains("lead-b"),
        "{error}"
    );
    assert_eq!(
        board_state(&fixture),
        before,
        "a refused takeover mutates nothing"
    );
    assert!(!fixture.state.exists(), "no lead runtime was launched");
}

/// Gate D: the window between durable final refs and the root status is
/// reconciled from that evidence. The Lead is never re-entered, and the
/// persisted result and refs come back unchanged.
#[tokio::test]
async fn partial_final_is_reconciled_without_lead_replay() {
    let fixture = Fixture::new("partial-final");
    let sha256 = fixture.write_worker_artifact();
    fixture.register(&acp_agent("worker", "worker", &[WORKER_ARTIFACT]));
    fixture.register(&codex_agent("lead", &fixture, &[]));
    let mut board = fixture.open_board();
    let root = board
        .create_task(
            "finish the objective",
            None,
            TaskKind::Reasoning,
            Some("lead".into()),
        )
        .unwrap();
    board.assign(root, "lead").unwrap();
    let child = board
        .create_task(
            "worker artifact",
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
            status: TaskStatus::Succeeded,
            result: Some("worker result".into()),
            error: None,
        })
        .unwrap();
    board.set_status(child, TaskStatus::Succeeded).unwrap();
    let artifact = ArtifactMeta {
        path: WORKER_ARTIFACT.into(),
        sha256,
    };
    board.record_artifact(child, &artifact).unwrap();
    // Exactly the crash window: refs and a succeeded root attempt are durable,
    // but the root itself never settled.
    board
        .record_final_refs(
            root,
            &[child],
            &[SelectedArtifactRef {
                task_id: child,
                artifact: artifact.clone(),
            }],
        )
        .unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id: root,
            attempt: 1,
            agent_id: "lead".into(),
            status: TaskStatus::Succeeded,
            result: Some(LEAD_ANSWER.into()),
            error: None,
        })
        .unwrap();
    board.set_status(root, TaskStatus::Running).unwrap();
    drop(board);

    let outcome = runner(&fixture)
        .resume(root)
        .await
        .expect("the partial final reconciles");

    assert_eq!(outcome.lead_agent, "lead");
    assert_eq!(outcome.result.answer, LEAD_ANSWER);
    assert_eq!(outcome.result.task_refs, vec![child]);
    assert_eq!(outcome.result.artifact_refs.len(), 1);
    assert_eq!(outcome.result.artifact_refs[0].artifact, artifact);
    let board = fixture.open_board();
    assert_eq!(
        board.task(root).unwrap().unwrap().status,
        TaskStatus::Succeeded
    );
    let root_attempts = board.attempts(root).unwrap();
    assert_eq!(root_attempts.len(), 1, "no attempt was replayed");
    assert_eq!(root_attempts[0].result.as_deref(), Some(LEAD_ANSWER));
    let (refs, artifacts) = board.final_refs(root).unwrap();
    assert_eq!(refs, vec![child]);
    assert_eq!(artifacts.len(), 1);
    assert!(
        !fixture.state.exists(),
        "the lead runtime was never entered"
    );
}

/// Gate E: a final commit is one transaction. The injected fault rejects the
/// last step — the root's own success — so every earlier step must roll back
/// with it rather than leaving a succeeded attempt behind.
#[tokio::test]
async fn a_rejected_root_final_commit_leaves_no_partial_final() {
    let fixture = Fixture::new("atomic-final");
    let sha256 = fixture.write_worker_artifact();
    register_trio(
        &fixture,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
        ],
        "lead",
    );
    Connection::open(&fixture.database)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_root_success BEFORE UPDATE ON team_tasks
             WHEN NEW.status = 'succeeded' AND NEW.kind = 'reasoning'
             BEGIN SELECT RAISE(ABORT, 'injected root success rejection'); END;",
        )
        .unwrap();

    let error = runner(&fixture)
        .run("deliver the objective")
        .await
        .expect_err("the injected failure aborts the run");
    assert!(matches!(error, TeamRunnerError::Lead(_)), "{error}");

    let board = fixture.open_board();
    let root_attempts = board.attempts(1).unwrap();
    assert_eq!(root_attempts.len(), 1);
    assert_eq!(root_attempts[0].status, TaskStatus::Failed);
    assert!(
        root_attempts[0].result.is_none(),
        "the rejected answer is not durable"
    );
    let (refs, artifacts) = board.final_refs(1).unwrap();
    assert!(
        refs.is_empty() && artifacts.is_empty(),
        "the rejected selection rolled back with the commit"
    );
    assert_eq!(board.task(1).unwrap().unwrap().status, TaskStatus::Failed);
    // The delegated work that did succeed is untouched: only the root's own
    // final commit was rejected.
    assert_eq!(board.attempts(2).unwrap()[0].status, TaskStatus::Succeeded);
}

/// Gate F: two concurrent resumes of the same failed root have exactly one
/// claimant. The loser fails before it can enter the Lead runtime, so neither a
/// duplicate attempt nor a duplicate Lead process can appear.
#[tokio::test]
#[cfg(unix)]
async fn concurrent_resume_has_single_claimant() {
    let fixture = Fixture::new("concurrent-resume");
    // A real exec Lead that takes seconds: both resumes reach the claim while
    // the claimant is provably still running.
    let script = ExecLeadScript::new(&fixture, "lead", &[complete_reply(2, None, "")], 3);
    fixture.register(&acp_agent("worker", "worker", &[]));
    fixture.register(&script.record("lead"));
    let root = failed_root_with_succeeded_child(&fixture, "lead");

    let resume_on_its_own_thread = |database: PathBuf, repo: PathBuf| {
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let runner = TeamRunner::new(&database, &repo, TeamRunOptions::default());
            runtime.block_on(runner.resume(root))
        })
    };
    let first = resume_on_its_own_thread(fixture.database.clone(), fixture.repo.clone());
    let second = resume_on_its_own_thread(fixture.database.clone(), fixture.repo.clone());
    let outcomes = [first.join().unwrap(), second.join().unwrap()];

    let claimants = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    let refusals: Vec<&TeamRunnerError> = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().err())
        .collect();
    assert_eq!(
        claimants, 1,
        "exactly one resume may enter the lead: {outcomes:?}"
    );
    assert_eq!(refusals.len(), 1);
    assert!(
        matches!(
            refusals[0],
            TeamRunnerError::RootNotResumable { root: 1, .. }
        ),
        "{}",
        refusals[0]
    );
    let board = fixture.open_board();
    let root_attempts = board.attempts(1).unwrap();
    assert_eq!(
        root_attempts.len(),
        2,
        "only the claimant appended an attempt"
    );
    assert_eq!(root_attempts[0].status, TaskStatus::Failed);
    assert_eq!(root_attempts[1].status, TaskStatus::Succeeded);
    assert_eq!(
        board.task(1).unwrap().unwrap().status,
        TaskStatus::Succeeded
    );
    assert_eq!(script.launches(), 1, "the lead runtime was launched once");
}
