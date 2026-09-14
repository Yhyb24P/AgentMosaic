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
absolute_deadline=IN_PROGRESS
process_tree_cleanup=IN_PROGRESS
codex_exec_runtime=PARTIAL (durable driver, JSONL normalization, bounded process group, thread binding, resume argv, and deterministic scheduler test)
codex_exec_lead=PARTIAL (strict shared decision contract; root running-attempt binding is persisted on thread.started and restored across a new Lead instance via exec resume; one same-thread repair and deterministic fixtures pass; strict output-schema/live probe remain)
claude_cli_runtime=IN_PROGRESS
claude_cli_lead=IN_PROGRESS
cli_observability=IN_PROGRESS
tui_observability=IN_PROGRESS
recovery_no_replay=IN_PROGRESS
```

## Deterministic tests

```text
fmt=PASS (full workspace canonical gate after Codex exec work)
clippy=PASS (full workspace canonical gate after Codex exec work)
test=PASS (full workspace canonical gate after Codex exec work; authenticated live cases intentionally ignored)
release_build=PASS (full workspace canonical gate after Codex exec work)
diff_check=PASS (full workspace canonical gate after Codex exec work)
```

## Runtime matrix

| Agent | Executable | Version | Adapter | Protocol | Probe | Event conformance | Resume | Cancel | Notes |
|---|---|---|---|---|---|---|---|---|---|
| Codex | `/home/yhshy/.local/bin/codex` | 0.154.0 | codex-exec | JSONL | pending | pending | pending | pending | exact contract pin |
| Qwen | `/home/yhshy/.npm-global/bin/qwen` | 0.23.3 | acp | v1 | pending | pending | pending | pending | installed version below research reference |
| Kimi | `/home/yhshy/.local/bin/kimi` | 0.42.0 | acp | v1 | pending | pending | pending | pending | installed version below research reference |
| OpenCode | `/home/yhshy/.opencode/bin/opencode` | 1.18.7 | acp | v1 | pending | pending | pending | pending | installed version below research reference |
| Claude | `/home/yhshy/.local/bin/claude` | 2.1.268 | claude-cli | stream-json | PASS (2026-09-15 version/help probe) | pending | pending | pending | verified `--bare -p --output-format stream-json --verbose --include-partial-messages --json-schema --resume --permission-mode --permission-prompts none`; no live prompt sent |

## Heterogeneous E2E

```text
lead=pending
workers=pending
root_task_id=pending
delegated_task_ids=pending
artifact_refs=pending
restart_final_reconstruction=pending
runtime_event_replay=pending
manual_agent_to_agent_copy_paste=false
```

## Compatibility

```text
codex_app_server_compat=PASS (G2 deterministic product path)
v0.3_release_untouched=pending final verification
old_driver_strings_restorable=pending
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
