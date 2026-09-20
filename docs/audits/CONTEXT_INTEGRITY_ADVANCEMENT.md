# Lead context integrity and durable recovery

Date: 2026-09-20. Source candidate: `1855507c91f04b656f140a213080f8648fdf01a2`.
Baseline: main `8088252` plus the historical S1 audit `bbf5efa`.
Branch: `fix/lead-context-integrity`. Development version stays `0.5.0-dev`;
SQLite stays v12 and the Lead decision wire is unchanged.

## Findings and implemented changes

1. Whole-document UTF-8 prefix truncation could hand malformed JSON to either
   Lead backend. The shared renderer now serializes complete JSON, searches a
   64..4096-byte per-text cap using actual encoded size, and marks truncated
   prose. Complete candidate IDs, task IDs, artifact paths and digests survive.
   If even minimum excerpts plus complete references exceed the budget, it
   returns a capacity error before starting the Lead turn. It never silently
   drops rows or abbreviates an identity. The minimum is a source-text cap,
   not a claim that 64 source bytes occupy 64 JSON bytes.
2. Artifact context previously discarded the owning task ID. `LeadContext`
   now carries the existing `SelectedArtifactRef` type, and renders
   `task_id/path/sha256`. This is an in-memory context change, with no migration
   or change to stored artifact/final-reference semantics.
3. The S1 report misclassified long result summaries as reachable only through
   advanced commands. Its own 31-result case fits within the public 32-task
   limit. That classification is corrected; original measurements remain
   explicitly historical.
4. A real interrupted-run experiment exposed a round-counter defect:
   `complete` was rejected at local round zero even when resumed context had a
   succeeded descendant. Completion now relies on the existing authoritative
   board checks (succeeded descendant, non-root, exact artifact membership).
5. A real Lead reacting to an injected worker failure correctly asked for a
   follow-up, but the loop rejected it because there were no *successful*
   results. Either a success or a failure can now ground a follow-up. An empty
   context still cannot, and final completion still needs a successful task.
6. Resuming a failed Codex Exec root left its Lead attempt failed, so binding
   restoration refused to start. `TeamRunner::resume` now marks the existing
   Lead attempt and root running before driving the brain. This preserves the
   existing attempt-1 settlement convention and its foreign binding. Successful
   roots still return before this code and remain byte-for-byte unchanged.

## Deterministic evidence

All required local gates passed on the source candidate:

```text
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features --quiet
cargo build --locked --release --workspace
scripts/ci/check_identity.sh
git diff --check

378 passed / 0 failed / 25 ignored
```

The 25 ignored cases require external runtimes or credentials. This count is
not a count of outstanding defects; the selected live experiments below are
reported separately, not silently promoted to a run of all ignored tests.

| Requirement | Checked-in evidence |
|---|---|
| 8/31/32 long results; quotes, escapes, Chinese and emoji | `codex_lead::tests::default_task_budget_preserves_json_and_all_result_ids` |
| Complete long paths, distinct digests and task owners under pressure | `codex_lead::tests::artifact_identity_and_ownership_survive_text_reduction` |
| Capacity failure before starting a Lead | `codex_lead::tests::excessive_metadata_fails_before_spawning_a_lead` |
| Public 33,000-character agent ID; both Lead adapters; root settles failed | CLI `project_onboarding::oversized_public_agent_id_fails_without_launching_either_lead_runtime` |
| Public `am run`, 32 real scheduled tasks, escaped summaries, all artifact owners, fresh-process inspection | CLI `context_integrity::default_run_delivers_32_escaped_results_and_exact_artifact_owners` (deterministic external JSONL peers) |
| Board artifact ownership reaches the actual external Lead prompt | `team_runner_product::one_objective_becomes_a_durable_team_result` |
| Immediate completion after interrupted work without new tasks/attempts | `lead::tests::resumed_root_can_complete_immediately_without_new_tasks_or_attempts` |
| Failed-only context permits recovery; empty context still refuses it | `lead::tests::failure_only_context_can_drive_a_successful_follow_up`, `follow_up_without_any_terminal_evidence_is_rejected` |
| Failed Exec root is running before binding and completes existing work | `team_runner_product::failed_exec_lead_resume_marks_root_running_and_completes_existing_work` |
| Current and older boards remain readable | Workspace migration suites, including authentic v8 and frozen published-v0.3 fixtures |

Negative controls: temporarily restoring whole-JSON slicing made the long-result
regression fail with `EOF while parsing a string` at column 32554. That mutation
was reverted before the gates above. The failed-Exec-resume product regression
was run before its fix and failed with `root Lead attempt is not running before
Codex exec resume`; it passed afterwards. Live observations below independently
exposed the two Lead-loop gates.

## Live experiments

Scratch root: `/tmp/am-context-research.WoQ79X`. No credentials or runtime
databases are checked into the repository. Codex CLI was `0.155.1`; Qwen was
`0.23.4`. A transparent Python launcher recorded the synthetic Lead input and
the runtime's visible decision/usage events while forwarding stdin/stdout.
It did not make orchestration decisions or write task-board rows.

### Review, fix and verify

Real Codex Exec Lead plus real Qwen ACP reviewer and fixer, public `am run`:

```text
root 1 succeeded
task 2 reviewer succeeded: observed all three pricing tests failing
task 3 fixer succeeded: changed addition to multiplication; tests passed
task 4 reviewer succeeded: reran unchanged tests; all three passed
final selected tasks: 3, 4
elapsed: 277.614 seconds
```

The reviewer owns `review.txt`; the fixer owns `pricing.py`. The final selection
named the latest reviewer task (4), not its earlier task (2), even though both
recorded the same path at different digests. Independent `python3 -m unittest -v`
also passed all three tests. `test_pricing.py` remains the seeded test fixture.

```text
pricing.py selected SHA256
  e5ced3fa3ea8abd8b0566c378552ae733860ac9cb6387dcc7937eace7aebaa82
review.txt selected SHA256 (task 4)
  7743b8ed2e9568bb8b6020a16c930070c9e263a3ea414c906d6e48d6f849fb49
test_pricing.py SHA256
  677607adfe57a5fe70dbf651ea20b28df13059947676d3b5b257f056356f211f
```

### Interruption and failed-only follow-up

In the interruption experiment a real Qwen worker completed `receipt.txt`.
The launcher paused before the next real Lead process, after the child was
durably succeeded. The test killed the exact AM process and its identified
paused launcher group. Resume then exposed the round-zero completion defect;
retrying the failed root exposed the attempt-status defect. After both fixes,
the **same board** resumed successfully: only root 1 and child 2 exist, the
child's attempt row is unchanged, and a further succeeded-root resume leaves
the database hash unchanged.

The failed-only experiment uses a **real Codex Lead and a deterministic
fault-injection JSONL worker**, not a claimed real coding runtime. The primary
task exhausts two attempts with a diagnostic naming `RECOVERY_ACTION`. The
Lead's follow-up supplies that action, creates `fallback.txt`, and selects only
the successful task. This first exposed the successful-results-only gate. After
the fixes, the same board has root 1 succeeded, task 2 failed, task 3 succeeded;
the original failed attempts are unchanged. An external connection-reset failure
occurred during one resume; it is not counted as a product pass. The subsequent
explicit resume succeeded and succeeded-root resume preserved the database hash.

## Context and cost measurements

The repair run's actual transmitted contexts were:

| Lead round | Results | Context bytes | Full Exec prompt bytes | Runtime-reported input tokens | Cached input tokens | Output tokens |
|---|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 983 | 2800 | 15315 | 12160 | 75 |
| 1 | 1 | 1884 | 3701 | 31592 | 27264 | 171 |
| 2 | 2 | 2393 | 4210 | 49025 | 39424 | 273 |
| 3 | 3 | 3106 | 4923 | 67858 | 55552 | 444 |

Byte counts come from the bytes delivered to the subprocess, not a reconstructed
prompt. Usage columns are raw `turn.completed.usage` counters, not token estimates
from bytes. Their cumulative/per-turn semantics were not independently qualified;
do not sum them as billed tokens or compare them directly with context bytes.
The fixed Exec contract plus framing contributes 1,817 bytes here. Provider
history, user/runtime instructions, caching and runtime configuration are outside
the renderer's budget. Runtime observations are not part of Lead context.

The largest repair context is below 10% of the default 32,554-byte JSON budget.
This is a small case study, not a workload distribution or an optimization
benchmark. No compression benefit, token saving or performance improvement is
claimed. Context compression, Status/Summary/Trace, retrieval integration and the
closed ProcessSupervisor experiment remain unjustified by this measurement.

## Release checks and remaining scope

Local package creation and checksum verification passed; the extracted binary
outside the source tree reports `am 0.5.0-dev`. The license report covers 220
third-party crates and passed the packaging policy.

```text
source candidate 1855507c91f04b656f140a213080f8648fdf01a2
binary SHA256    15a661d2a5b0d890445e1f60ea696cdf68c6bc084caa6a648ff01f28f2eadf3d
archive SHA256   5f4cdf2bca28c38ccbfe763eb9f353a0299e66073d85aaca70fb292285bee4ad
archive          agentmosaic-v0.5.0-dev-x86_64-unknown-linux-gnu.tar.gz
```

The GitHub API was checked read-only: main still points to `8088252`, and the
latest public release remains `v0.3.0` (2026-09-14). No tag or public release has
been created by this work. This document records local candidate verification,
not a published v0.5 or a claim that all runtime profiles are requalified.

Fresh projects using the extracted package also passed:

- `recovery-release`: interrupted after child 2 succeeded; direct resume
  finished root 1. Child attempts and artifact bytes were unchanged. A second
  resume of the succeeded root left the database hash unchanged.
- `failure-release`: root 1 succeeded, primary task 2 failed after two
  attempts, recovery task 3 succeeded on its first attempt. The real Lead used
  the injected failure diagnostic without manual message transfer. Total
  runtime was 41.560 seconds.

The full copied-package review/fix experiment and remote PR gates are pending;
their results will be appended after completion. These do not change the
already-verified local deterministic or copied-package recovery results.
