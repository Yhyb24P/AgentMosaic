use std::sync::{Arc, Barrier};

use agentmosaic_storage::{
    ExtendedExternalRuntimeBinding, ExternalRuntimeBinding, RuntimeEventStoreError,
    SqliteTaskBoard, MAX_RUNTIME_EVENT_QUERY,
};
use agentmosaic_team::{
    RuntimeEvent, RuntimeEventPolicy, RuntimeEventRecord, TaskAttempt, TaskBoard, TaskKind,
    TaskStatus, MAX_DURABLE_RUNTIME_PAYLOAD_BYTES,
};
use rusqlite::Connection;

fn path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "agentmosaic_runtime_events_{name}_{}.db",
        std::process::id()
    ))
}

fn seed_attempt(path: &std::path::Path) -> u64 {
    let mut board = SqliteTaskBoard::open(Connection::open(path).unwrap()).unwrap();
    let task = board
        .create_task("observe runtime", None, TaskKind::Bulk, None)
        .unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id: task,
            attempt: 1,
            agent_id: "worker".into(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })
        .unwrap();
    task
}

fn record(task_id: u64, event: RuntimeEvent) -> RuntimeEventRecord {
    RuntimeEventRecord {
        task_id,
        attempt: 1,
        agent_id: "worker".into(),
        runtime_name: Some("fake-acp".into()),
        native_session_id: Some("session".into()),
        event,
    }
}

#[test]
fn extended_binding_and_bounded_events_survive_reopen() {
    let path = path("roundtrip");
    let _ = std::fs::remove_file(&path);
    let task = seed_attempt(&path);
    {
        let mut board = SqliteTaskBoard::open(Connection::open(&path).unwrap()).unwrap();
        assert_eq!(board.schema_version().unwrap(), 12);
        board
            .upsert_external_binding_extended(&ExtendedExternalRuntimeBinding {
                binding: ExternalRuntimeBinding {
                    team_task_id: task,
                    attempt: 1,
                    agent_id: "worker".into(),
                    runtime_kind: "acp".into(),
                    native_thread_id: Some("session".into()),
                    native_turn_id: None,
                    lifecycle_state: "running".into(),
                },
                runtime_name: Some("fake-acp".into()),
                runtime_version: Some("1.0".into()),
                protocol_kind: Some("acp".into()),
                protocol_version: Some("1".into()),
                capabilities_json: Some(r#"{"resume":true}"#.into()),
                started_at: Some("start".into()),
                finished_at: None,
            })
            .unwrap();
        let first = board
            .append_runtime_event(record(
                task,
                RuntimeEvent::SessionStarted {
                    native_session_id: "session".into(),
                },
            ))
            .unwrap();
        let second = board
            .append_runtime_event(record(
                task,
                RuntimeEvent::AssistantMessageCompleted {
                    text: "界".repeat(20_000),
                },
            ))
            .unwrap();
        assert_eq!((first.sequence, second.sequence), (1, 2));
        assert!(
            serde_json::to_vec(&second.record).unwrap().len() <= MAX_DURABLE_RUNTIME_PAYLOAD_BYTES
        );
    }

    let board = SqliteTaskBoard::open(Connection::open(&path).unwrap()).unwrap();
    let binding = board.external_binding_extended(task, 1).unwrap().unwrap();
    assert_eq!(binding.runtime_name.as_deref(), Some("fake-acp"));
    assert_eq!(binding.protocol_version.as_deref(), Some("1"));
    let events = board.runtime_events(task, 1, 0, 10).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].record.event.policy(), RuntimeEventPolicy::Durable);
    assert!(events[1].created_at.parse::<u64>().is_ok());
    assert_eq!(board.latest_runtime_events(task, 1).unwrap().len(), 1);
    let _ = std::fs::remove_file(path);
}

#[test]
fn live_only_and_missing_attempt_events_fail_closed() {
    let path = path("fail_closed");
    let _ = std::fs::remove_file(&path);
    let task = seed_attempt(&path);
    let mut board = SqliteTaskBoard::open(Connection::open(&path).unwrap()).unwrap();
    let live = board
        .append_runtime_event(record(
            task,
            RuntimeEvent::AssistantMessageDelta {
                text: "partial".into(),
            },
        ))
        .unwrap_err();
    assert!(matches!(live, RuntimeEventStoreError::LiveOnly(_)));

    let mut missing = record(
        task,
        RuntimeEvent::RuntimeError {
            code: None,
            message: "failed".into(),
        },
    );
    missing.attempt = 2;
    assert!(matches!(
        board.append_runtime_event(missing).unwrap_err(),
        RuntimeEventStoreError::MissingAttempt { .. }
    ));
    assert!(board
        .runtime_events(task, 1, 0, MAX_RUNTIME_EVENT_QUERY + 1)
        .unwrap()
        .is_empty());
    let _ = std::fs::remove_file(path);
}

#[test]
fn overlapping_writers_allocate_one_monotonic_sequence() {
    let path = path("concurrent");
    let _ = std::fs::remove_file(&path);
    let task = seed_attempt(&path);
    let writers = 12;
    let barrier = Arc::new(Barrier::new(writers));
    let handles = (0..writers)
        .map(|number| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut board = SqliteTaskBoard::open(Connection::open(path).unwrap()).unwrap();
                barrier.wait();
                board
                    .append_runtime_event(record(
                        task,
                        RuntimeEvent::RuntimeWarning {
                            code: Some(format!("writer-{number}")),
                            message: "overlap".into(),
                        },
                    ))
                    .unwrap()
                    .sequence
            })
        })
        .collect::<Vec<_>>();
    let mut allocated = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    allocated.sort_unstable();
    assert_eq!(allocated, (1..=writers as u64).collect::<Vec<_>>());
    let board = SqliteTaskBoard::open(Connection::open(&path).unwrap()).unwrap();
    assert_eq!(
        board.runtime_events(task, 1, 0, writers).unwrap().len(),
        writers
    );
    let _ = std::fs::remove_file(path);
}
