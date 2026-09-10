#!/usr/bin/env bash
# Deploy the repo's staged pkg/ files to Tanzania's live system paths, with a
# timestamped backup of what's replaced, then reload/restart. Run with sudo:
#     sudo bash deploy-local.sh
set -euo pipefail

REPO="/home/ron/Projects/hls-livecam-server"
BK="/home/ron/Projects/_backups/LIVE-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$BK"

# NOTE: the live docroot is nginx `root /var/www/hls-livecam` -- NOT
# /usr/share/hls-livecam-server (a stale leftover the server does not read).
declare -A MAP=(
  ["$REPO/pkg/usr/share/hls-livecam-server/index.html"]="/var/www/hls-livecam/index.html"
  ["$REPO/pkg/usr/local/bin/broadcast-api"]="/usr/local/bin/broadcast-api"
  ["$REPO/pkg/etc/nginx/conf.d/hls-livecam.conf"]="/etc/nginx/conf.d/hls-livecam.conf"
)
# /etc/hls-livecam/notches.json is deliberately NOT in MAP or the payload:
# it's curated, per-room tuning data (the live 12-entry list is nothing like
# the repo's old seed), and it was never installed over here anyway -- only
# backed up. The .deb payload used to ship a stale seed copy under this same
# path with no conffiles guard, so every package upgrade would have silently
# clobbered a live-tuned list; that seed file is now removed from the
# package entirely (same fix already applied to cams.json once before).
# Live backups of whatever's on disk still happen below, unconditionally.
BK_NOTCHES="/etc/hls-livecam/notches.json"

echo "== backing up live files to $BK =="
for src in "${!MAP[@]}"; do
  dst="${MAP[$src]}"
  [ -f "$dst" ] && cp -a "$dst" "$BK/$(basename "$dst")" && echo "  backed up $dst"
done
[ -f "$BK_NOTCHES" ] && cp -a "$BK_NOTCHES" "$BK/notches.json" && echo "  backed up $BK_NOTCHES"
# vendored player dir (new: the live node is 404 on /vendor/hls.min.js)
[ -f /usr/share/hls-livecam-server/vendor/hls.min.js ] && \
  cp -a /usr/share/hls-livecam-server/vendor/hls.min.js "$BK/hls.min.js.live" || true

echo "== installing =="
install -D -m0644 "$REPO/pkg/usr/share/hls-livecam-server/vendor/hls.min.js" \
        /usr/share/hls-livecam-server/vendor/hls.min.js
echo "  installed /usr/share/hls-livecam-server/vendor/hls.min.js"
install -m0644 "$REPO/pkg/usr/share/hls-livecam-server/index.html" /var/www/hls-livecam/index.html
echo "  installed index.html"
install -m0755 "$REPO/pkg/usr/local/bin/broadcast-api" /usr/local/bin/broadcast-api
echo "  installed broadcast-api"
install -m0644 "$REPO/pkg/etc/nginx/conf.d/hls-livecam.conf" /etc/nginx/conf.d/hls-livecam.conf
echo "  installed nginx conf"

echo "== python syntax check on installed broadcast-api =="
python3 -m py_compile /usr/local/bin/broadcast-api && echo "  broadcast-api: py OK"

echo "== nginx config test + reload =="
nginx -t && systemctl reload nginx && echo "  nginx reloaded"

echo "== restart broadcast-api =="
systemctl restart broadcast-api && echo "  broadcast-api restarted"

echo "== done. verifying =="
sleep 2
curl -s -m8 -o /dev/null -w "  viewer :8080 -> %{http_code}\n" http://127.0.0.1:8080/
curl -s -m8 -o /dev/null -w "  /vendor/hls.min.js -> %{http_code}\n" http://127.0.0.1:8080/vendor/hls.min.js
curl -s -m8 http://127.0.0.1:8080/ | grep -o "/vendor/hls.min.js\|cdnjs" | head -1 | sed 's/^/  viewer player src: /'
echo "  backup of replaced files: $BK"
