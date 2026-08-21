#!/usr/bin/env bash
# v5.8.1 to both nodes. Run this yourself -- it needs a real TTY for sudo.
# Every step is failure-gated. The checks it prints are a courtesy only;
# independent verification happens from the Claude session afterward.
set -euo pipefail

ARCH_PKG=/tmp/aurbuild/hls-livecam-server-5.8.1-1-any.pkg.tar.zst
DEB=/tmp/debbuild/hls-livecam-server_5.8.1_amd64.deb

echo "== tanzania (local, Arch) =="
test -f "$ARCH_PKG" || { echo "missing $ARCH_PKG"; exit 1; }
sudo pacman -U --noconfirm "$ARCH_PKG"
sudo systemctl restart broadcast-api
sleep 2
systemctl is-active broadcast-api

echo
echo "== tina (Debian, over ssh) =="
test -f "$DEB" || { echo "missing $DEB"; exit 1; }
scp "$DEB" tina:/tmp/hls-livecam-server_5.8.1_amd64.deb
ssh -t tina 'sudo dpkg -i /tmp/hls-livecam-server_5.8.1_amd64.deb && sudo systemctl restart broadcast-api && sleep 2 && systemctl is-active broadcast-api && rm -f /tmp/hls-livecam-server_5.8.1_amd64.deb'

echo
echo "Both install steps returned success. This is a courtesy check, not"
echo "confirmation -- independent verification from the Claude session is"
echo "still the source of truth."
