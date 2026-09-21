#!/usr/bin/env bash
# Deterministic, credential-free normal-path candidate smoke (RC gate T1/G2).
#
# The tested `am` is a *release* binary copied OUT of the source tree, together
# with the development-only mock runtimes. It then drives the first-class
# product path in separate processes:
#
#   am init
#   am agent add lead   --role reasoner --adapter codex-app-server
#   am agent add worker --role worker   --adapter acp --artifact worker-result.txt
#   am agent add utility --role utility --adapter acp
#   am doctor --json
#   am run --json "<objective>"
#   am status --json / am final --json / am artifact --json   (fresh processes)
#
# The Lead is the scripted `codex_bridge_mock` app-server and the Worker/Utility
# are `acp_m2_mock`; every other layer is the real product: durable registry,
# SQLite board, scheduler, Lead loop, artifact hashing and final grounding. No
# credentials, no model selection, no network.
#
# usage: scripts/ci/normal-path-smoke.sh [release-dir]
#
# Exit 0 prints a small sanitized receipt; anything else is a gate failure.
set -euo pipefail

RELEASE_DIR="${1:-target/release}"
if [ ! -d "${RELEASE_DIR}" ]; then
  echo "missing release directory ${RELEASE_DIR}" >&2
  exit 2
fi
RELEASE_DIR="$(cd "${RELEASE_DIR}" && pwd)"

AM_SRC="${RELEASE_DIR}/am"
ACP_SRC="${RELEASE_DIR}/acp_m2_mock"
CODEX_SRC="${RELEASE_DIR}/codex_bridge_mock"
for binary in "${AM_SRC}" "${ACP_SRC}" "${CODEX_SRC}"; do
  if [ ! -x "${binary}" ]; then
    echo "missing release binary ${binary}; run: cargo build --release --workspace" >&2
    exit 2
  fi
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/agentmosaic-normal-path.XXXXXX")"
cleanup() { rm -rf "${WORK}"; }
trap cleanup EXIT

BIN="${WORK}/bin"
REPO="${WORK}/repo"
mkdir -p "${BIN}" "${REPO}"
cp "${AM_SRC}" "${BIN}/am"
cp "${ACP_SRC}" "${BIN}/acp_m2_mock"
cp "${CODEX_SRC}" "${BIN}/codex_bridge_mock"
AM="${BIN}/am"

fail() {
  echo "NORMAL_PATH_SMOKE=FAIL: $*" >&2
  exit 1
}

# A project the run can own: a real Git repository with one committed file and
# the exact worker artifact bytes the run will ground its answer in.
cd "${REPO}"
git init --quiet .
git config user.email "normal-path-smoke@example.invalid"
git config user.name "normal path smoke"
printf 'placeholder\n' > README.md
git add README.md
git -c commit.gpgsign=false commit --quiet -m "smoke baseline"
printf 'worker artifact\n' > worker-result.txt
ARTIFACT_SHA="$(sha256sum worker-result.txt | cut -d' ' -f1)"
TOKEN="RCTOK-$(openssl rand -hex 8)"

# 1. The first-class path: init, three registered Agents, a ready team.
"${AM}" init >/dev/null
"${AM}" agent add lead --role reasoner --adapter codex-app-server -- "${BIN}/codex_bridge_mock" >/dev/null
"${AM}" agent add worker --role worker --adapter acp --artifact worker-result.txt -- "${BIN}/acp_m2_mock" --mode sync >/dev/null
"${AM}" agent add utility --role utility --adapter acp -- "${BIN}/acp_m2_mock" --mode sync >/dev/null

DOCTOR="$("${AM}" doctor --json)"
printf '%s' "${DOCTOR}" | grep -q '"ready":true' || fail "doctor did not report ready: ${DOCTOR}"

# 2. One `am run --json`. The Lead delegates a bulk task to `worker` and a
#    utility task to `utility`, then completes by selecting the worker task and
#    its artifact with the exact digest the file already has.
DELEGATE='{"action":"delegate","tasks":[{"kind":"bulk","target":"worker","objective":"produce worker-result.txt"},{"kind":"utility","target":"utility","objective":"produce the utility result"}]}'
COMPLETE="{\"action\":\"complete\",\"answer\":\"lead synthesized final answer ${TOKEN}\",\"selected_task_ids\":[2],\"selected_artifacts\":[{\"task_id\":2,\"path\":\"worker-result.txt\",\"sha256\":\"${ARTIFACT_SHA}\"}]}"
esc_delegate="$(printf '%s' "${DELEGATE}" | sed 's/"/\\"/g')"
esc_complete="$(printf '%s' "${COMPLETE}" | sed 's/"/\\"/g')"
REPLIES="[\"${esc_delegate}\",\"${esc_complete}\"]"
LEAD_STATE="${WORK}/lead-state.json"

RUN="$(CODEX_BRIDGE_MOCK_REPLIES="${REPLIES}" CODEX_BRIDGE_MOCK_STATE="${LEAD_STATE}" \
  "${AM}" run --json "Objective: produce ${TOKEN} and report completion.")"

printf '%s' "${RUN}" | grep -q '"status":"succeeded"' || fail "run did not succeed: ${RUN}"
printf '%s' "${RUN}" | grep -q "\"sha256\":\"${ARTIFACT_SHA}\"" || fail "run did not ground the exact worker artifact: ${RUN}"
printf '%s' "${RUN}" | grep -q '"task_refs":\[2\]' || fail "run did not select the worker task: ${RUN}"

RUN_ID="$(printf '%s' "${RUN}" | sed -n 's/.*"run_id":\([0-9]*\).*/\1/p')"
[ -n "${RUN_ID}" ] || fail "run payload carried no run_id: ${RUN}"
RUN_ANSWER="$(printf '%s' "${RUN}" | sed -n 's/.*"answer":"\([^"]*\)".*/\1/p')"
printf '%s' "${RUN_ANSWER}" | grep -q "${TOKEN}" || fail "run answer lost the token: ${RUN}"

# The Lead's own prompt carried the durable worker result: the answer is
# grounded in the board, not in the objective alone.
# The mock state is JSON, so the nested prompt quotes are escaped.
grep -qF 'task_id\":2' "${LEAD_STATE}" || fail "the Lead prompt never saw the worker task result"
grep -q "${ARTIFACT_SHA}" "${LEAD_STATE}" || fail "the Lead prompt never saw the worker artifact digest"

# 3. Fresh processes read the durable state back and must reproduce it.
STATUS="$("${AM}" status --json)"
printf '%s' "${STATUS}" | grep -q '"status":"succeeded"' || fail "status did not show a succeeded run: ${STATUS}"
printf '%s' "${STATUS}" | grep -q "{\"id\":2,\"assignee\":\"worker\",\"status\":\"succeeded\"" || fail "status did not show the succeeded worker child: ${STATUS}"
printf '%s' "${STATUS}" | grep -q "\"sha256\":\"${ARTIFACT_SHA}\"" || fail "status artifact digest mismatch: ${STATUS}"

FINAL="$("${AM}" final --json "${RUN_ID}")"
printf '%s' "${FINAL}" | grep -q "\"answer\":\"${RUN_ANSWER}\"" || fail "final did not reproduce the run answer: ${FINAL}"

ARTIFACT="$("${AM}" artifact --json 2)"
printf '%s' "${ARTIFACT}" | grep -q "\"task_id\":2" || fail "artifact owner is not the worker: ${ARTIFACT}"
printf '%s' "${ARTIFACT}" | grep -q "\"path\":\"worker-result.txt\"" || fail "artifact path is not worker-result.txt: ${ARTIFACT}"
printf '%s' "${ARTIFACT}" | grep -q "\"sha256\":\"${ARTIFACT_SHA}\"" || fail "artifact digest mismatch: ${ARTIFACT}"

# The recorded digest is the digest of the bytes actually on disk.
ON_DISK_SHA="$(sha256sum worker-result.txt | cut -d' ' -f1)"
[ "${ON_DISK_SHA}" = "${ARTIFACT_SHA}" ] || fail "on-disk digest ${ON_DISK_SHA} != recorded ${ARTIFACT_SHA}"

cat <<RECEIPT
AM_SHA256=$(sha256sum "${AM}" | cut -d' ' -f1)
AM_VERSION=$("${AM}" --version)
ROOT_ID=${RUN_ID}
ROOT_STATUS=succeeded
WORKER_TASK_ID=2
WORKER_STATUS=succeeded
ARTIFACT_PATH=worker-result.txt
ARTIFACT_SHA256=${ARTIFACT_SHA}
SELECTED_TASK_MATCH=true
SELECTED_ARTIFACT_MATCH=true
FRESH_READBACK_MATCH=true
LEAD_PROMPT_SAW_WORKER_RESULT=true
NORMAL_PATH_SMOKE=PASS
RECEIPT
