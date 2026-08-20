#!/usr/bin/env bash
# Runs ON tina, invoked by deploy_p2_tina.sh via `ssh -t`. Real TTY, real
# sudo password prompt. Every step is failure-gated (set -e) -- if any cp
# or restart fails, the script stops instead of printing success anyway.
set -euo pipefail

echo "-- installing CV Mode Phase 1+2 files (sudo password prompt is real) --"
sudo cp /tmp/cv_processor.py.new /usr/share/hls-livecam-server/cv_processor.py
sudo cp /tmp/cv_detect.py.new    /usr/share/hls-livecam-server/cv_detect.py
sudo cp /tmp/broadcast-api.new   /usr/local/bin/broadcast-api
sudo chmod 755 /usr/local/bin/broadcast-api
sudo chmod 644 /usr/share/hls-livecam-server/cv_processor.py /usr/share/hls-livecam-server/cv_detect.py

echo "-- restarting broadcast-api --"
sudo systemctl stop broadcast-api
sudo systemctl start broadcast-api
sleep 1
sudo systemctl is-active broadcast-api

echo "-- cleaning up staged files --"
rm -f /tmp/cv_processor.py.new /tmp/cv_detect.py.new /tmp/broadcast-api.new /tmp/deploy_p2_tina_remote.sh

echo "remote install steps completed without error"
