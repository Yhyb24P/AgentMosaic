# Pinned upstream source research — Codex final-result fidelity

Researched: 2026-09-13 (RC repair R0/R2)

Reference runtime (installed and pinned):

```text
codex-cli 0.154.0
tag      rust-v0.154.0
commit   6b9826e3aa83b1a5947db50f4332cb9c65f1b340
```

Source obtained from `https://github.com/openai/codex` by shallow fetch of exactly
commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`, checked out read-only-like into
`target/upstream-ref/codex` (gitignored build area, never committed).

## `thread/read` (exact upstream shape)

`codex-rs/app-server-protocol/src/protocol/v2/thread.rs:1667`

```text
ThreadReadParams {
    thread_id: String,       // wire: "threadId"
    include_turns: bool,     // wire: "includeTurns"  (rename_all = camelCase, serde default)
}
ThreadReadResponse { thread: Thread }
```

`codex-rs/app-server-protocol/src/protocol/v2/thread_data.rs:204` — `Thread` carries
`turns: Vec<Turn>` (populated only when `includeTurns` is true).

`thread_data.rs:386`:

```text
Turn {
    id: String,
    items: Vec<ThreadItem>,
    items_view: TurnItemsView,   // camelCase, default "full"
    status: TurnStatus,          // completed | interrupted | failed | inProgress
    error: Option<TurnError>,
    started_at / completed_at / duration_ms
}
```

`codex-rs/app-server-protocol/src/protocol/v2/item.rs:230`:

```text
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ThreadItem { ... }
```

`item.rs:250` — the completed visible response:

```text
AgentMessage {
    id: String,
    text: String,
    phase: Option<MessagePhase>,        // camelCase; "commentary" | "finalAnswer"
    memory_citation: Option<...>,
    delivery: Option<AgentMessageDelivery>,  // camelCase; "async"
    questions: Option<...>,
}
```

`codex-rs/protocol/src/models.rs:914`:

```text
MessagePhase { Commentary, FinalAnswer }
// FinalAnswer == "The assistant's terminal answer text for the current turn."
```

## Implemented selection rule (R2)

```text
TurnCompleted notification -> thread_id / turn_id
  -> thread/read { threadId, includeTurns: true }
  -> select the exact turn whose id == turn_id
  -> require turn.status == completed (interrupted/failed -> fail closed)
  -> among the turn's items, choose the last non-empty AgentMessage
     preferring phase == finalAnswer, skipping delivery == async
  -> otherwise fail closed
  -> bound and sanitize the visible text
```

Reasoning items, item deltas and raw transcript internals are never used as the
final answer.

## Reliability finding

`crates/agent-code-runtime/src/codex_app_server.rs` `request()` / `request_ack()`
deliberately drop notifications received while a correlated response is pending.
A `turn/completed` notification racing any pending request would be lost, so the
queue addition in R2 is required, with a regression test.
