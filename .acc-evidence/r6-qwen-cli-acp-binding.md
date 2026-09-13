# Sanitized Qwen ACP normal-CLI evidence

- Source state: `5f632aa76926ab58e094b77bdc690107300672dc` plus the
  read-only binding-inspection addition bound by the subsequent commit.
- Runtime command: registered `qwen --acp` through `agent-code-cli run-acp`.
- Auth: method name `openai` only; no credential value was read, logged, or
  committed. The isolated Git directory, database path, session ID, prompt,
  raw frames, model output, and provider endpoint are not retained.

## Command result

The normal CLI path registered the Qwen worker, submitted one bounded isolated
Git task, and ran it with a 600-second contention-calibrated budget. Exit `0`:
`completed task=1 agent=qwen`.

The authoritative CLI inspection then reported:

```text
task=1 status=succeeded assignee=qwen
task=1 path=qwen-worker.txt sha256=9fe33998687d40bf81f889c22a491a9e403c477fb827611815c4c4ed5eb71f67
task=1 attempt=1 agent=qwen runtime_kind=acp lifecycle_state=completed external_reference_present=true
```

The artifact's independently calculated SHA-256 was the same digest. The
binding command deliberately reports only whether a foreign reference exists,
not its value.

Sanitized command-log hashes: register
`32fa17913ca2dad25669b3cc2daf1802eddc1aa02e1d17c547f8863811d1efd7`;
submit `d8538a29a0115184bb7e6cfbae17a97a5484c360137b969a3b5627fa20143237`;
run `d56127539d96c0b45f44c4260d843bd832ef819fd196cb95942a6371a1dd53bf`;
exit receipt `9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`.

This is real Qwen result/artifact/binding evidence for M3. It does not prove
continuation, cancellation, recovery/reconcile, or M5 cross-runtime team E2E.

## Same-session continuation requalification

At source `125d32d6b33ed741c10c0536c96ed4b5f38bb23d`, the current local Qwen
profile also passed the focused live driver command:

```text
cargo test -p agent-code-runtime qwen_acp_reuses_one_authenticated_session_for_follow_up -- --ignored --nocapture
```

Exit `0`; 1 passed, 0 failed, 0 ignored, 22 filtered out; 11.64 seconds.
The sanitized log SHA-256 is
`46988f8c998381ad0d1e5a655a027103925a8cef15dba6ebaa700dbc70a7f747` and
the exit-receipt SHA-256 is
`9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`.
No session ID, prompt, response, frame, endpoint, or credential was retained.
This requalifies the driver's same-session API, not a user-facing CLI
continuation/recovery workflow.

## Normal CLI continuation / session-resume evidence

The same isolated board then created a separate pending task 2 and ran:

```text
agent-code-cli continue-acp <db> 2 qwen 1 <isolated-cwd> openai 600
```

Exit `0`; observed output: `continued task=2 agent=qwen source_task=1`.
Authoritative inspection showed task 2 as succeeded with its own attempt-1
binding: `runtime_kind=acp`, `lifecycle_state=completed`, and
`external_reference_present=true`. Task 1 remained succeeded; it was not
replayed. The task-2 summary, foreign session reference, model output, raw
frames, credentials, and endpoint were not retained.

Sanitized log hashes: submit
`897e65c715bb3fcfe2e62fb9acdbd9c61c471b38e52ab445a1ba7d793b687ba3`;
continuation `60e83f42c8cf30e3751d18b4ac7b147423b1324e69dc9cc8ba2cb965693ccdc3`;
exit receipt `9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`.

This is user-facing Qwen session resume evidence. It does not prove active
cross-process cancellation or full scheduler-driven M5 E2E.

## Peer-confirmed active cancellation requalification

At source `ed0235a9ae5a0a3f715fad792264552c6775a4ad`, the focused live command
below issued the typed stable-v1 ACP cancellation notification against the
driver-created active session and accepted cancellation only after the peer's
terminal `StopReason::Cancelled`:

```text
cargo test -p agent-code-runtime acp_m2_probe_cancel_active_session -- --ignored --nocapture
```

Exit `0`; 1 passed, 0 failed, 0 ignored, 22 filtered out; 1.13 seconds.
Sanitized log SHA-256:
`a2b6765a3100a39a25ba0b606ffac8ddc747cdd1799add32f606d47259bd5b0c`;
exit receipt SHA-256:
`9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`.
The session ID, prompt, response, endpoint, and credentials were not retained.

## Scheduler-managed Qwen worker plus deterministic utility

At source `2011347ca49263124c610d5ddb6f6a776fd2b3c4` plus the focused live
harness addition, the command below passed against the real local Qwen
runtime:

```text
cargo test -p agent-code-runtime --test codex_live real_scheduler_runs_qwen_worker_and_utility_on_one_board -- --ignored --nocapture
```

Exit `0`; 1 passed, 0 failed, 0 ignored, 3 filtered out; 19.69 seconds.
Sanitized log SHA-256:
`0974724bd3393719fe0360a91644a4e15dd63c7c2391c59b4eae05cf7577c54a`;
exit receipt SHA-256:
`9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`.

The scheduler created both tasks on one SQLite board: Qwen executed the
bounded isolated repair/check and returned an artifact; the deterministic
utility returned a directed message addressed to Codex. The test asserts the
Qwen task succeeded, its artifact is durable, and the scheduler-created
attempt has a completed foreign ACP binding. No session ID, prompt, model text,
credentials, raw frame, or temporary path is retained. This is M5 partial
evidence only; it does not yet include live Codex Lead follow-up, retry,
reassignment, user override, or reopen recovery in the same topology.

## Scheduler Qwen + utility + live Codex Lead follow-up

At the working source following `47e25c1e96a61a1ce1d411dbb07f9d837b3d925f`,
the same ignored harness was extended and re-run:

```text
cargo test -p agent-code-runtime --test codex_live real_scheduler_runs_qwen_worker_and_utility_on_one_board -- --ignored --nocapture
```

Exit `0`; 1 passed, 0 failed, 0 ignored, 3 filtered out; 37.99 seconds.
The sanitized command-log SHA-256 is
`780064fb2cc836a38c7e12bcddbd8bf39025f82510ab33a614aca488d724e676`.

The scheduler ran the real Qwen ACP worker and deterministic utility on its
SQLite board. A separately created, assigned Codex Lead task then made one
allowlisted `ras_request_context` call during the same real Codex app-server
turn. That call received bounded persisted board context only; no teammate
result appeared in the Codex user prompt. The test asserted the exact Lead
artifact, committed the Lead result and Codex external binding, selected the
Qwen artifact as a final reference, and reopened the board to inspect the
persisted succeeded task and final references. It retained no foreign ID,
prompt, model response, endpoint, credential, or raw frame.

This adds the missing live Codex Lead follow-up to this narrow topology. It is
still partial M5 evidence: retry, reassignment, user override, and recovery
must be exercised in the same scheduler-owned topology before M5 can be
marked passed.

## Scheduler-owned Codex driver and strict Qwen result repair

The scheduler topology was then changed so Codex is itself an
`AgentDriver` rather than a harness-managed app-server call. Its external
thread/turn binding is persisted against the scheduler-created running attempt
before the turn proceeds. `ras_request_context` now returns a bounded board
projection containing directed messages plus succeeded worker summaries and
artifact hashes; it does not return a raw transcript or accept runtime identity
from tool arguments.

The shared ACP worker still rejects an invalid peer result. When a completed
worker's first response is not the required strict JSON object, it sends one
bounded correction request on the same ACP session. Only that final response
is parsed with the existing strict schema; a second invalid response remains a
failed task. The correction branch is covered by the local ACP lifecycle mock.

The current live command passed after this repair:

```text
cargo test -p agent-code-runtime --test codex_live real_scheduler_runs_qwen_worker_and_utility_on_one_board -- --ignored --nocapture
```

Exit `0`; 1 passed, 0 failed, 0 ignored, 3 filtered out; 46.35 seconds.
Sanitized log SHA-256:
`69e9e64e7919c4a118df7c51b15a61d977138cb625d602b35546ca4e8746d012`.
The fixture asserts the real Qwen ACP task, utility task, scheduler-created
Codex Lead attempt, one persisted collaboration request, completed external
bindings, exact Qwen/Lead artifacts, selected final reference, and SQLite
reopen. Codex used `gpt-5.5` with `low` reasoning effort. No prompt, model
response, external identifier, credential, endpoint, or raw protocol frame was
retained.

Two pre-repair scheduler invocations failed closed because the Qwen response
was not a strict peer result; they are not counted as successful evidence.
This live pass is stronger M5 partial evidence, but retry, reassignment, user
override, and recovery have not yet been demonstrated together in this real
topology, so M5 remains `NOT_RUN`.
