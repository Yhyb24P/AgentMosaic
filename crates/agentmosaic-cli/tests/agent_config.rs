//! Optional artifact configuration and rejection of removed adapter options.

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
    project.join(".agentmosaic").join("state-v14.db")
}

/// The durable registry row, or `None` when the Agent was never persisted.
fn registry_row(project: &Path, id: &str) -> Option<AgentRegistryRecord> {
    SqliteAgentRegistry::open(database_of(project))
        .unwrap()
        .get_agent(id)
        .unwrap()
}

/// Persist the artifact options the runtime actually reads.
#[test]
fn default_registration_has_no_extra_options() {
    let root = project("default_options");
    ok_in(
        &root,
        &[
            "agent",
            "add",
            "lead",
            "--role",
            "reasoner",
            "--adapter",
            "codex-exec",
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

#[test]
fn removed_event_budget_is_refused_without_persisting_an_agent() {
    let root = project("removed_event_budget");
    let output = run_cli_in(
        &root,
        &[
            "agent",
            "add",
            "lead",
            "--role",
            "reasoner",
            "--adapter",
            "codex-exec",
            "--max-events",
            "4000",
            "--",
            "codex",
        ],
    );
    assert!(!output.status.success());
    assert!(stdout(&output).is_empty());
    assert!(registry_row(&root, "lead").is_none());
    fs::remove_dir_all(root).unwrap();
}
