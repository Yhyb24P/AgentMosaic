# ProcessSupervisor — evidence-based experiment design

Status: **design only, nothing implemented.** This document defines the
experiment that would have to pass before any shared process-supervision code is
written. It is deliberately its own line: context-tier work and jCodeMunch are
separate hypotheses and must not be mixed into this experiment or its PRs.

Base: canonical `main` at `adbd299` (v0.5 subtraction merged).

## 1. Question

Four runtime families each supervise an external process today. Do they share
enough *observable* contract that one supervision layer could own the shared
mechanics without weakening any family-specific guarantee?

Sub-questions, in the order they must be answered:

```text
Q1  How much of the supervision code is genuinely the same?
Q2  Which mechanics are shared, which merely look similar, and which are
    genuinely different contracts?
Q3  Can the shared mechanics move into one owner while every current
    deterministic test, the three process-tree regressions and the live probes
    stay green and semantically unchanged?
Q4  Only if Q1-Q3 pass: is the result measurably smaller and simpler, rather
    than the same complexity under a new name?
```

## 2. Explicit non-goals

```text
no behavior change (deadline scope, cancel semantics, reaping, error taxonomy)
no new runtime adapter, driver kind or wire string
no schema, migration or fixture change
no context-tier / Status-Summary-Trace redesign in this line
no jCodeMunch integration in this line
no replacement of the ACP SDK's own session/process lifecycle
no change to codex-app-server compatibility behavior
```

## 3. Current-state evidence (measured on `adbd299`)

Supervision surface sizes:

```text
crates/agentmosaic-runtime/src/codex_exec.rs        732 lines
crates/agentmosaic-runtime/src/claude_cli.rs        656 lines
crates/agentmosaic-runtime/src/codex_app_server.rs  802 lines
crates/agentmosaic-runtime/src/acp_worker.rs       2241 lines
                                             total 4431
```

Duplication measurement (the two JSONL supervisors, `run_invocation` bodies):

```text
codex_exec::run_invocation       139 lines
claude_cli::run_invocation       136 lines
textual similarity                0.785
identity-normalized similarity    0.895
identical matching lines            108
```

Cross-module reuse that already exists: `claude_cli` calls
`codex_exec::bounded_stderr` (1 site) and `codex_exec::terminate_group`
(2 sites). The crate therefore already treats the spawn/pipe/kill skeleton as
shared mechanics; it just keeps two copies of it.

Mechanisms per family:

| Mechanic | Codex exec | Claude CLI | Codex app-server | ACP |
|---|---|---|---|---|
| own process group | yes (`process_group(0)`) | yes | yes | delegated to the ACP SDK |
| group termination | `terminate_group` on write failure/error | same helper | `killpg` from `Drop` | SDK lifecycle |
| budget scope | one execution budget | one execution budget | one budget per request plus an event bound | one budget across handshake, binding and turn |
| stdout pumping | line reader + `sync_channel(256)` | same shape | line reader + bounded queue | SDK update stream |
| stderr bound | 16 KiB helper | same helper | same helper | SDK stderr |
| stdin transport | prompt on stdin | prompt on stdin | JSON-RPC requests | SDK session |
| peer-level cancel | none | none | none | peer-confirmed `cancelled` |
| cancel fallback | deadline + group kill | deadline + group kill | request deadline + `Drop` reap | deadline + peer confirmation |
| error type | `RuntimeError` | `RuntimeError` | `CodexBridgeError` | `AcpWorkerError` |
| normalization | `normalize_event` | `normalize_stream_event` | bridge events | `map_acp_update` |
| durable events | `RuntimeEventDispatcher` | same | same | same |

Test coverage that any experiment must keep green:

```text
acp_m2_lifecycle.rs      21 tests (deadline, slow-drip, cancel boundary, group+grandchild reap, resume gating)
codex_exec_driver.rs      1 test   (binding + normalized events + artifact)
codex_exec_lead.rs        4 tests  (strict decision contract, repair, resume)
codex_bridge_driver.rs    2 tests  (app-server compatibility driver)
codex_bridge_queue.rs     2 tests  (queue replay, silent-peer deadline)
claude_cli_driver.rs      2 tests  (worker binding/events + live probe)
codex_live.rs             5 tests  (ignored live probes; the real-runtime evidence)
inline unit tests: codex_exec 8, claude_cli 8, codex_app_server 12, acp_worker 23
```

Three of those tests exist specifically because supervision once leaked:
`timeout_reaps_the_acp_wrapper_process_group_and_its_grandchild`,
`deadline_reaps_the_codex_exec_process_group_and_its_grandchild` and
`deadline_reaps_the_claude_process_group_and_its_grandchild`. They are the
behavioral floor for this experiment.

## 4. Hypotheses

```text
H1 (shared skeleton)
    The spawn/pipe/deadline/kill skeleton of codex_exec and claude_cli can be
    owned once without changing any observable behavior of either family.
    Falsified by: any change in the deadline, reaping, error-mapping or event
    behavior those families' deterministic tests assert.

H2 (similarity is real, not cosmetic)
    The measured 0.785/0.895 similarity is mechanical, not a coincidence of
    formatting: after extraction the duplicated supervision lines drop by at
    least 60% while each family keeps its own parsing and normalization.
    Falsified by: extraction that needs more new adapter code than it removes.

H3 (divergence is contractual, not accidental)
    The per-family differences in the table above are real contracts, so a
    shared layer must parameterize them rather than flatten them. The four
    contractual differences are budget scope, cancel semantics, reaping owner
    and error taxonomy.
    Falsified by: a difference that turns out to be accidental and whose removal
    no test or live probe notices.

H4 (no complexity transfer)
    Any accepted change is deletion-dominant: no new crate, no new public trait
    unless the evidence proves one is required, and
    (added supervision code - removed duplicated code) is negative.
    Falsified by: an abstraction that renames the four implementations instead
    of removing three copies of the shared part.
```

## 5. Metrics (definitions fixed before measuring)

```text
M1 duplicated_supervision_lines = lines shared by >=2 families, measured by
   identity-normalized diff of the supervision regions
M2 per_family_contract_tests_green = the test list in section 3, all green, with
   no #[ignore] and no new allow
M3 process_tree_regressions = the three group+grandchild reap tests, green
M4 live_probe_parity = the ignored live probes for ACP (qwen/kimi/opencode) and
   Claude, run once before and once after, same conclusions
M5 error_mapping_parity = for each family the same failure inputs produce the
   same RuntimeError variant as before the experiment
M6 net_deletion = added production lines minus removed production lines
```

Every metric is measured on the same machine, toolchain and target dir, before
and after. No claim about speed or binary size is derived from these numbers.

## 6. Experiment sequence

Each step is one branch, one PR, one hypothesis. A step that fails its gate is
reported and abandoned; it is never "fixed" by loosening a test.

```text
E0  measurement only (the numbers in this document, re-run and recorded)
    cut: none
    negative control: none needed
    gate: numbers reproducible from a clean checkout

E1  extract the shared JSONL supervision skeleton used by exactly two families
    cut: codex_exec + claude_cli only; app-server and ACP untouched
    cheapest distinguishing test: the two families' inline tests plus the
      codex_exec_driver, codex_exec_lead and claude_cli_driver targets
    negative control: remove the extracted helper and confirm the old path is
      still needed, i.e. the helper is not dead weight
    gate: M2 + M3 + M5 green, M1 down >=60% for those two families, M6 negative
    rollback: revert the single PR

E2  decide app-server by evidence, not by symmetry
    cut: candidate only if E1 proves the shared layer can express a per-request
      budget and a Drop-owned reaper without weakening either
    cheapest distinguishing test: codex_bridge_driver, codex_bridge_queue and
      the real app-server live probe
    negative control: a deliberately wrong shared implementation (one
      execution-wide budget) must fail the bridge deadline test
    gate: as E1, plus no change to the codex-app-server compatibility contract in
      AGENTS.md or docs/runtimes/codex.md

E3  ACP: keep the SDK lifecycle, share only the surrounding mechanics
    cut: never replace the ACP SDK's session/process ownership; align
      deadline/event/error plumbing only if E1 shows a real shared shape
    cheapest distinguishing test: acp_m2_lifecycle (21 tests), the
      group+grandchild regression and a real peer-confirmed cancel probe
    negative control: peer-confirmed cancel must still fail closed when the peer
      does not confirm
    gate: M2 + M3 + M4 green, and no change to cancel semantics

E4  only if E1-E3 pass: measure the result (M1-M6) and decide
    gate: net deletion and no contract drift; otherwise stop and keep the
      family-local supervisors
```

## 7. Decision rules

```text
PROCEED to implementation only when:
  - the E1 gate is green, and
  - the shared shape expresses every contractual difference in H3 without
    flattening it, and
  - at least two of the four families are fully covered by the shared layer
    with M6 negative.

STOP (keep the current four supervisors) when:
  - extraction requires a new public trait or crate to be useful, or
  - any family-specific contract must be weakened to fit, or
  - the M1 reduction is achieved mainly by moving code rather than removing it.

This document does not authorize implementation. E0/E1 start only after the
documentation-hygiene line and the review of this design are complete.
```

## 8. Risks

| Risk | Why it matters | Mitigation in the experiment |
|---|---|---|
| deadline scope drift | app-server uses a per-request budget, the JSONL families one execution budget | E2 keeps the app-server budget explicitly parameterized and uses its deadline test as the negative control |
| cancel semantics drift | ACP is peer-confirmed; the others are deadline plus kill | E3 forbids replacing the SDK cancel path; the cancel tests are hard gates |
| reaping regression | the three grandchild tests exist because reaping once leaked | M3 is a hard gate at every step |
| error-mapping drift | three error types map into one `RuntimeError` taxonomy | M5 compares the produced variant per failure input |
| abstraction by rename | a shared layer can hide the same complexity | H4 and M6 make negative net deletion an explicit failure |
| live-probe variance | the live ACP/Claude probes depend on local runtimes and isolated runtime configuration | run them before and after with the same isolated configuration; treat a difference as a blocker, not a flake |

## 9. Evidence storage

Raw before/after measurements, per-step diffs and logs go to a `/tmp` evidence
directory for each step and are summarised in the PR that performs the step.
This document, not code, is the artifact of the current line.
