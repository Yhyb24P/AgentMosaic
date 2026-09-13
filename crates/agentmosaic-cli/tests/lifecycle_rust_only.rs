use std::process::{Command, Output};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn rust_only_normal_path_does_not_start_a_driver() {
    let database = std::env::temp_dir().join(format!(
        "agentmosaic_lifecycle_rust_only_{}_{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let db = database.to_string_lossy().into_owned();

    let register = cli()
        .args([
            "register",
            &db,
            "acp-worker",
            "acp-worker",
            "worker",
            "acp",
            "codex",
            "-w --acp,local",
            "2",
            "qwen,local-model",
        ])
        .output()
        .unwrap();
    assert!(
        register.status.success(),
        "register failed: {}",
        text(&register)
    );
    assert!(text(&register).contains("registered agent=acp-worker"));

    let listed = cli().args(["registry", &db]).output().unwrap();
    assert!(
        listed.status.success(),
        "registry failed: {}",
        text(&listed)
    );
    let registry_output = text(&listed);
    assert!(registry_output.contains("id=acp-worker name=acp-worker tier=worker"));
    assert!(registry_output.contains("driver_kind=acp"));
    assert!(registry_output.contains("executable=codex"));
    assert!(registry_output.contains("concurrency=2"));
    assert!(registry_output.contains("tags=[\"qwen\",\"local-model\"]"));

    let submit = cli()
        .args(["submit", &db, "bulk", "inspect", "the", "tree"])
        .output()
        .unwrap();
    assert!(submit.status.success(), "submit failed: {}", text(&submit));
    let submitted = text(&submit);
    let task = submitted
        .trim()
        .strip_prefix("submitted task=")
        .expect("submitted task id");

    let pending = cli().args(["status", &db]).output().unwrap();
    assert!(pending.status.success());
    let pending_output = text(&pending);
    assert!(pending_output.contains(&format!("task={task} status=pending")));

    assert!(cli()
        .args(["cancel", &db, task])
        .status()
        .unwrap()
        .success());
    assert!(cli()
        .args(["resume", &db, task])
        .status()
        .unwrap()
        .success());
    assert!(cli()
        .args(["override", &db, task, "acp-worker"])
        .status()
        .unwrap()
        .success());

    let assigned = cli().args(["status", &db]).output().unwrap();
    let assigned_output = text(&assigned);
    assert!(assigned_output.contains(&format!("task={task} status=assigned")));
    assert!(assigned_output.contains("assignee=acp-worker"));

    let artifact = cli().args(["artifact", &db, task]).output().unwrap();
    assert!(artifact.status.success());
    assert!(text(&artifact).trim().is_empty());

    let finalizer = cli().args(["final", &db, task]).output().unwrap();
    assert!(!finalizer.status.success());
    let message = String::from_utf8_lossy(&finalizer.stderr);
    assert!(message.contains("no successful result for task"));

    assert!(cli()
        .args(["registry", &db, "1"])
        .status()
        .unwrap()
        .success());
    let _ = std::fs::remove_file(database);
}
