use std::process::Command;

use agentmosaic_storage::{ExternalRuntimeBinding, SqliteTaskBoard};
use agentmosaic_team::{
    AgentTaskResult, ArtifactMeta, TaskAttempt, TaskBoard, TaskKind, TaskStatus,
};
use rusqlite::Connection;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_am"))
}

#[test]
fn public_interface_is_am() {
    let version = cli().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(String::from_utf8_lossy(&version.stdout), "am 0.3.0\n");

    // The default help centers the normal path.
    let help = cli().arg("--help").output().unwrap();
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    for command in [
        "init", "agent", "doctor", "run", "status", "final", "artifact", "tui", "advanced",
    ] {
        assert!(
            help_text.contains(command),
            "help must document `{command}`"
        );
    }
    // The 13 compatibility commands stay callable but are not advertised.
    let compatibility = [
        "register",
        "registry",
        "run-acp",
        "continue-acp",
        "run-team",
        "resume-team",
        "submit",
        "cancel",
        "override",
        "recover",
        "recover-all",
        "resume",
        "binding",
    ];
    for command in compatibility {
        assert!(
            !help_text.contains(command),
            "`{command}` must not be advertised by the default help"
        );
    }

    // `am advanced` is where they are named.
    let advanced = cli().arg("advanced").output().unwrap();
    assert!(advanced.status.success());
    let advanced_text = String::from_utf8_lossy(&advanced.stdout);
    for command in compatibility {
        assert!(
            advanced_text.contains(command),
            "`am advanced` must list `{command}`"
        );
    }

    let invalid = cli().arg("definitely-not-a-command").output().unwrap();
    assert!(!invalid.status.success());
}

#[test]
fn tui_rejects_an_unopenable_database() {
    let missing = std::env::temp_dir().join("agentmosaic-missing-dir/board.db");
    let output = cli()
        .args(["tui", &missing.to_string_lossy()])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn normal_path_reads_and_controls_the_authoritative_board() {
    let database = std::env::temp_dir().join(format!(
        "agentmosaic_cli_normal_{}_{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let mut board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
        let task = board
            .create_task("deliver exact result", None, TaskKind::Bulk, None)
            .unwrap();
        board.assign(task, "worker").unwrap();
        let attempt = TaskAttempt {
            task_id: task,
            attempt: 1,
            agent_id: "worker".into(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        };
        board.record_attempt(&attempt).unwrap();
        board
            .commit_successful_result(
                &TaskAttempt {
                    status: TaskStatus::Succeeded,
                    result: Some("done".into()),
                    ..attempt
                },
                &AgentTaskResult {
                    task_id: task,
                    summary: "done".into(),
                    artifacts: vec![ArtifactMeta {
                        path: "result.txt".into(),
                        sha256: "hash".into(),
                    }],
                    message: None,
                },
            )
            .unwrap();
        let interrupted = board
            .create_task("recover explicitly", None, TaskKind::Bulk, None)
            .unwrap();
        board.assign(interrupted, "worker").unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: interrupted,
                attempt: 1,
                agent_id: "worker".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(interrupted, TaskStatus::Running).unwrap();
    }
    let db = database.to_string_lossy().into_owned();
    let status = cli().args(["status", &db]).output().unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("status=succeeded"));
    assert!(String::from_utf8_lossy(&status.stdout).contains("attempts=1"));
    let artifact = cli().args(["artifact", &db, "1"]).output().unwrap();
    assert!(artifact.status.success());
    assert!(String::from_utf8_lossy(&artifact.stdout).contains("sha256=hash"));
    let final_result = cli().args(["final", &db, "1"]).output().unwrap();
    assert!(final_result.status.success());
    assert_eq!(String::from_utf8_lossy(&final_result.stdout), "done\n");
    let recover = cli().args(["recover", &db, "2"]).output().unwrap();
    assert!(recover.status.success());
    assert!(String::from_utf8_lossy(&recover.stdout).contains("interrupted_attempt=1"));
    let recovered_status = cli().args(["status", &db]).output().unwrap();
    assert!(String::from_utf8_lossy(&recovered_status.stdout).contains("task=2 status=failed"));
    let second_interrupted = {
        let mut board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
        let task = board
            .create_task("recover all explicitly", None, TaskKind::Utility, None)
            .unwrap();
        board.assign(task, "utility").unwrap();
        board
            .record_attempt(&TaskAttempt {
                task_id: task,
                attempt: 1,
                agent_id: "utility".into(),
                status: TaskStatus::Running,
                result: None,
                error: None,
            })
            .unwrap();
        board.set_status(task, TaskStatus::Running).unwrap();
        board
            .upsert_external_binding(&ExternalRuntimeBinding {
                team_task_id: task,
                attempt: 1,
                agent_id: "utility".into(),
                runtime_kind: "native".into(),
                native_thread_id: None,
                native_turn_id: None,
                lifecycle_state: "running".into(),
            })
            .unwrap();
        task
    };
    let recover_all = cli().args(["recover-all", &db]).output().unwrap();
    assert!(recover_all.status.success());
    assert!(
        String::from_utf8_lossy(&recover_all.stdout).contains(&format!("{second_interrupted}:1"))
    );
    let recovered_board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
    assert_eq!(
        recovered_board
            .external_binding(second_interrupted, 1)
            .unwrap()
            .unwrap()
            .lifecycle_state,
        "interrupted"
    );
    assert!(cli().args(["resume", &db, "2"]).status().unwrap().success());
    assert!(cli().args(["cancel", &db, "1"]).status().unwrap().success());
    assert!(cli().args(["resume", &db, "1"]).status().unwrap().success());
    assert!(cli()
        .args(["override", &db, "1", "worker-override"])
        .status()
        .unwrap()
        .success());
    let final_status = cli().args(["status", &db]).output().unwrap();
    let text = String::from_utf8_lossy(&final_status.stdout);
    assert!(text.contains("status=assigned"));
    assert!(text.contains("assignee=worker-override"));
    let _ = std::fs::remove_file(database);
}
