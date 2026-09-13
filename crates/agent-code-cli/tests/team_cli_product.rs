//! Product tests for the CLI's `run-team` / `resume-team` surface.
//!
//! The whole path is exercised through the real binary: `register` three real
//! Agents, `run-team` one objective (the Lead is the scripted Codex app-server
//! mock, the Worker and Utility are the ACP mock), then `status` and `final` on
//! the durable root. No credentials and no live runtime.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

const LEAD_ANSWER: &str = "lead synthesized final answer";

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_agent-code-cli"))
}

fn unique_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ras_team_cli_{name}_{}_{}",
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
    PathBuf::from(env!("CARGO_BIN_EXE_agent-code-cli"))
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
        .args(["build", "-p", "agent-code-runtime", "--bins"])
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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn run_cli(args: &[&str]) -> Output {
    cli().args(args).output().expect("the CLI runs")
}

fn register(args: &[&str]) {
    let output = run_cli(args);
    assert!(
        output.status.success(),
        "register failed: {}{}",
        stdout(&output),
        stderr(&output)
    );
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
        "selected_artifacts": [],
    })
    .to_string()
}

/// Register the three Agents through the CLI itself, including the optional
/// driver-config field.
fn register_trio(db: &str, codex_mock: &Path, acp_mock: &Path) {
    let reply_script = serde_json::to_string(&[delegate_script(), complete_script(2)]).unwrap();
    let lead_config = json!({
        "mcp_command": codex_mock.display().to_string(),
        "overrides": [format!("codex_bridge_mock.replies={reply_script}")],
        "max_events": 64,
    })
    .to_string();
    let worker_config = json!({"timeout_seconds": 60}).to_string();
    let codex = codex_mock.display().to_string();
    let acp = acp_mock.display().to_string();

    register(&[
        "register",
        db,
        "lead",
        "lead",
        "reasoner",
        "codex-app-server",
        &codex,
        "-",
        "1",
        "-",
        "-",
        &lead_config,
    ]);
    register(&[
        "register",
        db,
        "worker",
        "worker",
        "worker",
        "acp",
        &acp,
        "-",
        "1",
        "-",
        "-",
        &worker_config,
    ]);
    register(&[
        "register", db, "utility", "utility", "utility", "acp", &acp, "-", "1", "-", "-", "-",
    ]);
}

#[test]
fn run_team_and_final_reproduce_one_durable_answer() {
    let root = unique_root("run-team");
    let database = root.join("board.db");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let db = database.to_string_lossy().into_owned();
    let repo_arg = repo.to_string_lossy().into_owned();
    let codex = mock_binary("codex_bridge_mock");
    let acp = mock_binary("acp_m2_mock");
    register_trio(&db, &codex, &acp);

    // The registry surface carries a bounded note for a driver config.
    let listed = run_cli(&["registry", &db]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    let registry_output = stdout(&listed);
    assert!(registry_output.contains("driver_kind=codex-app-server"));
    assert!(registry_output.contains("driver_config={\"mcp_command\""));

    let run = run_cli(&[
        "run-team",
        &db,
        &repo_arg,
        "deliver",
        "the",
        "objective",
        "--lead",
        "lead",
    ]);
    let run_output = stdout(&run);
    assert!(run.status.success(), "run-team failed: {run_output}");
    assert!(run_output.contains("root=1 lead=lead"), "{run_output}");
    assert!(run_output.contains(LEAD_ANSWER), "{run_output}");
    assert!(run_output.contains("task_refs: 2"), "{run_output}");

    let status = run_cli(&["status", &db]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_output = stdout(&status);
    assert!(
        status_output.contains("task=1 status=succeeded"),
        "{status_output}"
    );
    assert!(
        status_output.contains("task=2 status=succeeded"),
        "{status_output}"
    );

    // The durable final answer is reproduced read-only, straight from the board.
    let final_result = run_cli(&["final", &db, "1"]);
    assert!(final_result.status.success(), "{}", stderr(&final_result));
    assert_eq!(stdout(&final_result), format!("{LEAD_ANSWER}\n"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_team_usage_errors_exit_two() {
    let root = unique_root("usage");
    let database = root.join("board.db");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let db = database.to_string_lossy().into_owned();
    let repo_arg = repo.to_string_lossy().into_owned();

    // A missing objective, a missing repository, and a malformed bound are all
    // usage errors.
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
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?} did not exit 2: {}",
            stderr(&output)
        );
    }

    let _ = std::fs::remove_dir_all(root);
}
