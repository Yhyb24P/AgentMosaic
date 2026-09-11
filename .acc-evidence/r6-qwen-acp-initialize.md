# R6 Qwen Code ACP initialize probe (sanitized)

Executed on 2026-09-11 (+08:00) from an isolated temporary directory:

```text
timeout 15s qwen --acp --bare
```

The client sent exactly one ACP `initialize` request with protocol version 1,
empty client capabilities, and non-secret client identity; it then sent an
`initialized` notification solely to test whether this runtime accepts it.

| Item | Observed value |
|---|---|
| process exit | `0` |
| negotiated protocol | `1` |
| agent identity | `qwen-code` / `Qwen Code` / `0.23.3` |
| session capability | `loadSession`; `sessionCapabilities.resume` |
| prompt capability | image, audio, embedded context |
| MCP capability | HTTP, SSE |
| auth method identifiers | `openai`, `openai-responses` |
| `initialized` notification | rejected: JSON-RPC `-32601`, method not found |

The raw stdout and stderr remained outside the repository. Their SHA-256
values, retained only as opaque evidence references, were respectively
`dc098ec133a4346d13023629142ded245ff670ff2257700f6b80cb272e64258d` and
`66eda23bdd8e8259d9a1de924e45ec987b9eff66da5f24eb05a5a96356929380`.
No endpoint, token, credential, home path, prompt body, session id, or raw
runtime transcript is present in this file.

## Session/new follow-up (sanitized)

A second execution held stdin open, sent the same initialize request, then
sent one `session/new` request with an isolated temporary `cwd` and an empty
MCP-server list. No prompt, tool, file mutation, or credential access was
requested.

| Item | Observed value |
|---|---|
| process exit | `0` |
| `session/new` | JSON-RPC error `-32000` |
| normalized error | `Authentication required: Use Qwen Code CLI to authenticate first.` |
| raw stdout SHA-256 | `c6b547427c53f91f87dc38b24138bb10ecff36d51b7b9474b505bfdf147c8087` |
| raw stderr SHA-256 | `97538282200484d55cabaa951419f518db2d9a29f2d0e04728944980e4bbf916` |

Conclusion: initialization is **SUPPORTED** and current session creation is
**BLOCKED_AUTH_REQUIRED**. Same-session result return, cancellation, reconnect,
and recovery remain **NOT_RUN**. This does not establish adapter readiness or
reduce Qwen Code's project scope.

## Official Rust ACP driver follow-up

After the official `agent-client-protocol` 2.1.0 dependency closure was
resolved and compiled, its shared Rust driver performed the same bounded
connection attempt. The ignored local test
`qwen_acp_reports_auth_required_without_running_a_task` exited `0` on
2026-09-11: it received `Authentication required` during session establishment,
before `send_prompt` could run. This confirms the Rust driver follows the
observed Qwen profile without a hand-written transport or a task execution.

## Authenticated E2E follow-up (2026-09-11)

The `Authentication required` blocker was an environment condition. The ACP
server rejects `session/new` when no auth type is selected, and a `--bare`
launch does not load the user's provider settings. Observed unblock, with no
credential value read or recorded:

- Launch `qwen --acp` without `--bare`; the local provider resolves
  authentication without exposing credential material to this project.
- `initialize` advertises `authMethods: [openai, openai-responses]`.
- `authenticate` with `methodId: "openai"` succeeds.
- `session/new` succeeds; `currentModelId` is `qwen38(openai)` (local vLLM).
- A bounded one-word `session/prompt` returned `stopReason: "end_turn"`.
- The user's `settings.json` was byte-identical before and after.

Driver change: `AcpWorkerConfig` gained `auth_method: Option<String>`; when
set, the driver sends `authenticate` before `session/new`. The ignored test
`qwen_acp_authenticated_session_completes_a_bounded_task` passed locally on
2026-09-11 (88 s wall clock, local vLLM). The bare no-credential test still
passes and documents the bare path. Full Rust gate after the change: 137
passed, 0 failed, 4 ignored; `cargo fmt --check`, `cargo clippy
--all-targets`, and `git diff --check` all clean.

## R6 same-session and coding-task evidence (sanitized)

Both commands below ran against source state
`git:447f170e34dcf724883ff7955689d40f5b59040d` plus the uncommitted R6
productization paths documented in `implementation_report.md`. Their raw test
logs remain outside the repository; only these opaque SHA-256 references and
observations are retained.

| Executed at | Command | Exit | Observed result | Raw log SHA-256 |
|---|---|---:|---|---|
| 2026-09-11T21:31:xx+08:00 | `cargo test -p agent-code-runtime qwen_acp_reuses_one_authenticated_session_for_follow_up --offline -- --ignored --nocapture` | 0 | one authenticated ACP session produced a first and follow-up strict bounded result | `23b3a887ed9fe1d6b6b3045a2946ef85edd997d896be4dd22c43a3e1922abd1a` |
| 2026-09-11T21:32:xx+08:00 | `cargo test -p agent-code-runtime qwen_acp_completes_a_bounded_isolated_coding_task --offline -- --ignored --nocapture` | 0 | Qwen changed only the designated file in an isolated Git directory; the deterministic check passed | `6b069e27c796ade27a9666d4cfc247a0bf9fe1db51ab55bb532892fce82b71ca` |

The first follow-up attempt failed closed because the runtime's response did
not satisfy the strict JSON peer-result contract. It was neither persisted nor
counted as success. The successful rerun used an exact JSON response request.
No raw response, credentials, endpoint, session identifier, prompt body, or
working-directory path is retained.

## Current live availability regression (sanitized)

On the later dirty R6 worktree, two distinct authenticated local Qwen ACP
tasks reached the configured 180-second driver timeout:

| Command | Exit | Observed result | Raw log SHA-256 |
|---|---:|---|---|
| `cargo test -p agent-code-runtime --test codex_live real_codex_thread_turn_uses_bounded_qwen_peer_result --offline -- --ignored --nocapture` | 101 | cross-runtime worker timed out; no success/artifact/acceptance committed | `c406d0a79ac558b0af1bded99a35c24d3018c495e25e6e44f2581eec1774120d` |
| `cargo test -p agent-code-runtime qwen_acp_completes_a_bounded_isolated_coding_task --offline -- --ignored --nocapture` | 101 | isolated worker timed out | `6b84dcfdcc261237bd61ac8e710559fe943ae30c31e75ee97b12fc87e6877254` |

Both subprocess trees had exited when inspected after the test. At that point
the live-task classification was `BLOCKED_RUNTIME_UNRESPONSIVE`; it did not
alter the earlier authenticated-session evidence or infer an authentication
failure. The later calibrated-budget cross-runtime rerun recorded in
`.acc-evidence/r6-codex-qwen-live.md` supersedes that availability conclusion
for the bounded worker route; this historical timeout record remains auditable.
