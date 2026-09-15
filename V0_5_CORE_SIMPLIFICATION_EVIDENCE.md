# AgentMosaic v0.5 Core Simplification — S2 Subtraction Evidence

Every entry is an actual result: command output, a remote workflow conclusion,
a SQLite read-back or a file hash. Nothing is promoted from source inspection
alone. Causal claims about build/runtime speed are deliberately absent.

## Identity

```text
canonical_v04_main=1a4a71a85b615ec0a076b965d8b49259aae9c1a1 (PR #15, merge commit)
merge_method=merge commit (no squash, no rebase); 94 commits of v0.4 history preserved
s2_branch=refactor/v0.5-core-simplification
s2_start_sha=0fa169ac055ed0e2b83ac01ca840be6f407b59b0 (main + cherry-picked ea3b887)
audit_subject=d5c0a4958af29a56642a84fd0ca615d64785e504
schema_version=12
```

## Precondition

```text
v04_in_main=yes  (git merge-base --is-ancestor d5c0a49 origin/main -> 0)
worktree_clean=yes
audit_target_drift=none (post-merge tree == audit-subject tree; S2 target paths identical)
audit_evidence_preserved=ea3b887 cherry-picked as 0fa169a; package reference files are byte-identical
                         (audit md fd09aaf5…, matrix 7f3df0d1…)
```

## Protected hashes

```text
schema_rs_before=7f16be89a16366615f38aac461eee3567f84afb97e44e92dbd2e682efbcaaad0
schema_rs_after =7f16be89a16366615f38aac461eee3567f84afb97e44e92dbd2e682efbcaaad0
v03_fixture_before=e7b430ad17b8c2be3704f540ca921c77b2ba063c9661300411fbda51334adc3b
v03_fixture_after =e7b430ad17b8c2be3704f540ca921c77b2ba063c9661300411fbda51334adc3b
v8_fixture_before=e338c0ae4d76ea9b94507a50cc9a61bcc30ecd609132b16c8edea4fc76efb9bb
v8_fixture_after =e338c0ae4d76ea9b94507a50cc9a61bcc30ecd609132b16c8edea4fc76efb9bb
v0.3.0_tag=0560f388c976a2a1318fd7c923e14e410d04997c -> fb8cc9080584ed2687576fba406a4cff6dbbce2c (unchanged)
```

## Baseline (P0.5, measured on the S2 start)

```text
workspace_crates=10
local_edges=21
gross_src_loc=30323
gross_src_bytes=1121222
integration_test_loc=13016
cargo_packages=288
cargo_lock_packages=288
public_surface=see docs/audits/S0_S1_REACHABILITY_MATRIX.csv (104 rows)
release_binary_bytes=12583960
tests=482 passed / 0 failed / 24 ignored
canonical_gates=identity, fmt, clippy, tests, release, diff-check all PASS (--locked)
```

## Wave A — retired native AgentLoop / dispatch

```text
hypothesis      no supported product root uses the AM-owned model/tool loop
evidence        S0/S1 probe P1; audit sections 11/17/22
deleted         runtime/src/agent.rs, runtime/src/dispatch.rs, runtime/tests/e2e.rs,
                the mod/exports in runtime/src/lib.rs, and the five runtime dependencies
                that existed only for them (core/model/tools/workspace/context)
lock_delta      5 path edges removed from agentmosaic-runtime
expected_test_changes=8 retired-feature tests removed, 0 added (5 agent + 2 dispatch inline, 1 native e2e)
negative_control=DriverKind round-trip (all six strings), driver_factory units, and the
                acp_m2_lifecycle / codex_exec_lead / codex_bridge_driver / claude_cli_driver
                targets all pass; schema untouched
targeted_gate   check bin am PASS; runtime tests PASS; cli tests PASS
full_gate       identity/fmt/clippy/tests(474)/release/diff-check PASS
commit          refactor(runtime): remove retired native agent loop (3df15c6)
```

## Wave B — retired native journal / observations

```text
hypothesis      the product persists through the team board, registry and schema, not the
                per-session native journal
evidence        S0/S1 probe P3; audit sections 13/17/22
deleted         storage/src/journal.rs, storage/src/observations.rs, storage/tests/n12.rs,
                the SqliteJournal export, and the context/core/model storage dependencies
helper_move     board.rs now owns the one-line timestamp helper it used to borrow
lock_delta      3 path edges removed from agentmosaic-storage
compat_tests_rewritten=r3_database_migrates_preserving_data now drives the real migration and
                asserts representation through SQL (no session implementation)
schema_unchanged=yes (hash identical; the six legacy tables still created and migrated)
expected_test_changes=6 retired-feature tests removed, 0 added
negative_control=authentic v8 fixture, published v0.3 fixture and fresh v12 all migrate;
                schema_v8_migration now also asserts every historical native table survives
full_gate       identity/fmt/clippy/tests(468)/release/diff-check PASS
commit          refactor(storage): retire native journal implementation (0bf1243)
```

## P6 / Wave C — five legacy crates

```text
probe_worktree  /tmp/am-v05-s2/probe-p6 detached at 0bf1243
probe_result    PASS: metadata, check --workspace --all-targets --all-features,
                test --workspace --all-features (379 passed / 0 failed / 24 ignored),
                build --release --workspace and git diff --check all rc=0; --locked check
                and test also rc=0
probe_deltas    crates 10 -> 5, local edges 21 -> 9 (audited expectation), packages 288 -> 225,
                binary 12583960 -> 12287432 bytes
applied         workspace members + five crate directories removed; active docs that had
                become false corrected (AGENTS.md crate list, docs/architecture.md); audit
                material in docs/audits left untouched
residual_refs   none outside docs/audits (checked repo-wide)
expected_test_changes=89 crate-local tests removed, 0 added
full_gate       identity/fmt/clippy/tests(379)/release/diff-check PASS
commit          refactor(workspace): remove retired native-agent crates (adfc0e7)
```

## Wave D — ACC implementation

```text
hypothesis      no current product root reads or writes ACC
evidence        S0/S1 probe P4b; audit sections 12/13/17/22
deleted         team/src/acc.rs and its glob re-export, storage/src/acc_store.rs and its
                export, the dead cli library target (ACC inspection helper), acc_e2e.rs
lock_delta      none (no external dependency was reachable only through ACC)
compat_tests_rewritten=acc_migration.rs drives the real pre-ACC upgrade and asserts the
                preserved rows and acc_* tables through SQLite, not through SqliteAccStore
                or the graph API
hard_invariant  schema.rs byte-identical; acc_* tables untouched
expected_test_changes=11 retired-feature tests removed, 1 added (the rewritten representation test)
negative_control=published v0.3 + authentic v8 migration PASS, all six DriverKind strings
                restore, and the am binary ran init/agent add/list/advanced/doctor on a fresh
                project (team ready)
full_gate       identity/fmt/clippy/tests(369)/release/diff-check PASS
commit          refactor(compat): retire ACC implementation surface (488ea97)
```

## Final measurement (H)

```text
workspace_crates        10 -> 5    (cli, runtime, storage, team, tui)
local_normal_edges      21 -> 9
cargo_packages          288 -> 225
cargo_lock_packages     288 -> 225
tracked_src_files       78 -> 47
gross_src_loc           30323 -> 22796   (-7527)
gross_src_bytes         1121222 -> 860885 (-260337)
integration_test_loc    13016 -> 11991   (-1025)
release_binary_bytes    12583960 -> 12214664
tests                   482 -> 369 passed, 0 failed, 24 ignored (live cases unchanged)
```

The audit's estimate was ~-7.4k gross src LOC; the measured value is -7,527, and
every removed line belongs to a component classified DEAD_CANDIDATE with probe
evidence. Binary size and package count are observations: no causal
build/runtime performance claim is made. No new crate, public abstraction,
feature flag or compatibility wrapper was added; the only new production code
is the one-line timestamp helper moved into the storage board that owns it.

## Test ledger

Every removed or rewritten test target is listed with its old and remaining
contract in `docs/audits/S2_TEST_LEDGER.csv`: the retired-feature tests of
waves A (8), B (6) and D (11), the 89 crate-local tests of the five crates
removed in wave C, and the two representation-level rewrites. No `#[ignore]`,
broad `allow` or legacy feature flag was used to reach a green suite.

## Real heterogeneous E2E (F)

CodexExec Lead + Qwen ACP + Kimi ACP + OpenCode ACP on the final S2 code:

```text
root=1 lead=codex-exec (foreign thread persisted)   children=2,3,4 all succeeded
qwen-worker (2)   -> qwen-result.txt     sha256 8da41ead27fa6d0ad9abec8ac22326698fcc368e5c4a2fd64fe7bdd1c8955e6b
kimi-worker (3)   -> kimi-result.txt     sha256 cdce3f0a3dc212e9d334abacd6a3ec19f23397a5a6dc9b74c989d113ae48228f
opencode-worker(4)-> opencode-result.txt sha256 1356d0b1e4b1512143eb037c95b0136bde9a570cbd4a9e0ef762ec7129a0f6c9
runtime bindings  qwen-code 0.23.4 acp/1, Kimi Code CLI 0.43.1 acp/1, OpenCode 1.18.31 acp/1
runtime events    9 / 4 / 4 durable observations (visible kinds only)
lead synthesis    "qwen-result.txt, kimi-result.txt, opencode-result.txt"
restart           fresh am status/final/artifact/events reconstruct run, refs and digests
no_replay         resume-team on the succeeded root reproduced the answer with byte-identical
                  board state; artifact files unchanged; events unique (9/9, no duplicates)
manual_copy_paste=false; no secrets persisted (isolated KIMI_CODE_HOME and isolated
                  OpenCode install under /tmp; the temporary kimi config copy was deleted)
```

## Compatibility (D)

```text
SCHEMA_VERSION=12 unchanged; all 21 tables still created and migrated
v8 -> v12 PASS; published-v0.3 -> v12 PASS; fresh v12 PASS; pre-ACC v4 -> v12 PASS
all six DriverKind durable strings restore (native, acp, cli, codex-app-server, codex-exec, claude-cli)
codex-app-server compatibility runtime and the internal am __internal codex-mcp bridge unchanged
frozen fixture hashes unchanged; v0.3 tag/Release untouched
```

## Canonical quality (G)

```text
identity / fmt / clippy / test / release build / git diff --check: all PASS on the final head
remote Draft PR workflows: all green on the exact verified head (below)
```

## Remote CI (I)

```text
pr=#16 (Draft, base main, head refactor/v0.5-core-simplification)
verified_head=c0e97792b4f8d63762347dd678f9c175a1cfbb6f
rust          run 35011874403  success  c0e97792b4f8d63762347dd678f9c175a1cfbb6f
rust-quality  run 35011874461  success  c0e97792b4f8d63762347dd678f9c175a1cfbb6f
Release/plan  run 35011874359  success  c0e97792b4f8d63762347dd678f9c175a1cfbb6f
PR_MERGED=false ; V0_5_RELEASE_CREATED=false
```

`verified_head` is the commit that carried the code, the audit documents and
the S2 evidence. The commit that adds this CI block changes documentation only;
the same three workflows were required to run green on it as well, and no code
changed after `verified_head`.

## Final status

```text
V0_4_BEHAVIOR_PRESERVED=true
S2_SUBTRACTION_COMPLETE=true
CORE_SIMPLIFICATION_READY=true
PR_IMPLEMENTATION_READY=true

PR_MERGED=false
V0_5_RELEASE_CREATED=false
```
