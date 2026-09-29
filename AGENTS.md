# AgentMosaic project boundaries

AgentMosaic connects heterogeneous external Agents around one project. The Lead plans,
delegates and synthesizes; workers return results and artifacts automatically. External
runtimes own models, credentials and tool loops. AM owns the durable team layer.

## Product surface

- Five crates: storage, runtime, team, tui and cli, all prefixed `agentmosaic-`.
- One product binary: `am`.
- Normal path: init → agent add → doctor → run → status/events/final/artifact/tui.
- Run recovery: `am run --recover <id>` after the controller has stopped, then
  `am run --resume <id>`. Never automatically reclaim an active root.
- Runtimes: ACP v1 and Codex exec/Claude CLI workers; Codex exec is the Lead adapter.
- Schema generation 14 uses `.agentmosaic/state-v14.db`, with eight business tables.
  `am import` reads released schema 11 or the explicit schema 12 development baseline
  into a separate file. No historical startup upgrades or reuse of experimental v13.
- Product SemVer and storage generation are independent.

## Invariants

SQLite Board is authoritative. Attempts are append-only; root claim and final/recovery
settlement are atomic. Keep the durable Lead and never replay successful descendants.
Final results reference real tasks and artifacts with owner, path and SHA-256.
Runtime events are bounded observations and never task authority.

Adapters retain deadlines, process-group termination/reaping, bounded output, artifact
containment and hashes. Preserve Lead decision JSON fields and task/final semantics.
Retired driver strings belong at the storage decoding boundary and cannot become runnable
current drivers.

Do not recreate internal Agent/model/tool loops, policy/approval platforms, mandatory
independent verifiers, qualification systems, app-server/MCP compatibility, or compiled
WorkspaceLease/DB-owner experiments. Development Agent task-orchestration rules are not
AM capabilities. Actor Scheduler, WorkspaceLease, Experience/DecisionEngine, Beacon/Jev
and adaptive routing remain frozen during v0.5 convergence.

## Repository

- `crates/`: the five Rust workspace crates.
- `contracts/`: current Lead decision wire contract.
- `docs/`: current usage, architecture, runtime, recovery, import and status only.
- `scripts/ci/`: deterministic validation fixtures and checks.
- `.github/`: normal CI, candidate checks and release/site workflows.
- `CHANGELOG.md` and GitHub Releases: release history; older documents live in Git.

`docs/status.md` is the current product facts entry. Research proposals do not become
implementation or release requirements. Keep `main` and at most one active topic branch;
merge completed work through squash. Preserve unique experiment work outside the product
tree rather than introducing permanent archive branches.

## Verification

```bash
scripts/ci/check_identity.sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --release --workspace
scripts/ci/check_hygiene.sh
```

Candidate acceptance additionally checks published schema 11 and development schema 12
import, fresh generation 14, copied-binary normal collaboration, recovery/no-replay, and
inspection without runtime startup. Real authenticated runtimes remain separate from
credential-free CI.
