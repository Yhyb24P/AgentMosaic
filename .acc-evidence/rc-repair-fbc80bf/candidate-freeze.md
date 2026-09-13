# Exact-candidate freeze — RC repair

- date: 2026-09-13
- branch: `v2/rust-agent-team`
- frozen candidate (executable source): `295c96a075cdf3987d8e66fa75fce14d15611b3a`
- report/evidence commit (no executable or Cargo-source change):
  `4330af1` — `docs(rc): bind the frozen candidate hash in the current-state block`
- proof that the later commit changes no executable source:

```text
git diff --stat 295c96a HEAD -- crates Cargo.toml Cargo.lock   ->   empty
```

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

- exit `0`; `1 passed; 0 failed; 0 ignored; 1 filtered out; finished in 89.57s`
- real `codex-cli 0.154.0` Lead and real Qwen Code `0.23.3` Worker, driven only by the
  public CLI `run-team`; the test creates no delegated task, reads no plan file, runs no
  worker itself, and finalizes no refs.

## Copied release-binary smoke

`scripts/rc-release-smoke.sh` (release binaries copied outside the source tree):
`run_team_exit=0`, `TOKEN_IN_ROOT_ANSWER=yes`, `TOKEN_IN_WORKER_RESULT=yes`.
See `release-binary-smoke.md`.

## Evidence log

- `.acc-evidence/rc-repair-fbc80bf/exact-candidate-gates.log`
  sha256 `2fc5a7fa865dc02abfc24197aa4cf1a3609fda0e2672596fafd19e3e487907da`
- worktree after the run: only the newly written log file was untracked; no modification
  to any tracked file.

## Readiness claims

```text
LOCAL_PRODUCT_RC_READY        = true   (see product_self_reaudit.md)
REMOTE_DETERMINISTIC_CI_READY = false  (branch workflow not yet run on this candidate)
PUBLIC_RELEASE_READY          = false  (no tag, no GitHub Release, not authorized)
```

No git tag and no GitHub Release was created or pushed by this repair.
