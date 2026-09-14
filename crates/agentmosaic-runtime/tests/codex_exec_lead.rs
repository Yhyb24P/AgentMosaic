use std::time::Duration;

use agentmosaic_runtime::{CodexExecLeadBrain, CodexExecLeadConfig, LaunchSpec};
use agentmosaic_team::{LeadBrain, LeadContext, LeadDecision};

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
