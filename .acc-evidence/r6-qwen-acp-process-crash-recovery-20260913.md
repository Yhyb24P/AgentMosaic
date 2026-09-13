# R6 Qwen ACP process-crash and recovery receipt

Date: 2026-09-13

This is a bounded, real-runtime recovery drill.  It stores neither ACP frames,
prompts, credentials, endpoint details, private transcripts, nor native
session identifiers.

## Command and observed result

```text
cargo test -p agent-code-runtime --test codex_live \
  real_qwen_acp_process_crash_recovers_without_replay \
  -- --ignored --nocapture

exit code: 0
observed: 1 passed; 0 failed; 0 ignored; 4 filtered out; finished in 1.62s
```

## What the live harness established

1. An isolated wrapper launched exactly `qwen --acp`; the Rust ACP driver
   authenticated using the configured method and created an external session.
2. Before the prompt can produce a terminal result, the driver persisted a
   `running` ACP external binding on a real SQLite board.
3. The harness read only its own wrapper-written PID, verified `/proc` showed
   both `qwen` and `--acp`, then sent `SIGKILL` to that exact child.  It never
   searches for or signals an arbitrary system process.
4. The in-flight controller future was abandoned before terminal settlement,
   modelling a host interruption after the durable binding.  A fresh board
   instance observed the task and attempt still `running`, with no artifact.
5. `recover_interrupted_attempt` atomically made the attempt failed, changed
   the binding to `interrupted`, required explicit resume, and performed no
   replay.  A second recovery is idempotently empty.

This is a real external-Qwen process-crash plus SQLite reopen/recovery proof.
It does not by itself seal M5: the separate complete team topology and final
candidate-bound evidence remain required.
