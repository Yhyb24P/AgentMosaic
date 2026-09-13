# Kimi Code runtime probe — Phase 2.4B

Local discovery initially found native `kimi` version `0.39.1`. The current
local runtime was subsequently re-probed as `0.42.0`. Local help exposes
non-interactive prompt mode, `--output-format stream-json`, `--session`,
`--continue`, `doctor`, provider management, and `acp` as an Agent Client
Protocol server over stdio.

No remote request was made. Tool/MCP details, event schema, tool-result return,
cancellation, reconnect, and retry remain UNKNOWN. Candidate:
**structured-message driver**, pending a bounded live ACP or CLI probe.

## Phase 2.5A bounded ACP result (superseded by 0.42.0 re-probe)

An isolated `kimi acp` stdio JSON-RPC probe sent only `initialize` and
`initialized`, then exited 0. Real stdout confirmed protocol version 1,
agent identity/version, session load/list/resume/close/delete/fork capability,
prompt image/embedded-context capability, MCP HTTP/SSE capability, and logout.
The response offered a terminal login auth method, but no verified active auth
state. The probe therefore stopped at `BLOCKED_AUTH_REQUIRED`; no task was sent.

| Capability | State | Evidence |
|---|---|---|
| initialize / initialized | SUPPORTED | real ACP stdout |
| external runtime identity | SUPPORTED | real agent identity/version |
| session inspect/resume capability | SUPPORTED | ACP session capabilities |
| MCP transport capability | SUPPORTED | ACP MCP HTTP/SSE capabilities |
| client task / lifecycle events | BLOCKED_AUTH_REQUIRED | no authenticated task permitted |
| runtime tool request / same-ID response | BLOCKED_AUTH_REQUIRED | no authenticated task permitted |
| cancel / interrupt | BLOCKED_AUTH_REQUIRED | no authenticated task permitted |
| restart reconcile / retry schema | BLOCKED_AUTH_REQUIRED | no authenticated session permitted |

## 0.42.0 re-probe — bounded ACP turn

The current local `kimi` executable reports `0.42.0`. A JSON-RPC-only stdio
probe in an isolated temporary directory held its input open through
`initialize`, `initialized`, and `session/new`. It completed with exit `0`.
The initialize response advertised ACP protocol version 1, terminal `login`
as an available auth method, session list/resume/close/delete/fork capabilities,
and MCP HTTP/SSE transport capability. `session/new` returned a session
reference. No credential, endpoint, session identifier, raw frame, prompt, or
transcript is retained.

The ignored Rust test
`kimi_acp_completes_a_bounded_no_tool_turn` then used the normal
`AcpWorkerDriver` against `kimi acp` in a fresh isolated directory. It issued
one bounded prompt that prohibited tools, shell, network, and filesystem
writes. The driver observed a nonempty external session reference and a
completed response; it passed with exit `0` in 1.79 seconds. The peer did not
produce the driver's required single strict JSON result object in the paired
negative check, so `parse_peer_result` rejected it fail-closed. This proves a
bounded live ACP turn, not structured-result interoperability or an adapter.

| Capability | State | Evidence |
|---|---|---|
| initialize / initialized / session-new | SUPPORTED | sanitized 0.42.0 ACP probe |
| external session reference | SUPPORTED | 0.42.0 ACP probe and bounded-turn test |
| bounded client task and terminal response | SUPPORTED | `kimi_acp_completes_a_bounded_no_tool_turn` |
| strict structured peer result | UNSUPPORTED (observed output) | driver rejected non-single-JSON response fail-closed |
| tool request / same-ID response | UNKNOWN | no tool was exposed or requested |
| cancel / interrupt | UNKNOWN | not probed |
| restart / reconcile / retry | UNKNOWN | not probed |

Kimi is therefore classified `KIMI_READY` only in the narrow M4 sense that a
real bounded task turn passed. The current candidate remains a
**bounded-context non-interactive worker**, not a structured-message adapter;
no adapter or readiness claim follows from this probe.
