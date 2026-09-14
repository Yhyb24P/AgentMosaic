use std::time::Duration;

use agentmosaic_runtime::{CodexExecLeadBrain, CodexExecLeadConfig, LaunchSpec};
use agentmosaic_storage::SqliteTaskBoard;
use agentmosaic_team::{
    LeadBrain, LeadContext, LeadDecision, TaskAttempt, TaskBoard, TaskKind, TaskStatus,
};
use rusqlite::Connection;

#[tokio::test]
#[cfg(unix)]
async fn exec_lead_turn_uses_the_shared_strict_decision_contract() {
    let root = std::env::current_dir().unwrap();
    let reply = r#"{"action":"delegate","tasks":[{"kind":"bulk","target":"worker","objective":"inspect"}]}"#;
    let mut lead = CodexExecLeadBrain::new(
        CodexExecLeadConfig {
            launch: LaunchSpec::new(
                "sh",
                vec!["-c".into(), format!("printf '%s\\n' '{{\"type\":\"thread.started\",\"thread_id\":\"lead-thread\"}}' '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":{reply:?}}}}}'")],
            )
            .unwrap(),
            working_directory: root,
            max_prompt_bytes: 1024,
            max_answer_bytes: 1024,
            timeout: Duration::from_secs(1),
            isolate: false,
            binding_database: None,
            binding_agent_id: None,
        },
        vec!["worker".into()],
    )
    .unwrap();
    let decision = lead
        .decide(&LeadContext {
            root_task_id: 1,
            objective: "test".into(),
            round: 0,
            candidates: vec!["worker".into()],
            results: Vec::new(),
            artifacts: Vec::new(),
            failures: Vec::new(),
            messages: Vec::new(),
        })
        .await
        .unwrap();
    assert!(
        matches!(decision, LeadDecision::Delegate(tasks) if tasks.len() == 1 && tasks[0].target.as_deref() == Some("worker"))
    );
}

#[tokio::test]
#[cfg(unix)]
async fn rejected_reply_is_repaired_once_through_exec_resume() {
    let root = std::env::current_dir().unwrap();
    let script = r#"if [ "$1" = resume ]; then printf '%s\n' '{"type":"thread.started","thread_id":"lead-thread"}' '{"type":"item.completed","item":{"type":"agent_message","text":"{\"action\":\"delegate\",\"tasks\":[{\"kind\":\"tool\",\"target\":\"worker\",\"objective\":\"repair\"}]}"}}'; else printf '%s\n' '{"type":"thread.started","thread_id":"lead-thread"}' '{"type":"item.completed","item":{"type":"agent_message","text":"{}"}}'; fi"#.to_owned();
    let mut lead = CodexExecLeadBrain::new(
        CodexExecLeadConfig {
            launch: LaunchSpec::new("sh", vec!["-c".into(), script]).unwrap(),
            working_directory: root,
            max_prompt_bytes: 1024,
            max_answer_bytes: 1024,
            timeout: Duration::from_secs(1),
            isolate: false,
            binding_database: None,
            binding_agent_id: None,
        },
        vec!["worker".into()],
    )
    .unwrap();
    let decision = lead
        .decide(&LeadContext {
            root_task_id: 1,
            objective: "test".into(),
            round: 0,
            candidates: vec!["worker".into()],
            results: Vec::new(),
            artifacts: Vec::new(),
            failures: Vec::new(),
            messages: Vec::new(),
        })
        .await
        .unwrap();
    assert!(matches!(decision, LeadDecision::Delegate(tasks) if tasks[0].objective == "repair"));
}

#[tokio::test]
#[cfg(unix)]
async fn exec_lead_persists_its_foreign_thread_on_the_running_root_attempt() {
    let root = std::env::temp_dir().join(format!("am_exec_lead_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let database = root.join("board.db");
    let task = {
        let mut board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
        let task = board
            .create_task("root", None, TaskKind::Reasoning, Some("lead".into()))
            .unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: task,
                attempt: 1,
                agent_id: "lead".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(task, TaskStatus::Running).unwrap();
        task
    };
    let mut lead = CodexExecLeadBrain::new(CodexExecLeadConfig {
        launch: LaunchSpec::new("sh", vec!["-c".into(), "printf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"lead-thread\"}' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"{\\\"action\\\":\\\"delegate\\\",\\\"tasks\\\":[{\\\"kind\\\":\\\"bulk\\\",\\\"target\\\":\\\"worker\\\",\\\"objective\\\":\\\"work\\\"}]}\"}}'".into()]).unwrap(),
        working_directory: root.clone(), max_prompt_bytes: 1024, max_answer_bytes: 1024, timeout: Duration::from_secs(1), isolate: false,
        binding_database: Some(database.clone()), binding_agent_id: Some("lead".into()),
    }, vec!["worker".into()]).unwrap();
    lead.decide(&LeadContext {
        root_task_id: task,
        objective: "root".into(),
        round: 0,
        candidates: vec!["worker".into()],
        results: Vec::new(),
        artifacts: Vec::new(),
        failures: Vec::new(),
        messages: Vec::new(),
    })
    .await
    .unwrap();
    let board = SqliteTaskBoard::open(Connection::open(database).unwrap()).unwrap();
    assert_eq!(
        board
            .external_binding(task, 1)
            .unwrap()
            .unwrap()
            .native_thread_id
            .as_deref(),
        Some("lead-thread")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
#[cfg(unix)]
async fn new_exec_lead_instance_resumes_the_root_binding() {
    let root = std::env::temp_dir().join(format!("am_exec_resume_{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let database = root.join("board.db");
    let task = {
        let mut board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
        let task = board
            .create_task("root", None, TaskKind::Reasoning, Some("lead".into()))
            .unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: task,
                attempt: 1,
                agent_id: "lead".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(task, TaskStatus::Running).unwrap();
        board
            .upsert_external_binding(&agentmosaic_storage::ExternalRuntimeBinding {
                team_task_id: task,
                attempt: 1,
                agent_id: "lead".into(),
                runtime_kind: "codex-exec".into(),
                native_thread_id: Some("saved-thread".into()),
                native_turn_id: None,
                lifecycle_state: "running".into(),
            })
            .unwrap();
        task
    };
    let script = r#"if [ "$1" = resume ]; then printf '%s\n' '{"type":"thread.started","thread_id":"saved-thread"}' '{"type":"item.completed","item":{"type":"agent_message","text":"{\"action\":\"delegate\",\"tasks\":[{\"kind\":\"bulk\",\"target\":\"worker\",\"objective\":\"resumed\"}]}"}}'; else exit 9; fi"#;
    let mut lead = CodexExecLeadBrain::new(
        CodexExecLeadConfig {
            launch: LaunchSpec::new("sh", vec!["-c".into(), script.into()]).unwrap(),
            working_directory: root.clone(),
            max_prompt_bytes: 1024,
            max_answer_bytes: 1024,
            timeout: Duration::from_secs(1),
            isolate: false,
            binding_database: Some(database),
            binding_agent_id: Some("lead".into()),
        },
        vec!["worker".into()],
    )
    .unwrap();
    let decision = lead
        .decide(&LeadContext {
            root_task_id: task,
            objective: "root".into(),
            round: 0,
            candidates: vec!["worker".into()],
            results: Vec::new(),
            artifacts: Vec::new(),
            failures: Vec::new(),
            messages: Vec::new(),
        })
        .await
        .unwrap();
    assert!(matches!(decision, LeadDecision::Delegate(tasks) if tasks[0].objective == "resumed"));
    let _ = std::fs::remove_dir_all(root);
}
