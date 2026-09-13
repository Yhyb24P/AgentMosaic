# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
