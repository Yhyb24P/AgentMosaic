# Codex runtime

Codex is the reference high-intelligence Lead. It runs through the
`codex-app-server` driver kind.

## Registration

An Agent is registered with driver kind `codex-app-server` and a non-secret driver
config JSON object:

```json
{
  "mcp_command": "/abs/path/to/target/release/am-codex-mcp",
  "model": "gpt-5.5",
  "max_events": 200,
  "artifact_paths": ["lead-final.txt"],
  "overrides": ["model=\"gpt-5.5\"", "model_reasoning_effort=\"low\""]
}
```

- `mcp_command` is required and must point at the built `am-codex-mcp` bridge.
- `overrides` are Codex config overrides; the driver appends the MCP bridge wiring for
  the run.
- When the Agent is the run's Lead, `model`, `max_prompt_bytes` and `max_answer_bytes`
  are also read.

## How it runs

The driver spawns a resident `codex app-server` and keeps one thread across planning,
follow-up and synthesis. It persists the external thread/turn references through the
durable task board (`ExternalRuntimeBinding`), so a resumed run re-attaches instead of
replaying.

The Lead's decisions are strict JSON validated by the product; a decision that does not
match `contracts/lead_decision.schema.json` fails closed after at most one bounded
correction turn. A Lead brain failure propagates and the root task cannot become
`succeeded`.

## The `am-codex-mcp` bridge

`am-codex-mcp` is a narrow stdio MCP bridge. The driver wires it in as the MCP server
`agentmosaic` with per-task environment:

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
