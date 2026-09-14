# Architecture

AgentMosaic is a heterogeneous Agent coding/work team. One objective goes in and one
durable team result comes out.

The one job: connect Agents with different strengths to one project. High-intelligence
Agents do planning, hard reasoning, architecture, synthesis and review. Local or cheap
Agents and deterministic workers do repetitive, long-running, file-heavy, data-heavy and
tool-heavy work. Results and artifacts flow back automatically to the Agent that
continues the reasoning, with no manual copy/paste between Agents.

Communication, scheduling, recovery and safety boundaries are supporting mechanics that
let several Agents finish work. They are not the product.

## Layers

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

## Cargo workspace

Ten small crates under `crates/`:

| Crate | Responsibility |
|---|---|
| `agentmosaic-core` | session state machine, Agent loop, events, recovery |
| `agentmosaic-model` | async model client (OpenAI-compatible HTTP first) |
| `agentmosaic-tools` | the five atomic tools |
| `agentmosaic-workspace` | project rules, Git worktree/checkpoint, path handling, diff/rollback |
| `agentmosaic-context` | context budget, truncation, compaction, repository map |
| `agentmosaic-storage` | small SQLite journal and durable board |
| `agentmosaic-runtime` | native Agent loop, external drivers (Codex app-server / ACP), team runner |
| `agentmosaic-team` | Agent registry, lead, task board, scheduling, result flow |
| `agentmosaic-tui` | ratatui/crossterm read-only board view |
| `agentmosaic-cli` | the public `am` command |

## Native Coding Agent

The native Rust Coding Agent is a recoverable tool-calling runtime. It exposes five
atomic tools:

- `view_file` — workspace-contained, paginated, returns a file hash.
- `edit_file` — exact unique match, expected file hash, limited line-ending/trailing
  whitespace normalization, atomic write, syntax guard with rollback.
- `write_file` — new files or explicit short-file replacement, with size bounds.
- `search_dir` — bounded path/line/match records, never whole files.
- `execute_command` — structured `program + argv + cwd + timeout + env` by default, with
  process-group termination and output truncation.

Reliability mechanics — path containment, command timeout, process-group termination,
worktree isolation, output truncation, atomic writes, Git checkpoints and rollback — are
kept because they make a Coding Agent reliable. They are runtime mechanics, not a
control-plane product.

## Team layer

The team layer only divides work and moves results between Agents. Agents have a tier
(`Reasoner`, `Worker`, `Utility`), a driver, and a concurrency bound. A normal team has
exactly one Reasoner and at least one Worker; Utility Agents are optional. Routing is
deterministic: reasoning/review goes to a Reasoner, bulk/tool work goes to a Worker, and
utility work prefers a Utility then falls back to a Worker. An explicit valid target wins;
utility work never implicitly falls back to a Reasoner. A worker result
automatically becomes context for its parent task, the Lead, and any explicitly
addressed Agent.

Driver boundaries:

- `NativeCodingAgentDriver` — the Rust state machine, model client and five tools.
- `CodexAppServer` — bounded Codex app-server bridge with persisted external
  thread/turn references and allowlisted collaboration tools.
- `AcpWorkerDriver` — shared ACP boundary for external coding CLIs; it returns bounded
  structured results and configured relative artifact hashes rather than wrapping the
  runtime in a second tool loop.
- `UtilityDriver` — deterministic worker for tests/build/search/batch.

The durable runtime registry (`agent_registry`) records each Agent's tier, driver kind
(`native`, `acp`, `cli`, or `codex-app-server`), executable, driver args, concurrency,
tags, runtime version, and an optional non-secret driver-config JSON object. The
`am register`/`am registry` verbs record and list registrations without launching any
driver; `am run-team` reconstructs the real drivers from these rows.

## Durability

The authoritative state is one SQLite database. Storage schema version is **11**.
Task/result/artifact/final-reference semantics and the runtime wire strings
(`TaskKind`, `DriverKind`) are stable identifiers; they are not renamed by branding
work. The Lead's strict decision wire is checked in at
`contracts/lead_decision.schema.json`.
