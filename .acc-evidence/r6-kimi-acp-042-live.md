# Sanitized Kimi Code 0.42.0 ACP bounded-turn evidence

- Source state before this evidence: `d1c01f78b554ae878ddd89734321b0fdba789f4b`
  plus the focused ignored-test addition later bound by the committing tree.
- Runtime command: `kimi acp` through `AcpWorkerDriver`, stdio JSON-RPC only.
- Working directory: fresh isolated temporary directory; its path is not
  retained. No credential, endpoint, session ID, raw JSON-RPC frame, prompt,
  model response, transcript, environment value, or provider setting is in
  this evidence.

## 1. Initialization and session discovery

The local executable reported `kimi 0.42.0`. The held-stdin probe sent only
`initialize`, `initialized`, and `session/new`; it exited `0`. Sanitized
observations: ACP protocol 1; agent name `Kimi Code CLI`; terminal `login`
method advertised; session list/resume/close/delete/fork and MCP HTTP/SSE
capabilities advertised; `session/new` returned a session reference. The
stdout hash is `e388a4b54a1e38f1dd830559d833ae850ed84fce91b3792f3c8dc6367fea5766`;
stderr was empty (`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`);
the exit-receipt hash is `9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`.

## 2. Bounded no-tool turn

```text
cargo test -p agent-code-runtime kimi_acp_completes_a_bounded_no_tool_turn -- --ignored --nocapture
```

Exit `0`; observed result: 1 passed, 0 failed, 0 ignored, 22 filtered out;
test elapsed 1.79 seconds. The log SHA-256 is
`4dd98403f9e0bc2b18f10ea9b2fc75f108b1a570fd0ff9b5c2af1a18e9d0a1df`; the
exit-receipt SHA-256 is
`9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`.

The test creates and removes its isolated directory, sends one bounded prompt
prohibiting tools/shell/network/filesystem writes, and asserts only a nonempty
external session reference and a nonempty terminal response. It does not
persist those values.

## Negative structured-result observation

The paired call through `execute_task` ended the live turn but failed with
`InvalidPeerResult(expected one strict JSON object)`, as required by the
existing fail-closed parser. Its sanitized test-log SHA-256 is
`be6e413170232aa7c752745b8dcf5b8c0114522078d16773c8c1de868e31b6e1`.
This is evidence against treating a completed Kimi runtime turn as an accepted
team result. No adapter was implemented.
