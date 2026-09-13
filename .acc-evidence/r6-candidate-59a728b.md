# R6 candidate evidence manifest

Candidate code commit:
`59a728b1839f613dee24e3fe635853947cd628ac`

The candidate contains the only executable change in this freeze: the ignored
real Qwen ACP crash/reopen harness and its test-only `libc` dependency.  The
documentation commit containing this manifest changes no executable Rust
source.

## Requalified commands

```text
cargo test -p agent-code-runtime --test codex_live \
  real_qwen_acp_process_crash_recovers_without_replay \
  -- --ignored --nocapture
exit 0; 1 passed, 0 failed, 0 ignored, 4 filtered out; 1.62s

cargo test -p agent-code-runtime --test codex_live \
  real_scheduler_runs_qwen_worker_and_utility_on_one_board \
  -- --ignored --nocapture
exit 0; 1 passed, 0 failed, 0 ignored, 4 filtered out; 47.16s

cargo fmt --all -- --check                                    exit 0
cargo clippy --workspace --all-targets --all-features -- -D warnings
                                                                exit 0
cargo test --workspace --all-features                          exit 0
  observed 175 passed, 0 failed, 16 ignored
git diff --check                                               exit 0
```

## Evidence hashes

```text
implementation_report.md
  e09c27cc3c3b3b3cbd98e35962188ca52d610ec587f1ad1917883c997858154b
.acc-evidence/r6-m5-scheduler-live-20260913.md
  996255dfd0fd98b7d43cc645c8a9ccceebb835d28447f689b09eb004f270a9d5
.acc-evidence/r6-qwen-acp-process-crash-recovery-20260913.md
  b57182003c1bf7268d9c58f2264a5f18aed9ca83a808c6a28e8d20ed8d2730d4
```

All three are sanitized: no token, credential, endpoint, private path, raw
wire frame, prompt, transcript, or native session identifier is included.
