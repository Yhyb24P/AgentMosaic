# Current status

Current source facts. Public release status is separate from the local development tree.

| Item | Value |
|---|---|
| Latest published release | v0.3.0; schema 11 |
| Current source version | 0.5.0 release candidate |
| Source SQLite schema | 14; `.agentmosaic/state-v14.db` |
| Product binary | am |
| Lead adapter | codex-exec |
| Worker adapters | acp (ACP v1), codex-exec, claude-cli |

The current source is a breaking subtraction candidate, not a newly published release.
Existing state is upgraded with explicit [import](migration.md); the original is preserved.

## Normal path

```text
am init
am agent add lead --role reasoner --adapter codex-exec -- codex
am agent add worker --role worker --adapter acp -- qwen --acp
am doctor
am run "<objective>"
am status / am events / am final / am artifact / am tui
am run --recover <run-id>    # explicitly settle after the controller has stopped
am run --resume <run-id>
```

SQLite owns task/attempt/result/artifact truth. Root final settlement is atomic; attempts
are append-only and successful descendants are never replayed. Runtime events provide
bounded progress observations. External runtimes own models, authentication and tools.

## Product reduction

The compatibility CLI and app-server/MCP bridge are removed. Current DriverKind has only
ACP, exec and Claude. Retired persisted driver strings stay inspectable and cannot run.
Fresh state has eight business tables; session/ACC, messages and collaboration receipts
are absent. Lease/DB-owner experimental code and task briefs are outside the product tree.

Actor Scheduler, Experience/DecisionEngine, adaptive routing and WorkspaceLease integration
remain paused. Development Agent self-lock is a task orchestration issue and creates no
AgentMosaic Execution Contract, NeedsReplan or audit-loop feature.

## Limits

Prebuilt releases target Linux x86_64. Live Codex/Qwen checks require externally authenticated
runtimes and are not automatic CI. Explicit recovery assumes the controller is stopped;
it does not prove ownership or terminate a live controller. No tag, release or deployment
is part of this subtraction.
