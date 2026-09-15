# ProcessSupervisor E1 — Codex Exec + Claude CLI JSONL supervision

Authorization for this line, as given:

```text
E0_AUTHORIZED=true ; E1_AUTHORIZED=true
E2_AUTHORIZED=false ; E3_AUTHORIZED=false ; E4_AUTHORIZED=false
```

```text
branch      experiments/process-supervisor-e1
base        main@565657c6d62ad5c697b5675e65d577e9051927ef
scope       codex_exec.rs, claude_cli.rs, their characterization tests,
            a crate-private process-mechanics helper, E1 evidence
design      docs/experiments/process-supervisor.md
```

## Result first

```text
E1_PROCEED=false
KEEP_FAMILY_LOCAL_SUPERVISORS=true
```

The shared skeleton is real and can be extracted without changing any observed
behavior — the four behavioral controls, the deterministic suites and both real
probes say so. It still fails the frozen cost gate: the extraction removes 63%
of the duplicated mechanics but **adds** net production lines (+61 counting all
lines, +6 counting code lines only), because a boundary-compliant shared
primitive has to carry a typed failure surface plus per-family failure wording
to keep vendor wording out of the shared layer. Per the design, that is a
legitimate experimental outcome and the extraction was reverted, not landed.

## E0 baseline (measured on `565657c`)

```text
codex_exec.rs                          732 lines
claude_cli.rs                          656 lines
codex_exec::run_invocation body        139 lines
claude_cli::run_invocation body        136 lines
textual similarity                       0.785
identity-normalized similarity           0.895
identity-normalized identical lines      108   <- M1 baseline
```

Existing cross-module reuse: `claude_cli` already called
`codex_exec::bounded_stderr` (1 site) and `codex_exec::terminate_group`
(2 sites). `codex_app_server.rs` keeps its own private `bounded_stderr`; that
is E2 territory and was not touched.

## Behavioral negative controls (run BEFORE extraction, one at a time)

Each control was applied alone, run, and reverted; the tree was verified clean
after every round.

| Control | Injected break | Test | Required outcome | Observed |
|---|---|---|---|---|
| NC1 codex | `terminate_group` → direct-child `kill()` | `deadline_reaps_the_codex_exec_process_group_and_its_grandchild` | FAIL | **FAILED** — `Codex exec timeout left grandchild 1733874 alive` |
| NC1 claude | `terminate_group` → direct-child `kill()` | `deadline_reaps_the_claude_process_group_and_its_grandchild` | FAIL | **FAILED** — `Claude timeout left grandchild 1734830 alive` |
| NC2 codex | deadline resets on every received line | `streamed_output_does_not_extend_the_codex_exec_deadline` | FAIL | **FAILED** |
| NC2 claude | deadline resets on every received line | `streamed_output_does_not_extend_the_claude_deadline` | FAIL | **FAILED** |

Both families therefore have characterization tests that can actually detect a
broken process-tree or absolute-deadline contract, which was the precondition
for attempting extraction. (No new characterization test was needed: both
families already had a grandchild-reap regression and a streamed-output
deadline regression from the v0.4 reliability work.)

## The extraction that was attempted

One new crate-private module (`process_mechanics.rs`) owning exactly the
allowed mechanics — spawn with its own process group and piped stdio, stdin
delivery, stdout line pump, bounded stderr, one absolute deadline, process-group
termination, wait/reap — plus a `LineProcessFailure` value type that carries only
mechanics-level facts. Each family kept its own decode, normalization, state
(thread/session id, final message), error wording and diagnostic attachment.

Measured with the extraction in place:

```text
run_invocation body LOC        139/136  ->  51/51
identity-normalized identical   108     ->  40    => M1 reduction 63.0%  (gate >=60% PASS)
normalized similarity            0.895   ->  0.784
new crate-private module        196 lines (148 code lines)
codex_exec.rs                  -81 code lines, claude_cli.rs -61 code lines
M6 all lines                   +61   (gate <0  FAIL)
M6 code lines only             +6    (gate <0  FAIL)
new crate                      0     (PASS)
new public API                 0     (PASS: the module is crate-private and not re-exported)
vendor semantics in the helper 0     (PASS: no vendor name, RuntimeEvent, id, payload
                                      shape, normalization, final-message, resume or
                                      permission knowledge inside the module)
```

## Gate evaluation

| Gate | Result |
|---|---|
| CodexExec deterministic parity | PASS — `cargo test -p agentmosaic-runtime --all-features`: 138 passed / 0 failed / 22 ignored, including all four characterization tests |
| Claude deterministic parity | PASS — same run |
| process-tree regressions | PASS — both grandchild reap regressions green under the extraction (and proven meaningful by NC1) |
| absolute-deadline regressions | PASS — both streamed-output deadline regressions green (and proven meaningful by NC2) |
| error/diagnostic parity (M5) | **PARTIAL.** Verified: `RuntimeError` class parity, `TimedOut`-vs-`Protocol` parity, and the family mapping/attachment rule preserved verbatim. Not independently characterized: stderr *attachment* behavior has no dedicated test |
| Codex real probe parity (M4-E1) | PASS — real `codex exec` turn through the driver: ok before (`565657c`) and ok after the extraction (thread binding persisted, `session_started` event, bounded result) |
| Claude real probe parity (M4-E1) | PASS — real Claude turn: ok before and ok after |
| M1 duplicated mechanics reduction >= 60% | PASS — 63.0% |
| M6 net production LOC < 0 | **FAIL — +61 all lines, +6 code only** |
| new crate = 0 | PASS |
| new public API = 0 | PASS |
| shared vendor semantics = 0 | PASS |

One gate fails, so:

```text
E1_PROCEED=false
KEEP_FAMILY_LOCAL_SUPERVISORS=true
```

## Why it fails, and what that tells us

The two supervisors share ~140 lines of skeleton each. A *correct* shared
primitive cannot simply absorb those lines and stop: it must return structured
failures (the shared layer may not write vendor wording, and it may not assume
what a line means), so each family needs a small failure-mapping function and a
diagnostic-attachment rule. The module plus that surface is larger than the one
copy it removes. The duplication is real, but it is cheaper than the
boundary-compliant abstraction that would remove it.

Stated precisely, so the record is not read as a general impossibility result:

```text
The tested boundary-compliant extraction costs more than the duplication it
removes; under the frozen E1 contract there is no evidence to replace the
family-local supervisors.
```

## What landed, and what did not

Landed: one ignored live test,
`codex_exec_driver::live_exec_turn_binds_a_thread_and_returns_a_bounded_result`.
The Codex Exec family had no real-runtime probe while Claude did; E1 needed one
for M4-E1, and it is a genuine gap in the characterization surface. It runs only
with `--ignored`, so CI is unaffected.

Not landed: the extraction. The attempted diff is archived as evidence
(`e1-extraction-attempt.diff`) and `codex_exec.rs`, `claude_cli.rs` and
`lib.rs` are byte-identical to `565657c`.

Not attempted: E2 (codex app-server), E3 (ACP), E4 — all unauthorized.

## Follow-up hypotheses (NOT authorized; each would need its own E1'-style gate)

```text
1. Share a strictly smaller primitive: child ownership, piped stdio, line pump
   and reaping only, leaving the deadline loop and error wording family-local.
   Open question: does that still reach >=60% M1 reduction, or does the deadline
   loop stay duplicated and keep M1 under the threshold?
2. Share the failure surface but let one neutral wording table serve both
   families (no vendor name). Open question: is changing the user-visible
   diagnostics acceptable, given M5 only requires class parity?
3. Leave both families as they are, and spend the next experiment on a different
   hypothesis entirely.
```

The decision taken on this review is **3**, and the line is closed:

```text
PROCESS_SUPERVISOR_LINE_CLOSED=true
```

A smaller primitive would keep most of the deadline-loop duplication (plausibly
trading a passing M1 for a better M6), neutral wording would change
user-visible diagnostics for the sake of the abstraction, app-server adds a
per-request budget and a Drop-owned lifecycle, and ACP's lifecycle and
peer-confirmed cancel belong to the SDK. The line reopens only if a new external
runtime makes the same skeleton three or more copies, which would change the
economics.

## Local raw evidence (ephemeral; not part of the repository)

```text
raw evidence dir              /tmp/am-ps-e1
E0 baseline                   e0-baseline.txt
negative controls             nc1-codex.log, nc1-claude.log, nc2-codex.log, nc2-claude.log
real probes (before/after)    probe-codex-before.log, probe-codex-after.log,
                              probe-claude-before.log, probe-claude-after.log
M1 / M6                       m1-m6-after.txt, m6-exact.txt
attempted extraction diff     e1-extraction-attempt.diff (771 lines, reverted)
final state gates             e1-gates.txt (identity, fmt, clippy, workspace tests
                              369 passed / 0 failed / 25 ignored, release build, diff-check)
```

These files are local scratch evidence, not a durable project artifact. The
merged repository retains the summarized measurements and conclusions above; a
machine that no longer has the directory loses the raw logs, not the result.

## Status

```text
E0_COMPLETE=true
E1_COMPLETE=true
E1_PROCEED=false
KEEP_FAMILY_LOCAL_SUPERVISORS=true
PROCESS_SUPERVISOR_LINE_CLOSED=true
E2_AUTHORIZED=false
E3_AUTHORIZED=false
E4_AUTHORIZED=false
```
