#!/usr/bin/env bash
set -euo pipefail

export WEBOS_ROOT="/opt/wasmos"
export WEBOS_APPS_DIR="${WEBOS_APPS_DIR:-/apps}"
export WEBOS_DATA_DIR="${WEBOS_DATA_DIR:-/data}"

mkdir -p "$WEBOS_APPS_DIR" "$WEBOS_DATA_DIR/shared" "$WEBOS_DATA_DIR/apps"

/usr/local/bin/start-backend.sh &
BACKEND_PID=$!

cleanup() {
  kill "$BACKEND_PID" 2>/dev/null || true
}
trap cleanup EXIT

exec /usr/local/bin/kiosk-chromium.sh
