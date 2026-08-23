#!/usr/bin/env bash
# tests/smoke.sh — end-to-end health check for a running hls-livecam.
# Assumes `livecam start` has been run. Exits non-zero on the first failure.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

WEB="$(grep -E '^WEB_PORT='  config.env | cut -d= -f2 | tr -d ' ')"; WEB="${WEB:-8080}"
HLS="$(grep -E '^HLS_PORT='  config.env | cut -d= -f2 | tr -d ' ')"; HLS="${HLS:-8888}"
RTSP="$(grep -E '^RTSP_PORT=' config.env | cut -d= -f2 | tr -d ' ')"; RTSP="${RTSP:-8554}"

pass=0; fail=0
ck() { # ck "name" cmd...
  local name="$1"; shift
  if "$@" >/dev/null 2>&1; then echo "  ok    $name"; pass=$((pass+1))
  else echo "  FAIL  $name"; fail=$((fail+1)); fi
}
ck_eq() { # ck_eq "name" expected actual
  if [ "$2" = "$3" ]; then echo "  ok    $1"; pass=$((pass+1))
  else echo "  FAIL  $1 (want '$2' got '$3')"; fail=$((fail+1)); fi
}

echo "hls-livecam smoke test"

ck "viewer served"        curl -sf "http://127.0.0.1:$WEB/"
ck "HLS m3u8 200"         curl -sf "http://127.0.0.1:$HLS/cam/index.m3u8"
ck "api/feed-mode"        curl -sf "http://127.0.0.1:$WEB/api/feed-mode"
ck "api/dark"             curl -sf "http://127.0.0.1:$WEB/api/dark"

# broadcast round-trip
curl -sf -X POST "http://127.0.0.1:$WEB/api/broadcast" --data 'smoke-test-msg' >/dev/null 2>&1
ck_eq "broadcast round-trip" "smoke-test-msg" "$(curl -sf "http://127.0.0.1:$WEB/broadcast.txt")"
curl -sf -X POST "http://127.0.0.1:$WEB/api/broadcast" --data '' >/dev/null 2>&1  # clear

# decode a real frame from HLS
if ffmpeg -hide_banner -loglevel error -i "http://127.0.0.1:$HLS/cam/index.m3u8" \
    -frames:v 1 -y /tmp/livecam_smoke.jpg >/dev/null 2>&1 && \
    [ -s /tmp/livecam_smoke.jpg ]; then
  echo "  ok    decode HLS frame"; pass=$((pass+1))
else
  echo "  FAIL  decode HLS frame"; fail=$((fail+1))
fi
rm -f /tmp/livecam_smoke.jpg

echo "---"
echo "passed: $pass   failed: $fail"
[ "$fail" -eq 0 ]
