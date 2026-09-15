# S0 Context Flow & Token Cost Audit

Pure measurement. No implementation, no `Status` / `Summary` / `Trace` design,
no behavior change. This answers "what does the Lead actually receive, how big
is each part, and what never reaches it" before any context-efficiency idea is
allowed to become a plan.

```text
base            main@8088252ab06c23d6a1a0083dd7013e0a2867d21b
measured object LeadContext -> coded Lead prompt (both Lead brains)
data source     one real CodexExec-Lead heterogeneous run
                (/tmp/am-v05-s2/e2e/.agentmosaic/state.db, 4 tasks, 3 workers)
method          code reading (file:line) + SQLite read-back + a faithful
                re-implementation of the render model in the code
```

## 1. What `LeadContext` contains

`crates/agentmosaic-team/src/lead.rs:53-67` defines exactly eight fields, and
`lead.rs:330-371` fills them from the board each round:

| Field | Filled from | Bounded by |
|---|---|---|
| `root_task_id` | the run's root | - |
| `objective` | `team_tasks.objective` | per-entry bound at render time |
| `round` | Lead loop counter | - |
| `candidates` | scheduler registry agent ids | - |
| `results` | succeeded descendants' attempt results | per-entry bound |
| `artifacts` | `artifacts` rows of those tasks | per-entry bound |
| `failures` | failed attempts' bounded error text | `bounded_error` then per-entry bound |
| `messages` | `messages_to(lead_agent)` | per-entry bound |

There is no runtime-event field, no raw transcript, no tool payload and no
hidden reasoning. Events are a durable observation plane only.

## 2. What reaches the Lead vs what is stored

Measured on the real run:

```text
attempt result text stored        316 bytes   -> rendered (all 3 succeeded results)
artifact rows                       3 rows    -> rendered (path + 64-hex digest each)
messages addressed to the Lead      0         -> nothing to render
runtime events stored              17 events / 4,706 bytes payload  -> 0 bytes reach the Lead
objective                          605 bytes -> re-rendered into every round
```

So the observation plane is ~3× the size of the decision-bearing context
(4,706 vs 1,498 bytes at round 1; 6× versus round 0) and contributes nothing to
it. That is intended (events are observation, not task truth), and it means
"give the Lead more runtime events" is a *product* decision rather than a
transport fix.

## 3. Byte shares per round

Faithful re-implementation of `codex_lead::render_context`: compact JSON of the
eight fields, every text bounded to `per_text`, whole payload bounded to
`budget`.

```text
round 0 (no results yet): context 778 bytes, prompt 991 bytes
  objective 78.8% | candidates 6.9% | envelope+ids 13.2% | results/artifacts/failures/messages ~0.3% each

round 1 (3 results + 3 artifacts): context 1,498 bytes, prompt 1,711 bytes
  objective 40.9% | results 27.4% | artifacts 21.0% | candidates 3.6%
  envelope+ids 6.9% | failures/messages 0.1% each

round 2 (unchanged board): 1,498 bytes — the context is a pure function of the
  durable board, so an unchanged board re-sends an identical payload.
```

## 4. The bound model (where truncation would start)

```text
max_prompt_bytes (default)       32,768         team_runner.rs:52
prefix + suffix + newlines           214
budget                           32,554
entries = results + artifacts + failures + messages + 1
per_text = clamp(budget // entries, 64, 4096)

entries <= 7     per_text = 4,096 (ceiling)  <- the observed run (entries 1 then 7)
entries 8..508   per_text = 32,554 // entries
entries >= 509   per_text = 64 (floor)
```

Consequences that matter for any future context work:

* the context can never exceed `budget`, so unbounded growth is not the risk;
* at 8+ entries the per-entry allowance starts dropping, and past 508 entries it
  is 64 bytes — silent, per-entry truncation rather than a loud refusal;
* the objective competes with results for the same allowance, so its share fell
  from 78.8% to 40.9% once three workers reported.

## 5. Fixed per-turn overhead

The two Lead brains do not send the same fixed preamble:

```text
DEVELOPER_INSTRUCTIONS                                 1,602 bytes
codex-exec Lead        instructions + prefix + context + suffix on EVERY turn  ~1.8 KB fixed/turn
codex app-server Lead  instructions once at thread start; later turns carry only prefix + context + suffix
```

For the observed run that is ~3,313 bytes per codex-exec turn versus ~1,711 for
the app-server shape, before provider-side tokenization. This is a transport
asymmetry between the two Lead runtimes, not a defect in either.

## 6. Which information actually participates in decisions

`codex_lead::parse_reply` accepts only the strict decision object and validates
every reference against the context it was given:

```text
decision-bearing : candidates (target must be one of them), results + artifacts
                   (task ids and artifact path+sha256 must match the context)
advisory         : failures, messages (context to reason about, never referenced by id)
never decision input: durable runtime events (absent from LeadContext by construction)
```

So "context efficiency" work can only touch the advisory and objective parts
without changing decision semantics; the results/artifacts block is what the
validator and the final-ref contract depend on.

## 7. Duplicated propagation measured today

```text
objective       stored in team_tasks.objective, re-rendered into every round's context
child summary   stored in team_task_runs.result AND re-rendered into the Lead context
artifact        path+sha256 stored in artifacts AND team_final_artifact_refs
                AND re-rendered into the Lead context
runtime events  stored only; rendered nowhere
```

## 8. Open questions this audit does not answer

These are the questions a `Status` / `Summary` / `Trace` proposal would have to
answer with evidence, and none of them is designed here:

```text
1. In real runs, do entries ever exceed 7 (where per-entry truncation starts)?
   Requires multi-round runs with several workers and failures, not one 3-worker run.
2. Does any real Lead decision ever depend on a `failures` or `messages` entry
   that would be dropped by a summary? Requires decision-level traces.
3. Is the objective's per-round re-send worth removing? It is 40-79% of the
   context here; removal changes retry semantics and needs its own experiment.
4. Would a compact Status/Summary/Trace replace the results block, or sit beside
   it? Only (2) can answer that.
```

## 9. Explicit non-goals

No context, prompt, transport, storage or Lead behavior was changed. No new
module, field, event, schema or wire value. No `Status`/`Summary`/`Trace`
implementation, no jCodeMunch integration, no ProcessSupervisor work (that line
is closed: `docs/experiments/process-supervisor-e1.md`).

## 10. Evidence

```text
measurement output   /tmp/am-ctx-audit-evidence.txt (local scratch)
source board         /tmp/am-v05-s2/e2e/.agentmosaic/state.db (real run from the
                     S2 heterogeneous E2E; not a durable repository artifact)
code references      lead.rs:53-67, lead.rs:330-371, codex_lead.rs:244-254,
                     codex_lead.rs:257-313, codex_lead.rs:613-622, team_runner.rs:52
```

## Status

```text
S0_CONTEXT_FLOW_AUDIT_COMPLETE=true
CONTEXT_IMPLEMENTATION_CHANGED=false
STATUS_SUMMARY_TRACE_DESIGNED=false
JCODEMUNCH_INTEGRATED=false
PROCESS_SUPERVISOR_LINE_CLOSED=true
```
