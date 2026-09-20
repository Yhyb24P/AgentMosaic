//! End-to-end context construction at the public default task limit.
use std::{fs, path::Path, process::Command};

fn am(root: &Path, args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_am"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null)
}

#[test]
#[cfg(unix)]
fn default_run_delivers_32_escaped_results_and_exact_artifact_owners() {
    let root = std::env::temp_dir().join(format!(
        "am_context_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    assert!(Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&root)
        .status()
        .unwrap()
        .success());
    am(&root, &["init"]);
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/context_runtime.py");
    // A valid multi-component path longer than each text excerpt under pressure.
    let artifact = format!("{}result.txt", "nested-directory/".repeat(8));
    fs::create_dir_all(root.join(&artifact).parent().unwrap()).unwrap();
    fs::write(root.join(&artifact), "artifact bytes\n").unwrap();
    fs::write(root.join("expected-path.txt"), &artifact).unwrap();
    for (id, role) in [("lead", "reasoner"), ("worker", "worker")] {
        let mut args = vec![
            "agent",
            "add",
            id,
            "--role",
            role,
            "--adapter",
            "codex-exec",
        ];
        if id == "worker" {
            args.extend(["--artifact", artifact.as_str()]);
        }
        args.extend(["--", "python3", fixture.to_str().unwrap(), id]);
        am(&root, &args);
    }
    let result = am(
        &root,
        &["run", "--json", "exercise the full default task budget"],
    );
    assert_eq!(result["status"], "succeeded");
    assert_eq!(result["task_refs"].as_array().unwrap().len(), 32);
    assert_eq!(result["artifact_refs"].as_array().unwrap().len(), 32);
    let payload = fs::read_to_string(root.join("delivered-context.json")).unwrap();
    assert!(payload.len() <= 32554);
    let context: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert!(context["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|result| result["summary"]
            .as_str()
            .unwrap()
            .ends_with(" [truncated]")));
    assert_eq!(context["artifacts"], result["artifact_refs"]);
    // Fresh-process inspection reconstructs the selected owners and digests.
    let final_result = am(&root, &["final", "--json"]);
    assert_eq!(final_result["answer"], result["answer"]);
    let artifacts = am(&root, &["artifact", "--json"]);
    assert_eq!(artifacts["artifacts"], result["artifact_refs"]);
    use agentmosaic_team::TaskBoard;
    let board = agentmosaic_storage::SqliteTaskBoard::open(
        rusqlite::Connection::open(root.join(".agentmosaic/state.db")).unwrap(),
    )
    .unwrap();
    let (tasks, selected) = board.final_refs(1).unwrap();
    assert_eq!(tasks.len(), 32);
    assert_eq!(selected.len(), 32);
    assert!(
        selected
            .iter()
            .all(|reference| tasks.contains(&reference.task_id)
                && reference.artifact.path == artifact)
    );
    drop(board);
    fs::remove_dir_all(root).unwrap();
}
