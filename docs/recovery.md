# Recovery

The authoritative state is the SQLite board. Recovery commands operate on durable state
and never replay completed work.

## After an interruption

If a run is interrupted while attempts are in flight, their attempts are left in a
`running` state in the database. Close them explicitly, then resume the root:

```bash
am recover-all .agentmosaic/state.db
am resume-team .agentmosaic/state.db /path/to/repo 1
```

- `am recover <database> <task-id>` closes one interrupted attempt.
- `am recover-all <database>` closes every interrupted attempt.
- `am resume-team <database> <repo> <root-task-id>` rebuilds the drivers/brain from
  durable state, closes interrupted descendants without replaying them, and is
  idempotent on an already-succeeded root.

External runtime bindings are marked `interrupted` for the closed attempts; inspect them
with `am binding <database> <task-id>`.

## Reopening a task

```bash
am resume .agentmosaic/state.db <task-id>      # schedule a task again
am cancel .agentmosaic/state.db <task-id>      # cancel a task
am override .agentmosaic/state.db <task-id> <agent-id>   # reassign to an explicit agent
```

## Guarantees

- A root task cannot become `succeeded` after a Lead brain failure; the failure
  propagates instead of being swallowed.
- `resume-team` on an already-succeeded root is a no-op rather than a replay.
- Read-only commands (`status`, `registry`, `artifact`, `binding`, `final`, `tui`) never
  start a driver or mutate runtime state.
