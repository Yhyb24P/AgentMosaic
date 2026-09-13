# Getting started

AgentMosaic connects Agents with different strengths to one project. One objective in,
one durable team result out.

## Build

```bash
cargo build --release --workspace
```

This produces the two shipping binaries:

- `target/release/am` — the only first-class user command.
- `target/release/am-codex-mcp` — the Codex app-server MCP bridge used by the Codex Lead.

The read-only board dashboard is reached through `am tui <database>`; there is no
separate public TUI binary.

## Register Agents

AgentMosaic is driven by a durable registry. Register the reference Codex Lead, the Qwen
Code Worker, and a utility agent:

```bash
# 1. Codex Lead (reference Reasoner, codex-app-server driver)
am register ./team.db codex-lead codex-lead reasoner \
  codex-app-server codex - 1 codex,lead - \
  '{"mcp_command":"/abs/path/to/target/release/am-codex-mcp","model":"gpt-5.5","max_events":200,"overrides":["model=\"gpt-5.5\"","model_reasoning_effort=\"low\""]}'

# 2. Qwen Code Worker and a utility agent (ACP driver)
am register ./team.db qwen-worker qwen-worker worker \
  acp qwen --acp 1 - - \
  '{"auth_method":"openai","timeout_seconds":600,"artifact_paths":["worker.txt"]}'
am register ./team.db qwen-utility qwen-utility utility \
  acp qwen --acp 1 - - '{"auth_method":"openai","timeout_seconds":600}'
```

`mcp_command` must be the absolute path of the built `am-codex-mcp` bridge. The optional
10th field is one non-secret JSON object of driver options; a key that looks like a
credential (`token`, `key`, `secret`, `password`, `endpoint`) is refused.

## Run one objective

```bash
am run-team ./team.db /path/to/repo "produce worker.txt and summarize it"
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
