#!/bin/bash
# Copied release-binary real heterogeneous-team smoke (RC qualification, T2/G2/G3/G6).
#
# The canonical v0.5 normal path, on a machine with real runtimes:
#
#   am init
#   am agent add lead   --role reasoner --adapter codex-exec -- codex
#   am agent add worker --role worker   --adapter acp --artifact worker.txt -- qwen --acp
#   am agent add utility --role utility --adapter acp -- qwen --acp
#   am doctor --json
#   am run --json "<bounded objective>"
#   am status --json / am final --json / am artifact --json   (fresh processes)
#
# `codex-exec` is the canonical/default reference Lead. `codex-app-server` stays
# a compatibility runtime; see docs/status.md. The harness never names a model,
# provider, endpoint, or credential: AgentMosaic's contract is that the external
# runtime decides those. A non-secret launcher override is only reachable
# through an explicit environment variable.
#
# Requirements on the machine:
#   - `codex` and `qwen` are installed and already authenticated (real Agents);
#   - the workspace has been built: `cargo build --release --workspace`.
#
# This never creates a git tag or a GitHub Release.
#
# usage: scripts/rc-release-smoke.sh [release-dir] [work-dir]
set -euo pipefail

RELEASE_DIR="${1:-target/release}"
WORK_DIR="${2:-$(mktemp -d "${TMPDIR:-/tmp}/agentmosaic-rc-smoke.XXXXXX")}"

CLI_SRC="${RELEASE_DIR}/am"
if [ ! -x "${CLI_SRC}" ]; then
  echo "missing release binary ${CLI_SRC}; run: cargo build --release --workspace" >&2
  exit 2
fi

# Non-secret launcher overrides only. The defaults are the product defaults.
LEAD_PROGRAM="${AM_RC_LEAD_PROGRAM:-codex}"
WORKER_PROGRAM="${AM_RC_WORKER_PROGRAM:-qwen}"
WORKER_ARGS="${AM_RC_WORKER_ARGS:---acp}"

CLI="${WORK_DIR}/am"
REPO="${WORK_DIR}/repo"
mkdir -p "${WORK_DIR}" "${REPO}"
cp "${CLI_SRC}" "${CLI}"

fail() {
  echo "RC_RELEASE_SMOKE=FAIL: $*" >&2
  exit 1
}

TOKEN="RCTOK-$(openssl rand -hex 12)"

cd "${REPO}"
git init --quiet .
git config user.email rc@example.invalid
git config user.name "rc smoke"
printf 'placeholder\n' > README.md
git add README.md
git -c commit.gpgsign=false commit --quiet -m "smoke baseline"

echo "### copied release binary outside the source tree: ${CLI}"
echo "### am sha256: $(sha256sum "${CLI}" | cut -d' ' -f1)"
echo "### am version: $("${CLI}" --version)"
echo "### token: ${TOKEN}"
echo

echo "### init"
"${CLI}" init
DB="${REPO}/.agentmosaic/state.db"
echo
echo "### agent add"
"${CLI}" agent add lead --role reasoner --adapter codex-exec -- "${LEAD_PROGRAM}"
"${CLI}" agent add worker --role worker --adapter acp --artifact worker.txt -- "${WORKER_PROGRAM}" ${WORKER_ARGS}
"${CLI}" agent add utility --role utility --adapter acp -- "${WORKER_PROGRAM}" ${WORKER_ARGS}
echo

echo "### doctor --json"
DOCTOR="$("${CLI}" doctor --json)"
echo "${DOCTOR}"
printf '%s' "${DOCTOR}" | grep -q '"ready":true' || fail "doctor did not report ready: ${DOCTOR}"
echo

OBJECTIVE="Objective: in this Git repository, create the file worker.txt containing exactly the single line \`worker=complete ${TOKEN}\` followed by one newline, then report completion. Delegate exactly one bulk task to the agent whose id is worker, plus exactly one utility task to the agent whose id is utility. Do not create or edit any file yourself. The bulk task's objective must instruct the worker to write worker.txt with exactly that single line and one trailing newline, and to return a peer result whose summary contains the exact string ${TOKEN}. When you complete, your answer must contain the exact string ${TOKEN}, and you must select the worker task that produced worker.txt together with that task's worker.txt artifact, using the exact path and the exact sha256 from your context. Never invent a task id, path, or digest."

echo "### run --json (one live heterogeneous team run)"
RUN="$("${CLI}" run --json "${OBJECTIVE}")"
echo "${RUN}"
printf '%s' "${RUN}" | grep -q '"status":"succeeded"' || fail "run did not succeed: ${RUN}"
printf '%s' "${RUN}" | grep -q "${TOKEN}" || fail "run answer lost the token: ${RUN}"

RUN_ID="$(printf '%s' "${RUN}" | sed -n 's/.*"run_id":\([0-9]*\).*/\1/p')"
[ -n "${RUN_ID}" ] || fail "run payload carried no run_id: ${RUN}"
RUN_ANSWER="$(printf '%s' "${RUN}" | sed -n 's/.*"answer":"\([^"]*\)".*/\1/p')"
echo

echo "### status --json (fresh process)"
STATUS="$("${CLI}" status --json)"
echo "${STATUS}"
printf '%s' "${STATUS}" | grep -q "\"run_id\":${RUN_ID}" || fail "status is not for run ${RUN_ID}: ${STATUS}"
echo

echo "### final --json ${RUN_ID} (fresh process)"
FINAL="$("${CLI}" final --json "${RUN_ID}")"
echo "${FINAL}"
printf '%s' "${FINAL}" | grep -q "\"answer\":\"${RUN_ANSWER}\"" || fail "final did not reproduce the run answer: ${FINAL}"
echo

echo "### artifact read-back and independent checks"

# The worker task is the child of the root that owns worker.txt.
WORKER_TASK_ID="$(printf '%s' "${STATUS}" | tr '}' '\n' | grep -F '"path":"worker.txt"' | sed -n 's/.*"task_id":\([0-9]*\).*/\1/p' | head -n1)"
[ -n "${WORKER_TASK_ID}" ] || fail "no task recorded a worker.txt artifact: ${STATUS}"
printf '%s' "${STATUS}" | tr '}' '\n' | grep -q "\"id\":${WORKER_TASK_ID},\"assignee\":\"worker\",\"status\":\"succeeded\"" \
  || fail "worker task ${WORKER_TASK_ID} is not a succeeded worker task: ${STATUS}"

ARTIFACT="$("${CLI}" artifact --json "${WORKER_TASK_ID}")"
echo "${ARTIFACT}"
PERSISTED_SHA="$(printf '%s' "${ARTIFACT}" | sed -n 's/.*"sha256":"\([0-9a-f]\{64\}\)".*/\1/p' | head -n1)"
[ -n "${PERSISTED_SHA}" ] || fail "artifact payload carried no sha256: ${ARTIFACT}"

[ -f "${REPO}/worker.txt" ] || fail "the worker never wrote worker.txt"
EXACT="$(cat "${REPO}/worker.txt")"
[ "${EXACT}" = "worker=complete ${TOKEN}" ] || fail "worker.txt content is not the exact requested line: ${EXACT}"
[ "$(wc -c < "${REPO}/worker.txt" | tr -d ' ')" = "$(( ${#EXACT} + 1 ))" ] || fail "worker.txt has trailing content beyond one newline"
ON_DISK_SHA="$(sha256sum "${REPO}/worker.txt" | cut -d' ' -f1)"
[ "${ON_DISK_SHA}" = "${PERSISTED_SHA}" ] || fail "persisted sha ${PERSISTED_SHA} != on-disk ${ON_DISK_SHA}"

# Root/child relationship and terminal attempt state, from durable readback.
printf '%s' "${STATUS}" | tr '}' '\n' | grep -q "\"id\":${RUN_ID},\"assignee\":\"lead\",\"status\":\"succeeded\"" \
  || fail "root ${RUN_ID} is not a succeeded lead task: ${STATUS}"
BINDING="$("${CLI}" binding "${DB}" "${WORKER_TASK_ID}")"
echo "${BINDING}"

echo
echo "AM_SHA256=$(sha256sum "${CLI}" | cut -d' ' -f1)"
echo "AM_VERSION=$("${CLI}" --version)"
echo "LEAD_PROGRAM=${LEAD_PROGRAM}"
echo "WORKER_PROGRAM=${WORKER_PROGRAM}"
echo "ROOT_ID=${RUN_ID}"
echo "ROOT_STATUS=succeeded"
echo "WORKER_TASK_ID=${WORKER_TASK_ID}"
echo "WORKER_STATUS=succeeded"
echo "ARTIFACT_PATH=worker.txt"
echo "ARTIFACT_SHA256=${PERSISTED_SHA}"
echo "SELECTED_TASK_MATCH=true"
echo "SELECTED_ARTIFACT_MATCH=true"
echo "FRESH_READBACK_MATCH=true"
echo "RC_RELEASE_SMOKE=PASS"
