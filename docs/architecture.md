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

Every Agent runs inside its own external runtime (an ACP peer, a Codex CLI or a Claude
CLI). AgentMosaic owns the durable team layer around them, not the model/tool loop.

## Cargo workspace

Five crates under `crates/`:

| Crate | Responsibility |
|---|---|
| `agentmosaic-storage` | SQLite task board, agent registry, runtime bindings/events, schema migration |
| `agentmosaic-runtime` | external runtime adapters (ACP, Codex exec/app-server, Claude CLI), Lead brains, team runner |
| `agentmosaic-team` | Agent registry, lead, task board, scheduling, result flow |
| `agentmosaic-tui` | ratatui/crossterm read-only board view |
| `agentmosaic-cli` | the public `am` command |

## External Agent runtimes

Each Agent is a registered external runtime command. AgentMosaic spawns it, bounds the
turn, normalizes what it reports into durable runtime events, and keeps task truth on the
board. Reliability mechanics that still apply are owned by the adapter that spawns the
runtime: absolute deadlines, process-group termination with reaping, output bounds and
artifact hashing.

Supported adapters: ACP v1 (any conforming peer), Codex `exec --json` (worker and Lead),
Codex `app-server --stdio` (compatibility runtime, including the internal `am
__internal codex-mcp` bridge), and Claude CLI `stream-json` (worker).

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

- `AcpWorkerDriver` — shared ACP boundary for external coding CLIs; it returns bounded
  structured results and configured relative artifact hashes rather than wrapping the
  runtime in a second tool loop.
- `CodexExec` (worker and Lead) — `codex exec --json` with a persisted foreign thread.
- `ClaudeCli` — Claude CLI `stream-json` worker with a persisted foreign session.
- `CodexAppServer` — bounded Codex app-server bridge with persisted external
  thread/turn references and allowlisted collaboration tools.

The durable runtime registry (`agent_registry`) records each Agent's tier, driver kind
(`acp`, `codex-exec`, `claude-cli`, or `codex-app-server`; the retired `native` and `cli`
strings stay readable for existing rows), executable, driver args, concurrency, tags,
runtime version, and an optional non-secret driver-config JSON object. The `am
register`/`am registry` verbs record and list registrations without launching any driver;
`am run` reconstructs the real drivers from these rows.

## Durability

The authoritative state is one SQLite database. Storage schema version is **12**.
Task/result/artifact/final-reference semantics and the runtime wire strings
(`TaskKind`, `DriverKind`) are stable identifiers; they are not renamed by branding
work. The Lead's strict decision wire is checked in at
`contracts/lead_decision.schema.json`.
