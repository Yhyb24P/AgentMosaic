//! The `am run` output contract.
//!
//! `am run` is the product's own run surface: it reports the team's lifecycle
//! on stderr and keeps stdout composable, so `am run "..." > answer.txt`
//! captures the final answer and nothing else. Every test here drives the real
//! binary end to end against the scripted Codex app-server mock (the Lead) and
//! the ACP mock (the Worker and the Utility), with no credentials and no live
//! runtime.
//!
//! The compatibility `run-team` spelling is deliberately *not* this surface: its
//! scriptable `root=`/`answer:`/`task_refs=`/`artifact_refs=` payload is
//! unchanged, and one test below pins exactly that.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

const LEAD_ANSWER: &str = "lead synthesized final answer";
/// The one artifact the registered Worker records, present before the run.
const ARTIFACT: &str = "result.txt";
/// The mock Codex app-server reads its script from this variable when a
/// registration carries no `overrides` of its own: `am agent add` writes only
/// non-secret launch facts, and the scripted replies are the test's business.
const REPLIES_ENV: &str = "CODEX_BRIDGE_MOCK_REPLIES";

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

fn unique_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_run_output_{name}_{}_{}",
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

/// A real project: `am init`, then three Agents registered through the CLI.
struct TeamProject {
    root: PathBuf,
    repo: PathBuf,
    database: PathBuf,
    /// The scripted Lead replies, as the mock reads them.
    replies: String,
}

impl TeamProject {
    /// A ready team whose Lead delegates one bulk task to the Worker and then
    /// completes with a grounded answer.
    fn ready(name: &str) -> Self {
        Self::with_replies(name, &scripted_replies())
    }

    fn with_replies(name: &str, replies: &str) -> Self {
        Self::with_lead(name, &mock_binary("codex_bridge_mock"), replies)
    }

    /// A ready team whose Lead is the named program. Every registration goes
    /// through the CLI, exactly as a user's would.
    fn with_lead(name: &str, lead: &Path, replies: &str) -> Self {
        let root = unique_root(name);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let acp = mock_binary("acp_m2_mock");
        // The ACP Worker records exactly the artifacts it was registered with,
        // so the file has to exist before the run.
        std::fs::write(repo.join(ARTIFACT), format!("{name}\n")).unwrap();

        let init = cli().arg("init").current_dir(&repo).output().unwrap();
        assert!(
            init.status.success(),
            "am init failed: {}{}",
            stdout(&init),
            stderr(&init)
        );

        add_agent(
            &repo,
            &[
                "lead",
                "--role",
                "reasoner",
                "--adapter",
                "codex-app-server",
                "--",
            ],
            lead,
        );
        add_agent(
            &repo,
            &[
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
        );
        add_agent(
            &repo,
            &["utility", "--role", "utility", "--adapter", "acp", "--"],
            &acp,
        );

        Self {
            database: repo.join(".agentmosaic").join("state.db"),
            root,
            repo,
            replies: replies.to_string(),
        }
    }

    /// `am run` exactly as a user runs it: in the project, with the scripted
    /// Lead, capturing both streams separately.
    fn run(&self, args: &[&str]) -> Output {
        cli()
            .arg("run")
            .args(args)
            .current_dir(&self.repo)
            .env(REPLIES_ENV, &self.replies)
            .output()
            .expect("the CLI runs")
    }

    /// Any other command of this project.
    fn cli(&self, args: &[&str]) -> Output {
        cli()
            .args(args)
            .current_dir(&self.repo)
            .output()
            .expect("the CLI runs")
    }

    /// The durable digest of the Worker's artifact, read back through the
    /// product's own inspection surface rather than recomputed here.
    fn recorded_digest(&self, task: u64) -> String {
        let output = self.cli(&["artifact", &task.to_string()]);
        assert!(
            output.status.success(),
            "am artifact failed: {}",
            stderr(&output)
        );
        let text = stdout(&output);
        text.lines()
            .find_map(|line| {
                let rest = line.strip_prefix(&format!("task={task} path={ARTIFACT} sha256="))?;
                Some(rest.trim().to_string())
            })
            .unwrap_or_else(|| panic!("no recorded {ARTIFACT} digest for task {task}:\n{text}"))
    }
}

impl Drop for TeamProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn add_agent(repo: &Path, fields: &[&str], program: &Path) {
    let output = cli()
        .args(["agent", "add"])
        .args(fields)
        .arg(program)
        .current_dir(repo)
        .output()
        .expect("the CLI runs");
    assert!(
        output.status.success(),
        "am agent add {} failed: {}{}",
        fields[0],
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

/// The reply sequence a fresh project's first run consumes: delegate, then
/// complete against the bulk task, which is always task 2 of a fresh board.
fn scripted_replies() -> String {
    serde_json::to_string(&[delegate_script(), complete_script(2)]).unwrap()
}

/// A Lead that cannot produce a decision: the run reaches its root, then fails.
fn unparsable_replies() -> String {
    serde_json::to_string(&["this is not a decision".to_string()]).unwrap()
}

#[test]
fn the_answer_is_the_only_thing_on_stdout() {
    let project = TeamProject::ready("stdout_only");
    let run = project.run(&["deliver the objective"]);
    assert!(
        run.status.success(),
        "am run failed (exit {:?}):\n{}\n{}",
        run.status.code(),
        stdout(&run),
        stderr(&run)
    );

    assert_eq!(stdout(&run), format!("{LEAD_ANSWER}\n"));
    for noise in [
        "root=",
        "answer:",
        "task_refs:",
        "artifact_refs:",
        "run #",
        "lead=",
    ] {
        assert!(
            !stdout(&run).contains(noise),
            "stdout carried `{noise}`:\n{}",
            stdout(&run)
        );
    }
    assert!(
        !stderr(&run).is_empty(),
        "the run reported no progress on stderr"
    );
}

#[test]
fn progress_reports_the_run_the_lead_rounds_and_the_worker_task() {
    let project = TeamProject::ready("progress");
    let run = project.run(&["deliver the objective"]);
    assert!(run.status.success(), "{}", stderr(&run));
    let progress = stderr(&run);

    // The run chrome: the root is created first, so it is run #1.
    assert!(progress.contains("run #1  started"), "{progress}");
    assert!(progress.contains("lead=lead"), "{progress}");
    assert!(progress.contains("run #1  complete"), "{progress}");
    // The Lead's rounds.
    assert!(progress.contains("planning"), "{progress}");
    assert!(progress.contains("reviewing"), "{progress}");
    // The Worker's task.
    assert!(progress.contains("task #2"), "{progress}");
    assert!(progress.contains("delegated"), "{progress}");
    assert!(progress.contains("worker  running"), "{progress}");
    assert!(progress.contains("worker  completed"), "{progress}");
    assert!(progress.contains("bulk -> worker"), "{progress}");
    assert!(progress.contains("attempt 1"), "{progress}");

    // The closing sections are furniture, not the answer.
    assert!(
        progress.contains("\nnext\n  am status 1\n  am final 1\n  am tui\n"),
        "{progress}"
    );
}

#[test]
fn the_artifact_notice_carries_the_path_and_an_abbreviated_digest() {
    let project = TeamProject::ready("artifact_notice");
    let run = project.run(&["deliver the objective"]);
    assert!(run.status.success(), "{}", stderr(&run));

    // The durable digest, straight from the board.
    let digest = project.recorded_digest(2);
    assert_eq!(digest.len(), 64, "a full sha256: {digest}");

    let progress = stderr(&run);
    assert!(progress.contains(ARTIFACT), "{progress}");
    assert!(progress.contains("artifact  result.txt"), "{progress}");
    let abbreviated = format!("sha256 {}\u{2026}", &digest[..8]);
    assert!(
        progress.contains(&abbreviated),
        "the notice did not abbreviate the digest to `{abbreviated}`:\n{progress}"
    );
    assert!(
        !progress.contains(&digest),
        "the notice printed the whole digest:\n{progress}"
    );
    // The digest is furniture: stdout is still only the answer.
    assert_eq!(stdout(&run), format!("{LEAD_ANSWER}\n"));
}

#[test]
fn quiet_prints_the_answer_and_no_routine_progress() {
    // The flag is honored after the objective too, where clap's trailing
    // positional would otherwise make it part of the objective.
    let project = TeamProject::ready("quiet");
    let run = project.run(&["deliver the objective", "--quiet"]);
    assert!(
        run.status.success(),
        "quiet am run failed (exit {:?}): {}",
        run.status.code(),
        stderr(&run)
    );

    assert_eq!(stdout(&run), format!("{LEAD_ANSWER}\n"));
    assert_eq!(stderr(&run), "", "quiet still wrote progress");
}

#[test]
fn json_prints_the_typed_object_and_no_human_text() {
    let project = TeamProject::ready("json");
    let run = project.run(&["--json", "deliver the objective"]);
    assert!(
        run.status.success(),
        "json am run failed (exit {:?}): {}",
        run.status.code(),
        stderr(&run)
    );

    // TASK07 replaced TASK05's reserved placeholder with the typed object.
    let value: serde_json::Value =
        serde_json::from_str(stdout(&run).trim()).expect("one JSON object on stdout");
    assert_eq!(value["answer"], serde_json::json!(LEAD_ANSWER));
    assert_eq!(value["status"], serde_json::json!("succeeded"));
    assert_eq!(stderr(&run), "", "the machine surface wrote human text");
}

#[test]
fn a_run_that_cannot_start_points_at_the_readiness_check() {
    let root = unique_root("pre_root_failure");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let init = cli().arg("init").current_dir(&repo).output().unwrap();
    assert!(init.status.success(), "{}", stderr(&init));

    // No Agent is registered, so the run fails before a root exists.
    let run = cli()
        .args(["run", "deliver the objective"])
        .current_dir(&repo)
        .output()
        .unwrap();

    assert!(!run.status.success(), "a team with no agents cannot run");
    assert_eq!(stdout(&run), "", "a failed run wrote to stdout");
    assert_eq!(
        stderr(&run),
        "run     starting  deliver the objective\nRun could not start.\n  am doctor\n"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_failed_run_reports_a_human_reason_and_the_preserved_state() {
    let project = TeamProject::with_replies("post_root_failure", &unparsable_replies());
    let run = project.run(&["deliver an objective the lead cannot answer"]);

    assert!(
        !run.status.success(),
        "a failing lead must not report success"
    );
    assert_eq!(stdout(&run), "", "a failed run wrote to stdout");
    let failure = stderr(&run);
    assert!(failure.contains("run #1  failed"), "{failure}");
    assert!(
        failure.contains("\nReason\n  the lead run failed: "),
        "{failure}"
    );
    assert!(
        failure.contains("\n\nState was preserved.\n  am status 1\n"),
        "{failure}"
    );
    // The user's first line is chrome, and the reason is a sentence: no raw
    // error enum ever reaches the terminal.
    for raw in [
        "Unavailable(",
        "Brain(",
        "InvalidDecision(",
        "LeadError",
        "TeamRunnerError",
    ] {
        assert!(
            !failure.contains(raw),
            "the reason leaked `{raw}`:\n{failure}"
        );
    }
}

/// A Lead whose runtime is not an app-server at all: the root is created, then
/// the run fails on the first Lead turn. This is the failure class the user is
/// most likely to meet, and its reason has to read as a sentence.
#[cfg(unix)]
#[test]
fn a_lead_runtime_that_cannot_answer_reports_a_human_reason() {
    use std::os::unix::fs::PermissionsExt;

    let script = unique_root("failing_lead").join("not-an-app-server.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let project = TeamProject::with_lead("unavailable_lead", &script, "[]");
    let run = project.run(&["deliver an objective the lead cannot answer"]);
    let _ = std::fs::remove_file(&script);

    assert!(!run.status.success(), "a failing lead must not succeed");
    assert_eq!(stdout(&run), "", "a failed run wrote to stdout");
    let failure = stderr(&run);
    assert!(failure.contains("run #1  failed"), "{failure}");
    assert!(
        failure.contains("\nReason\n  the lead run failed: lead brain unavailable: "),
        "{failure}"
    );
    assert!(
        failure.contains("\n\nState was preserved.\n  am status 1\n"),
        "{failure}"
    );
    for raw in ["Unavailable(", "Brain(", "LeadError", "TeamRunnerError"] {
        assert!(
            !failure.contains(raw),
            "the reason leaked `{raw}`:\n{failure}"
        );
    }
}

#[test]
fn neither_stream_carries_a_runtime_id_or_the_lead_context() {
    let project = TeamProject::ready("no_leaks");
    let run = project.run(&["deliver the objective"]);
    assert!(run.status.success(), "{}", stderr(&run));
    let streams = [stdout(&run), stderr(&run)];

    for leak in [
        // The ACP mock's session id and the Codex mock's thread id.
        "acp-m2-mock-session",
        "mock-thread",
        // The rendered Lead context prompt and the raw decision JSON.
        "Current lead context",
        "\"action\"",
        "\"selected_task_ids\"",
        // The driver argv and the host bridge the adapter owns.
        "codex_bridge_mock",
        "acp_m2_mock",
        "app-server --stdio",
    ] {
        for stream in &streams {
            assert!(
                !stream.contains(leak),
                "the run rendered `{leak}`:\n{stream}"
            );
        }
    }
}

/// HARD CONSTRAINT: only `am run` changed. The compatibility `run-team` spelling
/// still prints its scriptable payload, and still prints nothing else.
#[test]
fn the_compatibility_run_team_payload_is_unchanged() {
    let project = TeamProject::ready("run_team_unchanged");
    let run = cli()
        .args(["run-team"])
        .arg(&project.database)
        .arg(&project.repo)
        .args(["deliver the objective"])
        .current_dir(&project.repo)
        .env(REPLIES_ENV, &project.replies)
        .output()
        .unwrap();

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
    assert!(payload.contains("artifact_refs: -\n"), "{payload}");
    assert!(
        payload.lines().all(
            |line| ["root=", "answer: ", "task_refs: ", "artifact_refs: "]
                .iter()
                .any(|prefix| line.starts_with(prefix))
        ),
        "run-team printed a line that is not part of its payload:\n{payload}"
    );
    assert_eq!(stderr(&run), "", "run-team grew progress output");
}
