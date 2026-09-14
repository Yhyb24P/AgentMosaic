//! The first five minutes, driven through the real binary.
//!
//! `init` teaches the next step, `agent add`/`list`/`remove` speak to the user,
//! and `doctor` answers the only question an operator has: can this project
//! run? The readiness facts are real — the product's own bounded staged probing
//! decides them — and the unready fixtures below are launch programs that do
//! not exist on any PATH.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use agentmosaic_storage::{SqliteAgentRegistry, SqliteTaskBoard};
use agentmosaic_team::{ArtifactMeta, TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

/// A launch program that is not installed anywhere: `agent list` must still
/// list it, and `doctor` must not call it ready.
const MISSING_LEAD: &str = "agentmosaic-lead-not-installed";
const MISSING_WORKER: &str = "agentmosaic-worker-not-installed";

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

fn unique_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_onboarding_ux_{name}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn run_cli(args: &[&str]) -> Output {
    cli().args(args).output().expect("the CLI runs")
}

fn run_cli_in(directory: &Path, args: &[&str]) -> Output {
    cli()
        .current_dir(directory)
        .args(args)
        .output()
        .expect("the CLI runs")
}

/// Run one command that must succeed and return its stdout.
fn ok_in(directory: &Path, args: &[&str]) -> String {
    let output = run_cli_in(directory, args);
    assert!(
        output.status.success(),
        "{args:?} failed: {}{}",
        stdout(&output),
        stderr(&output)
    );
    stdout(&output)
}

/// A project initialized through the CLI itself.
fn project(name: &str) -> PathBuf {
    let root = unique_dir(name);
    let output = run_cli(&["init", &root.to_string_lossy()]);
    assert!(output.status.success(), "init failed: {}", stderr(&output));
    root
}

fn database_of(project: &Path) -> PathBuf {
    project.join(".agentmosaic").join("state.db")
}

fn add_lead(project: &Path, program: &str) -> String {
    ok_in(
        project,
        &[
            "agent",
            "add",
            "lead",
            "--role",
            "reasoner",
            "--adapter",
            "codex-app-server",
            "--",
            program,
        ],
    )
}

fn add_worker(project: &Path, id: &str, program: &str) -> String {
    ok_in(
        project,
        &[
            "agent",
            "add",
            id,
            "--role",
            "worker",
            "--adapter",
            "acp",
            "--",
            program,
            "--acp",
        ],
    )
}

/// The cargo target directory, as seen by this test binary.
fn target_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_am"))
        .parent()
        .expect("the CLI binary lives in the cargo target directory")
        .to_path_buf()
}

/// Locate one of the runtime's mock binaries. `cargo test --workspace` builds
/// every workspace member's binaries next to the CLI binary; a filtered run can
/// leave one out, so build the runtime binaries once and retry instead of
/// failing flakily.
fn mock_binary(name: &str) -> PathBuf {
    let candidate = target_dir().join(name);
    if candidate.is_file() {
        return candidate;
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "-p", "agentmosaic-runtime", "--bins"])
        .status()
        .expect("cargo is runnable");
    assert!(
        status.success(),
        "building the runtime mock binaries failed"
    );
    assert!(
        candidate.is_file(),
        "{} was not built at {}",
        name,
        candidate.display()
    );
    candidate
}

#[test]
fn init_teaches_lead_and_worker_without_naming_the_state_path() {
    let root = unique_dir("init");
    let output = run_cli(&["init", &root.to_string_lossy()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("initialized AgentMosaic"), "{text}");
    assert!(text.contains("project  "), "{text}");
    assert!(text.contains("Lead"), "{text}");
    assert!(text.contains("Worker"), "{text}");
    assert!(
        text.contains("am agent add lead --role reasoner --adapter codex-app-server -- codex"),
        "{text}"
    );
    assert!(
        text.contains("am agent add worker --role worker --adapter acp -- qwen --acp"),
        "{text}"
    );
    assert!(text.contains("am doctor"), "{text}");
    // The durable state exists, but the surface never names it.
    assert!(database_of(&root).is_file());
    assert!(!text.contains("state.db"), "{text}");
    assert!(!text.contains(".agentmosaic"), "{text}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn init_again_says_already_initialized_and_keeps_the_registry() {
    let root = project("init_again");
    add_lead(&root, MISSING_LEAD);

    let output = run_cli(&["init", &root.to_string_lossy()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert_eq!(
        text.lines().next(),
        Some("already initialized AgentMosaic"),
        "{text}"
    );
    assert!(!text.contains("state.db"), "{text}");
    assert!(!text.contains(".agentmosaic"), "{text}");
    // The next step follows the registry the project actually has, and the
    // registration itself survived: no state was recreated.
    assert!(text.contains("Next: add a Worker."), "{text}");
    assert!(stdout(&run_cli_in(&root, &["agent", "list"])).contains("lead"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_add_reports_registered_then_updated() {
    let root = project("agent_add");
    let first = add_worker(&root, "worker", "qwen");
    assert!(first.contains("registered agent `worker`"), "{first}");
    assert!(first.contains("role     worker"), "{first}");
    assert!(first.contains("adapter  acp"), "{first}");
    assert!(first.contains("launch   qwen --acp"), "{first}");
    assert!(first.contains("Next: am doctor"), "{first}");

    let second = add_worker(&root, "worker", "qwen");
    assert!(second.contains("updated agent `worker`"), "{second}");
    assert!(!second.contains("registered agent"), "{second}");

    // Upsert, not append: the registry still holds one row.
    let registry = SqliteAgentRegistry::open(database_of(&root)).unwrap();
    assert_eq!(registry.list_agents().unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_add_bounds_and_redacts_the_launch_argv() {
    let root = project("agent_add_bound");
    let added = ok_in(
        &root,
        &[
            "agent",
            "add",
            "wrapper",
            "--role",
            "worker",
            "--adapter",
            "acp",
            "--",
            "wrapper-not-installed",
            "--token=super-secret-value",
            "-ds",
        ],
    );
    let launch = added
        .lines()
        .find(|line| line.starts_with("launch"))
        .expect("the launch field is shown");
    assert!(!added.contains("super-secret-value"), "{added}");
    assert!(launch.contains("<redacted>"), "{added}");
    assert!(launch.contains("-ds"), "{added}");

    let long = "x".repeat(400);
    let bounded = ok_in(
        &root,
        &[
            "agent",
            "add",
            "long",
            "--role",
            "worker",
            "--adapter",
            "acp",
            "--",
            "wrapper",
            &long,
        ],
    );
    let launch = bounded
        .lines()
        .find(|line| line.starts_with("launch"))
        .expect("the launch field is shown");
    assert!(launch.ends_with("..."), "{bounded}");
    assert!(!bounded.contains(&long), "{bounded}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_list_is_role_first_and_never_probes_a_runtime() {
    let root = project("agent_list");
    // The Worker is registered first and `a-worker` also sorts before `lead`,
    // so id order would put it first: only role-first ordering can explain the
    // listing below.
    add_worker(&root, "a-worker", MISSING_WORKER);
    add_lead(&root, MISSING_LEAD);

    let listed = run_cli_in(&root, &["agent", "list"]);
    assert!(
        listed.status.success(),
        "listing must not probe a runtime: {}",
        stderr(&listed)
    );
    let text = stdout(&listed);
    assert!(text.starts_with("ID"), "{text}");
    for column in ["ROLE", "ADAPTER", "LAUNCH"] {
        assert!(text.contains(column), "{text}");
    }
    let label_row = |label: &str| {
        text.lines()
            .position(|line| line.starts_with(label))
            .unwrap_or_else(|| panic!("`{label}` is listed: {text}"))
    };
    assert!(text.contains("a-worker"), "{text}");
    assert!(text.contains(MISSING_WORKER), "{text}");
    assert!(text.contains(MISSING_LEAD), "{text}");
    assert!(
        label_row("lead") < label_row("a-worker"),
        "role-first ordering: {text}"
    );

    // The contrast: the same registry cannot be called ready.
    let doctor = run_cli_in(&root, &["doctor"]);
    assert!(!doctor.status.success());
    assert!(stderr(&doctor).contains("not ready"), "{}", stderr(&doctor));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_remove_keeps_the_durable_history() {
    let root = project("agent_remove_history");
    add_lead(&root, MISSING_LEAD);
    add_worker(&root, "worker", MISSING_WORKER);
    let task_id;
    {
        let mut board =
            SqliteTaskBoard::open(Connection::open(database_of(&root)).unwrap()).unwrap();
        // A run the Lead drove, with the worker's task below it.
        let run = board
            .create_task("historic run", None, TaskKind::Reasoning, None)
            .unwrap();
        board.assign(run, "lead").unwrap();
        let task = board
            .create_task("historic objective", Some(run), TaskKind::Bulk, None)
            .unwrap();
        task_id = task;
        board.assign(task, "worker").unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: task,
                attempt: 1,
                agent_id: "worker".into(),
                status: TaskStatus::Succeeded,
                result: Some("historic result".into()),
                error: None,
            })
            .unwrap();
        board.set_status(task, TaskStatus::Succeeded).unwrap();
        board.set_status(run, TaskStatus::Succeeded).unwrap();
        board
            .record_artifact(
                task,
                &ArtifactMeta {
                    path: "out/result.txt".into(),
                    sha256: "a".repeat(64),
                },
            )
            .unwrap();
    }

    let removed = ok_in(&root, &["agent", "remove", "worker"]);
    assert!(removed.contains("removed agent `worker`"), "{removed}");
    assert!(SqliteAgentRegistry::open(database_of(&root))
        .unwrap()
        .get_agent("worker")
        .unwrap()
        .is_none());

    // History is not registration: the task, its attempt and its artifact stay.
    let board = SqliteTaskBoard::open(Connection::open(database_of(&root)).unwrap()).unwrap();
    let task = board
        .task(task_id)
        .unwrap()
        .expect("the historic task survives the removal");
    assert_eq!(task.objective, "historic objective");
    assert_eq!(task.status, TaskStatus::Succeeded);
    assert_eq!(task.assignee.as_deref(), Some("worker"));
    assert_eq!(board.attempts(task.id).unwrap().len(), 1);
    assert_eq!(board.artifacts(task.id).unwrap().len(), 1);
    let status = ok_in(&root, &["status"]);
    assert!(status.contains("historic objective"), "{status}");
    assert!(status.contains("out/result.txt"), "{status}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_remove_warns_when_the_team_is_no_longer_runnable() {
    let root = project("agent_remove_warn");
    add_lead(&root, MISSING_LEAD);
    add_worker(&root, "worker", MISSING_WORKER);
    ok_in(
        &root,
        &[
            "agent",
            "add",
            "utility",
            "--role",
            "utility",
            "--adapter",
            "acp",
            "--",
            MISSING_WORKER,
        ],
    );

    // Removing an optional utility Agent leaves the team intact.
    let optional = ok_in(&root, &["agent", "remove", "utility"]);
    assert!(optional.contains("removed agent `utility`"), "{optional}");
    assert!(!optional.contains("am doctor will fail"), "{optional}");

    // Removing the only worker does not.
    let required = ok_in(&root, &["agent", "remove", "worker"]);
    assert!(required.contains("removed agent `worker`"), "{required}");
    assert!(required.contains("no longer runnable"), "{required}");
    assert!(required.contains("am doctor will fail"), "{required}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_remove_of_a_missing_id_fails_and_names_it() {
    let root = project("agent_remove_missing");
    let output = run_cli_in(&root, &["agent", "remove", "ghost"]);
    assert!(!output.status.success());
    assert!(stdout(&output).is_empty());
    let message = stderr(&output);
    assert!(message.contains("ghost"), "{message}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn doctor_default_is_a_decision_and_verbose_keeps_the_stages() {
    let root = project("doctor_unready");
    add_lead(&root, MISSING_LEAD);
    add_worker(&root, "worker", MISSING_WORKER);

    let output = run_cli_in(&root, &["doctor"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "the report belongs on stderr");
    let report = stderr(&output);
    assert!(report.contains("project   ready"), "{report}");
    assert!(report.contains("lead      not ready"), "{report}");
    assert!(report.contains("worker    not ready"), "{report}");
    assert!(
        report.contains("team      not ready  1 lead · 1 worker"),
        "{report}"
    );
    assert!(report.contains("\nReason\n  "), "{report}");
    assert!(report.contains("\nFix\n  "), "{report}");
    assert!(report.contains("am doctor"), "{report}");
    assert!(!report.contains("PROGRAM_"), "{report}");
    assert!(!report.contains("SESSION_OK"), "{report}");

    let verbose = run_cli_in(&root, &["doctor", "--verbose"]);
    assert!(!verbose.status.success());
    let stages = stderr(&verbose);
    assert!(stages.contains("PROGRAM_NOT_FOUND"), "{stages}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn doctor_without_a_lead_explains_the_missing_role() {
    let root = project("doctor_missing_role");
    let output = run_cli_in(&root, &["doctor"]);
    assert!(!output.status.success());
    let report = stderr(&output);
    assert!(
        report.contains("team      not ready  0 lead · 0 worker"),
        "{report}"
    );
    assert!(report.contains("no lead"), "{report}");
    assert!(report.contains("\nReason\n"), "{report}");
    assert!(report.contains("\nFix\n"), "{report}");
    fs::remove_dir_all(root).unwrap();
}

/// The success path is a real team of the workspace's own deterministic mock
/// runtimes: the `codex-app-server` Lead is `codex_bridge_mock` and the ACP
/// Worker is `acp_m2_mock`, the same fixtures the product's team tests drive.
/// Nothing here fakes readiness by pointing at an unrelated program.
#[test]
fn doctor_reports_a_ready_team_on_stdout() {
    let root = project("doctor_ready");
    let codex = mock_binary("codex_bridge_mock");
    let acp = mock_binary("acp_m2_mock");
    add_lead(&root, &codex.display().to_string());
    // The ACP mock takes no launch arguments; `--acp` belongs to the real
    // runtime's own argv and the mock would refuse it.
    ok_in(
        &root,
        &[
            "agent",
            "add",
            "worker",
            "--role",
            "worker",
            "--adapter",
            "acp",
            "--",
            &acp.display().to_string(),
        ],
    );

    let output = run_cli_in(&root, &["doctor"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
    let report = stdout(&output);
    assert!(report.contains("project   ready"), "{report}");
    assert!(report.contains("lead      ready"), "{report}");
    assert!(report.contains("worker    ready"), "{report}");
    assert!(
        report.contains("team      ready  1 lead · 1 worker"),
        "{report}"
    );
    assert!(report.contains("Ready to run."), "{report}");
    assert!(report.contains("am run \"<objective>\""), "{report}");
    assert!(!report.contains("Reason"), "{report}");
    fs::remove_dir_all(root).unwrap();
}
