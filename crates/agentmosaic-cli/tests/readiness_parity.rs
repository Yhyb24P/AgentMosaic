//! `am doctor` and `am run` must agree about the configuration a run reads.
//!
//! `am doctor` is the command `am run` sends an operator to when a run cannot
//! start. If doctor validates less than the run does, that suggestion becomes a
//! loop: doctor answers "Ready to run." about the very registry row the run
//! just refused. The property pinned here is the parity itself — a doctor
//! verdict of "ready" never coexists with a pre-root run failure — on the exact
//! configuration that reproduced the loop, on a second refused configuration,
//! and on a positive control whose team really is ready.
//!
//! Every fixture is a real project built through the CLI, and the runtime
//! binaries are the workspace's own mocks, so nothing here needs a network, a
//! credential, or a live Agent.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

fn unique_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_readiness_parity_{name}_{}_{}",
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

/// A project initialized through the CLI itself. The returned path is both the
/// project root and the repository a run works in.
fn project(name: &str) -> PathBuf {
    let root = unique_dir(name);
    let output = cli()
        .args(["init", &root.to_string_lossy()])
        .output()
        .expect("the CLI runs");
    assert!(output.status.success(), "init failed: {}", stderr(&output));
    root
}

fn database_of(project: &Path) -> PathBuf {
    project.join(".agentmosaic").join("state.db")
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

/// Register one Agent straight into the project's registry, so an arbitrary
/// `driver_config_json` can be persisted. The compatibility `register`
/// spelling is the only surface that writes the body verbatim; `-` in the
/// optional fields means "absent".
fn register(
    project: &Path,
    id: &str,
    role: &str,
    adapter: &str,
    program: &Path,
    config: Option<&str>,
) {
    let database = database_of(project);
    let output = cli()
        .arg("register")
        .arg(&database)
        .args([id, id, role, adapter])
        .arg(program)
        .args(["-", "1", "-", "-", config.unwrap_or("-")])
        .current_dir(project)
        .output()
        .expect("the CLI runs");
    assert!(
        output.status.success(),
        "register {id} failed: {}{}",
        stdout(&output),
        stderr(&output)
    );
}

/// The bounded reason a report or a failure block states, as one line.
fn reason_line(output: &str) -> Option<String> {
    let mut lines = output.lines();
    while let Some(line) = lines.next() {
        if line == "Reason" {
            return lines.next().map(|reason| reason.trim_start().to_string());
        }
    }
    None
}

fn run_could_not_start(run: &Output) -> bool {
    stderr(run).contains("Run could not start.")
}

/// The parity property itself: a doctor verdict of "ready" never coexists with
/// a pre-root run failure. Both directions are asserted, so a fixture cannot
/// pass by never running at all.
fn assert_parity(doctor: &Output, run: &Output) {
    if doctor.status.success() {
        assert!(
            !run_could_not_start(run),
            "doctor was ready and the run failed before its root:\ndoctor:\n{}\nrun:\n{}",
            stderr(doctor),
            stderr(run)
        );
    }
    if run_could_not_start(run) {
        assert!(
            !doctor.status.success(),
            "the run failed before its root while doctor said ready:\ndoctor:\n{}\nrun:\n{}",
            stderr(doctor),
            stderr(run)
        );
    }
}

/// A fixture whose Lead carries `config`, plus a valid ACP worker so the run
/// gets past the team composition check.
fn refused_lead(name: &str, config: &str) -> PathBuf {
    let root = project(name);
    register(
        &root,
        "lead",
        "reasoner",
        "codex-app-server",
        &mock_binary("codex_bridge_mock"),
        Some(config),
    );
    register(
        &root,
        "worker",
        "worker",
        "acp",
        &mock_binary("acp_m2_mock"),
        None,
    );
    root
}

/// The reproduced defect. `{"max_events":"not-a-number"}` is a stored body the
/// run's parser refuses; doctor has to refuse it too, and the run has to say
/// why instead of pointing at the check that just said the team was ready.
#[test]
fn a_stored_body_the_run_refuses_is_a_doctor_verdict_too() {
    let root = refused_lead("bad_max_events", r#"{"max_events":"not-a-number"}"#);

    let doctor = run_cli_in(&root, &["doctor"]);
    assert!(
        !doctor.status.success(),
        "doctor accepted a body the run refuses:\n{}",
        stdout(&doctor)
    );
    assert!(
        stdout(&doctor).is_empty(),
        "the report belongs on stderr:\n{}",
        stdout(&doctor)
    );
    let report = stderr(&doctor);
    assert!(report.contains("lead      not ready"), "{report}");
    assert!(
        report.contains("team      not ready  1 lead · 1 worker"),
        "{report}"
    );
    assert!(
        report.contains("`max_events` must be a number"),
        "the report does not name the configuration problem:\n{report}"
    );

    let run = run_cli_in(&root, &["run", "probe objective"]);
    assert!(!run.status.success(), "the run cannot start");
    assert_eq!(stdout(&run), "", "a failed run wrote to stdout");
    let failure = stderr(&run);
    assert!(
        failure.contains("Run could not start.\n\nReason\n  "),
        "{failure}"
    );
    assert!(
        failure.contains("`max_events` must be a number"),
        "the pre-root failure does not carry the real reason:\n{failure}"
    );
    assert!(
        !failure.contains("Run could not start.\n  am doctor"),
        "the suggestion still replaces the reason:\n{failure}"
    );
    assert!(
        failure.contains("\nCheck\n  am doctor --verbose\n"),
        "the check is still named, after the reason:\n{failure}"
    );

    // The two surfaces explain one configuration the same way.
    assert_eq!(
        reason_line(&failure).as_deref(),
        reason_line(&report).as_deref(),
        "doctor and run disagree about the configuration"
    );
    assert_parity(&doctor, &run);
    let _ = fs::remove_dir_all(root);
}

/// A zero event budget is refused by the Codex driver itself. Doctor has to
/// reach the same verdict through the same construction.
#[test]
fn a_zero_event_budget_is_not_ready_for_doctor_either() {
    let root = refused_lead("zero_max_events", r#"{"max_events":0}"#);

    let doctor = run_cli_in(&root, &["doctor"]);
    assert!(
        !doctor.status.success(),
        "doctor accepted a zero event budget:\n{}",
        stdout(&doctor)
    );
    let report = stderr(&doctor);
    assert!(report.contains("lead      not ready"), "{report}");
    assert!(
        report.contains("max_events must be greater than zero"),
        "the report does not name the configuration problem:\n{report}"
    );

    let run = run_cli_in(&root, &["run", "probe objective"]);
    assert!(!run.status.success());
    assert_eq!(stdout(&run), "", "a failed run wrote to stdout");
    assert_eq!(
        reason_line(&stderr(&run)).as_deref(),
        reason_line(&report).as_deref(),
        "doctor and run disagree about the configuration"
    );
    assert_parity(&doctor, &run);
    let _ = fs::remove_dir_all(root);
}

/// The positive control: a valid Codex Lead with a tuned event budget and a
/// valid ACP worker. Doctor is ready, so the property under test is the other
/// half of the parity — the same configuration carries a run to its durable
/// root.
#[test]
fn a_ready_verdict_carries_the_same_configuration_through_a_run() {
    let root = project("ready_control");
    let codex = mock_binary("codex_bridge_mock");
    let acp = mock_binary("acp_m2_mock");
    // The worker records exactly the artifact it is registered with.
    fs::write(root.join("result.txt"), "parity control\n").unwrap();
    ok_in(
        &root,
        &[
            "agent",
            "add",
            "lead",
            "--role",
            "reasoner",
            "--adapter",
            "codex-app-server",
            "--max-events",
            "4000",
            "--",
            &codex.display().to_string(),
        ],
    );
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
            "--artifact",
            "result.txt",
            "--",
            &acp.display().to_string(),
        ],
    );

    let doctor = run_cli_in(&root, &["doctor"]);
    assert!(
        doctor.status.success(),
        "the ready control is not ready:\n{}{}",
        stdout(&doctor),
        stderr(&doctor)
    );
    let report = stdout(&doctor);
    assert!(report.contains("Ready to run."), "{report}");
    assert!(
        report.contains("team      ready  1 lead · 1 worker"),
        "{report}"
    );

    // The same configuration, through a real run: the scripted Lead delegates
    // one bulk task and completes, and the run reaches its durable root.
    let run = cli()
        .args(["run", "deliver the objective"])
        .current_dir(&root)
        .env("CODEX_BRIDGE_MOCK_REPLIES", scripted_replies())
        .output()
        .expect("the CLI runs");
    assert!(
        run.status.success(),
        "the ready control could not run:\n{}{}",
        stdout(&run),
        stderr(&run)
    );
    assert_eq!(stdout(&run), "lead synthesized final answer\n");
    assert_parity(&doctor, &run);
    let _ = fs::remove_dir_all(root);
}

/// The reply sequence the control's run consumes: delegate one bulk task to the
/// worker, then complete against it, which is always task 2 of a fresh board.
fn scripted_replies() -> String {
    serde_json::to_string(&[
        json!({
            "action": "delegate",
            "tasks": [
                {"kind": "bulk", "target": "worker", "objective": "produce the worker result"},
            ],
        })
        .to_string(),
        json!({
            "action": "complete",
            "answer": "lead synthesized final answer",
            "selected_task_ids": [2],
            "selected_artifacts": [],
        })
        .to_string(),
    ])
    .unwrap()
}
