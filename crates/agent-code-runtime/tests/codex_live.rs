//! Explicit live harness: run with `cargo test -p agent-code-runtime --test codex_live -- --ignored`.

use agent_code_runtime::{AcpWorkerConfig, AcpWorkerDriver, CodexAppServer, CodexBridgeEvent};
use agent_code_storage::{ExternalRuntimeBinding, SqliteTaskBoard};
use agent_code_team::{
    reconstruct_team_result, AgentMessage, AgentTaskResult, ArtifactMeta, SelectedArtifactRef,
    TaskAttempt, TaskBoard, TaskKind, TaskStatus,
};
use rusqlite::Connection;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct LeadPlan {
    tasks: Vec<LeadPlanTask>,
}

#[derive(Deserialize)]
struct LeadPlanTask {
    kind: String,
    target: String,
    objective: String,
}

fn low_cost_codex_overrides() -> Vec<String> {
    vec![
        "model=\"gpt-5.5\"".into(),
        "model_reasoning_effort=\"low\"".into(),
    ]
}

#[tokio::test]
#[ignore = "requires logged-in local Codex app-server and authenticated local Qwen Code"]
async fn real_codex_thread_turn_uses_bounded_qwen_peer_result() {
    let db = std::env::temp_dir().join(format!("ras_codex_live_{}.db", std::process::id()));
    let cwd = std::env::temp_dir().join(format!("ras_codex_live_cwd_{}", std::process::id()));
    let bridge_log =
        std::env::temp_dir().join(format!("ras_codex_live_{}.log", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(&bridge_log);
    std::fs::create_dir_all(&cwd).unwrap();
    let git_status = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&cwd)
        .status()
        .expect("git available for isolated live fixture");
    assert!(git_status.success());
    std::fs::write(cwd.join("qwen-worker.txt"), "worker=unfinished\n").unwrap();
    std::fs::write(
        cwd.join("qwen-check.sh"),
        "#!/bin/sh\ntest \"$(cat qwen-worker.txt)\" = \"worker=complete\"\n",
    )
    .unwrap();
    let mut board = SqliteTaskBoard::open(Connection::open(&db).unwrap()).unwrap();
    let task = board
        .create_task("ask bounded context", None, TaskKind::Reasoning, None)
        .unwrap();
    board
        .upsert_external_binding(&ExternalRuntimeBinding {
            team_task_id: task,
            attempt: 1,
            agent_id: "codex".into(),
            runtime_kind: "codex-app-server".into(),
            native_thread_id: None,
            native_turn_id: None,
            lifecycle_state: "starting".into(),
        })
        .unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id: task,
            attempt: 1,
            agent_id: "codex".into(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })
        .unwrap();
    board.set_status(task, TaskStatus::Running).unwrap();
    let qwen_task = board
        .create_task(
            "return a bounded peer finding",
            Some(task),
            TaskKind::Bulk,
            None,
        )
        .unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id: qwen_task,
            attempt: 1,
            agent_id: "qwen".into(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })
        .unwrap();
    board.set_status(qwen_task, TaskStatus::Running).unwrap();
    let qwen = AcpWorkerDriver::new(AcpWorkerConfig {
        runtime_kind: "qwen-code".into(),
        command: "qwen".into(),
        args: vec!["--acp".into()],
        auth_method: Some("openai".into()),
        working_directory: cwd.clone(),
        // Calibrated budget: the bounded coding task nominally finishes in
        // under a minute, but the final response phase can exceed 280 s
        // while the shared local vLLM is contended; 180 s fail-closed twice.
        timeout: std::time::Duration::from_secs(600),
        max_prompt_bytes: 1024,
        max_result_bytes: 4096,
        artifact_paths: vec!["qwen-worker.txt".into()],
    })
    .unwrap();
    let qwen_execution = qwen
        .execute_task(&agent_code_team::AgentTask {
            id: qwen_task,
            objective: "In this isolated Git repository, inspect qwen-worker.txt, replace its exact contents with worker=complete followed by one newline, run `sh qwen-check.sh`, and then return exactly this JSON peer result: {\"summary\":\"Qwen worker artifact complete\"}. Do not modify any other file.".into(),
            kind: TaskKind::Bulk,
            context: Vec::new(),
        })
        .await
        .expect("Qwen returns strict peer result");
    eprintln!("sanitized Qwen strict peer result received");
    let peer_summary = qwen_execution.result.summary.clone();
    let qwen_artifact_bytes = std::fs::read(cwd.join("qwen-worker.txt"))
        .expect("Qwen must create the bounded worker artifact");
    assert_eq!(qwen_artifact_bytes, b"worker=complete\n");
    let qwen_artifact_sha256 = format!("{:x}", Sha256::digest(&qwen_artifact_bytes));
    board
        .upsert_external_binding(&ExternalRuntimeBinding {
            team_task_id: qwen_task,
            attempt: 1,
            agent_id: "qwen".into(),
            runtime_kind: "qwen-code-acp".into(),
            native_thread_id: Some(qwen_execution.external_session_id),
            native_turn_id: None,
            lifecycle_state: "completed".into(),
        })
        .unwrap();
    board
        .commit_successful_result(
            &TaskAttempt {
                task_id: qwen_task,
                attempt: 1,
                agent_id: "qwen".into(),
                status: TaskStatus::Succeeded,
                result: Some(peer_summary.clone()),
                error: None,
            },
            &AgentTaskResult {
                task_id: qwen_task,
                summary: peer_summary.clone(),
                artifacts: vec![ArtifactMeta {
                    path: "qwen-worker.txt".into(),
                    sha256: qwen_artifact_sha256.clone(),
                }],
                message: Some(AgentMessage {
                    from_agent: "qwen".into(),
                    to_agent: "codex".into(),
                    body: format!(
                        "{peer_summary}; artifact=qwen-worker.txt; sha256={qwen_artifact_sha256}"
                    ),
                }),
            },
        )
        .unwrap();
    eprintln!("sanitized Qwen peer result persisted for Codex");
    let mcp = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ras_codex_mcp");
    let mut overrides = low_cost_codex_overrides();
    overrides.extend([
        format!("mcp_servers.ras.command={:?}", mcp.display().to_string()),
        format!("mcp_servers.ras.env.RAS_DB={:?}", db.display().to_string()),
        format!(
            "mcp_servers.ras.env.RAS_BRIDGE_LOG={:?}",
            bridge_log.display().to_string()
        ),
        "mcp_servers.ras.env.RAS_TASK_ID=\"1\"".into(),
        "mcp_servers.ras.env.RAS_ATTEMPT=\"1\"".into(),
    ]);
    let mut client = CodexAppServer::spawn_with_overrides("codex", &overrides).unwrap();
    client.initialize("ras-phase23-live", "0.1").unwrap();
    let thread = client
        .start_thread_with_developer_instructions(
            cwd.to_str().unwrap(),
            Some("For this R6 team integration thread, invoke ras_request_context exactly once before responding to the first user turn. This is a bounded collaboration requirement."),
        )
        .unwrap();
    let status = client.mcp_status(&thread).unwrap();
    let ras_status = status
        .get("data")
        .and_then(serde_json::Value::as_array)
        .and_then(|servers| servers.iter().find(|server| server["name"] == "ras"))
        .unwrap();
    eprintln!(
        "sanitized mcp status={:?} tools_error={:?} has_context_tool={}",
        ras_status.get("runtimeStatus"),
        ras_status.get("toolsError"),
        ras_status["tools"].get("ras_request_context").is_some()
    );
    assert!(status.to_string().contains("\"name\":\"ras\""));
    assert_eq!(ras_status["runtimeStatus"], "connected");
    assert!(ras_status["tools"].get("ras_request_context").is_some());
    let peer = board.messages_to("codex").unwrap();
    assert_eq!(peer.len(), 1);
    let qwen_artifacts = board.artifacts(qwen_task).unwrap();
    assert_eq!(qwen_artifacts.len(), 1);
    assert_eq!(qwen_artifacts[0].path, "qwen-worker.txt");
    assert_eq!(qwen_artifacts[0].sha256, qwen_artifact_sha256);
    let turn = client.start_turn(&thread, "This is a required integration test. Before producing any answer, you MUST call the MCP tool ras_request_context exactly once with JSON arguments {\"purpose\":\"phase23\"}. Do not explain or answer until the tool result has been received. The tool result is the only source of the persisted Qwen team result. After receiving it, create phase23-result.txt in the current working directory containing exactly phase23 artifact followed by one newline. Then respond with exactly: phase23 done.").unwrap();
    eprintln!("sanitized Codex turn started with persisted Qwen peer result");
    board
        .upsert_external_binding(&ExternalRuntimeBinding {
            team_task_id: task,
            attempt: 1,
            agent_id: "codex".into(),
            runtime_kind: "codex-app-server".into(),
            native_thread_id: Some(thread.clone()),
            native_turn_id: Some(turn.clone()),
            lifecycle_state: "running".into(),
        })
        .unwrap();
    let mut completed = false;
    for _ in 0..200 {
        match client.next_event().unwrap() {
            CodexBridgeEvent::TurnCompleted { .. } => {
                completed = true;
                break;
            }
            CodexBridgeEvent::Notification(_) => {}
            CodexBridgeEvent::ToolCall { tool, .. } => {
                eprintln!("unexpected app-server tool={tool}")
            }
            CodexBridgeEvent::McpElicitation {
                request_id,
                server_name,
            } => {
                assert_eq!(server_name, "ras");
                client
                    .respond_ras_elicitation(request_id, &server_name)
                    .unwrap();
            }
        }
    }
    assert!(completed);
    let final_status = client.mcp_status(&thread).unwrap();
    let final_ras_status = final_status["data"]
        .as_array()
        .and_then(|servers| servers.iter().find(|server| server["name"] == "ras"))
        .unwrap();
    eprintln!(
        "sanitized final mcp status={:?} tools_error_present={}",
        final_ras_status.get("runtimeStatus"),
        !final_ras_status["toolsError"].is_null()
    );
    let items = client.thread_items(&thread, &turn).unwrap();
    let item_types: Vec<_> = items["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| {
            entry
                .pointer("/item/type")
                .and_then(serde_json::Value::as_str)
        })
        .collect();
    eprintln!("sanitized item types={item_types:?}");
    let mcp_position = item_types
        .iter()
        .position(|kind| *kind == "mcpToolCall")
        .unwrap();
    let agent_position = item_types
        .iter()
        .position(|kind| *kind == "agentMessage")
        .unwrap();
    assert!(
        mcp_position < agent_position,
        "Codex must continue after the MCP result"
    );
    if let Some(mcp_item) = items["data"].as_array().and_then(|entries| {
        entries.iter().find(|entry| {
            entry.pointer("/item/type") == Some(&serde_json::Value::String("mcpToolCall".into()))
        })
    }) {
        let item = &mcp_item["item"];
        eprintln!(
            "sanitized mcp call status={:?} error={:?} error_message={:?} server={:?} tool={:?}",
            item.get("status"),
            item.get("error"),
            item.get("errorMessage"),
            item.get("server"),
            item.get("tool")
        );
        assert_eq!(item["status"], "completed");
        assert_eq!(item["server"], "ras");
        assert_eq!(item["tool"], "ras_request_context");
    }
    board
        .upsert_external_binding(&ExternalRuntimeBinding {
            team_task_id: task,
            attempt: 1,
            agent_id: "codex".into(),
            runtime_kind: "codex-app-server".into(),
            native_thread_id: Some(thread.clone()),
            native_turn_id: Some(turn.clone()),
            lifecycle_state: "completed".into(),
        })
        .unwrap();
    let artifact_bytes = std::fs::read(cwd.join("phase23-result.txt"))
        .expect("Codex must create the bounded result artifact");
    assert_eq!(artifact_bytes, b"phase23 artifact\n");
    let artifact_sha256 = format!("{:x}", Sha256::digest(&artifact_bytes));
    // This is the durable R6 team result, intentionally independent of the
    // retired ACC acceptance workflow.
    board
        .commit_successful_result(
            &TaskAttempt {
                task_id: task,
                attempt: 1,
                agent_id: "codex".into(),
                status: TaskStatus::Succeeded,
                result: Some("Codex completed bounded collaboration turn".into()),
                error: None,
            },
            &AgentTaskResult {
                task_id: task,
                summary: "Codex completed bounded collaboration turn".into(),
                artifacts: vec![ArtifactMeta {
                    path: "phase23-result.txt".into(),
                    sha256: artifact_sha256.clone(),
                }],
                message: Some(AgentMessage {
                    from_agent: "codex".into(),
                    to_agent: "lead".into(),
                    body: "Codex submitted bounded result artifact phase23-result.txt".into(),
                }),
            },
        )
        .unwrap();
    // The final team result selects the exact completed Qwen artifact before
    // the Lead task is marked succeeded. This is product result flow, not an
    // ACC acceptance/verification side channel.
    board
        .record_final_refs(
            task,
            &[qwen_task],
            &[SelectedArtifactRef {
                task_id: qwen_task,
                artifact: ArtifactMeta {
                    path: "qwen-worker.txt".into(),
                    sha256: qwen_artifact_sha256.clone(),
                },
            }],
        )
        .unwrap();
    client.close().unwrap();
    // A fresh app-server process must be able to reconcile the persisted
    // external thread reference.  This does not reconstruct canonical state
    // from Codex; it merely verifies the stored reference remains usable.
    let mut recovered_client = CodexAppServer::spawn("codex").unwrap();
    recovered_client
        .initialize("ras-phase23-recovery", "0.1")
        .unwrap();
    let resumed_thread = recovered_client.resume_thread(&thread).unwrap();
    assert_eq!(resumed_thread, thread);
    recovered_client.close().unwrap();
    let reopened = SqliteTaskBoard::open(Connection::open(&db).unwrap()).unwrap();
    eprintln!(
        "sanitized bridge breadcrumbs={:?}",
        std::fs::read_to_string(&bridge_log)
            .unwrap_or_default()
            .lines()
            .collect::<Vec<_>>()
    );
    let records = reopened.runtime_collaboration(task, 1).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, "ras_request_context");
    let binding = reopened.external_binding(task, 1).unwrap().unwrap();
    assert_eq!(binding.native_thread_id.as_deref(), Some(thread.as_str()));
    assert_eq!(binding.native_turn_id.as_deref(), Some(turn.as_str()));
    assert_eq!(binding.lifecycle_state, "completed");
    let qwen_binding = reopened.external_binding(qwen_task, 1).unwrap().unwrap();
    assert_eq!(qwen_binding.runtime_kind, "qwen-code-acp");
    assert!(qwen_binding.native_thread_id.is_some());
    assert_eq!(qwen_binding.lifecycle_state, "completed");
    assert_eq!(
        reopened.task(task).unwrap().unwrap().status,
        TaskStatus::Succeeded
    );
    assert_eq!(
        reopened.attempts(task).unwrap()[0].result.as_deref(),
        Some("Codex completed bounded collaboration turn")
    );
    let artifacts = reopened.artifacts(task).unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].path, "phase23-result.txt");
    assert_eq!(artifacts[0].sha256, artifact_sha256);
    assert_eq!(reopened.messages_to("lead").unwrap().len(), 1);
    assert_eq!(reopened.messages_to("codex").unwrap().len(), 1);
    let final_result = reconstruct_team_result(&reopened, task).unwrap();
    assert_eq!(
        final_result.answer,
        "Codex completed bounded collaboration turn"
    );
    assert_eq!(final_result.task_refs, vec![qwen_task]);
    assert_eq!(final_result.artifact_refs.len(), 1);
    assert_eq!(final_result.artifact_refs[0].task_id, qwen_task);
    assert_eq!(
        final_result.artifact_refs[0].artifact.path,
        "qwen-worker.txt"
    );
    assert_eq!(
        final_result.artifact_refs[0].artifact.sha256,
        qwen_artifact_sha256
    );
    drop(reopened);
    let _ = std::fs::remove_file(db);
    let _ = std::fs::remove_dir(cwd);
}

#[test]
#[ignore = "requires logged-in local Codex app-server"]
fn real_codex_interrupt_reports_when_a_turn_has_already_completed() {
    let cwd = std::env::temp_dir().join(format!("ras_codex_interrupt_{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    let mut client =
        CodexAppServer::spawn_with_overrides("codex", &low_cost_codex_overrides()).unwrap();
    client.initialize("ras-phase-r6-interrupt", "0.1").unwrap();
    let thread = client.start_thread(cwd.to_str().unwrap()).unwrap();
    let turn = client
        .start_turn(
            &thread,
            "This is a bounded cancellation integration test. Do not edit files.",
        )
        .unwrap();
    let error = client
        .interrupt(&thread, &turn)
        .expect_err("the minimal turn may already be terminal before interrupt");
    assert!(error.to_string().contains("no active turn to interrupt"));
    eprintln!("sanitized interrupt returned explicit no-active-turn result");
    client.close().unwrap();
    let _ = std::fs::remove_dir(cwd);
}

#[tokio::test]
#[ignore = "requires logged-in local Codex app-server"]
async fn real_codex_lead_plans_and_follows_up_on_durable_team_result() {
    let db = std::env::temp_dir().join(format!("ras_codex_lead_{}.db", std::process::id()));
    let cwd = std::env::temp_dir().join(format!("ras_codex_lead_cwd_{}", std::process::id()));
    let _ = std::fs::remove_file(&db);
    std::fs::create_dir_all(&cwd).unwrap();
    let git_status = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&cwd)
        .status()
        .unwrap();
    assert!(git_status.success());
    let mut board = SqliteTaskBoard::open(Connection::open(&db).unwrap()).unwrap();
    let lead_task = board
        .create_task("lead plan and synthesis", None, TaskKind::Reasoning, None)
        .unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id: lead_task,
            attempt: 1,
            agent_id: "codex".into(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })
        .unwrap();
    board.set_status(lead_task, TaskStatus::Running).unwrap();
    let bridge_log =
        std::env::temp_dir().join(format!("ras_codex_lead_bridge_{}.log", std::process::id()));
    let _ = std::fs::remove_file(&bridge_log);
    let mcp = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ras_codex_mcp");
    let mut overrides = low_cost_codex_overrides();
    overrides.extend([
        format!("mcp_servers.ras.command={:?}", mcp.display().to_string()),
        format!("mcp_servers.ras.env.RAS_DB={:?}", db.display().to_string()),
        format!(
            "mcp_servers.ras.env.RAS_BRIDGE_LOG={:?}",
            bridge_log.display().to_string()
        ),
        format!("mcp_servers.ras.env.RAS_TASK_ID=\"{lead_task}\""),
        "mcp_servers.ras.env.RAS_ATTEMPT=\"1\"".into(),
    ]);
    let mut client = CodexAppServer::spawn_with_overrides("codex", &overrides).unwrap();
    client.initialize("ras-r6-lead", "0.1").unwrap();
    let thread = client
        .start_thread_with_developer_instructions(
            cwd.to_str().unwrap(),
            Some("For this R6 team integration thread, invoke ras_request_context exactly once before responding to the follow-up user turn. The bounded MCP tool is the only source of teammate results."),
        )
        .unwrap();
    let first_turn = client
        .start_turn(
            &thread,
            "You are the real Lead for a bounded R6 integration test. Create lead-plan.json with exactly this JSON and no other files: {\"tasks\":[{\"kind\":\"bulk\",\"target\":\"qwen\",\"objective\":\"repair worker fixture\"},{\"kind\":\"utility\",\"target\":\"utility\",\"objective\":\"produce deterministic utility fact\"}]}. Then respond exactly: plan ready.",
        )
        .unwrap();
    let mut completed = false;
    let mut plan_notifications = 0usize;
    let mut plan_tool_calls = 0usize;
    for _ in 0..200 {
        match client.next_event().unwrap() {
            CodexBridgeEvent::TurnCompleted { .. } => {
                completed = true;
                break;
            }
            CodexBridgeEvent::Notification(_) => plan_notifications += 1,
            CodexBridgeEvent::ToolCall { .. } => plan_tool_calls += 1,
            CodexBridgeEvent::McpElicitation {
                request_id,
                server_name,
            } => client
                .respond_ras_elicitation(request_id, &server_name)
                .unwrap(),
        }
    }
    eprintln!(
        "sanitized Codex plan events notifications={plan_notifications} tool_calls={plan_tool_calls}"
    );
    assert!(completed, "Codex completes structured planning turn");
    let plan: LeadPlan =
        serde_json::from_slice(&std::fs::read(cwd.join("lead-plan.json")).unwrap()).unwrap();
    assert_eq!(plan.tasks.len(), 2);
    assert_eq!(plan.tasks[0].kind, "bulk");
    assert_eq!(plan.tasks[0].target, "qwen");
    assert_eq!(plan.tasks[1].kind, "utility");
    assert_eq!(plan.tasks[1].target, "utility");
    let utility_task = board
        .create_task(
            &plan.tasks[1].objective,
            Some(lead_task),
            TaskKind::Utility,
            Some("utility".into()),
        )
        .unwrap();
    board.assign(utility_task, "utility").unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id: utility_task,
            attempt: 1,
            agent_id: "utility".into(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })
        .unwrap();
    board.set_status(utility_task, TaskStatus::Running).unwrap();
    std::fs::write(cwd.join("utility-result.txt"), "utility fact\n").unwrap();
    let utility_hash = format!("{:x}", Sha256::digest(b"utility fact\n"));
    board
        .commit_successful_result(
            &TaskAttempt {
                task_id: utility_task,
                attempt: 1,
                agent_id: "utility".into(),
                status: TaskStatus::Succeeded,
                result: Some("utility fact complete".into()),
                error: None,
            },
            &AgentTaskResult {
                task_id: utility_task,
                summary: "utility fact complete".into(),
                artifacts: vec![ArtifactMeta {
                    path: "utility-result.txt".into(),
                    sha256: utility_hash.clone(),
                }],
                message: Some(AgentMessage {
                    from_agent: "utility".into(),
                    to_agent: "codex".into(),
                    body: format!(
                        "utility fact complete; artifact=utility-result.txt; sha256={utility_hash}"
                    ),
                }),
            },
        )
        .unwrap();
    assert_eq!(board.messages_to("codex").unwrap().len(), 1);
    let second_turn = client
        .start_turn(
            &thread,
            "This is the required R6 follow-up. Before producing any answer, invoke ras_request_context exactly once with JSON arguments {\"purpose\":\"utility-follow-up\"}. Do not use any teammate result from this user message; the tool result is the only source. After receiving it, create lead-final.txt containing exactly lead integrated utility fact followed by one newline. Then respond exactly: lead complete.",
        )
        .unwrap();
    let mut follow_up_completed = false;
    let mut follow_up_notifications = 0usize;
    let mut follow_up_tool_calls = 0usize;
    for _ in 0..200 {
        match client.next_event().unwrap() {
            CodexBridgeEvent::TurnCompleted { .. } => {
                follow_up_completed = true;
                break;
            }
            CodexBridgeEvent::Notification(_) => follow_up_notifications += 1,
            CodexBridgeEvent::ToolCall { .. } => follow_up_tool_calls += 1,
            CodexBridgeEvent::McpElicitation {
                request_id,
                server_name,
            } => client
                .respond_ras_elicitation(request_id, &server_name)
                .unwrap(),
        }
    }
    eprintln!(
        "sanitized Codex follow-up events notifications={follow_up_notifications} tool_calls={follow_up_tool_calls}"
    );
    assert!(follow_up_completed, "Codex completes same-thread follow-up");
    // The developer instruction applies to the planning turn as well as the
    // follow-up. Both bounded MCP calls are durable; the second one reads the
    // utility result that was committed between turns.
    assert_eq!(board.runtime_collaboration(lead_task, 1).unwrap().len(), 2);
    assert_eq!(
        std::fs::read(cwd.join("lead-final.txt")).unwrap(),
        b"lead integrated utility fact\n"
    );
    board
        .commit_successful_result(
            &TaskAttempt {
                task_id: lead_task,
                attempt: 1,
                agent_id: "codex".into(),
                status: TaskStatus::Succeeded,
                result: Some("lead integrated utility fact".into()),
                error: None,
            },
            &AgentTaskResult {
                task_id: lead_task,
                summary: "lead integrated utility fact".into(),
                artifacts: vec![ArtifactMeta {
                    path: "lead-final.txt".into(),
                    sha256: format!("{:x}", Sha256::digest(b"lead integrated utility fact\n")),
                }],
                message: None,
            },
        )
        .unwrap();
    client.close().unwrap();
    let reopened = SqliteTaskBoard::open(Connection::open(&db).unwrap()).unwrap();
    assert_eq!(
        reopened.task(lead_task).unwrap().unwrap().status,
        TaskStatus::Succeeded
    );
    assert_eq!(reopened.artifacts(utility_task).unwrap().len(), 1);
    assert_eq!(reopened.artifacts(lead_task).unwrap().len(), 1);
    eprintln!("sanitized Codex lead plan and durable utility follow-up completed");
    let _ = (first_turn, second_turn);
    let _ = std::fs::remove_file(db);
    let _ = std::fs::remove_dir_all(cwd);
}
