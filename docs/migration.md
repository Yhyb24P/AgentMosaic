# Database import

The current source writes generation 14 in `.agentmosaic/state-v14.db`. It is a breaking
update from v0.3.0. Use the generation 14 import path when upgrading old projects.

From the project root, after stopping all controllers using the source database:

```bash
am import .agentmosaic/state.db
am agent list
am doctor
```

Import accepts released schema 11 (v0.1.0–v0.3.0) and the schema 12 development baseline.
Schema 12 is an explicit one-time import allowance, not a promise to support all historical
development schemas. Schema 1/2/8, experimental 13, unknown and future generations are
refused. Ordinary startup opens only empty/current generation 14 state and never upgrades
an old database in place. Existing destination files are refused.

The source opens read only with a consistent SQLite snapshot. Its tasks, attempts,
results, errors, task artifacts, registry, external bindings and selected final references
retain their IDs and values. Schema 12 runtime observations are copied; schema 11 starts
with empty observations. Identifier high-water marks are preserved. Retired session/ACC,
collaboration receipts and messages stay in the source file and are absent from the new
product database. No runtime is started during import.

Import validates source integrity and relationships. A source with session-only artifacts
is refused with an actionable error: those artifacts have no task owner and cannot be
silently turned into task artifacts. Preserve the original and prepare a task-only copy
for import. Invalid references or attempts also fail without publishing a destination.

Retired `native`, `cli` and `codex-app-server` registry strings remain visible in
`am agent list`; they are refused for execution. Register a current adapter for a new run.
Changing an adapter does not convert a foreign runtime thread: an unfinished app-server
run must be completed using its old release or left closed while starting a new run.
Successful imported runs remain inspectable without any runtime process.

The separate filename prevents accidental reuse through default old-version discovery.
It does not prevent an old binary deliberately given the new database path from writing
it. Never run old and new controllers against the same database.
