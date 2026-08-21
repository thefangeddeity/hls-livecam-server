#!/usr/bin/env bash
# Point tina's CV render at the same view tanzania uses.
#
# tina currently runs CV_EDGE_STYLE=sharpie with aggressive edge/unsharp
# values that were tuned FOR sharpie. Rendering tina's real frames three
# ways showed plainly that those overrides -- not the overlay style itself
# -- are what make overlay ugly on this sensor:
#
#   sharpie (current)          topographic contour lines, abstract
#   overlay + tina's overrides manufactured halos and false texture
#   overlay + tanzania's config soft but genuinely readable -- side table,
#                              plant, couch surface, feeder, cabinet
#
# The reason is CV_EDGE_SHARPNESS_MIN/OFF: at defaults (35/70) tina's
# measured sharpness (~36) sits just above the floor, so the edge stage
# barely engages and the image is left alone. tina's CV_EDGE_SHARPNESS_OFF=120
# forced it fully on at all times.
#
# So this REMOVES tina's overrides rather than adding new keys.
set -euo pipefail
F=/etc/hls-livecam/device.env
sudo cp "$F" "$F.bak-$(date +%Y%m%d-%H%M%S)"
sudo sed -i \
  -e 's/^CV_EDGE_STYLE=sharpie/CV_EDGE_STYLE=overlay/' \
  -e '/^CV_EDGE_STRENGTH=/d' \
  -e '/^CV_EDGE_SHARPNESS_OFF=/d' \
  -e '/^CV_UNSHARP_AMOUNT=/d' \
  -e '/^CV_SHARPIE_/d' \
  "$F"
# tina's cat-only detector narrowing, as config instead of a source edit
# that every package upgrade reverts (it is currently a hand-edit in
# cv_detect.py that no released version has ever shipped).
grep -q '^CV_DETECT_CLASSES=' "$F" || echo 'CV_DETECT_CLASSES=cat' | sudo tee -a "$F" >/dev/null
sudo systemctl restart broadcast-api
sleep 2
systemctl is-active broadcast-api
echo "--- effective CV config now ---"
grep -E '^CV_' "$F"
