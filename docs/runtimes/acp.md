# ACP boundary

`AcpWorkerDriver` is the shared boundary for external coding CLIs that speak the Agent
Client Protocol over stdio. It is a bounded worker boundary, not a second tool loop.

## Contract

- **In** — one scheduler task with an objective, bounded context, a working directory,
  and a resolved executable + args.
- **Out** — one bounded structured peer result: a strict JSON summary plus the hashes of
  the configured relative artifacts. Nothing outside that shape is accepted; a
  non-conforming response fails closed.
- **Identity** — the opaque external session reference is stored in
  `ExternalRuntimeBinding` keyed by canonical task + attempt. It stays an external
  reference and never becomes the authority for task state.
- **Authority** — the durable `SqliteTaskBoard` remains the single source of truth for
  task, attempt, artifact and final-reference state.

## Lifecycle

```text
spawn executable -> initialize -> [authenticate] -> session/new
  -> session/prompt -> collect bounded result -> persist binding + artifacts
```

A follow-up reuses the live session when the runtime supports it. Cancellation and
interrupt are driven through the same driver boundary.

## Persisted wire values

The driver kind string for this boundary is `acp` and is persisted verbatim in the
registry and bindings. Renaming the product does not rename persisted protocol or wire
strings.
