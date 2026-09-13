# Qwen Code runtime

Qwen Code is the reference Worker. It runs through the `acp` driver kind
(`AcpWorkerDriver`), which speaks the Agent Client Protocol over stdio.

## Registration

```json
{
  "auth_method": "openai",
  "timeout_seconds": 600,
  "max_prompt_bytes": 4096,
  "max_result_bytes": 4096,
  "artifact_paths": ["worker.txt"]
}
```

- `auth_method` — when set, the driver sends an ACP `authenticate` request with that
  method id before `session/new`. Without it, a bare launch may be rejected by the
  runtime as unauthenticated.
- `artifact_paths` — paths relative to the task working directory; the driver records
  their hashes with the result rather than copying file bodies.

## What the driver does

- Starts the executable with the registered driver args (for example `qwen --acp`).
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
  reports it is unauthenticated, the bounded turn cannot proceed and the driver reports
  that failure instead of retrying indefinitely.
