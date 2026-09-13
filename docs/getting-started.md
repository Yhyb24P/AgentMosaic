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

This creates project-local durable state at `.agentmosaic/state.db` under the Git root
and adds `/.agentmosaic/` to `.gitignore`. Commands run anywhere inside the project
discover that state automatically.

## Register Lead and Worker

An external runtime retains its own login, credentials, provider, model and launcher
profile; AgentMosaic only stores the argv it should execute.

```bash
am agent add lead --role reasoner --adapter codex-app-server -- codex
am agent add worker --role worker --adapter acp -- qwen --acp
```

Everything after `--` is persisted opaque argv. Do not put credentials in it. A local
custom launcher keeps its exact argv, for example `codex -ds` or `aweswitch qw --acp`.

`--adapter` accepts `acp` or `codex-app-server`. A `utility` Agent is registered the same
way with `--role utility`.

## Doctor

```bash
am doctor
```

`doctor` reports the project and schema state, probes each configured adapter through its
own protocol without a task prompt or a login flow, and prints the team tier counts. It
reports `LEAD_SELECTION_AMBIGUOUS_OR_MISSING` unless exactly one `reasoner` is
registered; a team run needs that single Lead and will not guess.

## Run objective

```bash
am run "produce worker.txt and summarize it"
```

`am run` discovers the project state and runs the whole team: it opens/migrates the
board, loads the persisted agent registry, builds the validated registry, resolves the
Lead, constructs the real drivers, creates one durable root `reasoning` task plus its Lead
attempt, and runs a resident Codex `CodexLeadBrain` through `Lead` + `Scheduler`.
Delegated tasks execute on real Qwen workers over ACP, and the final visible Codex answer
plus the exact selected task/artifact refs are persisted on the root.

## Inspect durable result

```bash
am status .agentmosaic/state.db    # task/status lines with parent=<id|->
am registry .agentmosaic/state.db  # persisted Agent registry
am final .agentmosaic/state.db 1   # durable root answer
am artifact .agentmosaic/state.db 2  # exact artifact hash
am binding .agentmosaic/state.db 2   # external runtime binding
am tui .agentmosaic/state.db       # read-only dashboard; q exits
```

These read-only surfaces never start a driver or mutate runtime state. To resume an
interrupted run, see [Recovery](recovery.md).

## Build from source

```bash
cargo build --release --workspace
```

This produces `target/release/am`, the only shipped product binary. Its Codex MCP bridge
is a fixed internal command, never a separately installed binary.

## Next

- [Architecture](architecture.md)
- [CLI reference](cli.md)
- [Recovery](recovery.md)
- [Codex runtime](runtimes/codex.md)
- [Qwen Code runtime](runtimes/qwen-code.md)
- [ACP boundary](runtimes/acp.md)
