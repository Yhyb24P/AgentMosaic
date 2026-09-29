use agentmosaic_storage::{ExternalRuntimeBinding, SqliteTaskBoard};
use agentmosaic_team::{TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}
fn project() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "am_recovery_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    assert!(cli()
        .current_dir(&path)
        .arg("init")
        .output()
        .unwrap()
        .status
        .success());
    path
}
fn board(root: &Path) -> SqliteTaskBoard {
    SqliteTaskBoard::open(Connection::open(root.join(".agentmosaic/state-v14.db")).unwrap())
        .unwrap()
}
fn run(root: &Path, args: &[&str]) -> Output {
    cli().current_dir(root).args(args).output().unwrap()
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn public_interface_is_am() {
    let output = cli().arg("--help").output().unwrap();
    assert!(output.status.success());
    let text = text(&output);
    for command in [
        "init", "import", "agent", "doctor", "run", "status", "events", "final", "artifact", "tui",
    ] {
        assert!(text.contains(command), "{command}");
    }
    assert!(!text.contains("advanced"));
    assert!(!text.contains("__internal"));
}

#[test]
fn recovery_is_explicit_scoped_and_does_not_replay() {
    let root = project();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exec_runtime.py");
    for (id, role) in [("lead", "reasoner"), ("worker", "worker")] {
        let registration = run(
            &root,
            &[
                "agent",
                "add",
                id,
                "--role",
                role,
                "--adapter",
                "codex-exec",
                "--",
                fixture.to_str().unwrap(),
            ],
        );
        assert!(registration.status.success(), "{}", text(&registration));
    }
    let mut db = board(&root);
    let task = db
        .create_task("interrupted root", None, TaskKind::Reasoning, None)
        .unwrap();
    db.assign(task, "lead").unwrap();
    db.record_attempt(&TaskAttempt {
        task_id: task,
        attempt: 1,
        agent_id: "lead".into(),
        status: TaskStatus::Running,
        result: None,
        error: None,
    })
    .unwrap();
    db.set_status(task, TaskStatus::Running).unwrap();
    db.upsert_external_binding(&ExternalRuntimeBinding {
        team_task_id: task,
        attempt: 1,
        agent_id: "lead".into(),
        runtime_kind: "codex-exec".into(),
        native_thread_id: Some("foreign-thread".into()),
        native_turn_id: None,
        lifecycle_state: "running".into(),
    })
    .unwrap();
    let child = db
        .create_task("interrupted child", Some(task), TaskKind::Bulk, None)
        .unwrap();
    db.assign(child, "worker").unwrap();
    db.record_attempt(&TaskAttempt {
        task_id: child,
        attempt: 1,
        agent_id: "worker".into(),
        status: TaskStatus::Running,
        result: None,
        error: None,
    })
    .unwrap();
    db.set_status(child, TaskStatus::Running).unwrap();
    drop(db);
    let resume = run(&root, &["run", "--resume", "1", "--json"]);
    assert!(!resume.status.success());
    assert!(
        text(&resume).contains("was not reclaimed"),
        "{}",
        text(&resume)
    );
    assert_eq!(
        board(&root).task(task).unwrap().unwrap().status,
        TaskStatus::Running
    );
    let invalid = run(&root, &["run", "--recover", "2"]);
    assert!(!invalid.status.success());
    assert!(text(&invalid).contains("not a run"));
    let recovery = run(&root, &["run", "--recover", "1", "--json"]);
    assert!(recovery.status.success(), "{}", text(&recovery));
    assert!(recovery.stderr.is_empty());
    let payload: serde_json::Value = serde_json::from_slice(&recovery.stdout).unwrap();
    assert_eq!(payload["recovered_attempt"], 1);
    let db = board(&root);
    assert_eq!(db.task(task).unwrap().unwrap().status, TaskStatus::Failed);
    assert_eq!(db.task(child).unwrap().unwrap().status, TaskStatus::Running);
    assert_eq!(
        db.external_binding(task, 1)
            .unwrap()
            .unwrap()
            .lifecycle_state,
        "interrupted"
    );
    assert_eq!(db.attempts(task).unwrap().len(), 1);
    let again = run(&root, &["run", "--recover", "1", "--json"]);
    assert!(again.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&again.stdout).unwrap()["recovered_attempt"],
        serde_json::Value::Null
    );
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn run_modes_require_one_operation() {
    let root = project();
    for args in [
        vec!["run"],
        vec!["run", "--resume", "1", "--recover", "1"],
        vec!["run", "--resume", "1", "new objective"],
        vec!["run", "--recover", "1", "new objective"],
    ] {
        assert!(!run(&root, &args).status.success(), "{args:?}");
    }
    std::fs::remove_dir_all(root).unwrap();
}
