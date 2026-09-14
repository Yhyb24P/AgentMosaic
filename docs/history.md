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

## Archived development lines

Four draft pull requests on retired pre-AgentMosaic development lines were
closed unmerged on 2026-09-14. They belong to a product line that is no
longer developed; none of that code was merged, requalified, or carried
into the current product. Their branches and commits remain in the
repository as development history — preserved exactly, not deleted.

| PR | Historical purpose | Head SHA | Disposition |
|---|---|---|---|
| #10 “feat: complete trusted multi-agent productization” (`next/agent-workspace-launcher`) | trusted multi-agent productization line | `2b56aa7a705c4a6b44246c3217b975d7f58e9909` | closed unmerged · superseded |
| #11 “feat: product hardening baseline (unqualified candidate)” (`next/product-hardening`) | product hardening baseline stacked on top of #10 | `cb1475202acfffdc52bcf83410b1f69b37f1c0ff` | closed unmerged · superseded |
| #12 “chore: candidate contract repair and requalification preflight” (`next/candidate-requalification`) | candidate requalification line; head pinned by the immutable tag `v1.0.0-rc.82` | `ca67f55acf95afd114e5af3059bd224ce45adf29` | closed unmerged · superseded |
| #13 “Infrastructure hardening for Agent collaboration” (`preview/agent-control-closure`) | historical control-plane / communication infrastructure preview | `dcdef968d1aa2de8beb3461304beb704a81685b3` | closed unmerged · superseded |

The historical line `#10 -> #11 -> #12` is stacked: the #10 head and the #11
head are ancestors of the #12 head (verified with `git merge-base
--is-ancestor` on 2026-09-14), and the `v1.0.0-rc.82` annotated tag points
at exactly that #12 head commit. The tag is never moved or deleted, and
#10/#11/#12 history is retained by the immutable tag chain.

The current product boundary is documented in `AGENTS.md`, `README.md`, and
this file. The former control-plane/communication preview and all other
archived product lines are **not** the current architecture; content of
those lines was not reintroduced into current docs or code.
