use std::fs;
use std::path::PathBuf;
use std::process::Command;

use agentmosaic_storage::SqliteAgentRegistry;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

fn temporary_project(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "agentmosaic_{name}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn project_onboarding_discovers_git_root_and_preserves_launch_argv() {
    let root = temporary_project("project_onboarding");
    let nested = root.join("nested/worktree");
    fs::create_dir_all(&nested).unwrap();
    assert!(Command::new("git")
        .args(["init", "--quiet", &root.to_string_lossy()])
        .status()
        .unwrap()
        .success());

    let first = cli()
        .args(["init", &nested.to_string_lossy()])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = cli()
        .args(["init", &nested.to_string_lossy()])
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(root.join(".agentmosaic/state.db").is_file());
    let ignore = fs::read_to_string(root.join(".gitignore")).unwrap();
    assert_eq!(
        ignore
            .lines()
            .filter(|line| line.trim() == "/.agentmosaic/")
            .count(),
        1
    );

    let added = cli()
        .current_dir(&nested)
        .args([
            "agent",
            "add",
            "wrapper",
            "--role",
            "worker",
            "--adapter",
            "acp",
            "--",
            "agentmosaic-wrapper-not-installed",
            "qw",
            "-ds",
            "--acp",
            "--unknown-flag",
        ])
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );

    let lead = cli()
        .current_dir(&nested)
        .args([
            "agent",
            "add",
            "lead",
            "--role",
            "reasoner",
            "--adapter",
            "codex-app-server",
            "--",
            "agentmosaic-lead-not-installed",
        ])
        .output()
        .unwrap();
    assert!(
        lead.status.success(),
        "{}",
        String::from_utf8_lossy(&lead.stderr)
    );

    let list = cli()
        .current_dir(&nested)
        .args(["agent", "list"])
        .output()
        .unwrap();
    assert!(list.status.success());
    let list = String::from_utf8_lossy(&list.stdout);
    assert!(list.contains("wrapper"));
    // `agent list` renders a bounded LAUNCH column now, so the intent of this
    // test — the opaque argv survives end-to-end through CLI registration — is
    // read back from the durable registry instead.
    let registered = SqliteAgentRegistry::open(root.join(".agentmosaic/state.db"))
        .unwrap()
        .get_agent("wrapper")
        .unwrap()
        .expect("the wrapper Agent is registered");
    assert_eq!(
        serde_json::from_str::<Vec<String>>(registered.driver_args_json.as_deref().unwrap())
            .unwrap(),
        vec!["qw", "-ds", "--acp", "--unknown-flag"]
    );

    let doctor = cli().current_dir(&nested).arg("doctor").output().unwrap();
    assert!(!doctor.status.success());
    // The default doctor report is a decision on stderr: the decision lines and
    // the remediation, not the bounded diagnostic stages.
    assert!(doctor.stdout.is_empty(), "the report belongs on stderr");
    let doctor = String::from_utf8_lossy(&doctor.stderr);
    assert!(doctor.contains("project   ready"));
    assert!(doctor.contains("lead      not ready"));
    assert!(doctor.contains("wrapper   not ready"));
    assert!(doctor.contains("team      not ready  1 lead · 1 worker"));
    assert!(doctor.contains("\nReason\n"));
    assert!(doctor.contains("\nFix\n"));
    assert!(!doctor.contains("PROGRAM_"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn init_uses_non_git_directory_and_uninitialized_project_fails_clearly() {
    let root = temporary_project("non_git_project");
    fs::create_dir_all(&root).unwrap();
    let before = cli().current_dir(&root).arg("doctor").output().unwrap();
    assert!(!before.status.success());
    assert!(String::from_utf8_lossy(&before.stderr).contains("am init"));

    let initialized = cli()
        .args(["init", &root.to_string_lossy()])
        .output()
        .unwrap();
    assert!(initialized.status.success());
    assert!(root.join(".agentmosaic/state.db").is_file());
    assert!(!root.join(".gitignore").exists());
    fs::remove_dir_all(root).unwrap();
}
