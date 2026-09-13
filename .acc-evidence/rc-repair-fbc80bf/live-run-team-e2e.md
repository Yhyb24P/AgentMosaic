# RC repair W5 — live `run-team` production E2E (sanitized)

- date: 2026-09-13
- branch: `v2/rust-agent-team`
- worktree: dirty RC repair tree; baseline `cargo test --workspace --all-features`
  was 255 passed / 0 failed / 16 ignored before this workstream
- live runtimes: `codex-cli 0.154.0` (PATH `codex`), Qwen Code `0.23.3`
  (PATH `qwen`), both already authenticated on this machine
- pinned codex source: `target/upstream-ref/codex` @ tag `rust-v0.154.0`

## Test added

`crates/agent-code-cli/tests/team_live_product.rs`

- `live_run_team_produces_a_durable_worker_dependent_answer` — `#[ignore]`, drives
  only `env!("CARGO_BIN_EXE_agent-code-cli")`. It registers a real
  `codex-app-server` Lead and two real Qwen `acp` agents through `register`,
  issues exactly one `run-team`, then reads the durable result back through
  `final`, `resume-team`, `status`, `artifact`, and `binding` in separate
  processes. It never creates a delegated task, never reads a plan file, never
  runs Qwen itself, never injects a worker result into a Codex prompt, and never
  finalizes refs.
- `failed_lead_leaves_root_failed_and_final_reports_no_answer` — deterministic,
  not ignored, R9 failure path (below). Starts no live runtime.

## Decisive live command

```text
cargo test -p agent-code-cli --test team_live_product -- --ignored --nocapture
```

- exit code: `0`
- observed: `1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 85.04s`
- sanitized log: `.acc-evidence/rc-repair-fbc80bf/live-run-team-e2e.log`
- log SHA-256: `a15615fcb24d6c6ea576541ae17897b2a5fcd039e2f5c0029e463a364731aa7e`

### Observed durable result

```text
root=1 lead=codex-lead
answer: Completed: worker.txt was produced with RCTOK-18d4e2515c686960001094eb.
task_refs: 2
artifact_refs: task=2 path=worker.txt sha256=23f3ef2f0a550aff9886f9c6bcef54ddac5d2fdef5ca8fe849db49cb97f3c979
```

- root task id: `1` (`reasoning`, assignee `codex-lead`, `parent=-`, `succeeded`)
- worker task id: `2` (`bulk`, assignee `qwen-worker`, `parent=1`, `succeeded`)
- utility task id: `3` (`utility`, assignee `qwen-utility`, `parent=1`, `succeeded`)
- final answer: `Completed: worker.txt was produced with RCTOK-18d4e2515c686960001094eb.`
- persisted final task refs: `[2]`
- persisted final artifact refs: `(task=2, path=worker.txt, sha256=23f3ef2f0a550aff9886f9c6bcef54ddac5d2fdef5ca8fe849db49cb97f3c979)`
- `artifact 2` (CLI): `task=2 path=worker.txt sha256=23f3ef2f0a550aff9886f9c6bcef54ddac5d2fdef5ca8fe849db49cb97f3c979`
- `final 2` (worker's own persisted peer summary) contains the same token:
  `worker.txt at repo root contains the single line worker=complete RCTOK-18d4e2515c686960001094eb ...`
- `binding 2`: `attempt=1 agent=qwen-worker runtime_kind=acp lifecycle_state=completed external_reference_present=true`

### New-process reproduction

`final 1` and `resume-team 1 --lead codex-lead` are separate CLI processes with
fresh SQLite connections. Both reproduced the same answer, `task_refs: 2`, and
the same `worker.txt` sha256 as the original `run-team`. `resume-team` returns
the `reconstruct_team_result` view of the board, so the reproduced refs are read
back from the durable database, not from in-memory run state.

### Dependency proof (causal, not coincidental)

The test asserts, together:

1. the random token appears in the **persisted root answer** (`final 1`);
2. the **worker task id `2` is in the persisted final task refs** (`[2]`,
   reproduced by `resume-team` in a new process);
3. the token also appears in the **worker's own persisted result** (`final 2`),
   i.e. the answer's token is consistent with what the delegated worker
   returned, not merely echoed from the root objective;
4. the Lead selected the exact worker artifact `(task=2, worker.txt,
   23f3ef…)`, no other task contributed an artifact;
5. the live worker's on-disk `worker.txt` equals exactly
   `worker=complete RCTOK-18d4e2515c686960001094eb\n`, and `sha256sum` of that
   file equals the recorded artifact digest.

The Lead cannot complete without grounding its answer in a succeeded descendant
task (`Lead::verify_completion`), and the only task that recorded a `worker.txt`
artifact is the Qwen bulk worker, so the token in the root answer is reachable
only through the delegated worker task that the Lead selected. Exactly one
`run-team` command and no human transfer are issued; the test harness performs
no orchestration.

## Product defect found and fixed

The first live attempt failed:

```text
run-team: the lead run failed: Brain(Unavailable("codex lead failed to read the
completed codex lead turn: Protocol(\"thread/read response has no turn \")"))
```

Root cause: the pinned upstream protocol
(`codex-rs/app-server-protocol/src/protocol/v2/turn.rs:509`,
`TurnCompletedNotification { thread_id, turn: Turn }`) sends the turn as a full
object under `params.turn.id`. The client read a flat `params.turnId`, so the
turn id was empty and the completed-turn snapshot could never be selected.

Fix (`crates/agent-code-runtime/src/codex_app_server.rs`):

- `interpret_event` for `turn/completed` now calls `completed_turn_id(params)`.
- `completed_turn_id` prefers the upstream `params.turn.id` and falls back to
  the flat `params.turnId` so the scripted mock stays readable.

Regression test: `codex_app_server::tests::turn_completed_reads_the_upstream_turn_object`
(accepts the upstream turn object, the flat fallback, and prefers the turn
object when both are present).

No change was needed to the Lead's decision contract: with the transport fixed,
the real Lead produced a valid strict-JSON decision on the first turn and the
real worker produced the required file, with no markdown fencing or correction
turn.

## Additive CLI observability change

`crates/agent-code-cli/src/main.rs` `status` now renders `parent=<id|->` between
`attempts=` and `objective=`. This is additive (every existing assertion is a
`contains`), and it is what lets the required CLI-only assertion "`status`
shows at least one child task whose parent is the root" be made. Covered by the
live test and by every `status` assertion in the workspace suite.

## R9 failure path

The existing deterministic coverage in
`crates/agent-code-runtime/tests/team_runner_product.rs` already proves a
rejected Lead decision (`an_invalid_lead_decision_leaves_the_root_not_succeeded`)
and an unroutable driver kind (`an_unsupported_driver_kind_fails_the_run`) leave
the root not succeeded. What was missing was the product-surface assertion that
`final` reports no successful answer. Added
`failed_lead_leaves_root_failed_and_final_reports_no_answer`:

- registers the Lead with a non-app-server executable that exits immediately;
- `run-team` exits non-zero;
- `status` shows the root `failed` and no task `succeeded`;
- `final <root>` exits non-zero with `no successful result`.

Command: `cargo test -p agent-code-cli --test team_live_product failed_lead_leaves_root_failed_and_final_reports_no_answer -- --nocapture`
— exit code `0`, `1 passed; 0 failed`.
Log: `.acc-evidence/rc-repair-fbc80bf/failure-path-failed-lead.log`,
SHA-256 `e80d6c629652f2ecf4d9b5c8510dce1410230ee4a84830d7359a5a92c7a4465d`.

## Workspace verification (final code)

```text
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

- `cargo fmt --all`: clean
- clippy: clean, `Finished` with `-D warnings`
- workspace test: exit `0`, **257 passed / 0 failed / 17 ignored**

Baseline was 255/0/16; the two new passing tests are
`codex_app_server::tests::turn_completed_reads_the_upstream_turn_object` and
`crates/agent-code-cli` `failed_lead_leaves_root_failed_and_final_reports_no_answer`,
and the new ignored test is
`live_run_team_produces_a_durable_worker_dependent_answer`.

- full log: `.acc-evidence/rc-repair-fbc80bf/workspace-test.log`
  (SHA-256 `f60b8591994b4f563dd9ba37d6bbd63ba63c5f9da34dd19874e194256fcfe326`)
- every test name with its result:
  `.acc-evidence/rc-repair-fbc80bf/workspace-test-names.txt`
  (SHA-256 `a4969d44612c9576ee9cbb061538abbabea525a222cb1c21206cf741b1eef86e`;
  257 `ok`, 17 `ignored`)

## Sanitization

The retained logs contain only product CLI output: task ids, objectives, the
random `RCTOK-…` marker, persisted summaries, artifact path/sha256, status
lines, and result counts. No credential, endpoint, raw protocol frame, hidden
reasoning, or foreign session/thread identifier is retained; the `binding`
lines report only `external_reference_present=true`.

## Uncertainty / not verified

- The live test is inherently model-dependent. It passed on three consecutive
  live executions (run 2: 110.57s, run 3: 85.32s, final: 85.04s). On run 2 the
  Qwen worker's first attempt returned a non-strict JSON peer result and the
  scheduler's bounded retry (`attempts=2`) recovered; runs 3/final needed no
  retry.
- `resume-team` is exercised for the idempotent succeeded-root path only; the
  live test does not kill an in-flight run to exercise mid-run recovery.
- The deterministic failure path proves root-failed + `final`-no-answer for a
  Lead process that exits immediately, not for every possible failure mode.
