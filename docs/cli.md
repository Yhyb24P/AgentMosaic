# CLI reference

`am` is the only first-class user command. Command semantics are unchanged from the
product line's previous executable name; only the identity changed.

```text
usage: am <init|agent|doctor|run|register|registry|run-acp|continue-acp|run-team|resume-team|submit|status|cancel|override|recover|recover-all|resume|artifact|binding|final|tui> [fields]
       am init [PATH]
       am agent add <id> --role <reasoner|worker|utility> --adapter <acp|codex-app-server> [--name NAME] [--concurrency N] [--tag TAG] [--artifact RELPATH] -- <program> [arg ...]
       am agent list
       am doctor
       am run "<objective...>"
       am run-team <database> <repo> "<objective...>" [--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]
       am resume-team <database> <repo> <root-task-id> [--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]
       am tui <database>
```

`am --version` prints `am 0.2.1`.

## Commands

| Command | Effect |
|---|---|
| `init` | Create/open `.agentmosaic/state.db` at the project root. |
| `agent add` | Persist an Agent identity, role, adapter and opaque LaunchSpec argv. |
| `agent list` | Render the discovered project registry. |
| `doctor` | Decide whether the project and team can run. The default report is decision-oriented: `project`, `lead`, `worker` and `team` lines, and — when it is not ready — a `Reason` and a `Fix`. `am doctor --verbose` adds the bounded diagnostic stages (`PROGRAM_FOUND`, `LAUNCHSPEC_VALID`, `SPAWN_OK`, `PROTOCOL_OK`, `SESSION_OK`, `READY`, or the first bounded failure). It never authenticates. |
| `run` | Discover project state and delegate to the existing TeamRunner. |
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

## Normal onboarding

```bash
am init
am agent add lead --role reasoner --adapter codex-app-server -- codex
am agent add worker --role worker --adapter acp -- qwen --acp
am doctor
am run "complete the objective"
```

Tokens, provider selection, models, endpoints and launcher profiles are owned
by the external runtime. Everything after `--` is stored as opaque argv; do
not put credentials in it.
`doctor` initializes each configured adapter and opens only its smallest safe
readiness session; it sends no task prompt and never opens a login flow.

A normal `am run` requires exactly one `reasoner` and at least one `worker`.
`utility` Agents are optional; utility work falls back to the Worker tier when
none is registered.

Local launcher flags remain opaque LaunchSpec argv. For example:

```bash
am agent add lead-ds --role reasoner --adapter codex-app-server -- codex -ds
am agent add utility --role utility --adapter acp -- aweswitch qw --acp
```

## Advanced compatibility: register grammar

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
  - legacy ACP `auth_method` remains readable for existing boards but is not
    normal onboarding.
  - legacy `mcp_command` remains readable for existing boards. New product
    runs inject `am __internal codex-mcp` and do not require a helper binary.

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
