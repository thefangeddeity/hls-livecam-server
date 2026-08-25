#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/hls-livecam-dmg.XXXXXX")"
APP="$STAGE/HLS Livecam.app"
CONTENTS="$APP/Contents"
RES="$CONTENTS/Resources"
RUNTIME="$RES/app"
MACOS="$CONTENTS/MacOS"
VERSION="1.0.0-audio"
BUILD_STAMP="$(date -u '+%Y%m%dT%H%M%SZ')"
GIT_COMMIT="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
GIT_DIRTY="$(git -C "$ROOT" status --porcelain 2>/dev/null | wc -l | tr -d ' ')"

# The filename carries the build stamp so a stale DMG is obvious at a glance
# rather than after a wasted trip to the console.
OUT="$DIST/HLS-Livecam-${VERSION}-${BUILD_STAMP}.dmg"

# Install-time policy, shared with install-macos.py. Empty identity = ad-hoc,
# which is this fleet's permanent state by decision.
CODESIGN_IDENTITY=""
if [[ -f "$ROOT/scripts/install.env" ]]; then
  CODESIGN_IDENTITY="$(grep -E '^CODESIGN_IDENTITY=' "$ROOT/scripts/install.env" \
    | cut -d= -f2- | sed 's/#.*//' | xargs || true)"
fi
[[ -n "$CODESIGN_IDENTITY" ]] || CODESIGN_IDENTITY="-"

trap 'rm -rf "$STAGE"' EXIT

for p in \
  "$ROOT/bin/livecam" \
  "$ROOT/bin/camdash-gui" \
  "$ROOT/gui" \
  "$ROOT/web" \
  "$ROOT/vendor" \
  "$ROOT/.venv/bin/python"
do
  [[ -e "$p" ]] || { echo "ERROR: missing $p" >&2; exit 1; }
done

PY="$ROOT/.venv/bin/python"
FFMPEG="$(command -v ffmpeg || true)"
[[ -n "$FFMPEG" ]] || { echo "ERROR: ffmpeg not found" >&2; exit 1; }

echo "[1/6] Preflight"
"$PY" -c 'import PySide6; print("PySide6 OK")'
"$ROOT/vendor/mediamtx" --version
"$FFMPEG" -hide_banner -version | head -1

echo "[2/6] Bundle"
mkdir -p "$MACOS" "$RUNTIME"

cat > "$CONTENTS/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
 "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
<key>CFBundleName</key><string>HLS Livecam</string>
<key>CFBundleDisplayName</key><string>HLS Livecam</string>
<key>CFBundleIdentifier</key><string>com.livecam.hls-livecam</string>
<key>CFBundleExecutable</key><string>hls-livecam</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>CFBundleShortVersionString</key><string>1.0.0-audio</string>
<key>CFBundleVersion</key><string>1.0.0</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSCameraUsageDescription</key><string>HLS Livecam uses the camera to publish the live room video feed.</string>
<key>NSMicrophoneUsageDescription</key><string>HLS Livecam uses the microphone to publish sound from the room.</string>
<key>NSAppleEventsUsageDescription</key><string>HLS Livecam adds itself to your Login Items so the stream resumes after a restart.</string>
</dict>
</plist>
PLIST

echo "$VERSION" > "$RES/VERSION"

ditto "$ROOT/bin" "$RUNTIME/bin"
ditto "$ROOT/gui" "$RUNTIME/gui"
ditto "$ROOT/web" "$RUNTIME/web"
ditto "$ROOT/vendor" "$RUNTIME/vendor"

# Render the viewer from its template rather than shipping whatever
# web/index.html happens to be sitting in the build machine's working tree.
# Nothing in the installed bundle runs livecam-setup, so the page copied here
# is the page the operator gets, permanently. Rendering makes the shipped
# viewer a function of committed source instead of local disk state.
BUILD_HLS_PORT="$(grep -E '^HLS_PORT=' "$ROOT/config.env" 2>/dev/null | cut -d= -f2 | tr -d ' ')"
[[ -n "$BUILD_HLS_PORT" ]] || BUILD_HLS_PORT=8888
sed "s|@HLS_PORT@|${BUILD_HLS_PORT}|g" \
  "$ROOT/web/index.template.html" > "$RUNTIME/web/index.html"

# An unsubstituted placeholder would ship a viewer that cannot reach the
# stream, and it is invisible until someone opens the page.
if grep -q '@[A-Z_]*@' "$RUNTIME/web/index.html"; then
  echo "ERROR: unsubstituted placeholder in rendered index.html" >&2
  grep -n '@[A-Z_]*@' "$RUNTIME/web/index.html" >&2
  exit 1
fi

# The audio panel has been lost once already by regenerating the viewer from a
# template that never carried it. Fail the build rather than ship a silent one.
if ! grep -q 'roomAudioPlayer' "$RUNTIME/web/index.html"; then
  echo "ERROR: rendered viewer has no audio player" >&2
  exit 1
fi
# Dereference while copying the venv. It contains an absolute symlink into
# Homebrew (.venv/bin/python3.14 -> /usr/local/opt/...), and codesign refuses
# to seal a bundle containing a link pointing outside it: "invalid destination
# for symbolic link in bundle". ditto preserves symlinks, so this cannot use
# it. install-macos.py has always done this; this script had not, and shipped
# unverifiable bundles because it signed without ever verifying.
python3 -c 'import shutil,sys; shutil.copytree(sys.argv[1], sys.argv[2], symlinks=False)' \
  "$ROOT/.venv" "$RUNTIME/.venv"

# Guard against shipping a bundle that silently lost today's source. These two
# are new and are the whole point of this build; a copy that failed to pick
# them up would produce a DMG that looks fine and behaves like last week's.
# Purge bytecode copied in from the source tree. Beyond breaking the signature
# later, a .pyc embeds the absolute path of the file it was compiled from, so
# stale caches drag the build machine's checkout path into the shipped bundle.
find "$RUNTIME" -name '__pycache__' -type d -prune -exec rm -rf {} + 2>/dev/null || true

# Install-time policy travels inside the bundle: first-run setup reads it from
# /Applications on a machine that has no checkout.
cp "$ROOT/scripts/install.env" "$RES/install.env"

for required in bin/livecam_platform.py bin/livecam-supervisor bin/livecam \
                bin/broadcast-api bin/livecam-firstrun; do
  [[ -f "$RUNTIME/$required" ]] || {
    echo "ERROR: $required missing from bundle" >&2; exit 1; }
done
[[ -f "$RES/install.env" ]] || { echo "ERROR: install.env missing" >&2; exit 1; }

# The whole point of this build is that setup no longer needs the repo. Fail
# loudly if a dev-machine path survived into anything that ships.
if grep -rn "Projects/mac-hls-livecam" "$RUNTIME/bin" "$MACOS" 2>/dev/null; then
  echo "ERROR: hardcoded dev path found in shipped files (above)" >&2
  exit 1
fi

if [[ -f "$ROOT/camdash-gui.app/Contents/Resources/AppIcon.icns" ]]; then
  cp "$ROOT/camdash-gui.app/Contents/Resources/AppIcon.icns" "$RES/AppIcon.icns"
fi

echo "[3/6] Relocatable livecam state"
python3 - "$RUNTIME/bin/livecam" <<'PYCODE'
from pathlib import Path
import sys

p = Path(sys.argv[1])
s = p.read_text()

s = s.replace(
    'STATE_DIR="$ROOT/state"\n'
    'LOG_DIR="$STATE_DIR/logs"\n'
    'VENDOR="$ROOT/vendor"\n'
    'WEB_DIR="$ROOT/web"\n'
    'CONFIG="$ROOT/config.env"\n',
    'APP_SUPPORT="$HOME/Library/Application Support/HLS Livecam"\n'
    'STATE_DIR="$APP_SUPPORT/state"\n'
    'LOG_DIR="$STATE_DIR/logs"\n'
    'VENDOR="$ROOT/vendor"\n'
    'WEB_DIR="$ROOT/web"\n'
    'CONFIG="$APP_SUPPORT/config.env"\n',
    1
)

s = s.replace(
    'mkdir -p "$STATE_DIR" "$LOG_DIR"',
    'mkdir -p "$APP_SUPPORT" "$STATE_DIR" "$LOG_DIR"',
    1
)

p.write_text(s)
PYCODE

echo "[4/6] Launcher"
cat > "$MACOS/hls-livecam" <<'LAUNCHER'
#!/bin/bash
set -uo pipefail

HERE="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$HERE/Resources/app"
PY="$ROOT/.venv/bin/python"
LOGS="$HOME/Library/Application Support/HLS Livecam/state/logs"

# Homebrew's bin is not on a Finder-launched process's PATH, and ffmpeg lives
# there. Without this the first-run dependency check would report ffmpeg
# missing on a Mac that has it.
export PATH="$ROOT/bin:/usr/local/bin:/opt/homebrew/bin:$PATH"
export PYTHONPATH="$ROOT${PYTHONPATH:+:$PYTHONPATH}"

mkdir -p "$LOGS"

# First-run setup: permissions, Login Item, autologin, supervisor. Marker-
# guarded, so this is a cheap no-op on every launch after the first. Never let
# it block the GUI from opening -- the GUI is also where you go to diagnose.
"$PY" "$ROOT/bin/livecam-firstrun" >>"$LOGS/firstrun.log" 2>&1 || true

cd "$ROOT"
exec "$PY" -m gui.app "$@"
LAUNCHER

chmod +x "$MACOS/hls-livecam"

echo "[5/6] DMG layout"
ln -s /Applications "$STAGE/Applications"

cat > "$STAGE/README.txt" <<README
HLS Livecam ${VERSION}
Build ${BUILD_STAMP}  commit ${GIT_COMMIT}$([[ "$GIT_DIRTY" != "0" ]] && echo " (+${GIT_DIRTY} uncommitted)")

SETUP
  1. Drag HLS Livecam.app to Applications.
  2. Right-click it in Applications and choose Open. (Required once: this
     build is ad-hoc signed, so a normal double-click is blocked.)
  3. Click through the prompts. You will be asked for camera and microphone
     access, for Automation, and -- if you enable automatic login -- for your
     password and admin rights.

That is the whole install. No separate installer, no Terminal, no checkout.
Setup runs from inside the app on first launch and does not repeat.

REQUIREMENT
FFmpeg must be installed:  brew install ffmpeg
The app will tell you if it is missing.
README

# Record what this actually is, inside the bundle, so a machine under test can
# be identified without trusting the filename.
cat > "$RES/BUILDINFO" <<INFO
version=${VERSION}
build=${BUILD_STAMP}
commit=${GIT_COMMIT}
uncommitted_files=${GIT_DIRTY}
built_by=$(whoami)@$(hostname -s)
INFO

# Purge bytecode before signing. Any __pycache__ generated after the signature
# invalidates the seal the moment it is written, and a broken seal means the
# cdhash no longer matches -- which voids every TCC grant pinned to it. The
# installer had exactly this bug: it ran its Python import test after signing.
find "$RUNTIME" -name '__pycache__' -type d -prune -exec rm -rf {} + 2>/dev/null || true

if [[ "$CODESIGN_IDENTITY" == "-" ]]; then
  echo "      ad-hoc signing (no Developer ID configured)"
else
  echo "      signing with: $CODESIGN_IDENTITY"
fi
codesign --force --deep --sign "$CODESIGN_IDENTITY" "$APP" >/dev/null
codesign --verify --deep --strict "$APP" || {
  echo "ERROR: signature failed to verify" >&2; exit 1; }
echo "      signature verified"

echo "[6/6] DMG"
mkdir -p "$DIST"
rm -f "$OUT"

hdiutil create \
  -volname "HLS Livecam 1.0.0-audio" \
  -srcfolder "$STAGE" \
  -ov \
  -format UDZO \
  "$OUT" >/dev/null

echo
echo "BUILD COMPLETE"
echo "DMG: $OUT"
echo "SIZE: $(du -h "$OUT" | awk '{print $1}')"
