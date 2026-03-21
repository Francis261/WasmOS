#!/usr/bin/env bash
set -euo pipefail

export WEBOS_ROOT="${WEBOS_ROOT:-/opt/wasmos}"
export WEBOS_APPS_DIR="${WEBOS_APPS_DIR:-/apps}"
export WEBOS_DATA_DIR="${WEBOS_DATA_DIR:-/data}"
export PORT="${PORT:-8080}"

exec node "$WEBOS_ROOT/server/index.js"
