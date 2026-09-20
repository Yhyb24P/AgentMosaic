# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Lead context stays valid JSON within its byte budget, including escaped and
  multilingual results. Text reductions are marked; complete IDs, artifact
  paths and digests survive, or the run reports a capacity error before the turn.
- Lead context carries each artifact's owning task ID for exact final selection.
- A resumed Lead can complete from already-successful descendants immediately;
  a failed root's Lead attempt becomes running before restoring its runtime binding.
- A failed worker outcome can ground a follow-up even when no worker succeeded.
- An ACP worker's peer result is read from the message text that follows its last
  tool activity, so progress narration between tool calls no longer fails the
  strict one-object contract; the whole turn is still recorded as its transcript.

## [0.3.0] - 2026-09-14

The interface milestone: `am` becomes a team interface rather than an engineering
surface, without a schema migration and with durable data, wire formats and the Lead
decision contract unchanged.

### Added

- Live team lifecycle during `am run`: run chrome, the Lead's rounds, each delegated
  task, worker attempts, artifact notices and next-command suggestions.
- Project-aware inspection: `am status [RUN]`, `am status --all`, `am final [RUN]`,
  `am artifact [TASK]` and `am tui` need no database path.
- `am agent remove <ID>`, which removes only the registry entry and preserves all
  historical tasks, results, artifacts and bindings.
- Machine-readable output: `--json` on `run`, `doctor`, `agent list`, `status`,
  `status <RUN>`, `status --all`, `final`, `final <RUN>`, `artifact` and
  `artifact <TASK>`.
- `--quiet` for `am run`, and `--verbose` for `am doctor`.
- `am advanced`, which lists the compatibility and low-level commands.
- A live-refresh read-only TUI that redraws on a bounded tick and reloads the
  registry on every refresh.
- A non-authoritative run-event projection over the durable task board.

### Changed

- Default help centers the normal workflow; the compatibility commands are hidden
  but remain callable at their current spellings.
- `am init`, `am agent add`, `am agent list` and `am doctor` are user-oriented,
  decision-first, and no longer print the SQLite state path.
- `am run` writes the final answer to stdout and all progress to stderr.

### Fixed

- The first Lead round accepts a single delegated task, matching the checked-in
  decision contract and the Lead's own instructions.
- Directed messages use the actual resolved Lead agent id instead of the literal
  `lead`.

## [0.2.1] - 2026-09-14

### Fixed

- A Lead + Worker team is now a valid runnable team; Utility Agents are optional.
- Utility tasks fall back to Worker Agents when no Utility Agent is registered.
- `am doctor` now validates the same minimum-team requirements used by `am run`.

## [0.2.0] - 2026-09-13

First public release under the AgentMosaic identity. The workspace was renamed to one
namespace and the onboarding surface was rebuilt around a single project-local team
state, while durable data and wire formats stayed compatible.

### Added

- **Project-aware onboarding.** `am init` creates project-local durable state at
  `.agentmosaic/state.db` under the Git root (and keeps it out of version control),
  replacing the previous positional `<database>` invocation for normal use.
- **`am agent add <id> --role <reasoner|worker|utility> --adapter <acp|codex-app-server>
  -- <program> [arg ...]`.** An Agent identity, role, adapter and opaque LaunchSpec argv
  are persisted in one step; everything after `--` is stored exactly as given and is
  never interpreted by AgentMosaic. `am agent list` renders the discovered registry.
- **`am doctor`.** Inspects project, team and runtime readiness without authentication:
  it probes each configured adapter through its own protocol and reports a bounded
  readiness classification, then reports `LEAD_SELECTION_AMBIGUOUS_OR_MISSING` unless
  exactly one reasoner is registered. `doctor` never opens a login flow.
- **`am run "<objective>"`.** Discovers the enclosing project state and delegates to the
  existing `TeamRunner`, so the team entrypoint no longer needs a hand-written database
  path or repository argument.
- **Distribution contract (`dist-workspace.toml`).** cargo-dist builds a shell installer
  and an archive for `x86_64-unknown-linux-gnu` only, and `release.yml` publishes them
  to the GitHub Release for a pushed version tag.
- **Checked-in documentation tree.** `docs/getting-started.md`, `docs/architecture.md`,
  `docs/cli.md`, `docs/recovery.md`, `docs/runtimes/{acp,codex,qwen-code}.md`,
  `docs/history.md` and `docs/releases/v0.1.0.md` now describe the shipped product.
- **`scripts/ci/check_identity.sh`**, an identity gate that fails the build if a retired
  identity reappears as an actively-shipped name, if retired working-history residue is
  tracked again, or if any workspace package or the public `am` binary target drifts.
- **Package/release tooling.** `scripts/release/package_release.sh` and
  `scripts/release/third_party_licenses.py` produce the third-party license bundle
  carried by release archives.

### Changed

- **One namespace for the whole workspace.** Cargo packages, Rust import paths, the
  configuration namespace and the environment prefix were unified under `agentmosaic`;
  the public binary is `am`. Old binaries, crate names, import prefixes, the old
  repository URL and the old branch name are deliberately not kept as aliases.
- **The Codex collaboration bridge is internal.** It is reachable only as the hidden
  `am __internal codex-mcp` command, is absent from `am --help`, and is never installed
  or configured separately. Existing boards that still reference a helper command
  remain readable.
- **`doctor` reports Lead selection explicitly** instead of leaving an ambiguous or
  missing Lead to be discovered during a team run.

### Removed

- **Tracked development evidence and stale roadmaps.** `.acc-evidence/**`,
  `implementation_report.md`, `product_self_audit.md`, `product_self_reaudit.md`,
  `docs/v2/**` and the runtime probe notes were removed as history hygiene; the retired
  Python control plane remains historical Git content only.

### Compatibility

- **SQLite schema stays v11.** No migration is required, and a board created by the
  `v0.1.0` release remains readable.
- **Persisted wire identifiers and semantics are unchanged**: `TaskKind` and
  `DriverKind` strings, task/result/artifact/final-reference semantics, and the Lead
  decision JSON fields all keep their `v0.1.0` meaning.
- The previous positional CLI grammar remains available as the advanced compatibility
  surface documented in `docs/cli.md`.

### Verified

- The full workspace gates pass: `scripts/ci/check_identity.sh`, `cargo fmt --all --
  --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features`, `cargo build --release --workspace` and
  `git diff --check`.
- `am init`, `am agent add`, `am doctor` and `am run` are covered by
  `crates/agentmosaic-cli/tests/project_onboarding.rs` against the real project-local
  state file.

## [0.1.0] - 2026-09-13

First stable public release of the Rust heterogeneous-Agent product line.

Earlier `v0.1.0-rc*` and `v1.0.0-rc.*` tags belong to pre-release and historical
development lines. They are not stable releases and are left untouched as history.

### Added

- **One-objective product entrypoint.** `agent-code-cli run-team <database> <repo>
  "<objective>"` starts the whole heterogeneous team: it loads the persisted agent
  registry, resolves exactly one Lead, constructs the real runtime drivers, creates one
  durable root task, runs the Lead/Scheduler loop, and persists the final answer with
  the exact selected task and artifact references. `resume-team <database> <repo>
  <root-task-id>` re-drives a durable root without replaying completed work.
- **Resident Codex Lead brain.** Codex runs as the reference high-intelligence Lead
  through a resident `codex app-server` thread shared across planning, follow-up and
  synthesis. Its decisions are strict JSON, validated by the product and failed closed
  after at most one bounded correction turn.
- **Async, fallible `LeadBrain` contract.** The Lead brain can hold an external runtime,
  and a brain failure propagates instead of being swallowed; the root task cannot become
  succeeded after a Lead failure.
- **Real Codex result fidelity.** The scheduler-facing Codex driver persists the actual
  bounded final visible agent message of the completed turn (`thread/read`, exact turn,
  `finalAnswer` preferred), replacing the previous fixed placeholder.
- **Durable root and result containment.** Delegated and follow-up tasks always hang off
  the current root, and a selected final task must be a succeeded descendant of the root.
- **Schema v11** with `agent_registry.driver_config_json` for non-secret driver options
  (ACP auth method and timeouts, expected artifact paths, Codex MCP bridge and overrides).
  Credential-looking keys are refused.
- **`codex-app-server` driver kind**, so a Codex runtime is identified explicitly rather
  than by executable name matching.
- **Authentic historical migration fixture.** `crates/agent-code-storage/tests/fixtures/schema_v8.sql`
  is the verbatim schema v8 DDL taken from the repository's own history, and it migrates
  all the way to the current schema version.
- **Read-only TUI dashboard** and read-only board views (`status`, `registry`, `artifact`,
  `binding`, `final`) that never start a runtime or mutate runtime state.

### Changed

- `register` accepts an optional non-secret driver-configuration JSON object (fields 8–10).
- `status` prints each task's `parent` id.
- Bounded workspace search (`search_dir`) walks deterministically, so a bounded search
  returns reproducible results on any filesystem.

### Removed

- The retired Python `researchd` control plane and its qualification workflows; the stale
  Python-era CI workflows were replaced with credential-free Rust workflows.

### Verified

- Real Codex `0.154.0` Lead and real Qwen Code `0.23.3` Worker completed an end-to-end
  team run through the public CLI, with the durable answer depending on delegated worker
  output and reproduced after reopening the database.
- Full workspace checks (`cargo fmt`, `cargo clippy --all-features -D warnings`,
  `cargo test --workspace --all-features`, release build, `git diff --check`) pass on the
  frozen candidate.
- Copied release binaries, run outside the source tree, complete a real team run.
