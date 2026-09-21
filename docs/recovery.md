# Recovery

The authoritative state is the SQLite board. Recovery commands operate on durable state
and never replay completed work.

## After an interruption

If a run is interrupted while attempts are in flight, their attempts are left in a
`running` state in the database. Close them explicitly, then resume the root:

```bash
am recover-all <database>
am resume-team <database> /path/to/repo 1
```

- `am recover <database> <task-id>` closes one interrupted attempt.
- `am recover-all <database>` closes every interrupted attempt.
- `am resume-team <database> <repo> <root-task-id>` rebuilds the drivers/brain from
  durable state, closes interrupted descendants without replaying them, and is
  idempotent on an already-succeeded root.

For an unfinished root, `resume-team` continues the root's durable Lead and appends a
new attempt: a failed attempt keeps its own status, result and error, and never
becomes running again. `--lead <agent-id>` only asserts that same agent; a resume
refuses to replace a root's Lead. A `running` root is refused rather than reclaimed,
so two live resumes can never both enter the Lead — close an interrupted one with
`am recover` first. If completed descendants already provide enough evidence, the
Lead can finish immediately on the first resumed round without creating another
task. A failed descendant is also valid evidence for a follow-up; a successful task
is still required to ground the final answer.

External runtime bindings are marked `interrupted` for the closed attempts; inspect them
with `am binding <database> <task-id>`.

## Reopening a task

```bash
am resume <database> <task-id>      # schedule a task again
am cancel <database> <task-id>      # cancel a task
am override <database> <task-id> <agent-id>   # reassign to an explicit agent
```

These are the compatibility spellings and take an explicit state database path as
`<database>`; `am advanced` lists them and the [CLI reference](cli.md) carries the full
grammar.

## Guarantees

- A root task cannot become `succeeded` after a Lead brain failure; the failure
  propagates instead of being swallowed.
- `resume-team` on an already-succeeded root is a no-op rather than a replay.
- A crash between the Lead's final refs and the root status is repaired from that
  durable evidence, without replaying the Lead.
- Read-only commands (`status`, `registry`, `artifact`, `binding`, `final`, `tui`) never
  start a driver or mutate runtime state.
