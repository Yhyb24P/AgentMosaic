# AgentMosaic v0.5 Team Runner — RC qualification evidence

Point-in-time engineering evidence for the v0.5 team-runner RC candidate. Only
commands that were actually run appear here; every outcome is the observed result
of this round on this machine. Historical audits are frozen and are never rewritten
to match today.

## 1. Identity

```text
base_sha        fb23184e6e50ba89ed9f4a09941eaa33bfbf9d73   (origin/main, PR #22 merged)
candidate_sha   0b90f09a148914c8700298758f82c9677f512eda   (branch task/v05-team-runner-rc)
branch          task/v05-team-runner-rc
workspace       0.5.0-dev
sqlite_schema   12
rustc           1.94.1 (e408947bf 2026-03-25)
cargo           1.94.1 (29ea6fb6a 2026-03-24)
host            Linux x86_64
```

This document itself is a follow-up documentation commit on top of `candidate_sha`;
the qualified code state is exactly `candidate_sha`.

`git status --short` at `candidate_sha`, after the commit:

```text
(clean; the only untracked residue in the worktree is the pre-existing, ignored
 `researchd.db`, which is not tracked and predates this round)
```

## 2. Deterministic gates (candidate_sha)

```text
scripts/ci/check_identity.sh                                  PASS
cargo fmt --all -- --check                                    PASS
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
                                                              PASS (exit 0)
cargo test --locked --workspace --all-features                PASS
    397 passed / 0 failed / 25 ignored
cargo build --locked --release --workspace                    PASS
cargo test --locked -p agentmosaic-storage --test published_v03_migration
                                                              PASS (1 passed)
cargo test --locked -p agentmosaic-storage --test schema_v8_migration
                                                              PASS (2 passed)
git diff --check                                              PASS
```

Test inventory: 422 tests collected; `25` are `#[ignore]` live/credentialed tests
across `agentmosaic-cli/tests/team_live_product.rs` (2),
`agentmosaic-runtime` unit tests (16), `codex_live.rs` (5),
`claude_cli_driver.rs` (1) and `codex_exec_driver.rs` (1).

`deterministic tests` = the 397 non-ignored tests above. `ignored live tests` = the 25
`#[ignore]` tests. `actual executed live tests` = the two in section 5. An ignored
test is never counted as a pass.

## 3. The normal-path gate this round adds

The first-class product path is `init -> agent add -> doctor -> run -> status/final/artifact`.
The old candidate smoke only proved the low-level `submit -> status` pair, and the old
live smoke only drove the `register`/`run-team` compatibility surface. Both are replaced:

- `scripts/ci/normal-path-smoke.sh` — deterministic, credential-free, copied **release**
  binary outside the source tree, driving the full first-class path with the
  development-only mocks (`codex_bridge_mock` Lead, `acp_m2_mock` worker/utility).
- `scripts/rc-release-smoke.sh` — the canonical **live** normal path: `codex-exec` Lead +
  real Qwen ACP worker, copied release binary.
- `.github/workflows/candidate.yml` — now covers identity, fmt, `--locked` clippy/tests,
  `--locked` release build, diff hygiene, published-v0.3 migration, authentic-v8 migration
  and the copied-binary normal-path smoke.

No model, provider, endpoint or credential is named anywhere in the harness; a non-secret
launcher override is reachable only through `AM_RC_LEAD_PROGRAM` / `AM_RC_WORKER_PROGRAM`.

### 3.1 Deterministic normal path (release binary, `target/release`)

```text
ROOT_ID=1  ROOT_STATUS=succeeded
WORKER_TASK_ID=2  WORKER_STATUS=succeeded  (child of root 1)
ARTIFACT_PATH=worker-result.txt
ARTIFACT_SHA256=748874e7a2f36d93fdad55bb30e303eac4127c4d16ca659dcff86f7bfdf7cced
SELECTED_TASK_MATCH=true  SELECTED_ARTIFACT_MATCH=true  FRESH_READBACK_MATCH=true
LEAD_PROMPT_SAW_WORKER_RESULT=true
NORMAL_PATH_SMOKE=PASS
```

`doctor.ready == true`; the run `status == succeeded`; the selected artifact belongs to
the succeeded worker child; its recorded SHA-256 equals the file's SHA-256; `final` in a
fresh process reproduces the durable answer; the Lead's own prompt carried the durable
worker result and artifact digest.

### 3.2 Deterministic normal path (cargo-dist archive binary)

See section 6. Same assertions, `NORMAL_PATH_SMOKE=PASS`.

## 4. Live heterogeneous team (G6)

```text
command   scripts/rc-release-smoke.sh target/release
runtime   codex-cli 0.155.1 (Lead, adapter codex-exec)
          qwen 0.24.2      (Worker/Utility, adapter acp)
AM_SHA256 92f7399423105e540d16b4f2daf000166c77d4258e35b28b46b626df9fd5bff9
AM_VERSION am 0.5.0-dev
```

Run receipt (sanitized; no credentials, raw auth, provider payload, hidden reasoning or
foreign session id retained):

```text
ROOT_ID=1          ROOT_STATUS=succeeded
WORKER_TASK_ID=2   WORKER_STATUS=succeeded
ARTIFACT_PATH=worker.txt
ARTIFACT_SHA256=d22f4df07ae04aaa391350da5351eda961919ab7afe84b4ecbe283e6fa5b30e0
SELECTED_TASK_MATCH=true  SELECTED_ARTIFACT_MATCH=true  FRESH_READBACK_MATCH=true
binding: task=2 attempt=1 agent=worker runtime_kind=acp
         lifecycle_state=completed external_reference_present=true
RC_RELEASE_SMOKE=PASS
```

The Lead actually delegated: the run payload's `task_refs` is `[2]`, the worker task is a
succeeded child of the root, and the persisted answer carries the random token the
objective introduced. `worker.txt` on disk is exactly `worker=complete <token>` plus one
newline, and the persisted artifact SHA-256 equals the on-disk SHA-256. `doctor.ready ==
true` for the `codex-exec` Lead and both ACP agents.

## 5. Recovery without replay (G5)

### 5.1 Repeatable deterministic harness

```text
command   scripts/rc-recovery-smoke.sh target/release
```

1. the copied release binary starts a real team run (mock Lead + counting mock worker);
2. the worker completes one artifact durably;
3. the run is interrupted (SIGKILL) while the root is still Running and a second task is
   stuck, so the interruption window is deterministic rather than a race;
4. `am status` shows `task=1 running attempts=1`, `task=2 succeeded attempts=1`,
   `task=3 running attempts=1`;
5. `am recover <db> 1` explicitly closes the interrupted root attempt;
6. `am resume-team <db> <repo> 1 --lead lead` continues the same durable root and Lead;
7. the run completes from the durable evidence.

```text
ROOT_ATTEMPTS_BEFORE=1  ROOT_ATTEMPTS_AFTER=2   ROOT_STATUS_AFTER=succeeded
WORKER_TASK_ID=2  WORKER_ATTEMPTS_BEFORE=1  WORKER_ATTEMPTS_AFTER=1
WORKER_ARTIFACT_SHA256=748874e7…  (unchanged across recovery)
WORKER_LAUNCHES=1                (the worker process was launched exactly once)
CANONICAL_LEAD=lead              (root assignee unchanged)
RECOVERY_NO_REPLAY=PASS
```

The child task id is unchanged, the child attempt count is unchanged, the artifact bytes
and SHA-256 are unchanged, the worker was invoked exactly once, the root gained a new
attempt without rewriting attempt 1, and the canonical Lead is unchanged. "No replay" is
proven by the invocation counter and attempt history, not only by the final result.

### 5.2 Live runtime gate

```text
cargo test --locked -p agentmosaic-runtime --test codex_live \
  real_qwen_acp_process_crash_recovers_without_replay -- --ignored
→ test result: ok. 1 passed; 0 failed; ... finished in 2.19s
```

A real Qwen ACP process is started, bound durably, then killed; after the controller
disappears the reopened board still shows the attempt Running with `lifecycle_state =
running`, and the recovery primitive closes it to Failed with an `interrupted` binding and
no artifacts — one attempt, no replay.

## 6. Release artifact (G8)

Built on the frozen candidate with the checked-in cargo-dist configuration
(`cargo-dist 0.33.0`, target `x86_64-unknown-linux-gnu`), no tag or GitHub Release created:

```text
command   dist build --artifacts=local --target x86_64-unknown-linux-gnu --allow-dirty
archive   target/distrib/agentmosaic-cli-x86_64-unknown-linux-gnu.tar.xz
archive_sha256 (per-artifact .sha256 sidecar, verified) =
          13156e108bc1c9408ba39fa7f5ba595b7f110164acc2b81778122ef47bfe1269
archive contents  [bin] am  [misc] CHANGELOG.md, LICENSE, README.md
extracted am sha256 = 84896e6caf3470a98d08657b11f73584f73f29d9fbc1081edc6d06cfa1e1c9bb
am --version        = am 0.5.0-dev
dist manifest       records the archive plus its `.sha256` sidecar and the `am` asset
                    (`agentmosaic-cli-x86_64-unknown-linux-gnu-exe-am`)
```

The extracted `am` (from the archive, in an independent temporary directory) passes the
deterministic normal-path smoke: `NORMAL_PATH_SMOKE=PASS` with
`ARTIFACT_SHA256=748874e7…`, `SELECTED_TASK_MATCH=true`, `FRESH_READBACK_MATCH=true`.

The archive's `am` is a `dist`-profile build (`lto=thin`), so its bytes differ from
`target/release/am`; both binaries pass the same smoke.

Installer smoke: **not covered**. A global (curl-sh) installer resolves a GitHub Release
URL for a tag; no tag or Release may be created this round, so an installer smoke cannot
exercise the candidate. Recorded as uncovered rather than substituted.

## 7. Migration compatibility (G7)

```text
published_v03_database_migrates_additively_to_v12              PASS
authentic_v8_database_migrates_to_current_and_preserves_rows   PASS
v10_database_migrates_to_current_adding_runtime_foundation     PASS
```

Schema stays v12; the published v0.3 fixture and the authentic v8 fixture both migrate
additively and preserve their rows.

## 8. Negative control (not committed)

A deliberately non-conforming fixture, run outside the production tree and recorded here
only: the scripted Lead completes by selecting the worker task with an artifact
SHA-256 of all zeros (a digest that is well-formed but wrong).

```text
am run --json "<objective>"   exit=2
stderr  "the lead run failed: the lead completed without grounding the answer in
         completed tasks"
board   task=1 status=failed (root);  worker/utility children succeeded but ungrounded
```

The normal-path gate asserts `ARTIFACT_SHA256 == actual file SHA-256`, so this fixture
would fail the gate: the gate is proven to be able to fail, not only to pass.

## 9. Scope and cleanliness (G1, G11)

No framework expansion, no schema/wire redesign, no renamed persisted identifiers.
The only Rust change is the `am init` onboarding hint (`codex-app-server` →
`codex-exec`, the canonical default) and its assertion. No tag, Release, merge, force
push or ruleset change was executed. No `.db`, raw transcript, credential or temporary
evidence is tracked.

## Bounded P0/P1 code review (T6)

Reviewed only for defects that would break team-runner reliability, with the existing
deterministic suite as the evidence for each area. No reproducible P0/P1 defect was found
this round, so no code was changed for it.

| Area | Evidence | Result |
|---|---|---|
| root / task state transitions | `scheduler::tests::failed_task_retries_then_reassigns`, `team_runner_product` attempt-history tests (all pass) | ok |
| retry / resume attempt numbering | `failed_root_resume_appends_new_attempt`, `root_exec_binding_history_is_one_row_per_attempt` | ok |
| scheduler concurrency quota | `scheduler::tests::same_agent_respects_max_concurrency`, `independent_tasks_run_concurrently`, `capacity_queued_task_stays_assigned_until_driver_can_start` (G4) | ok |
| artifact ownership / hash | `normal-path-smoke.sh` asserts owner task + SHA-256 == file SHA-256; the live smoke re-checks on disk | ok |
| final grounding | negative control: an ungrounded selection is refused and the root fails closed | ok |
| copied-binary / runtime path | this round's normal-path gate and live gate both run a binary copied outside the source tree | ok |
| runtime process cleanup / absolute timeout | `acp_m2_lifecycle` suite (cancel, crash, repair, hang) | ok |
| durable binding | `codex_exec_lead` + `binding` readback; the live smoke shows `lifecycle_state=completed` on the worker attempt | ok |
| normal vs advanced CLI root invariants | `submit`, `override`, `resume`, `run-acp`, `continue-acp` all refuse a team root (`advanced.rs` guards; `compatibility_matrix` passes) | ok |
| context-capacity failure fails closed | `project_onboarding::oversized_public_agent_id_fails_without_launching_either_lead_runtime` (no runtime starts; the root is left failed) | ok |

Refactoring was deliberately not done: no evidence-based correctness defect justified it.

## 10. Known untested areas and remaining risks

- Remote CI (`rust-candidate`) on `candidate_sha` was **not** run: the workflow is
  `workflow_dispatch`. It is now run: see section 11 for the passing remote run.
- Windows/macOS distribution: not built (Linux `x86_64` only).
- Installer smoke: not covered (see section 6).
- Live qualification is machine-dependent: it needs already-authenticated `codex` and
  `qwen`. It is not reproducible in credential-free CI.
- The deterministic normal-path gate uses scripted mocks; it proves the product plumbing
  and grounding, not model behaviour. Model behaviour is covered only by the live gate.
- Only `codex-exec` (Lead) and `qwen` (ACP worker/utility) were exercised live this round.
  `codex-app-server` lead, `claude-cli`, and other ACP peers were not re-run live.

## 11. Verdict

### Remote CI (G10)

```text
gh workflow run rust-candidate --ref task/v05-team-runner-rc -f ref=<candidate_sha>
run 35643390303  rust-candidate  conclusion=success
  ✓ identity gate   ✓ fmt   ✓ clippy --locked   ✓ tests --locked   ✓ release build
  ✓ diff hygiene    ✓ published v0.3 migration   ✓ authentic v8 migration
  ✓ copied-binary normal-path smoke
```

Draft PR #23 (`task/v05-team-runner-rc` → `main`) carries the same code; the merge-tree
checks `rust` and `rust_quality` both pass on it. The candidate workflow is triggered
again on the final branch head so the green run names the exact frozen head.

```text
BASE_SHA=fb23184e6e50ba89ed9f4a09941eaa33bfbf9d73
CANDIDATE_SHA=0b90f09a148914c8700298758f82c9677f512eda

TEAM_RUNNER_SCOPE_FROZEN=true
NORMAL_PATH_QUALIFIED=true
RELEASE_ARTIFACT_QUALIFIED=true
LIVE_HETEROGENEOUS_TEAM_QUALIFIED=true
RECOVERY_NO_REPLAY_QUALIFIED=true
MIGRATION_COMPATIBILITY_QUALIFIED=true

RC_CANDIDATE_READY=true
PUBLIC_RELEASE_READY=false
```

G0–G11 are satisfied with the evidence above, including a green remote `rust-candidate` run
that now covers the copied-binary normal-path smoke. `PUBLIC_RELEASE_READY` is `false`
because tag / Release are a separate, explicitly authorized step.

## 12. Repository governance (recommendation only — not executed)

`main` has no branch protection (REST returns 404) and no rulesets (empty list). Actual
check contexts observed on `main` are `rust` and `rust_quality` (`rust-candidate` is
`workflow_dispatch` only and does not run on push/PR). Suggested, pending explicit
authorization:

```text
target: main
require a pull request before merging
require status checks: rust, rust_quality
block force pushes
block branch deletion
```

PR #20 (`audits/context-flow-token-cost`): evidence-based recommendation — **close as
superseded**. Its single file `docs/audits/S0_CONTEXT_FLOW_BYTE_BUDGET_AUDIT.md` states
`BYTE_BUDGET_MEASURED=true` and `L2_TRUNCATION_IMPLIES_INVALID_JSON=true` and explicitly
leaves the S1 question open; main already carries those same findings, answered and
extended, in `docs/audits/S1_CONTEXT_RENDERER_BOUNDARY_REACHABILITY_AUDIT.md` (merged via
PR #21). PR #20's merge-base (`8088252`) predates that work, so merging it would re-open a
stale tree. Not closed, because closing needs authorization.
