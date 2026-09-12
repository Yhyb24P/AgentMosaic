# ACC/0.1 Rust-v2 Implementation Report

## 1. Machine-readable summary
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
{"active_roadmap":"R6-R8","active_product":"heterogeneous-agent-coding-team","milestones":{"M1_CODEX_TEAM_READY":"IN_PROGRESS","M2_ACP_DRIVER_READY":"IN_PROGRESS","M3_QWEN_WORKER_READY":"IN_PROGRESS","M4_KIMI_PROFILE_CLASSIFIED":"KIMI_AUTH_REQUIRED","M5_R6_TEAM_READY":"NOT_RUN","M6_R6_SEALED":"NOT_RUN","M7_R7_NORMAL_PATH_READY":"IN_PROGRESS","M8_R8_DEBLOATED":"NOT_RUN","M9_PRODUCT_RC_READY":"NOT_RUN"}}
```

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
| Codex | `codex-cli 0.154.0` | app-server stdio + allowlisted RAS MCP | thread/turn, same-turn bounded context, artifact, persisted binding/reopen, thread resume, real same-thread two-task plan and durable utility follow-up | M1 in progress: full Qwen topology/final selected refs still missing |
| Qwen Code | `0.23.3` | `qwen --acp` via `agent-client-protocol 2.1.0` | authenticated session/follow-up/coding evidence; after 600 s budget calibration the live cross-runtime command passed in 136.09 s | `IN_PROGRESS`: live bounded worker/result-artifact-to-Lead evidence exists; real ACP continuation/cancel remains required before M3 can pass |
| Kimi Code | `0.39.1` | `kimi acp` candidate | ACP initialize live; session/task blocked by local authentication readiness | `KIMI_AUTH_REQUIRED` |

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
