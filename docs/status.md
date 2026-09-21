# Current status

One page of present tense facts. Point-in-time engineering evidence lives in
`docs/audits/` and is never rewritten to match today; this page is the one answer for
what the product is now.

## Versions

| Item | Value |
|---|---|
| Current stable release | `v0.3.0` (latest GitHub Release) |
| Current development version | `0.5.0-dev` (workspace `Cargo.toml`) |
| SQLite schema version | `12` |
| Public CLI | `am` (the only shipped product binary) |
| Prebuilt platform | Linux `x86_64` (`x86_64-unknown-linux-gnu`), shell installer |

`main` has moved well past the `v0.3.0` release; no newer tag has been cut. A release is a
separate, explicitly authorized step.

## Supported runtimes

Lead (reasoner):

| Adapter | Status | Interface |
|---|---|---|
| `codex-exec` | **canonical / default** | stable `codex exec --json` machine interface |
| `codex-app-server` | compatibility | resident `codex app-server --stdio` |

Worker / utility:

| Adapter | Interface |
|---|---|
| `acp` | ACP **v1** wire protocol (any conforming peer, e.g. Qwen Code) |
| `codex-exec` | `codex exec --json` |
| `codex-app-server` | `codex app-server --stdio` |
| `claude-cli` | Claude CLI `stream-json` |

ACP v1 is the wire protocol. The workspace dependency `agent-client-protocol = 2.1.0` is a
*package* version and is **not** "ACP v2"; see [runtimes/acp.md](runtimes/acp.md).

Login, credentials, provider, model and launcher profile are owned by the external
runtime. AgentMosaic stores only opaque launch argv and never selects a model.

## Canonical normal path

```text
am init
am agent add lead   --role reasoner --adapter codex-exec -- codex
am agent add worker --role worker   --adapter acp -- qwen --acp
am doctor
am run "<objective>"
am status / am final / am artifact / am tui
```

The normal path is first class; the compatibility commands (`register`, `run-team`,
`resume-team`, `recover`, `binding`, ...) keep their spellings but are hidden from
`am --help` and listed by `am advanced`.

## Durable invariants

- SQLite task board is the authoritative state; runtime events are observation only.
- Attempt history is append-only; a resume appends a new root attempt and never rewrites a
  finished one.
- A run's root keeps its durable Lead; a successful descendant is never replayed.
- The final answer is grounded in real succeeded tasks and artifacts with full path,
  owner task and SHA-256.
- Schema stays v12; `TaskKind` / `DriverKind` and the Lead decision wire are stable.

## Known limitations

- Prebuilt distribution is Linux `x86_64` only; other platforms build from source.
- Live qualification (real Codex + Qwen) needs already-authenticated runtimes and is not
  run by CI, which has no model credentials.
- `codex-app-server` is a compatibility runtime, not the default.

## Release qualification

See [audits/V0_5_TEAM_RUNNER_RC.md](audits/V0_5_TEAM_RUNNER_RC.md) for the point-in-time
evidence. `PUBLIC_RELEASE_READY` is always `false` until a tag / GitHub Release is
explicitly authorized; it is not a technical-gate failure.
