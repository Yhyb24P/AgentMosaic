//! The machine-readable (`--json`) output contract, end to end.
//!
//! Every `--json` invocation prints exactly one typed object on stdout and
//! nothing else, so `am ... --json | jq` is a stable contract rather than a
//! rendering that happens to be parseable. The tests below drive the real
//! binary against the scripted Codex app-server mock (the Lead) and the ACP
//! mock (the Worker and the Utility), exactly as `run_output.rs` does: the
//! fixture is mirrored rather than shared, because each integration test file
//! is its own crate.
//!
//! What is asserted here is the contract, not the bytes: one complete JSON
//! value per invocation, whole digests where the human notice abbreviates,
//! no terminal control sequence, no argv/credential/runtime-id leak, and no
//! success object on a failed run.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{json, Value};

const LEAD_ANSWER: &str = "lead synthesized final answer";
/// The one artifact the registered Worker records. Its exact bytes are fixed
/// here so the recorded digest is a known constant: the Lead can only select an
/// artifact whose durable digest matches, which is itself an integrity check.
const ARTIFACT: &str = "result.txt";
const ARTIFACT_CONTENT: &str = "machine output fixture\n";
const ARTIFACT_SHA256: &str = "95f598b503939e93230f9a88391366370d0d84fcbce6dd9321427b54c0bd6243";

/// The mock Codex app-server reads its script from this variable when a
/// registration carries no `overrides` of its own: `am agent add` writes only
/// non-secret launch facts, and the scripted replies are the test's business.
const REPLIES_ENV: &str = "CODEX_BRIDGE_MOCK_REPLIES";

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

fn unique_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_machine_output_{name}_{}_{}",
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

/// Parse one complete JSON value: the parse must consume the whole payload and
/// leave no trailing text, which is exactly the piping contract.
fn parse_one(text: &str) -> Value {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value = Value::deserialize(&mut deserializer).expect("one JSON value");
    deserializer
        .end()
        .expect("no trailing text after the JSON value");
    value
}

/// No terminal control sequence, and no framing the parser did not ask for: a
/// payload is one line of printable text.
fn assert_no_control_bytes(text: &str) {
    let bytes = text.as_bytes();
    assert!(
        !bytes.contains(&0x1b),
        "an ESC byte reached the payload:\n{text}"
    );
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            assert_eq!(
                index,
                bytes.len() - 1,
                "a payload carried an embedded newline:\n{text}"
            );
            continue;
        }
        assert!(
            *byte >= 0x20 && *byte != 0x7f,
            "a control byte 0x{byte:02x} reached the payload:\n{text}"
        );
    }
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
    /// completes with a grounded answer that selects the Worker's artifact.
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
        std::fs::write(repo.join(ARTIFACT), ARTIFACT_CONTENT).unwrap();

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

    /// Any other command of this project, in the project directory.
    fn cli(&self, args: &[&str]) -> Output {
        cli()
            .args(args)
            .current_dir(&self.repo)
            .output()
            .expect("the CLI runs")
    }

    /// One `--json` invocation, asserted to be exactly one typed object on
    /// stdout with an empty stderr, and returned parsed.
    fn json(&self, args: &[&str]) -> Value {
        let output = self.cli(args);
        assert!(
            output.status.success(),
            "`am {}` failed: {}{}",
            args.join(" "),
            stdout(&output),
            stderr(&output)
        );
        assert_eq!(
            stderr(&output),
            "",
            "`am {}` wrote to stderr in JSON mode",
            args.join(" ")
        );
        let text = stdout(&output);
        assert_no_control_bytes(&text);
        parse_one(&text)
    }

    /// The ten `--json` invocations of the contract, in the order the task
    /// book lists them. `run` comes first: it is what creates the state the
    /// other nine read.
    fn every_json_invocation(&self) -> Vec<Value> {
        vec![
            self.run_json("deliver the objective"),
            self.json(&["doctor", "--json"]),
            self.json(&["agent", "list", "--json"]),
            self.json(&["status", "--json"]),
            self.json(&["status", "1", "--json"]),
            self.json(&["status", "--all", "--json"]),
            self.json(&["final", "--json"]),
            self.json(&["final", "1", "--json"]),
            self.json(&["artifact", "--json"]),
            self.json(&["artifact", "2", "--json"]),
        ]
    }

    fn run_json(&self, objective: &str) -> Value {
        let run = self.run(&["--json", objective]);
        assert!(
            run.status.success(),
            "json am run failed (exit {:?}): {}",
            run.status.code(),
            stderr(&run)
        );
        assert_eq!(stderr(&run), "", "the machine surface wrote human text");
        let text = stdout(&run);
        assert_no_control_bytes(&text);
        parse_one(&text)
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

/// The Lead completes against the bulk task and selects the one artifact it
/// produced, so the run object carries a real, whole digest.
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

/// The reply sequence a fresh project's first run consumes: delegate, then
/// complete against the bulk task, which is always task 2 of a fresh board.
fn scripted_replies() -> String {
    serde_json::to_string(&[delegate_script(), complete_script(2)]).unwrap()
}

/// A Lead that cannot produce a decision: the run reaches its root, then fails.
fn unparsable_replies() -> String {
    serde_json::to_string(&["this is not a decision".to_string()]).unwrap()
}

/// The piping contract for the quiet surface: stdout is the answer, and the
/// redirection a shell performs captures nothing else.
#[test]
fn quiet_stdout_is_only_the_final_answer() {
    let project = TeamProject::ready("quiet_pipe");
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

/// `am run --json` prints exactly one object with the required keys, whole
/// values, and none of the runtime internals the human surface also hides.
#[test]
fn run_json_is_exactly_one_typed_object() {
    let project = TeamProject::ready("run_object");
    let run = project.run(&["--json", "deliver the objective"]);
    assert!(
        run.status.success(),
        "json am run failed (exit {:?}): {}",
        run.status.code(),
        stderr(&run)
    );
    assert_eq!(stderr(&run), "", "the machine surface wrote human text");

    let text = stdout(&run);
    assert_no_control_bytes(&text);
    let value = parse_one(&text);
    let object = value.as_object().expect("one JSON object");
    assert_eq!(object.len(), 6, "unexpected fields:\n{text}");
    assert_eq!(object["run_id"], json!(1));
    assert_eq!(object["lead_agent"], json!("lead"));
    assert_eq!(object["status"], json!("succeeded"));
    assert_eq!(object["answer"], json!(LEAD_ANSWER));
    assert_eq!(object["task_refs"], json!([2]));
    let refs = object["artifact_refs"].as_array().expect("artifact_refs");
    assert_eq!(refs.len(), 1, "{text}");
    assert_eq!(refs[0]["task_id"], json!(2));
    assert_eq!(refs[0]["path"], json!(ARTIFACT));
    assert_eq!(refs[0]["sha256"], json!(ARTIFACT_SHA256));
    assert_eq!(ARTIFACT_SHA256.len(), 64, "a full sha256");

    // The machine object is the product's own result, not a runtime dump.
    for leak in [
        "mock-thread",
        "acp-m2-mock-session",
        "Current lead context",
        "\"action\"",
        "codex_bridge_mock",
        "acp_m2_mock",
    ] {
        assert!(
            !text.contains(leak),
            "the payload rendered `{leak}`:\n{text}"
        );
    }
}

/// JSON preserves values the human renderers are free to shorten: the recorded
/// digest is emitted whole everywhere it appears, while the progress notice
/// still abbreviates it.
#[test]
fn json_keeps_the_full_digest_where_the_human_notice_abbreviates_it() {
    let project = TeamProject::ready("digest_fidelity");
    let run = project.run(&["deliver the objective"]);
    assert!(run.status.success(), "{}", stderr(&run));

    let progress = stderr(&run);
    let abbreviated = format!("sha256 {}\u{2026}", &ARTIFACT_SHA256[..8]);
    assert!(progress.contains(&abbreviated), "{progress}");
    assert!(
        !progress.contains(ARTIFACT_SHA256),
        "the human notice printed the whole digest:\n{progress}"
    );

    // Every machine surface that names an artifact names all of it.
    let status = project.json(&["status", "--json"]);
    assert_eq!(status["artifacts"][0]["sha256"], json!(ARTIFACT_SHA256));
    let artifact = project.json(&["artifact", "--json"]);
    assert_eq!(artifact["artifacts"][0]["sha256"], json!(ARTIFACT_SHA256));
    let task = project.json(&["artifact", "2", "--json"]);
    assert_eq!(task["artifacts"][0]["sha256"], json!(ARTIFACT_SHA256));
    assert_eq!(
        task["artifacts"][0]["path"],
        json!(ARTIFACT),
        "the whole path is preserved"
    );
}

/// Text is never wrapped or cut either: the human status line abbreviates a
/// long objective, and the machine object carries it whole.
#[test]
fn json_keeps_a_long_objective_whole() {
    let project = TeamProject::ready("long_objective");
    let objective = format!("deliver {}", "x".repeat(200));
    let run = project.run(&["--json", &objective]);
    assert!(run.status.success(), "{}", stderr(&run));

    let value = parse_one(&stdout(&run));
    assert_eq!(value["answer"], json!(LEAD_ANSWER));
    let status = project.json(&["status", "--json"]);
    assert_eq!(status["objective"], json!(objective));
    assert_eq!(status["tasks"][0]["objective"], json!(objective));

    let human = project.cli(&["status"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human);
    assert!(
        text.contains("..."),
        "the human objective is bounded:\n{text}"
    );
    assert!(!text.contains(&objective), "{text}");
}

/// Every one of the ten `--json` invocations is one complete JSON value: the
/// parse consumes the whole payload and no trailing text is left over.
#[test]
fn every_json_invocation_parses_as_one_complete_value() {
    let project = TeamProject::ready("all_invocations");
    let values = project.every_json_invocation();
    assert_eq!(values.len(), 10);
    for value in &values {
        assert!(value.is_object() || value.is_array(), "{value:?}");
    }
    assert_eq!(values[1]["ready"], json!(true), "doctor --json");
    assert_eq!(values[2]["agents"].as_array().map(Vec::len), Some(3));
    assert_eq!(values[3]["run_id"], json!(1), "status --json");
    assert_eq!(values[4]["run_id"], json!(1), "status 1 --json");
    assert_eq!(
        values[5]["runs"].as_array().map(Vec::len),
        Some(1),
        "status --all --json"
    );
    assert_eq!(values[6]["answer"], json!(LEAD_ANSWER), "final --json");
    assert_eq!(values[7]["answer"], json!(LEAD_ANSWER), "final 1 --json");
    assert_eq!(
        values[8]["artifacts"].as_array().map(Vec::len),
        Some(1),
        "artifact --json"
    );
    assert_eq!(
        values[9]["artifacts"].as_array().map(Vec::len),
        Some(1),
        "artifact 2 --json"
    );
}

/// `am doctor --json` is a decision: ready, with each runtime's class and the
/// team's composition.
#[test]
fn doctor_json_is_the_readiness_decision() {
    let project = TeamProject::ready("doctor_decision");
    let doctor = project.json(&["doctor", "--json"]);
    assert_eq!(doctor["ready"], json!(true));
    assert_eq!(doctor["schema_version"], json!(11));
    assert_eq!(
        doctor["team"],
        json!({"lead": 1, "worker": 1, "utility": 1})
    );
    assert_eq!(doctor["reason"], Value::Null);
    assert_eq!(doctor["fix"], json!([]));
    let agents = doctor["agents"].as_array().expect("agents");
    assert_eq!(agents.len(), 3);
    for agent in agents {
        assert_eq!(agent["ready"], json!(true));
        assert_eq!(agent["stage"], json!("ready"));
        assert!(
            ["reasoner", "worker", "utility"].contains(&agent["role"].as_str().unwrap()),
            "{agent}"
        );
    }
    let lead = agents
        .iter()
        .find(|agent| agent["id"] == json!("lead"))
        .expect("the lead");
    assert_eq!(lead["adapter"], json!("codex-app-server"));
}

/// A team that cannot run still produces the decision object — with the reason
/// and the fix — and still exits non-zero, so the object is never a fake
/// success.
#[test]
fn doctor_json_reports_not_ready_and_stays_nonzero() {
    let root = unique_root("doctor_not_ready");
    let init = cli().arg("init").current_dir(&root).output().unwrap();
    assert!(init.status.success(), "{}", stderr(&init));

    let doctor = cli()
        .args(["doctor", "--json"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        !doctor.status.success(),
        "an unready team must not report success"
    );
    assert_eq!(stderr(&doctor), "", "the decision belongs on stdout");
    let text = stdout(&doctor);
    assert_no_control_bytes(&text);
    let value = parse_one(&text);
    assert_eq!(value["ready"], json!(false));
    assert_eq!(value["team"], json!({"lead": 0, "worker": 0, "utility": 0}));
    assert!(value["reason"].is_string(), "{text}");
    assert!(
        value["fix"].as_array().is_some_and(|fix| !fix.is_empty()),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A failed run writes no success object: stdout stays empty and the failure
/// keeps the human reason on stderr with a non-zero exit.
#[test]
fn a_failed_run_writes_no_success_json() {
    let project = TeamProject::with_replies("json_failure", &unparsable_replies());
    let run = project.run(&["--json", "deliver an objective the lead cannot answer"]);
    assert!(
        !run.status.success(),
        "a failing lead must not report success"
    );
    assert_eq!(stdout(&run), "", "a failed run wrote a payload");
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
}

/// The legacy `<database>` spellings keep their historical text, so `--json`
/// is refused by name instead of guessing a shape for them.
#[test]
fn json_refuses_the_legacy_database_spellings() {
    let project = TeamProject::ready("legacy_refusal");
    let database = project.database.to_string_lossy().into_owned();
    for (command, args) in [
        ("status", vec!["status", database.as_str(), "--json"]),
        ("final", vec!["final", database.as_str(), "1", "--json"]),
        (
            "artifact",
            vec!["artifact", database.as_str(), "2", "--json"],
        ),
    ] {
        let output = project.cli(&args);
        assert!(!output.status.success(), "`am {command} --json` succeeded");
        assert_eq!(stdout(&output), "", "a refusal wrote to stdout");
        let message = stderr(&output);
        assert!(message.contains(command), "{message}");
        assert!(message.contains("no JSON output"), "{message}");
    }
}

/// The launch field is the human surface's bounded, redacted rendering: a
/// credential-shaped argument never reaches a payload, and raw argv never does.
#[test]
fn agent_list_json_hides_credentials_and_raw_argv() {
    let root = unique_root("agent_list_secrets");
    let init = cli().arg("init").current_dir(&root).output().unwrap();
    assert!(init.status.success(), "{}", stderr(&init));
    let program = mock_binary("codex_bridge_mock");
    let added = cli()
        .args(["agent", "add", "lead", "--role", "reasoner"])
        .args(["--adapter", "codex-app-server", "--"])
        .arg(&program)
        .arg("--token=super-secret")
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "am agent add failed: {}{}",
        stdout(&added),
        stderr(&added)
    );

    let output = cli()
        .args(["agent", "list", "--json"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert_no_control_bytes(&text);
    let value = parse_one(&text);
    let agents = value["agents"].as_array().expect("agents");
    assert_eq!(agents.len(), 1, "{text}");
    assert_eq!(agents[0]["id"], json!("lead"));
    assert_eq!(agents[0]["role"], json!("reasoner"));
    assert_eq!(agents[0]["adapter"], json!("codex-app-server"));
    assert_eq!(agents[0]["concurrency"], json!(1));
    assert_eq!(agents[0]["tags"], json!([]));
    let launch = agents[0]["launch"].as_str().expect("a launch rendering");
    assert!(launch.contains("<redacted>"), "{launch}");
    for leak in ["super-secret", "--token", "driver_args", "executable"] {
        assert!(
            !text.contains(leak),
            "the payload rendered `{leak}`:\n{text}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}
