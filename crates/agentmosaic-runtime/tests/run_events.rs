//! The team lifecycle projection, checked against the durable board.
//!
//! This is the product path: the durable registry, the SQLite board, the
//! scheduler, the Lead loop, and the runtime's scripted mock binaries. The sink
//! below opens the same database on every event and records what the board
//! already holds at that instant, which is how the ordering rule — emit after
//! the durable mutation, never inside the board lock — is proven rather than
//! assumed.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agentmosaic_runtime::{TeamRunOptions, TeamRunner, TeamRunnerError};
use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry, SqliteTaskBoard};
use agentmosaic_team::{RunEvent, RunEventSink, TaskBoard};
use rusqlite::{Connection, OptionalExtension};
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
            "agentmosaic_run_events_{name}_{}_{}",
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

fn delegate_one_worker_reply() -> String {
    json!({
        "action": "delegate",
        "tasks": [
            {"kind": "bulk", "target": "worker", "objective": "produce the worker artifact"},
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

/// What the durable board held at the instant one event was emitted.
#[derive(Debug, Clone, Default)]
struct Durable {
    task_status: Option<String>,
    attempt_status: Option<String>,
    artifact_paths: Vec<String>,
    final_task_refs: Vec<u64>,
    final_artifact_paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct Observed {
    event: RunEvent,
    durable: Durable,
}

/// Records every event together with a fresh read of the durable board, so the
/// test can assert what was already persisted when the event arrived.
struct SqliteObservingSink {
    database: PathBuf,
    seen: Mutex<Vec<Observed>>,
    read_errors: Mutex<Vec<String>>,
}

impl SqliteObservingSink {
    fn new(database: impl Into<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            database: database.into(),
            seen: Mutex::new(Vec::new()),
            read_errors: Mutex::new(Vec::new()),
        })
    }

    fn observations(&self) -> Vec<Observed> {
        self.seen.lock().unwrap().clone()
    }

    fn events(&self) -> Vec<RunEvent> {
        self.observations()
            .into_iter()
            .map(|observed| observed.event)
            .collect()
    }

    fn read_errors(&self) -> Vec<String> {
        self.read_errors.lock().unwrap().clone()
    }

    /// A read-only probe: it never starts a write transaction, so it can never
    /// contend with the board it is observing.
    fn observe(&self, event: &RunEvent) -> Result<Durable, String> {
        let connection = Connection::open(&self.database).map_err(|error| error.to_string())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let task = event_task(event);
        let mut durable = Durable {
            task_status: optional_string(
                &connection,
                "SELECT status FROM team_tasks WHERE id = ?1",
                [task as i64],
            )?,
            artifact_paths: strings(
                &connection,
                "SELECT path FROM artifacts WHERE task_id = ?1",
                [task as i64],
            )?,
            ..Durable::default()
        };
        if let Some(attempt) = event_attempt(event) {
            durable.attempt_status = optional_string(
                &connection,
                "SELECT status FROM team_task_runs WHERE task_id = ?1 AND attempt = ?2",
                rusqlite::params![task as i64, attempt as i64],
            )?;
        }
        if let RunEvent::RunCompleted { root_task_id, .. } = event {
            durable.final_task_refs = integers(
                &connection,
                "SELECT selected_task_id FROM team_final_task_refs WHERE root_task_id = ?1",
                [*root_task_id as i64],
            )?;
            durable.final_artifact_paths = strings(
                &connection,
                "SELECT path FROM team_final_artifact_refs WHERE root_task_id = ?1",
                [*root_task_id as i64],
            )?;
        }
        Ok(durable)
    }
}

impl RunEventSink for SqliteObservingSink {
    fn emit(&self, event: &RunEvent) {
        match self.observe(event) {
            Ok(durable) => self.seen.lock().unwrap().push(Observed {
                event: event.clone(),
                durable,
            }),
            Err(error) => self
                .read_errors
                .lock()
                .unwrap()
                .push(format!("{event:?}: {error}")),
        }
    }
}

fn optional_string(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Option<String>, String> {
    connection
        .query_row(sql, params, |row| row.get::<_, String>(0))
        .optional()
        .map_err(|error| error.to_string())
}

fn strings(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<String>, String> {
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params, |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

fn integers(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<u64>, String> {
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params, |row| row.get::<_, i64>(0))
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map(|values| values.into_iter().map(|value| value as u64).collect())
        .map_err(|error| error.to_string())
}

fn event_task(event: &RunEvent) -> u64 {
    match event {
        RunEvent::TaskDelegated { task_id, .. }
        | RunEvent::AttemptStarted { task_id, .. }
        | RunEvent::AttemptFailed { task_id, .. }
        | RunEvent::TaskSucceeded { task_id, .. }
        | RunEvent::TaskFailed { task_id, .. }
        | RunEvent::ArtifactRecorded { task_id, .. } => *task_id,
        RunEvent::RunStarted { root_task_id, .. }
        | RunEvent::LeadRoundStarted { root_task_id, .. }
        | RunEvent::RunCompleted { root_task_id, .. }
        | RunEvent::RunFailed { root_task_id, .. }
        | RunEvent::RunResumed { root_task_id } => *root_task_id,
    }
}

fn event_attempt(event: &RunEvent) -> Option<u32> {
    match event {
        RunEvent::AttemptStarted { attempt, .. } | RunEvent::AttemptFailed { attempt, .. } => {
            Some(*attempt)
        }
        _ => None,
    }
}

fn find(observations: &[Observed], matches: impl Fn(&RunEvent) -> bool) -> &Observed {
    observations
        .iter()
        .find(|observed| matches(&observed.event))
        .unwrap_or_else(|| {
            panic!(
                "no event matched; saw {:?}",
                observations
                    .iter()
                    .map(|observed| &observed.event)
                    .collect::<Vec<_>>()
            )
        })
}

fn index_of(observations: &[Observed], matches: impl Fn(&RunEvent) -> bool) -> usize {
    observations
        .iter()
        .position(|observed| matches(&observed.event))
        .expect("event present")
}

// The ordering rule, proven against the real SQLite board: when an event
// arrives, the durable state it reports is already there.
#[tokio::test]
async fn every_event_observes_its_durable_mutation() {
    let fixture = Fixture::new("ordering");
    let sha256 = fixture.write_worker_artifact();
    register_trio(
        &fixture,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
        ],
        "lead",
    );
    let sink = SqliteObservingSink::new(&fixture.database);
    let outcome = runner(&fixture)
        .with_sink(sink.clone())
        .run("deliver the objective")
        .await
        .expect("the team run completes");
    assert_eq!(outcome.root_task_id, 1);
    assert_eq!(
        sink.read_errors(),
        Vec::<String>::new(),
        "the sink must be able to read the board at every event"
    );

    let observations = sink.observations();
    assert_eq!(
        observations.first().map(|observed| &observed.event),
        Some(&RunEvent::RunStarted {
            root_task_id: 1,
            lead_agent: "lead".into(),
        })
    );
    assert!(matches!(
        observations.last().map(|observed| &observed.event),
        Some(RunEvent::RunCompleted { .. })
    ));

    // RunStarted: the root exists and is already Running.
    let started = find(&observations, |event| {
        matches!(event, RunEvent::RunStarted { .. })
    });
    assert_eq!(started.durable.task_status.as_deref(), Some("running"));

    // TaskDelegated: the team_tasks row exists before the event.
    for task_id in [2u64, 3] {
        let delegated = find(
            &observations,
            |event| matches!(event, RunEvent::TaskDelegated { task_id: id, .. } if *id == task_id),
        );
        assert!(
            delegated.durable.task_status.is_some(),
            "task {task_id} must already exist when it is reported as delegated"
        );
    }

    // AttemptStarted: the team_task_runs row exists and is running.
    let attempt = find(&observations, |event| {
        matches!(
            event,
            RunEvent::AttemptStarted {
                task_id: 2,
                attempt: 1,
                ..
            }
        )
    });
    assert_eq!(attempt.durable.attempt_status.as_deref(), Some("running"));

    // ArtifactRecorded: the artifact row is already committed.
    let artifact = find(&observations, |event| {
        matches!(event, RunEvent::ArtifactRecorded { task_id: 2, .. })
    });
    assert_eq!(artifact.durable.artifact_paths, vec![WORKER_ARTIFACT]);

    // TaskSucceeded: the task row is succeeded and its artifact rows are there.
    let succeeded = find(&observations, |event| {
        matches!(event, RunEvent::TaskSucceeded { task_id: 2, .. })
    });
    assert_eq!(succeeded.durable.task_status.as_deref(), Some("succeeded"));
    assert_eq!(succeeded.durable.artifact_paths, vec![WORKER_ARTIFACT]);
    match &succeeded.event {
        RunEvent::TaskSucceeded {
            agent_id,
            artifact_count,
            ..
        } => {
            assert_eq!(agent_id, "worker");
            assert_eq!(*artifact_count, succeeded.durable.artifact_paths.len());
        }
        other => panic!("unexpected event: {other:?}"),
    }

    // RunCompleted: the final refs are persisted before the event, and the
    // reported count is the durable selection, not a recollection.
    let completed = find(&observations, |event| {
        matches!(event, RunEvent::RunCompleted { .. })
    });
    assert_eq!(completed.durable.task_status.as_deref(), Some("succeeded"));
    assert_eq!(completed.durable.final_task_refs, vec![2]);
    assert_eq!(
        completed.durable.final_artifact_paths,
        vec![WORKER_ARTIFACT]
    );
    match &completed.event {
        RunEvent::RunCompleted {
            selected_task_ids,
            artifact_count,
            ..
        } => {
            assert_eq!(selected_task_ids, &completed.durable.final_task_refs);
            assert_eq!(
                *artifact_count,
                completed.durable.final_artifact_paths.len()
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

// A failing run reports the attempt, then the task, then the run — each after
// its own durable mutation.
#[tokio::test]
async fn a_failed_run_reports_the_attempt_the_task_and_the_run() {
    let fixture = Fixture::new("failure");
    // The worker's declared artifact never appears, so its driver fails at run
    // time; the Lead cannot ground a completion and the run fails too.
    fixture.register(&acp_agent("worker", "worker", &["missing.txt"]));
    fixture.register(&codex_agent(
        "lead",
        &fixture,
        &[delegate_one_worker_reply(), complete_reply(2, None, "")],
    ));
    let sink = SqliteObservingSink::new(&fixture.database);
    let error = TeamRunner::new(
        &fixture.database,
        &fixture.repo,
        TeamRunOptions {
            max_retries: 1,
            ..TeamRunOptions::default()
        },
    )
    .with_sink(sink.clone())
    .run("deliver the objective")
    .await
    .expect_err("an ungrounded completion fails the run");
    assert!(
        matches!(error, TeamRunnerError::Lead(_)),
        "unexpected error: {error}"
    );
    assert_eq!(sink.read_errors(), Vec::<String>::new());

    let observations = sink.observations();
    let failed_attempt = find(&observations, |event| {
        matches!(
            event,
            RunEvent::AttemptFailed {
                task_id: 2,
                attempt: 1,
                ..
            }
        )
    });
    assert_eq!(
        failed_attempt.durable.attempt_status.as_deref(),
        Some("failed")
    );
    let failed_task = find(&observations, |event| {
        matches!(event, RunEvent::TaskFailed { task_id: 2, .. })
    });
    assert_eq!(failed_task.durable.task_status.as_deref(), Some("failed"));
    let failed_run = find(&observations, |event| {
        matches!(
            event,
            RunEvent::RunFailed {
                root_task_id: 1,
                ..
            }
        )
    });
    assert_eq!(failed_run.durable.task_status.as_deref(), Some("failed"));

    let attempt = index_of(&observations, |event| {
        matches!(event, RunEvent::AttemptFailed { .. })
    });
    let task = index_of(&observations, |event| {
        matches!(event, RunEvent::TaskFailed { .. })
    });
    let run = index_of(&observations, |event| {
        matches!(event, RunEvent::RunFailed { .. })
    });
    assert!(attempt < task, "{:?}", sink.events());
    assert!(task < run, "{:?}", sink.events());
    assert!(
        !observations
            .iter()
            .any(|observed| matches!(observed.event, RunEvent::RunCompleted { .. })),
        "a failed run never reports completion"
    );
}

// A resume reports itself once it is about to continue; an idempotent return
// publishes nothing, because nothing happened.
#[tokio::test]
async fn a_resume_reports_itself_and_an_idempotent_return_publishes_nothing() {
    let fixture = Fixture::new("resume");
    let sha256 = fixture.write_worker_artifact();
    register_trio(&fixture, &["not a decision".to_string()], "lead");
    let runner = runner(&fixture);
    runner
        .run("deliver the objective")
        .await
        .expect_err("the first run fails");

    // Correct the Lead's configuration and resume the same durable root.
    fixture.register(&codex_agent(
        "lead",
        &fixture,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
        ],
    ));
    let sink = SqliteObservingSink::new(&fixture.database);
    let outcome = TeamRunner::new(&fixture.database, &fixture.repo, TeamRunOptions::default())
        .with_sink(sink.clone())
        .resume(1)
        .await
        .expect("the resumed run completes");
    assert_eq!(outcome.root_task_id, 1);

    let events = sink.events();
    assert_eq!(
        events.first(),
        Some(&RunEvent::RunResumed { root_task_id: 1 }),
        "{events:?}"
    );
    assert!(matches!(events.last(), Some(RunEvent::RunCompleted { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, RunEvent::RunFailed { .. })));

    // The root already succeeded: resuming again returns the durable result and
    // publishes nothing at all.
    let idle = SqliteObservingSink::new(&fixture.database);
    let again = TeamRunner::new(&fixture.database, &fixture.repo, TeamRunOptions::default())
        .with_sink(idle.clone())
        .resume(1)
        .await
        .expect("an idempotent resume succeeds");
    assert_eq!(again.result.task_refs, vec![2]);
    assert_eq!(idle.events(), Vec::<RunEvent>::new());
}

// No event carries a native runtime id: the mocks persist one on the board, and
// it must not appear anywhere in the projection.
#[tokio::test]
async fn no_event_carries_a_native_runtime_id() {
    let fixture = Fixture::new("native-ids");
    let sha256 = fixture.write_worker_artifact();
    register_trio(
        &fixture,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &sha256),
        ],
        "lead",
    );
    let sink = SqliteObservingSink::new(&fixture.database);
    runner(&fixture)
        .with_sink(sink.clone())
        .run("deliver the objective")
        .await
        .expect("the team run completes");

    let board = fixture.open_board();
    let mut native_ids = Vec::new();
    for task_id in board.task_ids().unwrap() {
        for attempt in board.attempts(task_id).unwrap() {
            let binding = board.external_binding(task_id, attempt.attempt).unwrap();
            if let Some(binding) = binding {
                if let Some(thread) = binding.native_thread_id {
                    native_ids.push(thread);
                }
            }
        }
    }
    assert!(
        !native_ids.is_empty(),
        "the mock runtimes must persist a native runtime id, or this proves nothing"
    );
    let rendered = format!("{:?}", sink.events());
    for native_id in native_ids {
        assert!(!native_id.is_empty());
        assert!(
            !rendered.contains(native_id.as_str()),
            "a native runtime id leaked into the projection: {native_id}"
        );
    }
}

/// The durable shape of a run, independent of the projection.
fn durable_projection(fixture: &Fixture) -> Vec<String> {
    let board = fixture.open_board();
    let mut rows = Vec::new();
    for id in board.task_ids().unwrap() {
        let task = board.task(id).unwrap().unwrap();
        rows.push(format!(
            "task {id} {:?} {:?} {:?} {:?}",
            task.kind, task.parent_task, task.assignee, task.status
        ));
        for attempt in board.attempts(id).unwrap() {
            rows.push(format!(
                "attempt {} {} {:?} {:?}",
                attempt.attempt, attempt.agent_id, attempt.status, attempt.result
            ));
        }
        for artifact in board.artifacts(id).unwrap() {
            rows.push(format!("artifact {} {}", artifact.path, artifact.sha256));
        }
        rows.push(format!("final {:?}", board.final_refs(id).unwrap()));
    }
    rows
}

// The default runner — no sink attached — leaves exactly the durable state it
// left before the projection existed.
#[tokio::test]
async fn the_default_sink_leaves_the_run_unchanged() {
    let plain = Fixture::new("default-plain");
    let plain_sha = plain.write_worker_artifact();
    register_trio(
        &plain,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &plain_sha),
        ],
        "lead",
    );
    let plain_outcome = runner(&plain)
        .run("deliver the objective")
        .await
        .expect("the unobserved run completes");

    let observed = Fixture::new("default-observed");
    let observed_sha = observed.write_worker_artifact();
    register_trio(
        &observed,
        &[
            delegate_reply(),
            complete_reply(2, Some(WORKER_ARTIFACT), &observed_sha),
        ],
        "lead",
    );
    let sink = SqliteObservingSink::new(&observed.database);
    let observed_outcome = runner(&observed)
        .with_sink(sink.clone())
        .run("deliver the objective")
        .await
        .expect("the observed run completes");

    assert_eq!(plain_outcome.root_task_id, observed_outcome.root_task_id);
    assert_eq!(plain_outcome.lead_agent, observed_outcome.lead_agent);
    assert_eq!(plain_outcome.result.answer, observed_outcome.result.answer);
    assert_eq!(
        plain_outcome.result.artifact_refs,
        observed_outcome.result.artifact_refs
    );
    assert_eq!(durable_projection(&plain), durable_projection(&observed));
    assert!(!sink.events().is_empty());
}
