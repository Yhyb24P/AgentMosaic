//! `am agent add --max-events`, driven through the real binary.
//!
//! The option is the normal path's way to express the Codex event budget the
//! Lead brain and the Codex team driver read from `driver_config_json`. These
//! tests read the durable registry row back, so what is asserted is the
//! persisted configuration the runtime will actually parse — not the printed
//! summary. The two shapes without `--max-events` are the regression guard:
//! they must stay byte-identical to what the surface wrote before the option
//! existed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use agentmosaic_storage::{AgentRegistryRecord, SqliteAgentRegistry};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

fn unique_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_agent_config_{name}_{}_{}",
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

/// A project initialized through the CLI itself.
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

/// The durable registry row, or `None` when the Agent was never persisted.
fn registry_row(project: &Path, id: &str) -> Option<AgentRegistryRecord> {
    SqliteAgentRegistry::open(database_of(project))
        .unwrap()
        .get_agent(id)
        .unwrap()
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
fn max_events_alone_persists_the_typed_key() {
    let root = project("max_events_only");
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
            "codex",
        ],
    );

    let row = registry_row(&root, "lead").expect("the Agent is registered");
    assert_eq!(
        row.driver_config_json.as_deref(),
        Some(r#"{"max_events":4000}"#)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn max_events_joins_the_artifact_paths_object() {
    let root = project("max_events_both");
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
            "--artifact",
            "result.txt",
            "--max-events",
            "4000",
            "--",
            "codex",
        ],
    );

    let row = registry_row(&root, "lead").expect("the Agent is registered");
    // One object, two keys, deterministic order: the runtime parses both.
    assert_eq!(
        row.driver_config_json.as_deref(),
        Some(r#"{"artifact_paths":["result.txt"],"max_events":4000}"#)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn zero_max_events_is_refused_and_persists_nothing() {
    let root = project("max_events_zero");
    let output = run_cli_in(
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
            "0",
            "--",
            "codex",
        ],
    );
    assert!(!output.status.success());
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
    let message = stderr(&output);
    assert!(
        message.contains("max-events must be greater than zero"),
        "{message}"
    );
    assert!(registry_row(&root, "lead").is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn max_events_is_refused_for_the_acp_adapter() {
    let root = project("max_events_acp");
    let output = run_cli_in(
        &root,
        &[
            "agent",
            "add",
            "worker",
            "--role",
            "worker",
            "--adapter",
            "acp",
            "--max-events",
            "4000",
            "--",
            "qwen",
            "--acp",
        ],
    );
    assert!(!output.status.success());
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
    let message = stderr(&output);
    assert!(
        message.contains("--max-events applies to the codex-app-server adapter, not acp"),
        "{message}"
    );
    assert!(registry_row(&root, "worker").is_none());
    fs::remove_dir_all(root).unwrap();
}

/// The regression guard: without the new flag the persisted body is exactly
/// what the surface wrote before the option existed.
#[test]
fn without_max_events_the_persisted_shape_is_unchanged() {
    let root = project("max_events_absent");
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
            "--artifact",
            "out/result.txt",
            "--",
            "codex",
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
            "--",
            "qwen",
            "--acp",
        ],
    );

    let lead = registry_row(&root, "lead").expect("the Agent is registered");
    assert_eq!(
        lead.driver_config_json.as_deref(),
        Some(r#"{"artifact_paths":["out/result.txt"]}"#)
    );
    let worker = registry_row(&root, "worker").expect("the Agent is registered");
    assert_eq!(worker.driver_config_json, None);
    fs::remove_dir_all(root).unwrap();
}

/// The tuned Lead is still a normal Lead: readiness does not depend on the
/// event budget.
#[test]
fn doctor_still_passes_with_a_max_events_lead() {
    let root = project("max_events_doctor");
    let codex = mock_binary("codex_bridge_mock");
    let acp = mock_binary("acp_m2_mock");
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
    let report = stdout(&output);
    assert!(
        report.contains("team      ready  1 lead · 1 worker"),
        "{report}"
    );
    assert!(report.contains("Ready to run."), "{report}");
    fs::remove_dir_all(root).unwrap();
}
