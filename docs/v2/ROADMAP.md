# Rust v2 Roadmap

Active roadmap for `research-agent-system`. The previous Trusted Control Plane,
qualification and verification product direction is retired. This document replaces it
as the active plan.

## Product

A heterogeneous Agent coding/work team:

- high-intelligence Agents: planning, difficult reasoning, architecture, synthesis, review;
- local/cheap Agents: repetitive, long-running, data/file/tool-heavy work;
- deterministic utility workers when no model is needed.

Results and artifacts flow back automatically to the Agent that continues the reasoning.
No human copy/paste between Agents.

The native Rust Coding Agent is a recoverable tool-calling runtime, not a control plane.
External full Coding Agents (Codex/Claude-style CLIs) connect through an
`ExternalCliAgentDriver` and are not wrapped in a second tool loop.

## Status

- Reference baseline: `8cf27dc2a9e03ffbc1fbd091a576e0fb0f16bb93` on
  `preview/agent-control-closure`.
- Development branch: `v2/rust-agent-team`.
- The former Python `researchd` implementation was removed in R8; Git history
  retains the historical reference.
- R4 is sealed at `2692869` with exact-commit CI green. Its acceptance record
  is [`R4_ACCEPTANCE.md`](R4_ACCEPTANCE.md).
- R5 has a local, unpushed implementation stack under review; it is not sealed.
  See [`R5_STATUS.md`](R5_STATUS.md) for the current commits and unresolved
  review blockers.
- R6–R8 are implemented on the same branch: the Rust binaries are the normal
  path and the retired Python surface is deleted. The historical milestone
  evidence remains in `implementation_report.md` as history.

### RC repair status (2026-09-13)

An independent [`product_self_audit.md`](../../product_self_audit.md) (2026-09-13)
falsified the previous `PRODUCT_RC_READY` claim and found four S1 defects. Their
repair state:

| Defect | Finding | Repair state |
|---|---|---|
| S1-1 | no automatic normal team entrypoint | `agent-code-cli run-team`/`resume-team` added as the product entrypoint |
| S1-2 | the live test harness owned orchestration | orchestration moved into product `TeamRunner`; the harness only reads durable surfaces |
| S1-3 | Codex final result was a fixed placeholder | the driver persists the actual bounded final visible Codex agent message via `thread/read` exact-turn extraction |
| S1-4 | the "v8" fixture was generated from current DDL | replaced by authentic historical DDL `crates/agent-code-storage/tests/fixtures/schema_v8.sql` (commit `e7649230af388aa61fb851f1c4631e679b08e49b`, blob `21d10ff9c45e444f29b37f9082d1fd99b6333b56`) |

The product entrypoint is
`agent-code-cli run-team <database> <repo> "<objective>"`, with `resume-team` for
recovery. Storage schema is now v11 (`agent_registry.driver_config_json`,
non-secret options only). The Lead's decisions are strict JSON validated by the
product against [`contracts/lead_decision.schema.json`](../../contracts/lead_decision.schema.json);
a rejected decision fails closed after at most one bounded correction turn.

A real production E2E was verified on 2026-09-13 with real `codex-cli 0.154.0`
and real Qwen Code `0.23.3`, through the public CLI only: root task `1`
(`reasoning`, `codex-lead`, succeeded), delegated worker task `2` (`bulk`,
`qwen-worker`, parent `1`, succeeded), utility task `3` (parent `1`, succeeded),
a final answer containing a random worker-produced token, and persisted final
refs `[2]` plus artifact `task=2 path=worker.txt
sha256=23f3ef2f0a550aff9886f9c6bcef54ddac5d2fdef5ca8fe849db49cb97f3c979`.
Evidence: `.acc-evidence/rc-repair-fbc80bf/`; `cargo test --workspace
--all-features` reported 257 passed / 0 failed / 17 ignored.

Readiness remains split: `LOCAL_PRODUCT_RC_READY`,
`REMOTE_DETERMINISTIC_CI_READY`, and `PUBLIC_RELEASE_READY` are separate claims.
The exact-candidate freeze commit is `89ac979d333fe3fc2e311fb566f3ab0056bec4c5`
and the post-repair `product_self_reaudit.md` reports no remaining S0/S1 blocker,
so `LOCAL_PRODUCT_RC_READY` is claimed. Deterministic remote CI is also green on
that candidate (`rust.yml` and `rust-candidate`), so
`REMOTE_DETERMINISTIC_CI_READY` is claimed as well, and `v0.1.0` is published as a
GitHub Release (Latest, non-prerelease), so `PUBLIC_RELEASE_READY` is claimed for
that release. The historical pre-release tags are untouched.

## Crate graph

```text
Cargo.toml
crates/
  agent-code-core/       # session state machine, Agent loop, events, recovery
  agent-code-model/      # async model client (OpenAI-compatible HTTP first)
  agent-code-tools/      # the five atomic tools
  agent-code-workspace/  # project rules, Git worktree/checkpoint, path handling, diff/rollback
  agent-code-context/    # context budget, truncation, compaction, repository map
  agent-code-storage/    # small SQLite journal
  agent-code-runtime/    # the native Agent loop: model decisions, tool dispatch, durable history
  agent-code-team/       # Agent registry, lead, task board, scheduling, result flow
  agent-code-tui/        # ratatui/crossterm
  agent-code-cli/        # clap
```

Core tables: `sessions`, `agent_turns`, `tool_calls`, `checkpoints`, `team_tasks`,
`team_task_runs`, `messages`, `artifacts`. Do not reproduce the old qualification/audit
schema.

## Plan

- R0 Direction reset. Replace `AGENTS.md`, rewrite README positioning and product
  description, mark Python as legacy/transition, stop using qualification/control-plane
  closure as the active roadmap. Landed in this commit plus `docs/v2/ROADMAP.md`.
- R1 Rust core. Root Cargo workspace; durable Agent state machine; SQLite journal; model
  trait; recovery. CI: `cargo fmt --check`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace`.
- R2 Tools/workspace. Five tools, precise edit engine, workspace containment, structured
  commands, Git worktree/checkpoints and rollback.
- R3 Context. Context budgeting, output truncation, compaction, durable-vs-model history
  separation.
- R4 Single-Agent E2E. One native Rust Coding Agent inspects, edits, tests,
  self-corrects and delivers in a small real Git repository.
- R5 Team. AgentDriver, Reasoner/Worker/Utility tiers, task board, deterministic
  routing, concurrency, result/message flow, retry/reassign.
- R6 Real Agents. Connect one high-intelligence Agent and local Qwen; external full
  Coding Agents use `ExternalCliAgentDriver`.
- R7 TUI cutover. ratatui UX; the Rust binary is the normal path; legacy Python no
  longer launches by default.
- R8 Delete legacy. Completed: unreachable policy/verifier/qualification/
  backup/control-plane Python code and its dedicated package/test surface were
  removed after R6/R7 parity. Prefer deletion over compatibility wrappers.

## Blocking E2E

First, a single Agent:

```text
small Git repo -> inspect -> edit bug -> run check/test -> self-correct -> deliver patch
```

Second, a team:

```text
user objective -> high-intelligence Lead -> >=2 delegated tasks -> local Qwen/utility
worker performs bulk/tool work -> results/artifacts flow back automatically -> Lead uses
them -> final answer
```

## Do not recreate as core

- `PolicyEngine` / `ApprovalService`
- a mandatory independent Verifier
- IQ/DQ/RQ qualification
- backup/DR as a product subsystem
- the `WorkOrder + Attempt + Delegation + Invocation` quartet
- trust-zone / capability / audit systems as product identity

## Acceptance matrix

### Native Coding Agent

- N01 loads project rules and shallow repository map.
- N02 creates isolated Git worktree/checkpoint.
- N03 `view_file` paginates and returns file hash.
- N04 `edit_file` requires unique match and stale-hash protection.
- N05 syntax-invalid edit rolls back.
- N06 `write_file` obeys create/short-file bounds.
- N07 `search_dir` returns bounded matches.
- N08 `execute_command` uses structured argv by default.
- N09 command timeout terminates child/process group.
- N10 output truncation preserves head/tail metadata.
- N11 model context stays within configured budget.
- N12 old observations compact without deleting durable history.
- N13 restart recovers workspace/session.
- N14 interrupted non-idempotent command is not blindly replayed.
- N15 delivery includes diff, checks and known failures.

### Heterogeneous team

- T01 configure at least one Reasoner and one local Worker.
- T02 submit one objective.
- T03 Lead creates structured subtasks.
- T04 reasoning routes to Reasoner.
- T05 bulk/tool work routes to Worker/Utility.
- T06 two independent worker tasks run concurrently.
- T07 worker result reaches Lead's next context.
- T08 worker artifact reaches Lead.
- T09 Agent message reaches intended Agent context.
- T10 Lead creates follow-up based on worker result.
- T11 failed task retries.
- T12 failed task reassigns.
- T13 user can override assignment.
- T14 local Qwen completes one representative worker task.
- T15 high-intelligence Agent completes reasoning/synthesis side.
- T16 final result requires no human copy/paste between Agents.

### De-bloating

- D01 product is no longer described as Trusted Control Plane.
- D02 normal path requires no PolicyEngine.
- D03 normal path requires no ApprovalService.
- D04 normal path requires no independent Verifier.
- D05 normal path requires no qualification subsystem.
- D06 normal path requires no backup/DR subsystem.
- D07 Rust schema does not reproduce WorkOrder+Attempt+Delegation+Invocation quartet.
- D08 crate graph/contracts remain small and understandable.
- D09 final normal path does not launch legacy Python.
- D10 remaining legacy modules have concrete reasons to exist.
