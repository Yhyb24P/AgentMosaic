# AgentMosaic

[简体中文](README.zh-CN.md)

[![CI](https://github.com/Yhyb24P/AgentMosaic/actions/workflows/rust.yml/badge.svg?branch=main)](https://github.com/Yhyb24P/AgentMosaic/actions/workflows/rust.yml)
[![Latest release](https://img.shields.io/github/v/release/Yhyb24P/AgentMosaic)](https://github.com/Yhyb24P/AgentMosaic/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

**Run heterogeneous coding agents as one durable team.**

AgentMosaic connects a high-reasoning Lead with coding agents, local models and
deterministic workers around one project. Give the team one objective: the Lead plans and
delegates, workers execute, and their results and artifacts flow back automatically for
review and synthesis.

Local models are reached through ACP-compatible runtimes; AgentMosaic does not host or
select models itself.

No manual copy/paste between Agents.

## Install

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://am.yhshyp.xyz/install.sh | sh
```

```bash
am --version
```

Prebuilt releases currently target Linux x86_64. To build the binary yourself instead,
see [Build from source](#build-from-source).

## Quickstart

```bash
am init

am agent add lead \
  --role reasoner \
  --adapter codex-exec -- codex

am agent add worker \
  --role worker \
  --adapter acp -- qwen --acp

am doctor

am run "implement the task, verify it, and summarize the result"
```

`am init` creates project-local durable state and keeps it out of version control. Every
`am` command discovers that state from anywhere inside the project.

Everything after `--` is opaque launch argv. AgentMosaic stores it and executes it
exactly; it never interprets launcher-specific flags, and credentials never belong there.

`am doctor` checks project, team and runtime readiness without authenticating anything and
returns one decision: `Ready to run.`, or a `Reason` with the `Fix`. A run needs exactly
one `reasoner` registered as the Lead. `am doctor --verbose` adds the bounded per-Agent
diagnostic stages.

### Optional: add a utility worker

A utility Agent is registered the same way and is used for bounded tool-heavy work.
It is optional: a normal team needs exactly one Reasoner and at least one Worker.
When no Utility is registered, utility work falls back to the Worker tier:

```bash
am agent add utility --role utility --adapter acp -- <program> --acp
```

A local launcher keeps its own argv, but the launch command registered for
`codex-exec` must remain valid when AgentMosaic appends `exec --json`, so `codex` on its
own is a valid launcher. The same `codex` binary also backs the `codex-app-server`
compatibility runtime, whose launch command must additionally remain valid when
AgentMosaic appends `app-server --stdio`; named Codex profiles are not currently a
portable app-server configuration mechanism, so use app-server-compatible `-c`
overrides, or a wrapper that expands to them:

```bash
am agent add lead --role reasoner --adapter codex-app-server -- codex   # compatibility Lead
```

`am agent add --max-events N` raises the per-turn Codex event budget when a real run has
hit the default bound.

## Why AgentMosaic?

Driving two agents by hand looks like this:

| Manual Agent workflow | AgentMosaic |
|---|---|
| Reasoning model plans | Give the team one objective |
| You copy the instructions into another Agent | The Lead delegates the work |
| The worker executes and you copy the result back | Workers execute and return results |
| The reasoning model reviews, then you repeat | The Lead follows up, then persists one durable result |

So the expensive reasoning model spends its budget on planning, hard reasoning and
synthesis, while coding agents, local models and deterministic workers take the
repetitive, long-running, file-heavy and tool-heavy work.

You stop being the transport between Agents, and an interrupted run stays inspectable
instead of being lost in a chat scroll.

## How it works

```text
                    one objective
                         |
                         v
                  Lead / Reasoner
                 /      |       \
                v       v        v
             Agent    Agent    Worker
                \       |       /
                 +-- results ---+
                         |
                         v
                 review / follow-up
                         |
                         v
                  durable result
```

The Lead does planning, hard reasoning, synthesis and review. Workers complete the tasks
it delegates to them. Every result and artifact lands on the durable board, so the Lead
can follow up, ask for a correction, or close the objective with one final answer.

## Runtime boundary

### AgentMosaic owns

```text
roles
delegation
task state
result / artifact flow
bounded contracts
recovery
```

### External runtimes own

```text
login
credentials
provider
model
launcher profile
```

ACP-compatible coding runtimes communicate with AgentMosaic over a bounded worker
boundary. The ACP driver takes one scheduler task and returns a bounded structured result
plus artifact hashes; the SQLite board remains the authoritative source of state.

Codex is the current reference high-reasoning Lead. `codex-exec` is the canonical/default
Lead runtime: it drives the stable `codex exec --json` machine interface and persists the
foreign thread, so a resumed run continues that thread. The `codex-app-server`
compatibility runtime keeps one resident `codex app-server` process whose thread stays
resident across planning, follow-up and synthesis; it persists the external thread/turn
binding the same way. Either way, any Agent or runtime that satisfies the same boundary
can take the Lead role.

## Durability and recovery

- Project-local SQLite state, not an in-memory session.
- Delegated tasks, results and artifacts are persisted as they happen.
- The Lead's decisions follow a strict, checked-in contract and fail closed.
- An interrupted run can be resumed without replaying work that already succeeded.
- Completed work is never unconditionally replayed.
- Inspection commands never start a runtime.

```bash
am status          # this project's current run, task by task
am status --all    # every run of this project, newest first
am final           # the durable final answer
am artifact        # recorded artifact paths and hashes
am tui             # live read-only team board; q exits
```

Inspection needs no database path: each command finds this project's durable state
itself. Add `--json` to get exactly one JSON object on stdout.

`am advanced` lists the compatibility and low-level commands, which keep their old
spellings; `am agent remove <id>` removes one Agent. See [Recovery](docs/recovery.md) and
the [CLI reference](docs/cli.md).

## Documentation

- [Getting started](docs/getting-started.md)
- [Architecture](docs/architecture.md)
- [CLI reference](docs/cli.md)
- [Recovery](docs/recovery.md)
- [Codex runtime](docs/runtimes/codex.md)
- [ACP runtime](docs/runtimes/acp.md)
- [Qwen Code runtime](docs/runtimes/qwen-code.md)
- [Status](docs/status.md)
- [Release history](docs/releases/v0.1.0.md) / [History](docs/history.md)

## Build from source

```bash
cargo build --release --workspace
```

This produces `target/release/am`, the only shipped product binary. Its Codex
collaboration bridge is a fixed hidden internal command; it is not separately installed
or configured.

Required checks for a change:

```bash
scripts/ci/check_identity.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --release --workspace
git diff --check
```

## License

Apache License 2.0 (ALv2). See the `LICENSE` file at the repository root.
