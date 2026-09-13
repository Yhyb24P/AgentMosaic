# Getting started

AgentMosaic connects Agents with different strengths to one project. One objective in,
one durable team result out.

## Build

```bash
cargo build --release --workspace
```

This produces `target/release/am`, the only shipped product binary. Its Codex
MCP bridge is a fixed internal command, never a separately installed binary.

## Initialize and add Agents

AgentMosaic keeps one project-local durable registry. An external runtime retains
its own login, credentials, provider, model and launcher profile.

```bash
am init
am agent add lead --role reasoner --adapter codex-app-server -- codex
am agent add worker --role worker --adapter acp -- qwen --acp
am agent add utility --role utility --adapter acp -- aweswitch qw --acp
am doctor
```

Everything after `--` is persisted opaque argv. Do not put credentials in it.
For a local custom launcher, preserve its exact argv, for example
`codex -ds` or `aweswitch qw --acp`.

## Run one objective

```bash
am run "produce worker.txt and summarize it"
```

`am run-team` opens/migrates the board, loads the persisted agent registry, builds the
validated registry, resolves the Lead, constructs the real drivers, creates one durable
root `reasoning` task plus its Lead attempt, and runs a resident Codex `CodexLeadBrain`
through `Lead` + `Scheduler`. Delegated tasks execute on real Qwen workers over ACP, and
the final visible Codex answer plus the exact selected task/artifact refs are persisted
on the root.

Without `--lead`, exactly one registered `reasoner` must exist; zero or several fails
rather than guessing.

## Read the result

```bash
am status ./team.db            # task/status lines with parent=<id|->
am registry ./team.db          # persisted Agent registry
am final ./team.db 1           # durable root answer
am artifact ./team.db 2        # exact artifact hash
am binding ./team.db 2         # external runtime binding
am tui ./team.db               # read-only dashboard; q exits
```

These read-only surfaces never start a driver or mutate runtime state.

## Next

- [CLI reference](cli.md)
- [Recovery](recovery.md)
- [Codex runtime](runtimes/codex.md)
- [Qwen Code runtime](runtimes/qwen-code.md)
- [ACP boundary](runtimes/acp.md)
