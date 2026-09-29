# Recovery

SQLite is the authoritative task board. Recovery settles durable state and preserves
completed results; it never replays completed work.

## After an interruption

First ensure the process that owned the run and its external runtime have stopped.
Inspect the root from inside the project, explicitly close its interrupted attempt,
then continue:

```bash
am status 1
am run --recover 1
am run --resume 1
```

`--recover` requires a root reasoning task. It closes only that root's current running
attempt, marks its external binding interrupted, and records a failed state. It starts
no Agent. Repeating recovery when no running attempt exists is a no-op. `--json` reports
`run_id`, `recovered_attempt` (null for a no-op), and `status`.

`--resume` reconstructs the team from the project registry and continues the root's
persisted Lead. It claims the root atomically and appends a new attempt. It reconciles
interrupted descendants from durable state before continuing the Lead. Prior failed
attempts retain their status, result, and error. Completed descendants remain available
as evidence and are never replayed.

A running root is refused by resume; it is never automatically reclaimed. Explicit
recovery assumes the previous owning process has stopped. A succeeded root returns its
stored final answer and exact task/artifact references without starting a runtime.

## Guarantees

- Only one concurrent resume can claim the root and enter the Lead.
- Lead failure cannot publish a succeeded root.
- Task and artifact references are committed atomically with the successful root result.
- Durable final evidence repairs an interrupted final settlement without replaying the Lead.
- Inspection (`status`, `events`, `artifact`, `final`, `tui`, `agent list`) starts no task runtime.

Older schema 11 or 12 state requires an explicit import into schema 14 before recovery:
`am import .agentmosaic/state.db`. The source remains available and unchanged. See the
[CLI reference](cli.md) for the accepted generations and import refusals.
