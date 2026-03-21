#!/usr/bin/env bash
set -euo pipefail

URL="${WEBOS_URL:-http://127.0.0.1:8080/index.html}"

exec chromium \
  --kiosk \
  --incognito \
  --no-first-run \
  --disable-features=Translate,MediaRouter,OptimizationHints \
  --disk-cache-dir=/tmp/chromium-cache \
  --disk-cache-size=10485760 \
  --disable-component-update \
  --disable-background-networking \
  --disable-sync \
  --disable-extensions \
  --disable-session-crashed-bubble \
  --overscroll-history-navigation=0 \
  --autoplay-policy=no-user-gesture-required \
  "$URL"
