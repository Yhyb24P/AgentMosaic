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
JSON_INPUT="${RUN}" python3 - "${TOKEN}" <<'CHECK' || fail "run status/answer invalid"
import json, sys
d=json.loads(__import__("os").environ["JSON_INPUT"])
assert d["status"] == "succeeded" and sys.argv[1] in d["answer"]
CHECK

RUN_ID="$(JSON_INPUT="${RUN}" python3 -c 'import json,os; print(json.loads(os.environ["JSON_INPUT"])["run_id"])')"
[ -n "${RUN_ID}" ] || fail "run payload carried no run_id: ${RUN}"
RUN_ANSWER="$(JSON_INPUT="${RUN}" python3 -c 'import json,os; print(json.loads(os.environ["JSON_INPUT"])["answer"])')"
echo

echo "### status --json (fresh process)"
STATUS="$("${CLI}" status --json)"
echo "${STATUS}"
JSON_INPUT="${STATUS}" python3 - "${RUN_ID}" <<'CHECK' || fail "status is not for run ${RUN_ID}"
import json, os, sys
assert json.loads(os.environ["JSON_INPUT"])["run_id"] == int(sys.argv[1])
CHECK
echo

echo "### final --json ${RUN_ID} (fresh process)"
FINAL="$("${CLI}" final --json "${RUN_ID}")"
echo "${FINAL}"
JSON_INPUT="${FINAL}" python3 - "${RUN_ID}" "${RUN_ANSWER}" <<'CHECK' || fail "final did not reproduce run id and answer"
import json, os, sys
d=json.loads(os.environ["JSON_INPUT"])
assert d["run_id"] == int(sys.argv[1]) and d["answer"] == sys.argv[2]
CHECK
echo

echo "### artifact read-back and independent checks"

# The worker task is the child of the root that owns worker.txt.
WORKER_TASK_ID="$(JSON_INPUT="${STATUS}" python3 -c 'import json,os; d=json.loads(os.environ["JSON_INPUT"]); a=[x["task_id"] for x in d["artifacts"] if x["path"]=="worker.txt"]; assert len(a)==1; print(a[0])')" || fail "no unique worker.txt artifact"
[ -n "${WORKER_TASK_ID}" ] || fail "no task recorded a worker.txt artifact: ${STATUS}"
JSON_INPUT="${STATUS}" python3 - "${RUN_ID}" "${WORKER_TASK_ID}" <<'CHECK' || fail "root-child durable state mismatch"
import json, sys
d=json.loads(__import__("os").environ["JSON_INPUT"]); ts={t["id"]:t for t in d["tasks"]}
assert ts[int(sys.argv[1])]["assignee"]=="lead" and ts[int(sys.argv[1])]["status"]=="succeeded"
assert ts[int(sys.argv[2])]["assignee"]=="worker" and ts[int(sys.argv[2])]["status"]=="succeeded"
CHECK
ARTIFACT="$("${CLI}" artifact --json "${WORKER_TASK_ID}")"
echo "${ARTIFACT}"
PERSISTED_SHA="$(printf '%s' "${ARTIFACT}" | sed -n 's/.*"sha256":"\([0-9a-f]\{64\}\)".*/\1/p' | head -n1)"
[ -n "${PERSISTED_SHA}" ] || fail "artifact payload carried no sha256: ${ARTIFACT}"
JSON_INPUT="${ARTIFACT}" python3 - "${WORKER_TASK_ID}" "${PERSISTED_SHA}" <<'CHECK' || fail "artifact task/path/hash mismatch"
import json, sys
a=json.loads(__import__("os").environ["JSON_INPUT"])["artifacts"]
assert a == [{"task_id":int(sys.argv[1]),"path":"worker.txt","sha256":sys.argv[2]}]
CHECK

[ -f "${REPO}/worker.txt" ] || fail "the worker never wrote worker.txt"
EXACT="$(cat "${REPO}/worker.txt")"
[ "${EXACT}" = "worker=complete ${TOKEN}" ] || fail "worker.txt content is not the exact requested line: ${EXACT}"
[ "$(wc -c < "${REPO}/worker.txt" | tr -d ' ')" = "$(( ${#EXACT} + 1 ))" ] || fail "worker.txt has trailing content beyond one newline"
ON_DISK_SHA="$(sha256sum "${REPO}/worker.txt" | cut -d' ' -f1)"
[ "${ON_DISK_SHA}" = "${PERSISTED_SHA}" ] || fail "persisted sha ${PERSISTED_SHA} != on-disk ${ON_DISK_SHA}"
JSON_INPUT="${RUN}" python3 - "${WORKER_TASK_ID}" "${PERSISTED_SHA}" "${TOKEN}" <<'CHECK' || fail "run selected refs did not match worker evidence"
import json, sys
d=json.loads(__import__("os").environ["JSON_INPUT"])
assert d["task_refs"] == [int(sys.argv[1])]
assert d["artifact_refs"] == [{"task_id":int(sys.argv[1]),"path":"worker.txt","sha256":sys.argv[2]}]
assert d["status"] == "succeeded" and sys.argv[3] in d["answer"]
CHECK

# Parent is intentionally checked through the existing legacy read-only view;
# status --json has no parent field and the public contract stays unchanged.
LEGACY_STATUS="$("${CLI}" status "${DB}")"
printf '%s\n' "${LEGACY_STATUS}" | grep -Eq "task=${WORKER_TASK_ID} status=succeeded assignee=worker attempts=[0-9]+ parent=${RUN_ID}" || fail "worker is not a succeeded child of root: ${LEGACY_STATUS}"
printf '%s\n' "${LEGACY_STATUS}" | grep -Eq "task=${RUN_ID} status=succeeded assignee=lead attempts=[0-9]+ parent=-" || fail "root is not a succeeded lead root: ${LEGACY_STATUS}"
BINDING="$("${CLI}" binding "${DB}" "${WORKER_TASK_ID}")"
echo "${BINDING}"
printf '%s\n' "${BINDING}" | grep -q 'agent=worker' || fail "binding agent mismatch"
printf '%s\n' "${BINDING}" | grep -q 'runtime_kind=acp' || fail "binding runtime mismatch"
printf '%s\n' "${BINDING}" | grep -q 'lifecycle_state=completed' || fail "binding is not completed"
printf '%s\n' "${BINDING}" | grep -q 'external_reference_present=true' || fail "binding has no external reference"

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
echo "ROOT_CHILD_MATCH=true"
echo "ARTIFACT_HASH_MATCH=true"
echo "FRESH_READBACK_MATCH=true"
echo "RC_RELEASE_SMOKE=PASS"
