//! The public `am events` projection must list every durable observation of a
//! run exactly once.
//!
//! The observation plane is a read-only projection of the board, so a run's
//! subtree must not be walked twice: a duplicated line is a fabricated second
//! observation, and a reader that counts events would over-report the run.
//! Nothing here starts a runtime; the durable facts are written directly and
//! then read back through a fresh `am` process.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use agentmosaic_storage::SqliteTaskBoard;
use agentmosaic_team::{
    RuntimeEvent, RuntimeEventRecord, TaskAttempt, TaskBoard, TaskKind, TaskStatus,
};
use rusqlite::Connection;

fn unique_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "agentmosaic_cli_events_{name}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join(".agentmosaic")).unwrap();
    root
}

fn open(database: &Path) -> SqliteTaskBoard {
    SqliteTaskBoard::open(Connection::open(database).unwrap()).unwrap()
}

/// A durable root reasoning task with one succeeded bulk child, each with one
/// attempt and two persisted observations.
fn seed(database: &Path) -> (u64, u64) {
    let mut board = open(database);
    let root = board
        .create_task(
            "root objective",
            None,
            TaskKind::Reasoning,
            Some("lead".into()),
        )
        .unwrap();
    let child = board
        .create_task(
            "child objective",
            Some(root),
            TaskKind::Bulk,
            Some("worker".into()),
        )
        .unwrap();
    for (task, agent) in [(root, "lead"), (child, "worker")] {
        board
            .record_attempt(&TaskAttempt {
                task_id: task,
                attempt: 1,
                agent_id: agent.into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
    }
    for (task, agent, runtime, session) in [
        (root, "lead", "codex-exec", "lead-thread"),
        (child, "worker", "qwen-code", "worker-session"),
    ] {
        for event in [
            RuntimeEvent::SessionStarted {
                native_session_id: session.into(),
            },
            RuntimeEvent::AssistantMessageCompleted {
                text: format!("{agent} visible message"),
            },
        ] {
            board
                .append_runtime_event(RuntimeEventRecord {
                    task_id: task,
                    attempt: 1,
                    agent_id: agent.into(),
                    runtime_name: Some(runtime.into()),
                    native_session_id: Some(session.into()),
                    event,
                })
                .unwrap();
        }
    }
    (root, child)
}

fn events(root: &Path, target: Option<&str>) -> Vec<(u64, u32, u64)> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_am"));
    command.current_dir(root).arg("events");
    if let Some(target) = target {
        command.arg(target);
    }
    let output = command.arg("--json").output().expect("the CLI runs");
    assert!(
        output.status.success(),
        "am events failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("am events --json is JSON");
    payload["events"]
        .as_array()
        .expect("the projection carries an events array")
        .iter()
        .map(|event| {
            (
                event["task_id"].as_u64().unwrap(),
                event["attempt"].as_u64().unwrap() as u32,
                event["sequence"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn the_latest_run_is_projected_once() {
    let root = unique_root("latest_run");
    let (root_task, child) = seed(&root.join(".agentmosaic/state.db"));

    let projected = events(&root, None);
    let unique: BTreeSet<_> = projected.iter().copied().collect();
    assert_eq!(
        unique.len(),
        projected.len(),
        "the latest run repeated an observation: {projected:?}"
    );
    assert_eq!(
        unique,
        BTreeSet::from([
            (root_task, 1, 1),
            (root_task, 1, 2),
            (child, 1, 1),
            (child, 1, 2)
        ]),
        "the latest run must project every attempt's observations once"
    );
    // Per attempt the durable sequence stays monotonic in the projection.
    for task in [root_task, child] {
        let sequences: Vec<u64> = projected
            .iter()
            .filter(|(task_id, _, _)| *task_id == task)
            .map(|(_, _, sequence)| *sequence)
            .collect();
        assert_eq!(sequences, vec![1, 2], "task {task} lost sequence order");
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_explicit_root_and_an_explicit_child_are_projected_once() {
    let root = unique_root("explicit_target");
    let (root_task, child) = seed(&root.join(".agentmosaic/state.db"));

    let root_projection = events(&root, Some(&root_task.to_string()));
    assert_eq!(
        root_projection.len(),
        BTreeSet::from_iter(root_projection.iter().copied()).len(),
        "an explicit root repeated an observation: {root_projection:?}"
    );
    assert_eq!(
        root_projection.len(),
        4,
        "an explicit root covers its own subtree once: {root_projection:?}"
    );

    let child_projection = events(&root, Some(&child.to_string()));
    assert_eq!(
        child_projection,
        vec![(child, 1, 1), (child, 1, 2)],
        "an explicit child projects only its own observations"
    );

    let _ = std::fs::remove_dir_all(root);
}
