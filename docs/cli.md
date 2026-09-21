# CLI reference

`am` is the only first-class user command. The normal path is `init` → `agent add` →
`doctor` → `run` → `status` / `final` / `artifact` / `tui`. Compatibility and low-level
commands keep their established spellings; `am advanced` lists them.

```text
usage: am <init|agent|doctor|run|status|events|final|artifact|tui|advanced> [fields]
       am init [PATH]
       am agent add <id> --role <reasoner|worker|utility> --adapter <acp|codex-app-server|codex-exec|claude-cli> [--name NAME] [--concurrency N] [--tag TAG] [--artifact RELPATH] [--max-events N] -- <program> [arg ...]
       am agent list [--json]
       am agent remove <id>
       am doctor [--verbose] [--json]
       am run [--quiet] [--json] "<objective...>"
       am status [<run-id>] [--all] [--json]
       am events [<task-or-run-id>] [--json] [--follow]
       am final [<run-id>] [--json]
       am artifact [<task-id>] [--json]
       am tui [<database>]
       am advanced
```

`am --version` prints the binary's own version (`am <version>`).

## Commands

| Command | Effect |
|---|---|
| `init` | Create the project's durable state at the project root, or report that it already exists. Prints the Lead/Worker next steps. |
| `agent add` | Persist an Agent identity, role, adapter and opaque LaunchSpec argv. Reports `registered agent <id>`, or `updated agent <id>` when the id already existed. |
| `agent list` | Render the project registry as a role-first table, or one JSON object with `--json`. |
| `agent remove <id>` | Remove one Agent from the project registry. |
| `doctor` | Decide whether the project and team can run. The default report is decision-oriented: `project`, `lead`, `worker` and `team` lines, then `Ready to run.` or a `Reason` and a `Fix`. `am doctor --verbose` adds the bounded per-Agent diagnostic stages (`PROGRAM_FOUND`, `LAUNCHSPEC_VALID`, `SPAWN_OK`, `PROTOCOL_OK`, `SESSION_OK`, `READY`, or the first bounded failure, `PROGRAM_NOT_FOUND` or `PROTOCOL_UNAVAILABLE`); `--json` prints the decision as one object. It never authenticates. |
| `run` | Discover project state and run the team. Progress and lifecycle go to stderr; stdout carries the final answer alone. `--quiet` drops routine progress, `--json` prints one object instead of human text. |
| `status [<run-id>]` | Show the current run task by task, or one run by id, or every run with `--all`. Needs no database path. |
| `events [<task-or-run-id>]` | Project the durable normalized runtime observations of the current run or one task, with `--json` for one machine object and `--follow` to keep polling. Needs no database path. |
| `final [<run-id>]` | Print the durable final answer of the current run, or of one run by id. Needs no database path. |
| `artifact [<task-id>]` | Print recorded artifact paths and whole hashes for the current run, or for one task by id. Needs no database path. |
| `tui` | Live read-only terminal dashboard; `q` exits. Refreshes the durable board and reloads the registry while it runs. |
| `advanced` | List the compatibility and low-level commands below. |
| `register` | Insert/update one Agent registry row. |
| `registry` | List persisted Agent registrations. |
| `run-team` | Run one objective through the whole team. |
| `resume-team` | Continue a durable root's own Lead on a new attempt, without replaying completed work. |
| `run-acp` | Drive one bounded ACP worker task. |
| `continue-acp` | Continue an existing ACP worker session. |
| `submit` | Create one pending board task (no team run). |
| `cancel` | Cancel a task. |
| `override` | Reassign a non-root task to an explicit agent. |
| `recover` | Close one interrupted attempt. |
| `recover-all` | Close all interrupted attempts. |
| `resume` | Reopen a task for scheduling. |
| `binding` | Print the external runtime binding for a task attempt. |

## Normal onboarding

```bash
am init
am agent add lead --role reasoner --adapter codex-app-server -- codex
am agent add worker --role worker --adapter acp -- qwen --acp
am doctor
am run "complete the objective"
```

`am init` reports whether it created the state or found it already initialized, and prints
the Lead/Worker next steps. Tokens, provider selection, models, endpoints and launcher
profiles are owned by the external runtime. Everything after `--` is stored as opaque
argv; do not put credentials in it.

`doctor` initializes each configured adapter and opens only its smallest safe readiness
session; it sends no task prompt and never opens a login flow. A normal `am run` requires
exactly one `reasoner` and at least one `worker`. `utility` Agents are optional; utility
work falls back to the Worker tier when none is registered.

`am agent list` prints the registry as a role-first table, `am agent remove <id>` deletes
one Agent, and local launcher flags remain opaque LaunchSpec argv.

The launch command registered for `codex-app-server` must remain valid when AgentMosaic
appends `app-server --stdio`. Named Codex profiles are not currently a portable
app-server configuration mechanism; use app-server-compatible `-c` overrides, or a
wrapper that expands to them. When a real run hits the default event bound, tune it with
`--max-events N` (see [Tuning the Codex event budget](#tuning-the-codex-event-budget)).

```bash
am agent add lead --role reasoner --adapter codex-app-server -- codex
am agent add utility --role utility --adapter acp -- <program> --acp
```

### Tuning the Codex event budget

`--max-events N` is an advanced tuning option for high-event Codex backends. It sets how
many lifecycle events one Codex turn may spend before the turn is abandoned — the same
budget the Lead brain and the Codex team driver read from the persisted Agent
configuration. It is meaningful only for `--adapter codex-app-server`; `am agent add`
refuses it for `acp`, and refuses `0`. The default is unchanged, so register it only when
a real run has hit the bound:

```bash
am agent add lead --role reasoner --adapter codex-app-server --max-events 4000 -- codex
```

## Output streams

`am run` keeps the two streams separate: progress and lifecycle go to stderr, and stdout
carries the final answer alone, so `am run "..." > answer.txt` captures the answer and
nothing else.

```bash
am run "complete the objective" > answer.txt   # answer on stdout, progress on stderr
am run --quiet "complete the objective"        # no routine progress or next-step footer
am run --json "complete the objective"         # one JSON object on stdout, no human text
```

## Inspect a run

Inspection is project-aware: run it anywhere inside the project and it finds the durable
state itself, with no database path.

```bash
am status          # the current run, task by task
am status --all    # every run of this project, newest first
am status 1        # one run by id
am final           # the durable final answer
am final 1         # one run by id
am artifact        # recorded artifact paths and hashes
am artifact 2      # one task by id
am tui             # live read-only team board; q exits
```

## Machine-readable output

On success every `--json` invocation prints exactly one JSON object on stdout and nothing
else there; a failure writes its human diagnostic to stderr and exits non-zero:

```bash
am run --json "<objective>"
am doctor --json
am agent list --json
am status --json          # or: am status 1 --json
am status --all --json
am final --json           # or: am final 1 --json
am artifact --json        # or: am artifact 2 --json
```

`am run --json` reports the durable root, the Lead, the terminal status, the whole answer,
and the exact task and artifact references the answer was grounded in:

```json
{"run_id":1,"lead_agent":"lead","status":"succeeded","answer":"lead synthesized final answer","task_refs":[2],"artifact_refs":[{"task_id":2,"path":"result.txt","sha256":"5656fafa00d4f294bcb606cf4f7d4fa877390e46f583e8b3c8744ace104a31d1"}]}
```

Values are whole on this surface: digests are never abbreviated and long text is never
cut, unlike the human rendering.

## Compatibility commands

`am advanced` lists the compatibility and low-level commands. They keep their established
top-level spellings and stay callable at those spellings, but they are not part of the
normal onboarding path and they do not appear in `am --help`:

```text
am register <database> <id> <name> <tier> <driver_kind> <executable> [argv...] <concurrency> <tags> <driver_config>
am registry <database> [limit]
am run-acp <database> <task-id> <agent-id> <working-directory> <auth-method|-> <timeout-seconds> [artifact-paths]
am continue-acp <database> <task-id> <agent-id> <source-task-id> <working-directory> <auth-method|-> <timeout-seconds>
am run-team <database> <repo> "<objective>" [--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]
am resume-team <database> <repo> <root-task-id> [--lead <agent-id>] [--max-rounds N] [--max-tasks N] [--max-retries N]
am submit <database> <kind> "<objective>"
am cancel <database> <task>
am override <database> <task> <agent>
am recover <database> <task>
am recover-all <database>
am resume <database> <task>
am binding <database> <task> [attempt]
```

These take an explicit state database path as `<database>`. The project-aware `status`,
`final`, `artifact` and `tui` accept the same path in their first positional argument and
keep their legacy whole-board meaning, so existing scripts keep working.

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
