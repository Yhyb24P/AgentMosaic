//! Transport regression for the bounded Codex event queue.
//!
//! The mock app-server emits a `turn/completed` notification immediately before
//! the correlated `thread/read` response. On the old drop-notifications code
//! that event was lost; the queue must replay it.

use std::time::Duration;

use agentmosaic_runtime::{
    CodexAppServer, CodexBridgeError, CodexBridgeEvent, LaunchSpec, DEFAULT_FINAL_MESSAGE_MAX_BYTES,
};

const MOCK: &str = env!("CARGO_BIN_EXE_codex_bridge_mock");

#[test]
fn notification_racing_a_pending_request_is_queued_and_replayed() {
    let mut client = CodexAppServer::spawn(MOCK).expect("spawn mock app-server");
    client
        .initialize("agentmosaic-codex-bridge-test", "0.1")
        .unwrap();
    let thread = client.start_thread("/tmp").unwrap();
    let turn = client.start_turn(&thread, "mock turn").unwrap();

    // This request's loop consumes the notification that arrives before the
    // response; it must be retained rather than dropped.
    let summary = client.final_agent_message(&thread, &turn).unwrap();
    assert_eq!(summary, "mock final answer");
    assert!(summary.len() <= DEFAULT_FINAL_MESSAGE_MAX_BYTES);

    match client.next_event().unwrap() {
        CodexBridgeEvent::TurnCompleted { thread_id, turn_id } => {
            assert_eq!(thread_id, thread);
            assert_eq!(turn_id, turn);
        }
        other => panic!("expected queued TurnCompleted, got {other:?}"),
    }

    client.close().unwrap();
}

#[test]
fn silent_peer_fails_at_the_absolute_request_deadline() {
    let mut client = CodexAppServer::spawn_launch_with_timeout(
        LaunchSpec::new(MOCK, Vec::new()).unwrap(),
        &["codex_bridge_mock.silent=true".into()],
        Duration::from_millis(100),
    )
    .unwrap();
    let error = client.initialize("test", "0").unwrap_err();
    assert!(matches!(error, CodexBridgeError::Io(message) if message.contains("deadline")));
    client.close().unwrap();
}
