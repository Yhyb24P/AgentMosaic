# Rust v2 Product Self-Audit

Audit date: 2026-09-13. This is an independent audit under
`CODEX_PRODUCT_SELF_AUDIT.md`, not an implementation plan or a modification of
`implementation_report.md`. Its purpose was to falsify `PRODUCT_RC_READY`.

## 1. Source state

```text
HEAD:             fbc80bfe80837747cdfeb8d65e31ec641e9f552b
branch:           v2/rust-agent-team
dirty:            only untracked .audit/product-self-audit/ audit receipts
candidate source: 687093630af9ac811574b3de58e6e983f0e23d6f
evidence commit:  fbc80bfe80837747cdfeb8d65e31ec641e9f552b
SCHEMA_VERSION:   10
workspace version: 0.1.0 (all workspace crates)
```

The audit used an additional detached worktree at the exact candidate for
build/test work. The evidence commit differs from the candidate only by
`.acc-evidence/m9-rc-6870936.md` and `implementation_report.md`; it contains
no executable or Cargo-source change.

Audit receipts are deliberately separate from the candidate at
`research-agent-system/.audit/product-self-audit/`:

| Receipt | SHA-256 |
|---|---|
| `inventory.md` | `9e36cba4f7cb2e27fd2cae251e2afc4bd877cff94dca0baa5660034027018df1` |
| `exact-candidate-gates.log` | `a228e15b4d7b5c74a76e10f5ba32908f6999a710d1520f5c4a40fe650241bfec` |
| `v8-fixture-before.txt` | `95729d8a823d4f48ed080e55ec8f7ec6b2684931e185a5a33ae32a30b979f7eb` |

## 2. Findings

| ID | Area | Severity | Verdict | Evidence |
|---|---|---:|---|---|
| H1 / A1 | Normal product orchestration | S1 | `FAIL_NO_PRODUCT_ORCHESTRATION_ENTRYPOINT` | `agent-code-cli submit` only calls `create_task`; its command set has no `Lead`/`Scheduler` composition or registered-driver launch. `run-acp` is a separate, manual task/agent invocation. See `crates/agent-code-cli/src/main.rs:598-767`. |
| H2 / A2 | Production Lead versus test harness | S1 | `FAIL_TEST_HARNESS_IS_PRODUCT_ORCHESTRATOR` | `codex_live.rs` reads `lead-plan.json`, manually creates tasks, runs a worker, and manually commits/finalizes results. The production CLI never invokes `Lead`. See `crates/agent-code-runtime/tests/codex_live.rs:735-859`; `lead.rs` is a library implementation, not a production entrypoint. |
| H3 / A3 | Codex result fidelity | S1 | `FAIL_CODEX_RESULT_PLACEHOLDER` | `PersistedCodexTeamDriver` returns literal summary `Codex scheduler task completed` on `TurnCompleted`, rather than a bounded final Codex answer. See `crates/agent-code-runtime/src/codex_team_driver.rs:190-198`. |
| H4 / A4 | Release migration fixture | S1 | `FAIL_FAKE_OLD_SCHEMA_FIXTURE` | `make_v8_fixture` executes current `SCHEMA`, only drops v9 tables, and sets `user_version=8`. Before migration the generated DB reports `runtime_version` in `agent_registry`, though source comments identify it as v10. See `make_v8_fixture.rs:10-19`, `schema.rs:1-9`, receipt `v8-fixture-before.txt`. |
| H5 / A5 | User-facing documentation | S3 | `FAIL_DOC_DRIFT` | README says “R5 … under review” and “R7 in progress”; ROADMAP says R5 is unsealed/local/unpushed, while later report sections claim M1–M9 passed. See `README.md:5-10,122`; `docs/v2/ROADMAP.md:29-32`. |
| H6 / A6 | TUI product role | S2 | `LIMITED_READ_ONLY_TUI` | TUI renders board text and recognizes only `q`; it cannot submit, start, cancel, override, retry, resume, or inspect artifact/final result. See `crates/agent-code-tui/src/main.rs:31-47`. |
| H7 / A7 | Remote CI | S2 | Hypothesis refuted narrowly | GitHub has a successful `rust.yml` run for evidence commit `fbc80bf` (run `34754166636`, 2026-09-13T11:20:08Z). It runs format, non-all-features clippy, and workspace tests. It is not an immutable-candidate/tag release qualification. |
| A8 | CLI role | S1 | `FAIL_CLI_IS_BOARD_CONTROL_ONLY` | CLI is `BOARD_CONTROL_SURFACE`, not `PRODUCT_ORCHESTRATOR`: `submit/status/cancel/override/recover/resume/artifact/final/register/registry` manipulate or inspect SQLite; only manually selected `run-acp` executes a single external task. |
| A9 | Durable successful-result commit | S2 | partial implementation proved | `commit_successful_result` inserts message/artifacts, updates attempt, then marks task succeeded in one SQLite transaction. This prevents the stated partial-success crash state for that API. See `board.rs:245-301`. It does not prove a real worker result reaches a production Lead follow-up because no production team loop exists. |
| A10 | Qwen ACP product flow | S1 | `FAIL_QWEN_PRODUCT_FLOW` | `qwen --version` returned `0.23.3`, so runtime availability is not the finding. This audit did not accept historical report assertions: all relevant live tests are `#[ignore]`, and the exposed user path requires manual `run-acp`; the full required product flow was not independently demonstrated. |
| A11 | Crash/recovery evidence | S2 | `PARTIAL` | Synthetic and mock lifecycle tests exist. The real Qwen process-crash test is ignored. Therefore synthetic interrupted-row evidence is not treated as proof of a live external crash/recovery path in this audit. |
| A12 | Report current-state consistency | S3 | `FAIL_REPORT_CURRENT_STATE_AMBIGUOUS` | The beginning still declares `status=PARTIAL`, old `e23ae817` state, schema v7, and adapters not ready; later section 29 declares M1–M9 passed; section 57 claims local `PRODUCT_RC_READY`. The old block is not clearly marked historical/non-current. |

### H1–H7 disposition

```text
H1 CONFIRMED  — no documented/implemented normal team-loop command.
H2 CONFIRMED  — test code performs material orchestration absent from product entrypoints.
H3 CONFIRMED  — persisted Codex driver summary is a literal placeholder.
H4 CONFIRMED  — claimed v8 fixture structurally includes v10-only column.
H5 CONFIRMED  — README/ROADMAP materially lag asserted milestone state.
H6 CONFIRMED  — TUI is read-only, q-to-exit dashboard.
H7 REFUTED NARROWLY — a successful remote Rust workflow exists for fbc80bf; release-grade remote qualification remains unproven.
```

## 3. Inventory

The recorded inventory includes the current Git state, last 12 commits,
workspace metadata, schema version, production drivers, ignored live tests and
workflow sources. Production `AgentDriver` implementations are
`PersistedCodexTeamDriver`, `PersistedAcpWorkerDriver`, and `AcpWorkerDriver`;
other implementations found are test doubles. The CLI commands are:

```text
register registry run-acp continue-acp submit status cancel override
recover recover-all resume artifact binding final
```

TUI control is only `q`. There are 16 ignored live tests (Qwen, Kimi, Codex,
and live cross-runtime cases). Workflows present are `rust.yml`, plus stale
Python-era `quality.yml` and `candidate.yml` that reference removed Python
tools and product paths.

## 4. Exact-candidate build and executable audit

At detached `687093630af9ac811574b3de58e6e983f0e23d6f`:

| Command | Exit | Observed result |
|---|---:|---|
| `cargo fmt --all -- --check` | 0 | passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 0 | passed |
| `cargo test --workspace --all-features` | 0 | 175 passed, 0 failed, 16 ignored |
| `cargo build --release --workspace` | 0 | passed |
| `git diff --check` | 0 | passed |

A copied release `agent-code-cli` binary was run outside the source tree:
`--help`, `--version`, `submit`, and `status` returned exit 0. The resulting
task was `pending`, unassigned, with zero attempts. This is valid board smoke,
but directly demonstrates that it is not a normal heterogeneous-team execution
smoke: no registered agent, scheduler, Lead, or runtime was invoked.

Candidate/evidence binding itself is valid: the two commit diff contains only
the M9 evidence file and `implementation_report.md`. Binding cannot cure S1
product defects.

## 5. Migration and recovery audit

`SCHEMA_VERSION` is 10. The fixture generator creates `SCHEMA` at current
version before claiming it is v8. The generated pre-migration database had:

```text
PRAGMA user_version = 8
agent_registry columns include runtime_version TEXT
team_tasks row preserved v8 release task | bulk | pending
```

`runtime_version` is expressly declared a v10 addition. Therefore the fixture
does not represent a real v8 schema and cannot validate a v8-to-current
migration. Existing v4/v7 migration tests and the transactional durable-result
implementation remain useful narrow evidence, but they do not make the M9
fixture claim valid.

## 6. T01–T16 matrix

`REAL_PRODUCT_PATH` is reserved for behavior reachable by a new user through
the documented normal product path, not merely callable library/test code.

| Requirement | REAL_PRODUCT_PATH | TEST_ONLY | NOT_PROVEN | Evidence |
|---|---:|---:|---:|---|
| T01 configure Reasoner + Worker | yes |  |  | CLI `register` persists tier/driver config; it does not construct drivers. |
| T02 submit one objective | yes |  |  | CLI `submit` persists one pending `team_tasks` row. |
| T03 Lead creates structured subtasks |  | yes |  | `Lead` library/tests; live test manually consumes `lead-plan.json`. |
| T04 reasoning routes to Reasoner |  | yes |  | Scheduler library/test routing, no product loop. |
| T05 bulk/tool routes to Worker/Utility |  | yes |  | Scheduler library/test routing, no product loop. |
| T06 two independent workers concurrent |  | yes |  | Scheduler concurrency tests only. |
| T07 worker result reaches Lead next context |  | yes |  | `Lead::build_context` exists, but no executable Lead path. |
| T08 worker artifact reaches Lead |  | yes |  | library/test board flow only. |
| T09 directed Agent message reaches context |  | yes |  | Scheduler code/tests only. |
| T10 Lead follow-up based on result |  | yes |  | scripted Lead; live harness manually sequences turns/tasks. |
| T11 failed task retries |  | yes |  | Scheduler tests only. |
| T12 failed task reassigns |  | yes |  | Scheduler tests only. |
| T13 user overrides assignment | yes |  |  | CLI `override` writes board assignment. |
| T14 local Qwen completes representative worker task |  |  | yes | Runtime binary exists, but required live product flow was not re-proven; live cases are ignored. |
| T15 high-intelligence reasoning/synthesis |  |  | yes | Codex tests ignored and driver returns a placeholder summary. |
| T16 final result without human copy/paste |  |  | yes | No production orchestration entrypoint; harness performs sequencing. |

## 7. Milestone re-evaluation

| Milestone | Audit status | Basis |
|---|---|---|
| `M1_CODEX_TEAM_READY` | `PARTIAL` | app-server transport/bounded collaboration source exists, but final Codex result is a placeholder and live team flow is harness-orchestrated. |
| `M2_ACP_DRIVER_READY` | `PARTIAL` | ACP driver and local `qwen 0.23.3` exist; full independently verified product flow/recovery is not proven. |
| `M3_QWEN_WORKER_READY` | `NOT_PROVEN` | no audit-run live worker product flow; relevant tests are ignored. |
| `M4_KIMI_PROFILE_CLASSIFIED` | `PARTIAL` | ACP-oriented source/tests identify Kimi; no independent live audit execution. |
| `M5_R6_TEAM_READY` | `FAILED` | S1 normal orchestration and result-flow failures. |
| `M6_R6_SEALED` | `FAILED` | cannot seal a failed R6 team product path. |
| `M7_R7_NORMAL_PATH_READY` | `FAILED` | normal CLI is board control plus manual runtime invocation, not the required heterogeneous-team normal path. |
| `M8_R8_DEBLOATED` | `PARTIAL` | retired Python product source is removed, but user docs and two workflows retain stale Python-era/review claims. |
| `M9_PRODUCT_RC_READY` | `FAILED` | S1 findings H1–H4 independently block RC readiness. |

## 8. Release and remote status

```text
LOCAL_REFERENCE_PROFILE_READY = false
REMOTE_CI_READY               = PARTIAL
PUBLIC_RELEASE_READY          = false
```

Remote `rust.yml` completed successfully for `fbc80bf`, but it does not prove
the all-features exact-candidate release build or end-to-end product behavior.
No tag contains candidate `6870936`; no public release was found. The stale
Python candidate workflow cannot qualify the Rust candidate.

## 9. Allowed claims

- The exact candidate builds, formats, clippy-checks, and passes its current
  non-ignored Rust test suite: 175 passed, 0 failed, 16 ignored.
- SQLite board operations, registry persistence, the scheduler/Lead library,
  and external driver boundaries exist in Rust source.
- Successful-result writes through `commit_successful_result` are atomic with
  their message/artifact/attempt/task updates.
- A copied release CLI can show help/version and create/inspect a pending board
  task outside the source directory.
- A successful GitHub `rust.yml` run exists for `fbc80bf`.

## 10. Forbidden claims

- `PRODUCT_RC_READY`, `M9_PRODUCT_RC_READY = PASSED`, or
  `LOCAL_REFERENCE_PROFILE_READY = true`.
- A documented new-user heterogeneous-team loop with automatic Lead planning,
  delegation, execution, result return, follow-up, and synthesis.
- Live team E2E without manual test-harness orchestration.
- Codex final reasoning/synthesis result fidelity through
  `PersistedCodexTeamDriver`.
- Authentic v8-to-current release migration qualification.
- `PUBLIC_RELEASE_READY` or immutable release/tag qualification.
- Full remote CI qualification beyond the observed `rust.yml` run.

## 11. Minimal repair plan (not implemented)

1. Add one documented production command/path that loads the persisted agent
   registry, constructs the actual drivers, creates a Lead, and owns the full
   scheduler/Lead loop from one objective through durable final result. Do not
   move this responsibility into an integration test.
2. Change the Codex app-server mapping to capture a bounded, sanitized actual
   final answer and persist it as `AgentTaskResult.summary`; retain no hidden
   reasoning or raw transcript.
3. Replace the fake v8 fixture with explicit historical v8 DDL that excludes
   both v9 final-ref tables and v10 `agent_registry.runtime_version`; test
   `PRAGMA table_info` before and after migration plus preserved rows.
4. Re-run a bounded real Codex+Qwen team E2E solely through the new production
   entrypoint. Demonstrate Qwen result/artifact → durable board → Lead next
   context → Codex follow-up/final answer, then crash/reopen/recovery without
   manual task creation or prompt injection.
5. Update README, Chinese README, ROADMAP, report current summary, and Rust
   GitHub workflows together. Replace/remove retired Python workflows and add
   exact candidate release qualification only after the S1 corrections pass.

## Final verdict

`PRODUCT_RC_READY = FAILED`.

The S1 findings are sufficient independently: the current normal product path
does not orchestrate a heterogeneous team, the live test harness supplies that
missing orchestration, Codex result fidelity is placeholder-only, and the v8
release migration fixture is structurally invalid. No implementation code or
`implementation_report.md` was changed by this audit.
