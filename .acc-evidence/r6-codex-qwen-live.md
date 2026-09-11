# R6 Codex ↔ Qwen live bounded collaboration (sanitized)

Executed from an isolated temporary work directory on 2026-09-11:

```text
cargo test -p agent-code-runtime --test codex_live \
  real_codex_thread_turn_uses_bounded_qwen_peer_result \
  --offline -- --ignored --nocapture
```

Exit code: `0`; observed result: `1 passed; 0 failed; 1 filtered out` in
34.67 seconds. The sanitized raw test log stayed outside the repository and
had SHA-256 `18c39ddff3d77d0ee3064f32a2210af90bd4b01d4ffe75d5ed6ff2636ece3cd6`
before being moved to local trash.

Observed sequence:

1. Qwen Code ACP authenticated locally, edited the designated isolated Git
   worker artifact, ran its deterministic check, and returned a strict bounded
   JSON peer result through the official Rust ACP SDK.
2. Rust hashed and persisted that worker artifact plus its result as a directed
   Qwen → Codex board message before launching the Codex turn.
3. Codex app-server connected the allowlisted RAS MCP bridge, invoked
   `ras_request_context`, received the bounded response, and continued in the
   same turn.
4. Codex made the expected isolated artifact; its exact bytes and SHA-256 were
   committed as a submitted team result and verified after SQLite reopen.
5. The existing `SqliteAccStore` recorded `ArtifactPublish`,
   `TaskResultSubmitted`, `ReviewRequest`, and an independently-bound
   hash-review response in sequence. The task transitioned from
   `RESULT_SUBMITTED` to `ACCEPTED` only after that review. Reopen confirmed
   the manifest hash, four events, immutable artifact reference, and accepted
   task state.
6. The persisted external Codex thread reference resumed in a fresh app-server
   process.

No token, endpoint, session/thread id, prompt body, raw agent message, or
private transcript is retained. `RESULT_SUBMITTED != ACCEPTED` remains true:
the harness explicitly observes the submitted state before its separately
bound hash review. This narrow deterministic review is evidence for the
existing ACC transition, not a claim that a product-wide mandatory verifier
exists.

## Subsequent generic-artifact-driver regression (sanitized)

After the ACP driver was changed to return configured relative artifact hashes
as part of `AgentTaskResult`, the same live command was rerun on the dirty R6
worktree. It exited `101` after **180.40 seconds**: Qwen reached the configured
driver timeout before returning a result. The board committed no success,
artifact, or acceptance from that timed-out attempt. A process inspection
immediately after termination found no process belonging to that test. The raw
temporary log was moved to local trash after its SHA-256 was recorded as
`c406d0a79ac558b0af1bded99a35c24d3018c495e25e6e44f2581eec1774120d`.

This is current fail-closed evidence, not a passing rerun. The earlier success
remains historical evidence for the pre-artifact-collection source state.

## Root-cause attribution and calibrated-budget rerun (supersedes the above)

The two 180 s fail-closed exits were budget/contention events, not a hung
runtime or a driver regression. Attribution evidence, all on the same task
and fixture:

- The bounded coding task nominally finishes in under a minute (observed
  17.8 s and 49.8 s total, artifact byte-exact, check script exit 0).
- One observation under heavy load on the shared local vLLM node: the model
  completed the artifact work, but the final response phase alone exceeded
  280 s (total 298 s, response not yet delivered at the observation window).
- The agent's own Qwen Code session runs on the same local vLLM node, so
  agent activity is one of the contention sources during test windows.

The driver's artifact collection runs after the model response and cannot
extend the prompt phase; the 180 s budget was miscalibrated for a
multi-step coding task on a contended 27B model with xhigh reasoning.

Calibration: the Qwen-leg driver timeout in `codex_live.rs` (and the
matching ignored unit test) was raised from 180 s to 600 s with a
calibration note. Rerun of the same live command:

```text
cargo test -p agent-code-runtime --test codex_live \
  real_codex_thread_turn_uses_bounded_qwen_peer_result \
  --offline -- --ignored --nocapture
```

Exit code: `0`; observed `1 passed; 0 failed; 2 filtered out` in
**136.09 seconds**. Observed sequence: Qwen strict peer result received and
persisted as a directed board message; Codex MCP bridge connected and
completed `ras_request_context`; the Codex turn produced the expected
`fileChange`; the ACC result was submitted, independently hash-reviewed,
and accepted. Full Rust gate after the calibration: `cargo test` green,
`cargo fmt --check`, `cargo clippy --all-targets`, and `git diff --check`
all clean.

`QWEN_ACP_LIVE_TASK` is therefore **PASSING_UNDER_CALIBRATED_BUDGET** for
this narrow live command; it is not a product-wide readiness claim.
