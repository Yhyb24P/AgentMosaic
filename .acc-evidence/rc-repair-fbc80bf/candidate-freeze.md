# Exact-candidate freeze — RC repair

- date: 2026-09-13
- branch: `v2/rust-agent-team`
- frozen candidate (executable source): `89ac979d333fe3fc2e311fb566f3ab0056bec4c5`
- earlier candidate superseded by it: `295c96a075cdf3987d8e66fa75fce14d15611b3a`
  (identical except for the bounded-search determinism fix below)
- report/evidence commits (no executable or Cargo-source change): `4330af1`, `fbdd5f6`
- proof that later commits change no executable source:

```text
git diff --stat 89ac979 HEAD -- crates Cargo.toml Cargo.lock   ->   empty
```

## Why a second candidate was frozen

The first candidate passed every local gate, but the remote `rust-candidate` workflow
failed on `ubuntu-22.04` at
`crates/agent-code-workspace/tests/integration.rs::n07_search_bounded`
(`left: "b.rs"`, `right: "a.rs"`). `search_dir` relied on the `ignore` walker's
directory enumeration order, so a bounded search could return a different first match
for the same repository depending on the filesystem. That is a real product defect
(non-reproducible bounded search), not a bad test: `crates/agent-code-workspace/src/tools.rs`
now sorts walked file paths so the bounded early return is deterministic, and the n07
test writes its files in reverse lexicographic order to assert order-independence. That
crate is otherwise unrelated to the team path.

## Exact-candidate gates (run on HEAD with a clean worktree)

```text
cargo fmt --all -- --check                                              fmt_exit=0
cargo clippy --workspace --all-targets --all-features -- -D warnings    clippy_exit=0
cargo test --workspace --all-features                                   test_exit=0
cargo build --release --workspace                                       release_exit=0
git diff --check                                                        diff_check_exit=0
```

Workspace test counts on the exact candidate: **257 passed / 0 failed / 17 ignored**
(the live Codex+Qwen E2E is one of the 17 ignored tests and is run separately below).

## Live production E2E on the exact candidate

```text
cargo test -p agent-code-cli --test team_live_product -- --ignored --nocapture
```

- exit `0`; `1 passed; 0 failed; 0 ignored; 1 filtered out; finished in 47.16s`
- real `codex-cli 0.154.0` Lead and real Qwen Code `0.23.3` Worker, driven only by the
  public CLI `run-team`; the test creates no delegated task, reads no plan file, runs no
  worker itself, and finalizes no refs.
- durable result of that run: root `task=1` (`reasoning`, `codex-lead`, succeeded),
  worker `task=2` (`bulk`, `qwen-worker`, parent `1`, succeeded), utility `task=3`
  (parent `1`, succeeded), answer containing the run's random token, refs `[2]` and
  artifact `worker.txt` sha256
  `c71e4557ec191bc34bc97580aded7a15909c0a80b64bde34ce1b11a09e7fac7f`.

## Copied release-binary smoke

`scripts/rc-release-smoke.sh` (release binaries copied outside the source tree):
`run_team_exit=0`, `TOKEN_IN_ROOT_ANSWER=yes`, `TOKEN_IN_WORKER_RESULT=yes`.
See `release-binary-smoke.md`.

## Evidence log

- `.acc-evidence/rc-repair-fbc80bf/exact-candidate-gates.log`
  sha256 `be9b42c6d4d9ac64287935a9a06671c4831b2a9dc5aeb8e6a4e4d48381d349d4`
  (this log is the candidate's own gate record and is rewritten on each re-run; the
  hash above is for the run on `89ac979`)
- worktree after the run: only the freshly written log file was modified; no other
  tracked file changed.

## Readiness claims

```text
LOCAL_PRODUCT_RC_READY        = true   (see product_self_reaudit.md)
REMOTE_DETERMINISTIC_CI_READY = false  (branch workflow not yet run on this candidate)
PUBLIC_RELEASE_READY          = false  (no tag, no GitHub Release, not authorized)
```

No git tag and no GitHub Release was created or pushed by this repair.
