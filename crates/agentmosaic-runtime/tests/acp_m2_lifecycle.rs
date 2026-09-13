//! M2-B3 mock-backed lifecycle tests for `AcpWorkerDriver`.
//!
//! No credentials, no live runtime. The `acp_m2_mock` binary exercises
//! bounded-prompt semantics, a follow-up path, a timeout-and-child-cleanup
//! path, and a crash path.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agentmosaic_runtime::{
    AcpCancellation, AcpWorkerConfig, AcpWorkerDriver, AcpWorkerError, PersistedAcpWorkerDriver,
};
use agentmosaic_storage::SqliteTaskBoard;
use agentmosaic_team::{AgentDriver, AgentTask, TaskAttempt, TaskBoard, TaskKind, TaskStatus};
use rusqlite::Connection;

const MOCK: &str = env!("CARGO_BIN_EXE_acp_m2_mock");

fn mock_cwd(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "acp_m2_mock_{}_{}_{}",
        name,
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create mock workdir");
    dir
}

fn mock_pid_file(dir: &std::path::Path) -> PathBuf {
    dir.join("mock.pid")
}

fn mock_args(dir: &std::path::Path, mode: &str) -> Vec<String> {
    vec![
        "--mode".into(),
        mode.into(),
        "--pid-file".into(),
        mock_pid_file(dir).display().to_string(),
    ]
}

fn valid_config(dir: &std::path::Path, mode: &str) -> AcpWorkerConfig {
    AcpWorkerConfig {
        runtime_kind: "acp-m2-mock".into(),
        command: PathBuf::from(MOCK),
        args: mock_args(dir, mode),
        auth_method: None,
        working_directory: dir.to_path_buf(),
        timeout: Duration::from_secs(10),
        max_prompt_bytes: 1024,
        max_result_bytes: 4096,
        artifact_paths: Vec::new(),
    }
}

fn task_for(id: u64) -> AgentTask {
    AgentTask {
        id,
        objective: "acp m2 mock lifecycle".into(),
        kind: TaskKind::Tool,
        context: vec![],
    }
}

#[tokio::test]
async fn readiness_probe_initializes_and_opens_a_session_without_a_prompt() {
    let cwd = mock_cwd("readiness");
    let driver = AcpWorkerDriver::new(valid_config(&cwd, "sync")).expect("valid mock driver");
    driver
        .probe_readiness()
        .await
        .expect("mock accepts a safe initialize and session check");
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn invalid_configs_are_rejected_before_spawn() {
    let cwd = mock_cwd("invalid");
    let mut cfg = valid_config(&cwd, "sync");

    let mut variants: Vec<AcpWorkerConfig> = Vec::new();
    cfg.runtime_kind = "   ".into();
    variants.push(cfg.clone());
    cfg.runtime_kind = "acp-m2-mock".into();

    cfg.command = PathBuf::from("");
    variants.push(cfg.clone());
    cfg.command = PathBuf::from(MOCK);

    cfg.working_directory = std::env::temp_dir().join("acp_m2_missing_cwd_xyz");
    variants.push(cfg.clone());
    cfg.working_directory = cwd.clone();

    cfg.timeout = Duration::ZERO;
    variants.push(cfg.clone());
    cfg.timeout = Duration::from_secs(10);

    cfg.max_prompt_bytes = 0;
    variants.push(cfg.clone());
    cfg.max_prompt_bytes = 1024;

    cfg.max_result_bytes = 0;
    variants.push(cfg.clone());
    cfg.max_result_bytes = 4096;

    cfg.artifact_paths = vec![PathBuf::from("/tmp/acp_m2_abs_art.txt")];
    variants.push(cfg.clone());

    cfg.artifact_paths = vec![PathBuf::from("")];
    variants.push(cfg.clone());

    cfg.artifact_paths = vec![PathBuf::from("../escape/art.txt")];
    variants.push(cfg.clone());

    for variant in &variants {
        let result = AcpWorkerDriver::new(variant.clone());
        assert!(
            matches!(result, Err(AcpWorkerError::InvalidConfig(_))),
            "expected InvalidConfig, got {result:?}"
        );
    }

    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn session_binding_observer_runs_before_a_successful_prompt() {
    let cwd = mock_cwd("session-observer");
    let driver = AcpWorkerDriver::new(valid_config(&cwd, "sync")).expect("valid mock driver");
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observer_seen = observed.clone();
    let execution = driver
        .execute_task_with_session_observer(
            &task_for(17),
            Arc::new(move |session_id| {
                observer_seen.lock().unwrap().push(session_id.to_string());
                Ok(())
            }),
        )
        .await
        .expect("mock task succeeds after session observer");
    assert_eq!(observed.lock().unwrap().as_slice(), ["acp-m2-mock-session"]);
    assert_eq!(execution.external_session_id, "acp-m2-mock-session");
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn invalid_first_peer_result_gets_one_same_session_strict_repair() {
    let cwd = mock_cwd("strict-repair");
    let driver = AcpWorkerDriver::new(valid_config(&cwd, "repair")).expect("valid mock driver");
    let execution = driver
        .execute_task(&task_for(18))
        .await
        .expect("one bounded same-session repair returns strict result");
    assert_eq!(execution.external_session_id, "acp-m2-mock-session");
    assert!(execution.result.summary.starts_with("mock-ok-"));
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn persisted_scheduler_driver_binds_foreign_session_before_returning_result() {
    let cwd = mock_cwd("persisted-driver");
    let database = cwd.join("team.db");
    let mut board = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
    let task_id = board
        .create_task(
            "mock scheduled task",
            None,
            TaskKind::Tool,
            Some("worker".into()),
        )
        .unwrap();
    board.assign(task_id, "worker").unwrap();
    board
        .record_attempt(&TaskAttempt {
            task_id,
            attempt: 1,
            agent_id: "worker".into(),
            status: TaskStatus::Running,
            result: None,
            error: None,
        })
        .unwrap();
    board.set_status(task_id, TaskStatus::Running).unwrap();
    drop(board);
    let driver =
        PersistedAcpWorkerDriver::new(valid_config(&cwd, "sync"), database.clone(), "worker")
            .unwrap();
    let result = driver.run_task(task_for(task_id)).await.unwrap();
    assert_eq!(result.task_id, task_id);
    let reopened = SqliteTaskBoard::open(Connection::open(&database).unwrap()).unwrap();
    let binding = reopened.external_binding(task_id, 1).unwrap().unwrap();
    assert_eq!(binding.agent_id, "worker");
    assert_eq!(binding.runtime_kind, "acp");
    assert_eq!(binding.lifecycle_state, "completed");
    assert!(binding.native_thread_id.is_some());
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn follow_up_is_same_session_bounded_pull() {
    let cwd = mock_cwd("follow-up");
    let pid_file = mock_pid_file(&cwd);
    let driver = AcpWorkerDriver::new(valid_config(&cwd, "sync")).expect("valid mock driver");

    let result = driver
        .run_with_follow_up(&task_for(1), "second turn")
        .await
        .expect("mock follow-up should succeed");

    assert_eq!(result.external_session_id, "acp-m2-mock-session");
    assert_eq!(result.first_summary, "mock-ok-0");
    assert_eq!(result.follow_up_summary, "mock-ok-1");

    kill_pid_from(&pid_file).unwrap_or_else(|e| {
        eprintln!("[kill-rescue] {e}");
    });
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn hang_is_mapped_to_timed_out() {
    let cwd = mock_cwd("hang");
    let pid_file = mock_pid_file(&cwd);
    let mut cfg = valid_config(&cwd, "hang");
    cfg.timeout = Duration::from_millis(500);
    let driver = AcpWorkerDriver::new(cfg).expect("valid mock driver (hang, 500ms)");

    let task = task_for(3);
    let outcome = driver.run(&task).await;
    assert!(
        matches!(outcome, Err(AcpWorkerError::TimedOut)),
        "expected TimedOut, got {outcome:?}"
    );

    // The driver cleans up its child when the timeout drops the session
    // future, so no live mock should survive the timeout. A backstop SIGKILL
    // covers cleanup that is still in flight; the pid must be gone within
    // the bounded window below.
    let pid = std::fs::read_to_string(&pid_file)
        .expect("mock wrote pid file before hang")
        .trim()
        .to_string();
    let mut gone = !kill0(&pid);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !gone && Instant::now() < deadline {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(&pid)
            .status();
        std::thread::sleep(Duration::from_millis(200));
        gone = !kill0(&pid);
    }
    assert!(gone, "mock was not gone within the 2s post-timeout window");

    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn caller_cancellation_sends_session_cancel_and_requires_peer_confirmation() {
    let cwd = mock_cwd("cancel");
    let pid_file = mock_pid_file(&cwd);
    let driver = AcpWorkerDriver::new(valid_config(&cwd, "cancel-wait"))
        .expect("valid cancellable mock driver");
    let (cancellation, mut listener) = AcpCancellation::new();
    let task = task_for(30);
    let run = tokio::spawn(async move {
        driver
            .execute_task_with_cancellation(&task, &mut listener)
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    cancellation.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("cancelled run should settle")
        .expect("join cancelled run");
    assert!(
        matches!(outcome, Err(AcpWorkerError::Cancelled)),
        "expected peer-confirmed cancellation, got {outcome:?}"
    );

    kill_pid_from(&pid_file).ok();
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn crash_is_mapped_to_protocol_error() {
    let cwd = mock_cwd("crash");
    let pid_file = mock_pid_file(&cwd);
    let cfg = valid_config(&cwd, "crash");
    let driver = AcpWorkerDriver::new(cfg).expect("valid mock driver (crash)");

    let task = task_for(4);
    let outcome = driver.run(&task).await;
    assert!(
        matches!(outcome, Err(AcpWorkerError::Protocol(_))),
        "expected Protocol, got {outcome:?}"
    );

    kill_pid_from(&pid_file).ok();
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn slow_mode_completes_within_bounded_timeout() {
    let cwd = mock_cwd("slow");
    let pid_file = mock_pid_file(&cwd);
    let mut cfg = valid_config(&cwd, "slow");
    cfg.timeout = Duration::from_secs(8);
    let driver = AcpWorkerDriver::new(cfg).expect("valid mock driver (slow, 8s)");

    let task = task_for(5);
    let result = driver
        .run_with_follow_up(&task, "bump")
        .await
        .expect("slow mock should complete within timeout");

    assert_eq!(result.first_summary, "mock-ok-0");
    assert_eq!(result.follow_up_summary, "mock-ok-1");

    kill_pid_from(&pid_file).ok();
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn single_run_returns_session_id_and_raw() {
    let cwd = mock_cwd("single");
    let pid_file = mock_pid_file(&cwd);
    let cfg = valid_config(&cwd, "sync");
    let driver = AcpWorkerDriver::new(cfg).expect("valid mock driver");

    let task = task_for(2);
    let (sid, raw) = driver.run(&task).await.expect("mock run should succeed");

    assert_eq!(sid, "acp-m2-mock-session");
    assert_eq!(raw, r#"{"summary":"mock-ok-0"}"#);

    kill_pid_from(&pid_file).ok();
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn execute_task_collects_relative_artifacts() {
    let cwd = mock_cwd("artifact");
    let pid_file = mock_pid_file(&cwd);
    std::fs::write(cwd.join("out.txt"), b"result bytes\n").expect("write artifact");

    let mut cfg = valid_config(&cwd, "sync");
    cfg.artifact_paths = vec![PathBuf::from("out.txt")];
    let driver = AcpWorkerDriver::new(cfg).expect("valid mock driver (artifacts)");

    let task = task_for(6);
    let execution = driver
        .execute_task(&task)
        .await
        .expect("execute_task should succeed");

    assert_eq!(execution.external_session_id, "acp-m2-mock-session");
    assert_eq!(execution.result.task_id, task.id);
    assert_eq!(execution.result.summary, "mock-ok-0");
    assert_eq!(execution.result.artifacts.len(), 1);
    let artifact = &execution.result.artifacts[0];
    assert_eq!(artifact.path, "out.txt");
    let hash = artifact.sha256.as_str();
    assert_eq!(hash.len(), 64, "non-empty sha256 hex");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "hex hash");

    kill_pid_from(&pid_file).ok();
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn run_task_via_agent_driver_trait() {
    let cwd = mock_cwd("via-driver");
    let pid_file = mock_pid_file(&cwd);
    let cfg = valid_config(&cwd, "sync");
    let driver = AcpWorkerDriver::new(cfg).expect("valid mock driver");

    let task = task_for(8);
    let dyn_driver: Box<dyn AgentDriver> = Box::new(driver);
    let result = dyn_driver
        .run_task(task)
        .await
        .expect("run_task via trait object");

    assert_eq!(result.summary, "mock-ok-0");

    kill_pid_from(&pid_file).ok();
    let _ = std::fs::remove_dir_all(&cwd);
}

#[tokio::test]
async fn result_limit_is_enforced() {
    let cwd = mock_cwd("limit");
    let pid_file = mock_pid_file(&cwd);
    let mut cfg = valid_config(&cwd, "sync");
    cfg.max_result_bytes = 4;
    let driver = AcpWorkerDriver::new(cfg).expect("valid mock driver (limit=4)");

    let task = task_for(9);
    let outcome = driver.execute_task(&task).await;
    assert!(
        matches!(outcome, Err(AcpWorkerError::InvalidPeerResult(_))),
        "expected InvalidPeerResult, got {outcome:?}"
    );

    kill_pid_from(&pid_file).ok();
    let _ = std::fs::remove_dir_all(&cwd);
}

fn kill_pid_from(pid_file: &std::path::Path) -> Result<(), String> {
    let content = std::fs::read_to_string(pid_file).map_err(|e| format!("read pid file: {e}"))?;
    let pid = content.trim();
    if pid.is_empty() {
        return Ok(());
    }
    let status = std::process::Command::new("kill")
        .args(["-9", pid])
        .status()
        .map_err(|e| format!("kill: {e}"))?;
    if !status.success() {
        return Err(format!("kill -9 {pid} failed: {status}"));
    }
    Ok(())
}

fn kill0(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
