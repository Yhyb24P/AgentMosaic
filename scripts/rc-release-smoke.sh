#!/bin/bash
# Copied release-binary real heterogeneous-team smoke (RC qualification, R12).
#
# Copies the release CLI and the AgentMosaic Codex MCP bridge OUT of the source tree into a
# scratch directory, registers a real Codex Lead and real Qwen Worker/Utility agents
# through the public CLI, issues exactly one `run-team` for one objective, and reads the
# durable result back in separate processes.
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
MCP_SRC="${RELEASE_DIR}/am-codex-mcp"
for binary in "${CLI_SRC}" "${MCP_SRC}"; do
  if [ ! -x "${binary}" ]; then
    echo "missing release binary ${binary}; run: cargo build --release --workspace" >&2
    exit 2
  fi
done

CLI="${WORK_DIR}/am"
MCP="${WORK_DIR}/am-codex-mcp"
DB="${WORK_DIR}/team.db"
REPO="${WORK_DIR}/repo"
LOG="${WORK_DIR}/smoke.log"
mkdir -p "${WORK_DIR}" "${REPO}"
cp "${CLI_SRC}" "${CLI}"
cp "${MCP_SRC}" "${MCP}"

TOKEN="RCTOK-$(openssl rand -hex 12)"
CODEX_CFG=$(printf '{"mcp_command":"%s","artifact_paths":["lead-final.txt"],"max_events":200,"model":"gpt-5.5","overrides":["model=\\"gpt-5.5\\"","model_reasoning_effort=\\"low\\""]}' "${MCP}")
QWEN_CFG='{"auth_method":"openai","timeout_seconds":600,"max_result_bytes":4096,"artifact_paths":["worker.txt"]}'
UTIL_CFG='{"auth_method":"openai","timeout_seconds":600,"max_result_bytes":4096}'

cd "${REPO}"
git init --quiet .
git config user.email rc@example.invalid
git config user.name "rc smoke"
printf 'placeholder\n' > README.md
git add README.md
git commit --quiet -m "smoke baseline"

OBJECTIVE="Objective: in this repository, make the file worker.txt contain exactly the single line \`worker=complete ${TOKEN}\` followed by one newline, and then report completion. In your first round delegate exactly two tasks: (1) a bulk task targeted at agent id qwen-worker whose objective instructs the worker to write worker.txt with exactly that line and to return a peer JSON summary that contains the string ${TOKEN}; and (2) a utility task targeted at agent id qwen-utility whose objective asks it to reply that it has nothing to do. Do not write files yourself. When you complete, your answer must contain the string ${TOKEN}, and you must select the qwen-worker task that produced worker.txt together with that task's worker.txt artifact."

{
  echo "### copied binaries outside the source tree: ${WORK_DIR}"
  echo "### cli sha256: $(sha256sum "${CLI}" | cut -d' ' -f1)"
  echo "### mcp sha256: $(sha256sum "${MCP}" | cut -d' ' -f1)"
  echo "### token: ${TOKEN}"
  echo
  echo "### register"
  "${CLI}" register "${DB}" codex-lead codex-lead reasoner codex-app-server codex - 1 - - "${CODEX_CFG}"
  "${CLI}" register "${DB}" qwen-worker qwen-worker worker acp qwen --acp 1 - - "${QWEN_CFG}"
  "${CLI}" register "${DB}" qwen-utility qwen-utility utility acp qwen --acp 1 - - "${UTIL_CFG}"
  echo
  echo "### run-team (public product entrypoint, copied release binary)"
  "${CLI}" run-team "${DB}" "${REPO}" "${OBJECTIVE}"
  echo "run_team_exit=$?"
  echo
  echo "### status"
  "${CLI}" status "${DB}"
  echo
  echo "### final 1 (new process, fresh connection)"
  "${CLI}" final "${DB}" 1
  echo
  echo "### final 1 again (second new process)"
  "${CLI}" final "${DB}" 1
  echo
  echo "### worker final 2"
  "${CLI}" final "${DB}" 2
  echo
  echo "### artifacts"
  "${CLI}" artifact "${DB}" 2
  echo
  echo "### binding 2"
  "${CLI}" binding "${DB}" 2
  echo
  echo "### on-disk worker.txt"
  cat "${REPO}/worker.txt"
  echo "sha256: $(sha256sum "${REPO}/worker.txt" | cut -d' ' -f1)"
  echo
  echo "### dependency check"
  "${CLI}" final "${DB}" 1 | grep -q "${TOKEN}" && echo "TOKEN_IN_ROOT_ANSWER=yes" || echo "TOKEN_IN_ROOT_ANSWER=no"
  "${CLI}" final "${DB}" 2 | grep -q "${TOKEN}" && echo "TOKEN_IN_WORKER_RESULT=yes" || echo "TOKEN_IN_WORKER_RESULT=no"
} > "${LOG}" 2>&1

echo "log: ${LOG}"
echo "log sha256: $(sha256sum "${LOG}" | cut -d' ' -f1)"
grep -E "run_team_exit|TOKEN_IN_|^root=|task_refs|artifact_refs" "${LOG}"
