# Qwen Code runtime

Qwen Code is the reference Worker. It runs through the `acp` driver kind
(`AcpWorkerDriver`), which speaks the Agent Client Protocol over stdio.

## Registration

Register the runtime as opaque executable argv; its local login, provider,
model, endpoint and launcher profile stay owned by Qwen Code.

```bash
am agent add worker --role worker --adapter acp -- qwen -ds --acp
```

Optional artifact paths are relative to the task working directory; the driver
records their hashes with the result rather than copying file bodies. Do not
store credentials or authentication choices in AgentMosaic.

## What the driver does

- Starts the executable with the registered driver args (for example `qwen -ds --acp`).
- Establishes one ACP session and, for a follow-up, reuses that session rather than
  opening a second one.
- Returns a single bounded structured peer result. A response that is not the expected
  strict JSON object is rejected fail-closed by `parse_peer_result`.
- Persists the opaque external session reference through `ExternalRuntimeBinding`
  keyed by canonical task + attempt, so the reference is an external handle and never
  replaces the task board.

## Runtime quirks

- Some ACP runtimes answer the `initialized` notification with JSON-RPC `-32601`.
  The shared driver must tolerate the runtime profile's observed behavior rather than
  inferring it from generic ACP documentation.
- Authentication is an environment state, not a product feature: when the runtime
  reports it is unprepared, `am doctor` reports
  `RUNTIME_PREPARATION_REQUIRED` without starting a login flow.
