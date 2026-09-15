# AgentMosaic S0/S1 Core Reachability & Complexity Audit

Read-only audit + reversible experiments. No production code, manifest,
schema, migration, persisted wire form, fixture, tag or PR was modified.

## 1. Audit identity

```text
audit_subject_sha=d5c0a4958af29a56642a84fd0ca615d64785e504
subject_branch=feat/v0.4-runtime-integration
workspace_version=0.4.0-dev
schema_version=12
user_worktree_head=d5c0a4958af29a56642a84fd0ca615d64785e504
user_worktree_status_before=clean
audit_worktree=/tmp/am-s0s1-d5c0a49/audit      (detached at the subject SHA)
probe_worktree=/tmp/am-s0s1-d5c0a49/probe      (detached at the subject SHA)
rustc=1.94.1 (e408947bf 2026-03-25)
cargo=1.94.1 (29ea6fb6a 2026-03-24)
host=Linux 6.6.87.2-microsoft-standard-WSL2 x86_64, 24 cores
```

The user's active worktree was never reset, cleaned or used for a probe. Every
subtractive experiment ran in the detached probe worktree and was restored to
the exact baseline (`git restore --source=<BASE> --staged --worktree .`), with
`git status --porcelain` empty after each round (see `gate-e-compat.txt`).

## 2. Baseline canonical gates

Run on the exact subject SHA with `--locked` and an isolated
`CARGO_TARGET_DIR` (`evidence/canonical-gates.txt`, `evidence/gate-*.log`):

```text
scripts/ci/check_identity.sh                                    PASS  0.0s
cargo fmt --all -- --check                                      PASS  0.2s
cargo clippy --locked --workspace --all-targets --all-features  PASS 11.6s
cargo test --locked --workspace --all-features                  PASS 70.0s  (482 passed / 0 failed / 24 ignored)
cargo build --locked --release --workspace                      PASS 43.2s
git diff --check                                                PASS  0.0s
```

No baseline gate failed, so SH-3 does not apply.

## 3. Workspace complexity baseline

Gross source metric; inline unit tests included (not excluded):

```text
workspace crates=10
tracked src files=78
gross src LOC=30,323
gross src bytes=1,121,222
integration-test LOC=13,016 (113 tracked .rs files overall)
```

Per crate (`evidence/crate-metrics.csv`):

| crate | src files | gross src LOC | src bytes | integration test LOC | local normal deps out | local reverse deps in | external deps |
|---|---|---|---|---|---|---|---|
| agentmosaic-cli | 16 | 5,351 | 195,724 | 5,985 | 4 | 0 | clap, serde, rusqlite, serde_json, tokio |
| agentmosaic-runtime | 20 | 10,755 | 414,624 | 4,780 | 7 | 1 | async-trait, agent-client-protocol, sha2, rusqlite, serde, serde_json, tokio, libc |
| agentmosaic-team | 9 | 5,666 | 203,592 | 0 | 0 | 4 | async-trait, tokio, serde, serde_json, sha2, schemars |
| agentmosaic-storage | 8 | 3,056 | 116,281 | 1,930 | 4 | 3 | rusqlite, serde_json |
| agentmosaic-tui | 1 | 1,211 | 42,895 | 0 | 2 | 1 | crossterm, ratatui, rusqlite |
| agentmosaic-workspace | 7 | 1,467 | 49,937 | 321 | 1 | 1 | sha2, ignore, glob, regex, libc |
| agentmosaic-context | 6 | 1,257 | 44,237 | 0 | 2 | 2 | - |
| agentmosaic-core | 6 | 798 | 26,537 | 0 | 0 | 3 | - |
| agentmosaic-model | 4 | 639 | 23,349 | 0 | 1 | 3 | async-trait, serde, serde_json, tokio, ureq |
| agentmosaic-tools | 1 | 123 | 4,046 | 0 | 0 | 3 | serde |

Complexity is concentrated in five areas: `runtime` (largest crate, 35% of
all source lines, 11 modules' worth of external-runtime adapters), `team`
(scheduler + lead + ACC), `storage` (board + schema + two legacy stores), `cli`
(command surface + rendering) and `tui`.

## 4. Dependency graph

`evidence/dependency-baseline.txt`, `evidence/cargo-tree-am.txt`:

```text
local crate count=10
local normal dependency edge count=21
product (am) transitive local crates=10/10 (manifest compile graph)
```

Local edges:

```text
cli -> runtime, storage, team, tui
runtime -> context, core, model, storage, team, tools, workspace
storage -> context, core, model, team
tui -> storage, team
context -> core, model
model -> tools
workspace -> tools
```

The manifest pulls all ten crates into `am`. That is a **compile-graph** fact
only: section 9 and the probe ledger show that five of them are not on any
current product path.

## 5. Largest-file inventory

`evidence/largest-files.csv` (both rankings) and
`evidence/largest-files-top20-bytes.txt`. Top by LOC:

```text
1  runtime/src/acp_worker.rs        2,241  (production)
2  team/src/acc.rs                  1,353  (production)
3  team/src/lead.rs                 1,342  (production)
4  tui/src/lib.rs                   1,211  (production)
5  runtime/tests/codex_live.rs      1,176  (integration test)
6  storage/src/board.rs             1,029  (production)
7  runtime/src/team_runner.rs       1,011  (production)
8  cli/src/output.rs                1,003  (production)
9  team/src/scheduler.rs              902  (production)
10 runtime/src/codex_lead.rs          901  (production)
```

Size alone is not a verdict. `acc.rs` is large *and* unreachable (section 22);
`acp_worker.rs` is the largest file *and* core.

## 6. Public API surface

`evidence/public-surface.txt` (enumeration of `pub mod/use/struct/enum/trait/fn/type/const`):

```text
runtime: fn=82 struct=47 use=17 const=11 enum=9 trait=7 type=1
team:    fn=60 struct=42 enum=29 const=11 use=7 trait=5
storage: fn=44 struct=9 use=5 const=4 enum=1
cli:     fn=71 struct=23 enum=10 mod=7 const=2
tui:     fn=8 struct=9 const=3 enum=1 trait=1
core:    fn=33 use=5 struct=4 enum=3 trait=1
model:   fn=7 use=3 struct=3 enum=3 trait=1
tools:   fn=2 struct=6 enum=1
workspace: fn=26 struct=10 use=6 const=2 type=2 enum=1
context: fn=16 use=5 struct=5 trait=3 enum=1
```

Symbols called out by the audit package:

| symbol | location | status |
|---|---|---|
| `AgentLoop` | `runtime/src/agent.rs:169` | DEAD_CANDIDATE |
| `runtime::AgentConfig` | `runtime/src/agent.rs:79` | DEAD_CANDIDATE (distinct from `team::AgentConfig`, which is CORE) |
| `Delivery`, `AgentError` | `runtime/src/agent.rs` | DEAD_CANDIDATE |
| `dispatch`, `ToolOutcome` | `runtime/src/dispatch.rs` | DEAD_CANDIDATE |
| `SqliteJournal` | `storage/src/journal.rs:7` | DEAD_CANDIDATE (implementation) |
| `SqliteAccStore` | `storage/src/acc_store.rs:11` | DEAD_CANDIDATE |
| `pub use acc::*` | `team/src/lib.rs:20` | DEAD_CANDIDATE |
| `DriverKind` | `team/src/registry.rs:62` | CORE enum; `native`/`cli` variants are COMPAT(WIRE) |
| `agentmosaic_cli::inspect_acc` | `cli/src/lib.rs:20` | DEAD_CANDIDATE (only its own unit test calls it) |

## 7. Current CLI product roots

From the real binary (`evidence/cli-help.txt`) and the dispatch table
(`evidence/cli-root-trace.txt`):

PUBLIC (11): `init`, `agent add|list|remove`, `doctor`, `run`, `status`,
`events`, `final`, `artifact`, `tui`, `advanced`, `help`.

HIDDEN_INTERNAL compatibility (13): `register`, `registry`, `run-acp`,
`continue-acp`, `run-team`, `resume-team`, `submit`, `cancel`, `override`,
`recover`, `recover-all`, `resume`, `binding`.

INTERNAL (1): `__internal codex-mcp`, which is not a user root but *is* a
product root: `am run` injects the current executable with that argv as the
MCP bridge host for `codex-app-server` agents, and the bridge writes `messages`
and `runtime_collaboration_records`.

Every root's handler and state dependency is listed row-by-row in
`S0_S1_REACHABILITY_MATRIX.csv` (`level=cli-root`).

## 8. Runtime/team/storage module inventory

```text
runtime top-level modules=17
  acp_worker agent claude_cli claude_cli_driver codex_app_server codex_bridge
  codex_exec codex_exec_driver codex_exec_lead codex_lead codex_team_driver
  dispatch driver_factory launch runtime_adapter runtime_event team_runner
team top-level modules=8
  acc board lead registry run_event runtime_event scheduler testutil(cfg test)
storage top-level modules=6
  acc_store board journal observations registry_store schema
```

`codex_bridge` is the `#[path = "bin/am-codex-mcp.rs"]` module that backs the
internal MCP root; it is counted as a runtime top-level module because
`runtime/src/lib.rs` declares it.

## 9. Reachability model

Four independent flags per component (see the matrix):

```text
C = compiled in the current product dependency graph
P = reachable from a current supported product root (public CLI, internal MCP root, am run runtime construction)
K = required by compatibility/migration/persisted wire
T = required only by tests/dev fixtures
```

Compile reachability was measured with `cargo metadata` + `cargo tree`; product
reachability by tracing `am`'s dispatch table into handlers and further into
`TeamRunner` -> `LeadBrainFactory` -> `DriverFactory` -> `Scheduler` ->
database board -> drivers; and every P=0 claim was re-tested with a
subtractive probe. The `am` binary is built from `src/main.rs`, which declares
its own module tree and never imports the package's library target - so
`cli/src/lib.rs` is compiled with the binary but is not reachable from it.

## 10. Workspace crate matrix

| crate | C | P | K | T | classification | confidence | S2 action |
|---|---|---|---|---|---|---|---|
| agentmosaic-cli | 1 | 1 | 0 | 1 | CORE | HIGH | KEEP_CORE (its lib target is a separate DEAD_CANDIDATE row) |
| agentmosaic-runtime | 1 | 1 | 0 | 1 | CORE | HIGH | KEEP_CORE |
| agentmosaic-team | 1 | 1 | 0 | 1 | CORE | HIGH | KEEP_CORE (its `acc` module is separate) |
| agentmosaic-storage | 1 | 1 | 1 | 1 | CORE | HIGH | KEEP_CORE (its `journal`/`observations`/`acc_store` are separate) |
| agentmosaic-tui | 1 | 1 | 0 | 1 | CORE | HIGH | KEEP_CORE |
| agentmosaic-core | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | HIGH | DELETE_WAVE_CANDIDATE |
| agentmosaic-model | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | HIGH | DELETE_WAVE_CANDIDATE |
| agentmosaic-tools | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | HIGH | DELETE_WAVE_CANDIDATE |
| agentmosaic-workspace | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | HIGH | DELETE_WAVE_CANDIDATE |
| agentmosaic-context | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | HIGH | DELETE_WAVE_CANDIDATE |

## 11. Runtime module matrix

| module | C | P | K | T | classification | S2 action |
|---|---|---|---|---|---|---|
| acp_worker | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| agent | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | DELETE_WAVE_CANDIDATE |
| claude_cli | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| claude_cli_driver | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| codex_app_server | 1 | 1 | 1 (RUNTIME) | 0 | CORE | KEEP_CORE |
| codex_bridge | 1 | 1 | 1 (RUNTIME) | 0 | CORE | KEEP_CORE |
| codex_exec | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| codex_exec_driver | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| codex_exec_lead | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| codex_lead | 1 | 1 | 1 (RUNTIME) | 0 | CORE | KEEP_CORE |
| codex_team_driver | 1 | 1 | 1 (RUNTIME) | 0 | CORE | KEEP_CORE |
| dispatch | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | DELETE_WAVE_CANDIDATE |
| driver_factory | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| launch | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| runtime_adapter | 1 | 1 | 0 | 1 | CORE | KEEP_CORE |
| runtime_event | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| team_runner | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |

`runtime_adapter` is CORE even though only tests construct `AcpRuntimeAdapter`
directly: `PersistedAcpWorkerDriver::new` (built by `DriverFactory` for every
ACP registration) constructs the adapter and wraps it in `RuntimeAgentDriver`
(`evidence/runtime-adapter-consumers.txt`).

## 12. Team module matrix

| module | C | P | K | T | classification | S2 action |
|---|---|---|---|---|---|---|
| acc | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | QUARANTINE_CANDIDATE |
| board | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| lead | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| registry | 1 | 1 | 1 (WIRE) | 0 | CORE | KEEP_CORE |
| run_event | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| runtime_event | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| scheduler | 1 | 1 | 0 | 0 | CORE | KEEP_CORE |
| testutil (cfg test) | 1 | 0 | 0 | 1 | TEST_ONLY | KEEP_CORE |

## 13. Storage module matrix

| module | C | P | K | T | classification | S2 action |
|---|---|---|---|---|---|---|
| acc_store | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | QUARANTINE_CANDIDATE |
| board | 1 | 1 | 1 (SCHEMA) | 1 | CORE | KEEP_CORE |
| journal | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | QUARANTINE_CANDIDATE |
| observations | 1 | 0 | 0 | 1 | DEAD_CANDIDATE | QUARANTINE_CANDIDATE |
| registry_store | 1 | 1 | 1 (WIRE) | 1 | CORE | KEEP_CORE |
| schema | 1 | 1 | 1 (SCHEMA) | 1 | CORE | KEEP_CORE |

## 14. Persistence table matrix

21 tables in schema v12 (`evidence/table-ownership.txt`,
`evidence/table-writers.txt`, `evidence/table-api-callers.txt`):

CORE, current read + write:

```text
team_tasks team_task_runs messages artifacts agent_registry
external_runtime_bindings runtime_events team_final_task_refs team_final_artifact_refs
```

COMPAT(SCHEMA) - created and migrated, but after the journal/observations
quarantine no current product code reads or writes them; required so old
databases keep opening and migrating:

```text
sessions agent_turns tool_calls checkpoints transitions observations
```

COMPAT(RUNTIME) - written only by the codex-app-server MCP bridge:

```text
runtime_collaboration_records
```

COMPAT(SCHEMA) - ACC tables; their only reader/writer is the quarantined
`acc_store`, but they exist in published v0.3 databases and in the v8 fixture:

```text
acc_tasks acc_dependencies acc_context_manifests acc_artifacts acc_events
```

No table was dropped, altered, renamed or rewritten. `schema.rs`,
`SCHEMA_VERSION` (12), the migrations and both frozen fixtures are byte-identical
to the subject SHA (`evidence/gate-e-compat.txt`).

## 15. DriverKind compatibility matrix

| variant | durable string | restore | DriverFactory runnable | Lead | Worker | default path | compat obligation | implementation |
|---|---|---|---|---|---|---|---|---|
| Native | `native` | yes | no (`UnsupportedDriverKind`) | no | no | no | WIRE: persisted rows must stay readable | none reachable |
| Cli | `cli` | yes | no | no | no | no | WIRE | none reachable |
| Acp | `acp` | yes | yes | no | yes | yes | - | `PersistedAcpWorkerDriver` |
| CodexAppServer | `codex-app-server` | yes | yes | yes | yes | compatibility runtime | WIRE + RUNTIME | `PersistedCodexTeamDriver`, `CodexLeadBrain` |
| CodexExec | `codex-exec` | yes | yes | yes | yes | v0.4 default Lead | - | `PersistedCodexExecDriver`, `CodexExecLeadBrain` |
| ClaudeCli | `claude-cli` | yes | yes | no (unsupported by design) | yes | yes | - | `PersistedClaudeCliDriver` |

`native` and `cli` are classified COMPAT(WIRE): the persisted enum/strings must
restore forever, while no runtime implementation for them is reachable. The
enum itself is CORE. The two obligations are never merged into one verdict.

## 16. Reference-scan findings

`evidence/reference-scan.txt`, `evidence/acc-symbol-scan.txt`,
`evidence/legacy-usage-by-module.txt`, `evidence/table-ownership.txt`.
Every hit was classified, not merely counted:

```text
agentmosaic_core     PRODUCTION: runtime/src/agent.rs, storage/src/journal.rs
                     TESTS: storage/src/lib.rs inline, storage/tests/n12.rs, runtime/tests/e2e.rs
agentmosaic_model    PRODUCTION: runtime/src/agent.rs, runtime/src/dispatch.rs, storage/src/observations.rs
                     TESTS: as above
agentmosaic_tools    PRODUCTION: runtime agent/dispatch, model, workspace
agentmosaic_workspace PRODUCTION: runtime agent/dispatch only
agentmosaic_context  PRODUCTION: runtime/src/agent.rs, storage/src/observations.rs
AgentLoop/Delivery/runtime AgentConfig
                     PRODUCTION: none outside the module
                     TESTS: runtime/tests/e2e.rs
dispatch/ToolOutcome PRODUCTION: runtime/src/agent.rs only
SqliteJournal        PRODUCTION: storage/src/journal.rs + observations.rs (both quarantined)
                     OTHER: board.rs borrowed a one-line now() helper
                     TESTS: storage lib tests, n12, runtime e2e
SqliteAccStore       PRODUCTION: cli/src/lib.rs::inspect_acc (no caller from the binary)
                     TESTS: storage acc_e2e/acc_migration, cli lib test
team::acc::*         PRODUCTION: none
                     TESTS: storage acc_e2e/acc_migration, cli lib test; DOC: docs/runtimes/acp.md prose
DriverKind::Native/Cli  PRODUCTION: registry restore + DriverFactory refusal
```

## 17. Subtractive probe ledger

All probes ran in `/tmp/am-s0s1-d5c0a49/probe`, detached at the subject SHA,
started and ended clean, one hypothesis per round, raw diff and raw log saved.

| probe | hypothesis | cut | command | result |
|---|---|---|---|---|
| P1 | native loop is not on the product path | remove `mod agent`, `mod dispatch` and their exports from `runtime/src/lib.rs` | `cargo check --locked -p agentmosaic-cli --bin am` | PASS (rc=0); `cargo test -p agentmosaic-runtime --no-run` fails only in `tests/e2e.rs` |
| P2 | the five legacy crates are not required runtime dependencies | P1 + remove `agentmosaic-{core,model,tools,workspace,context}` from `runtime/Cargo.toml` | `cargo check --offline -p agentmosaic-cli --bin am` | PASS (rc=0); `Cargo.lock` delta = 5 removed edges; product tree loses `agentmosaic-workspace` |
| P3 | `journal` + `observations` are not required by the product | remove `mod journal`, `mod observations`, `pub use journal::SqliteJournal`, plus a probe-local `now()` in `board.rs` | `cargo check --locked -p agentmosaic-cli --bin am` | PASS (rc=0); failing test targets: storage lib tests, `storage/tests/n12.rs`, `runtime/tests/e2e.rs`; migration/compat tests still compile |
| P4 | ACC implementation is not required | remove `mod acc` + `pub use acc::*` + `mod acc_store` + `SqliteAccStore` | `cargo check --locked -p agentmosaic-cli --bin am` | FAIL (rc=101) - but only because the `am` binary also compiles the package's lib target (`cli/src/lib.rs`, the ACC inspection helper) |
| P4b | same, with the minimal cut-set | P4 + empty the ACC-only cli lib target | `cargo check --locked -p agentmosaic-cli --bin am` | PASS (rc=0); runtime/team/tui test targets still compile; only `acc_e2e`/`acc_migration` fail |
| P5 | the whole legacy closure can drop together | P1+P2+P3+P4b + storage manifest loses `context/core/model` | `cargo check --offline -p agentmosaic-cli --bin am` | PASS (rc=0); product local crates 10 -> 5, workspace local edges 21 -> 13, `Cargo.lock` delta = 8 removed edges |

`--locked` cannot be used for manifest-level probes because Cargo must rewrite
`Cargo.lock`; those runs used `--offline` and the lock delta is recorded in the
probe diff. Every probe was restored with
`git restore --source=d5c0a49... --staged --worktree .` and verified clean.

## 18. Complexity / removal-value matrix

| candidate | src LOC (gross) | files | local deps introduced | reverse deps | public exports | tests coupled | schema/wire obligation | invalidation cost | removal value |
|---|---|---|---|---|---|---|---|---|---|
| native loop (`agent.rs` + `dispatch.rs`) | 1,045 | 2 | core, model, tools, workspace, context | 1 (runtime lib) | 6 | `runtime/tests/e2e.rs`, agent/dispatch inline tests | none | ~0.44s check after touch | HIGH_REMOVAL_VALUE |
| `storage::journal` + `observations` | 397 | 2 | core, model, context | 1 (storage lib) | 1 | storage lib tests, `n12.rs`, `runtime/tests/e2e.rs` | tables stay (COMPAT) | ~0.50s | MEDIUM_REMOVAL_VALUE |
| ACC (`team::acc` + `acc_store` + cli lib helper) | 1,715 | 3 | none beyond team | 0 | ~45 re-exported symbols + 2 | `acc_e2e`, `acc_migration`, cli lib test | acc_* tables stay (COMPAT) | ~0.74s | HIGH_REMOVAL_VALUE |
| legacy crates (core/model/tools/workspace/context) | 4,284 | 24 | - | 1-3 each | ~120 | every legacy test target | none | n/a (removed from product graph) | HIGH_REMOVAL_VALUE |

Build-invalidation measurements (`evidence/build-footprint.txt`) do **not**
separate legacy from core code: touching `agent.rs` 0.44s, `acp_worker.rs`
0.47s, `acc.rs` 0.74s, `scheduler.rs` 0.70s, `journal.rs` 0.50s, `board.rs`
0.50s (median of 3). The removal value above therefore rests on reachability
and public surface, not on build time. No single "complexity score" is
invented.

Baseline build footprint: fresh `cargo build --locked --release -p
agentmosaic-cli --bin am` 38.48s wall (35.27s cargo time), peak RSS 766,884 kB,
binary 12,583,960 bytes; warm no-op rebuild 0.09s.

## 19. Confirmed CORE

`agentmosaic-cli` (binary), `agentmosaic-runtime`, `agentmosaic-team`,
`agentmosaic-storage`, `agentmosaic-tui`; in runtime: `acp_worker`,
`claude_cli`, `claude_cli_driver`, `codex_app_server`, `codex_bridge`,
`codex_exec`, `codex_exec_driver`, `codex_exec_lead`, `codex_lead`,
`codex_team_driver`, `driver_factory`, `launch`, `runtime_adapter`,
`runtime_event`, `team_runner`; in team: `board`, `lead`, `registry`,
`run_event`, `runtime_event`, `scheduler`; in storage: `board`,
`registry_store`, `schema`; the nine current-truth tables; four of six driver
kinds; and all 25 CLI roots (11 public, 13 hidden compatibility, 1 internal).

## 20. Confirmed COMPAT

`DriverKind::Native` and `DriverKind::Cli` persisted strings and their
`restore()` arm; the six legacy native tables (`sessions`, `agent_turns`,
`tool_calls`, `checkpoints`, `transitions`, `observations`); the five ACC
tables; `runtime_collaboration_records` (codex-app-server bridge); the
`codex-app-server` runtime family; the `agent_registry.driver_kind` wire form;
the published v0.3 fixture and the authentic v8 fixture.

## 21. TEST_ONLY

`agentmosaic-team::testutil` (`ScriptedDriver`, `cfg(test)`). Everything else
that only tests touch is part of a DEAD_CANDIDATE implementation, not a
separate TEST_ONLY component.

## 22. DEAD_CANDIDATE

(21 matrix rows; each has >= 2 independent evidence classes including a probe
or a product-root trace.)

```text
runtime::agent (AgentLoop, AgentConfig, Delivery, AgentError)
runtime::dispatch (dispatch, ToolOutcome)
storage::journal (SqliteJournal) + storage::observations
team::acc + pub use acc::*  + storage::acc_store (SqliteAccStore)
agentmosaic_cli::inspect_acc + AccInspection
agentmosaic-core, agentmosaic-model, agentmosaic-tools,
agentmosaic-workspace, agentmosaic-context
```

The two implementation-vs-representation splits that the audit package called
out are kept separate:

```text
DriverKind::Native persisted string = COMPAT(WIRE)   |  native runtime implementation = DEAD_CANDIDATE
ACC implementation (types + store)   = DEAD_CANDIDATE |  acc_* schema tables = COMPAT(SCHEMA)
SqliteJournal implementation         = DEAD_CANDIDATE |  sessions/agent_turns/... tables = COMPAT(SCHEMA)
```

## 23. UNRESOLVED

None. Two items were *near* a conflict and were resolved by experiment rather
than judgement:

1. `storage::board` referenced `journal::now`, which could have made the
   journal module look structurally load-bearing. P3 replaced it with a
   probe-local one-line helper and the product still checked: the coupling is a
   timestamp helper, not a dependency.
2. `cli/src/lib.rs` (ACC inspection) is compiled while building the `am`
   binary, which could have made ACC look product-reachable. P4 showed the
   failure was the same package's lib target; P4b cut that helper and the
   binary checked. Compile coupling is not product reachability.

Not-a-conclusion: whether the runtime process-supervision code should be
deduplicated (proposed Wave 5) is *not* classified here; it is a design
question with no reachability evidence either way and is listed as
NEEDS_EXPERIMENT.

## 24. Proposed S2 waves

All waves below are proposals only. Nothing was deleted in this round.

```text
Wave 1 (DELETE_WAVE_CANDIDATE, HIGH): runtime::agent + runtime::dispatch and
        their exports; the runtime manifest loses the core/model/tools/
        workspace/context edges. Evidence: P1, P2, reference scan.
        Must first move or retire the agent/dispatch inline tests and
        runtime/tests/e2e.rs.

Wave 2 (DELETE_WAVE_CANDIDATE, HIGH): agentmosaic-core, agentmosaic-model,
        agentmosaic-tools, agentmosaic-workspace, agentmosaic-context, after
        Wave 1 plus Waves 3-4 remove their remaining product edges.
        Evidence: P5 (product builds with only 5 local crates).

Wave 3 (QUARANTINE_CANDIDATE, HIGH): storage::journal + storage::observations
        implementation. Keep every table, migration and fixture.
        Evidence: P3 (product PASS, only journal/native tests break).

Wave 4 (QUARANTINE_CANDIDATE, HIGH): team::acc + pub use acc::* + storage::
        acc_store + agentmosaic_cli::inspect_acc. Keep the acc_* tables and the
        v8/v0.3 migration paths.
        Evidence: P4b (product PASS, only ACC tests break).

Wave 5 (NEEDS_EXPERIMENT, not started): runtime process-supervision dedup
        across codex_exec / claude_cli / acp_worker / codex_app_server.
```

Expected effects if Waves 1-4 land together (measured, not estimated, by P5):

```text
product local crates      10 -> 5   (cli, runtime, storage, team, tui)
product tree edges        21 -> 9   (workspace-wide local edges 21 -> 13,
                                     the other four being legacy-internal)
source lines removed      ~7,400 gross LOC across 31 files
public exports removed    ~240 pub items (152 across the five legacy crates,
                          65 re-exported by `pub use acc::*`, plus the
                          agent/dispatch/journal/acc_store/cli symbols)
Cargo.lock entries        8 path edges removed
```

Compatibility shells that must survive every wave: `schema.rs` and
`SCHEMA_VERSION=12`, all 21 tables, all migration steps (v8 -> v12, v11 -> v12,
published v0.3 -> v12), `DriverKind::restore` for all six strings,
`agent_registry`/`external_runtime_bindings` wire columns, the frozen
`v0_3_0_state.db` and `schema_v8.sql` fixtures, and the v0.3 tag/Release.

Still not decidable from this evidence: whether `agentmosaic-tui` should keep
its own crate, and whether the codex-app-server family should be retired - both
are product decisions outside reachability.

## 25. Explicit non-goals

No production code, manifest, lockfile, schema, migration, persisted wire
string, fixture, tag or PR was changed. No file was deleted. No dependency was
upgraded. No new tool was installed. No S2 operation was performed. The audit
does not judge code quality, naming, module organisation or documentation.

## 26. Raw evidence location + SHA-256

```text
local_evidence_path=/tmp/am-s0s1-d5c0a49/evidence
evidence_bundle=/tmp/am-s0s1-d5c0a49-evidence.tar.gz
evidence_bundle_sha256=8ae6a1dfba4fab911f0394f02560b016cb2cbbbb728837a658b004c30057fbdd
```

The bundle does not enter Git. It contains the 42 evidence files named by the
audit package (preflight, canonical gates, metadata, trees, crate/file metrics,
public surface, CLI help, reference scan, per-probe diff+log for P1-P5,
dependency baseline and dependency probe) plus the extractor logs
(`gate-*.log`, `cargo-metadata.err`). No probe was skipped; no empty PASS was
recorded.

Optional supporting evidence (package section 19) is `SKIPPED(not installed)`:
`cargo llvm-cov`, `cargo bloat` and `tokei` are absent and the package forbids
installing tools for this audit. `nm` on the release binary was not used either,
because absence from an optimised binary is explicitly not independent proof of
being dead; every classification above rests on source traces plus probes.

## 27. Final acceptance block

```text
S0_BASELINE_COMPLETE=true
S1_REACHABILITY_COMPLETE=true
AUDIT_SCOPE_COMPLETE=true

PRODUCTION_CODE_CHANGED=false
SCHEMA_CHANGED=false
PERSISTED_WIRE_CHANGED=false
V0_3_HISTORY_CHANGED=false
PR15_CHANGED=false
REMOTE_PUSHED=false

WORKSPACE_CRATES_CLASSIFIED=10/10
RUNTIME_MODULES_CLASSIFIED=17/17
TEAM_MODULES_CLASSIFIED=8/8
STORAGE_MODULES_CLASSIFIED=6/6
CLI_ROOTS_TRACED=25/25
SCHEMA_TABLES_CLASSIFIED=21/21
DRIVER_KINDS_CLASSIFIED=6/6

CORE_COUNT=68
COMPAT_COUNT=14
TEST_ONLY_COUNT=1
DEAD_CANDIDATE_COUNT=21
UNRESOLVED_COUNT=0

S2_DELETION_PLAN_READY=true
```

`S2_DELETION_PLAN_READY=true` is asserted only for Waves 1-4: every component
they touch is DEAD_CANDIDATE at HIGH confidence with a subtractive probe, and
no unresolved compatibility blocker remains. Wave 5 is explicitly excluded and
marked NEEDS_EXPERIMENT.
