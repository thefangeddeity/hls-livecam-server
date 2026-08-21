#!/usr/bin/env bash
# Enable the codec block-grid correction on tina at strength 0.6, live.
#
# Does three things in one go, deliberately: the new cv_processor.py
# IMPLEMENTS xdog, and tina's device.env still says CV_EDGE_STYLE=xdog from
# when that value was inert. Deploying the module without also pinning the
# style would silently switch tina's renderer to XDoG on restart -- the one
# change we agreed not to make. So the style is pinned to overlay (its
# actual current behaviour) in the same operation.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SHARE=/usr/share/hls-livecam-server
F=/etc/hls-livecam/device.env

echo "== staging modules (no sudo: $SHARE is ron-owned on tina) =="
for f in cv_processor.py cv_occupancy.py cv_persist.py cv_detect.py cv_scene.py; do
  scp -q "$REPO/pkg$SHARE/$f" "tina:$SHARE/$f"
  echo "   $f"
done

echo "== device.env: pin style, enable deblock (sudo prompt below is real) =="
ssh -t tina "sudo cp $F $F.bak-\$(date +%Y%m%d-%H%M%S) && \
  sudo sed -i 's/^CV_EDGE_STYLE=xdog/CV_EDGE_STYLE=overlay/' $F && \
  (grep -q '^CV_GRID_CORRECT_ENABLED=' $F \
     && sudo sed -i 's/^CV_GRID_CORRECT_ENABLED=.*/CV_GRID_CORRECT_ENABLED=1/' $F \
     || echo 'CV_GRID_CORRECT_ENABLED=1' | sudo tee -a $F >/dev/null) && \
  (grep -q '^CV_GRID_CORRECT_STRENGTH=' $F \
     || echo 'CV_GRID_CORRECT_STRENGTH=0.6' | sudo tee -a $F >/dev/null) && \
  (grep -q '^CV_DETECT_CLASSES=' $F \
     && sudo sed -i 's/^CV_DETECT_CLASSES=.*/CV_DETECT_CLASSES=cat,human/' $F \
     || echo 'CV_DETECT_CLASSES=cat,human' | sudo tee -a $F >/dev/null) && \
  sudo systemctl restart broadcast-api && sleep 3 && systemctl is-active broadcast-api"

echo
echo "== effective config =="
ssh tina "grep -E '^CV_EDGE_STYLE|^CV_GRID_|^CV_DETECT_CLASSES' $F"
echo
echo "Run scripts/tina_deblock_off.sh to revert."
