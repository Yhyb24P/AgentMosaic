# R6 Codex live team-result flow (sanitized)

Executed 2026-09-11T17:01:55+08:00:

```text
cargo test -p agent-code-runtime --test codex_live --offline -- --ignored --nocapture
```

Exit code: `0`. Observed: `1 passed; 0 failed; 0 ignored` in 19.47 seconds.

The isolated live harness observed only these protocol/result classifications:

- configured RAS MCP server was connected and exposed `ras_request_context`;
- Codex invoked `ras_request_context` once and the bounded bridge persisted it;
- the MCP tool call completed, and a later `agentMessage` appeared in the same
  Codex turn;
- the live item sequence included `fileChange` after collaboration;
- the isolated workspace contained the exact bounded artifact
  `phase23-result.txt` with the expected bytes;
- its SHA-256 plus a compact result summary and a directed lead message were
  committed through the SQLite result-flow transaction, then verified after
  reopening the board.

No thread id, turn id, tool arguments, prompt text, model message content,
filesystem path, credential, or raw protocol frame is retained here. A
completed runtime turn was recorded as a submitted team result only; it did
not perform ACC acceptance.

## Restart and interrupt follow-up (sanitized)

After the original bridge process closed, a new `codex app-server --stdio`
process initialized and successfully issued `thread/resume` using the opaque
external thread reference. The returned thread reference matched the stored
reference; canonical task/run state was still read from SQLite.

The live suite also exercised `turn/interrupt` against a minimal turn that had
already reached terminal state. The server returned the explicit JSON-RPC
`-32600` error `no active turn to interrupt`. This verifies the client's
correlated error handling for the observed void acknowledgement shape. It is
not evidence of successful cancellation of an active long-running turn.

The combined live suite was rerun after this addition with exit `0`: `2
passed; 0 failed; 0 ignored` in 20.07 seconds. No raw runtime identifiers,
messages, prompts, or paths were retained.

## R6 real Codex Lead plan/follow-up (sanitized)

The ignored probe
`real_codex_lead_plans_and_follows_up_on_durable_team_result` was run on
2026-09-11 with:

```text
cargo test -p agent-code-runtime --test codex_live \
  real_codex_lead_plans_and_follows_up_on_durable_team_result \
  --offline -- --ignored --nocapture
```

Exit code: `0`; observed result: `1 passed; 0 failed; 2 filtered out` in
15.69 seconds. The raw temporary log was moved to local trash after SHA-256
`ea2344d5e735b4668b31cac6abae5e1bafe62cdbffd94e79edfb777a38047010`.

Observed facts:

- real Codex created a structured two-task plan in an isolated Git directory;
- the harness created and completed the deterministic utility task via the
  existing SQLite board transaction, including its artifact hash and directed
  message to Codex;
- the follow-up was supplied from that durable board message to the same Codex
  thread, which created the expected final artifact;
- reopening SQLite confirmed the Lead and Utility artifacts.

An initial 40-event test bound exhausted before `turn/completed`; it exited
101 and did not persist a success. The bounded 200-event retry completed with
32 planning notifications and 31 follow-up notifications; only those counts,
not runtime content, were logged. The first raw-log SHA-256 was
`3c7dcfacfecb42c73de6ccb69b6ca8c2c6aa9213a7b57aef23a44be8bfee4585`.

The same probe was rerun after the live harness was pinned to locally
schema-confirmed app-server overrides `model="gpt-5.5"` and
`model_reasoning_effort="low"`. It exited `0` in 16.46 seconds with 44
planning notifications and 38 follow-up notifications. The raw temporary log
was moved to local trash after SHA-256
`2b8ae5273694c7b3c56600b158aa343206955bf8f10d0132f5f684030d5a96a6`.
