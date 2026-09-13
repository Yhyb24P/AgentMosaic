#!/usr/bin/env bash
# Package AgentMosaic release assets from binaries already built.
#
# usage: package_release.sh <release-dir> <source-dir> <out-dir> \
#            [release-commit] [target] [executable-candidate]
#
# Produces, under <out-dir>:
#   agentmosaic-v<version>-<target>.tar.gz
#   SHA256SUMS.txt
#   release-manifest.json
#
# The tarball contains the two shipping binaries `am` and `am-codex-mcp`, the project
# LICENSE, both READMEs and the third-party license report. The TUI is reached through
# `am tui` and has no separate public binary. Test/mock binaries are not packaged.
#
# This script never creates a git tag or a GitHub Release. Publishing a release is a
# separate, explicitly authorized action.
set -euo pipefail

RELEASE_DIR="${1:?usage: package_release.sh <release-dir> <source-dir> <out-dir> [release-commit] [target] [executable-candidate]}"
SOURCE_DIR="${2:?usage: package_release.sh <release-dir> <source-dir> <out-dir> [release-commit] [target] [executable-candidate]}"
OUT_DIR="${3:?usage: package_release.sh <release-dir> <source-dir> <out-dir> [release-commit] [target] [executable-candidate]}"
RELEASE_COMMIT="${4:-unknown}"
TARGET="${5:-x86_64-unknown-linux-gnu}"
EXECUTABLE_CANDIDATE="${6:-unknown}"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "${SOURCE_DIR}/Cargo.toml" | head -1)"
if [[ -z "${VERSION}" ]]; then
    echo "could not read workspace version from ${SOURCE_DIR}/Cargo.toml" >&2
    exit 2
fi
TAG="v${VERSION}"
NAME="agentmosaic-${TAG}-${TARGET}"

STAGE_ROOT="${OUT_DIR}/stage"
PKG="${STAGE_ROOT}/${NAME}"
rm -rf "${STAGE_ROOT}"
mkdir -p "${PKG}"

install -m 0755 "${RELEASE_DIR}/am"           "${PKG}/am"
install -m 0755 "${RELEASE_DIR}/am-codex-mcp" "${PKG}/am-codex-mcp"
install -m 0644 "${SOURCE_DIR}/LICENSE"             "${PKG}/LICENSE"
install -m 0644 "${SOURCE_DIR}/README.md"           "${PKG}/README.md"
install -m 0644 "${SOURCE_DIR}/README.zh-CN.md"     "${PKG}/README.zh-CN.md"
install -m 0644 "${OUT_DIR}/THIRD_PARTY_LICENSES.html" "${PKG}/THIRD_PARTY_LICENSES.html"

RUSTC_VERSION="$(rustc --version | awk '{print $2}')"
MTIME="@$(git -C "${SOURCE_DIR}" show -s --format=%ct HEAD)"

tar --create --gzip \
    --directory="${STAGE_ROOT}" \
    --owner=0 --group=0 --numeric-owner --sort=name \
    --mtime="${MTIME}" \
    --file="${OUT_DIR}/${NAME}.tar.gz" \
    "${NAME}"

CLI_SHA="$(sha256sum "${PKG}/am"           | awk '{print $1}')"
MCP_SHA="$(sha256sum "${PKG}/am-codex-mcp" | awk '{print $1}')"
TARBALL_SHA="$(sha256sum "${OUT_DIR}/${NAME}.tar.gz" | awk '{print $1}')"

cat > "${OUT_DIR}/release-manifest.json" <<JSON
{
  "version": "${VERSION}",
  "tag": "${TAG}",
  "release_commit": "${RELEASE_COMMIT}",
  "executable_source_candidate": "${EXECUTABLE_CANDIDATE}",
  "target": "${TARGET}",
  "rustc": "${RUSTC_VERSION}",
  "asset": "${NAME}.tar.gz",
  "asset_sha256": "${TARBALL_SHA}",
  "binaries": {
    "am": "${CLI_SHA}",
    "am-codex-mcp": "${MCP_SHA}"
  },
  "public_release_ready": false
}
JSON

( cd "${OUT_DIR}" && sha256sum "${NAME}.tar.gz" release-manifest.json > SHA256SUMS.txt )

echo "packaged: ${OUT_DIR}/${NAME}.tar.gz"
echo "tarball sha256: ${TARBALL_SHA}"
echo "contents:"
tar -tzf "${OUT_DIR}/${NAME}.tar.gz"
