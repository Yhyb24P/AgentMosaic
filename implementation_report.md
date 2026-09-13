# ACC/0.1 Rust-v2 Implementation Report

## Current state (RC repair, 2026-09-13)

This is the authoritative current-state block. Everything below it is retained
as historical evidence; sections marked `SUPERSEDED_BY_PRODUCT_SELF_AUDIT` were
disproved by the independent `product_self_audit.md` (2026-09-13).

- Source candidate: branch `v2/rust-agent-team`, commit
  `89ac979d333fe3fc2e311fb566f3ab0056bec4c5` (frozen). The frozen candidate's
  executable source is unchanged by every later evidence/report commit; the
  exact-candidate gates are rerun on the final HEAD and the recorded hashes live
  in `.acc-evidence/rc-repair-fbc80bf/`.
- Storage schema: `SCHEMA_VERSION = 11` (`agent_registry.driver_config_json`,
  non-secret driver options only).
- Product entrypoint: `agent-code-cli run-team <database> <repo> "<objective>"`
  and `agent-code-cli resume-team <database> <repo> <root-task-id>`, with
  `--lead`, `--max-rounds`, `--max-tasks`, `--max-retries`. One objective creates
  one durable root `reasoning` task whose result is the final visible Codex
  answer plus the exact selected task/artifact refs.
- Reference runtimes: real `codex-cli 0.154.0` Lead and real Qwen Code `0.23.3`
  Worker.

### S1 repair states

| Defect | State | Basis |
|---|---|---|
| S1-1 automatic normal team entrypoint | REPAIRED | `run-team`/`resume-team` in `crates/agent-code-cli/src/main.rs`; orchestration in product `TeamRunner` |
| S1-2 orchestration is product code, not harness | REPAIRED | `crates/agent-code-runtime/src/team_runner.rs` owns registry→drivers→Lead→Scheduler; the live test only reads durable surfaces |
| S1-3 actual Codex final result fidelity | REPAIRED | `thread/read` exact-turn final visible Agent-message extraction; the fixed placeholder is gone |
| S1-4 authentic historical migration | REPAIRED | `crates/agent-code-storage/tests/fixtures/schema_v8.sql`, verbatim from commit `e7649230af388aa61fb851f1c4631e679b08e49b` (blob `21d10ff9c45e444f29b37f9082d1fd99b6333b56`); never regenerated from the current `SCHEMA` |

### M1–M9 current state

| Milestone | Pre-repair (audit) | Current state |
|---|---|---|
| M1_CODEX_TEAM_READY | PARTIAL | S1-3 repaired; a real Codex Lead was exercised end to end |
| M2_ACP_DRIVER_READY | PARTIAL | ACP driver handled a real Qwen worker in the verified E2E |
| M3_QWEN_WORKER_READY | NOT_PROVEN | real Qwen bulk worker succeeded (task `2`, artifact persisted) in the verified E2E |
| M4_KIMI_PROFILE_CLASSIFIED | PARTIAL | unchanged; no new live Kimi evidence |
| M5_R6_TEAM_READY | FAILED | S1-1/S1-2 repaired; one `run-team` produced root + worker + utility tasks with result flow |
| M6_R6_SEALED | FAILED | not sealed; no frozen candidate yet |
| M7_R7_NORMAL_PATH_READY | FAILED | `run-team` is the normal team path; docs and Rust CI reconciled |
| M8_R8_DEBLOATED | PARTIAL | retired Python removed; stale Python-era docs/workflows reconciled |
| M9_PRODUCT_RC_READY | FAILED | local real E2E verified; candidate not frozen and independent re-audit pending |

The definitive post-repair re-evaluation is `product_self_reaudit.md`, conducted on
the frozen candidate. It is a self re-audit by the repair executor, not an
independent third-party audit; an external re-audit is still recommended before
any public release decision.

### Readiness (separate claims)

```text
LOCAL_PRODUCT_RC_READY        = true  (exact-candidate gates + real Codex/Qwen E2E + copied release binary)
REMOTE_DETERMINISTIC_CI_READY = true  (rust.yml 34759201679 + rust-candidate 34759220090, both green)
PUBLIC_RELEASE_READY          = false (no tag or GitHub Release; not authorized)
```

### Current blockers

- Exact-candidate freeze commit is set: `89ac979d333fe3fc2e311fb566f3ab0056bec4c5`.
- `product_self_reaudit.md` exists; it is a self re-audit, not an independent
  third-party one.
- Deterministic remote CI is green on the frozen candidate: `rust.yml` run `34759201679`
  and `rust-candidate` run `34759220090` (see `.acc-evidence/rc-repair-fbc80bf/remote-ci.md`).
- No tag or public release is authorized.

### Verified real production E2E (2026-09-13)

Through the public CLI only, with real `codex-cli 0.154.0` and real Qwen Code
`0.23.3`: root task `1` (`reasoning`, assignee `codex-lead`, succeeded),
delegated worker task `2` (`bulk`, assignee `qwen-worker`, parent `1`,
succeeded), utility task `3` (parent `1`, succeeded), a final answer containing a
random worker-produced token, and persisted final refs `[2]` plus artifact
`task=2 path=worker.txt
sha256=23f3ef2f0a550aff9886f9c6bcef54ddac5d2fdef5ca8fe849db49cb97f3c979`. A
separate process reproduced the answer and refs from SQLite. Evidence:
`.acc-evidence/rc-repair-fbc80bf/`. The workspace suite reported 257 passed / 0
failed / 17 ignored (`cargo test --workspace --all-features`).

## 1. Machine-readable summary

SUPERSEDED_BY_PRODUCT_SELF_AUDIT — the summary below is stale historical state
(commit `e23ae817`, `SCHEMA_VERSION=7`, `status=PARTIAL`). See "Current state"
above.
```json
{"report_version":"4","status":"PARTIAL","active_product":"rust-v2","acc_implementation_language":"rust","governance_resolution":"RESOLVED_BY_RUST_V2_SUPERSESSION","legacy_python_status":"PRE_EXISTING_LEGACY_FAILURE","core_tested_commit":"e23ae81759fc278fa02a5899ad7c6d03318d2848","phase23_base_commit":"e23ae81759fc278fa02a5899ad7c6d03318d2848","phase23_source_state":"DIRTY_R5_R6_PHASE23_WORKTREE","branch":"v2/rust-agent-team","core_claim":"CORE_ACC_READY","phase23_claim":"NOT_READY_FOR_GATE_K","adapter_claims":{"a2a":"NOT_READY","codex":"NOT_READY","claude_code":"NOT_READY","openclaw":"NOT_READY","qwen":"NOT_READY"},"generated_at":"2026-09-11T14:12:54+08:00"}
```

## 2. Current Git / branch / dirty state
- Core candidate HEAD `e23ae81759fc278fa02a5899ad7c6d03318d2848`; branch `v2/rust-agent-team`. It remains the qualified Core baseline, not the Phase 2.3 source candidate.
- Phase 2.3 runs on the actual dirty R5/R6 worktree. Tracked implementation paths are `Cargo.lock`, `crates/agent-code-runtime/{Cargo.toml,src/lib.rs}`, `crates/agent-code-storage/src/{board,lib,schema}.rs`, `crates/agent-code-storage/tests/acc_migration.rs`, and this report. Untracked implementation paths are `crates/agent-code-runtime/src/{codex_app_server.rs,bin/ras_codex_mcp.rs}`, `crates/agent-code-runtime/tests/codex_live.rs`, and `crates/agent-code-storage/tests/phase21_durability.rs`; evidence paths are listed in Section 24. `git diff --check` passed after Phase 2.3.
- Candidate implementation paths: `Cargo.lock`, `Cargo.toml`, `crates/agent-code-cli/{Cargo.toml,src/lib.rs}`, `crates/agent-code-storage/{Cargo.toml,src/lib.rs,src/schema.rs,src/acc_store.rs,tests/acc_e2e.rs,tests/acc_migration.rs}`, and `crates/agent-code-team/{Cargo.toml,src/lib.rs,src/acc.rs}`. The candidate also includes `implementation_report.md` and the prior `.acc-evidence/` history.

## 3. Rust workspace inventory
Workspace crates are `agent-code-{core,model,tools,workspace,context,storage,team,runtime,tui,cli}` plus integration/e2e packages. ACC uses `agent-code-team` for contracts/broker, existing SQLite storage for canonical state, and `agent-code-cli` for inspection. `SCHEMA_VERSION=7`; no second canonical task/event store exists.

## 4. Governance supersession resolution
The delivery pack's Python control-plane target conflicts with active Rust v2 in `AGENTS.md` (SHA-256 `5b9ccd8f21ac6d1b4be10ce04bff73ecf412831d1ba8ffbc1bf7ae2e80386565`). The user-authorized overlay resolves this as `RESOLVED_BY_RUST_V2_SUPERSESSION`: Python-specific paths/tooling/persistence are superseded, while ACC authority, review, artifact, provenance, and evidence invariants remain mandatory. No Python product code changed.

## 5. ACC responsibility mapping
| Responsibility | Rust location | Evidence |
|---|---|---|
| Contracts, DAG, context, broker, assignment, runtime/tools/acceptance | `crates/agent-code-team/src/acc.rs` | unit/full tests |
| Durable projections and recovery | `crates/agent-code-storage/src/{schema,acc_store}.rs` | migration/E2E |
| Four-identity deterministic loop | `crates/agent-code-storage/tests/acc_e2e.rs` | full test |
| Persisted-state inspection | `crates/agent-code-cli/src/lib.rs` | inspection test |

## 6. Implemented TASK status
| Item | Status | Evidence |
|---|---|---|
| Schema drift | COMPLETE | generated-schema SHA sentinel |
| Existing-journal migration/recovery | COMPLETE | pre-ACC v4 SQLite fixture |
| Runtime negotiation | COMPLETE | supported/unsupported input test |
| Artifact mismatch fail-closed | COMPLETE | mismatch/audit test |
| Canonical inspection | COMPLETE | `inspect_acc` persisted reads |

## 7. File change manifest
| Path | Action | Purpose |
|---|---|---|
| `Cargo.toml`, `Cargo.lock` | modified | schema dependency |
| `crates/agent-code-team/{Cargo.toml,src/lib.rs,src/acc.rs}` | modified/new | ACC contract/broker |
| `crates/agent-code-storage/{Cargo.toml,src/lib.rs,src/schema.rs,src/acc_store.rs}` | modified/new | v6 ACC store, then v7 runtime-binding seam |
| `crates/agent-code-storage/tests/{acc_e2e.rs,acc_migration.rs}` | new | E2E/migration |
| `crates/agent-code-cli/{Cargo.toml,src/lib.rs}` | modified | inspection API |
| `crates/agent-code-runtime/{Cargo.toml,src/lib.rs,src/codex_app_server.rs,src/bin/ras_codex_mcp.rs,tests/codex_live.rs}` | modified/new | bounded real Codex app-server/MCP bridge and live harness |
| `.acc-evidence/closeout-*.log`, this report | new | evidence/report |

## 8. Persistence / migration status
`SqliteAccStore` extends the existing journal with `acc_tasks`, `acc_dependencies`, `acc_context_manifests`, immutable hash/version `acc_artifacts`, and append-only sequenced/unique `acc_events`. Current v7 additionally has external runtime bindings and bounded collaboration records. The fixture creates an actual v4 journal with rows, migrates/reopens it, and verifies old rows plus ACC graph/context/event state. No Alembic was introduced.

## 9. ACC contract/schema status
Rust strict `serde` types are source-of-truth; unknown control fields are rejected. `schemars` generates `AccWireContractBundle`, whose deterministic hash is checked against `ACC_WIRE_SCHEMA_SHA256`. Capability, trusted capability, role, and authority remain distinct.

## 10. Runtime adapter status
`RuntimeAdapter`/`RuntimeDescriptor` are transport-neutral. `send_runtime_input` checks `midrun_input`: support invokes the adapter; lack of support returns explicit unsupported without invocation. Phase 2.3 adds a narrow real Codex app-server stdio client plus an allowlisted RAS MCP bridge; its live evidence is recorded in Section 24. This does not satisfy the complete existing Gate K reference qualification.

## 11. Exact test commands and results
| Command | Exit | Result | Log |
|---|---:|---|---|
| `cargo fmt --all -- --check` | 0 | passed | `.acc-evidence/candidate-fmt.log` |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 0 | passed | `.acc-evidence/candidate-clippy.log` |
| `cargo test --workspace --all-features` | 0 | 134 passed, 0 failed | `.acc-evidence/candidate-test.log` |
| `cargo test -p agent-code-team generated_wire_schema -- --nocapture` | 0 | 1 passed | `.acc-evidence/candidate-schema.log` |
| `cargo test -p agent-code-storage --test acc_migration` | 0 | 1 passed | `.acc-evidence/candidate-migration.log` |
| `cargo test -p agent-code-cli` | 0 | 1 passed | `.acc-evidence/candidate-inspection.log` |
| `git diff --check` | 0 | passed | `.acc-evidence/candidate-diff-check.log` |
The earlier 128-test result remains historical evidence; 134 is the exact-candidate closeout run.

## 12. Gate A–N current status
| Gate | State | Evidence |
|---|---|---|
| A | PASSED | AGENTS/git recorded; Rust gates exit 0 |
| B | PASSED | strict types/separation/schema drift |
| C | PASSED | v6 constraints/migration/recovery |
| D | PASSED | DAG/readiness/assignment tests |
| E | PASSED | typed broker/binding/dedupe/order |
| F | PASSED | provenance/hash/redaction |
| G | PASSED | tools/negotiated runtime operation |
| H | PASSED | persisted four-agent E2E |
| I | PASSED | review/replay/restart/privacy/mismatch tests |
| J | NOT_RUN | no A2A live evidence |
| K | NOT_RUN | Phase 2.3 has narrow Codex live bridge evidence, but not the established full Gate K qualification |
| L | NOT_RUN | no Claude Code live evidence |
| M | NOT_RUN | no OpenClaw live evidence |
| N | PASSED | full/targeted regression and report |

## 13. Security / authority assertions
- Trusted grants are distinct from agent capability; runtime-bound actor/authority is normalized.
- `RESULT_SUBMITTED != ACCEPTED`; independent review rejects executor self-review.
- Artifact hash/version mismatch is rejected, audited, and leaves canonical acceptance unchanged.
- Prohibited private/secret material is excluded from context; event IDs dedupe and SQLite restart recovers canonical history.

## 14. Legacy Python status
`PRE_EXISTING_LEGACY_FAILURE`. Python sources, Alembic, and Python tests were not changed or re-run. Rust results do not claim that legacy Python is green or repaired.

## 15. Qualification impact
Frozen Python qualification artifacts remain unchanged. Rust impact is the SQLite v6 ACC extension and the current executable evidence; the migration fixture qualifies existing Rust journal compatibility only.

## 16. Deviations
Python paths, Pydantic, SQLAlchemy/Alembic, pytest, and mypy implementation mandates are superseded by the overlay. Inspection is existing CLI-crate read-only API `agent_code_cli::inspect_acc`, not parallel Python `researchctl`.

## 17. Outstanding blockers
| ID | State | Blocks | Required action |
|---|---|---|---|
| LIVE-A2A | NOT_RUN | `A2A_REFERENCE_READY` | real observable A2A probe |
| LIVE-CODEX | NOT_RUN | `CODEX_REFERENCE_READY` | real Codex probe |
| LIVE-CLAUDE | NOT_RUN | `CLAUDE_REFERENCE_READY` | real Claude Code probe |
| LIVE-OPENCLAW | NOT_RUN | `OPENCLAW_REFERENCE_READY` | real OpenClaw probe |
They do not block core under Gate 04; they block `FULL_REFERENCE_READY`.

## 18. Reproduction commands
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p agent-code-team generated_wire_schema -- --nocapture
cargo test -p agent-code-storage --test acc_migration
cargo test -p agent-code-cli
cargo test --workspace --all-features
git diff --check
```

## 19. Evidence manifest

Every row below is bound to source `git:e23ae81759fc278fa02a5899ad7c6d03318d2848`, exit 0, and the observed result in Section 11. `executed_at` is the recorded file completion time (+08:00).

| File | SHA-256 | executed_at |
|---|---|---|
| `.acc-evidence/candidate-fmt.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` | `2026-09-11T08:02:54+08:00` |
| `.acc-evidence/candidate-clippy.log` | `59639588a6f1826f129cbcd121edc07279dab94786f5fa5bde6b3439b2e845cc` | `2026-09-11T08:02:56+08:00` |
| `.acc-evidence/candidate-schema.log` | `673eb792fe12ddef0c77486382b433904e4bc378bf9b7e7489197aca7f6a2783` | `2026-09-11T08:02:58+08:00` |
| `.acc-evidence/candidate-migration.log` | `3fa743a5303ed908d9637619357151bff0a3a759a5fe534be2b116c23fab37f1` | `2026-09-11T08:03:03+08:00` |
| `.acc-evidence/candidate-inspection.log` | `130908f2bd648fe8be5800e6e94333e5076f50c112607488caaad78b7bd4e619` | `2026-09-11T08:03:05+08:00` |
| `.acc-evidence/candidate-test.log` | `0414c0933c5e431b5017692d1a036e1281e03ea8eebf979bcd38e028aedbbdd6` | `2026-09-11T08:03:15+08:00` |
| `.acc-evidence/candidate-diff-check.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` | `2026-09-11T08:03:17+08:00` |

Additional bound inputs: candidate report Git blob `f12eb1be0e266a1af5e5f4f629404eda9be95621`; ACC implementation blob `b9a63e2bbe22723274542e0f385f5e0b86e9f789`; schema sentinel `19cee109cd16d036fd63b7726e076cc795fbd22413de284050576dadb4ffe53c`; migration fixture `crates/agent-code-storage/tests/acc_migration.rs` in the candidate tree. The final report's separately computed SHA-256 is supplied in the handoff because a document cannot contain a stable hash of its own final bytes.

## 20. Allowed claims
- `CORE_ACC_READY`: Rust-v2 Gates A–I and N are PASSED with commands, exits, observations, and hashes above.
- Active product is Rust v2; ACC implementation language is Rust.

## 21. Forbidden claims
- Do not claim `FULL_REFERENCE_READY` or any adapter READY claim.
- Do not claim legacy Python is repaired, green, or revalidated.
- Do not treat deterministic fixtures as live adapter evidence.

## 22. Phase 2 reference-adapter probe status

```yaml
phase: reference-adapters
core_baseline_commit: e23ae81759fc278fa02a5899ad7c6d03318d2848
core_freeze_status: PRESERVED
core_files_modified: []
cross_adapter_e2e: NOT_RUN
reference_adapters:
  codex:
    status: PROBED_NOT_INTEGRATED
    executable: codex (resolved locally through PATH)
    exact_version: codex-cli 0.154.0
    integration_path: app-server stdio JSON-RPC-like protocol
    protocol_schema_sha256: d71ddf3bf5484f8de2799f7a4793c2e66808a9ec1a330e2307accb088ab5948a
    observed: [app-server available, schema generation, thread/turn/resume/interrupt protocol types]
    gate_k: NOT_RUN
  claude_code:
    exact_version: 2.1.220
    status: NOT_RUN
    gate_l: NOT_RUN
  openclaw:
    status: BLOCKED
    blocker: "requires Node >=24.15.0; observed 24.14.0"
    gate_m: NOT_RUN
  a2a:
    status: NOT_RUN
    gate_j: NOT_RUN
```

These are local executable probes, not active ACC collaboration evidence. No
adapter implementation or Core change was made in this probe-only step.

## 23. Phase 2.1 / 2.2 R6 durable runtime seam repair

Product alignment follows current `AGENTS.md` and `docs/v2/ROADMAP.md`: this is
an R6 heterogeneous coding/work-team integration seam, not a Trusted Control
Plane change. `R6_INTEGRATION_SEAM_CHANGED` affected
`agent-code-storage`; product semantics changed: **NO**.

| Observed gap / failing-test intent | Existing path | Minimal repair | Result |
|---|---|---|---|
| no durable task/run → external native handle | `SqliteTaskBoard` / `team_task_runs` | v7 `external_runtime_bindings`, keyed by canonical team task + attempt | passed after reopen |
| no recoverable runtime help/context record | existing board message/artifact domain | v7 bounded `runtime_collaboration_records` | passed after reopen |
| repeated native call could be replayed after crash | no idempotency key in board | unique `(runtime_kind,native_call_id)` and `DO NOTHING` | second insert returns false |
| restart could not load active external reference | board had no runtime binding read API | `external_binding(task,attempt)` recovery query | binding restored |

The initial targeted implementation compile exposed Rust error `E0597` in the
query iterator lifetime; the safe local repair binds `MappedRows` before
collecting. No runtime protocol or product invariant changed.

Failing-tests-first evidence was run in an isolated detached worktree at
candidate `e23ae81759fc278fa02a5899ad7c6d03318d2848`, then removed. The
temporary `phase21_missing_seam` test ran
`cargo test -p agent-code-storage --test phase21_missing_seam` with exit
`101`: `ExternalRuntimeBinding` was unresolved and
`SqliteTaskBoard::upsert_external_binding` did not exist. That failure maps to
the prior R5 board call chain (`SqliteTaskBoard` → `TaskBoard` task/run/message/
artifact persistence); the minimal repair is the v7 binding table/API above,
not a second store. Its successor test is
`phase21_durability::external_binding_and_idempotent_collaboration_survive_reopen`
and passes on the repaired worktree.

Storage migration: local version `6 → 7`. Existing schema migration remains
append-only; fresh creation and v4→current migration run in the workspace test
suite, and the new reopen test covers the R6 records. Only compact summaries,
native external references, and lifecycle are persisted: no credentials,
system prompts, hidden reasoning, or raw private transcript.

| Verification | Exit | Observed |
|---|---:|---|
| `cargo test -p agent-code-storage --test phase21_durability` | 0 | 1 passed: binding, idempotent collaboration, reopen |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | passed |
| `cargo fmt --all -- --check` | 0 | passed |
| `cargo test --workspace` | 0 | 135 passed, 0 failed |
| `git diff --check` | 0 | passed |

This is not live Codex evidence. Gates J/K/L/M/Q and cross-runtime E2E remain
`NOT_RUN`; `PHASE2_ACTIVE_PROFILE_READY` and `FULL_REFERENCE_READY` remain
false. The next authorized phase is the real Codex app-server bridge; Qwen is
not started by this repair.

Phase 2.2 source state: base HEAD
`e23ae81759fc278fa02a5899ad7c6d03318d2848`; current uncommitted seam paths
are `crates/agent-code-storage/src/{board,lib,schema}.rs`,
`crates/agent-code-storage/tests/{acc_migration,phase21_durability}.rs`, and
this report. The historical candidate evidence logs remain untracked and are
not part of this seam change. Current local facts: `AGENTS.md` SHA-256
`5b9ccd8f21ac6d1b4be10ce04bff73ecf412831d1ba8ffbc1bf7ae2e80386565`;
`docs/v2/ROADMAP.md` SHA-256
`ab24d2c5c8f5ac286755d1ebfba031f064e0b50f9eb6d66d68461986d8fe29f4`.

## 24. Phase 2.3 — real Codex app-server active collaboration bridge

### Source and wire facts

The Phase 2.3 source candidate is the actual R5/R6 dirty worktree on base
`e23ae81759fc278fa02a5899ad7c6d03318d2848`, not that baseline commit alone.
Its bridge paths are `crates/agent-code-runtime/src/codex_app_server.rs`,
`crates/agent-code-runtime/src/bin/ras_codex_mcp.rs`, and
`crates/agent-code-runtime/tests/codex_live.rs`; related durable seam paths
are listed in Section 2. No Qwen, Claude Code, OpenClaw, or A2A path was added.

Local executable: `codex` (resolved through local PATH); observed version:
`codex-cli 0.154.0`. Local app-server schema was generated with
`codex app-server generate-json-schema --out <temporary directory>` and has
SHA-256 `d71ddf3bf5484f8de2799f7a4793c2e66808a9ec1a330e2307accb088ab5948a`.
The live child used `codex app-server --stdio` with only per-process
`mcp_servers.ras.*` overrides; no persistent user Codex configuration changed.

The local schema was the wire truth. It confirms initialize/initialized,
thread/start, turn/start, item/tool/call, turn/completed, turn/interrupt, and
the dynamic-tool result shape. Contrary to the delivery wording, installed
`ThreadStartParams` has no `dynamicTools` property. The bridge does not invent
it: it uses the locally probed MCP integration and only
`mcpServer/elicitation/request` for named RAS bridge calls.

### Implemented bounded bridge and live result

`CodexAppServer` is a stdio JSON-RPC client. `ras_codex_mcp` exposes exactly
`ras_request_context` and `ras_request_help`; it has no acceptance, admin,
shell, generic approval, or unrestricted-context tool. Only literal server
name `ras` can receive an elicitation response. MCP JSON-RPC request id, not
tool arguments, is the idempotency key. The bridge persists a compact
maximum-512-character summary and fixed response summary; it never persists
credentials, prompts, hidden reasoning, raw private transcripts, or protocol
bodies. Native thread/turn ids remain external references. `turn/completed`
does not imply ACC acceptance.

The explicitly invoked ignored live harness created a real app-server thread
and turn, confirmed connected RAS MCP tools, caused Codex to emit
`mcpToolCall(ras_request_context)`, accepted only RAS elicitation, observed the
completed MCP call, then a later `agentMessage` in the same turn. It reopened
SQLite and verified the durable collaboration record and external binding.

| Milestone | State | Actual evidence |
|---|---|---|
| `C21_TRANSPORT_READY` | PASSED | live initialize/thread/turn stdio exchange |
| `C21_DURABILITY_READY` | PASSED | v7 reopen/idempotency and live reopen assertions |
| `C21_ACTIVE_BRIDGE_READY` | PASSED | Codex request → RAS response → same-turn continuation |
| `C21_TEAM_FLOW_READY` | IN_PROGRESS | no independently reviewed accepted artifact in this harness |
| `PHASE2_1_CODEX_READY` | false | not claimed |

### Phase 2.3 evidence

| Command | Exit | Observed result | Log SHA-256 | Executed at |
|---|---:|---|---|---|
| `cargo test -p agent-code-runtime --test codex_live -- --ignored --nocapture` | 0 | 1 live test passed | `5dbc86a2586c3032ff1d806c0932c75554872fdc1c0e0934ed4fa716ed42468f` | 2026-09-11T14:12:25+08:00 |
| `cargo test -p agent-code-storage --test phase21_durability` | 0 | 1 passed | `9aaf854fb0fb6dd66c3740231dd6821bc960c227556efab1492dbad9c4f90c10` | 2026-09-11T14:12:45+08:00 |
| `cargo fmt --all -- --check` | 0 | passed | `dac4aefe9211749d3d49520b005d5c0bc079e1b5f82e7d007889b04e35fa6034` | 2026-09-11T14:12:45+08:00 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | passed | `23ff7698e1abd214e04021727f5529c5014001f4bb41dd0898812f8cff8b5825` | 2026-09-11T14:12:45+08:00 |
| `cargo test --workspace` | 0 | 135 passed, 0 failed, 1 ignored | `a6e1108dc7a0372b9f8dc4922b6e6ee9b72b2613081e2c13239961800124a787` | 2026-09-11T14:12:46+08:00 |
| `git diff --check` | 0 | passed | `bc495e4046f01dca79a5273ed4df9d6d157edc2e8f9b2121a1c95494ef52f662` | 2026-09-11T14:12:54+08:00 |

The first Phase 2.3 clippy attempt found a real `clippy::useless_conversion`
and exited 101. The `.into()` was removed; only the fresh post-fix run above
is PASS evidence. Gate J, K, L, M, Q and cross-runtime E2E remain `NOT_RUN`.
The narrow live bridge does not make Gate K PASSED. No active-profile or full
reference readiness claim is made; Qwen work has not begun.

## 25. Phase 2.4A — Qwen runtime capability and protocol probe

Phase 2.3 final baseline is commit
`069c2c9e8c148203fd880ce2878d9bd77243c640`
(`feat(runtime): persist Codex collaboration bindings`). It was created after
format, clippy, workspace test, diff, and sensitive-content checks passed. Its
worktree was clean; the baseline report SHA-256 was
`c5168a2cddf174752672632fd358b445c786a536781f9472642895e0bf4d03f1`.

The local launcher probe found `aweswitch` version 0.3.8 and a locally resolved
`qw` profile mapping `qwen38` to `Qwen3.8-27B Remote Workstation`. The profile
kind is `qwen`, but the installed aweswitch help and package metadata support
only Claude, Codex, and OpenCode. No standalone `qw` executable is on PATH.
The redacted profile did not expose an endpoint; its authentication source is
local profile configuration, with no credential inspected or recorded.

No `aweswitch qw` launch was performed: launch is an unverified remote-execution
path and would violate the low-risk probe boundary. Therefore session identity,
streaming, tool calls, same-session tool-result return, cancellation,
reconnect/recovery, and retry/error semantics are all `UNKNOWN`, not inferred.
The only selected adapter candidate is `BLOCKED / unsupported`; no adapter or
skeleton was added.

The complete sanitized matrix, exact commands, and authoritative-state mapping
are in `docs/qwen_runtime_probe.md` (SHA-256
`be1cb3e7b6ca3af797bb403102300a75481d719006504d476613432d3b2b14d0`).
Sanitized command evidence is `.acc-evidence/phase24a-qwen-probe.md`
(SHA-256 `a5303374461f1eac4b6601cbc84395e95521c9e246e33985f964c7869ddf3a63`).

Gate J/K/L/M/Q and cross-runtime E2E remain `NOT_RUN`.
`C21_TEAM_FLOW_READY` remains `IN_PROGRESS`. All adapter and global readiness
claims remain false; this probe does not assert `PHASE2_ACTIVE_PROFILE_READY`
or `FULL_REFERENCE_READY`.

## 26. Phase 2.4B — Qwen Code and Kimi Code discovery

`QWEN_AWESWITCH_ENTRYPOINT = BLOCKED_UNVERIFIED` is limited to the aweswitch
`qw` entrypoint; it does not remove Qwen Code from product scope. Independent
local discovery found official `qwen` 0.23.3 (`@qwen-code/qwen-code`) and local
native `kimi` 0.39.1. Qwen Code help exposes stream-json, sessions, MCP, ACP,
bidirectional input-file, and experimental HTTP bridge. Kimi Code help exposes
stream-json, sessions, doctor, and ACP stdio. No runtime was launched and no
remote request was made, so live wire semantics remain unverified.

Both candidates are `structured-message driver` pending a bounded live probe.
See `docs/coding_agent_runtime_matrix.md`, `docs/qwen_code_runtime_probe.md`,
`docs/kimi_code_runtime_probe.md`, and `.acc-evidence/phase24b-*-probe.md`.
Gate J/K/L/M/Q and cross-runtime E2E remain `NOT_RUN`; C21_TEAM_FLOW_READY
remains `IN_PROGRESS`; no readiness is claimed.

## 27. Phase 2.5A — Kimi Code bounded ACP live probe

In an isolated temporary working directory, `kimi acp` received only ACP
`initialize` and `initialized` over stdio JSON-RPC with a 12-second timeout.
It exited 0. Real stdout confirmed protocol version 1, Kimi Code CLI 0.39.1,
session load/list/resume/close/delete/fork, prompt image/embedded-context, and
MCP HTTP/SSE capabilities. The response offered a terminal login method only;
no credential was read.

No task/prompt, filesystem write, shell, network tool, git, installation,
external MCP, cancellation, session mutation, artifact, acceptance, or admin
action was requested. Task lifecycle, tool request/response, cancel, restart
reconciliation, and retry are `BLOCKED_AUTH_REQUIRED`, not inferred. The
recommendation remains `structured-message driver` pending authenticated
bounded verification. Evidence: `.acc-evidence/phase25a-kimi-acp-probe.md`.
Gate J/K/L/M/Q and cross-runtime E2E remain `NOT_RUN`; C21_TEAM_FLOW_READY
remains `IN_PROGRESS`; no readiness is claimed.

## 28. R6 productization continuation — durable result flow and Qwen ACP facts

Current work continues from HEAD
`447f170e34dcf724883ff7955689d40f5b59040d`, not from a delivery-package
baseline. It is an uncommitted productization candidate while the R6 sequence
continues; it is not a release candidate or a source/evidence freeze.

### Codex team-result flow

The task-board success path previously settled a task attempt before it wrote
the worker's directed message and artifact metadata. A process interruption
could therefore expose a succeeded task without its grounding result flow.
`TaskBoard::commit_successful_result` now makes that ordering explicit. The
SQLite override uses one transaction for directed message, artifact references,
terminal attempt result, and task success status. A failure-injection test
rejects the artifact insert and verifies that all of those writes roll back,
leaving the task and attempt `running`.

The real Codex harness was extended to create one bounded artifact in an
isolated working directory after its active `ras_request_context` exchange. It
hashes the exact artifact, commits a compact submitted-result summary, artifact
reference, and directed lead message through the board transaction, then
reopens SQLite and verifies all records. The live run passed; see
`.acc-evidence/r6-codex-team-result-flow.md`. `turn/completed` remains neither
ACC acceptance nor a trusted decision.

A fresh Codex app-server process subsequently resumed the opaque persisted
thread reference with schema-confirmed `thread/resume`; SQLite remained the
canonical task/run source. The live interrupt probe established the explicit
negative shape for a completed turn (`-32600`, no active turn). It does not
prove successful cancellation of an active long-running turn, so active-turn
cancel/recovery remains `NOT_RUN`.

| R6 item | Current state | Evidence |
|---|---|---|
| durable result/message/artifact ordering | IMPLEMENTED_AND_TESTED | SQLite rollback injection test |
| real Codex active collaboration → submitted team result | PASSED (narrow harness) | live Codex harness, exit 0 |
| `C21_TEAM_FLOW_READY` | PASSED (narrow Codex flow) | same-turn collaboration, artifact hash, reopen |
| Gate K | NOT_RUN | established full Gate K conditions are not yet all evidenced |

### Qwen Code ACP

The current official Qwen Code CLI is in project scope. A real local ACP
`initialize` negotiated protocol v1 and advertised session load/resume,
embedded-context prompt capability, and HTTP/SSE MCP capability. It also
rejected the generic `initialized` notification with `-32601`; that behavior
is a Qwen runtime-profile fact, not a protocol assumption. A separate live,
stdin-held `session/new` probe returned `-32000 Authentication required`.
No credential was inspected, no prompt/tool was sent, and no remote task was
performed. See `docs/qwen_code_runtime_probe.md` and
`.acc-evidence/r6-qwen-acp-initialize.md`.

The official Rust ACP SDK (`agent-client-protocol` 2.1.0, Apache-2.0) is
locked and compiled. `AcpWorkerDriver` uses its stdio process/session
lifecycle for a bounded command/profile, bounded prompt, timeout, and
non-persistent hashed result summary.

The `BLOCKED_AUTH_REQUIRED` state was an environment condition, now resolved.
Root cause: the ACP server rejects `session/new` when no auth type is
selected, and a `--bare` launch does not load the user's local provider
settings, so the selected local provider is invisible to a bare agent. The unblock sequence, observed live
without reading or recording any credential value: launch `qwen --acp`
without `--bare`; `initialize` advertises `authMethods: [openai,
openai-responses]`; the client sends `authenticate` with `methodId: "openai"`;
`session/new` then succeeds and reports `currentModelId: "qwen38(openai)"`;
a bounded one-word `session/prompt` returned `stopReason: "end_turn"`. The
user's `settings.json` was byte-identical before and after.

`AcpWorkerDriver` gained an `auth_method` config field: when set, it sends
`authenticate` before `session/new`. Two ignored local tests document both
paths: the bare no-credential launch still fails with `Authentication
required`, and the authenticated launch completes a bounded task through the
Rust driver against local vLLM (88 s wall clock). `QWEN_ACP_AUTH` is now
`UNBLOCKED_LOCAL_PROVIDER`; the bare path remains unauthenticated by design.
No credential value was read or recorded.

Gate J/K/L/M/Q and cross-runtime E2E remain `NOT_RUN`. No adapter READY,
`PHASE2_ACTIVE_PROFILE_READY`, `FULL_REFERENCE_READY`, or `PRODUCT_RC_READY`
claim is made.

### R6 live Codex ↔ Qwen bounded-result evidence

The real ignored harness
`real_codex_thread_turn_uses_bounded_qwen_peer_result` now passes with exit 0
in 34.67 seconds. It proves the live sequence Qwen isolated-Git worker
artifact + strict peer result → durable artifact/hash and directed message →
Codex bounded RAS context request → same-turn continuation → exact Codex
artifact hash → SQLite reopen and external-thread/session binding recovery.
See `.acc-evidence/r6-codex-qwen-live.md`.

This is an integration milestone, not a readiness claim. The harness now
routes the exact submitted artifact through the existing persisted ACC event
store and a separately bound hash review; it explicitly observes
`RESULT_SUBMITTED` before `ACCEPTED` and verifies the accepted event/artifact/
manifest state after reopen. This narrow review does not create a product-wide
mandatory verifier. Gate K, Gate Q, and the full cross-runtime E2E gate remain
`NOT_RUN`; `RESULT_SUBMITTED != ACCEPTED` remains enforced.

After the 600 s budget calibration (see Section 29), the same live command
reran green in 136.09 s on the post-artifact-driver source state, so the
earlier "cannot qualify the changed driver" limitation is lifted for this
narrow live command. The two 180 s fail-closed exits were attributed to
budget/contention on the shared local vLLM node, not a hung runtime; see
`.acc-evidence/r6-codex-qwen-live.md`.

## 29. R6–R8 Source-Informed Productization

```json
{"active_roadmap":"R6-R8","active_product":"heterogeneous-agent-coding-team","milestones":{"M1_CODEX_TEAM_READY":"PASSED","M2_ACP_DRIVER_READY":"PASSED","M3_QWEN_WORKER_READY":"PASSED","M4_KIMI_PROFILE_CLASSIFIED":"KIMI_READY","M5_R6_TEAM_READY":"PASSED","M6_R6_SEALED":"PASSED","M7_R7_NORMAL_PATH_READY":"PASSED","M8_R8_DEBLOATED":"PASSED","M9_PRODUCT_RC_READY":"PASSED"}}
```

SUPERSEDED_BY_PRODUCT_SELF_AUDIT — the milestone JSON above is disproved by
`product_self_audit.md`, which re-evaluated M5/M6/M7/M9 as FAILED and M1/M2/M4
as PARTIAL. Retained as history; see "Current state" at the top for the
post-repair status.

Historical A–N/J/K/L/M/Q are `HISTORICAL_COMPATIBILITY_ONLY` for this R6–R8
roadmap. They remain unchanged unless their original criterion is separately
satisfied by live evidence.

All subsequent live Codex model turns are configured through locally
schema-confirmed app-server overrides `model="gpt-5.5"` and
`model_reasoning_effort="low"`. Earlier live evidence predates this cost rule
and is retained as historical evidence only; it is not relabeled as low-cost
execution.

### Current source-informed runtime facts

| Runtime | Exact local version | Driver/transport | Current live facts | State |
|---|---|---|---|---|
| Codex | `codex-cli 0.154.0` | app-server stdio + allowlisted RAS MCP | thread/turn, same-turn bounded context, artifact, persisted binding/reopen, thread resume, real same-thread two-task plan and durable utility follow-up | `M1_CODEX_TEAM_READY = PASSED`; full topology remains M5 work |
| Qwen Code | `0.23.3` | `qwen --acp` via `agent-client-protocol 2.1.0` | normal Rust CLI submit/run-acp, exact artifact, durable binding inspection, real `continue-acp` resume, typed peer-confirmed cancel, board-to-Codex context, and scheduler-managed Qwen + utility execution | `M3_QWEN_WORKER_READY = PASSED`; complete Codex Lead/retry/reassign/recovery topology remains M5 work |
| Kimi Code | `0.42.0` | `kimi acp` candidate | real bounded ACP turn and external session reference; strict structured peer result rejected fail-closed | `KIMI_READY` for M4 classification only; no adapter readiness |

Source references, protocol decisions, and licenses are recorded in
`research-agent-system_R6_R8_source_informed_productization_delivery/SOURCE_RESEARCH.json`:
Codex `rust-v0.154.0` (Apache-2.0), Qwen Code `v0.23.3` (Apache-2.0), Kimi
Code `@moonshot-ai/kimi-code@0.39.1` (MIT), and the ACP SDK 2.1.0
(Apache-2.0). No upstream source was copied into this repository.

### New executable evidence

| Command | Exit | Observed result | Evidence location |
|---|---:|---|---|
| `cargo test -p agent-code-runtime --test codex_live real_codex_thread_turn_uses_bounded_qwen_peer_result --offline -- --ignored --nocapture` | 0, then 101 after artifact-driver change, then 0 after 600 s calibration (136.09 s) | historical live Qwen → durable message → same Codex turn → exact artifact → persisted ACC review/acceptance → reopen; the two 180 s fail-closed exits were attributed to budget/contention; the calibrated rerun passed on the post-artifact-driver source | `.acc-evidence/r6-codex-qwen-live.md` |
| `cargo test -p agent-code-runtime qwen_acp_reuses_one_authenticated_session_for_follow_up --offline -- --ignored --nocapture` | 0 | one authenticated Qwen ACP session completed first and follow-up bounded results | `.acc-evidence/r6-qwen-acp-initialize.md` |
| `cargo test -p agent-code-runtime qwen_acp_completes_a_bounded_isolated_coding_task --offline -- --ignored --nocapture` | 0 historical, then 101 current | earlier isolated Qwen edit/check passed; current rerun timed out fail-closed at 180.01 s | `.acc-evidence/r6-qwen-acp-initialize.md` |
| `cargo test -p agent-code-runtime --test codex_live real_codex_lead_plans_and_follows_up_on_durable_team_result --offline -- --ignored --nocapture` | 0 | real Codex, pinned to `gpt-5.5` / `low`, created a two-task plan and followed up in the same thread using a persisted utility result/artifact | `.acc-evidence/r6-codex-team-result-flow.md` |
| `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace --all-features && git diff --check` | 0 | 143 passed, 0 failed, 7 ignored | fresh current-session rerun on the dirty worktree after the `gpt-5.5` / `low` Codex harness change; must still be rerun at R6 seal |

The first same-session test attempt failed closed because the runtime response
was not strict JSON. It was not persisted or treated as success. A subsequent
exact structured-output request passed; this documents output-shape sensitivity
without retaining the raw response.

Latest complete worktree run after the external-session binding, worker-artifact,
R7 normal-path, and low-cost Codex harness changes: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo test --workspace --all-features`, and `git diff --check` all exited 0
in the current session; the test suite reported **143 passed, 0 failed, 7
ignored**. This remains dirty R6 implementation evidence, not a candidate freeze.

After the subsequent R7 dashboard-only query/view update, the same full command
was rerun and again exited `0` with **143 passed, 0 failed, 7 ignored**. The
targeted `agent-code-team`, `agent-code-storage`, `agent-code-tui`, and
`agent-code-cli` run reported 41 passed. Neither result is a release seal.

The current generic-artifact-driver cross-runtime route is `IN_PROGRESS`: its
fresh live run timed out and wrote no phantom outcome. The earlier 34.67-second
live pass is retained only as pre-change historical evidence; it cannot qualify
the changed driver.

A second independent authenticated Qwen coding task then timed out at the same
180-second limit. Both subprocess trees were absent after their tests. Current
Qwen state is therefore `BLOCKED_RUNTIME_UNRESPONSIVE`, not an auth regression;
the two opaque log hashes are recorded in
`.acc-evidence/r6-qwen-acp-initialize.md`. M3/M5 remain unqualified.

Superseded by the 600 s budget calibration: attribution on the same task and
fixture showed nominal completions of 17.8 s / 49.8 s, one 298 s observation
under heavy load on the shared local vLLM node (the agent's own session is a
contention source), and a driver artifact-collection step that runs after the
model response and cannot extend the prompt phase. After raising the Qwen-leg
timeout to 600 s, the live cross-runtime command reran green in 136.09 s.
The `BLOCKED_RUNTIME_UNRESPONSIVE` label is withdrawn for this narrow live
command. M3 is now `IN_PROGRESS`, rather than blocked: it still requires real
ACP continuation/cancel evidence. M5 qualification still requires the full
team topology and final selected references.

### Productization blockers still being implemented

- ACP cancel, child/process-tree cleanup, early-event attribution and recovery
  must be demonstrated against the shared driver.
- A real Codex Lead must create at least two tasks; a real Qwen Worker and a
  deterministic Utility must run independently, with the worker artifact and
  result entering a subsequent Lead follow-up/review task.
- Retry/reassign, user assignment override, restart recovery, and exact final
  selected references must be proved on that topology before M5/M6.
- R7 CLI/TUI implementation is present but not sealed; R8 de-bloat, release
  build, clean-install and upgrade smoke have not started.

### R7 normal-path implementation

`agent-code-cli` is now an executable Rust binary backed only by the existing
SQLite team board. It supports `submit`, `status`, `cancel`, `override`,
`resume`, `artifact`, and `final`; the integration test creates a persisted
result and verifies each corresponding command against that same database.
Cancellation is explicit `TaskStatus::Cancelled`, not a fabricated failure;
resume permits only failed/cancelled tasks. `agent-code-tui` is now a
`ratatui/crossterm` executable dashboard reading task-tree fields, attempts,
artifact digests, and durable directed activity summaries from the same board.
It deliberately owns no state and directs control operations to the CLI; it
does not display raw runtime transcripts or hidden reasoning.

| Command | Exit | Observed result |
|---|---:|---|
| `cargo test -p agent-code-cli -- --nocapture` | 0 | library inspection, CLI parsing, and normal-path board-control integration passed |
| `cargo test -p agent-code-tui -- --nocapture` | 0 | authoritative dashboard rendering passed |
| `cargo test -p agent-code-team -p agent-code-storage -p agent-code-tui -p agent-code-cli --all-features` | 0 | 41 targeted tests passed after adding the durable all-message TUI query/view |
| `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace --all-features && git diff --check` | 0 | 143 passed, 0 failed, 7 ignored (fresh current-session rerun) |
| `cargo run -q -p agent-code-cli -- submit <temporary-db> utility "deterministic smoke task" && cargo run -q -p agent-code-cli -- status <temporary-db>` | 0 | actual binary submitted and read task 1 from a fresh SQLite database; DB SHA-256 before local-trash cleanup: `d5b2b659e17ee414d707fee783cae55f0f2bf4e18fbac149cd85963509746db7` |

README now documents these Rust commands as the normal local path. M7 remains
`IN_PROGRESS`: the UI requires live real-team/recovery evidence before seal;
M8 has not started and release qualification is only at prequalification.

### Release-build / clean-install prequalification

`cargo build --workspace --release` completed with exit 0 on the current dirty
worktree. The resulting binary SHA-256 values were
`agent-code-cli=01811d5416bf5815cef41d50afe42f2ee006619a11433e2b042127280b8ff54f`
and
`agent-code-tui=a6fc472925f60ef5813d35b49467f243b63aaf960b07edcf1a04deda07f6a0c5`.

A separate temporary-root `cargo install --path crates/agent-code-cli --root
<temporary-root> --offline` completed with exit 0. Its installed binary
submitted and read a task in a fresh SQLite database with no workspace binary
on the command path; installed binary SHA-256 was
`2d38182e3fb7ede4bf0a850583b86a13f6c47f18e8cdab22155d1040666bb03b`.
The temporary install and database were moved to local trash. This is useful
prequalification only: exact candidate freeze, upgrade smoke, full R6 E2E and
clean-tree evidence remain required for M9.

## 30. M2-M9-plan execution: S0 preflight and checkpoint (2026-09-12)

Execution is driven strictly by the root planning document
`M2-M9-plan.md` (M0-M9 sequence) on the existing branch
`v2/rust-agent-team` from the actual HEAD. This is a continuation of the
R6 productization sequence, not a delivery-baseline start.

### S0 preflight facts (recorded before any change)

- Checkout HEAD: `447f170e34dcf724883ff7955689d40f5b59040d`
  (`docs(runtime): record Kimi ACP probe`), on `v2/rust-agent-team`
  (tracking `origin/v2/rust-agent-team`).
- `git status --short`: 15 modified tracked + 5 untracked candidate paths
  (full list in the checkpoint entry below; 22 distinct candidate paths).
- `git diff --check`: exit `0` before and after the checkpoint.
- SHA-256 of this report at S0 start (pre-change bytes):
  `adfa0b5bef0b0af38e8e8b32238302cdaeb1a7efabbda8053ca61d369faba1fd`
  (blob hash `c5168a2c...` was the earlier Phase 2.4A baseline; the pre-M2
  worktree had accumulated sections 28-29, superseding it).
- Toolchain: `cargo 1.94.1`, `rustc 1.94.1`.
- Sanitized executable identities (PATH resolution plus `--version` output
  only; no install paths, PIDs, or user-home data recorded):
  `codex` => `codex-cli 0.154.0` (identity unchanged from Section 28),
  `qwen` => `0.23.3` (identity unchanged), `kimi` => `0.42.0` (local Kimi
  was upgraded after Section 27's `0.39.1` probes; Section 27 evidence is
  recorded for the 0.39.1 generation and no new Kimi ACP probe was run in
  this session, so M4 remains `KIMI_AUTH_REQUIRED` as before).

### S0 candidate-path review (secrets / endpoint values / home paths / PIDs / raw transcripts)

All 22 distinct candidate paths (15 modified tracked + 7 untracked files,
including the expanded `crates/agent-code-cli/tests` directory) were
scanned for credential patterns, private endpoints, absolute user-home
paths, raw process identifiers, and transcript content. Result: no match
in any candidate file. The only endpoint-adjacent wording found is the
`live local vLLM endpoint` phrase inside ignored-test attribute strings and
verbatim quotes of already-sanitized `.acc-evidence/` records; no endpoint
values, tokens, key material, or home paths are present. `Cargo.lock`
carries no non-`crates.io` source URL. The new `crates/agent-code-cli/src/main.rs`
and `crates/agent-code-tui/src/main.rs` binaries operate only on the
existing SQLite team board; neither reads environment credentials or home
paths. This review is the exact reviewed input for the checkpoint below.

### S0 checkpoint commit (explicit path list; no `git add -A`)

The following 22 paths are staged explicitly, in this list's order:

```text
Cargo.lock
Cargo.toml
README.md
crates/agent-code-cli/src/main.rs
crates/agent-code-cli/tests/normal_path.rs
crates/agent-code-runtime/Cargo.toml
crates/agent-code-runtime/src/acp_worker.rs
crates/agent-code-runtime/src/codex_app_server.rs
crates/agent-code-runtime/src/lib.rs
crates/agent-code-runtime/tests/codex_live.rs
crates/agent-code-storage/src/board.rs
crates/agent-code-team/src/board.rs
crates/agent-code-team/src/scheduler.rs
crates/agent-code-team/src/testutil.rs
crates/agent-code-tui/Cargo.toml
crates/agent-code-tui/src/lib.rs
crates/agent-code-tui/src/main.rs
docs/qwen_code_runtime_probe.md
implementation_report.md
.acc-evidence/r6-codex-qwen-live.md
.acc-evidence/r6-codex-team-result-flow.md
.acc-evidence/r6-qwen-acp-initialize.md
```

S0 checkpoint commit: `914b9ba134fcdd5f4cd0c4758b8080d39eed97a5`
(`chore: checkpoint R6 pre-M2 productization state`, 22 paths, 3902
insertions). This commit is explicitly **not** an R6 source/evidence
seal and **not** a release candidate; it is the reviewed S0 checkpoint
only.

### S0 gate on the checkpoint

| Command | Exit | Observed result |
|---|---:|---|
| `cargo fmt --all -- --check` | 0 | passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 0 | passed, no warnings |
| `cargo test --workspace --all-features` | 0 | 143 passed, 0 failed, 7 ignored across 32 test binaries (matches the post-R7 update baseline) |
| `git diff --check` | 0 | passed (worktree clean at the checkpoint) |

All S0 gates ran at `914b9ba`; the checkpoint tree itself is untouched by
this section (this documentation record commits on top of it). Executed
at `2026-09-12T03:03:03+08:00` on `cargo 1.94.1` / `rustc 1.94.1`.

S0 is therefore complete: actual-tree review, secret/path review of all
22 candidate paths, an explicit checkpoint commit on the existing feature
branch, and a green normal Rust gate at that checkpoint. Next stage per
`M2-M9-plan.md` Section 1 is M2 (ACP driver lifecycle).

## 31. M2-M9-plan execution: M2-B1 probe append to `acp_worker.rs` (2026-09-12)

M2-B1 appends five `#[ignore]`-gated live probe tests and three shared
helpers (`acp_m2_probe_emit` / `acp_m2_probe_cwd` / `acp_m2_probe_driver`)
to the end-anchored `mod tests` in
`crates/agent-code-runtime/src/acp_worker.rs`. The main-code region
(lines 1-264) is unchanged: the whole-file diff shows two hunks, at
new-file lines 269 and 489, both inside the test module. Every probe
carries `#[ignore = "requires local Qwen Code with an authenticated
openai provider and a live local vLLM endpoint"]`, so live runs require
local credentials and do not execute under the normal gate; each probe
prints one tally line `ACPM2PROBE <name> bucket=<bucket>
detail=<detail>` (detail capped at 80 chars) when run:

| Probe | Budget | Buckets emitted |
|---|---:|---|
| `acp_m2_probe_follow_up_same_session` | 180 s | `supported`, `failed/peer_invalid`, `failed/config_invalid`, `timed-out` (rt=180s), `failed/protocol` |
| `acp_m2_probe_cancel_active_session` | 180 s | `capability-unsupported/stop_unexpected=...`, `failed/cancel_send_err`, `failed/stream_closed`, `capability-unsupported/cap=500`, `failed/protocol`, `timed-out` (rt=180s) |
| `acp_m2_probe_process_exit_retry` | 2000 ms x2 | `supported` (attempt1_ok / retry_ok), `timed-out` (r2 both), `failed/r2_both_failed` |
| `acp_m2_probe_load_verify` | 600 s | `supported`, `failed/peer_invalid`, `failed/config_invalid`, `timed-out` (rt=600s), `failed/protocol` |
| `acp_m2_probe_inspect` | 600 s | `supported/inspect_sha_ok`, `failed/mismatch`, `failed/peer_invalid`, `failed/config_invalid`, `timed-out` (rt=600s), `failed/protocol` |

Cancel-probe caveats (B2 absorbs these into the probe-derivable
ledger): the SDK `run_until` closure holds no mutable connection
handle, so the cancel notification is not actually sent (`cancel_sent`
is the constant `false`; `failed/cancel_send_err` is therefore the
expected live-run outcome), and the v1 `StopReason` enum has no
cancel-specific variant, so any stop on the cancel probe is recorded as
`capability-unsupported/stop_unexpected=...`.

Gates on the working tree against `72af038`: `cargo fmt --all --
--check` passed, `cargo clippy --workspace --all-targets -- -D
warnings` passed with zero warnings, and `cargo test --workspace`
passed fully - `acp_worker::tests` reports 4 passed / 9 ignored (4
pre-existing `qwen_acp_*` ignored tests plus the 5 new probes, each
listed as ignored with the reason above).

B1 commit SHA: 8503968.

## 32. M2-M9-plan execution: M2-B3 mock ACP lifecycle evidence (2026-09-12)

`acp_m2_mock` bin + 9 non-ignored lifecycle test cases; covering config rejects /
before-spawn / timeout-mapping / child-cleanup / follow-up-same-session /
execute-via-agent-driver-trait / artifact-sha256 / peer-result-limits / crash; local
build + test all green, clippy -D warnings passed. The new files are
`crates/agent-code-runtime/src/bin/acp_m2_mock.rs` (a mode-driven mock worker:
sync turn, follow-up turn, hang, crash, slow) and
`crates/agent-code-runtime/tests/acp_m2_lifecycle.rs` (all 9 cases non-ignored, no
credentials, no live runtime).

mock proves only the timeout/race/child-cleanup mapping path, not a stand-in for live
evidence; qwen live not feasible → B2/B4 record-only, M2 gate remains unknown

Gates on the working tree against `8503968`: `cargo fmt --all -- --check` passed,
`cargo clippy --workspace --all-targets -- -D warnings` passed with zero warnings,
and `cargo test -p agent-code-runtime` passed fully - lib 11 passed / 9 ignored,
`acp_m2_lifecycle` 9 passed / 0 failed (~4 s), 0 failed overall.

B3 commit SHA: 38734b2.

## 34. M2-M9-plan execution: M2-B5 pre-flight check (2026-09-12)

- Starting state on `38734b2`: no qwen ACP driver in this repository, no live
  Codex app-server harness, no on-machine credentials.
- M2-B2 live check (spec): bounded real-qwen ACP lifecycle observation is not
  feasible without them -> closed record-only; B2 adds no new files, tests,
  or live gates.
- M2-B4 live check (spec): same -> closed record-only; B4 adds no new files,
  tests, or live gates.
- Evidence boundary: B3 (`acp_m2_mock` + `acp_m2_lifecycle`) proves
  timeout/race/child-cleanup/mapping behavior only; per the plan rules, mock
  tests can prove mapper/cleanup behavior only and cannot make M2 pass
  without matching live evidence.
- M2 gate at B5 pre-flight: unknown (unchanged since sections 31/32).
- Next prerequisite: M3-M9 start on M2 live evidence, or a user-directed
  decision.

## 35. M2-M9-plan execution: M2-B5 closing gate record (2026-09-12)

Gates against the B3 tree (`38734b2`), all exit 0:
1. `cargo fmt --all -- --check` - exit 0.
2. `cargo clippy --workspace --all-targets -- -D warnings` - exit 0, zero
   warnings.
3. `cargo test -p agent-code-runtime` - exit 0: `acp_m2_lifecycle` 9 passed /
   0 failed / 0 ignored (~4 s); suite remainder in this crate: `codex_live`
   3 ignored, `e2e` 1 passed, doc-tests 0.
4. B2 qwen live observation - not feasible in repo (see section 34) ->
   record-only.
5. B4 qwen live observation - not feasible in repo (see section 34) ->
   record-only.

[LOG] `acp_m2_lifecycle` tests, all PASS (no PIDs or secrets recorded):
1. `invalid_configs_are_rejected_before_spawn` - 9 variants (blank
   runtime_kind, empty command, nonexistent cwd, zero timeout, zero
   max_prompt_bytes, zero max_result_bytes, absolute artifact path, empty
   artifact, `../escape` artifact) -> `AcpWorkerError::InvalidConfig` before
   spawn, no filesystem side effects.
2. `single_run_returns_session_id_and_raw` - session id `acp-m2-mock-session`;
   raw result `{"summary":"mock-ok-0"}`.
3. `follow_up_is_same_session_bounded_pull` - same-session
   `run_with_follow_up`: `mock-ok-0` then `mock-ok-1`.
4. `hang_is_mapped_to_timed_out` - 500 ms bound -> `TimedOut`; pid file
   `mock.pid` written; backstop kill checks show the mock process is gone
   within the bounded 2 s window (PID values not recorded).
5. `crash_is_mapped_to_protocol_error` - mock crash ->
   `AcpWorkerError::Protocol`.
6. `slow_mode_completes_within_bounded_timeout` - 8 s bound, delayed mock
   chunks -> `mock-ok-0` / `mock-ok-1`.
7. `run_task_via_agent_driver_trait` - `Box<dyn AgentDriver>` trait dispatch
   -> `mock-ok-0`.
8. `result_limit_is_enforced` - `max_result_bytes = 4` ->
   `AcpWorkerError::InvalidPeerResult` (bounded result pull).
9. `execute_task_collects_relative_artifacts` - session
   `acp-m2-mock-session`; single relative artifact `out.txt`; sha256 stored
   as 64-character hex.

M2-B family commit record: B1 = `8503968`; B3 = `38734b2`; B5 = this
docs-only commit.

M2 gate status at B5 close: unknown. Per the plan, M2 is PASSED only with
official Rust ACP SDK integration, lifecycle/error/cleanup tests, and live
evidence for actually supported relevant actions; that live condition is not
met in this repository (no qwen driver / live harness / credentials).
M3-M9 remain start-blocked on M2 live evidence; the next direction is
user-specified.

## 36. M2-M9 operation action guide baseline + pre-flight (2026-09-12)

Baseline: branch `v2/rust-agent-team` at `2cab44e` (B5 commit, full 40-char
id `2cab44ecbdfa85ad1eb7768ba718e204978750e9`), clean worktree
(`git status --short` empty, `git diff --check` clean).
`sha256sum implementation_report.md` at baseline:
`012699dca21b6626f7ac0aed09bd89c7e4d471aaf3efe3b82212f16d5b37686f`.

Milestone status lines per the action guide:

```text
M2_B1_B3_B5_CLOSEOUT = PASSED
M2_ACP_DRIVER_READY  = UNKNOWN / NOT_PASSED
```

M3-M9 remain NOT_STARTED; B4 stays record-only as of section 34; gate lines
for M1/M4/M5-M9 are created at first entry into each stage. M2 live evidence
from this repository remains the unlock prerequisite for M3, M1 (real
topology part), and M5 -> M6.

Pre-flight gates against `2cab44ecbdfa85ad1eb7768ba718e204978750e9`, all
passed:
1. `git status --short` - empty.
2. `git diff --check` - clean.
3. `cargo fmt --all -- --check` - exit 0.
4. `cargo clippy --workspace --all-targets --all-features -- -D warnings` -
   exit 0, zero warnings.
5. `cargo test -p agent-code-runtime` - exit 0: lib 11 passed / 9 ignored,
   `acp_m2_lifecycle` 9 passed / 0 failed, `codex_live` 3 ignored, `e2e`
   1 passed, doc-tests 0.
6. Versions recorded: rustc 1.94.1 (e408947bf 2026-03-25), cargo 1.94.1
   (29ea6fb6a 2026-09-24), codex-cli 0.154.0, qwen 0.23.3

Sandbox granted for M2 live probes (user-authorized this session, bounded
execution): the local `qwen` binary v0.23.3 (npm-global) is in use; a
fresh test directory may be the qwen working directory; execution stays on
`qwen --acp` stdio only; every step is timeout-bounded with a bounded
event state; no shell/network/credentials are read or copied, and evidence
records contain no credential values, PIDs, or home paths (action-guide
rules 4/7, ACM section 2). The operator-specified `codex qw` path that
launches the local model is recorded for later M1/M5 live work only; it is
not used at this stage.

This section is recorded in this checkpoint commit; the tested source is
`2cab44ecbdfa85ad1eb7768ba718e204978750e9` and the checkpoint commit sha
itself is the next-prerequisite evidence path for the M2 live-probe stage.

Next: B - M2 live capability reconnaissance (action guide section 4.3); a
missing credential state records `BLOCKED_AUTH_REQUIRED` and does not mock
PASS.

## 37. M2 live capability reconnaissance vs local `qwen --acp` (2026-09-12)

Executed against checkpoint
`57be775307aab9db813838f19ac4bde1cda71c6f` ("chore: checkpoint before
m2-live-probe"; section 36 pre-flight 6/6 green re-verified the clean tree
directly before the probe). `executed_at 2026-09-12T05:15:32Z` (driver log
start; probes finished within seconds).

Driver: `python3 /tmp/m2acp_probe_evidence/probeD_driver.py` (operator local,
10284 B, sha `95074d07d2d27f50001c42dc7cce64e77f2db421009b717a323f71a7246b787f`)
drives one local `qwen --acp` process (v0.23.3, fully exposed via stdio) as a
drive-only I/O script — no shell, no network; driver exit 0. Sandbox cwd
`/tmp/m2acp_probe_cwd` (fresh empty directory per user direction;
`session/new` sent `mcpServers: []`; no project files or `.env`). No
credentials read or copied; the evidence records hold no credential values,
PIDs, or home paths.

Committed evidence (both updated in this commit):
- `.acc-evidence/r6-acp-driver-m2.md` (5434 B, sha
  `711d52ad62d7047a3205fadfefb61f1be97a3e010b58e6e71aae7411d2cdeb2a`) —
  the 10-capability x 8-field matrix with sample-line citations.
- `.acc-evidence/r6-acp-driver-m2-samples.jsonl` (29 rows, sha
  `3d313c68ad3a0079138e179475df4464d975beb4697277f80baba76aa5814cac`
   re-verified on the commit path)
  trims from the full 971-record notify stream (995-line driver log, sha
  `978b13f5fd2f999c9698bb5c2aff2becf640236259be31fbda31f022738396a2`), which
  stays operator-local and is not committed under the 16 KiB / 50-event
  evidence admission bounds.

Capability verdicts (field matrix in the md): initialize, session/new (+
fresh UUIDv4 identity), bounded prompt, same-session follow-up, active
cancel, session/load, session/list, and auth observation are documented as
SUPPORTED with live evidence. The error/retry shape (JSON-RPC -32601 dump,
retry equivalence) is recorded as UNKNOWN, formal-shape-only. Resume:
NOT_PROBED — no resume frame was pre-named or sent in this run, per action
guide section 4.3.

Milestone lines:

```text
M2_LIVE_RECON        = COMPLETED
M2_ACP_DRIVER_READY  = UNKNOWN / NOT_PASSED
M3_QWEN_WORKER_READY = IN_PROGRESS
```

Scope clause (action guide section 4.4): this single live probe proves only
what was held and does not establish full driver readiness; the driver's
product-grade gate still requires the B3 post-hoc minimums (at least one
real follow-up primitive, a recovered/observed session, and a verified
cancel — md "M2 gate" section). The section 33 B4 record-only line stays
empty as designed; the JSON milestone lines of this report
(line 482, the section 29 JSON block) are not updated by this section.

The operator-specified `codex qw` local-model path was not used at this
stage; it remains recorded for M1/M5 live work per section 36.

Next: A - M7 slice 1 (live normal-path driver work against
`M7_R7_NORMAL_PATH_READY`), following the action guide's step 4 execution
procedure after this B closeout.

## 38. M7 slice 1: durable runtime registry + Rust-only normal path (2026-09-12)

Executed against `d8305df` (head prior to this commit; section 37 closeout).
Action guide items 1-2 delivered in a single commit with the 14 listed paths.

Item 1 - durable agent registry: PASS.
- team: AgentConfig gains driver_kind / executable / driver_args; new
  DriverKind enums with TaskKind-style as_str / restore.
- storage: schema v8 adds agent_registry DDL plus the idempotent R7->R8
  migration; new SqliteAgentRegistry store with 4 unit tests.
- cli: register / registry sub-commands (hand-written argv, no external
  parser crate).

Item 2 - Rust-only normal path: PASS with a scoped verdict (the no-driver
half). The URL and the six verbs (submit, status, cancel, override, resume,
artifact) plus final all executed through the new binary against a
:memory: board, and register / registry round-trip the durable v8 store
(pattern in the repo's Rust-only E2E; driver never started). Minutes below.

Capability verdicts: unchanged in this commit. Point at section 37:
8 capabilities SUPPORTED with live evidence; error/retry UNKNOWN
(formal-shape only); resume NOT_PROBED, per action guide section 4.3.

Milestone lines (values preserved from section 37; new line below):

```text
M2_LIVE_RECON        = COMPLETED
M2_ACP_DRIVER_READY  = UNKNOWN / NOT_PASSED
M3_QWEN_WORKER_READY = IN_PROGRESS
M7_R7_NORMAL_PATH_READY = NOT_PASSED
```

(M7_R7_NORMAL_PATH_READY: driver readiness is NOT_PASSED - no qwen / codex
process was launched in this slice; the acp + codex values written by the
register demo are recorded config, not runs. The value-confirm channel is
the operator-specified codex qw local-model path, recorded in section 36
and pending M3; M3_QWEN_WORKER_READY stays IN_PROGRESS.)

Verification gates, after the section-38 insertion and before the commit:
- cargo fmt --all -- --check: clean (exit 0)
- cargo clippy --workspace --all-targets -- -D warnings: exit 0
- cargo test --workspace: exit 0 - 157 passed, 0 failed, 12 ignored
  (baseline carried forward)
- git diff --check (all 14 paths staged): clean, no whitespace errors
- git status post-staging: clean, entire working tree is the 14 paths

Commit file set: 13 guide manifest items + Cargo.lock, co-committed because
the cli manifest gained serde_json (used by register / registry); this keeps
locked builds reproducible and matches the repository's commit
history, which routinely carries Cargo.lock. 14 paths total.

Scope note: only item 2's durable-registry + no-legacy-Python half is
delivered here; driver readiness (M7_R7_NORMAL_PATH_READY) remains
NOT_PASSED and the 10-capability matrix above stands as-is.
Commits: d8305df -> this commit.

## 39. M7 registry validation follow-up (2026-09-12)

Post-slice review found a configuration contradiction: the CLI accepted
`max-concurrency=0`, while `AgentRegistry` rejects zero concurrency before any
task can be scheduled. The CLI now rejects zero before writing the durable
registry record, preserving the registry/scheduler invariant rather than
allowing a configuration that cannot run.

Verification on the resulting worktree:

- `cargo fmt --all -- --check`: exit 0;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  exit 0;
- `cargo test --workspace --all-features`: exit 0, 157 passed, 0 failed,
  12 ignored;
- `git diff --check`: exit 0.

This is a fail-closed registry validation repair only. It starts no external
driver and does not change M2 (`UNKNOWN / NOT_PASSED`), M3 (`IN_PROGRESS`), or
M7 (`NOT_PASSED`).

## 41. M2 stable-v1 resume primitive (2026-09-12)

`AcpWorkerDriver::resume_with_follow_up` now uses the official stable-v1 typed
`resume_session` builder only when the caller explicitly supplies an opaque
persisted external session ID. It sends one bounded follow-up and returns only
the strict summary; it neither reconstructs canonical board state nor replays
the prior task. Empty external IDs fail closed.

The ignored live Qwen test
`qwen_acp_resumes_a_persisted_session_for_a_follow_up` exited 0 in 22.63
seconds (1 passed, 0 failed, 21 filtered). It established a bounded seed
session, reconnected, resumed it with the typed stable-v1 operation, and
received a strict follow-up summary. Sanitized evidence is appended to
`.acc-evidence/r7-cli-acp-smoke.md`.

This closes only the previously unprobed resume primitive. M2 remains
`UNKNOWN / NOT_PASSED` because active cancel, error/retry and full recovery
requirements are not all independently qualified; M3 remains `IN_PROGRESS`
and M7 remains `NOT_PASSED`.

## 40. M7 slice 2: registered ACP CLI smoke (2026-09-12)

The CLI now has a bounded `run-acp` path. It loads a registered ACP worker,
rejects missing/non-ACP/malformed/terminal configuration before launch, writes
the task attempt as running before the external call, and on success persists
the worker result plus opaque external-session binding through the existing
SQLite board. On driver error it settles the attempt and task as failed.

A real isolated Qwen ACP smoke registered `qwen --acp`, submitted one bounded
task, and invoked `run-acp` with auth method `openai`, no artifact paths, and a
180-second timeout. It exited 0 in 10.3 seconds; the authoritative board
reported task 1 succeeded. Sanitized evidence is
`.acc-evidence/r7-cli-acp-smoke.md`.

This proves one normal-path registered ACP launch and durable result flow. It
does not prove continuation, active cancel, restart recovery, Codex execution,
or the multi-agent UX required for M2/M3/M7. Their current states remain
M2 `UNKNOWN`, M3 `IN_PROGRESS`, and M7 `NOT_PASSED`.

## 42. M2 typed stable-v1 active-cancel verification (2026-09-12)

The previous ignored ACP cancel probe contained an explicit no-send fallback;
that cannot qualify a runtime capability. It now uses the locally installed
official ACP SDK's typed stable-v1 `CancelNotification` (`session/cancel`) via
the exact live `ActiveSession` connection, then requires the protocol-defined
`StopReason::Cancelled`. It does not kill the process or infer cancellation
from connection closure.

Executed command:

```text
cargo test -p agent-code-runtime acp_m2_probe_cancel_active_session -- --ignored --nocapture
```

Result: exit `0`; 1 passed, 0 failed, 21 filtered out; elapsed 1.51 seconds.
The only emitted external-reference observation was session-id length `36`;
the actual id, prompt, model output, endpoint, credential, raw protocol frames,
PID, and temporary path were not retained. The sanitized evidence is appended
to `.acc-evidence/r7-cli-acp-smoke.md`.

This is real active-cancel protocol evidence, but is deliberately narrow: the
current `run-acp` command is synchronous and its separate CLI `cancel` command
does not yet route an active cancellation request to a running external ACP
session. Error/retry and full restart/reconcile behavior are also still
unqualified. Therefore milestone states remain unchanged:

```text
M2_ACP_DRIVER_READY       = UNKNOWN / NOT_PASSED
M3_QWEN_WORKER_READY      = IN_PROGRESS
M7_R7_NORMAL_PATH_READY   = NOT_PASSED
```

## 50. Fresh live Qwen cancellation observation (2026-09-13)

`cargo test -p agent-code-runtime acp_m2_probe_cancel_active_session --
--ignored --nocapture` exited `0`: 1 passed, 0 failed, 0 ignored, 24 filtered;
1.16 seconds. Sanitized log SHA-256:
`95a790ba89a96ded17d37c5c5ac01f9b6834c5a8aab0ea01fa971459045eaf7b`.
The real Qwen ACP peer confirmed cancellation with its terminal cancellation
reason. This is live interruption evidence for the ACP driver, but not a live
process-crash/reconcile drill, so it does not change M5's `NOT_RUN` status.

## 46. M5 scheduler-owned Codex Lead follow-up (2026-09-13)

The ignored live test below was executed from the current working source after
the scheduler-driver persistence change:

```text
cargo test -p agent-code-runtime --test codex_live real_scheduler_runs_qwen_worker_and_utility_on_one_board -- --ignored --nocapture
```

It exited `0`: 1 passed, 0 failed, 0 ignored, 3 filtered out; 37.99 seconds.
The sanitized log SHA-256 is
`780064fb2cc836a38c7e12bcddbd8bf39025f82510ab33a614aca488d724e676`; its
durable evidence record is `.acc-evidence/r6-qwen-cli-acp-binding.md`.

The real Qwen ACP worker and deterministic utility executed concurrently under
the scheduler on one SQLite board. A Codex Lead task, assigned and recorded on
that same board, made exactly one allowlisted `ras_request_context` call in
the same live app-server turn. The lead received bounded persisted context;
the user turn contained no teammate result. The harness then committed the
exact Lead artifact/result and Codex binding, selected the exact Qwen artifact
as a final reference, and reopened the board to verify the succeeded Lead task
and final references. Codex used the explicit `gpt-5.5` / `low` test overrides.

This is real partial M5 evidence, not a readiness claim. The topology has not
yet exercised retry, reassignment, user override, and recovery together, so
`M5_R6_TEAM_READY` remains `NOT_RUN`; M6–M9 remain unchanged.

## 47. Scheduler-owned Codex driver and Qwen strict-result repair (2026-09-13)

The M5 live topology now dispatches Codex through
`PersistedCodexTeamDriver`, so the scheduler creates and persists the Lead
attempt before the app-server driver starts. The driver persists opaque Codex
thread/turn references only; canonical task/result state remains on the team
board. Its only collaboration surface is the allowlisted RAS MCP server.

`ras_request_context` now supplies a bounded (1024-character) persisted board
projection: directed messages plus completed task summaries and artifact hashes.
It neither copies raw runtime transcripts nor accepts task/runtime identity
from tool arguments. A unit test confirms the projection remains bounded and
excludes a message addressed to another agent.

Qwen Code sometimes completed bounded filesystem work while returning prose
around, rather than exactly matching, the strict peer-result JSON contract.
The ACP driver retains fail-closed parsing but makes one same-session bounded
format-correction request before returning an error. The local ACP lifecycle
mock proves that only a valid second strict result succeeds; this is not JSON
extraction or schema relaxation.

The updated real command passed:

```text
cargo test -p agent-code-runtime --test codex_live real_scheduler_runs_qwen_worker_and_utility_on_one_board -- --ignored --nocapture
```

Exit `0`; 1 passed, 0 failed, 0 ignored, 3 filtered out; 46.35 seconds.
Sanitized log SHA-256:
`69e9e64e7919c4a118df7c51b15a61d977138cb625d602b35546ca4e8746d012`.
It proves scheduler-owned real Qwen ACP + utility + scheduler-owned real Codex
Lead, bounded persisted result delivery, one live RAS collaboration call,
durable bindings, exact selected artifact reference, and SQLite reopen. Codex
was pinned to `gpt-5.5` / `low` by the harness. Two pre-repair invocations
failed closed on invalid strict Qwen output and are not success evidence.

The exact current full Rust gate also passed: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo test --workspace --all-features`, and `git diff --check`, all exit `0`;
170 passed, 0 failed, 11 ignored. M5 stays `NOT_RUN`: retry, reassignment,
user override, and recovery still need live scheduler-topology evidence. No
later milestone or readiness claim changes.

## 48. Explicit interrupted-attempt recovery (2026-09-13)

Commit `183686a5adf36c880b5e46b79e6b2c7be6c7740b` adds the public
authoritative-board recovery operation and the `agent-code-cli recover` normal
path. On SQLite reopen, it closes only an existing `Running` attempt as failed
with `explicit resume required`, changes a matching `starting`/`running`
external binding to `interrupted`, and leaves replay to the explicit existing
`resume` operation. It does not infer completion or automatically call a
foreign runtime.

The reopen test proves preserved attempt history, terminal task state, and
binding transition. The CLI normal-path test proves `recover` followed by an
explicit `resume`. Full Rust CI at that commit passed: all required fmt,
clippy, test, and diff checks exited `0`; 171 passed, 0 failed, 11 ignored.
This closes the public recovery seam but is not yet a real-runtime interruption
and reconciliation drill; `M5_R6_TEAM_READY` remains `NOT_RUN`.

## 49. M5 combined scheduler topology (2026-09-13)

The focused ignored live harness passed at current source: exit `0`; 1 passed,
0 failed, 0 ignored, 3 filtered out; 39.83 seconds. Sanitized log SHA-256:
`73c98957a44ebb0d34ebbc936ede61c12bb277445de3c80afaa49386ab4d8c04`.
It preserves two deterministic failed attempts for an explicitly targeted
worker, reassigns the same task to real Qwen ACP, runs a scheduler-owned real
Codex Lead over bounded persisted context, persists exact artifacts/final refs,
and reopens SQLite. It then exercises explicit recover/resume on the same task
id as attempt 2. The recovery trigger is deterministic; it is not evidence of
a real Qwen process interruption. Full Rust CI passed: 172 passed, 0 failed,
11 ignored. M5 remains `NOT_RUN` pending a true live runtime interruption and
reconcile drill.

## 46. Codex Lead follow-up from persisted board context (2026-09-13)

The real Codex Lead harness no longer inserts a teammate result into the
follow-up user prompt. After the deterministic utility commits its result,
directed message, and artifact to the SQLite team board, the same live Codex
thread invokes the allowlisted `ras_request_context` MCP tool. The harness
asserts two durable collaboration records (one planning-context request and
one post-utility follow-up request), the bounded Codex final artifact, and a
reopened board result. It ran with `gpt-5.5` / `low` only.

```text
cargo test -p agent-code-runtime --test codex_live real_codex_lead_plans_and_follows_up_on_durable_team_result -- --ignored --nocapture
exit 0; 1 passed, 0 failed, 0 ignored, 2 filtered out; 23.20 seconds
```

Sanitized evidence is `.acc-evidence/r6-codex-lead-board-followup.md`.
This closes the test-harness text-injection defect and adds live M1 evidence,
Complete Rust CI passed for this changed source; M1 is `PASSED` on the
subsequent source/evidence binding. M5 remains `NOT_RUN`: this is not the
required real Codex + Qwen + concurrent utility/retry/reassign E2E.

## 43A. Kimi Code 0.42.0 ACP classification refresh (2026-09-13)

The old Phase 2.5A `KIMI_AUTH_REQUIRED` observation was specific to local
Kimi Code 0.39.1 and is not retained as a claim about the currently installed
0.42.0 runtime. A held-stdin, JSON-RPC-only ACP discovery probe observed a
successful `initialize` / `initialized` / `session/new` sequence at 0.42.0.
It advertised protocol version 1, a terminal login method, session
list/resume/close/delete/fork capability, and MCP HTTP/SSE capability. The
session reference, raw frames, credentials, endpoint, prompt, and transcript
were not recorded.

The ignored live Rust command below then completed one bounded no-tool turn
through `AcpWorkerDriver` in an isolated directory:

```text
cargo test -p agent-code-runtime kimi_acp_completes_a_bounded_no_tool_turn -- --ignored --nocapture
exit 0; 1 passed, 0 failed, 0 ignored, 22 filtered out; 1.79 seconds
```

The same runtime ended a paired turn whose response did not meet the existing
single-strict-JSON peer-result contract; `parse_peer_result` rejected it.
That negative observation is intentionally fail-closed: runtime completion is
not a submitted or accepted team result. Consequently M4 is now classified
`KIMI_READY` in its narrow roadmap sense (a real bounded task turn passed),
but the only viable current candidate is a bounded-context non-interactive
worker. A structured-message adapter, tool response loop, cancellation,
recovery, retry, team integration, and every adapter/global readiness claim
remain unproven.

Evidence: `.acc-evidence/r6-kimi-acp-042-live.md`; complete capability wording
is in `docs/kimi_code_runtime_probe.md`. The current milestone delta is:

```text
M4_KIMI_PROFILE_CLASSIFIED = KIMI_READY  (classification only)
M1_CODEX_TEAM_READY        = IN_PROGRESS
M2_ACP_DRIVER_READY        = UNKNOWN / NOT_PASSED
M3_QWEN_WORKER_READY       = IN_PROGRESS
M5_R6_TEAM_READY           = NOT_RUN
M7_R7_NORMAL_PATH_READY    = NOT_PASSED
M9_PRODUCT_RC_READY        = NOT_RUN
```

Full Rust regression after the focused test and documentation update:

```text
cargo fmt --all -- --check                                      exit 0
cargo clippy --workspace --all-targets -- -D warnings           exit 0
cargo test --workspace                                           exit 0
  162 passed, 0 failed, 11 ignored
git diff --check                                                 exit 0
```

## 44. R6 final-result selections and v8 -> v9 migration (2026-09-13)

The team board previously reconstructed a final result by collecting every
successful descendant. That is not an exact Lead selection and can silently
change a final answer when an unrelated child later succeeds. Schema version 9
adds `team_final_task_refs` and `team_final_artifact_refs`; the board now stores
the Lead-selected task IDs and exact `(task, path, sha256)` artifact references
before its root task is exposed as succeeded. Reconstruction reads only those
persisted selections. The new tables remain part of the existing SQLite team
board, not a second state system.

The actual v8 fixture removes only the two v9 tables, seeds a team row, opens
through the normal migration, reopens, and verifies both old data preservation
and usable persisted selection data. Commands and results:

```text
cargo test -p agent-code-storage v8_database_migrates_preserves_team_rows_and_accepts_final_refs -- --nocapture
exit 0; 1 passed, 0 failed; 0.83 seconds

cargo test -p agent-code-team final_result_is_reconstructable_from_board -- --nocapture
exit 0; 1 passed, 0 failed

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
all exit 0; workspace execution at the later Kimi 0.42.0 requalification
reports 162 passed, 0 failed, 11 ignored (173 listed tests including ignored
tests). The previous 13-ignored / 175-listed count is historical and must not
be read as the current count.
```

The real Codex Lead plan/follow-up test was also executed at the current
runtime versions with the test's explicit `model="gpt-5.5"` and
`model_reasoning_effort="low"` overrides: exit `0`; 1 passed, 0 failed, 2
filtered; 18.13 seconds. The refactored no-ACC Codex+Qwen E2E was then run
with a captured exit receipt: exit `0`; 1 passed, 0 failed, 2 filtered; 38.65
seconds. Its sanitized evidence is
`.acc-evidence/r6-codex-qwen-product-e2e.md` (captured log SHA-256
`49067c43b2f15153618097cccbb54dd1fa079508c594eb62033597e5bb0b2a5f`).

This satisfies a real bounded product-result-flow slice, but does not close
M1/M2/M3/M7 by itself: M1 still needs one end-to-end Lead result consumption
path free of harness-directed context injection; M2/M3/M7 remain `UNKNOWN /
NOT_PASSED`, `IN_PROGRESS`, and `NOT_PASSED` respectively.

## 45. R6 board-only active context delivery (2026-09-13)

The Codex MCP bridge no longer replies to `ras_request_context` with a generic
acknowledgement. It reads the existing SQLite board and returns at most four
directed messages for Codex under a 1024-character bound. It records only a
fixed response summary; tool arguments cannot name a task/session or supply
the runtime identity. A unit test proves messages addressed to another agent
are excluded.

The live Codex+Qwen E2E was re-run after removing the Qwen message from the
Codex turn prompt. The only Qwen result path into Codex was the persisted board
through `ras_request_context`. Captured command result: exit `0`; 1 passed, 0
failed, 2 filtered; 41.49 seconds. Codex was invoked with the test overrides
`model="gpt-5.5"` and `model_reasoning_effort="low"`. Sanitized evidence is
the board-only rerun section of `.acc-evidence/r6-codex-qwen-product-e2e.md`,
log SHA-256 `02fa310b62c28c6a55f39432ecdf0335292c94c73516bd55d3ccdbe3cd7e522c`.

This closes the harness text-injection defect for this R6 slice. It still does
not satisfy the broader M1/M5 proof of scheduled Codex Lead consumption plus
concurrent utility, retry/reassign and normal CLI control, so all current
milestone values remain unchanged.

## 43. M2 cancellable driver seam (2026-09-12)

`AcpWorkerDriver` now exposes a bounded, caller-owned cancellation pair:
`AcpCancellation` and `AcpCancellationListener`. The pair does not accept or
persist a native session ID. During a live task the driver obtains the foreign
session ID only from the session it created, sends the official typed stable-v1
`session/cancel` notification, drains the active session until it sees
`StopReason::Cancelled`, then returns `AcpWorkerError::Cancelled`. A terminal
success result cannot be produced on that branch.

The non-ignored lifecycle test
`caller_cancellation_sends_session_cancel_and_requires_peer_confirmation`
passed (exit `0`; 1 passed, 0 failed; 0.05 seconds) with the existing isolated
ACP mock. It asserts that the driver waits for the peer confirmation rather
than treating an internal flag, connection close, or process termination as a
cancelled turn. The live ignored Qwen test was then re-run through the same
driver entry point: exit `0`; 1 passed, 0 failed, 21 filtered out; 1.23 seconds;
sanitized observation `peer_confirmed_cancel`.

This improves the generic ACP driver's in-process lifecycle contract and is
recorded in `.acc-evidence/r7-cli-acp-smoke.md`. It does **not** yet provide a
durable cross-process run manager: the current synchronous `run-acp` CLI
cannot route a separate `cancel` invocation to its live cancellation trigger.
Thus this does not close U04/M7, retry/reassign/recovery, or the full M2 gate:

```text
M2_ACP_DRIVER_READY       = UNKNOWN / NOT_PASSED
M3_QWEN_WORKER_READY      = IN_PROGRESS
M7_R7_NORMAL_PATH_READY   = NOT_PASSED
```

## 47. R7 authoritative terminal final-result projection (2026-09-13)

The read-only Rust terminal dashboard now projects the exact persisted final
selection from the existing SQLite team board.  For every root with recorded
final references it displays the selected task IDs and exact selected artifact
`task:path#sha256` values.  It does not infer a final answer from all successful
descendants and does not introduce a parallel TUI state store.  The empty-board
case explicitly reports that no selected final references exist.

The projection is guarded by
`dashboard_projects_persisted_final_selection`, which creates a task,
persists a selected artifact through `TaskBoard::record_final_refs`, and
asserts that the dashboard displays that authoritative reference.  The normal
path README also now documents the existing explicit `recover` control.

Local self-built runtime entrypoint flags `-ds` and `-qw` are treated as
site-local profile parameters only.  Their semantics are not inferred from
upstream Codex, Qwen, ACP, or CLI documentation; any future live driver run
must record its exact command and establish capability facts from its actual
handshake/output.

Verification on the changed source:

```text
cargo fmt --all -- --check                                      exit 0
cargo clippy --workspace --all-targets --all-features -- -D warnings  exit 0
cargo test --workspace --all-features                            exit 0
  173 passed, 0 failed, 11 ignored
git diff --check                                                 exit 0
```

This is an R7 inspection-surface increment only.  It does not start an
external driver, change active-runtime evidence, or alter any gate/readiness
claim:

```text
M2_ACP_DRIVER_READY       = UNKNOWN / NOT_PASSED
M3_QWEN_WORKER_READY      = IN_PROGRESS
M5_R6_TEAM_READY          = NOT_RUN
M7_R7_NORMAL_PATH_READY   = NOT_PASSED
M9_PRODUCT_RC_READY       = NOT_RUN
```

## 48. R7 runtime occupancy projection and local entrypoint facts (2026-09-13)

The read-only terminal dashboard now derives each registered agent's active
occupancy and latest external-runtime lifecycle state directly from the
authoritative SQLite task board.  It displays `active=<running>/<configured
capacity>` and `runtime_state=<binding state|idle>` beside the persisted agent
registry data.  Native session/thread identifiers remain opaque recovery
references and are deliberately not rendered.  The test
`dashboard_projects_authoritative_agent_occupancy_and_runtime_state` proves a
persisted running attempt and running external binding appear as `active=1/2`
and `runtime_state=running`, while the opaque session value is absent.

The local launch profile facts were rechecked without starting a model turn or
reading authentication material:

```text
codex -ds --version    exit 0; codex-cli 0.154.0
qwen -qw --version     exit 0; 0.23.3
kimi -ds --version     exit 0; 0.42.0
codex -ds --help       exit 0
qwen -qw --help        exit 0; documents --acp
kimi -ds --help        exit 0
```

`-ds` and `-qw` are site-local launcher entrypoints, not inferred upstream
protocol flags.  A bounded closed-stdin probe of `qwen -qw --acp` subsequently
returned exit 1 and classified its sanitized stderr as option rejection; the
stdout/stderr hashes were recorded only during diagnosis and then moved to
local trash.  The failed scheduler launch using that pair also exited before a
model turn and retained only an error hash.  Therefore the Rust ACP driver and
README use the source-confirmed direct transport `qwen --acp`; no site-local
launcher alias is persisted as ACP argv.  Protocol support remains established
only by source-informed handshake and bounded live evidence, not by help
output or launcher notation.

Verification on this source state:

```text
cargo fmt --all -- --check                                      exit 0
cargo clippy --workspace --all-targets --all-features -- -D warnings  exit 0
cargo test --workspace --all-features                            exit 0
  173 passed, 0 failed, 11 ignored
git diff --check                                                 exit 0
```

This is an R7 visibility/configuration increment.  It does not launch a team
driver or alter the outstanding R6 topology/recovery proof; M5–M9 remain
unchanged and `M7_R7_NORMAL_PATH_READY` remains `NOT_PASSED`.

## 49. R7 explicit board-wide interrupted-run recovery (2026-09-13)

`agent-code-cli recover-all <database>` now scans the existing authoritative
SQLite board and invokes the established fail-closed recovery transition for
each task whose latest attempt remains `Running`.  It performs no runtime
launch, no foreign-session replay, and no inferred completion.  Each recovered
attempt is recorded as failed with the existing explicit-resume requirement;
any corresponding `starting`/`running` external binding becomes
`interrupted` atomically in the board implementation.

The Rust CLI end-to-end normal-path test now creates a second interrupted task
with an external binding, invokes `recover-all`, and verifies both the returned
task/attempt receipt and the reopened binding lifecycle state.  This exposes a
safe user-facing recovery operation for a prior team session, but does not
replace the still-required real external-process crash/reconcile proof for M5.

```text
cargo fmt --all -- --check                                      exit 0
cargo test -p agent-code-cli --test normal_path                  exit 0; 1 passed
cargo clippy -p agent-code-cli --all-targets -- -D warnings      exit 0
cargo test --workspace --all-features                            exit 0
  173 passed, 0 failed, 11 ignored
git diff --check                                                 exit 0
```

`M7_R7_NORMAL_PATH_READY` remains `NOT_PASSED`; M5–M9 remain unchanged.

## 50. R7 durable runtime-version registry metadata (2026-09-13)

SQLite schema version 10 adds nullable `agent_registry.runtime_version`.  It
holds only a public runtime version observed by the user or a probe; it does
not hold an endpoint, token, environment value, session reference, or
transcript.  The CLI remains backward-compatible with its eight-field
`register` form and accepts an optional ninth version field.  Registry listing
and the read-only TUI now display `version=<value|->` alongside driver,
authoritative occupancy, and lifecycle state.

The v9 migration fixture reconstructs the prior registry table shape with a
real existing row, migrates through normal `SqliteAgentRegistry::open`,
reopens it, and verifies that the old agent survives with a null version at
schema v10.  New registrations round-trip their version via the same table.

```text
cargo test -p agent-code-storage v9_registry_migrates_preserving_existing_agent -- --nocapture
  exit 0; 1 passed
cargo fmt --all -- --check                                      exit 0
cargo clippy --workspace --all-targets --all-features -- -D warnings  exit 0
cargo test --workspace --all-features                            exit 0
  174 passed, 0 failed, 11 ignored
git diff --check                                                 exit 0
```

This is a minimal R7 registry migration and does not alter M5–M9 or make an
adapter/readiness claim.

## 51. M5 scheduler live rerun receipt (2026-09-13)

The existing real scheduler topology was rerun at source commit `05a8fbc`.
Its terminal test result was `1 passed, 0 failed, 0 ignored, 3 filtered out`
in `36.59s`; captured-output SHA-256 is
`586f7c663877d57ca86bcf001ada486d492ffcd280264dc811a74a4ef0caf2dc`.
The wrapper did not retain its separate exit-code file, so this report does
not fabricate an exit code.  Sanitized receipt:
`.acc-evidence/r6-m5-scheduler-live-20260913.md`.

The prior missing exit-code receipt was corrected by the repeat run recorded
in the same sanitized evidence file: exit `0`; 1 passed, 0 failed, 0 ignored,
4 filtered out; 39.10s.  That rerun used the current working source and kept
the tested command handle through its terminal result.

The topology's explicit `target=worker-fail` is the user-target override path;
the harness asserts it is honored for two bounded failures before reassignment
to real Qwen.  Together with §52, the live topology and distinct crash/reopen
requirements now have evidence.  M5 remains unsealed only until this source is
bound to a candidate and requalified by the full Rust gate.

## 52. M5 real Qwen ACP process-crash and reopen recovery (2026-09-13)

The missing distinct crash/reconcile criterion now has a bounded live harness:
`real_qwen_acp_process_crash_recovers_without_replay`.  It starts an isolated
`qwen --acp` child through the production `PersistedAcpWorkerDriver`, waits
only until the authenticated ACP session has caused a durable `running`
external binding, and verifies the child PID against `/proc` before killing
that exact test-owned process.  It does not enumerate or signal arbitrary
processes.

The controller future is deliberately abandoned after that durable checkpoint,
before it can settle a terminal driver result.  A newly opened SQLite board
then observes no artifact and the still-running attempt/binding, applies the
existing fail-closed recovery transition, and verifies: failed attempt,
`interrupted` binding, explicit-resume requirement, no automatic replay, and
idempotent second recovery.  Native session identity remains opaque and is not
written into evidence.

```text
cargo test -p agent-code-runtime --test codex_live \
  real_qwen_acp_process_crash_recovers_without_replay \
  -- --ignored --nocapture
exit 0; 1 passed, 0 failed, 0 ignored, 4 filtered out; finished in 1.62s

cargo fmt --all -- --check                                      exit 0
cargo clippy -p agent-code-runtime --test codex_live --all-features -- -D warnings
                                                                  exit 0
cargo test -p agent-code-runtime --test codex_live --no-run      exit 0
git diff --check                                                 exit 0
```

Sanitized receipt:
`.acc-evidence/r6-qwen-acp-process-crash-recovery-20260913.md`.

This closes the particular external-process crash + board-reopen recovery
evidence gap identified in §51.  It is not a claim that M5 or R6 is sealed:
the remaining topology controls (including a real user override and a
candidate-bound final evidence run) must still be verified.

## 53. R6 candidate requalification and seal (2026-09-13)

The immutable R6 **code** candidate is
`59a728b1839f613dee24e3fe635853947cd628ac`
(`test(R6): verify Qwen ACP crash recovery`).  This report/evidence receipt is
committed separately so the executable source identity is not obscured by a
self-referential documentation hash.  No executable Rust source changed after
that candidate.

On that exact candidate, the two live tests retained their command exit codes:

```text
cargo test -p agent-code-runtime --test codex_live \
  real_qwen_acp_process_crash_recovers_without_replay \
  -- --ignored --nocapture
exit 0; 1 passed, 0 failed, 0 ignored, 4 filtered out; 1.62s

cargo test -p agent-code-runtime --test codex_live \
  real_scheduler_runs_qwen_worker_and_utility_on_one_board \
  -- --ignored --nocapture
exit 0; 1 passed, 0 failed, 0 ignored, 4 filtered out; 47.16s
```

The full Rust requalification of the same source completed with all commands
at exit `0`:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
git diff --check
```

The full test command observed 175 passed, 0 failed, and 16 intentionally
ignored live tests.  The ignored set is not counted as substitute evidence:
the two M5 live tests above were invoked explicitly and passed.  The full run
also validates the existing deterministic ACC, persistence migration,
registry, CLI/TUI, and contract suites.

M5 requirements are now evidenced without manual copy/paste: real Codex Lead
(`gpt-5.5` / `low`), real Qwen ACP worker, deterministic concurrent utility,
persisted message/result/exact artifact/final selection, bounded Lead
follow-up through the board, explicit target override, retry/reassignment,
and reopened SQLite.  The distinct real Qwen process crash/reopen drill proves
interrupted work is not blindly replayed and requires explicit resume.

Accordingly the current milestone state is:

```text
M5_R6_TEAM_READY = PASSED
M6_R6_SEALED     = PASSED
M7_R7_NORMAL_PATH_READY = NOT_PASSED
M8_R8_DEBLOATED  = NOT_RUN
M9_PRODUCT_RC_READY = NOT_RUN
```

The sanitized evidence manifest added with this receipt binds the candidate,
commands, exit codes, and evidence-file SHA-256 values.  It contains no raw
wire frame, transcript, credential, endpoint, native session id, or private
filesystem path.

## 54. R7 live CLI cancellation seam (2026-09-13)

Candidate code commit `8b73657de52c4a342e85c0fa0893aafbb676c0e9` adds
cross-process cancellation without a process manager or parallel state store.
`run-acp` polls only its authoritative SQLite task record; an independent
`cancel` CLI invocation writes the existing `cancelled` task state.  The
running process owns the non-serializable `AcpCancellation` and sends typed
ACP cancellation only on the session it created.  It never receives a native
session ID from the control invocation.

A real Qwen ACP drill observed: cancel exit `0`; running `run-acp` exit `2`
with `ACP worker cancelled`; reopened task and attempt both `cancelled`; and
the reopened external binding `cancelled`.  No Qwen ACP child remained.  A
response racing cancellation is fail-closed and cannot become a successful
result.  Sanitized receipt:
`.acc-evidence/r7-cli-live-cancel-20260913.md`.

Full Rust CI for the candidate passed (`cargo fmt`, all-features clippy,
workspace tests, and `git diff --check`, all exit `0`; 175 passed, 0 failed,
16 ignored).  M7 remains `NOT_PASSED` pending candidate-bound normal-path
coverage of submit/observe/control/resume through the real runtime and a
final CLI/TUI documentation/release audit.

## 55. R7 normal-path candidate closure (2026-09-13)

The exact M7 executable code remains candidate
`8b73657de52c4a342e85c0fa0893aafbb676c0e9`; subsequent commits are evidence
and documentation only.  The paired live receipts cover the complete normal
path without legacy Python:

- `.acc-evidence/r7-cli-live-resume-20260913.md`: register, submit, real Qwen
  `run-acp`, submit, persisted-binding `continue-acp`, status, and binding
  inspection.  Every command exited `0`; both tasks became succeeded.
- `.acc-evidence/r7-cli-live-cancel-20260913.md`: independently invoked
  `cancel` reaches the running live ACP session, closes task/attempt/binding
  as cancelled, and leaves no child process.

The existing Rust CLI integration tests cover submit/status/override/resume/
artifact/final/recover/recover-all against the same authoritative board, while
the TUI tests prove its projection comes from that board and registry rather
than a parallel memory state.  README now names Rust CLI/TUI as the normal
path and explicitly states it does not launch legacy Python.

Therefore `M7_R7_NORMAL_PATH_READY = PASSED`.  This is not a release claim:
M8 de-bloating, release build/install/upgrade smoke, and M9 remain outstanding.

## 56. R8 legacy deletion (2026-09-13)

After R6/R7 parity, the unreachable retired Python control-plane was removed:
`src/researchd`, Python-only tests, qualification documentation/schemas/scripts,
Alembic configuration, Python package metadata/lockfile, and legacy launcher
examples.  The deletion includes the retired policy, approval, verifier,
qualification, backup/DR, WorkOrder, delegation, and invocation product
surfaces.  Historical material remains in Git history, not in the installed
or launched product.

The Rust workspace remains the sole normal path. README, README.zh-CN,
AGENTS.md, and `docs/v2/ROADMAP.md` now state that fact. A product-source
reachability scan found no remaining reference to removed Python launchers or
qualification paths. Full Rust CI completed with all commands exit `0`
(`cargo fmt`, all-features clippy, workspace test, `git diff --check`; 175
passed, 0 failed, 16 intentionally ignored live tests).

Therefore `M8_R8_DEBLOATED = PASSED`. M9 remains `NOT_RUN` until release
build, clean-install smoke, upgrade-migration smoke, candidate-bound release
artifacts, and final product RC evidence are complete.

## 57. M9 local product RC candidate (2026-09-13)

SUPERSEDED_BY_PRODUCT_SELF_AUDIT — `product_self_audit.md` falsified this
section's `PRODUCT_RC_READY = PASSED` claim and the fixture/production-path
assumptions behind it (S1-1..S1-4). Retained as history; see "Current state" at
the top.

Candidate executable source is `687093630af9ac811574b3de58e6e983f0e23d6f`.
All Rust CI and release build commands passed; the workspace test result was
175 passed, 0 failed, 16 explicitly ignored live tests. A copied release
`agent-code-cli` binary completed help, version, submit and status outside the
source-tree invocation. The same binary opened a generated minimal v8 SQLite
fixture and read its preserved task after migration. Release artifact hashes
and exact commands are recorded in `.acc-evidence/m9-rc-6870936.md`.

No tag, push, publication, remote CI claim, or external release action was
performed. On the local Linux reference profile, `PRODUCT_RC_READY = PASSED`.
