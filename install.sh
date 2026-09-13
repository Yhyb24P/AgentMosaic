#!/bin/sh
# AgentMosaic v0.2 installer entrypoint.
# A generated cargo-dist installer will replace this guard only after a
# compatible v0.2 GitHub Release exists. Never install v0.1 under v0.2 UX.
set -eu
printf '%s\n' 'AgentMosaic v0.2 installer is not published yet. No software was installed.' >&2
exit 1
