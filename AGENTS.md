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

The native Rust Coding Agent is the execution engine for model-backed Agents. External
Agents (Codex/Claude-style CLIs) plug in through drivers.

## Identity

```text
Brand              AgentMosaic / AM
Public CLI         am
Cargo prefix       agentmosaic-
Rust import prefix agentmosaic_
Codex helper       am-codex-mcp
Config namespace   agentmosaic
Env prefix         AGENTMOSAIC_
Development       0.2.0-dev
SQLite schema      11
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

Path containment, command timeout, process-group termination, output truncation, atomic
writes, file hashes, Git checkpoints and crash recovery stay. They make a Coding Agent
reliable. They are runtime mechanics, not a control-plane product.

## Rust workspace

A Cargo workspace of small crates:

```text
Cargo.toml
crates/
  agentmosaic-core/       # session state machine, Agent loop, events, recovery
  agentmosaic-model/      # async model client (OpenAI-compatible HTTP first)
  agentmosaic-tools/      # the five atomic tools
  agentmosaic-workspace/  # project rules, Git worktree/checkpoint, path handling, diff/rollback
  agentmosaic-context/    # context budget, truncation, compaction, repository map
  agentmosaic-storage/    # small SQLite journal
  agentmosaic-runtime/    # native Agent loop, external drivers (Codex app-server/ACP), product TeamRunner
  agentmosaic-team/       # Agent registry, lead, task board, scheduling, result flow
  agentmosaic-tui/        # ratatui/crossterm read-only board view
  agentmosaic-cli/        # the public `am` command
```

Five atomic tools: `view_file`, `edit_file`, `write_file`, `search_dir`,
`execute_command`. `execute_command` uses structured `program + argv + cwd + timeout +
env` by default. `edit_file` uses exact unique matching, an expected file hash, and only
limited line-ending/trailing-whitespace normalization.

Do not rename persisted data or protocol identifiers: SQLite schema stays v11, and
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
