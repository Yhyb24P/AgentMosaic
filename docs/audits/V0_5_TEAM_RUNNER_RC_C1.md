# AgentMosaic v0.5 Team Runner — C1 evidence closeout

This C1 audit supersedes the RC readiness verdict of
[V0_5_TEAM_RUNNER_RC.md](V0_5_TEAM_RUNNER_RC.md) while preserving that document
as frozen point-in-time evidence.

C1 closes evidence gaps only. No production Rust semantics, SQLite schema,
public JSON shape, runtime selection, Lead/Scheduler policy, retry policy, or
recovery semantics changed.

## Candidate and evidence identity

```text
BASE_MAIN_SHA=fb23184e6e50ba89ed9f4a09941eaa33bfbf9d73
PRE_C1_PR_HEAD=42b4e9dacd0343bf5d841e65f7510bc7d18b69af
C1_CANDIDATE_SHA=4a88e85c09fbe50a48dc216e00d19078aaf8ce34
C1_EVIDENCE_HEAD_SHA=reported in the final PR / C1 closeout; intentionally not self-recorded
PR=23
branch=task/v05-team-runner-rc
workspace_version=0.5.0-dev
SQLite_schema=12

rustc=rustc 1.94.1 (e408947bf 2026-03-25)
cargo=cargo 1.94.1 (29ea6fb6a 2026-03-24)
cargo-dist=cargo-dist 0.33.0
Codex=codex-cli 0.155.1
Qwen=0.24.2
host=Linux x86_64
```

The candidate commit contains executable, workflow, and qualification-script
state. All qualification below ran against that SHA. The later evidence commit
contains only this audit and the current-status link; it is a documentation-only
descendant and is not represented as the qualified candidate.

## Deterministic and migration gates

The full gate was run on the clean candidate checkout and passed:

```text
scripts/ci/check_identity.sh                                  PASS
cargo fmt --all -- --check                                    PASS
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
                                                              PASS
cargo test --locked --workspace --all-features                PASS
cargo build --locked --release --workspace                    PASS
cargo test --locked -p agentmosaic-storage --test published_v03_migration
                                                              PASS (1 passed)
cargo test --locked -p agentmosaic-storage --test schema_v8_migration
                                                              PASS (2 passed)
git diff --check                                              PASS
```

Workspace test totals: 397 passed / 0 failed / 25 ignored (44 result groups). The
existing credentialed live tests remain ignored by the ordinary suite; the two C1
live recovery gates below were run explicitly. No production source or Cargo files
changed in C1.

## Grounded real live normal path

`scripts/rc-release-smoke.sh` now parses each `--json` payload with Python's
standard `json` module and only prints a passing receipt after these comparisons
succeed: run task refs equal the discovered worker task; run artifact refs equal
the worker task, exact path, and persisted digest; the durable worker and root
have the expected assignees/statuses; the legacy read-only status confirms the
parent relationship; artifact JSON agrees with the on-disk SHA-256; bytes match
the requested content; fresh `status` and `final` readbacks match; and the worker
binding is completed with an external reference. No public JSON field was added.

The same checks passed first on the workspace release binary and then on the
`am` extracted from the cargo-dist archive (the latter is the C1-qualified
result):

```text
archive AM_SHA256=84896e6caf3470a98d08657b11f73584f73f29d9fbc1081edc6d06cfa1e1c9bb
ROOT_ID=1
WORKER_TASK_ID=2
WORKER_STATUS=succeeded
ARTIFACT_PATH=worker.txt
ARTIFACT_SHA256=1e27933728304208c85a7abe68b820afbb65ab857b383ef1225be29f79c851ae
SELECTED_TASK_MATCH=true
SELECTED_ARTIFACT_MATCH=true
ROOT_CHILD_MATCH=true
ARTIFACT_HASH_MATCH=true
FRESH_READBACK_MATCH=true
RC_RELEASE_SMOKE=PASS
```

The per-run token and raw model output are intentionally omitted.

## Recovery evidence

The deterministic harness remains a separate mock-runtime qualification:

```text
scripts/rc-recovery-smoke.sh target/release
root attempts       1 -> 2
worker attempts     1 -> 1
worker launches     1
RECOVERY_NO_REPLAY=PASS
```

The additional live Qwen crash primitive passed independently:

```text
cargo test --locked -p agentmosaic-runtime --test codex_live \
  real_qwen_acp_process_crash_recovers_without_replay -- --ignored
1 passed
```

`scripts/rc-recovery-live-smoke.sh` completed with a real CodexExec Lead and a
real Qwen ACP Worker. A development-only ACP mock supplied the bounded hanger.
The gate waited for durable worker success and hanger execution, interrupted only
the controller process group created by this script, recovered the root, and
resumed that same root with `--lead lead`.

The gate was run against the cargo-dist extracted `am`; it verified the same
worker task and artifact before and after, exact artifact bytes and digest,
worker launch count 1, worker attempt count 1, root attempt count 1 then 2,
canonical Lead `lead`, resumed `task_refs` and `artifact_refs` selecting that
original worker and SHA, final answer grounding, and the same CodexExec native
thread on both root attempts. It also confirmed the hanger remained running at
the interruption boundary before explicit root recovery. Thread ids were compared ephemerally in Python
against this test's temporary database; neither id nor any raw transcript was
printed or retained.

```text
ROOT_ATTEMPTS=1->2
WORKER_ATTEMPTS=1->1
WORKER_LAUNCHES=1
CANONICAL_LEAD=lead
LEAD_THREAD_REUSED=true
FINAL_TASK_REF_MATCH=true
FINAL_ARTIFACT_REF_MATCH=true
RECOVERY_LIVE_END_TO_END=PASS
```

The provisional candidate `4572d3d4b4f8b38cf82fd60ec4ac84ccbc908386` and its evidence descendant were superseded after review found the live-recovery harness did not assert resumed final refs or hanger residue. One earlier run reached the 120-second wait bound without durable worker success (`worker success not durable`). Its temporary fixture was cleaned by the harness, and that failure summary is preserved here. At that point the harness had no sanitized task-state receipt, so the result could not distinguish model delay from runtime delay; there was no evidence of a product defect. We classified the evidence gap as a harness limitation (B), increased the bounded window to 240 seconds, added sanitized state on timeout, and added final-ref and hanger-residue assertions. The single explained rerun on the new frozen candidate passed all assertions. The failure record was not removed.

## cargo-dist provenance and archive checks

The archive was built in a detached, clean worktree at the candidate SHA using
`dist build --artifacts=local --target x86_64-unknown-linux-gnu`, without
`--allow-dirty`. HEAD matched the candidate before and after build; tracked tree,
staged diff, and unstaged diff were clean. The archive shipped only `am`,
README, LICENSE, and CHANGELOG; development mocks were copied from the same
candidate build into an external qualification directory and were not added to
the archive.

```text
archive=agentmosaic-cli-x86_64-unknown-linux-gnu.tar.xz
archive_sha256=72cedb709e7a9fe895a937b5c30fa2c78e429c14b1614f2cbbfd143076550246
sidecar_sha256=verified (sha256sum -c: OK)
extracted_am_sha256=84896e6caf3470a98d08657b11f73584f73f29d9fbc1081edc6d06cfa1e1c9bb
extracted_am_version=am 0.5.0-dev
archive_contents=am, README.md, LICENSE, CHANGELOG.md
DIST_ARTIFACT_PROVENANCE_VERIFIED=true
DIST_ARTIFACT_NORMAL_PATH_QUALIFIED=true
DIST_ARTIFACT_LIVE_QUALIFIED=true
```

## Remote exact-candidate and PR checks

The candidate workflow now prints `QUALIFIED_SHA` after checkout and fails if a
40-character requested SHA does not exactly equal the checked-out HEAD.

```text
rust-candidate_run_id=35781520077
workflow_conclusion=success
job_candidate=success
QUALIFIED_SHA=4a88e85c09fbe50a48dc216e00d19078aaf8ce34
QUALIFIED_SHA_matches_C1_CANDIDATE_SHA=true
```

The final evidence-only PR head was required to pass both ordinary PR checks:
`rust` and `rust-quality`. Their final status is visible on PR #23; both passed
before the C1 readiness verdict was issued. The release-plan check also passed
and is not used as a substitute for `rust-candidate`.

## Gate matrix and verdict

```text
C1-G0  deterministic candidate gates                         PASS
C1-G1  selected run task ref compared to worker id            PASS
C1-G2  selected run artifact ref compared to path and SHA     PASS
C1-G3  root-child, worker terminal, fresh readback            PASS
C1-G4  clean exact-candidate cargo-dist provenance            PASS
C1-G5  extracted archive am deterministic normal path         PASS
C1-G6  extracted archive am real CodexExec + Qwen live path   PASS
C1-G7  deterministic recovery no-replay                       PASS
C1-G8  real Qwen crash primitive                              PASS
C1-G9  real root recovery/no-replay E2E                       PASS
C1-G10 Codex native thread reuse                              PASS
C1-G11 v0.3 and v8 migrations                                  PASS
C1-G12 remote rust-candidate exact SHA                        PASS
C1-G13 final evidence-head rust + rust-quality                PASS
C1-G14 no C1-generated DB/raw transcript/credential/temp evidence       PASS
```

```text
TEAM_RUNNER_SCOPE_FROZEN=true
NORMAL_PATH_QUALIFIED=true
LIVE_REFS_GROUNDED=true
DIST_ARTIFACT_PROVENANCE_VERIFIED=true
DIST_ARTIFACT_NORMAL_PATH_QUALIFIED=true
DIST_ARTIFACT_LIVE_QUALIFIED=true
RECOVERY_DETERMINISTIC_NO_REPLAY_QUALIFIED=true
RECOVERY_REAL_QWEN_CRASH_PRIMITIVE_QUALIFIED=true
RECOVERY_LIVE_END_TO_END_QUALIFIED=true
LEAD_THREAD_REUSE_VERIFIED=true
MIGRATION_COMPATIBILITY_QUALIFIED=true
REMOTE_DETERMINISTIC_CI_READY=true
PR_HEAD_CI_GREEN=true
RC_CANDIDATE_READY=true
PUBLIC_RELEASE_READY=false
INSTALLER_SMOKE=NOT_COVERED
```

C1 added no database, raw runtime transcript, credential, or temporary test
evidence to Git. The repository retains its pre-existing tracked v0.3 migration
fixture database and curated demo transcript files; C1 did not alter them. One
ignored `researchd.db` also existed in the shared workspace before C1 and remains
untracked; it is not in the candidate or evidence commit. No installer smoke, Windows/macOS prebuilt
qualification, branch-protection review, PR #20 closure, tag, or GitHub Release
was attempted. No release action is authorized by this audit.
