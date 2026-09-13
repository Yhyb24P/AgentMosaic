#!/usr/bin/env bash
# AgentMosaic identity gate.
#
# Rejects retired, actively-shipped identities in the current tree while allowing the
# narrow, truthful historical documents. Also asserts that no retired working-history
# residue stays tracked and that every workspace package carries the AgentMosaic prefix.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

retired='Research Agent System|research-agent-system|agent-code-|agent_code_|ras_codex_mcp|v2/rust-agent-team|docs/v2'
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

git grep -nE "$retired" -- ':!CHANGELOG.md' ':!docs/releases/v0.1.0.md' ':!docs/history.md' ':!scripts/ci/check_identity.sh' >"$tmp" || true
if [[ -s "$tmp" ]]; then
  echo 'stale active identity:'
  cat "$tmp"
  exit 1
fi

for p in .acc-evidence .audit implementation_report.md product_self_audit.md \
  product_self_reaudit.md docs/v2; do
  if git ls-files "$p" "$p/**" | grep -q .; then
    echo "historical residue tracked: $p"
    exit 1
  fi
done

python3 - <<'PY'
import json
import subprocess

meta = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"], text=True
    )
)
bad = [p["name"] for p in meta["packages"] if not p["name"].startswith("agentmosaic-")]
if bad:
    raise SystemExit(f"non-AgentMosaic packages: {bad}")

bins = {
    target["name"]
    for pkg in meta["packages"]
    for target in pkg["targets"]
    if "bin" in target["kind"]
}
if "am" not in bins:
    raise SystemExit("missing public `am` binary target")
if "am-codex-mcp" in bins:
    raise SystemExit("retired `am-codex-mcp` must not be a product binary target")
PY

echo 'identity gate: PASS'
