# R6 M5 scheduler live receipt

Source commit: `05a8fbc94986f2e0249cfa217dd3f056656eb244`

Command:

```text
cargo test -p agent-code-runtime --test codex_live real_scheduler_runs_qwen_worker_and_utility_on_one_board -- --ignored --nocapture
```

Observed terminal test result: `1 passed, 0 failed, 0 ignored, 3 filtered out`;
elapsed `36.59s`.

The captured command-output file SHA-256 is
`586f7c663877d57ca86bcf001ada486d492ffcd280264dc811a74a4ef0caf2dc`.
The launcher failed to retain its separate exit-code receipt after the process
finished. This record therefore reports the test framework's terminal result,
not a fabricated shell exit code. No runtime transcript, credential, endpoint,
native session reference, or private path is retained here.

This run exercises the existing live scheduler topology (real Qwen ACP worker,
deterministic utility, real Codex lead configured by the harness with
`gpt-5.5` and `low`, retry/reassign, selected refs, and reopen). It does not
by itself close the remaining real external-process crash/reconcile criterion.
