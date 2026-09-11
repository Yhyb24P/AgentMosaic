# Qwen Code runtime probe — Phase 2.4B

Local discovery found `qwen` version `0.23.3`, installed from npm as
`@qwen-code/qwen-code`. Local help exposes non-interactive prompt mode,
`--input-format stream-json`, `--output-format stream-json`, JSON FD/file
event output, `--session-id`, resume/continue, sessions management, MCP
configuration/management, ACP mode, and experimental `serve --http-bridge`.

R6 source/wire probe (`qwen --acp --bare`, isolated temporary working
directory) completed ACP `initialize` with exit 0.  Its response negotiated
protocol version 1, identified `qwen-code` 0.23.3, advertised `loadSession`,
session `resume`, prompt embedded context, and MCP HTTP/SSE capability.  It
advertised authentication methods `openai` and `openai-responses`; no
credential value was read or recorded.

The same live probe established an implementation-specific protocol quirk:
Qwen Code returns JSON-RPC `-32601` for an `initialized` notification.  A
shared driver therefore must not infer that notification from generic ACP
documentation; it must use this runtime profile's observed behavior.

A second live probe kept stdio open and sent a minimal `session/new` request
with only an isolated temporary `cwd` and an empty MCP-server list.  Qwen Code
returned JSON-RPC `-32000`: `Authentication required: Use Qwen Code CLI to
authenticate first.`  No prompt, tool invocation, remote work, or filesystem
change was requested. `QWEN_ACP_AUTH` is therefore
**BLOCKED_AUTH_REQUIRED** in the current environment.  It is an environment
state, not a statement about Qwen Code product support.

Actual tool-call shape, same-session tool-result semantics, cancellation,
reconnect, and retry remain UNKNOWN. Candidate: **live tool-capable ACP
driver**, pending a user-established Qwen Code authentication state and the
official Rust SDK dependency being reproducibly resolved. This is not an
adapter-readiness claim.
`QWEN_AWESWITCH_ENTRYPOINT` remains `BLOCKED_UNVERIFIED`; it does not describe
the independent official Qwen Code CLI found here.

## Unblock: `QWEN_ACP_AUTH = UNBLOCKED_LOCAL_PROVIDER` (2026-09-11)

The `BLOCKED_AUTH_REQUIRED` state was an environment condition, now resolved.
Root cause: the ACP server's `ensureAuthenticated` rejects `session/new` when
no auth type is selected, and `--bare` launch does not load the user's local
provider settings, so the selected provider is invisible to a bare agent.

Observed unblock sequence (live, no credential value read or recorded):

1. Launch `qwen --acp` without `--bare` so local user settings load; the
   provider resolves authentication without exposing it to this project.
2. `initialize` advertises `authMethods: [openai, openai-responses]`.
3. Client sends `authenticate` with `methodId: "openai"`; the server selects
   that auth type and resolves the key through the provider's envKey.
4. `session/new` succeeds; the session reports
   `currentModelId: "qwen38(openai)"`, i.e. the local vLLM endpoint.
5. `session/prompt` with a bounded one-word task returned
   `stopReason: "end_turn"`. The user's `settings.json` was byte-identical
   before and after the run.

`AcpWorkerDriver` gained an `auth_method` config field: when set, it sends
`authenticate` before `session/new`. The ignored test
`qwen_acp_authenticated_session_completes_a_bounded_task` exercises the full
authenticated path through the Rust driver and passed locally (88 s wall
clock, local vLLM). The bare no-credential test still asserts
`Authentication required`, documenting that the bare path remains
unauthenticated by design.

## R6 live worker facts (2026-09-11)

The shared official-Rust-SDK driver has now exercised two additional bounded
live paths against the same authenticated local profile:

- one ACP session completed both an initial strict peer result and a follow-up
  strict peer result without opening a second session;
- in an isolated temporary Git repository, Qwen inspected and changed only a
  designated text fixture, ran its deterministic shell check, and returned a
  strict bounded result.

The cross-runtime live harness also persists Qwen's artifact hash and directed
message before launching the Codex turn; the opaque external ACP session
reference is stored through the existing task-board binding and verified after
SQLite reopen. Evidence is sanitized in `.acc-evidence/r6-qwen-acp-initialize.md`
and `.acc-evidence/r6-codex-qwen-live.md`. Active cancel, interrupted-session
reconcile, and profile-wide process cleanup are still not proven, so this is
not a Qwen adapter readiness claim.

## Current live availability (2026-09-11)

Two later authenticated task runs — the isolated coding task and the
Codex↔Qwen worker route — each reached the driver's 180-second timeout. Both
terminated without a committed success and without an observed child process
remaining after the test. Therefore the current live-task state is
`BLOCKED_RUNTIME_UNRESPONSIVE`, distinct from the resolved authentication
state. Details and opaque log hashes are in
`.acc-evidence/r6-qwen-acp-initialize.md`.

## Live-task budget calibration (supersedes the above, 2026-09-11)

Attribution showed the runtime is responsive, not unresponsive. On the same
bounded coding task and fixture: nominal completions of 17.8 s and 49.8 s
(artifact byte-exact, check script exit 0), while one observation under heavy
load on the shared local vLLM node — the agent's own Qwen Code session is one
of the contention sources — saw the model finish the artifact work but the
final response phase alone exceed 280 s. The driver's artifact collection
runs after the model response and cannot extend the prompt phase; the 180 s
budget was miscalibrated for a multi-step coding task on a contended 27B
model with xhigh reasoning.

The Qwen-leg driver timeout in `codex_live.rs` and the matching ignored unit
test is now 600 s with a calibration note. The live cross-runtime command
reran green in 136.09 s: Qwen strict peer result persisted, Codex MCP
bridge completed `ras_request_context`, the Codex turn produced the expected
file change, and the ACC result was submitted, independently hash-reviewed,
and accepted. `BLOCKED_RUNTIME_UNRESPONSIVE` is withdrawn for this narrow
live command; the state is now passing under the calibrated budget. This
remains a narrow live-command result, not a Qwen adapter readiness claim.
See `.acc-evidence/r6-codex-qwen-live.md` for the full record.
