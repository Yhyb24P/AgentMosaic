//! The compatibility regression matrix.
//!
//! The 13 compatibility commands and the four legacy inspection spellings are
//! hidden from every help listing but remain callable at exactly their current
//! spellings. "Still callable" is not a claim that they exit non-zero: each
//! invocation here is proven to be *routed* — clap accepted it, the handler for
//! that command ran, and the outcome is that command's own.
//!
//! Two kinds of proof are used, and the distinction matters:
//!
//! - a command that can succeed against a scratch database must exit 0 and
//!   leave its durable effect behind (a task, a row, a status);
//! - a command that legitimately fails on the fixture must fail *by name*: its
//!   stderr is non-empty and identifies the command itself, never clap's
//!   `unrecognized subcommand`.
//!
//! Nothing here redesigns a compatibility command; the assertions pin the
//! behaviour that already exists.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use agentmosaic_storage::{ExternalRuntimeBinding, SqliteTaskBoard};
use agentmosaic_team::{TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use serde_json::json;

const LEAD_ANSWER: &str = "lead synthesized final answer";
const ARTIFACT: &str = "result.txt";
const ARTIFACT_CONTENT: &str = "compatibility matrix fixture\n";
/// The digest of [`ARTIFACT_CONTENT`], so the recorded artifact can be named
/// exactly.
const ARTIFACT_SHA256: &str = "d1c4576a48fca24337b2b4613ef7192f99e2130a65b29fce189d7c519f0d4f78";
const REPLIES_ENV: &str = "CODEX_BRIDGE_MOCK_REPLIES";

/// The 13 top-level compatibility commands, at their exact current spellings.
const COMPATIBILITY_COMMANDS: [&str; 13] = [
    "register",
    "registry",
    "run-acp",
    "continue-acp",
    "run-team",
    "resume-team",
    "submit",
    "cancel",
    "override",
    "recover",
    "recover-all",
    "resume",
    "binding",
];

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

fn unique_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_compat_{name}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// The cargo target directory, as seen by this test binary.
fn target_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_am"))
        .parent()
        .expect("the CLI binary lives in the cargo target directory")
        .to_path_buf()
}

/// Locate one of the runtime's mock binaries. A filtered `cargo test` run can
/// leave them unbuilt, so build the runtime binaries once and retry instead of
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
        "{name} was not built at {}",
        candidate.display()
    );
    candidate
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    cli()
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the CLI runs")
}

/// The text clap prints for a subcommand it does not know. A routed command can
/// never produce it.
fn assert_not_clap_error(message: &str, command: &str) {
    for clap_error in [
        "unrecognized subcommand",
        "unexpected argument",
        "Usage: am",
    ] {
        assert!(
            !message.contains(clap_error),
            "`{command}` was answered by clap (`{clap_error}`), not dispatched:\n{message}"
        );
    }
}

/// `am <command> ...` succeeded, printed to stdout only, and returned it.
fn ok_in(dir: &Path, args: &[&str]) -> String {
    let output = run_in(dir, args);
    assert!(
        output.status.success(),
        "`am {}` failed (exit {:?}): {}{}",
        args.join(" "),
        output.status.code(),
        stdout(&output),
        stderr(&output)
    );
    assert_eq!(
        stderr(&output),
        "",
        "`am {}` wrote to stderr while succeeding",
        args.join(" ")
    );
    stdout(&output)
}

/// `am <command> ...` failed with that command's own reason.
fn err_in(dir: &Path, args: &[&str]) -> String {
    let output = run_in(dir, args);
    assert!(
        !output.status.success(),
        "`am {}` unexpectedly succeeded",
        args.join(" ")
    );
    let message = stderr(&output);
    assert_not_clap_error(&message, args[0]);
    assert!(
        !message.is_empty(),
        "`am {}` failed with no reason at all",
        args.join(" ")
    );
    message
}

/// A scratch directory holding nothing but a database path.
struct Scratch {
    root: PathBuf,
    database: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = unique_root(name);
        let database = root.join("board.db");
        Self { root, database }
    }

    fn db(&self) -> String {
        self.database.to_string_lossy().into_owned()
    }

    fn dir(&self) -> &Path {
        &self.root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A scratch project: `am init`, a repo with the Worker's artifact present, and
/// three Agents registered through the CLI against the project database the
/// compatibility commands are also pointed at.
struct Team {
    root: PathBuf,
    repo: PathBuf,
    database: PathBuf,
    replies: String,
}

impl Team {
    fn new(name: &str) -> Self {
        let root = unique_root(name);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(ARTIFACT), ARTIFACT_CONTENT).unwrap();

        let init = cli().arg("init").current_dir(&repo).output().unwrap();
        assert!(init.status.success(), "am init failed: {}", stderr(&init));

        let codex = mock_binary("codex_bridge_mock");
        let acp = mock_binary("acp_m2_mock");
        for (fields, program) in [
            (
                vec![
                    "lead",
                    "--role",
                    "reasoner",
                    "--adapter",
                    "codex-app-server",
                    "--",
                ],
                &codex,
            ),
            (
                vec![
                    "worker",
                    "--role",
                    "worker",
                    "--adapter",
                    "acp",
                    "--artifact",
                    ARTIFACT,
                    "--",
                ],
                &acp,
            ),
            (
                vec!["utility", "--role", "utility", "--adapter", "acp", "--"],
                &acp,
            ),
        ] {
            let output = cli()
                .args(["agent", "add"])
                .args(&fields)
                .arg(program)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "am agent add {} failed: {}{}",
                fields[0],
                stdout(&output),
                stderr(&output)
            );
        }

        Self {
            database: repo.join(".agentmosaic").join("state.db"),
            root,
            repo,
            replies: scripted_replies(),
        }
    }

    fn db(&self) -> String {
        self.database.to_string_lossy().into_owned()
    }

    fn repo_arg(&self) -> String {
        self.repo.to_string_lossy().into_owned()
    }

    /// A command in the repo, with the scripted Lead replies available.
    fn run(&self, args: &[&str]) -> Output {
        cli()
            .args(args)
            .current_dir(&self.repo)
            .env(REPLIES_ENV, &self.replies)
            .output()
            .expect("the CLI runs")
    }
}

impl Drop for Team {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn delegate_script() -> String {
    json!({
        "action": "delegate",
        "tasks": [
            {"kind": "bulk", "target": "worker", "objective": "produce the worker result"},
            {"kind": "utility", "target": "utility", "objective": "produce the utility result"},
        ],
    })
    .to_string()
}

fn complete_script(task_id: u64) -> String {
    json!({
        "action": "complete",
        "answer": LEAD_ANSWER,
        "selected_task_ids": [task_id],
        "selected_artifacts": [{
            "task_id": task_id,
            "path": ARTIFACT,
            "sha256": ARTIFACT_SHA256,
        }],
    })
    .to_string()
}

fn scripted_replies() -> String {
    serde_json::to_string(&[delegate_script(), complete_script(2)]).unwrap()
}

/// The surface still advertises every compatibility command: a command that
/// disappeared from `am advanced` would still parse, but the matrix below is
/// about the surface a user actually has.
#[test]
fn the_compatibility_surface_still_lists_every_command() {
    let scratch = Scratch::new("advanced_listing");
    let listing = ok_in(scratch.dir(), &["advanced"]);
    for command in COMPATIBILITY_COMMANDS {
        assert!(
            listing.contains(command),
            "`am advanced` no longer lists `{command}`:\n{listing}"
        );
    }
}

/// A genuinely unknown top-level command is a clap error. This is the negative
/// control every `assert_not_clap_error` above is compared against.
#[test]
fn an_unknown_command_is_a_clap_error() {
    let scratch = Scratch::new("unknown_control");
    let output = run_in(scratch.dir(), &["definitely-not-a-command"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("unrecognized subcommand"),
        "{}",
        stderr(&output)
    );
}

/// `register` writes the row and `registry` reads it back.
#[test]
fn register_and_registry_route_and_persist() {
    let scratch = Scratch::new("register");
    let database = scratch.db();
    let registered = ok_in(
        scratch.dir(),
        &[
            "register", &database, "extra", "extra", "worker", "acp", "qwen", "--acp", "1", "-",
        ],
    );
    assert_eq!(
        registered.trim(),
        "registered agent=extra",
        "register's own payload"
    );

    let listed = ok_in(scratch.dir(), &["registry", &database]);
    assert!(listed.contains("id=extra"), "{listed}");
    assert!(listed.contains("tier=worker"), "{listed}");
    assert!(listed.contains("driver_kind=acp"), "{listed}");

    // The limit spelling is the same command.
    let limited = ok_in(scratch.dir(), &["registry", &database, "0"]);
    assert_eq!(limited.trim(), "", "a zero limit prints no rows");
}

/// `submit`, `override`, `cancel`, `resume`, `recover` and `recover-all` each
/// succeed and leave the durable status they claim.
#[test]
fn the_board_control_commands_route_and_persist() {
    let scratch = Scratch::new("board_control");
    let database = scratch.db();
    let dir = scratch.dir();

    // submit
    let submitted = ok_in(dir, &["submit", &database, "bulk", "bounded work"]);
    let task = submitted
        .trim()
        .strip_prefix("submitted task=")
        .unwrap_or_else(|| panic!("submit's own payload: {submitted}"))
        .to_string();

    let board = || ok_in(dir, &["status", &database]);
    assert!(
        board().contains(&format!("task={task} status=pending")),
        "{}",
        board()
    );

    // override
    let overridden = ok_in(dir, &["override", &database, &task, "worker"]);
    assert_eq!(
        overridden.trim(),
        format!("overrode task={task} agent=worker")
    );
    assert!(
        board().contains(&format!("task={task} status=assigned assignee=worker")),
        "{}",
        board()
    );

    // recover / recover-all find nothing to recover, and say so by name.
    let recovered = ok_in(dir, &["recover", &database, &task]);
    assert!(
        recovered.contains(&format!(
            "recover found no interrupted running attempt task={task}"
        )),
        "{recovered}"
    );
    let recovered_all = ok_in(dir, &["recover-all", &database]);
    assert!(
        recovered_all.contains("recover-all found no interrupted running attempts"),
        "{recovered_all}"
    );

    // cancel, then resume: the status really moves.
    let cancelled = ok_in(dir, &["cancel", &database, &task]);
    assert_eq!(cancelled.trim(), format!("cancelled task={task}"));
    assert!(
        board().contains(&format!("task={task} status=cancelled")),
        "{}",
        board()
    );

    let resumed = ok_in(dir, &["resume", &database, &task]);
    assert_eq!(resumed.trim(), format!("resumed task={task}"));
    assert!(
        board().contains(&format!("task={task} status=pending")),
        "{}",
        board()
    );
}

/// The reasoning root of a team run has its own resume entry point. `am resume`
/// refuses it and mutates nothing: moving a root to Pending would leave a state
/// `resume-team` refuses, which is the conflict this pins.
#[test]
fn resume_refuses_a_reasoning_root_without_mutating_it() {
    let scratch = Scratch::new("resume_reasoning_root");
    let database = scratch.db();
    let dir = scratch.dir();

    let root = ok_in(dir, &["submit", &database, "reasoning", "team objective"])
        .trim()
        .strip_prefix("submitted task=")
        .expect("submit's own payload")
        .to_string();
    // Failed/cancelled is exactly the shape `am resume` used to accept.
    ok_in(dir, &["cancel", &database, &root]);
    let before = std::fs::read(&scratch.database).expect("the board file");

    let message = err_in(dir, &["resume", &database, &root]);
    assert!(message.contains("resume-team"), "{message}");

    let after = std::fs::read(&scratch.database).expect("the board file");
    assert_eq!(before, after, "a refused resume must not mutate the board");
    assert!(
        ok_in(dir, &["status", &database]).contains(&format!("task={root} status=cancelled")),
        "the cancelled root keeps its status"
    );
}

/// A root's durable Lead is what a resume continues, and the compatibility
/// `override` moves its target to `Assigned`, a state no resume can claim.
/// Replacing a root's Lead would also bypass the cross-Lead protection, so the
/// command refuses a reasoning root before any mutation.
#[test]
fn override_refuses_reasoning_root_without_mutation() {
    let scratch = Scratch::new("override_reasoning_root");
    let database = scratch.db();
    let dir = scratch.dir();
    // A root with real history: its own attempt and an external binding, so the
    // refusal can be shown to leave all three untouched.
    let root = {
        let mut board = SqliteTaskBoard::open(
            rusqlite::Connection::open(&scratch.database).expect("open board"),
        )
        .expect("board");
        let root = board
            .create_task(
                "team objective",
                None,
                TaskKind::Reasoning,
                Some("lead".into()),
            )
            .expect("root");
        board.assign(root, "lead").expect("assign");
        board
            .record_attempt(&TaskAttempt {
                task_id: root,
                attempt: 1,
                agent_id: "lead".into(),
                status: TaskStatus::Failed,
                result: None,
                error: Some("interrupted synthesis".into()),
            })
            .expect("attempt");
        board.set_status(root, TaskStatus::Failed).expect("status");
        board
            .upsert_external_binding(&ExternalRuntimeBinding {
                team_task_id: root,
                attempt: 1,
                agent_id: "lead".into(),
                runtime_kind: "codex-exec".into(),
                native_thread_id: Some("thread-1".into()),
                native_turn_id: None,
                lifecycle_state: "failed".into(),
            })
            .expect("binding");
        root
    };
    let root_arg = root.to_string();
    let before = std::fs::read(&scratch.database).expect("the board file");

    let message = err_in(dir, &["override", &database, &root_arg, "worker-override"]);
    assert!(message.contains("reasoning root"), "{message}");

    assert_eq!(
        std::fs::read(&scratch.database).expect("the board file"),
        before,
        "a refused override must not mutate the board"
    );
    let connection = rusqlite::Connection::open(&scratch.database).expect("reopen");
    let (status, assignee): (String, Option<String>) = connection
        .query_row(
            "SELECT status, assignee FROM team_tasks WHERE id = ?1",
            [root as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("root row");
    assert_eq!(status, "failed");
    assert_eq!(assignee.as_deref(), Some("lead"));
    let attempts: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM team_task_runs WHERE task_id = ?1",
            [root as i64],
            |row| row.get(0),
        )
        .expect("attempt count");
    assert_eq!(attempts, 1);
    let bindings: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM external_runtime_bindings WHERE team_task_id = ?1",
            [root as i64],
            |row| row.get(0),
        )
        .expect("binding count");
    assert_eq!(bindings, 1);
}

/// `run-acp`, `continue-acp` and `binding` fail on a fixture with no bound
/// session, and each failure is that command's own — it names itself.
#[test]
fn the_acp_commands_route_and_fail_by_name() {
    let scratch = Scratch::new("acp_routes");
    let database = scratch.db();
    let dir = scratch.dir();
    let working = std::env::temp_dir().to_string_lossy().into_owned();

    ok_in(
        dir,
        &[
            "register", &database, "worker", "worker", "worker", "acp", "qwen", "--acp", "1", "-",
        ],
    );
    let first = ok_in(dir, &["submit", &database, "bulk", "source"])
        .trim()
        .strip_prefix("submitted task=")
        .unwrap()
        .to_string();
    let second = ok_in(dir, &["submit", &database, "bulk", "next"])
        .trim()
        .strip_prefix("submitted task=")
        .unwrap()
        .to_string();

    // The ACP driver refuses a working directory escape, so the command fails
    // with its own reason and records the attempt as failed.
    let run_acp = err_in(
        dir,
        &[
            "run-acp",
            &database,
            &second,
            "worker",
            &working,
            "-",
            "30",
            "../outside",
        ],
    );
    assert!(run_acp.starts_with("run-acp:"), "{run_acp}");
    assert!(
        ok_in(dir, &["status", &database]).contains(&format!("task={second} status=failed")),
        "run-acp's durable effect"
    );

    let continue_acp = err_in(
        dir,
        &[
            "continue-acp",
            &database,
            &first,
            "worker",
            &second,
            &working,
            "-",
            "30",
        ],
    );
    assert!(continue_acp.starts_with("continue-acp:"), "{continue_acp}");
    assert!(
        continue_acp.contains("no external binding"),
        "{continue_acp}"
    );

    let binding = err_in(dir, &["binding", &database, &first]);
    assert!(binding.starts_with("binding:"), "{binding}");
    assert!(binding.contains("no external runtime binding"), "{binding}");
}

/// `run-team` and `resume-team` route through the real team runner, and the
/// three legacy inspection spellings read the state they produced.
#[test]
fn run_team_resume_team_and_the_legacy_inspections_route() {
    let team = Team::new("team_routes");
    let database = team.db();
    let repo = team.repo_arg();

    // run-team: a full durable run, with the scripted Lead and the ACP mock.
    let run = team.run(&["run-team", &database, &repo, "deliver the objective"]);
    assert!(
        run.status.success(),
        "run-team failed: {}{}",
        stdout(&run),
        stderr(&run)
    );
    let payload = stdout(&run);
    assert!(payload.contains("root=1 lead=lead\n"), "{payload}");
    assert!(
        payload.contains(&format!("answer: {LEAD_ANSWER}\n")),
        "{payload}"
    );
    assert!(payload.contains("task_refs: 2\n"), "{payload}");
    assert!(
        payload.contains(&format!(
            "artifact_refs: task=2 path={ARTIFACT} sha256={ARTIFACT_SHA256}\n"
        )),
        "{payload}"
    );

    // resume-team: a succeeded root returns its persisted result.
    let resumed = team.run(&["resume-team", &database, &repo, "1"]);
    assert!(
        resumed.status.success(),
        "resume-team failed: {}{}",
        stdout(&resumed),
        stderr(&resumed)
    );
    assert!(
        stdout(&resumed).contains("root=1 lead=lead\n"),
        "{}",
        stdout(&resumed)
    );
    assert!(
        stdout(&resumed).contains(&format!("answer: {LEAD_ANSWER}\n")),
        "{}",
        stdout(&resumed)
    );

    // legacy `status <database>`: the whole board.
    let status = ok_in(&team.repo, &["status", &database]);
    assert!(status.contains("task=1 status=succeeded"), "{status}");
    assert!(status.contains("task=2 status=succeeded"), "{status}");

    // legacy `final <database> <root>`: the persisted answer.
    let final_result = ok_in(&team.repo, &["final", &database, "1"]);
    assert_eq!(final_result.trim(), LEAD_ANSWER);

    // legacy `artifact <database> <task>`: the recorded artifact.
    let artifact = ok_in(&team.repo, &["artifact", &database, "2"]);
    assert_eq!(
        artifact.trim(),
        format!("task=2 path={ARTIFACT} sha256={ARTIFACT_SHA256}")
    );
}

/// `am tui <database>` reaches the TUI's own entry point.
///
/// The live board needs a user at a terminal, so the invocation is proven
/// routed by the two failures it can produce here: a path that cannot be opened
/// as a database fails inside the TUI's own open path, and an id where a
/// database path belongs is refused by the CLI's `tui` handling. Neither is
/// clap's unknown-command error.
#[test]
fn the_legacy_tui_spelling_routes() {
    let scratch = Scratch::new("tui_route");
    let missing = scratch.dir().join("missing.db");

    let output = run_in(scratch.dir(), &["tui", &missing.to_string_lossy()]);
    assert!(
        !output.status.success(),
        "a missing database cannot be opened"
    );
    assert_eq!(stdout(&output), "", "the board wrote to stdout");
    assert!(!stderr(&output).is_empty(), "tui failed without a reason");
    assert_not_clap_error(&stderr(&output), "tui");

    // The same spelling with an id is the CLI's `tui` refusal, by name.
    let refused = err_in(scratch.dir(), &["tui", "7"]);
    assert!(refused.contains("`tui`"), "{refused}");
    assert!(refused.contains("database path"), "{refused}");
}
