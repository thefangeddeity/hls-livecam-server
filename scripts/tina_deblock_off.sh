#!/usr/bin/env bash
# Revert: turn the grid correction back off on tina. Leaves everything else
# (renderer, acuity, cat-only narrowing) exactly as it is.
set -euo pipefail
F=/etc/hls-livecam/device.env
ssh -t tina "sudo sed -i 's/^CV_GRID_CORRECT_ENABLED=.*/CV_GRID_CORRECT_ENABLED=0/' $F && \
  sudo systemctl restart broadcast-api && sleep 3 && systemctl is-active broadcast-api && \
  grep -E '^CV_EDGE_STYLE|^CV_GRID_' $F"
