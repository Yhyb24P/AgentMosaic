# Real terminal demo — one objective, a real Codex Lead and a real ACP worker

Raw evidence of one successful `am run` on the v0.3.0 candidate commit, executed in
a clean dedicated scratch repo (no research code, no real Git identity, no
credentials). It proves the v0.3 CLI/observability behaviour in a single real run:

```text
Reasoner: exactly 1   Worker: exactly 1   Utility: 0 registered
team      ready  1 lead · 1 worker
lead      ready  codex … (codex-app-server adapter)
worker    ready  qwen --acp (acp adapter)
run #1    complete
```

Lead = real Codex (`codex-app-server` adapter). Worker = real Qwen Code over ACP
(`qwen --acp`). No utility agent was registered. No human copy-paste, no second run,
no spliced output.

This is **not** the v0.2.1 demo. The v0.2.1 transcript in `../` is kept as historical
regression evidence and was not reused.

## Objective

The objective is a normal user goal. It names no task kind, no target agent id, no
selected refs, and never says how many tasks to create — the Lead decides the
delegation structure:

```text
Read input.txt and create result.txt with the count of each token, sorted
alphabetically. Verify the file, do not modify input.txt, and summarize the result.
```

The Lead chose exactly **one** delegated task (a `bulk` task targeted at `worker`).
That is the case the pre-v0.3 runtime rejected with `InvalidFirstDecision`, so this
run is also the release gate for that fix.

## Fixture and result

`input.txt` (unchanged for the whole run, SHA-256
`b884d3d51e68ee1906c5fd54c8ab3c3acd5b327aefdc5589f8dc1f1096629391`):

```text
beta
alpha
beta
gamma
```

`result.txt` — the durable worker artifact:

```text
alpha 1
beta 2
gamma 1
```

SHA-256 of `result.txt`:

```text
4297addc01e4f608b07fc9e71c6e8bb9ba836a12019da73bc219d97dd44a95d6
```

That digest is byte-identical to the artifact the v0.2.1 demo produced, from a
completely independent run on a different model backend.

## Files

| File | What it is |
|---|---|
| `transcript.txt` | Verbatim capture of the one successful run: every command and its output, assembled from the per-command capture files with nothing retyped |
| `input.txt` | The fixture the objective reads (verbatim copy) |
| `result.txt` | The durable artifact the worker produced (verbatim copy) |
| `manifest.txt` | Run metadata: versions, candidate commit, team shape, task ids, digests, the three documented deviations, source directory |

## What the run shows about the v0.3 CLI

- `am init` taught the next step (Lead, Worker, then `am doctor`) without naming any
  state path.
- `am doctor` gave a decision (`project / lead / worker / team` + `Ready to run.`);
  `am doctor --verbose` showed the bounded stages
  (`PROGRAM_FOUND LAUNCHSPEC_VALID SPAWN_OK PROTOCOL_OK SESSION_OK`).
- `am run` reported the team lifecycle on **stderr** — run chrome, the Lead's rounds,
  the delegated task, the worker's attempt, the artifact notice, and the `next`
  suggestions — while **stdout** carried only the final answer, so
  `am run "…" > answer.txt` composes.
- `am status`, `am final`, `am artifact` and `am tui` all worked with **no database
  path**.
- The machine surface (`am status --json`, `am final --json`, `am artifact --json`)
  emitted full, unabbreviated values where the human rendering abbreviates the digest.

## Durable state

Run state lives in `.agentmosaic/state.db` of the demo repo (deliberately **not**
committed here — see `manifest.txt`). The inspection commands replay against that
database with no path argument:

```bash
am status
am final
am artifact
```

## Documented deviations from the task book's literal forms

All three are recorded in `manifest.txt` with their captured evidence. None of them
is hidden, and none of them implies a capability the product does not have.

1. **The Lead launcher is not literally `codex -qw`.** The local `codex` shim
   translates `-qw` into `codex -p brain`, but codex-cli 0.154.0 does not accept
   `--profile` on `codex app-server`:

   ```text
   Error: --profile only applies to runtime commands and `codex mcp`: `codex`, `codex exec`,
   `codex review`, `codex resume`, `codex queue`, `codex archive`, `codex delete`,
   `codex unarchive`, `codex fork`, `codex mcp`, `codex sandbox`, and `codex debug prompt-input`.
   ```

   (See transcript section 8.) The same local profile — model `qwen38`, provider
   `brain`, `xhigh` reasoning effort — is therefore expressed with `-c` overrides the
   app-server does accept. With `-qw` the Lead fails readiness with
   `PROTOCOL_UNAVAILABLE`, and a real run fails with
   `the codex lead failed to start the resident codex lead thread: Closed`.

2. **The Lead's event budget is raised from 200 to 4000.** An `xhigh`-effort local
   model exceeded the built-in bounded per-turn event budget; the observed failure was
   `the codex lead turn did not complete within 200 events`. The budget is persisted
   driver config (`max_events`), which `am agent add` does not expose, so the Lead was
   registered through the compatibility `register` spelling. The worker used the
   normal `am agent add` path. Raising this one value — nothing else changed — turned
   the failing run into a successful one; a single run does not by itself prove that
   the budget was the *only* factor, so it is recorded as the observed limiter.

3. **The worker's artifact path is configured, not discovered.** The ACP driver
   records the artifacts it is told to report; `artifact_paths=["result.txt"]` was set
   at registration. AgentMosaic does not do automatic changed-file discovery, and this
   evidence does not claim it does.

## Acceptance contract

| Gate | Evidence |
|---|---|
| 1 real Codex Lead | transcript section 4, 9 (`lead ready`, `codex-app-server`) |
| 1 real ACP/Qwen worker | transcript section 5, 9 (`worker ready`, `qwen --acp`) |
| utility = 0 | transcript section 6 (`am agent list` shows two rows), section 7 |
| doctor = ready | transcript section 9, exit 0 |
| natural objective | `manifest.txt` `objective=`; no task kinds, targets or counts specified |
| visible lifecycle | transcript section 12 (run chrome, Lead rounds, task, worker, artifact) |
| stdout is only the answer | transcript section 13 |
| run exit 0 | transcript section 13 (`exit=0`) |
| durable final | transcript section 19 (`am final`, no path) |
| correct artifact | transcript sections 15–16, digest `4297addc…a95d6` |
| no database path in the normal path | transcript sections 18–20 |
| one coherent real run | single `run #1`, recorded once; captures are not spliced |

## What this does not claim

- No public video was produced; a terminal recording is optional for v0.3.0 and is not
  the release gate.
- The binary reports the pre-bump workspace version (`am 0.2.1`) because the version
  bump and tag belong to the release task. This evidence is bound to the candidate
  commit and the binary digest recorded in `manifest.txt`.
- No automatic artifact discovery, no write-capable TUI, and no browser collaboration
  panel is claimed.
