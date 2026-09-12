# R7 CLI ACP smoke (sanitized)

Executed 2026-09-12. The normal-path CLI registered one `qwen --acp` worker,
submitted one bounded task, and invoked `run-acp` with auth method `openai`, a
180-second timeout, an isolated empty working directory, and no artifact paths.

The command completed with exit code `0` in 10.3 seconds. The only retained
observations are:

- registration returned `registered agent=qwen-worker`;
- the driver returned `completed task=1 agent=qwen-worker`;
- the authoritative SQLite board reported task 1 as `succeeded` and assigned
  to `qwen-worker`;
- the disposable SQLite file SHA-256 was
  `eef1904adabd13b8bef268304ecd7b6a345e2378bdb7ddd6c121f3799778b37a`.

No credential, endpoint, prompt body, model response, external session ID,
raw frame, PID, or absolute temporary path is retained. This is evidence that
the registered CLI ACP path can start and persist one bounded worker task; it
does not establish continuation, active cancel, recovery, a Codex runtime, or
R7/M2/M3 readiness.

## Stable-v1 resume follow-up

The ignored Rust test
`qwen_acp_resumes_a_persisted_session_for_a_follow_up` ran against local
`qwen --acp` with the configured `openai` auth method. It created one bounded
seed session, used only its opaque returned session ID in a fresh ACP
connection's typed stable-v1 `session/resume` builder, and completed one strict
follow-up result. Exit code was `0`: 1 passed, 0 failed, 21 filtered out, in
22.63 seconds. No session ID, prompt, response, credential, endpoint, or raw
frame is retained. This proves the narrow resume primitive, not automatic task
replay or recovery readiness.

## Stable-v1 typed active-cancel probe

The ignored Rust test `acp_m2_probe_cancel_active_session` ran against local
`qwen --acp` with the configured `openai` authentication method. It created
one session, sent one bounded prompt, and then used the official stable-v1
typed `session/cancel` notification on that same live session connection. The
agent returned the protocol-defined `StopReason::Cancelled`.

Command: `cargo test -p agent-code-runtime acp_m2_probe_cancel_active_session
-- --ignored --nocapture`; exit code `0`; observed result: 1 passed, 0 failed,
21 filtered out, 1.51 seconds. The retained test output contains only the
opaque session-id length (`36`), not its value. No credential, endpoint,
prompt body, model response, raw frame, PID, or absolute temporary path is
retained.

This verifies the driver-facing protocol primitive only. It does not make the
separate CLI `cancel` command an active cross-process cancellation operation,
nor does it establish retry or recovery readiness.
