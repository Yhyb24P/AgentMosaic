//! Characterization tests for the frozen `am` surface: exit codes, stream
//! routing, and command acceptance.
//!
//! These tests deliberately do NOT freeze help/usage wording, output layout, or
//! whitespace. The v0.3 TASK01 parser rewrite and the TASK02/05/07 presentation
//! changes legitimately replace those. Only the contract that must survive the
//! rewrite is frozen here: which invocations succeed, which fail, and which
//! stream an invocation writes to.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use agentmosaic_storage::{SqliteAgentRegistry, SqliteTaskBoard};
use agentmosaic_team::{TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

fn unique_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_characterization_{name}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
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

fn run_cli_in(dir: &Path, args: &[&str]) -> Output {
    cli()
        .current_dir(dir)
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn database_of(project: &Path) -> PathBuf {
    project.join(".agentmosaic").join("state.db")
}

/// Initialize a project through the CLI itself and return its root directory.
fn initialized_project(name: &str) -> PathBuf {
    let root = unique_dir(name);
    let output = run_cli(&["init", &root.to_string_lossy()]);
    assert!(
        output.status.success(),
        "init failed: {}{}",
        stdout(&output),
        stderr(&output)
    );
    root
}

/// A one-task board with a succeeded attempt, so the read-only legacy
/// subcommands have durable state to report.
fn board_with_a_succeeded_task(name: &str) -> PathBuf {
    let root = unique_dir(name);
    let database = root.join("board.db");
    let mut board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
    let task = board
        .create_task("deliver exact result", None, TaskKind::Bulk, None)
        .unwrap();
    board.assign(task, "worker").unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id: task,
            attempt: 1,
            agent_id: "worker".into(),
            status: TaskStatus::Succeeded,
            result: Some("the durable answer".into()),
            error: None,
        })
        .unwrap();
    board.set_status(task, TaskStatus::Succeeded).unwrap();
    database
}

#[test]
fn version_prints_am_and_the_package_version() {
    let output = run_cli(&["--version"]);
    assert!(output.status.success());
    // `am <version>` is the frozen shape; the value tracks the package version,
    // so the TASK09 bump keeps this assertion honest.
    assert_eq!(
        stdout(&output),
        format!("am {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn no_arguments_is_an_error_on_stderr() {
    let output = run_cli(&[]);
    assert!(!output.status.success());
    assert!(!stderr(&output).is_empty());
}

#[test]
fn help_flags_exit_zero_with_non_empty_output() {
    for flag in ["-h", "--help"] {
        let output = run_cli(&[flag]);
        assert!(output.status.success(), "{flag} exited non-zero");
        assert!(!stdout(&output).is_empty(), "{flag} printed nothing");
    }
}

#[test]
fn init_creates_a_project() {
    let root = unique_dir("init");
    let output = run_cli(&["init", &root.to_string_lossy()]);
    assert!(output.status.success(), "init failed: {}", stderr(&output));
    assert!(!stdout(&output).is_empty());
    assert!(
        database_of(&root).is_file(),
        "init must create the state db"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn agent_add_persists_the_launch_argv_opaquely() {
    let project = initialized_project("agent_add");
    let argv = ["--acp", "--unknown-flag", "--model", "x y"];
    let mut args = vec![
        "agent",
        "add",
        "scraper",
        "--role",
        "worker",
        "--adapter",
        "acp",
        "--name",
        "Scraper",
        "--concurrency",
        "3",
        "--tag",
        "fast",
        "--artifact",
        "out/result.txt",
        "--",
        "/usr/bin/qwen",
    ];
    args.extend(argv);
    let output = run_cli_in(&project, &args);
    assert!(
        output.status.success(),
        "agent add failed: {}",
        stderr(&output)
    );

    let registry = SqliteAgentRegistry::open(database_of(&project)).unwrap();
    let agents = registry.list_agents().unwrap();
    let record = agents
        .iter()
        .find(|agent| agent.id == "scraper")
        .expect("the agent is registered");
    assert_eq!(record.name, "Scraper");
    assert_eq!(record.tier, "worker");
    assert_eq!(record.driver_kind.as_deref(), Some("acp"));
    assert_eq!(record.executable.as_deref(), Some("/usr/bin/qwen"));
    assert_eq!(record.max_concurrency, Some(3));
    assert_eq!(
        serde_json::from_str::<Vec<String>>(record.tags_json.as_deref().unwrap()).unwrap(),
        vec!["fast".to_string()]
    );
    // The launch argv survives byte-for-byte, including a flag the CLI does not
    // know and an argument containing a space.
    assert_eq!(
        record.driver_args_json.as_deref(),
        Some(serde_json::to_string(&argv).unwrap().as_str())
    );
    assert_eq!(
        serde_json::from_str::<Vec<String>>(record.driver_args_json.as_deref().unwrap()).unwrap(),
        argv.iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
    );
    let config =
        serde_json::from_str::<serde_json::Value>(record.driver_config_json.as_deref().unwrap())
            .unwrap();
    assert_eq!(config["artifact_paths"][0], "out/result.txt");

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn agent_list_succeeds() {
    let project = initialized_project("agent_list");
    let output = run_cli_in(&project, &["agent", "list"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn doctor_reports_an_unready_team_as_an_error() {
    let project = initialized_project("doctor");
    let output = run_cli_in(&project, &["doctor"]);
    assert!(!output.status.success());
    assert!(!stderr(&output).is_empty());
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn run_without_an_objective_is_an_error() {
    let project = initialized_project("run_empty");
    let output = run_cli_in(&project, &["run"]);
    assert!(!output.status.success());
    assert!(!stderr(&output).is_empty());
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn legacy_status_reads_the_board() {
    let database = board_with_a_succeeded_task("status");
    let db = database.to_string_lossy().into_owned();
    let output = run_cli(&["status", &db]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).is_empty());
    let _ = std::fs::remove_dir_all(database.parent().unwrap());
}

#[test]
fn legacy_final_prints_the_successful_result() {
    let database = board_with_a_succeeded_task("final");
    let db = database.to_string_lossy().into_owned();
    let output = run_cli(&["final", &db, "1"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stdout(&output).is_empty());
    let _ = std::fs::remove_dir_all(database.parent().unwrap());
}

#[test]
fn legacy_artifact_succeeds_even_without_artifacts() {
    let database = board_with_a_succeeded_task("artifact");
    let db = database.to_string_lossy().into_owned();
    // A task with no artifacts still succeeds; its rendering is not frozen.
    let output = run_cli(&["artifact", &db, "1"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let _ = std::fs::remove_dir_all(database.parent().unwrap());
}

#[test]
fn legacy_tui_rejects_an_unopenable_database() {
    let missing = std::env::temp_dir()
        .join("agentmosaic-characterization-missing-dir")
        .join("board.db");
    let output = run_cli(&["tui", &missing.to_string_lossy()]);
    assert!(!output.status.success());
}

#[test]
fn team_option_parsing_errors_are_rejected() {
    let root = unique_dir("team_usage");
    let database = root.join("board.db");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let db = database.to_string_lossy().into_owned();
    let repo_arg = repo.to_string_lossy().into_owned();

    // The same five malformed invocations the product surface already rejects:
    // a missing objective, a missing repository, a malformed bound, an unknown
    // flag, and a `resume-team` without a root.
    for args in [
        vec!["run-team", db.as_str(), repo_arg.as_str()],
        vec!["run-team", db.as_str()],
        vec![
            "run-team",
            db.as_str(),
            repo_arg.as_str(),
            "objective",
            "--max-rounds",
            "many",
        ],
        vec![
            "run-team",
            db.as_str(),
            repo_arg.as_str(),
            "objective",
            "--unknown",
            "x",
        ],
        vec!["resume-team", db.as_str(), repo_arg.as_str()],
    ] {
        let output = run_cli(&args);
        assert!(!output.status.success(), "{args:?} was accepted");
        assert!(
            !stderr(&output).is_empty(),
            "{args:?} said nothing on stderr"
        );
    }

    let _ = std::fs::remove_dir_all(root);
}

// TASK01 hazard: `main` opens (and therefore creates) the database at argv[1]
// before it matches the command. A valid legacy invocation must still be
// dispatched, not reported as an unknown command.
#[test]
fn legacy_command_is_not_treated_as_unknown() {
    let root = unique_dir("legacy_dispatch");
    let database = root.join("board.db");
    let db = database.to_string_lossy().into_owned();
    assert!(!database.exists());

    let submit = run_cli(&["submit", &db, "bulk", "an objective"]);
    assert!(
        submit.status.success(),
        "a valid legacy command was rejected: {}",
        stderr(&submit)
    );
    assert!(!stdout(&submit).is_empty());

    let status = run_cli(&["status", &db]);
    assert!(status.status.success(), "{}", stderr(&status));
    assert!(!stderr(&status).contains("usage:"), "dispatched as unknown");

    let _ = std::fs::remove_dir_all(root);
}

// Documents current behaviour that TASK01 must fix: matching an unknown command
// still creates a database file at argv[1], because the eager open runs before
// the command is matched. TASK01 should update this test.
#[test]
fn unknown_command_creates_the_database_file_before_matching() {
    let root = unique_dir("unknown_command");
    let database = root.join("created-by-unknown-command.db");
    assert!(!database.exists());

    let output = run_cli(&["definitely-not-a-command", &database.to_string_lossy()]);

    assert!(!output.status.success());
    assert!(!stderr(&output).is_empty());
    assert!(
        database.is_file(),
        "current behaviour creates the database before matching the command"
    );

    let _ = std::fs::remove_dir_all(root);
}
