//! Live production end-to-end test for the team product surface, plus the
//! deterministic failure path beside it.
//!
//! The ignored test drives *only* the public product entrypoint — the real
//! `agent-code-cli` binary — with a registered real Codex Lead
//! (`codex-app-server`) and two real Qwen Code ACP agents (`worker`/`utility`).
//! The test itself never creates a delegated task, never reads a plan file,
//! never runs Qwen directly, never injects a worker result into a Codex prompt,
//! and never finalizes refs or results: the Lead, the scheduler, and the
//! drivers do all of that, and the test only reads the durable surfaces back
//! through fresh CLI processes.
//!
//! Run the live test explicitly:
//!
//! ```text
//! cargo test -p agent-code-cli --test team_live_product -- --ignored --nocapture
//! ```
//!
//! The non-ignored test at the bottom is the deterministic R9 failure path: a
//! Lead that cannot answer must leave the root observably failed, and `final`
//! must report no successful answer. It starts no live runtime.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_agent-code-cli"))
}

fn unique_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ras_team_live_{name}_{}_{}",
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

/// Locate one of the runtime's binaries. `cargo test -p agent-code-cli` can
/// leave the runtime binaries unbuilt, so build them once and retry instead of
/// failing flakily.
fn runtime_binary(name: &str) -> PathBuf {
    let candidate = target_dir().join(name);
    if candidate.is_file() {
        return candidate;
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "-p", "agent-code-runtime", "--bins"])
        .status()
        .expect("cargo is runnable");
    assert!(status.success(), "building the runtime binaries failed");
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

fn run_cli(args: &[&str]) -> Output {
    cli().args(args).output().expect("the CLI runs")
}

/// Run a CLI command that must succeed; the raw streams are part of the
/// message so a failure is diagnosable from the test output alone.
fn run_ok(args: &[&str]) -> String {
    let output = run_cli(args);
    assert!(
        output.status.success(),
        "`{}` failed (exit {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        args.join(" "),
        output.status.code(),
        stdout(&output),
        stderr(&output)
    );
    stdout(&output)
}

fn register(args: &[&str]) {
    let output = run_cli(args);
    assert!(
        output.status.success(),
        "register failed (exit {:?}): {}{}",
        output.status.code(),
        stdout(&output),
        stderr(&output)
    );
}

/// A unique random token. It is placed in the objective *and* must be carried
/// by the worker's own persisted result, so a pass proves the answer depends
/// on the delegated worker, not on the Lead echoing the objective alone.
fn random_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("RCTOK-{:016x}{:08x}", nanos as u64, std::process::id())
}

fn initialize_git_repo(repo: &Path) {
    std::fs::create_dir_all(repo).unwrap();
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(repo)
        .status()
        .expect("git is available");
    assert!(status.success(), "git init failed");
    std::fs::write(repo.join("README.md"), "team live product e2e\n").unwrap();
    let status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo)
        .status()
        .expect("git is available");
    assert!(status.success(), "git add failed");
    let status = Command::new("git")
        .args([
            "-c",
            "user.email=ras@example.invalid",
            "-c",
            "user.name=RAS Live Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "initial commit",
        ])
        .current_dir(repo)
        .status()
        .expect("git is available");
    assert!(status.success(), "git commit failed");
}

/// The exact objective handed to the one `run-team` invocation. It is
/// self-contained: the token, the required file content, the two delegated
/// tasks, and the exact selection the Lead must persist.
fn objective(token: &str) -> String {
    format!(
        "Objective: in this Git repository, create the file worker.txt containing exactly the \
single line `worker=complete {token}` followed by one newline, then report completion. \
You must delegate exactly one bulk task to agent id qwen-worker, plus exactly one utility \
task to agent id qwen-utility (the utility task only needs to return a short peer JSON \
summary). Do not create or edit any file yourself. \
The bulk task's objective must instruct the qwen-worker agent to write worker.txt with \
exactly that single line and one trailing newline, and to return a peer JSON summary whose \
summary string contains the exact string {token}. \
When you complete, your answer must contain the exact string {token}, and you must select \
the worker task that produced worker.txt together with that task's worker.txt artifact, \
using the exact path and the exact sha256 from your context. Never invent a task id, path, \
or digest."
    )
}

fn parse_root(output: &str) -> u64 {
    output
        .lines()
        .find_map(|line| line.strip_prefix("root="))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("run-team output had no root=<id>:\n{output}"))
}

/// The rendered final answer: everything after `answer: ` up to the next
/// rendered field. The answer itself may contain newlines.
fn answer_of(output: &str) -> String {
    let start = output
        .find("answer: ")
        .map(|index| index + "answer: ".len())
        .unwrap_or_else(|| panic!("team output had no answer:\n{output}"));
    let rest = &output[start..];
    let end = rest.find("\ntask_refs:").unwrap_or(rest.len());
    rest[..end].trim_end().to_string()
}

fn field_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let start = text.find(key)? + key.len();
    let rest = &text[start..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    Some(&rest[..end])
}

fn task_refs_of(output: &str) -> Vec<u64> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("task_refs: "))
        .map(|rest| {
            if rest.trim() == "-" {
                Vec::new()
            } else {
                rest.split(',')
                    .filter_map(|value| value.trim().parse().ok())
                    .collect()
            }
        })
        .unwrap_or_default()
}

/// The rendered `artifact_refs:` selections: `(task_id, path, sha256)`.
fn artifact_refs_of(output: &str) -> Vec<(u64, String, String)> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("artifact_refs: "))
        .filter_map(|rest| {
            Some((
                field_after(rest, "task=")?.parse().ok()?,
                field_after(rest, "path=")?.to_string(),
                field_after(rest, "sha256=")?.to_string(),
            ))
        })
        .collect()
}

/// One `status` line: `task=<id> status=<s> assignee=<a> attempts=<n>
/// parent=<p> objective=<o>`.
struct StatusLine {
    id: u64,
    status: String,
    parent: Option<u64>,
}

fn status_lines(text: &str) -> Vec<StatusLine> {
    text.lines()
        .filter(|line| line.starts_with("task="))
        .map(|line| StatusLine {
            id: field_after(line, "task=").unwrap().parse().unwrap(),
            status: field_after(line, "status=").unwrap().to_string(),
            parent: field_after(line, "parent=").and_then(|value| value.parse().ok()),
        })
        .collect()
}

fn artifact_sha(text: &str, path: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let listed = field_after(line, "path=")?;
        (listed == path).then(|| field_after(line, "sha256=").map(str::to_string))?
    })
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[test]
#[ignore = "requires authenticated local codex-cli and qwen-code; performs one live team run"]
fn live_run_team_produces_a_durable_worker_dependent_answer() {
    let root = unique_root("run_team");
    let repo = root.join("repo");
    initialize_git_repo(&repo);
    let database = root.join("board.db");
    let db = database.to_string_lossy().into_owned();
    let repo_arg = repo.to_string_lossy().into_owned();
    let mcp = runtime_binary("ras_codex_mcp");

    let lead_config = json!({
        "mcp_command": mcp.display().to_string(),
        "artifact_paths": ["lead-final.txt"],
        "max_events": 200,
        "model": "gpt-5.5",
        "overrides": ["model=\"gpt-5.5\"", "model_reasoning_effort=\"low\""],
    })
    .to_string();
    let worker_config = json!({
        "auth_method": "openai",
        "timeout_seconds": 600,
        "max_result_bytes": 4096,
        "artifact_paths": ["worker.txt"],
    })
    .to_string();
    let utility_config = json!({
        "auth_method": "openai",
        "timeout_seconds": 600,
        "max_result_bytes": 4096,
    })
    .to_string();

    register(&[
        "register",
        &db,
        "codex-lead",
        "codex-lead",
        "reasoner",
        "codex-app-server",
        "codex",
        "-",
        "1",
        "-",
        "-",
        &lead_config,
    ]);
    register(&[
        "register",
        &db,
        "qwen-worker",
        "qwen-worker",
        "worker",
        "acp",
        "qwen",
        "--acp",
        "1",
        "-",
        "-",
        &worker_config,
    ]);
    register(&[
        "register",
        &db,
        "qwen-utility",
        "qwen-utility",
        "utility",
        "acp",
        "qwen",
        "--acp",
        "1",
        "-",
        "-",
        &utility_config,
    ]);

    let token = random_token();
    let request = objective(&token);

    // Exactly one live `run-team`. Everything after this is read-only.
    let run = run_cli(&["run-team", &db, &repo_arg, &request]);
    eprintln!(
        "--- run-team (exit {:?}) ---\n{}\n{}",
        run.status.code(),
        stdout(&run),
        stderr(&run)
    );
    assert!(
        run.status.success(),
        "run-team failed (exit {:?}):\n{}\n{}",
        run.status.code(),
        stdout(&run),
        stderr(&run)
    );
    let run_output = stdout(&run);
    let root_id = parse_root(&run_output);
    let answer = answer_of(&run_output);
    let run_refs = task_refs_of(&run_output);
    let run_artifacts = artifact_refs_of(&run_output);
    assert!(
        answer.contains(&token),
        "the root answer did not contain the token `{token}`:\n{answer}"
    );
    assert!(
        !run_refs.is_empty(),
        "run-team selected no task refs:\n{run_output}"
    );

    let expectations = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // `final` in a fresh process reproduces the persisted answer.
        let final_output = run_ok(&["final", &db, &root_id.to_string()]);
        let final_answer = final_output.trim_end().to_string();
        assert_eq!(
            final_answer,
            answer.trim_end(),
            "`final` did not reproduce the run-team answer"
        );
        assert!(
            final_answer.contains(&token),
            "the persisted answer lost the token:\n{final_answer}"
        );

        // `resume-team` in a fresh process reproduces the answer *and* the
        // exact persisted task/artifact refs straight from the durable board.
        let resume_output = run_ok(&[
            "resume-team",
            &db,
            &repo_arg,
            &root_id.to_string(),
            "--lead",
            "codex-lead",
        ]);
        eprintln!("--- resume-team ---\n{resume_output}");
        assert_eq!(
            answer_of(&resume_output).trim_end(),
            final_answer,
            "resume-team did not reproduce the durable answer"
        );
        assert_eq!(
            task_refs_of(&resume_output),
            run_refs,
            "resume-team did not reproduce the durable task refs"
        );
        assert_eq!(
            artifact_refs_of(&resume_output),
            run_artifacts,
            "resume-team did not reproduce the durable artifact refs"
        );

        // `status` shows the succeeded root and at least one child of it.
        let status = run_ok(&["status", &db]);
        eprintln!("--- status ---\n{status}");
        let lines = status_lines(&status);
        let root_line = lines
            .iter()
            .find(|line| line.id == root_id)
            .unwrap_or_else(|| panic!("status did not list the root task {root_id}"));
        assert_eq!(root_line.status, "succeeded", "the root did not succeed");
        assert_eq!(root_line.parent, None, "the root must have no parent");
        let children: Vec<u64> = lines
            .iter()
            .filter(|line| line.parent == Some(root_id))
            .map(|line| line.id)
            .collect();
        assert!(
            !children.is_empty(),
            "status showed no child of the root:\n{status}"
        );

        // Identify the worker task from its recorded artifact: only the bulk
        // worker task records worker.txt.
        let mut worker = None;
        let mut worker_sha = None;
        for line in &lines {
            if line.id == root_id {
                continue;
            }
            let artifact = run_cli(&["artifact", &db, &line.id.to_string()]);
            if !artifact.status.success() {
                continue;
            }
            if let Some(sha) = artifact_sha(&stdout(&artifact), "worker.txt") {
                worker = Some(line.id);
                worker_sha = Some(sha);
            }
        }
        let worker = worker.expect("no task recorded a worker.txt artifact");
        let worker_sha = worker_sha.unwrap();
        let worker_artifact = run_ok(&["artifact", &db, &worker.to_string()]);
        eprintln!("--- worker {worker} artifact ---\n{worker_artifact}");
        assert!(
            children.contains(&worker),
            "the worker task {worker} is not a child of the root {root_id}"
        );
        assert!(
            is_sha256_hex(&worker_sha),
            "worker.txt is not recorded with a 64-hex sha256: {worker_sha}"
        );

        // The live worker really wrote the file, and the recorded digest is the
        // digest of what it wrote.
        let bytes = std::fs::read(repo.join("worker.txt")).expect("worker.txt exists in the repo");
        assert_eq!(
            String::from_utf8_lossy(&bytes),
            format!("worker=complete {token}\n"),
            "worker.txt does not contain exactly the requested line"
        );
        let digest = Command::new("sha256sum")
            .arg(repo.join("worker.txt"))
            .output()
            .expect("sha256sum is available");
        assert!(digest.status.success(), "sha256sum failed");
        let computed = stdout(&digest)
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        assert_eq!(
            computed, worker_sha,
            "the recorded worker.txt digest does not match the file the worker wrote"
        );

        // Dependency proof, in two parts:
        // 1. the token is in the persisted root answer;
        assert!(
            final_answer.contains(&token),
            "the persisted root answer lost the token"
        );
        // 2. the worker task is in the persisted final task refs.
        assert!(
            run_refs.contains(&worker),
            "the worker task {worker} is not in the persisted final task refs {run_refs:?}"
        );
        // ... and the token reached the root answer *through the worker*: it is
        // in the worker's own persisted result, not only in the objective.
        let worker_result = run_ok(&["final", &db, &worker.to_string()]);
        eprintln!("--- worker {worker} persisted result ---\n{worker_result}");
        assert!(
            worker_result.contains(&token),
            "the worker's persisted result did not contain the token:\n{worker_result}"
        );

        // The Lead selected the exact worker.txt artifact.
        assert!(
            run_artifacts.iter().any(|(task, path, sha)| *task == worker
                && path == "worker.txt"
                && *sha == worker_sha),
            "run-team did not select the worker.txt artifact: {run_artifacts:?}"
        );

        // The worker task carries a durable external ACP session reference.
        let binding = run_ok(&["binding", &db, &worker.to_string()]);
        eprintln!("--- worker {worker} binding ---\n{binding}");
        assert!(
            binding.contains("runtime_kind=acp"),
            "the worker binding is not ACP:\n{binding}"
        );
        assert!(
            binding.contains("external_reference_present=true"),
            "the worker binding has no external session reference:\n{binding}"
        );
    }));

    if let Err(payload) = expectations {
        eprintln!("--- failure diagnostics: full board state ---");
        let status = stdout(&run_cli(&["status", &db]));
        eprintln!("status:\n{status}");
        for line in status_lines(&status) {
            let id = line.id.to_string();
            eprintln!("--- task {id} ---");
            eprintln!("final:    {:?}", run_cli(&["final", &db, &id]));
            eprintln!("artifact: {:?}", run_cli(&["artifact", &db, &id]));
            eprintln!("binding:  {:?}", run_cli(&["binding", &db, &id]));
        }
        std::panic::resume_unwind(payload);
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// R9 failure path, deterministic and not ignored: a Lead that cannot produce a
/// decision (here its app-server executable exits immediately) must leave the
/// durable root observably `failed` — never `succeeded` — and `final` must
/// report no successful answer. No live runtime starts, and no network or
/// credential is used.
#[cfg(unix)]
#[test]
fn failed_lead_leaves_root_failed_and_final_reports_no_answer() {
    use std::os::unix::fs::PermissionsExt;

    let root = unique_root("failure_path");
    let repo = root.join("repo");
    initialize_git_repo(&repo);
    let database = root.join("board.db");
    let db = database.to_string_lossy().into_owned();
    let repo_arg = repo.to_string_lossy().into_owned();

    // Not an app-server: it exits before answering `initialize`.
    let bad_lead = root.join("bad-app-server.sh");
    std::fs::write(&bad_lead, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&bad_lead, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Any existing file satisfies the codex driver's `mcp_command` guard; the
    // failing Lead never reaches it.
    let existing = std::env::current_exe().unwrap().display().to_string();
    let lead_config = json!({"mcp_command": existing, "max_events": 32}).to_string();
    let worker_config = json!({"auth_method": "openai", "timeout_seconds": 30}).to_string();

    let bad = bad_lead.display().to_string();
    register(&[
        "register",
        &db,
        "codex-lead",
        "codex-lead",
        "reasoner",
        "codex-app-server",
        &bad,
        "-",
        "1",
        "-",
        "-",
        &lead_config,
    ]);
    register(&[
        "register",
        &db,
        "qwen-worker",
        "qwen-worker",
        "worker",
        "acp",
        "qwen",
        "--acp",
        "1",
        "-",
        "-",
        &worker_config,
    ]);
    register(&[
        "register",
        &db,
        "qwen-utility",
        "qwen-utility",
        "utility",
        "acp",
        "qwen",
        "--acp",
        "1",
        "-",
        "-",
        "-",
    ]);

    let run = run_cli(&[
        "run-team",
        &db,
        &repo_arg,
        "deliver an objective the lead cannot answer",
    ]);
    assert!(
        !run.status.success(),
        "a failing lead must not report success:\n{}\n{}",
        stdout(&run),
        stderr(&run)
    );

    let status = stdout(&run_cli(&["status", &db]));
    eprintln!("--- failure-path status ---\n{status}");
    let lines = status_lines(&status);
    let root_line = lines
        .iter()
        .find(|line| line.parent.is_none())
        .unwrap_or_else(|| panic!("status listed no root task:\n{status}"));
    assert_eq!(
        root_line.status, "failed",
        "the root must be observably failed, not {}",
        root_line.status
    );
    assert!(
        !status.contains("status=succeeded"),
        "a failed run left a succeeded task:\n{status}"
    );

    let final_attempt = run_cli(&["final", &db, &root_line.id.to_string()]);
    assert!(
        !final_attempt.status.success(),
        "final reported a successful answer for a failed root:\n{}",
        stdout(&final_attempt)
    );
    assert!(
        stderr(&final_attempt).contains("no successful result"),
        "final did not report the absence of a successful result:\n{}",
        stderr(&final_attempt)
    );

    let _ = std::fs::remove_dir_all(&root);
}
