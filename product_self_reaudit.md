# Product self re-audit — RC repair

- date: 2026-09-13
- branch: `v2/rust-agent-team`
- frozen candidate (executable source): `89ac979d333fe3fc2e311fb566f3ab0056bec4c5`
  (supersedes `295c96a075cdf3987d8e66fa75fce14d15611b3a`, which the frozen candidate
  equals except for the bounded-search determinism fix recorded below)
- audited tree: HEAD at the freeze docs commit (report/evidence only; no executable or
  Cargo-source change relative to the frozen candidate)
- falsification baseline: `product_self_audit.md` (unchanged, sha256
  `f6df35e5225b52a9d28431378f5f2ce0a28afc30347a4880de6fc0564e53295d`)

## Authorship and independence

This pass was performed by the executor of the repair on the frozen candidate. It uses the
contract's mandatory questions and it actively looks for defects rather than restating the
implementation, but it is **not** an independent third-party audit. An external
re-audit remains the strongest possible evidence and is recommended before any public
release decision. Every claim below cites a command or a file, so an external auditor can
re-run it.

## Mandatory questions

### Q1 — Does `run-team` really instantiate Lead + Scheduler + drivers?

Yes. `crates/agent-code-runtime/src/team_runner.rs` (`TeamRunner::run`) opens the board,
loads `agent_registry` via `SqliteAgentRegistry`, builds a validated `AgentRegistry`,
resolves exactly one Lead `reasoner`, builds real drivers through
`crates/agent-code-runtime/src/driver_factory.rs`, creates the durable root task plus its
Lead attempt, constructs `CodexLeadBrain` + `Lead` + `Scheduler`, and calls
`lead.run_on_root`.

Evidence:
- deterministic: `team_runner_product.rs::one_objective_becomes_a_durable_team_result`;
- live: `.acc-evidence/rc-repair-fbc80bf/live-run-team-e2e.log` — `root=1 lead=codex-lead`,
  child `task=2 ... assignee=qwen-worker ... parent=1 ... status=succeeded`;
- the CLI arm is thin: `crates/agent-code-cli/src/main.rs` only parses argv and calls
  `TeamRunner`.

### Q2 — Does any live test still create delegated tasks manually?

The **product** live E2E does not: `crates/agent-code-cli/tests/team_live_product.rs`
issues exactly one `run-team` and then only reads durable surfaces (`status`, `final`,
`artifact`, `binding`, `resume-team`). It creates no task, reads no plan file, runs no
worker, injects no result into a Codex prompt, and finalizes no refs.

`crates/agent-code-runtime/tests/codex_live.rs` **does** still create tasks and parse a
plan file. This is retained lower-level live runtime evidence, explicitly permitted:
contract-01/14 says `codex_live.rs` "can remain lower-level live runtime evidence" and
"simply stops being the product orchestrator". It is not the product path and nothing in
the product code calls it. Residual risk noted below.

### Q3 — Is the root/task Codex result actual bounded visible model text?

Yes. `crates/agent-code-runtime/src/codex_app_server.rs` implements `thread/read`
(`{"threadId":…,"includeTurns":true}`) and selects the exact turn, preferring
`phase == "finalAnswer"` and otherwise the last non-empty non-async `agentMessage`, bounded
on a UTF-8 boundary; it fails closed when no valid message exists. The former fixed
placeholder `"Codex scheduler task completed"` is gone from non-test code (workspace grep
returns nothing). The live run's persisted root answer is real synthesized text that
contains the worker-produced token — it cannot be a constant.

Two genuine upstream-conformance defects were found and fixed by consulting the pinned
source at tag `rust-v0.154.0`: `turn/completed` carries `params.turn.id` (a full `Turn`
object), not a flat `turnId`; and notifications arriving while a request is pending were
being dropped, now queued in a bounded buffer.

### Q4 — Is the historical v8 structurally authentic?

Yes, and independently re-checked: `crates/agent-code-storage/tests/fixtures/schema_v8.sql`
is byte-identical (whitespace-normalized `diff`) to the `SCHEMA` const body at commit
`e7649230af388aa61fb851f1c4631e679b08e49b` (blob
`21d10ff9c45e444f29b37f9082d1fd99b6333b56`). At v8: `user_version == 8`;
`agent_registry` exists **without** `runtime_version` and without `driver_config_json`;
`team_final_task_refs` / `team_final_artifact_refs` are absent.

One correction to an earlier working assumption (not a contract requirement): the
authentic v8 DDL **does** contain `acc_*`, `external_runtime_bindings` and
`runtime_collaboration_records`, because those predate v8. Asserting them absent would
have required fabricating the fixture. The tests assert them present with an explanatory
comment. The pre/post assertion list, the generator rewrite
(`src/bin/make_v8_fixture.rs`) and the unit test at `src/lib.rs` all now use only the
historical DDL; a v10→v11 previous-current test covers the newest step.

### Q5 — Does a copied release binary run a real heterogeneous team?

Yes. `scripts/rc-release-smoke.sh` copies `agent-code-cli` and `ras_codex_mcp` **out of**
the source tree, registers a real Codex Lead and real Qwen Worker/Utility, and issues one
`run-team`:

```text
run_team_exit=0
root=1 lead=codex-lead
task_refs: 2
artifact_refs: task=2 path=worker.txt sha256=a0c06db608e87f6f1512b8e0cb475acc85bb48878afb48a722f33929eef5a697
TOKEN_IN_ROOT_ANSWER=yes
TOKEN_IN_WORKER_RESULT=yes
```

A dependency proof is included: the token is randomly generated per run, and it appears in
the worker's own persisted result, in the on-disk artifact, and in the persisted root
answer, with the worker task id in the persisted final refs.

### Q6 — Do docs accurately describe the CLI and read-only TUI?

Yes. `README.md` / `README.zh-CN.md` show register Lead + Worker + utility → `run-team` →
`status` → `final`/`artifact` → recover/resume, document the full `register` grammar
including the optional driver-config JSON, and state that `submit` alone is only a pending
board task and not a team run. The TUI and the read-only commands are explicitly labelled
read-only (`README.md:136,192,230`; `README.zh-CN.md:66,100`). `docs/v2/ROADMAP.md` has a
current RC-repair status section; `AGENTS.md` gained `agent-code-runtime` and the product
entrypoint.

### Q7 — Is the report current-state block unambiguous?

Yes. `implementation_report.md` opens with a single `## Current state (RC repair,
2026-09-13)` block carrying the frozen candidate hash, schema 11, the S1 table, the M1–M9
table, the three separated readiness claims, the blockers and the verified live E2E. The
stale original summary and the disproved RC claims are retained as history and marked
`SUPERSEDED_BY_PRODUCT_SELF_AUDIT`. No historical section was rewritten or deleted.

## S1 re-evaluation

| Defect | Verdict | Basis |
|---|---|---|
| S1-1 no automatic heterogeneous-team product entrypoint | **PASSED** | `run-team` / `resume-team`; deterministic + live evidence (`root=1`, real worker child) |
| S1-2 live harness materially performs product orchestration | **PASSED** | orchestration lives in `TeamRunner`/`DriverFactory`; the product live test only reads durable surfaces |
| S1-3 Codex result is a fixed placeholder | **PASSED** | placeholder removed; `thread/read` exact-turn final visible message; grep proof; live answer is model text |
| S1-4 v8 fixture is current-schema relabeled | **PASSED** | byte-identical historical DDL; v8 preconditions asserted; generator rewritten |

No S0 or S1 blocker remains.

## T01–T16

| Item | Verdict | Basis |
|---|---|---|
| T01 configure a Reasoner and a local Worker | PASSED | `register` for reasoner/worker/utility; live test |
| T02 submit one objective | PASSED | one `run-team` objective creates the root |
| T03 Lead creates structured subtasks | PASSED | strict JSON decision wire; live worker+utility children |
| T04 reasoning routes to the Reasoner | PASSED | tier routing unchanged; Lead is the registered reasoner |
| T05 bulk/tool work routes to Worker/Utility | PASSED | live `task=2 bulk → qwen-worker`, `task=3 utility → qwen-utility` |
| T06 two independent worker tasks run concurrently | PASSED | existing scheduler concurrency tests; unchanged |
| T07 worker result reaches the Lead's next context | PASSED | `build_context` reads board results; live answer contains the worker token |
| T08 worker artifact reaches the Lead | PASSED | `build_context.artifacts`; Lead selected the exact `worker.txt` digest |
| T09 directed Agent message reaches the intended Agent | PASSED | `messages_to("lead")` in `build_context`; existing message tests |
| T10 Lead creates a follow-up from a worker result | PASSED | follow-up path forces root containment; `team_e2e` follow-up assertion |
| T11 failed task retries | PASSED | existing scheduler retry tests |
| T12 failed task reassigns | PASSED | `team_e2e` reassignment assertion |
| T13 user can override assignment | PASSED | `override` command; existing CLI tests |
| T14 local Qwen completes a representative worker task | PASSED | live run task `2`, artifact + digest persisted |
| T15 high-intelligence Agent completes reasoning/synthesis | PASSED | real Codex Lead produced the structured decisions and the final answer |
| T16 final result requires no human copy/paste between Agents | PASSED | one command; no manual Agent-to-Agent transfer anywhere in the product path |

## M1–M9

| Milestone | Current state |
|---|---|
| M1_CODEX_TEAM_READY | repaired on the S1-3 axis; a real Codex Lead ran end to end |
| M2_ACP_DRIVER_READY | ACP driver executed a real Qwen worker in the live run |
| M3_QWEN_WORKER_READY | proven for the exercised worker task (artifact + digest persisted) |
| M4_KIMI_PROFILE_CLASSIFIED | unchanged; no new live Kimi evidence and none is in RC-repair scope |
| M5_R6_TEAM_READY | S1-1/S1-2 repaired; one `run-team` produced root + worker + utility with durable result flow |
| M6_R6_SEALED | not sealed by this pass; sealing is a separate decision |
| M7_R7_NORMAL_PATH_READY | `run-team` is the normal team path; docs and Rust CI reconciled |
| M8_R8_DEBLOATED | retired Python removed earlier; stale Python-era docs/workflows now reconciled |
| M9_PRODUCT_RC_READY | local product path verified on a frozen candidate; see readiness below |

This pass does not claim any milestone as globally sealed; it reports the state it could
verify.

## Readiness

```text
LOCAL_PRODUCT_RC_READY        = true
REMOTE_DETERMINISTIC_CI_READY = true    (rust.yml + rust-candidate green on the frozen candidate)
PUBLIC_RELEASE_READY          = false   (no tag, no GitHub Release, not authorized)
```

`REMOTE_DETERMINISTIC_CI_READY` is supported by run `34759201679` (`rust.yml`, push) and
run `34759220090` (`rust-candidate`, workflow_dispatch), both green on `89ac979`; see
`remote-ci.md`.

`LOCAL_PRODUCT_RC_READY` is supported by: exact-candidate `fmt`/`clippy`/test/release-build/
`git diff --check` all exit 0 with a clean worktree (257 passed / 0 failed / 17 ignored), a
real Codex+Qwen E2E through the public CLI on that candidate (47.16s, exit 0), DB reopen in
separate processes reproducing the answer and refs, a product-path failure case leaving the
root non-succeeded, and a copied-release-binary run outside the source tree.

One further defect was found and repaired *because* remote CI was run: `search_dir` depended
on directory enumeration order, so a bounded search was not reproducible across filesystems
(the remote `ubuntu-22.04` runner returned `b.rs` where other environments returned `a.rs`).
`crates/agent-code-workspace/src/tools.rs` now sorts walked paths, and the n07 test asserts
order-independence. This is why the frozen candidate is `89ac979` rather than `295c96a`.

## Residual risks and things this pass did NOT prove

1. **Model dependence.** The live E2E passed repeatedly but LLM behaviour varies. In one
   recorded run the Qwen worker's first peer result was non-strict JSON and the scheduler's
   bounded retry recovered it; the final run needed no retry.
2. **Resume after a mid-run crash is not live-exercised end to end.** Product recovery is
   covered deterministically (`resume_recovers_interrupted_descendants_without_replaying_them`,
   the interrupted-attempt tests) and the succeeded-root resume path was exercised live; a
   kill-during-live-run recovery was not.
3. **The older `codex_live.rs` harness still performs manual orchestration** by design (it is
   lower-level runtime evidence, not the product path). A reviewer could misread it as the
   product path; the docs and this report state the distinction.
4. **First-round delegation must contain at least two subtasks.** A legitimate single-task
   objective is rejected with `InvalidFirstDecision`. That is pre-existing, intentional
   product behaviour, but it is a usability edge a user should know about.
5. **The Lead brain calls the synchronous app-server transport directly inside its async
   `decide`.** It is correct for the current-thread runtime the runner uses, but it blocks
   that runtime thread for the duration of a Codex turn.
6. **Remote deterministic CI is green on the frozen candidate** (`rust.yml` run
   `34759201679` and `rust-candidate` run `34759220090`), so
   `REMOTE_DETERMINISTIC_CI_READY` is claimed. Neither workflow runs the authenticated
   Codex/Qwen E2E, which remains a local reference-profile gate.
7. **Out of scope by contract**: Kimi integration, OpenClaw, A2A, ACP unstable v2, TUI
   redesign, retired control-plane systems. Their absence is not a defect of this repair.
