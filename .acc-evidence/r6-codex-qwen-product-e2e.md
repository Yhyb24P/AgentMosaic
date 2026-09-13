# R6 Codex + Qwen product-result E2E (sanitized)

- executed: 2026-09-13
- tested source: working tree containing the v8 -> v9 final-reference migration
- command: `cargo test -p agent-code-runtime --test codex_live real_codex_thread_turn_uses_bounded_qwen_peer_result -- --ignored --nocapture`
- exit code: `0`
- result: 1 passed, 0 failed, 0 ignored, 2 filtered; 38.65 seconds
- captured stdout/stderr SHA-256: `49067c43b2f15153618097cccbb54dd1fa079508c594eb62033597e5bb0b2a5f`
- exit receipt SHA-256: `9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`

The live isolated-Git flow used Qwen Code `--acp` for the bounded worker
artifact and Codex app-server with test overrides `model="gpt-5.5"` and
`model_reasoning_effort="low"`. The retained observations establish:

- Qwen strict peer result persisted before Codex execution;
- the RAS MCP bridge was connected and `ras_request_context` completed;
- Codex continued after the MCP tool result and created the exact bounded
  artifact;
- the bridge persisted one bounded collaboration record;
- Codex/Qwen external bindings, task result, artifact digest, directed
  messages, and the explicit final task/artifact selection were reopened from
  the existing SQLite team board.

Only sanitized event kinds, tool name, boolean/status fields, hashes, command,
and result counts are retained. No prompt body, model text, hidden reasoning,
session/thread ID, credential, endpoint, raw protocol frame, PID, or temporary
path is committed.

This is a real R6 product-result-flow evidence item. It does not by itself
close M1/M5/M7: full Lead scheduling, retry/reassign, concurrent utility work,
and normal-path CLI control remain separate requirements.
