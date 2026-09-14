# v0.4 Runtime Integration Evidence

## Baseline

```text
branch=feat/v0.4-runtime-integration
start_sha=c90ec9560f80cc8f956057e686fd20d96efb93a4
end_sha=IN_PROGRESS
workspace_version=0.4.0-dev
schema_version=11 (G1 target: 12)
dirty_state_preserved=true (baseline was clean)
baseline_canonical_gates=PASS
```

## Migration

```text
v11_to_v12=IN_PROGRESS
v0.3_db_compat=IN_PROGRESS
v8_to_v12=IN_PROGRESS
```

## Architecture gates

```text
runtime_event_foundation=IN_PROGRESS
role_runtime_decoupled=IN_PROGRESS
generic_acp=IN_PROGRESS
absolute_deadline=IN_PROGRESS
process_tree_cleanup=IN_PROGRESS
codex_exec_runtime=IN_PROGRESS
codex_exec_lead=IN_PROGRESS
claude_cli_runtime=IN_PROGRESS
claude_cli_lead=IN_PROGRESS
cli_observability=IN_PROGRESS
tui_observability=IN_PROGRESS
recovery_no_replay=IN_PROGRESS
```

## Deterministic tests

```text
fmt=PASS (baseline)
clippy=PASS (baseline)
test=PASS (baseline)
test_discovered=445
test_passed=428
test_failed=0
test_ignored=17
release_build=PASS (baseline)
diff_check=PASS (baseline)
```

## Runtime matrix

| Agent | Executable | Version | Adapter | Protocol | Probe | Event conformance | Resume | Cancel | Notes |
|---|---|---|---|---|---|---|---|---|---|
| Codex | `/home/yhshy/.local/bin/codex` | 0.154.0 | codex-exec | JSONL | pending | pending | pending | pending | exact contract pin |
| Qwen | `/home/yhshy/.npm-global/bin/qwen` | 0.23.3 | acp | v1 | pending | pending | pending | pending | installed version below research reference |
| Kimi | `/home/yhshy/.local/bin/kimi` | 0.42.0 | acp | v1 | pending | pending | pending | pending | installed version below research reference |
| OpenCode | `/home/yhshy/.opencode/bin/opencode` | 1.18.7 | acp | v1 | pending | pending | pending | pending | installed version below research reference |
| Claude | `/home/yhshy/.local/bin/claude` | 2.1.268 | claude-cli | stream-json | pending | pending | pending | pending | exact local CLI contract to be probed |

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
codex_app_server_compat=pending
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
