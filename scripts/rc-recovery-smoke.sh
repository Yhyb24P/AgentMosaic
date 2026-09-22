#!/usr/bin/env bash
# Repeatable recovery / no-replay qualification for the copied release binary
# (RC gate T3/G5).
#
# A real team run is started with the copied release `am`, completes one worker
# artifact durably, and is then killed while the root is still Running and
# another delegated task is stuck. An explicit recovery closes the interrupted
# root attempt, `resume-team` continues the *same* durable root and Lead, and the
# run is completed from the durable evidence. The gate then proves the already
# successful work was not replayed:
#
#   child task id unchanged; child attempt count unchanged; artifact sha and
#   bytes unchanged; the worker process was launched exactly once; the root
#   gained a new attempt without rewriting attempt 1; the canonical Lead is
#   unchanged.
#
# The Lead and Worker are the development-only mocks; the recovery mechanics are
# the real board, scheduler and TeamRunner. `am recover`/`am resume-team` are the
# product's recovery spellings.
#
# usage: scripts/rc-recovery-smoke.sh [release-dir]
set -euo pipefail

RELEASE_DIR="${1:-target/release}"
RELEASE_DIR="$(cd "${RELEASE_DIR}" && pwd)"
for binary in am acp_m2_mock codex_bridge_mock; do
  if [ ! -x "${RELEASE_DIR}/${binary}" ]; then
    echo "missing release binary ${RELEASE_DIR}/${binary}; run: cargo build --release --workspace" >&2
    exit 2
  fi
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/agentmosaic-recovery.XXXXXX")"
cleanup() {
  pkill -f "${WORK}/bin/" >/dev/null 2>&1 || true
  rm -rf "${WORK}"
}
trap cleanup EXIT

BIN="${WORK}/bin"
REPO="${WORK}/repo"
mkdir -p "${BIN}" "${REPO}"
cp "${RELEASE_DIR}/am" "${BIN}/am"
cp "${RELEASE_DIR}/acp_m2_mock" "${BIN}/acp_m2_mock"
cp "${RELEASE_DIR}/codex_bridge_mock" "${BIN}/codex_bridge_mock"
AM="${BIN}/am"

fail() {
  echo "RECOVERY_SMOKE=FAIL: $*" >&2
  [ -f "${WORK}/run.err" ] && sed -n '1,20p' "${WORK}/run.err" >&2
  exit 1
}

# A counting wrapper around the mock worker: every launch appends a byte, so a
# replayed task is observable independently of the board. Development-only.
COUNTER="${WORK}/worker-launches"
cat >"${BIN}/worker.sh" <<WRAPPER
#!/bin/sh
printf x >>"${COUNTER}"
exec "${BIN}/acp_m2_mock" --mode sync "\$@"
WRAPPER
chmod +x "${BIN}/worker.sh"

cd "${REPO}"
git init --quiet .
git config user.email "recovery-smoke@example.invalid"
git config user.name "recovery smoke"
printf 'placeholder\n' > README.md
git add README.md
git -c commit.gpgsign=false commit --quiet -m "smoke baseline"
printf 'worker artifact\n' > worker-result.txt
ARTIFACT_SHA="$(sha256sum worker-result.txt | cut -d' ' -f1)"
ARTIFACT_BYTES="$(sha256sum "${REPO}/worker-result.txt" | cut -d' ' -f1)"
TOKEN="RCTOK-$(openssl rand -hex 8)"

"${AM}" init >/dev/null
# The project state lives at <root>/.agentmosaic/state.db; discover it.
DB="${REPO}/.agentmosaic/state.db"

"${AM}" agent add lead --role reasoner --adapter codex-app-server -- "${BIN}/codex_bridge_mock" >/dev/null
"${AM}" agent add worker --role worker --adapter acp --artifact worker-result.txt -- "${BIN}/worker.sh" >/dev/null
"${AM}" agent add hanger --role worker --adapter acp -- "${BIN}/acp_m2_mock" --mode hang >/dev/null
"${AM}" agent add utility --role utility --adapter acp -- "${BIN}/acp_m2_mock" --mode sync >/dev/null

# Round 1 delegates a bulk task to the counting worker and a tool task to the
# hanging agent. The worker succeeds; the hang keeps the root Running so the
# interruption window is deterministic instead of a race.
DELEGATE='{"action":"delegate","tasks":[{"kind":"bulk","target":"worker","objective":"produce worker-result.txt"},{"kind":"tool","target":"hanger","objective":"never finish"}]}'
RUN_REPLIES="[\"$(printf '%s' "${DELEGATE}" | sed 's/"/\\"/g')\"]"

set -m
CODEX_BRIDGE_MOCK_REPLIES="${RUN_REPLIES}" CODEX_BRIDGE_MOCK_STATE="${WORK}/lead-run.json" \
  "${AM}" run --json "Objective: produce ${TOKEN} and report completion." \
  >"${WORK}/run.out" 2>"${WORK}/run.err" &
RUN_PID=$!
set +m

# Wait until the worker's success is durable, then interrupt the run.
succeeded=no
for _ in $(seq 1 120); do
  if ! kill -0 "${RUN_PID}" 2>/dev/null; then break; fi
  if "${AM}" status "${DB}" 2>/dev/null | grep -q "task=2 status=succeeded"; then
    succeeded=yes
    break
  fi
  sleep 0.25
done
[ "${succeeded}" = yes ] || fail "worker task never became durable-succeeded"

kill -KILL -- "-${RUN_PID}" 2>/dev/null || kill -KILL "${RUN_PID}" 2>/dev/null || true
pkill -f "${WORK}/bin/" >/dev/null 2>&1 || true
disown "${RUN_PID}" 2>/dev/null || true
wait "${RUN_PID}" 2>/dev/null || true

# The interruption left the root Running, the worker succeeded, the hanger
# Running. Nothing may have rewritten the worker.
BEFORE="$("${AM}" status "${DB}")"
printf '%s' "${BEFORE}" | grep -q "task=1 status=running assignee=lead attempts=1 " || fail "root was not left Running: ${BEFORE}"
printf '%s' "${BEFORE}" | grep -q "task=2 status=succeeded assignee=worker attempts=1 " || fail "worker attempt count changed before resume: ${BEFORE}"
printf '%s' "${BEFORE}" | grep -q "task=3 status=running assignee=hanger attempts=1 " || fail "hanger was not left Running: ${BEFORE}"
[ "$(cat "${COUNTER}" 2>/dev/null | wc -c | tr -d ' ')" = "1" ] || fail "the worker was not launched exactly once before interruption"

# Explicit recovery closes the interrupted root attempt, then the resume
# continues the same root and Lead from durable evidence.
"${AM}" recover "${DB}" 1 >/dev/null || fail "explicit root recovery failed"
COMPLETE="{\"action\":\"complete\",\"answer\":\"resumed lead answer ${TOKEN}\",\"selected_task_ids\":[2],\"selected_artifacts\":[{\"task_id\":2,\"path\":\"worker-result.txt\",\"sha256\":\"${ARTIFACT_SHA}\"}]}"
RESUME_REPLIES="[\"$(printf '%s' "${COMPLETE}" | sed 's/"/\\"/g')\"]"

RESUME="$(CODEX_BRIDGE_MOCK_REPLIES="${RESUME_REPLIES}" CODEX_BRIDGE_MOCK_STATE="${WORK}/lead-resume.json" \
  "${AM}" resume-team "${DB}" "${REPO}" 1 --lead lead)"
printf '%s' "${RESUME}" | grep -q "${TOKEN}" || fail "resume did not produce the token answer: ${RESUME}"

AFTER="$("${AM}" status "${DB}")"

# No replay: identical child id, one attempt, same artifact, one launch.
printf '%s' "${AFTER}" | grep -q "task=2 status=succeeded assignee=worker attempts=1 " || fail "worker task was replayed or its attempt rewritten: ${AFTER}"
[ "$(cat "${COUNTER}" | wc -c | tr -d ' ')" = "1" ] || fail "the worker process was launched more than once: replay"
AFTER_SHA="$(sha256sum worker-result.txt | cut -d' ' -f1)"
[ "${AFTER_SHA}" = "${ARTIFACT_SHA}" ] || fail "worker artifact bytes changed across recovery"
[ "${AFTER_SHA}" = "${ARTIFACT_BYTES}" ] || fail "worker artifact digest changed across recovery"
printf '%s' "${AFTER}" | grep -q "task=1 status=succeeded assignee=lead attempts=2 " || fail "root did not gain a new attempt after resume: ${AFTER}"

# The canonical Lead is unchanged, and the final answer is durable.
FINAL="$("${AM}" final "${DB}" 1)"
printf '%s' "${FINAL}" | grep -q "${TOKEN}" || fail "final did not reproduce the resumed answer: ${FINAL}"

cat <<RECEIPT
AM_SHA256=$(sha256sum "${AM}" | cut -d' ' -f1)
AM_VERSION=$("${AM}" --version)
ROOT_ID=1
ROOT_ATTEMPTS_BEFORE=1
ROOT_ATTEMPTS_AFTER=2
ROOT_STATUS_AFTER=succeeded
WORKER_TASK_ID=2
WORKER_ATTEMPTS_BEFORE=1
WORKER_ATTEMPTS_AFTER=1
WORKER_ARTIFACT_SHA256=${ARTIFACT_SHA}
WORKER_LAUNCHES=1
CANONICAL_LEAD=lead
RECOVERY_NO_REPLAY=PASS
RECEIPT
