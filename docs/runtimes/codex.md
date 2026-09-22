# Codex runtime

Codex is the reference high-intelligence Lead, reached through two driver kinds:

- `codex-exec` — the default, driving the stable `codex exec --json` machine interface
  (worker and Lead).
- `codex-app-server` — the resident `codex app-server --stdio` bridge, kept as the
  compatibility runtime (worker and Lead) with the internal
  `am __internal codex-mcp` MCP bridge.

## Registration

An Agent is registered with the project-aware command and its opaque external
launch argv:

```bash
am agent add lead --role reasoner --adapter codex-exec -- codex
```

`codex-exec` is the canonical/default Lead runtime. Register the compatibility
Lead runtime with `--adapter codex-app-server`; its launch command must additionally
remain valid when AgentMosaic appends `app-server --stdio`.

AgentMosaic does not select a Codex model, account, provider, credentials or
launcher profile. Existing v11 records containing `mcp_command` remain readable,
but new product paths do not require or write it.

## How it runs

`codex-exec` drives the stable `codex exec --json` machine interface and supplies each
prompt on stdin, so it never leaks a prompt into argv. It starts one `codex exec` process
per Lead turn and resumes the foreign thread by id (`codex exec resume --json <thread>`),
which is why it carries no resident-process requirement.

`codex-app-server` drives a resident `codex app-server --stdio` process and keeps one
thread across planning, follow-up and synthesis.

Both persist the external thread/turn references through the durable task board
(`ExternalRuntimeBinding`), so a resumed run re-attaches instead of replaying.

The Lead's decisions are strict JSON validated by the product; a decision that does not
match `contracts/lead_decision.schema.json` fails closed after at most one bounded
correction turn. A Lead brain failure propagates and the root task cannot become
`succeeded`.

## Internal MCP bridge

`am __internal codex-mcp` is a fixed narrow stdio MCP bridge used by the
`codex-app-server` compatibility runtime. The normal CLI
injects its own executable as the bridge host; users never configure a second
binary. The driver wires it in as the MCP server `agentmosaic` with per-task
environment:

```text
AGENTMOSAIC_DB          the board database path
AGENTMOSAIC_TASK_ID     the bounded task id
AGENTMOSAIC_ATTEMPT     the attempt number
AGENTMOSAIC_BRIDGE_LOG  optional bridge audit log path
```

It exposes exactly two allowlisted tools:

- `agentmosaic_request_context` — request bounded team/context results.
- `agentmosaic_request_help` — request bounded team help.

Tool arguments cannot select task or runtime identity; the bridge accepts identity only
from the MCP request correlation id. The Codex app-server elicitation path accepts only
this configured `agentmosaic` MCP server.
