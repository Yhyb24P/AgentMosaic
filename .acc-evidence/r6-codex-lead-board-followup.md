# Sanitized Codex Lead board-follow-up evidence

- Source baseline: `2695077ba73850f704e1046a0eab7eba0fc745c7` plus the
  focused live-harness change that is bound by the subsequent commit.
- Runtime: local `codex app-server --stdio`, with test overrides
  `model="gpt-5.5"` and `model_reasoning_effort="low"`.
- Working tree: an isolated temporary Git directory and SQLite board. Paths,
  thread/turn IDs, prompts, model output, raw JSON-RPC frames, credentials,
  and private reasoning were not retained.

## Command

```text
cargo test -p agent-code-runtime --test codex_live real_codex_lead_plans_and_follows_up_on_durable_team_result -- --ignored --nocapture
```

Exit `0`; observed result: 1 passed, 0 failed, 0 ignored, 2 filtered out;
elapsed 23.20 seconds. Sanitized test-log SHA-256:
`ecdea30f44ffade3b1125423554d60e6ac56ad9c3f0b7ad400b6e33d370e810b`.
Exit-receipt SHA-256:
`9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`.

## Observed flow

1. Real Codex created the bounded two-task plan in its thread.
2. The deterministic utility result, directed message, and artifact were
   committed to the authoritative SQLite board before the follow-up turn.
3. The follow-up prompt contained no teammate result. Codex invoked the
   allowlisted `ras_request_context` path; two durable collaboration records
   exist because the developer instruction also caused one bounded context
   request during planning.
4. The same Codex thread completed the follow-up and created the expected
   bounded final artifact. The Codex result/artifact were committed to the
   board, which was reopened for assertions.

This is live evidence for the Codex portion of M1. It does not prove the
required real Qwen-worker concurrency/retry/reassign topology for M5.
