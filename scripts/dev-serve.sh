#!/usr/bin/env bash
set -euo pipefail
export WEBOS_ROOT="$(pwd)"
export WEBOS_APPS_DIR="$(pwd)/web/apps"
export WEBOS_DATA_DIR="$(pwd)/.data"
mkdir -p "$WEBOS_DATA_DIR/shared" "$WEBOS_DATA_DIR/apps"
node server/index.js
