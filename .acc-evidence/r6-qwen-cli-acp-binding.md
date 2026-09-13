# Sanitized Qwen ACP normal-CLI evidence

- Source state: `5f632aa76926ab58e094b77bdc690107300672dc` plus the
  read-only binding-inspection addition bound by the subsequent commit.
- Runtime command: registered `qwen --acp` through `agent-code-cli run-acp`.
- Auth: method name `openai` only; no credential value was read, logged, or
  committed. The isolated Git directory, database path, session ID, prompt,
  raw frames, model output, and provider endpoint are not retained.

## Command result

The normal CLI path registered the Qwen worker, submitted one bounded isolated
Git task, and ran it with a 600-second contention-calibrated budget. Exit `0`:
`completed task=1 agent=qwen`.

The authoritative CLI inspection then reported:

```text
task=1 status=succeeded assignee=qwen
task=1 path=qwen-worker.txt sha256=9fe33998687d40bf81f889c22a491a9e403c477fb827611815c4c4ed5eb71f67
task=1 attempt=1 agent=qwen runtime_kind=acp lifecycle_state=completed external_reference_present=true
```

The artifact's independently calculated SHA-256 was the same digest. The
binding command deliberately reports only whether a foreign reference exists,
not its value.

Sanitized command-log hashes: register
`32fa17913ca2dad25669b3cc2daf1802eddc1aa02e1d17c547f8863811d1efd7`;
submit `d8538a29a0115184bb7e6cfbae17a97a5484c360137b969a3b5627fa20143237`;
run `d56127539d96c0b45f44c4260d843bd832ef819fd196cb95942a6371a1dd53bf`;
exit receipt `9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`.

This is real Qwen result/artifact/binding evidence for M3. It does not prove
continuation, cancellation, recovery/reconcile, or M5 cross-runtime team E2E.
