# v0.4 Runtime Integration Evidence

## Baseline

```text
branch=feat/v0.4-runtime-integration
start_sha=c90ec9560f80cc8f956057e686fd20d96efb93a4
end_sha=IN_PROGRESS
workspace_version=0.4.0-dev
schema_version=12
dirty_state_preserved=true (baseline was clean)
baseline_canonical_gates=PASS
```

## Migration

```text
v11_to_v12=PASS (additive binding columns + runtime_events)
v0.3_db_compat=PASS (published am 0.3.0 fixture SHA-256 e7b430ad17b8c2be3704f540ca921c77b2ba063c9661300411fbda51334adc3b)
v8_to_v12=PASS (authentic checked-in v8 fixture)
```

## Architecture gates

```text
runtime_event_foundation=PASS
role_runtime_decoupled=PASS (LeadBrainFactory; unsupported Lead runtimes fail before root creation)
generic_acp=PASS (typed ACP v1 adapter; exact running-attempt binding; capability-gated resume; normalized durable events; deny-by-default permissions)
absolute_deadline=PARTIAL (ACP, Codex exec, Claude CLI, and Codex app-server deterministic deadline regressions pass)
process_tree_cleanup=PARTIAL (ACP/Codex exec/Claude process groups plus Codex app-server bounded stderr and Drop reaping are covered; a real local Codex app-server interrupt boundary passed; full cancellation-race matrix remains)
codex_exec_runtime=PASS (durable driver, JSONL normalization, bounded process group, thread binding, resume argv, managed strict worker-result schema with fail-closed parsing, fresh/resume real `exec --json --output-schema` probes, and real public TeamRunner scheduler-driver test with durable binding and persisted final result)
codex_exec_lead=PARTIAL (strict shared decision parser/validator; root running-attempt binding is persisted on thread.started and restored across a new Lead instance via exec resume; one same-thread repair and deterministic fixtures pass. Real 0.154.0 probes reject the required discriminated `oneOf` schema and also require every property, making its response-schema dialect unable to express the Lead's mutually exclusive wire contract; parser remains the fail-closed authority.)
claude_cli_runtime=PARTIAL (verified stream-json argv; bounded JSONL supervisor; durable worker driver/session binding/resume; permission and parent-tool topology normalization; deterministic SQLite integration test; CLI registration and non-invasive doctor probe)
claude_cli_lead=NOT_SUPPORTED (worker runtime is intentionally not accepted as a Lead until a strict structured decision contract is implemented and tested)
cli_observability=PASS (`am events <TASK_OR_RUN> [--json] [--follow]` provides bounded durable replay/live handoff with privacy-filtered summaries and short binding IDs)
tui_observability=PASS (read-only board projects bounded durable task/attempt/agent/runtime observations with privacy-filtered summaries; direct projection test proves foreign session IDs never render)
recovery_no_replay=PARTIAL (TeamRunner interrupted-descendant recovery and succeeded-root idempotency tests pass; real isolated Qwen ACP crash recovery resumes without replay; `am events --follow` now fails closed rather than clearing cursors and replaying durable observations; full cross-runtime cancellation/recovery matrix remains)
```

## Deterministic tests

```text
fmt=PASS (full workspace canonical gate after the retained live-TeamRunner receipt)
clippy=PASS (full workspace canonical gate after the retained live-TeamRunner receipt)
test=PASS (full workspace canonical gate after the retained live-TeamRunner receipt; authenticated live cases intentionally ignored there and are recorded separately below)
release_build=PASS (full workspace canonical gate after the retained live-TeamRunner receipt)
diff_check=PASS (full workspace canonical gate after the retained live-TeamRunner receipt)
```

## Runtime matrix

| Agent | Executable | Version | Adapter | Protocol | Probe | Event conformance | Resume | Cancel | Notes |
|---|---|---|---|---|---|---|---|---|---|
| Codex | `/home/yhshy/.local/bin/codex` | 0.154.0 | codex-exec | JSONL | PASS (logged-in local app-server interrupt, fresh/resume strict schema probes, and real TeamRunner worker driver, 2026-09-15) | PASS (real strict worker results, thread/usage, durable binding, and deterministic normalizer coverage) | PASS (same foreign thread successfully resumed through real `exec resume --json --output-schema`) | PARTIAL (real app-server interrupt boundary passed) | exact exec contract pin; Codex Exec Lead is separately partial because pinned response-schema dialect cannot represent its discriminated contract |
| Qwen | `/home/yhshy/.npm-global/bin/qwen` | 0.23.4 | acp | v1 | PASS (real TeamRunner worker and utility tasks, plus isolated ACP crash-recovery/no-replay test, 2026-09-15) | PARTIAL (real worker result/artifact/binding plus deterministic ACP v1 mapping suite) | PASS (real isolated Qwen ACP crash recovery resumes without replay) | PARTIAL (deterministic cancel/process-tree suite) | installed version matches research reference; successful TeamRunner receipt retains only non-secret assertions |
| Kimi | `/home/yhshy/.local/bin/kimi` | 0.43.1 | acp | v1 | PASS (real read-only v1 initialize probe, 2026-09-15) | NOT_RUN (terminal login advertised; no authenticated prompt sent) | NOT_RUN (no authenticated session created) | NOT_RUN (no authenticated session created) | newer than research pin 0.43.0; probe reported loadSession plus list/resume/close/delete/fork capabilities |
| OpenCode | `/home/yhshy/.opencode/bin/opencode` | 1.18.7 | acp | v1 | NOT_RUN (installed below research pin 1.18.30) | NOT_RUN (version below pin) | NOT_RUN (version below pin) | NOT_RUN (version below pin) | `opencode acp` command is present, but its behavior is not accepted as pinned conformance evidence |
| Claude | `/home/yhshy/.local/bin/claude` | 2.1.268 | claude-cli | stream-json | PASS (real bare/dontAsk read-only stream probe, 2026-09-15) | PARTIAL (real init, nested text/thinking frames, assistant/result, usage/cost; deterministic permission/topology and durable-worker SQLite tests) | PASS (same foreign session successfully resumed through real bare/dontAsk `--resume`) | NOT_SUPPORTED | verified `--bare -p --output-format stream-json --verbose --include-partial-messages --json-schema --resume --permission-mode --permission-prompts none`; real probe confirmed thinking is observed but not persisted |

## Heterogeneous E2E

```text
lead=Codex app-server (real public `am run-team` product entrypoint)
workers=Qwen ACP worker + Qwen ACP utility (both real local processes; no manual copy/paste)
root_task_id=1 (successful real run on 2026-09-15; non-secret receipt retained)
delegated_task_ids=2 (worker), 3 (utility), both succeeded as root children
artifact_refs=PASS (worker task 2 selected `worker.txt`; SHA-256 `54aaa002d6b8b1d91535bc3a058e739f0dba4d3c889e3418bfde1f2cc76539ba` matched the written file)
restart_final_reconstruction=PASS (fresh `am final` and fresh `am resume-team` reproduced the answer and exact task/artifact refs)
runtime_event_replay=PARTIAL (durable CLI replay is task/run scoped and sequence ordered; follow observer is no-replay or fails closed at capacity)
manual_agent_to_agent_copy_paste=false
```

## Compatibility

```text
codex_app_server_compat=PASS (G2 deterministic product path plus real authenticated Lead planning and durable utility follow-up probe, 2026-09-15)
v0.3_release_untouched=PASS (local annotated tag object `0560f388c976a2a1318fd7c923e14e410d04997c`, targeting `fb8cc9080584ed2687576fba406a4cff6dbbce2c`, matches the v0.3 audit)
old_driver_strings_restorable=PASS (registry round-trip test covers native, acp, cli, codex-app-server, codex-exec, and claude-cli)
```

## Final

```text
DETERMINISTIC_RUNTIME_FOUNDATION_READY=false
LIVE_HETEROGENEOUS_E2E_READY=false
AGENT_RUNTIME_INTEGRATION_READY=false
```

## Remaining blockers

Implementation is in progress; no STOP-HARD condition has been observed.

The v0.3 compatibility fixture was created by the published `am 0.3.0`
binary (`f8a683bb9eb00bd8d1f932cc27808192e45ff223afc613aab37e0eaf01c7bbf0`)
against deterministic Codex app-server and ACP peers. Before migration it is
schema v11 and contains a completed root/worker run, two attempts, one
artifact, final task/artifact references, two registry rows and an ACP foreign
binding. The migration test copies it before opening, so the frozen fixture is
never modified by v0.4 code.
