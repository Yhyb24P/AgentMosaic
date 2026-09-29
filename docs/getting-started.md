# Getting started

AgentMosaic connects Agents with different strengths to one project. One objective in,
one durable team result out.

## Install

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://am.yhshyp.xyz/install.sh | sh
```

```bash
am --version
```

Current prebuilt target: Linux x86_64. To build the binary yourself instead, see
[Build from source](#build-from-source).

## Initialize project

```bash
am init
```

This creates project-local durable state under the Git root and adds `/.agentmosaic/` to
`.gitignore`. Commands run anywhere inside the project discover that state automatically,
so no later command needs a database path.

## Register Lead and Worker

An external runtime retains its own login, credentials, provider, model and launcher
profile; AgentMosaic only stores the argv it should execute.

```bash
am agent add lead --role reasoner --adapter codex-exec -- codex
am agent add worker --role worker --adapter acp -- qwen --acp
```

Everything after `--` is persisted opaque argv. Do not put credentials in it. A local
launcher keeps its exact argv, but the launch command registered for `codex-exec` must
remain valid when AgentMosaic appends `exec --json`. `codex-exec` is the canonical/default
reference Lead. `--adapter` accepts `acp`, `codex-exec` or `claude-cli`.
A Utility Agent is optional; utility work falls back to Worker when none is registered.

## Doctor

```bash
am doctor
```

`doctor` reports one decision: `Ready to run.` when the project and team can run, or a
`Reason` plus a `Fix` when they cannot. It probes each configured adapter through its own
protocol without a task prompt or a login flow. A team run needs exactly one `reasoner`
registered as the Lead and will not guess.

```bash
am doctor --verbose    # add the bounded per-Agent diagnostic stages
am doctor --json       # print the decision as one JSON object
```

Verbose output gives each Agent one bounded token per stage: `PROGRAM_FOUND`,
`LAUNCHSPEC_VALID`, `SPAWN_OK`, `PROTOCOL_OK`, `SESSION_OK`, `READY`, or the first
failure, `PROGRAM_NOT_FOUND` or `PROTOCOL_UNAVAILABLE`.

## Run objective

```bash
am run "produce worker.txt and summarize it"
```

`am run` discovers the project state and runs the whole team: it opens the current-generation
board, loads the persisted agent registry, builds the validated registry, resolves the
Lead, constructs the real drivers, creates one durable root `reasoning` task plus its Lead
attempt, and drives the Lead through `Lead` + `Scheduler`. The Lead runs through
`codex-exec`, starting an external process per turn and persisting
its foreign thread binding. Delegated tasks run on the configured external workers.
The final answer and selected task/artifact references settle atomically on the root.

Progress and lifecycle go to stderr, so stdout carries the final answer alone:

```bash
am run "produce worker.txt and summarize it" > answer.txt
am run --quiet "produce worker.txt and summarize it"   # no routine progress
am run --json "produce worker.txt and summarize it"    # one JSON object on stdout
```

## Inspect a run

Inspection is project-aware, so none of these needs a database path:

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

Add `--json` to any of them for exactly one JSON object on stdout:

```bash
am status --all --json
am final --json
am artifact --json
```

Inspection never starts a driver. To resume an interrupted run, see [Recovery](recovery.md).
To upgrade an older project database, see [Database import](migration.md).

## Build from source

```bash
cargo build --release --workspace
```

This produces `target/release/am`, the only shipped product binary.

## Next

- [Architecture](architecture.md)
- [CLI reference](cli.md)
- [Recovery](recovery.md)
- [Codex runtime](runtimes/codex.md)
- [Qwen Code runtime](runtimes/qwen-code.md)
- [ACP boundary](runtimes/acp.md)
