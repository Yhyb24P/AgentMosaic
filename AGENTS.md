# Project instructions

## Positioning

`research-agent-system` is a heterogeneous Agent coding/work team.

The one job: connect Agents with different strengths to one project. High-intelligence
Agents do planning, hard reasoning, architecture, synthesis and review. Local or cheap
Agents and deterministic workers do repetitive, long-running, file-heavy, data-heavy and
tool-heavy work. Results and artifacts flow back automatically to the Agent that
continues the reasoning, with no manual copy/paste between Agents.

Communication, scheduling, recovery and safety boundaries are supporting mechanics that
let several Agents finish work. They are not the product.

The native Rust Coding Agent is the execution engine for model-backed Agents. External
Agents (Codex/Claude-style CLIs) plug in through adapters.

## Direction

The previous "Trusted Control Plane / qualification / verification" product direction is
retired. Do not extend it. The active work is the Rust v2 strangler rewrite on branch
`v2/rust-agent-team` (baseline `8cf27dc2a9e03ffbc1fbd091a576e0fb0f16bb93`).

- The former Python `researchd` control plane and qualification framework were
  removed in R8. They are historical Git content, not a product path.
- The active roadmap is `R0 -> R8`, documented in `docs/v2/ROADMAP.md`.
- The product normal path now exists as
  `agent-code-cli run-team <db> <repo> "<objective>"` (with `resume-team` for
  recovery). One objective produces one durable root `reasoning` task whose
  result is the final visible Codex answer plus the exact selected task/artifact
  refs. Codex is the reference high-intelligence Lead; Qwen Code is the
  reference Worker.
- Storage schema is v11; the Lead's strict decision wire is checked in at
  `contracts/lead_decision.schema.json`.

## Do not recreate as core

Do not build these back into the product:

- `PolicyEngine` / `ApprovalService`
- a mandatory independent Verifier
- IQ/DQ/RQ qualification
- backup/DR as a product subsystem
- the `WorkOrder + Attempt + Delegation + Invocation` quartet
- trust-zone / capability / audit systems as product identity

Narrow runtime mechanics that genuinely help an Agent finish work may survive, but they
are not the product and not the roadmap.

## Engineering guards that remain

Path containment, command timeout, process-group termination, output truncation, atomic
writes, file hashes, Git checkpoints and crash recovery stay. They make a Coding Agent
reliable. They are runtime mechanics, not a control-plane product.

## Rust target

A Cargo workspace of small crates:

```text
Cargo.toml
crates/
  agent-code-core/       # session state machine, Agent loop, events, recovery
  agent-code-model/      # async model client (OpenAI-compatible HTTP first)
  agent-code-tools/      # the five atomic tools
  agent-code-workspace/  # project rules, Git worktree/checkpoint, path handling, diff/rollback
  agent-code-context/    # context budget, truncation, compaction, repository map
  agent-code-storage/    # small SQLite journal
  agent-code-runtime/    # native Agent loop, external drivers (Codex app-server/ACP), product TeamRunner
  agent-code-team/       # Agent registry, lead, task board, scheduling, result flow
  agent-code-tui/        # ratatui/crossterm
  agent-code-cli/        # clap
```

Five atomic tools: `view_file`, `edit_file`, `write_file`, `search_dir`,
`execute_command`. `execute_command` uses structured `program + argv + cwd + timeout +
env` by default. `edit_file` uses exact unique matching, an expected file hash, and only
limited line-ending/trailing-whitespace normalization.

Required Rust CI:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

## Structure

- `crates/`: the Rust v2 workspace (active).
- `docs/v2/`: the active Rust v2 roadmap and contracts.
