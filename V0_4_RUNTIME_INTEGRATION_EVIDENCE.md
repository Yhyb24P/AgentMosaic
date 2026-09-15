# v0.4 Runtime Integration Evidence

Every entry below is an actual result: deterministic test output, a remote
workflow conclusion, a live CLI transcript, a SQLite read-back, or a file hash.
Nothing here is promoted from source inspection alone.

`end_sha` is the frozen implementation head whose remote gates are green. The
commit that adds or updates this document changes documentation only; no code
changed after `end_sha`.

## Baseline

```text
branch=feat/v0.4-runtime-integration
start_sha=c90ec9560f80cc8f956057e686fd20d96efb93a4
end_sha=32dddcc933dadd921495dda08fc2b4e172654e39
base_main=c90ec9560f80cc8f956057e686fd20d96efb93a4
workspace_version=0.4.0-dev
schema_version=12
dirty_state_preserved=true (every worktree was clean before each edit)
```

## Remote CI

Exact head `32dddcc933dadd921495dda08fc2b4e172654e39` (all PR-triggered
workflows, queried with `gh run view --json headSha,conclusion`):

```text
rust          run 35002084208  success  32dddcc933dadd921495dda08fc2b4e172654e39
rust-quality  run 35002084245  success  32dddcc933dadd921495dda08fc2b4e172654e39
Release/plan  run 35002084213  success  32dddcc933dadd921495dda08fc2b4e172654e39
```

Earlier checkpoint on `c7dbe48` (same green set, before the two streaming
deadline regressions were added):

```text
rust          run 35000927350  success  c7dbe480355bbfc50029b625f9a5264ccb81cf6c
rust-quality  run 35000927334  success  c7dbe480355bbfc50029b625f9a5264ccb81cf6c
Release/plan  run 35000927201  success  c7dbe480355bbfc50029b625f9a5264ccb81cf6c
```

Earlier checkpoint on the P0-repair head `9492b33`:

```text
rust          run 34995681746  success  9492b33ebc3482f9a501905dccc68c7df83287c3
rust-quality  run 34995681756  success  9492b33ebc3482f9a501905dccc68c7df83287c3
Release/plan  run 34995681750  success  9492b33ebc3482f9a501905dccc68c7df83287c3
```

## P0 repairs and the CI failures they exposed

```text
identity_fixture_hash_before=e7b430ad17b8c2be3704f540ca921c77b2ba063c9661300411fbda51334adc3b
identity_fixture_hash_after =e7b430ad17b8c2be3704f540ca921c77b2ba063c9661300411fbda51334adc3b
identity_gate_negative_control=FAIL as required (stale active identity: docs/__identity_negative_control.md:1)
codex_exec_mock_stress_runs=50/50 pass (cargo test -p agentmosaic-runtime --test codex_exec_lead rejected_reply_is_repaired_once_through_exec_resume)
```

Three separate repairs were needed before PR CI could be trusted:

1. `a1e5856` — the identity gate flagged the binary published-v0.3 SQLite
   fixture. The exception is exactly that one path; every other tracked file is
   still scanned, and a tracked text file carrying a retired identity still
   fails the gate (negative control above).
2. `b46f315` — the deterministic Codex exec fixtures printed JSONL and exited
   without reading the prompt that production writes to stdin, so a correct
   `write_all` could race the child's exit into `EPIPE`. Every Codex exec
   fixture now reads stdin to EOF and asserts a non-empty prompt before
   answering. Production transport is unchanged, and the previously failing
   test passed 50 consecutive invocations locally.
3. `9492b33` — once those two repairs let `rust-quality` reach its later steps,
   the release-hygiene gate rejected the tree for tracking a `.db` file. The
   frozen compatibility fixture is force-tracked migration evidence, not a
   runtime database, so the gate excludes exactly that path; a tracked
   negative-control `.db` was still rejected.

## Defects found and fixed during closeout

```text
eb0bed1  am events projected every descendant observation twice (the latest run
         was expanded, then its reasoning root was expanded again). A duplicated
         line is a fabricated second observation and, with --follow, a replay.
         Deterministic CLI regression: crates/agentmosaic-cli/tests/events_projection.rs
         fails on the previous code (6 rows where 4 are unique).
f30ea52  Codex exec and Claude deadlines killed the direct child but nothing
         proved the whole process group was reaped. Each family now has a
         deterministic grandchild regression; removing group termination makes
         the Codex exec case fail with the surviving grandchild pid.
32dddcc  Slow-drip deadline coverage existed only for ACP. Codex exec and Claude
         now stream a frame every 100ms under a 350ms budget and must still time
         out inside the window; sliding the deadline per event makes the Codex
         exec case run the whole stream and fail.
```

## Canonical local gates (on `32dddcc`)

```text
identity=      PASS (scripts/ci/check_identity.sh)
fmt=           PASS (cargo fmt --all -- --check)
clippy=        PASS (cargo clippy --workspace --all-targets --all-features -- -D warnings)
tests=         PASS (cargo test --workspace --all-features: 482 passed, 0 failed, 24 ignored live cases recorded separately)
release_build= PASS (cargo build --release --workspace)
diff_check=    PASS (git diff --check)
```

## Migration

```text
v11_to_v12=PASS (additive binding columns + runtime_events)
published_v0_3_to_v12=PASS (published am 0.3.0 fixture, SHA-256 e7b430ad17b8c2be3704f540ca921c77b2ba063c9661300411fbda51334adc3b, copied before opening so the frozen fixture is never modified)
v8_to_v12=PASS (authentic checked-in v8 fixture)
old_driver_strings_restorable=PASS (registry round-trip covers native, acp, cli, codex-app-server, codex-exec, claude-cli)
```

## Architecture gates

```text
runtime_event_foundation=PASS (schema v12 observation plane; a success-looking observation leaves task, attempt, artifacts and final refs untouched — crates/agentmosaic-storage/tests/runtime_events_v12.rs)
task_authority_separation=PASS (only Scheduler/Driver/Lead settle task truth; runtime events are observation-only)
role_runtime_decoupled=PASS (LeadBrainFactory; unsupported Lead runtimes fail before a root exists)
generic_acp=PASS (one typed ACP v1 adapter drives Qwen, Kimi and OpenCode; no per-vendor parser)
codex_exec_worker=PASS (durable driver, JSONL normalization, bounded process group, thread binding, resume argv, managed strict worker-result schema)
codex_exec_lead=PASS (see the Codex Exec Lead section)
claude_cli_worker=PASS (see the Claude section)
claude_cli_lead=UNSUPPORTED (worker runtime is deliberately not accepted as a Lead until a strict structured decision contract exists and is tested; allowed final status for v0.4)
cli_observability=PASS (am events/status/final/artifact replay durable observations with privacy-filtered summaries and short foreign ids)
tui_observability=PASS (read-only board projection; a direct test proves foreign session ids never render)
```

### Codex Exec Lead

Deterministic suite at this head: strict decision parsing, exactly one
same-thread repair, fail-closed second rejection, root running-attempt binding
persisted on `thread.started`, and a new Lead instance restoring and resuming
the persisted foreign thread (`crates/agentmosaic-runtime/tests/codex_exec_lead.rs`).

Real TeamRunner run (codex-cli 0.154.0, `/home/yhshy/.local/bin/codex`, public
`am run`, Codex Exec Lead + real Qwen ACP worker):

```text
project scratch, root task   1 (reasoning, lead, succeeded)
delegated child task         2 (bulk, qwen-worker, succeeded)
lead foreign binding         runtime_kind=codex-exec, native_thread_id=01a0a5ed-a95b-74c1-96f9-0e24ad604800, persisted on attempt 1
worker binding               runtime_kind=acp, qwen-code 0.23.4, protocol v1, capabilities persisted
final task refs              root 1 -> task 2
am final after reopen        reproduced the durable answer from a fresh process
resume of the succeeded root byte-identical board state (no replay, no new events)
```

Codex's own rollout file for that foreign thread
(`~/.codex/sessions/2026/09/16/rollout-2026-09-16T00-36-51-01a0a5ed-....jsonl`)
shows one thread containing: the round-0 `delegate` reply, then the round-1
`complete` reply that AM rejected strictly for a missing `selected_artifacts`
field, then AM's single same-thread repair prompt, then the accepted `complete`.
That is same-thread continuity plus the authoritative AM parser in one artifact.

Provider limitation, recorded separately and non-blocking: codex-cli 0.154.0
`--output-schema` cannot express the Lead's discriminated union, so the Lead
relies on the prompt contract while AM's parser/validator stays authoritative.

### Claude CLI

```text
deterministic=PASS (verified argv --bare -p --output-format stream-json --verbose --include-partial-messages; thinking and raw tool payloads dropped; permission/subagent topology normalization; absolute deadline)
live worker turn=PASS (real claude 2.1.268 turn through the scheduler-facing driver: summary returned, claude-cli binding persisted, lifecycle completed, only visible events durable)
live resume=PASS (a second attempt holding the same foreign session resumed it; the persisted session id is unchanged)
live cleanup=PASS (deadline reaps the Claude process group and its grandchild)
claude_cli_lead=UNSUPPORTED (no strict structured decision contract exists for this runtime)
```

### Compatibility boundary

`codex app-server` compatibility is unchanged and remains the v0.3 path; it is
not the v0.4 default. ACC was not deleted or redesigned.

## Lifecycle, deadline and recovery

```text
absolute_deadline=PASS (ACP, Codex exec, Claude CLI and Codex app-server all fail at a single monotonic budget)
slow_drip=PASS (streamed output cannot extend the absolute deadline in any process family: ACP slow-drip plus new Codex exec and Claude streaming regressions, each of which fails when the deadline is allowed to slide per event)
cancel=PASS (ACP peer-confirmed cancel for Qwen, Kimi and OpenCode; Codex exec and Claude have no protocol cancel, so their bounded outcome is deadline + process-group termination)
process_tree_cleanup=PASS (ACP wrapper + grandchild, Codex exec wrapper + grandchild, Claude wrapper + grandchild; two of the three are new regressions that fail when group termination is removed)
no_orphan=PASS (no owned descendant survived a timeout or cancellation in any family; the live E2E and probes left no stray child)
recovery_no_replay=PASS (TeamRunner interrupted-descendant recovery, idempotent succeeded-root resume, real isolated Qwen ACP crash recovery, and the E2E resume that reproduced byte-identical board state)
event_replay=PASS (durable CLI projection lists every observation exactly once — new regression; --follow fails closed at capacity instead of replaying)
```

## Runtime matrix

| Agent | Executable | Version | Adapter | Protocol | Probe | Events | Resume | Cancel | Deadline/Cleanup |
|---|---|---|---|---|---|---|---|---|---|
| Codex Exec | `/home/yhshy/.local/bin/codex` | 0.154.0 | codex-exec | JSONL | PASS (real TeamRunner Lead run + real TeamRunner worker) | PASS (thread, usage, command/file summaries; no raw payloads) | PASS (same foreign thread resumed through `exec resume --json`) | bounded (no protocol interrupt; process-group termination) | PASS (absolute deadline + group reap) |
| Qwen | `/home/yhshy/.npm-global/bin/qwen --acp` | 0.23.4 | acp | v1 | PASS (5 live probes at this head: auth-required, bounded task, same-session follow-up, resume, isolated coding task) | PASS (durable normalized events + binding) | PASS | PASS (peer-confirmed `cancelled`) | PASS |
| Kimi | `/home/yhshy/.local/bin/kimi acp` | 0.43.1 | acp | v1 | PASS (3 live probes at this head, isolated `KIMI_CODE_HOME`) | PASS (including a real permission request/resolution) | PASS | PASS (peer-confirmed) | PASS |
| OpenCode | `/tmp/opencode-iso/node_modules/.bin/opencode acp` (isolated) | 1.18.31 | acp | v1 | PASS (3 live probes at this head: bounded, resume, active-session cancel) | PASS (file_changed, tool, usage events for a real turn) | PASS | PASS (peer-confirmed once the peer's turn is active) | PASS |
| Claude | `/home/yhshy/.local/bin/claude` | 2.1.268 | claude-cli | stream-json | PASS (live worker turn + resume) | PASS (visible only; thinking dropped) | PASS | bounded (no protocol cancel; process-group termination) | PASS |

Environment facts recorded, not worked around by weakening the product:

```text
kimi     the user's global ~/.kimi-code/config.toml ends with [thinking] enabled=false,
         so an ACP session binds thinkingEffort=off and this kimi-code build refuses a
         reasoning-by-default model ("declares no off effort"). The user's configuration
         was not modified; the conformance probes ran under an isolated KIMI_CODE_HOME
         that omits that section.
opencode the user's installation is 1.18.7, below the 1.18.30 research floor, and has no
         provider configured. An isolated side-by-side 1.18.31 install (npm prefix under
         /tmp, isolated XDG dirs, provider key read from the environment) was used instead.
         The user's installation hash and mtime are unchanged.
```

## Heterogeneous E2E

Real topology: Codex Exec Lead + Qwen ACP + Kimi ACP + OpenCode ACP, driven by
the public `am run` entrypoint.

```text
root_task=1 (reasoning, lead)
lead=codex-exec, foreign thread 01a0a614-9b8a... persisted on the running root attempt
qwen_task=2  (bulk, qwen-worker)  -> qwen-result.txt     sha256 8da41ead27fa6d0ad9abec8ac22326698fcc368e5c4a2fd64fe7bdd1c8955e6b
kimi_task=3  (bulk, kimi-worker)  -> kimi-result.txt     sha256 cdce3f0a3dc212e9d334abacd6a3ec19f23397a5a6dc9b74c989d113ae48228f
opencode_task=4 (bulk, opencode-worker) -> opencode-result.txt sha256 1356d0b1e4b1512143eb037c95b0136bde9a570cbd4a9e0ef762ec7129a0f6c9
all_children_succeeded=true (one attempt each, no retries)
worker_bindings=qwen-code 0.23.4 acp/1, Kimi Code CLI 0.43.1 acp/1, OpenCode 1.18.31 acp/1 (foreign session ids persisted per attempt)
runtime_events=9 (qwen), 12 (kimi, including permission_requested/resolved), 15 (opencode)
artifact_files=every recorded digest matched the file on disk (sha256sum -c PASS); no manual copy/paste
final_refs=3 selected task ids + 3 selected artifacts with exact paths and digests
lead_synthesis=root answer "qwen-result.txt, kimi-result.txt, opencode-result.txt"
restart_final=fresh am status/final/events/artifact processes reconstructed run, answer, refs and digests
resume_no_replay=resume-team on the succeeded root reproduced the answer and refs with byte-identical board state; artifacts unchanged
runtime_event_authority=no runtime event settled any task; the new storage regression proves a success-looking observation leaves task truth untouched
manual_copy_paste=false
```

Privacy on the public surfaces: durable events carry bounded visible summaries
and short foreign ids only; no thinking/thought chunks, raw wire frames or
credentials are persisted, and foreign session ids never become canonical task
ids.

## Compatibility

```text
codex_app_server_compat=PASS (v0.3 behaviour preserved; not the v0.4 default)
v0.3_release_untouched=PASS (annotated tag object 0560f388c976a2a1318fd7c923e14e410d04997c -> commit fb8cc9080584ed2687576fba406a4cff6dbbce2c, unchanged; no v0.4.0 tag or Release created)
old_driver_strings_restorable=PASS
published_v03_fixture_bytes=PASS (SHA-256 unchanged through every storage change)
```

## Final

```text
DETERMINISTIC_RUNTIME_FOUNDATION_READY=true
LIVE_HETEROGENEOUS_E2E_READY=true
AGENT_RUNTIME_INTEGRATION_READY=true
V0_4_IMPLEMENTATION_COMPLETE=true
PR_IMPLEMENTATION_READY=true
PR_MERGED=false
V0_4_RELEASE_CREATED=false
```

PR #15 stays Draft; converting it to Ready for Review is left to explicit user
authorization.
