#!/usr/bin/env bash
# Package the v0.1.0 release assets (P9).
#
# Builds, from binaries and files already present:
#   research-agent-system-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
#   SHA256SUMS.txt
#   release-manifest.json
#
# The tarball intentionally contains only the shipping binaries, the project
# LICENSE, both READMEs and the third-party license report. The workspace also
# builds `acp_m2_mock` and `codex_bridge_mock`; those are test/mock binaries and
# are deliberately NOT packaged.
set -euo pipefail

RELEASE_DIR="${1:?usage: package_release.sh <release-dir> <source-dir> <out-dir> [release-commit]}"
SOURCE_DIR="${2:?usage: package_release.sh <release-dir> <source-dir> <out-dir> [release-commit]}"
OUT_DIR="${3:?usage: package_release.sh <release-dir> <source-dir> <out-dir> [release-commit]}"
RELEASE_COMMIT="${4:-unknown}"

VERSION="0.1.0"
TAG="v0.1.0"
TARGET="x86_64-unknown-linux-gnu"
EXECUTABLE_FREEZE="89ac979d333fe3fc2e311fb566f3ab0056bec4c5"
NAME="research-agent-system-${TAG}-${TARGET}"

STAGE_ROOT="${OUT_DIR}/stage"
PKG="${STAGE_ROOT}/${NAME}"
rm -rf "${STAGE_ROOT}"
mkdir -p "${PKG}"

install -m 0755 "${RELEASE_DIR}/agent-code-cli" "${PKG}/agent-code-cli"
install -m 0755 "${RELEASE_DIR}/agent-code-tui" "${PKG}/agent-code-tui"
install -m 0755 "${RELEASE_DIR}/ras_codex_mcp"  "${PKG}/ras_codex_mcp"
install -m 0644 "${SOURCE_DIR}/LICENSE"             "${PKG}/LICENSE"
install -m 0644 "${SOURCE_DIR}/README.md"           "${PKG}/README.md"
install -m 0644 "${SOURCE_DIR}/README.zh-CN.md"     "${PKG}/README.zh-CN.md"
install -m 0644 "${OUT_DIR}/THIRD_PARTY_LICENSES.html" "${PKG}/THIRD_PARTY_LICENSES.html"

RUSTC_VERSION="$(rustup run 1.98.1 rustc --version | awk '{print $2}')"
MTIME="@$(git -C "${SOURCE_DIR}" show -s --format=%ct HEAD)"

tar --create --gzip \
    --directory="${STAGE_ROOT}" \
    --owner=0 --group=0 --numeric-owner --sort=name \
    --mtime="${MTIME}" \
    --file="${OUT_DIR}/${NAME}.tar.gz" \
    "${NAME}"

CLI_SHA="$(sha256sum "${PKG}/agent-code-cli"   | awk '{print $1}')"
TUI_SHA="$(sha256sum "${PKG}/agent-code-tui"   | awk '{print $1}')"
MCP_SHA="$(sha256sum "${PKG}/ras_codex_mcp"    | awk '{print $1}')"
TARBALL_SHA="$(sha256sum "${OUT_DIR}/${NAME}.tar.gz" | awk '{print $1}')"

cat > "${OUT_DIR}/release-manifest.json" <<JSON
{
  "version": "${VERSION}",
  "tag": "${TAG}",
  "release_commit": "${RELEASE_COMMIT}",
  "executable_source_candidate": "${EXECUTABLE_FREEZE}",
  "target": "${TARGET}",
  "rustc": "${RUSTC_VERSION}",
  "asset": "${NAME}.tar.gz",
  "asset_sha256": "${TARBALL_SHA}",
  "binaries": {
    "agent-code-cli": "${CLI_SHA}",
    "agent-code-tui": "${TUI_SHA}",
    "ras_codex_mcp": "${MCP_SHA}"
  },
  "reference_profile": {
    "codex": "0.154.0",
    "qwen_code": "0.23.3"
  },
  "public_release_ready": false
}
JSON

( cd "${OUT_DIR}" && sha256sum "${NAME}.tar.gz" release-manifest.json > SHA256SUMS.txt )

echo "packaged: ${OUT_DIR}/${NAME}.tar.gz"
echo "tarball sha256: ${TARBALL_SHA}"
echo "contents:"
tar -tzf "${OUT_DIR}/${NAME}.tar.gz"
