# AgentMosaic

[简体中文](README.zh-CN.md)

AgentMosaic (`AM`) is a **heterogeneous Agent coding/work team**. One objective goes in
and one durable team result comes out.

The one job: connect Agents with different strengths to one project. High-intelligence
Agents do planning, hard reasoning, architecture, synthesis and review. Local or cheap
Agents and deterministic workers do repetitive, long-running, file-heavy, data-heavy and
tool-heavy work. Results and artifacts flow back automatically to the Agent that
continues the reasoning, with no manual copy/paste between Agents.

Communication, scheduling, recovery and safety boundaries are supporting mechanics that
let several Agents finish work. They are not the product.

For the former product identity and the stable `v0.1.0` release, see
[docs/history.md](docs/history.md).

## Architecture

```text
User
  |
  v
Team Session
  |
  v
Lead / Reasoning Agent
  | delegate
  +------------------+-------------------+
  v                  v                   v
Reasoning Agent    Local Model Agent   Utility Worker
  |                  |                   |
  +----- result / files / messages ------+
                       |
                       v
              Lead integrates result
                       |
                       v
                    Deliver
```

Each native model-backed Agent runs the same internal loop:

```text
Init -> Observe -> Model Decision -> Tool Execution -> Observe -> ...
     -> Verify -> Deliver / Rollback
```

## Build

```bash
cargo build --release --workspace
```

This produces `target/release/am`, the only shipped product binary. The Codex
collaboration bridge is a fixed hidden internal command; it is not separately
configured or installed.

## Quickstart — one heterogeneous team objective

One objective in, one durable team result out. AgentMosaic owns roles,
machine protocols and LaunchSpecs; each external runtime owns its login,
credentials, provider, model and launcher profile.

```bash
# 1. initialize project-local durable state
am init

# 2. register opaque external launch argv (do not put credentials here)
am agent add lead --role reasoner --adapter codex-app-server -- codex -ds
am agent add worker --role worker --adapter acp -- qwen -ds --acp
am agent add utility --role utility --adapter acp -- aweswitch qw --acp

# 3. check project and protocol preparation without managing authentication
am doctor

# 4. run one objective through the whole team
am run "produce worker.txt and summarize it"
```

Flags accepted by both `run-team` and `resume-team`:

```text
--lead <agent-id>   select the Lead when more than one reasoner is registered
--max-rounds N      bound the Lead's reasoning rounds
--max-tasks N       bound the delegated task budget
--max-retries N     bound per-agent retries before the scheduler reassigns
```

`submit` alone only creates a pending board task; `run-team` is the team entrypoint.
Everything operates directly on the authoritative SQLite board.

## How a team run works

`am run-team` opens/migrates the board, loads the persisted agent registry, builds the
validated registry, resolves the Lead, constructs the real drivers, creates one durable
root `reasoning` task plus its Lead attempt, and runs a resident Codex `CodexLeadBrain`
through `Lead` + `Scheduler`. Delegated tasks execute on real Qwen workers over ACP, and
the final visible Codex answer plus the exact selected task/artifact refs are persisted
on the root. `am resume-team` rebuilds the drivers/brain from durable state, closes
interrupted descendants without replaying them, and is idempotent on an already-succeeded
root.

The Lead's decisions are strict JSON validated by the product; a decision that does not
match the contract fails closed after at most one bounded correction turn. The decision
wire is checked in at
[`contracts/lead_decision.schema.json`](contracts/lead_decision.schema.json).

The read-only commands `status`, `registry`, `artifact`, `binding`, `final`, and the `tui`
dashboard never start a driver or mutate runtime state.

## Documentation

- [Architecture](docs/architecture.md)
- [Getting started](docs/getting-started.md)
- [CLI reference](docs/cli.md)
- [Recovery](docs/recovery.md)
- [Runtimes](docs/runtimes/acp.md): [Codex](docs/runtimes/codex.md), [Qwen Code](docs/runtimes/qwen-code.md)
- [History](docs/history.md)

## Rust development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --release --workspace
git diff --check
```

## License

Apache License 2.0 (ALv2). See the `LICENSE` file at the repository root.
