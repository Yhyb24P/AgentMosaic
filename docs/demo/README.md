# Real terminal demo — one objective, a real Codex Lead and a real ACP worker

Raw evidence of one successful `am run` on **`am 0.2.1`**, run in a clean
dedicated scratch repo (no research code, no real Git identity, no credentials).
It proves the v0.2.1 minimal-team contract in a single real run:

```text
Reasoner: exactly 1   Worker: >= 1   Utility: optional (0 registered here)
team      READY reasoner=1 worker=1 utility=0
```

Lead = real Codex (`codex-app-server` adapter), Worker = real Qwen Code over
ACP (`qwen --acp`). No Utility agent was registered; the utility task the
Lead delegated fell back to the worker tier, exactly as the v0.2.1 routing
contract specifies. No human copy-paste, no second run, no spliced output.

## Objective

One objective in, one verified file out: read `input.txt` (four lines:
`beta alpha beta gamma`), produce `result.txt` with one `<token> <count>`
line per unique token, sorted lexicographically, then have the Lead review
both worker results and persist the final result.

Expected artifact content:

```text
alpha 1
beta 2
gamma 1
```

SHA-256 of `result.txt`:
`4297addc01e4f608b07fc9e71c6e8bb9ba836a12019da73bc219d97dd44a95d6`

## Files

| File | What it is |
|---|---|
| `transcript.txt` | Raw captured evidence of the one successful run, as recorded; command outputs are verbatim, nothing was retyped or re-entered |
| `result.txt` | The durable artifact the worker produced (verbatim copy) |
| `manifest.txt` | Run metadata: versions, team shape, task ids, SHA-256 digests, source paths |

## Durable state

Run state lives in `.agentmosaic/state.db` of the demo repo (deliberately
**not** committed here — see `manifest.txt`). All three durable commands
replay byte-identically against that database:

```bash
am status  .agentmosaic/state.db
am final   .agentmosaic/state.db 1
am artifact .agentmosaic/state.db 2
```

This was re-verified on 2026-09-14: each command exited 0 and its output
matched the captured evidence below byte for byte.

## Acceptance contract (task book gate)

| Gate | Evidence |
|---|---|
| fresh install reports `am 0.2.1` | cargo-dist (GitHub) install receipt; `am --version` re-verified |
| clean dedicated repo | single-commit scratch repo `demo baseline`, identity `demo@example.invalid` |
| exactly one reasoner / ≥1 worker / utility=0 | `am doctor` line `team READY reasoner=1 worker=1 utility=0` |
| doctor READY | `am doctor` project/state/lead/worker/team lines all `READY` |
| run exit=0 | captured `am run` stdout complete, final answer + refs emitted |
| real Codex Lead | lead adapter `codex-app-server`: `SPAWN_OK PROTOCOL_OK SESSION_OK READY` |
| real ACP/Qwen worker | worker adapter `acp` (`qwen --acp`): `SPAWN_OK PROTOCOL_OK SESSION_OK READY` |
| worker produced `result.txt` | task 2 `succeeded`, artifact `task=2 path=result.txt` |
| content correct | task 3 verification + re-verified SHA-256 `4297addc…` |
| artifact recorded | `am artifact` output for task 2 |
| durable final result | `am final` output for root task 1 (identical on replay) |
| no secrets | no credentials in demo repo, demo state, or this directory |
| raw transcript preserved | `transcript.txt` assembled only from this single run's captures |
| visual demo derives from that exact run | **not produced** — see status below |

## Status

```text
REAL_TERMINAL_DEMO = NOT_READY
```

The raw evidence of the successful run is preserved here. No terminal
recording (cast/asciinema) existed of that run, so no video asset was
produced from it, and none will: per the task rules, no re-typed
re-enactment and no splicing of other runs may stand in for the recording.
Any future published video must be recorded from a new, dedicated run that
records the terminal from start to finish. The website keeps its current
no-demo state until such an asset exists.
