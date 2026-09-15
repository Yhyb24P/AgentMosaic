# Project instructions

## Positioning

`AgentMosaic` (short name `AM`) is a heterogeneous Agent coding/work team.

The one job: connect Agents with different strengths to one project. High-intelligence
Agents do planning, hard reasoning, architecture, synthesis and review. Local or cheap
Agents and deterministic workers do repetitive, long-running, file-heavy, data-heavy and
tool-heavy work. Results and artifacts flow back automatically to the Agent that
continues the reasoning, with no manual copy/paste between Agents.

Communication, scheduling, recovery and safety boundaries are supporting mechanics that
let several Agents finish work. They are not the product.

AgentMosaic does not implement a model client or a tool loop of its own. Every Agent
executes inside an external runtime (a Codex CLI, a Claude CLI or any ACP peer) reached
through a registered runtime adapter; AgentMosaic owns the durable team layer around it.

## Identity

```text
Brand              AgentMosaic / AM
Public CLI         am
Cargo prefix       agentmosaic-
Rust import prefix agentmosaic_
Internal bridge    am __internal codex-mcp (not a separate binary)
Config namespace   agentmosaic
Env prefix         AGENTMOSAIC_
Development       0.5.0-dev
SQLite schema      12
```

The only first-class user command is `am`. Do not reintroduce retired names or aliases
(former brand, repository, branch, former crate and import prefixes, former executables,
or the former Codex helper name). Historical identity belongs only in `docs/history.md`,
`CHANGELOG.md` and `docs/releases/v0.1.0.md`.

## Do not recreate as core

Do not build these back into the product:

- `PolicyEngine` / `ApprovalService`
- a mandatory independent Verifier
- IQ/DQ/RQ qualification
- backup/DR as a product subsystem
- the `WorkOrder + Attempt + Delegation + Invocation` quartet
- trust-zone / capability / audit systems as product identity

Narrow runtime mechanics that genuinely help an Agent finish work may survive, but they
are not the product.

## Engineering guards that remain

The runtime adapters keep the guards that make an external Agent safe to supervise:
absolute deadlines, process-group termination with reaping, bounded output, artifact
path containment and file hashes. The board keeps crash recovery and no-replay
settlement. They are runtime mechanics, not a control-plane product.

## Rust workspace

A Cargo workspace of small crates:

```text
Cargo.toml
crates/
  agentmosaic-storage/    # SQLite task board, agent registry, runtime events, schema migration
  agentmosaic-runtime/    # external runtime adapters (ACP, Codex, Claude), Lead brains, TeamRunner
  agentmosaic-team/       # Agent registry, lead, task board, scheduling, result flow
  agentmosaic-tui/        # ratatui/crossterm read-only board view
  agentmosaic-cli/        # the public `am` command
```

The workspace holds the durable team layer and the adapters for those external runtimes;
it holds no Agent implementation of its own.

Do not rename persisted data or protocol identifiers: SQLite schema stays v12, and
task/result/artifact/final-reference semantics, `TaskKind` and `DriverKind` wire strings,
and Lead decision JSON fields are stable.

Required Rust CI:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --release --workspace
git diff --check
```

Also required: `scripts/ci/check_identity.sh` (retired-identity gate).

## Structure

- `crates/`: the Rust workspace.
- `contracts/`: checked-in protocol contracts (Lead decision schema).
- `docs/`: current product documentation.
- `scripts/`: release tooling and qualification helpers.
