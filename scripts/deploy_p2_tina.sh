#!/usr/bin/env bash
# CV Mode Phase 2 (foveal temporal accumulation) -- deploy the 3 modified
# files to tina and restart broadcast-api. Not a package upgrade: tina is
# on 5.6.0 and this is pre-ship code, same shape the Phase 1 deploy attempt
# should have been.
#
# Run this yourself, in your own terminal -- it needs a real TTY for the
# sudo password on tina. Every step below is failure-gated; nothing here
# echoes success without checking the actual exit code of what produced it.
# The self-check at the end is a courtesy only. It is NOT the source of
# truth -- independent verification (reading the live files back, hitting
# the endpoint) happens from the Claude session separately afterward. This
# is the direct fix for the earlier run where DEPLOY_OK printed and nothing
# had actually landed.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/pkg"
HOST="tina"

echo "== Pre-deploy state on ${HOST} =="
BEFORE_TS=$(ssh "$HOST" "systemctl show -p ActiveEnterTimestamp broadcast-api" | cut -d= -f2)
echo "broadcast-api ActiveEnterTimestamp (before): ${BEFORE_TS}"

echo "== Staging files to ${HOST}:/tmp (no sudo needed for this part) =="
scp "${REPO}/usr/share/hls-livecam-server/cv_processor.py" "${HOST}:/tmp/cv_processor.py.new"
scp "${REPO}/usr/share/hls-livecam-server/cv_detect.py"    "${HOST}:/tmp/cv_detect.py.new"
scp "${REPO}/usr/local/bin/broadcast-api"                  "${HOST}:/tmp/broadcast-api.new"
scp "$(dirname "${BASH_SOURCE[0]}")/deploy_p2_tina_remote.sh" "${HOST}:/tmp/deploy_p2_tina_remote.sh"

echo "== Installing on ${HOST} (sudo password prompt below is real -- type it) =="
ssh -t "$HOST" "bash /tmp/deploy_p2_tina_remote.sh"

echo "== Self-check (courtesy only) =="
AFTER_TS=$(ssh "$HOST" "systemctl show -p ActiveEnterTimestamp broadcast-api" | cut -d= -f2)
echo "broadcast-api ActiveEnterTimestamp (after):  ${AFTER_TS}"
if [ "$AFTER_TS" = "$BEFORE_TS" ]; then
    echo "WARNING: ActiveEnterTimestamp did not change -- restart may not have taken effect"
fi

CODE=$(ssh "$HOST" "curl -s -m 3 -o /dev/null -w '%{http_code}' http://127.0.0.1:5000/api/foveal-mode")
echo "GET /api/foveal-mode HTTP status: ${CODE}"
if [ "$CODE" != "200" ]; then
    echo "WARNING: foveal-mode endpoint not responding 200 -- deploy likely incomplete"
fi

echo
echo "Done running the script. This output is a courtesy check, not confirmation --"
echo "independent verification from the Claude session is still the source of truth."
