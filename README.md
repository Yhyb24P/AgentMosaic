# Research Agent System

[简体中文](README.zh-CN.md)

> **Status.** The active direction is a Rust v2 rewrite of the native Coding Agent and
> the heterogeneous Agent team layer, on branch `v2/rust-agent-team`. The Python
> `researchd` control-plane implementation was removed in R8 and is no longer the
> product. The RC repair adds the product team entrypoint
> `agent-code-cli run-team <db> <repo> "<objective>"` (plus `resume-team`) and storage
> schema v11. A verified real Codex + Qwen Code team run through the public CLI is
> recorded in `.acc-evidence/rc-repair-fbc80bf/`. See
> [the roadmap](docs/v2/ROADMAP.md) and
> [R5 status](docs/v2/R5_STATUS.md).

Research Agent System is a **heterogeneous Agent coding/work team**.

The one job: connect Agents with different strengths to one project. High-intelligence
Agents do planning, hard reasoning, architecture, synthesis and review. Local or cheap
Agents and deterministic workers do repetitive, long-running, file-heavy, data-heavy and
tool-heavy work. Results and artifacts flow back automatically to the Agent that
continues the reasoning, with no manual copy/paste between Agents.

Communication, scheduling, recovery and safety boundaries are supporting mechanics that
let several Agents finish work. They are not the product.

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

Reliability mechanics (path containment, command timeout, worktree isolation, output
truncation, atomic writes, rollback) are kept because they make a Coding Agent reliable.
They are runtime mechanics, not a control-plane product.

## Team layer

The team layer only divides work and moves results between Agents. It does not become an
enterprise workflow engine. Agents have a tier (`Reasoner`, `Worker`, `Utility`), a
driver, and a concurrency bound. Routing is deterministic: reasoning/review goes to a
Reasoner, bulk/tool work goes to a Worker or Utility, an explicit target wins, otherwise
the configured default. A worker result automatically becomes context for its parent task,
the Lead, and any explicitly addressed Agent.

Current driver boundaries:

- `NativeCodingAgentDriver` — the Rust state machine + model client + five tools.
- `CodexAppServer` — bounded Codex app-server bridge with persisted external
  thread/turn references and allowlisted collaboration tools.
- `AcpWorkerDriver` — shared ACP boundary for Qwen/Kimi-style external coding
  CLIs; it returns bounded structured results and configured relative artifact
  hashes, rather than wrapping the runtime in a second tool loop.
- `UtilityDriver` — deterministic worker for tests/build/search/batch.

The durable runtime registry (`agent_registry`) records each Agent's tier, driver
kind (`native`, `acp`, `cli`, or `codex-app-server`), executable, driver args,
concurrency, tags, runtime version, and an optional non-secret driver-config JSON
object. The CLI `register`/`registry` verbs record and list registrations without
launching any driver or legacy Python; `run-team` reconstructs the real drivers
from these rows.

## Roadmap

The active plan is `R0 -> R8` in [docs/v2/ROADMAP.md](docs/v2/ROADMAP.md):

- R0 direction reset (this repositioning)
- R1 Rust core (workspace, state machine, SQLite journal, model trait, recovery)
- R2 tools and workspace
- R3 context budget and recovery
- R4 single-Agent E2E
- R5 team scheduler
- R6 real Agents (high-intelligence + local Qwen)
- R7 TUI cutover
- R8 delete legacy

The two blocking E2Es are: a single native Agent inspecting, editing, testing,
self-correcting and delivering a patch in a small real Git repository; and a team where a
Lead delegates at least two tasks, local/utility workers perform the work, results and
artifacts flow back, and the Lead uses them to produce the final answer.

## Rust development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

## Quickstart — one heterogeneous team objective

Codex is the reference high-intelligence Lead; Qwen Code is the reference Worker.
One objective in, one durable team result out:

```text
configure/register the Codex Lead
configure/register the Qwen Worker (and a utility agent)
run-team one objective
status            (read-only durable team dashboard)
final / artifact  (the durable answer and its exact refs)
recover / resume  (after an interruption)
```

`submit` alone only creates a pending board task; it is not a team run. The
`run-team` command is the team entrypoint. Everything operates directly on the
authoritative SQLite board and never launches legacy Python.

`register` grammar (8 to 10 trailing fields):

```text
agent-code-cli register <database> <agent-id> <name> <tier> <driver-kind> <executable> <driver-args> <max-concurrency> <tags> [<runtime-version-or->] [<driver-config-json-or->]
```

- `tier` is `reasoner`, `worker`, or `utility`.
- `driver-kind` is `native`, `acp`, `cli`, `codex-app-server`, or `-`.
- `driver-args` and `tags` are comma-separated; use `-` for none.
- `runtime-version` is the optional 9th field; use `-` for none.
- the optional 10th field is one non-secret JSON object of driver options, or `-`.
  A key that looks like a credential (`token`, `key`, `secret`, `password`,
  `endpoint`) is refused, so provider credentials can never be stored here.
  - `acp`: `auth_method`, `timeout_seconds`, `max_prompt_bytes`,
    `max_result_bytes`, `artifact_paths`.
  - `codex-app-server`: `mcp_command` (required: an existing file, the built
    `ras_codex_mcp` binary), `artifact_paths`, `max_events`, `overrides`. When
    this agent is the run's Lead it also reads `model`, `max_prompt_bytes`, and
    `max_answer_bytes`.
  Keep site-local launcher aliases out of this database.

Build the workspace so `ras_codex_mcp` and `agent-code-cli` exist, then point
`mcp_command` at the absolute path of the built bridge:

```bash
cargo build --release --workspace
```

```bash
# 1. register the Codex Lead (reference Reasoner, codex-app-server driver)
cargo run -p agent-code-cli -- register ./team.db codex-lead codex-lead reasoner \
  codex-app-server codex - 1 codex,lead - \
  '{"mcp_command":"/abs/path/to/target/release/ras_codex_mcp","model":"gpt-5.5","max_events":200,"overrides":["model=\"gpt-5.5\"","model_reasoning_effort=\"low\""]}'

# 2. register the Qwen Code Worker and a utility agent (ACP driver)
cargo run -p agent-code-cli -- register ./team.db qwen-worker qwen-worker worker \
  acp qwen --acp 1 - - \
  '{"auth_method":"openai","timeout_seconds":600,"artifact_paths":["worker.txt"]}'
cargo run -p agent-code-cli -- register ./team.db qwen-utility qwen-utility utility \
  acp qwen --acp 1 - - '{"auth_method":"openai","timeout_seconds":600}'

# 3. run one objective through the whole team
cargo run -p agent-code-cli -- run-team ./team.db /path/to/repo "produce worker.txt and summarize it"

# 4. read-only durable board views (no runtime is launched)
cargo run -p agent-code-cli -- status ./team.db
cargo run -p agent-code-cli -- registry ./team.db
cargo run -p agent-code-tui -- ./team.db          # read-only dashboard; q exits

# 5. the durable answer and its exact refs
cargo run -p agent-code-cli -- final ./team.db 1
cargo run -p agent-code-cli -- artifact ./team.db 2
cargo run -p agent-code-cli -- binding ./team.db 2

# 6. after an interruption: close interrupted attempts, then resume
cargo run -p agent-code-cli -- recover-all ./team.db
cargo run -p agent-code-cli -- resume-team ./team.db /path/to/repo 1
```

Flags accepted by both `run-team` and `resume-team`:

```text
--lead <agent-id>   select the Lead when more than one reasoner is registered
--max-rounds N      bound the Lead's reasoning rounds
--max-tasks N       bound the delegated task budget
--max-retries N     bound per-agent retries before the scheduler reassigns
```

Without `--lead`, exactly one registered `reasoner` must exist; zero or several
fails rather than guessing.

`run-team` opens/migrates the board, loads the persisted agent registry, builds
the validated registry, resolves the Lead, constructs the real drivers, creates
one durable root `reasoning` task plus its Lead attempt, and runs a resident Codex
`CodexLeadBrain` through `Lead` + `Scheduler`. Delegated tasks execute on real
Qwen workers over ACP, and the final visible Codex answer plus the exact selected
task/artifact refs are persisted on the root. `resume-team` rebuilds the
drivers/brain from durable state, closes interrupted descendants without replaying
them, and is idempotent on an already-succeeded root.

The Lead's decisions are strict JSON validated by the product; a decision that
does not match the contract fails closed after at most one bounded correction
turn. The decision wire is checked in at
[`contracts/lead_decision.schema.json`](contracts/lead_decision.schema.json).

The read-only commands `status`, `registry`, `artifact`, `binding`, and `final`,
plus the TUI dashboard, never start a driver or mutate the board's runtime state.
`status` prints each task with its `parent=<id|->` link. Live Qwen ACP and real
Codex Lead evidence is recorded in `implementation_report.md` and
`.acc-evidence/`.

## R8 legacy removal

The retired Python `researchd` control plane, Alembic migration chain,
qualification framework, and their dedicated tests/scripts were removed after
Rust R6/R7 parity. Historical implementation remains available in Git history;
it is not installed, launched, or required by the current product.

## License

Apache License 2.0 (ALv2). See the `LICENSE` file at the repository root.
