# CLI reference

`am` is the only first-class user command. Command semantics are unchanged from the
product line's previous executable name; only the identity changed.

```text
usage: am <register|registry|run-acp|continue-acp|run-team|resume-team|submit|status|cancel|override|recover|recover-all|resume|artifact|binding|final|tui> <database> [fields]
       am run-team <database> <repo> "<objective...>" [--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]
       am resume-team <database> <repo> <root-task-id> [--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]
       am tui <database>
```

`am --version` prints `am 0.2.0-dev`.

## Commands

| Command | Effect |
|---|---|
| `register` | Insert/update one Agent registry row. |
| `registry` | List persisted Agent registrations. |
| `run-team` | Run one objective through the whole team. |
| `resume-team` | Re-drive a durable root task without replaying completed work. |
| `run-acp` | Drive one bounded ACP worker task. |
| `continue-acp` | Continue an existing ACP worker session. |
| `submit` | Create one pending board task (no team run). |
| `status` | Print each task with status, assignee, attempts and `parent=<id|->`. |
| `cancel` | Cancel a task. |
| `override` | Reassign a task to an explicit agent. |
| `recover` | Close one interrupted attempt. |
| `recover-all` | Close all interrupted attempts. |
| `resume` | Reopen a task for scheduling. |
| `artifact` | Print a task's artifact path and hash. |
| `binding` | Print the external runtime binding for a task attempt. |
| `final` | Print the durable final result for a root task. |
| `tui` | Read-only terminal dashboard; `q` exits. |

## register grammar

8 to 10 trailing fields:

```text
am register <database> <agent-id> <name> <tier> <driver-kind> <executable> <driver-args> <max-concurrency> <tags> [<runtime-version-or->] [<driver-config-json-or->]
```

- `tier` is `reasoner`, `worker`, or `utility`.
- `driver-kind` is `native`, `acp`, `cli`, `codex-app-server`, or `-`.
- `driver-args` and `tags` are comma-separated; use `-` for none.
- `runtime-version` is the optional 9th field; use `-` for none.
- the optional 10th field is one non-secret JSON object of driver options, or `-`.
  A key that looks like a credential (`token`, `key`, `secret`, `password`,
  `endpoint`) is refused, so provider credentials can never be stored here.
  - `acp`: `auth_method`, `timeout_seconds`, `max_prompt_bytes`, `max_result_bytes`,
    `artifact_paths`.
  - `codex-app-server`: `mcp_command` (required: an existing file, the built
    `am-codex-mcp` binary), `artifact_paths`, `max_events`, `overrides`. When this
    agent is the run's Lead it also reads `model`, `max_prompt_bytes`, and
    `max_answer_bytes`.

Keep site-local launcher aliases out of this database.

## run-team flags

```text
--lead <agent-id>   select the Lead when more than one reasoner is registered
--max-rounds N      bound the Lead's reasoning rounds
--max-tasks N       bound the delegated task budget
--max-retries N     bound per-agent retries before the scheduler reassigns
```

`submit` alone only creates a pending board task; `run-team` is the team entrypoint.
The Lead's decisions are strict JSON validated by the product, and a decision that does
not match the contract fails closed after at most one bounded correction turn. The wire
is checked in at `contracts/lead_decision.schema.json`.
