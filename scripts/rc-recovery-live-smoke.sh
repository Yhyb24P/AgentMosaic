#!/usr/bin/env bash
# Real CodexExec Lead + real Qwen ACP Worker root interruption/recovery gate.
# Development-only acp_m2_mock is used solely as the stable hanger.
set -euo pipefail

AM_SRC="${1:-target/release/am}"
DEV_BIN="${2:-target/release}"
[[ -x "${AM_SRC}" && -x "${DEV_BIN}/acp_m2_mock" ]] || { echo "usage: $0 <am> <same-candidate-dev-bin>" >&2; exit 2; }
for cmd in codex qwen python3 git sha256sum; do command -v "$cmd" >/dev/null || { echo "missing runtime/tool: $cmd" >&2; exit 2; }; done
WORK="$(mktemp -d "${TMPDIR:-/tmp}/agentmosaic-live-recovery.XXXXXX")"
cleanup() {
  if [[ -n "${RUN_PID:-}" ]]; then kill -KILL -- "-${RUN_PID}" 2>/dev/null || true; fi
  if [[ -n "${HANGER_PID_FILE:-}" && -s "${HANGER_PID_FILE}" ]]; then
    hanger_pid="$(cat "${HANGER_PID_FILE}")"
    kill -KILL "${hanger_pid}" 2>/dev/null || true
  fi
  rm -rf "${WORK}"
}
trap cleanup EXIT
mkdir -p "${WORK}/bin" "${WORK}/repo"
BIN="${WORK}/bin"; REPO="${WORK}/repo"; cp "${AM_SRC}" "${BIN}/am"; AM="${BIN}/am"
cp "${DEV_BIN}/acp_m2_mock" "${BIN}/acp_m2_mock"
COUNTER="${WORK}/worker-launches"; HANGER_PID_FILE="${WORK}/hanger.pid"
cat >"${BIN}/qwen-count" <<WRAP
#!/bin/sh
printf x >>"${COUNTER}"
exec qwen --acp "\$@"
WRAP
cat >"${BIN}/hanger" <<WRAP
#!/bin/sh
printf '%s\\n' "\$\$" >"${HANGER_PID_FILE}"
exec "${BIN}/acp_m2_mock" --mode hang "\$@"
WRAP
chmod +x "${BIN}/qwen-count" "${BIN}/hanger"
cd "${REPO}"; git init --quiet; git config user.email rc@example.invalid; git config user.name 'rc recovery'; printf 'baseline\n' > README.md; git add README.md; git -c commit.gpgsign=false commit --quiet -m baseline
"${AM}" init >/dev/null
DB="${REPO}/.agentmosaic/state.db"
"${AM}" agent add lead --role reasoner --adapter codex-exec -- codex >/dev/null
"${AM}" agent add worker --role worker --adapter acp --artifact worker.txt -- "${BIN}/qwen-count" >/dev/null
"${AM}" agent add hanger --role worker --adapter acp -- "${BIN}/hanger" >/dev/null
"${AM}" doctor --json >/dev/null
: >"${COUNTER}" # doctor readiness probes do not count as task launches
TOKEN="RCTOK-$(openssl rand -hex 10)"
OBJECTIVE="In this repository, delegate exactly one bulk task to worker and exactly one tool task to hanger. Worker must create worker.txt with exactly the single line worker=${TOKEN} followed by one newline and return a JSON peer summary containing ${TOKEN}. Do not create worker.txt yourself. The hanger will remain running and the controller may be interrupted. After interruption, if worker has already succeeded, never delegate worker again; complete using that existing worker task and exact worker.txt artifact SHA. Your answer must contain ${TOKEN}. Do not invent ids or hashes."
# This is the controller process group created by this harness; no process-name kills are used.
set -m
"${AM}" run --json "${OBJECTIVE}" >"${WORK}/run.out" 2>"${WORK}/run.err" & RUN_PID=$!
set +m
ROOT_ID=; WORKER_ID=; HANGER_ID=
for _ in $(seq 1 240); do
  if ! kill -0 "${RUN_PID}" 2>/dev/null; then break; fi
  STATUS="$("${AM}" status "${DB}" 2>/dev/null || true)"
  ROOT_ID="$(printf '%s\n' "$STATUS" | awk '$1 ~ /^task=/ && /parent=-/ {sub("task=", "", $1); print $1; exit}')"
  WORKER_ID="$(printf '%s\n' "$STATUS" | awk '$1 ~ /^task=/ && /assignee=worker/ {sub("task=", "", $1); print $1; exit}')"
  HANGER_ID="$(printf '%s\n' "$STATUS" | awk '$1 ~ /^task=/ && /assignee=hanger/ {sub("task=", "", $1); print $1; exit}')"
  if [[ -n "$ROOT_ID" && -n "$WORKER_ID" && -n "$HANGER_ID" ]] && \
     printf '%s\n' "$STATUS" | grep -Eq "task=${ROOT_ID} status=running assignee=lead attempts=1" && \
     printf '%s\n' "$STATUS" | grep -Eq "task=${WORKER_ID} status=succeeded assignee=worker attempts=1" && \
     printf '%s\n' "$STATUS" | grep -Eq "task=${HANGER_ID} status=running assignee=hanger"; then break; fi
  sleep .5
done
[[ -n "$ROOT_ID" && -n "$WORKER_ID" && -n "$HANGER_ID" ]] || { echo 'recovery live gate: task window not reached' >&2; cat "${WORK}/run.err" >&2; exit 1; }
STATUS="$("${AM}" status "${DB}")"
printf '%s\n' "$STATUS" | grep -Eq "task=${ROOT_ID} status=running assignee=lead attempts=1" || { echo 'root not running at interruption window' >&2; exit 1; }
printf '%s\n' "$STATUS" | grep -Eq "task=${WORKER_ID} status=succeeded assignee=worker attempts=1" || { echo 'worker success not durable' >&2; exit 1; }
[[ "$(wc -c <"${COUNTER}" | tr -d ' ')" = 1 ]] || { echo 'worker launch count before interruption != 1' >&2; exit 1; }
ART_BEFORE="$("${AM}" artifact --json "$WORKER_ID")"
SHA_BEFORE="$(JSON_INPUT="$ART_BEFORE" python3 -c 'import json,os; a=json.loads(os.environ["JSON_INPUT"])["artifacts"]; assert len(a)==1 and a[0]["path"]=="worker.txt"; print(a[0]["sha256"])')"
[[ -f worker.txt && "$(sha256sum worker.txt | cut -d' ' -f1)" = "$SHA_BEFORE" ]] || { echo 'worker artifact is not durable/hash grounded' >&2; exit 1; }
[[ "$(cat worker.txt)" = "worker=${TOKEN}" && "$(wc -c <worker.txt | tr -d ' ')" = "$(( ${#TOKEN} + 8 ))" ]] || { echo 'worker bytes differ from requested exact line' >&2; exit 1; }
LEAD_BINDING="$("${AM}" binding "$DB" "$ROOT_ID" 1)"
grep -q 'agent=lead' <<<"$LEAD_BINDING" && grep -q 'runtime_kind=codex-exec' <<<"$LEAD_BINDING" && grep -q 'external_reference_present=true' <<<"$LEAD_BINDING" || { echo 'root attempt lacks real CodexExec binding' >&2; exit 1; }
kill -KILL -- "-${RUN_PID}" 2>/dev/null || kill -KILL "$RUN_PID" 2>/dev/null || true
wait "$RUN_PID" 2>/dev/null || true; RUN_PID=
# Confirm interrupted root and durable child before explicit root-only recovery.
STATUS="$("${AM}" status "$DB")"
printf '%s\n' "$STATUS" | grep -Eq "task=${ROOT_ID} status=running assignee=lead attempts=1" || { echo 'root state changed before recover' >&2; exit 1; }
printf '%s\n' "$STATUS" | grep -Eq "task=${WORKER_ID} status=succeeded assignee=worker attempts=1" || { echo 'worker state changed before recover' >&2; exit 1; }
"${AM}" recover "$DB" "$ROOT_ID" >/dev/null
RESUME="$("${AM}" resume-team "$DB" "$REPO" "$ROOT_ID" --lead lead)"
printf '%s\n' "$RESUME" | grep -q "$TOKEN" || { echo 'resumed result lost token' >&2; exit 1; }
STATUS="$("${AM}" status "$DB")"
printf '%s\n' "$STATUS" | grep -Eq "task=${ROOT_ID} status=succeeded assignee=lead attempts=2" || { echo 'root did not succeed at attempt 2' >&2; exit 1; }
printf '%s\n' "$STATUS" | grep -Eq "task=${WORKER_ID} status=succeeded assignee=worker attempts=1" || { echo 'worker replayed or rewritten' >&2; exit 1; }
[[ "$(wc -c <"${COUNTER}" | tr -d ' ')" = 1 ]] || { echo 'worker launch count changed across resume' >&2; exit 1; }
[[ "$(sha256sum worker.txt | cut -d' ' -f1)" = "$SHA_BEFORE" ]] || { echo 'worker artifact SHA changed' >&2; exit 1; }
ART_AFTER="$("${AM}" artifact --json "$WORKER_ID")"
[[ "$ART_AFTER" = "$ART_BEFORE" ]] || { echo 'durable worker artifact changed' >&2; exit 1; }
FINAL="$("${AM}" final --json "$ROOT_ID")"
JSON_INPUT="$FINAL" python3 - "$ROOT_ID" "$TOKEN" <<'CHECK'
import json, os, sys
d=json.loads(os.environ['JSON_INPUT'])
assert d['run_id']==int(sys.argv[1]) and sys.argv[2] in d['answer']
CHECK
# Compare native thread ids ephemerally; never print or persist them.
DB_PATH="$DB" ROOT_ID="$ROOT_ID" python3 <<'CHECK'
import os, sqlite3
c=sqlite3.connect(os.environ['DB_PATH'])
r=c.execute("select agent_id,runtime_kind,native_thread_id,lifecycle_state from external_runtime_bindings where team_task_id=? and attempt=1", (int(os.environ['ROOT_ID']),)).fetchone()
s=c.execute("select agent_id,runtime_kind,native_thread_id,lifecycle_state from external_runtime_bindings where team_task_id=? and attempt=2", (int(os.environ['ROOT_ID']),)).fetchone()
assert r and s and r[0]==s[0]=='lead' and r[1]==s[1]=='codex-exec'
assert r[2] and s[2] and r[2]==s[2]
assert r[3]=='interrupted' and s[3]=='completed'
CHECK
cat <<RECEIPT
ROOT_ID=${ROOT_ID}
ROOT_ATTEMPTS=1->2
WORKER_TASK_ID=${WORKER_ID}
WORKER_ATTEMPTS=1->1
WORKER_LAUNCHES=1
WORKER_ARTIFACT_SHA256=${SHA_BEFORE}
CANONICAL_LEAD=lead
LEAD_THREAD_REUSED=true
RECOVERY_LIVE_END_TO_END=PASS
RECEIPT
