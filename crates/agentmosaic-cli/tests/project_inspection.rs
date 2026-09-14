//! The project-aware inspection surface.
//!
//! `status`, `final`, `artifact` and `tui` discover `<project>/.agentmosaic/
//! state.db` the way `am run` and `am doctor` do, and speak in runs (root
//! reasoning tasks) and tasks. The legacy positional forms keep their exact
//! historical output.
//!
//! TUI project resolution is covered in two places: the `target` module's unit
//! tests exercise the resolution rule itself as a pure function, and the
//! no-project test below proves the binary stops with the remediation instead
//! of opening anything.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use agentmosaic_storage::SqliteTaskBoard;
use agentmosaic_team::{ArtifactMeta, TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

fn unique_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_project_inspection_{name}_{}_{}",
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

fn run_cli_in(dir: &Path, args: &[&str]) -> Output {
    cli()
        .current_dir(dir)
        .args(args)
        .output()
        .expect("the CLI runs")
}

/// Initialize a project through the CLI itself and return its root directory.
fn project(name: &str) -> PathBuf {
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

fn database_of(project: &Path) -> PathBuf {
    project.join(".agentmosaic").join("state.db")
}

fn open_board(project: &Path) -> SqliteTaskBoard {
    SqliteTaskBoard::open(Connection::open(database_of(project)).unwrap()).unwrap()
}

/// A run is a root reasoning task.
fn create_run(board: &mut SqliteTaskBoard, objective: &str, lead: &str) -> u64 {
    let root = board
        .create_task(objective, None, TaskKind::Reasoning, None)
        .unwrap();
    board.assign(root, lead).unwrap();
    root
}

fn add_child(board: &mut SqliteTaskBoard, root: u64, objective: &str, agent: &str) -> u64 {
    let child = board
        .create_task(objective, Some(root), TaskKind::Bulk, None)
        .unwrap();
    board.assign(child, agent).unwrap();
    child
}

fn succeed(board: &mut SqliteTaskBoard, task: u64, agent: &str, result: &str) {
    board
        .record_attempt(&TaskAttempt {
            task_id: task,
            attempt: 1,
            agent_id: agent.into(),
            status: TaskStatus::Succeeded,
            result: Some(result.into()),
            error: None,
        })
        .unwrap();
    board.set_status(task, TaskStatus::Succeeded).unwrap();
}

fn fail(board: &mut SqliteTaskBoard, task: u64, agent: &str, error: &str) {
    board
        .record_attempt(&TaskAttempt {
            task_id: task,
            attempt: 1,
            agent_id: agent.into(),
            status: TaskStatus::Failed,
            result: None,
            error: Some(error.into()),
        })
        .unwrap();
    board.set_status(task, TaskStatus::Failed).unwrap();
}

fn add_artifact(board: &mut SqliteTaskBoard, task: u64, path: &str) {
    board
        .record_artifact(
            task,
            &ArtifactMeta {
                path: path.into(),
                sha256: "a".repeat(64),
            },
        )
        .unwrap();
}

/// Every `#<id>` the run rendering mentions: `run #7`, `  #8  worker  ...`, and
/// `  #9  path`.
fn mentioned_ids(text: &str) -> BTreeSet<u64> {
    let mut ids = BTreeSet::new();
    for line in text.lines() {
        let mut rest = line;
        while let Some(index) = rest.find('#') {
            rest = &rest[index + 1..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                continue;
            }
            if let Ok(id) = digits.parse() {
                ids.insert(id);
            }
            rest = &rest[digits.len()..];
        }
    }
    ids
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_discovery_walks_up_from_a_subdirectory() {
    let root = project("nested_discovery");
    let run = {
        let mut board = open_board(&root);
        let run = create_run(&mut board, "deliver the parser", "lead");
        let child = add_child(&mut board, run, "implement the parser", "worker");
        succeed(&mut board, child, "worker", "parser implemented");
        succeed(&mut board, run, "lead", "the durable answer");
        run
    };

    let from_root = run_cli_in(&root, &["status"]);
    assert!(from_root.status.success(), "{}", stderr(&from_root));

    let nested = root.join("deeply").join("nested").join("worktree");
    fs::create_dir_all(&nested).unwrap();
    let from_nested = run_cli_in(&nested, &["status"]);
    assert!(from_nested.status.success(), "{}", stderr(&from_nested));

    // The subdirectory sees exactly the same run.
    assert_eq!(stdout(&from_root), stdout(&from_nested));
    let text = stdout(&from_root);
    assert!(text.contains(&format!("run #{run}  succeeded")), "{text}");
    assert!(text.contains("objective  deliver the parser"), "{text}");

    // The same discovery feeds the other project-aware forms.
    let final_result = run_cli_in(&nested, &["final"]);
    assert!(final_result.status.success(), "{}", stderr(&final_result));
    assert_eq!(stdout(&final_result), "the durable answer\n");
    cleanup(&root);
}

#[test]
fn outside_a_project_the_normal_commands_report_the_remediation() {
    let root = unique_dir("no_project");

    let status = run_cli_in(&root, &["status"]);
    assert!(!status.status.success());
    assert!(
        stderr(&status).contains("no initialized AgentMosaic project found"),
        "{}",
        stderr(&status)
    );
    assert!(stderr(&status).contains("am init"), "{}", stderr(&status));

    for args in [&["status", "--all"][..], &["final"][..], &["artifact"][..]] {
        let output = run_cli_in(&root, args);
        assert!(
            !output.status.success(),
            "{args:?} succeeded outside a project"
        );
        assert!(
            stderr(&output).contains("no initialized AgentMosaic project found"),
            "{args:?}: {}",
            stderr(&output)
        );
    }

    // TUI resolves its database the same way, and stops at the remediation
    // instead of opening anything.
    let tui = run_cli_in(&root, &["tui"]);
    assert!(!tui.status.success());
    assert!(
        stderr(&tui).contains("no initialized AgentMosaic project found"),
        "{}",
        stderr(&tui)
    );
    cleanup(&root);
}

/// A project with no run yet is a normal state, not an error: inspection says
/// so, while the commands that need a run refuse by name.
#[test]
fn a_fresh_project_reports_that_it_has_no_runs() {
    let root = project("fresh_project");

    for args in [&["status"][..], &["status", "--all"][..]] {
        let output = run_cli_in(&root, args);
        assert!(output.status.success(), "{args:?}: {}", stderr(&output));
        assert!(
            stdout(&output).contains("no runs in this project yet"),
            "{args:?}: {}",
            stdout(&output)
        );
    }

    for args in [&["final"][..], &["artifact"][..]] {
        let output = run_cli_in(&root, args);
        assert!(!output.status.success(), "{args:?} succeeded");
        assert!(
            stderr(&output).contains("no runs in this project yet"),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    cleanup(&root);
}

#[test]
fn the_newest_run_wins_even_when_it_failed() {
    let root = project("newest_run");
    let (first, second) = {
        let mut board = open_board(&root);
        let first = create_run(&mut board, "first objective", "lead");
        succeed(&mut board, first, "lead", "the first answer");
        let second = create_run(&mut board, "second objective", "lead");
        fail(&mut board, second, "lead", "the lead could not answer");
        (first, second)
    };

    let status = run_cli_in(&root, &["status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let text = stdout(&status);
    assert!(text.contains(&format!("run #{second}  failed")), "{text}");
    assert_eq!(mentioned_ids(&text), BTreeSet::from([second]));

    // A failed newest run is never skipped for an older successful one.
    let final_result = run_cli_in(&root, &["final"]);
    assert!(!final_result.status.success());
    let message = stderr(&final_result);
    assert!(message.contains(&format!("run #{second}")), "{message}");
    assert!(message.contains("no final answer"), "{message}");
    assert!(message.contains("am status"), "{message}");
    assert!(!stdout(&final_result).contains("the first answer"));

    // Naming the older run still reads its durable answer.
    let older = run_cli_in(&root, &["final", &first.to_string()]);
    assert!(older.status.success(), "{}", stderr(&older));
    assert_eq!(stdout(&older), "the first answer\n");
    cleanup(&root);
}

#[test]
fn a_still_running_run_is_the_latest_run_and_has_no_final() {
    let root = project("running_run");
    let (first, second, child) = {
        let mut board = open_board(&root);
        let first = create_run(&mut board, "older objective", "lead");
        succeed(&mut board, first, "lead", "the older answer");
        let second = create_run(&mut board, "in flight objective", "lead");
        let child = add_child(&mut board, second, "long worker task", "worker");
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
        board.set_status(second, TaskStatus::Running).unwrap();
        (first, second, child)
    };

    let status = run_cli_in(&root, &["status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let text = stdout(&status);
    assert!(text.contains(&format!("run #{second}  running")), "{text}");
    assert!(
        text.contains(&format!("  #{child}  worker  running  long worker task")),
        "{text}"
    );
    assert_eq!(mentioned_ids(&text), BTreeSet::from([second, child]));
    assert!(!text.contains(&format!("#{first}")));

    let final_result = run_cli_in(&root, &["final"]);
    assert!(!final_result.status.success());
    let message = stderr(&final_result);
    assert!(
        message.contains(&format!("run #{second} is running")),
        "{message}"
    );
    assert!(message.contains("no final answer"), "{message}");
    cleanup(&root);
}

#[test]
fn status_all_lists_every_run_newest_first() {
    let root = project("status_all");
    let (first, second, third) = {
        let mut board = open_board(&root);
        let first = create_run(&mut board, "first objective", "lead");
        succeed(&mut board, first, "lead", "first answer");
        let second = create_run(&mut board, "second objective", "lead");
        fail(&mut board, second, "lead", "no answer");
        let third = create_run(&mut board, "third objective", "lead");
        board.set_status(third, TaskStatus::Running).unwrap();
        (first, second, third)
    };

    let output = run_cli_in(&root, &["status", "--all"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let lines: Vec<String> = stdout(&output).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines[0].starts_with(&format!("run #{third}  running")),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with(&format!("run #{second}  failed")),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with(&format!("run #{first}  succeeded")),
        "{}",
        lines[2]
    );

    // The listing is deterministic.
    let again = run_cli_in(&root, &["status", "--all"]);
    assert_eq!(stdout(&output), stdout(&again));
    cleanup(&root);
}

#[test]
fn run_status_shows_only_the_subtree_of_the_selected_run() {
    let root = project("subtree_isolation");
    let (first, first_child, second, second_child) = {
        let mut board = open_board(&root);
        let first = create_run(&mut board, "first run objective", "lead");
        let first_child = add_child(&mut board, first, "first run child", "worker");
        add_artifact(&mut board, first_child, "first.txt");
        succeed(&mut board, first_child, "worker", "first child done");
        succeed(&mut board, first, "lead", "first answer");

        let second = create_run(&mut board, "second run objective", "lead");
        let second_child = add_child(&mut board, second, "second run child", "worker");
        add_artifact(&mut board, second_child, "second.txt");
        succeed(&mut board, second_child, "worker", "second child done");
        succeed(&mut board, second, "lead", "second answer");
        (first, first_child, second, second_child)
    };

    let first_status = run_cli_in(&root, &["status", &first.to_string()]);
    assert!(first_status.status.success(), "{}", stderr(&first_status));
    let first_text = stdout(&first_status);
    assert_eq!(
        mentioned_ids(&first_text),
        BTreeSet::from([first, first_child])
    );
    assert!(first_text.contains("first run child"), "{first_text}");
    assert!(!first_text.contains("second run child"));

    let second_status = run_cli_in(&root, &["status", &second.to_string()]);
    assert!(second_status.status.success(), "{}", stderr(&second_status));
    let second_text = stdout(&second_status);
    assert_eq!(
        mentioned_ids(&second_text),
        BTreeSet::from([second, second_child])
    );
    assert!(!second_text.contains("first run child"));
    assert!(!second_text.contains(&format!("#{second_child}  first.txt")));
    cleanup(&root);
}

#[test]
fn a_task_that_is_not_a_run_is_rejected_by_name() {
    let root = project("not_a_run");
    let (_, child) = {
        let mut board = open_board(&root);
        let run = create_run(&mut board, "the only run", "lead");
        let child = add_child(&mut board, run, "a child task", "worker");
        (run, child)
    };

    let status = run_cli_in(&root, &["status", &child.to_string()]);
    assert!(!status.status.success());
    assert!(stdout(&status).is_empty());
    assert!(
        stderr(&status).contains(&format!("task #{child} is not a run")),
        "{}",
        stderr(&status)
    );

    let final_result = run_cli_in(&root, &["final", &child.to_string()]);
    assert!(!final_result.status.success());
    assert!(
        stderr(&final_result).contains(&format!("task #{child} is not a run")),
        "{}",
        stderr(&final_result)
    );

    let missing = run_cli_in(&root, &["status", "9999"]);
    assert!(!missing.status.success());
    assert!(
        stderr(&missing).contains("no task #9999"),
        "{}",
        stderr(&missing)
    );
    cleanup(&root);
}

#[test]
fn the_legacy_forms_keep_their_exact_output() {
    let root = unique_dir("legacy_forms");
    let database = root.join("board.db");
    {
        let mut board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
        let task = board
            .create_task("legacy objective", None, TaskKind::Bulk, None)
            .unwrap();
        board.assign(task, "worker").unwrap();
        succeed(&mut board, task, "worker", "legacy answer");
        add_artifact(&mut board, task, "legacy.txt");
    }
    let db = database.to_string_lossy().into_owned();

    // `status <database>` is still the whole board, one `task=` line per row.
    let status = run_cli(&["status", &db]);
    assert!(status.status.success(), "{}", stderr(&status));
    let text = stdout(&status);
    assert_eq!(
        text,
        "task=1 status=succeeded assignee=worker attempts=1 parent=- objective=legacy objective\n"
    );
    assert!(text.lines().all(|line| line.starts_with("task=")), "{text}");
    assert!(!text.contains("run #"));

    let final_result = run_cli(&["final", &db, "1"]);
    assert!(final_result.status.success(), "{}", stderr(&final_result));
    assert_eq!(stdout(&final_result), "legacy answer\n");

    let artifact = run_cli(&["artifact", &db, "1"]);
    assert!(artifact.status.success(), "{}", stderr(&artifact));
    assert_eq!(
        stdout(&artifact),
        format!("task=1 path=legacy.txt sha256={}\n", "a".repeat(64))
    );

    let missing = root.join("missing").join("board.db");
    let tui = run_cli(&["tui", &missing.to_string_lossy()]);
    assert!(!tui.status.success());
    cleanup(&root);
}

#[test]
fn a_bare_number_that_is_also_a_file_is_refused_in_and_out_of_a_project() {
    let outside = unique_dir("ambiguous_outside");
    fs::write(outside.join("7"), "not a database").unwrap();
    for args in [
        ["status", "7"],
        ["final", "7"],
        ["artifact", "7"],
        ["tui", "7"],
    ] {
        let output = run_cli_in(&outside, &args);
        assert!(!output.status.success(), "{args:?} was accepted");
        assert!(
            stderr(&output).contains("ambiguous target `7`"),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    cleanup(&outside);

    let root = project("ambiguous_inside");
    fs::write(root.join("9"), "not a database").unwrap();
    for args in [
        ["status", "9"],
        ["final", "9"],
        ["artifact", "9"],
        ["tui", "9"],
    ] {
        let output = run_cli_in(&root, &args);
        assert!(!output.status.success(), "{args:?} was accepted");
        assert!(
            stderr(&output).contains("ambiguous target `9`"),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    cleanup(&root);
}

#[test]
fn artifacts_are_scoped_to_the_latest_run_and_to_one_task() {
    let root = project("artifact_scope");
    let (first_child, second, second_child, second_grandchild) = {
        let mut board = open_board(&root);
        let first = create_run(&mut board, "first run objective", "lead");
        let first_child = add_child(&mut board, first, "first run child", "worker");
        add_artifact(&mut board, first_child, "old.txt");
        succeed(&mut board, first_child, "worker", "first done");
        succeed(&mut board, first, "lead", "first answer");

        let second = create_run(&mut board, "second run objective", "lead");
        add_artifact(&mut board, second, "summary.txt");
        let second_child = add_child(&mut board, second, "second run child", "worker");
        add_artifact(&mut board, second_child, "new.txt");
        let second_grandchild =
            add_child(&mut board, second_child, "second run grandchild", "worker");
        add_artifact(&mut board, second_grandchild, "deep.txt");
        succeed(&mut board, second_grandchild, "worker", "grandchild done");
        succeed(&mut board, second_child, "worker", "child done");
        succeed(&mut board, second, "lead", "second answer");
        (first_child, second, second_child, second_grandchild)
    };

    // No target: every artifact recorded anywhere in the latest run.
    let latest = run_cli_in(&root, &["artifact"]);
    assert!(latest.status.success(), "{}", stderr(&latest));
    let text = stdout(&latest);
    let sha = "a".repeat(64);
    assert_eq!(
        text,
        format!(
            "task={second} path=summary.txt sha256={sha}\n\
             task={second_child} path=new.txt sha256={sha}\n\
             task={second_grandchild} path=deep.txt sha256={sha}\n"
        )
    );
    assert!(!text.contains("old.txt"));

    // One task: only that task's artifacts.
    let one = run_cli_in(&root, &["artifact", &second_child.to_string()]);
    assert!(one.status.success(), "{}", stderr(&one));
    assert_eq!(
        stdout(&one),
        format!("task={second_child} path=new.txt sha256={sha}\n")
    );

    // The other run's artifacts stay reachable through the task itself.
    let older = run_cli_in(&root, &["artifact", &first_child.to_string()]);
    assert!(older.status.success(), "{}", stderr(&older));
    assert_eq!(
        stdout(&older),
        format!("task={first_child} path=old.txt sha256={sha}\n")
    );
    cleanup(&root);
}

#[test]
fn the_run_rendering_never_prints_the_database_path() {
    let root = project("no_database_path");
    let (run, child) = {
        let mut board = open_board(&root);
        let run = create_run(&mut board, "an objective", "lead");
        let child = add_child(&mut board, run, "a child objective", "worker");
        add_artifact(&mut board, child, "report.txt");
        succeed(&mut board, child, "worker", "child done");
        succeed(&mut board, run, "lead", "the durable answer");
        (run, child)
    };

    let mut payloads = vec![
        stdout(&run_cli_in(&root, &["status"])),
        stdout(&run_cli_in(&root, &["status", "--all"])),
        stdout(&run_cli_in(&root, &["status", &run.to_string()])),
        stdout(&run_cli_in(&root, &["final"])),
        stdout(&run_cli_in(&root, &["artifact"])),
        stdout(&run_cli_in(&root, &["artifact", &child.to_string()])),
    ];
    assert!(payloads.iter().all(|payload| !payload.is_empty()));
    payloads.push(stderr(&run_cli_in(&root, &["status", "9999"])));

    for payload in payloads {
        assert!(!payload.contains(".agentmosaic"), "{payload}");
        assert!(!payload.contains("state.db"), "{payload}");
    }
    cleanup(&root);
}
