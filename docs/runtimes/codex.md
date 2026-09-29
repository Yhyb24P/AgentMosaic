# Codex runtime

`codex-exec` drives the external `codex exec --json` interface for the Lead and workers.

```bash
am agent add lead --role reasoner --adapter codex-exec -- codex
```

The launch argv remains valid when the adapter appends `exec --json`. AgentMosaic does
not select a model, provider, credential or launcher profile. Prompts travel on stdin.
Each turn starts an external process; subsequent turns use `codex exec resume --json`
with the persisted foreign thread ID.

The Lead contract renderer and strict decision parser are independent of the transport.
The decision wire is checked in at `contracts/lead_decision.schema.json`. Malformed
responses get at most one bounded correction turn, then fail the root attempt.

Process deadlines, output bounds, process group termination/reaping, artifact containment
and hashes remain adapter responsibilities. Successful tasks are not replayed on run
resume. See [Recovery](../recovery.md).

Earlier app-server registrations remain inspectable after explicit import, but cannot
run. Register a current adapter for a new run. Foreign app-server threads cannot be
resumed through the exec transport.
