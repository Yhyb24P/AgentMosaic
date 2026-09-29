#!/usr/bin/env bash
# Reject tracked runtime data and retired product documentation.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
git diff --check
if git ls-files -- . ':(exclude)crates/agentmosaic-storage/tests/fixtures/v0_3_0_state.db' \
  | grep -Eq '(^|/)([^/]+\.db(-wal|-shm)?|[^/]+-evidence(-check)?\.json)$'; then
  echo "runtime database or external evidence is tracked" >&2
  exit 1
fi
if git ls-files docs/audits docs/demo docs/experiments docs/releases docs/benchmarks \
  V0_4_RUNTIME_INTEGRATION_EVIDENCE.md V0_5_CORE_SIMPLIFICATION_EVIDENCE.md | grep -q .; then
  echo "retired documentation is tracked" >&2
  exit 1
fi
