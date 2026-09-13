# History

This file is the only current document that intentionally preserves the product's
previous identity. Everything else in the repository uses the current AgentMosaic
identity.

## v0.1.0 — first stable public release

On 2026-09-13 the Rust heterogeneous-Agent product line published its first stable
public release, `v0.1.0`, as the repository `Yhyb24P/research-agent-system`. That release
shipped the product under its former identity:

- former brand: `Research Agent System`
- former repository: `Yhyb24P/research-agent-system`
- former public executable: `agent-code-cli`
- former standalone TUI executable: `agent-code-tui`
- former Codex helper: `ras_codex_mcp`
- former crate prefix: `agent-code-`

The published tag, its tag object, its GitHub Release, and its release assets are
immutable historical truth. They are never moved, deleted, recreated or re-signed. The
v0.1.0 release notes are preserved verbatim at
[`docs/releases/v0.1.0.md`](releases/v0.1.0.md), and `CHANGELOG.md` keeps the historical
entries as written.

## The retired control plane

An earlier Python `researchd` control plane — with its Alembic migration chain,
qualification framework, policy/approval services and verifier — was removed from the
product line. It remains historical Git content only. It is not installed, launched, or
required by the current product, and it must not be rebuilt as core.

## Rename to AgentMosaic

After `v0.1.0` the line was normalized around a single identity:

- brand `AgentMosaic`, short name `AM`
- repository `Yhyb24P/AgentMosaic`, default branch `main`
- public command `am`
- crate prefix `agentmosaic-`, Rust import prefix `agentmosaic_`
- Codex helper `am-codex-mcp`
- environment prefix `AGENTMOSAIC_`, configuration namespace `agentmosaic`
- development version `0.2.0-dev`

The rename is deliberately breaking for external names: old binaries, crate/import
names, the old repository URL and the old branch name are not retained as aliases. It is
**not** breaking for durable data: the SQLite schema stays at version 11, task/result/
artifact/final-reference semantics are unchanged, persistence and protocol wire strings
are unchanged, and a board created by the published v0.1.0 `agent-code-cli` remains
readable by the new `am` command.
