from pathlib import Path
import shutil
import re
import sys
import subprocess
import tempfile
import os
import plistlib

ROOT = Path.home() / "Projects/mac-hls-livecam"
SOURCE_APP = ROOT / "camdash-gui.app"
APP = Path("/Applications/HLS Livecam.app")
CONTENTS = APP / "Contents"
RES = CONTENTS / "Resources"
RUNTIME = RES / "app"
MACOS = CONTENTS / "MacOS"
SUPPORT = Path.home() / "Library/Application Support/HLS Livecam"
LAUNCH = Path.home() / "Library/LaunchAgents/com.livecam.autostart.plist"
BIN = Path("/usr/local/bin")
DIST = ROOT / "dist"

VERSION = "1.0.0-audio"

def run(cmd, check=True):
    print("+", " ".join(map(str, cmd)))
    return subprocess.run(cmd, check=check, text=True)


def load_install_env():
    """Install-time policy (autologin mode, signing identity, notary profile)."""
    env = {}
    p = Path(__file__).resolve().parent / "install.env"
    try:
        for line in p.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                env[k.strip()] = v.split("#", 1)[0].strip()
    except FileNotFoundError:
        pass
    return env


IENV = load_install_env()


# ------------------------------------------------------------
# Automatic login.
# ------------------------------------------------------------

def autologin_current_user():
    try:
        out = subprocess.run(
            ["defaults", "read", "/Library/Preferences/com.apple.loginwindow",
             "autoLoginUser"],
            capture_output=True, text=True, timeout=15)
        return out.stdout.strip() if out.returncode == 0 else ""
    except Exception:
        return ""


def filevault_on():
    try:
        out = subprocess.run(["fdesetup", "status"], capture_output=True,
                             text=True, timeout=20).stdout
        return "FileVault is On" in out
    except Exception:
        return False


def encode_kcpassword(password):
    """Encode for /etc/kcpassword.

    macOS stores the autologin password XOR'd against a fixed key and padded to
    a multiple of the key length. This is obfuscation, not encryption -- the key
    ships with the OS. It is written 0600 root:wheel, and the practical security
    boundary is physical access to a box that boots straight to a desktop by
    design. Do not treat this file as a secret store.
    """
    key = bytes([0x7D, 0x89, 0x52, 0x23, 0xD2, 0xBC, 0xDD, 0xEA, 0xA3, 0xB9, 0x1F])
    data = password.encode("utf-8")
    # Pad to a whole number of key lengths; a password that is an exact
    # multiple gets a full extra block, which is what loginwindow expects.
    pad = len(key) - (len(data) % len(key))
    data += b"\x00" * pad
    return bytes(b ^ key[i % len(key)] for i, b in enumerate(data))


def configure_autologin(user):
    mode = IENV.get("AUTOLOGIN_MODE", "kcpassword").lower()
    print(f"[autologin] mode={mode}")

    if mode == "skip":
        print("  skipped by install.env")
        return

    if filevault_on():
        print("  !! FileVault is ON -- automatic login is impossible while it is"
              " enabled.\n     Disable FileVault or accept a manual unlock at"
              " every boot.")
        return

    if autologin_current_user() == user:
        print(f"  already enabled for '{user}'")
        return

    if mode == "guide":
        print("  Automatic login is OFF. Opening System Settings.")
        print("  Set: Users & Groups > Automatically log in as > "
              f"{user}")
        run(["open", "x-apple.systempreferences:com.apple.Users-Groups-Settings.extension"],
            check=False)
        input("  Press Return once you have set it... ")
        if autologin_current_user() == user:
            print("  verified: automatic login is now enabled")
        else:
            print("  !! still not enabled -- the box will stop at the login "
                  "window after a reboot and will not stream until someone "
                  "logs in.")
        return

    # mode == kcpassword
    import getpass
    print(f"  Writing /etc/kcpassword for '{user}' (needs sudo).")
    print("  The password is used to encode that file and is not stored "
          "anywhere by this installer.")
    pw = getpass.getpass(f"  Login password for {user}: ")
    if not pw:
        print("  empty password -- skipping autologin setup")
        return

    blob = encode_kcpassword(pw)
    del pw
    tmp = Path(tempfile.mkdtemp(prefix="kcp-")) / "kcpassword"
    tmp.write_bytes(blob)
    tmp.chmod(0o600)
    try:
        run(["sudo", "install", "-m", "0600", "-o", "root", "-g", "wheel",
             str(tmp), "/etc/kcpassword"])
        run(["sudo", "defaults", "write",
             "/Library/Preferences/com.apple.loginwindow", "autoLoginUser",
             user])
    finally:
        try:
            tmp.unlink()
            tmp.parent.rmdir()
        except Exception:
            pass

    if autologin_current_user() == user:
        print("  verified: automatic login enabled")
    else:
        print("  !! autoLoginUser did not take -- check the sudo output above")

print("=== HLS Livecam macOS installer ===")
print("Source:", ROOT)
print("Target:", APP)

# ------------------------------------------------------------
# 1. Sanity checks.
# ------------------------------------------------------------

for p in [
    ROOT / "bin/livecam",
    ROOT / "bin/camdash",
    ROOT / "bin/camdash-gui",
    ROOT / "gui",
    ROOT / "web",
    ROOT / "vendor/mediamtx",
    ROOT / ".venv/bin/python",
]:
    if not p.exists():
        raise SystemExit(f"ERROR: missing {p}")

run([str(ROOT / ".venv/bin/python"), "-c",
     "import PySide6; print('PySide6:', PySide6.__file__)"])
run([str(ROOT / "vendor/mediamtx"), "--version"])

# ------------------------------------------------------------
# 2. Stop an old installed app LaunchAgent only if present.
#    Do not stop the currently running camera services.
# ------------------------------------------------------------

if LAUNCH.exists():
    print("Removing previous installed LaunchAgent...")
    run(["launchctl", "bootout",
         f"gui/{os.getuid()}/com.livecam.autostart"], check=False)

# ------------------------------------------------------------
# 3. Rebuild installed app from scratch.
# ------------------------------------------------------------

print("[1/10] Rebuilding /Applications/HLS Livecam.app...")

if APP.exists():
    shutil.rmtree(APP)

for d in [MACOS, RUNTIME, RES]:
    d.mkdir(parents=True, exist_ok=True)

# Copy runtime.
for name in ["bin", "gui", "web", "vendor"]:
    src = ROOT / name
    dst = RUNTIME / name
    shutil.copytree(src, dst, symlinks=True)

# Python virtual environments commonly contain symlinks into Homebrew.
# Those links point outside the .app bundle and codesign rejects them.
src = ROOT / ".venv"
dst = RUNTIME / ".venv"
shutil.copytree(src, dst, symlinks=False)

# Render the viewer from its template rather than shipping whatever
# web/index.html happens to be in the working tree.
#
# The copytree above takes web/ wholesale, and index.html is a GENERATED file
# -- so an installer run would bake in whatever stale render was lying around
# and overwrite a newer viewer on the target. The DMG build already renders;
# this path did not, which meant the two installers could disagree about what
# the viewer is. Same substitution, same guards.
tpl = ROOT / "web" / "index.template.html"
if tpl.exists():
    hls_port = "8888"
    cfg = ROOT / "config.env"
    if cfg.exists():
        for line in cfg.read_text().splitlines():
            if line.startswith("HLS_PORT="):
                hls_port = line.split("=", 1)[1].split("#")[0].strip() or hls_port

    label = "unknown"
    try:
        out = subprocess.run(["git", "-C", str(ROOT), "describe", "--tags", "--always"],
                             capture_output=True, text=True, timeout=5)
        if out.returncode == 0:
            label = out.stdout.strip() or label
    except Exception:
        pass

    rendered = (tpl.read_text()
                .replace("@HLS_PORT@", hls_port)
                .replace("@VERSION@", label)
                .replace("@BUILD_LABEL@", label))

    # Both are invisible until someone opens the page, so fail here instead.
    leftover = re.findall(r"@[A-Z_]+@", rendered)
    if leftover:
        sys.exit(f"ERROR: unsubstituted placeholder(s) in index.html: {sorted(set(leftover))}")
    if "roomAudioPlayer" not in rendered:
        sys.exit("ERROR: rendered viewer has no audio player")

    (RUNTIME / "web" / "index.html").write_text(rendered)
    print(f"      viewer rendered from template ({label}, HLS port {hls_port})")

# Copy config template if present.
if (ROOT / "config.env.example").exists():
    shutil.copy2(ROOT / "config.env.example", RUNTIME / "config.env.example")

# Existing app icon.
icon = ROOT / "camdash-gui.app/Contents/Resources/AppIcon.icns"
if icon.exists():
    shutil.copy2(icon, RES / "AppIcon.icns")
    
# Bundle GUI/capture helpers.
helpers = SOURCE_APP / "Contents/Helpers"
if helpers.exists():
    runtime_helpers = RUNTIME / "Helpers"
    if runtime_helpers.exists():
        shutil.rmtree(runtime_helpers)
    shutil.copytree(helpers, runtime_helpers, symlinks=True)


# ------------------------------------------------------------
# 4. Make the installed livecam use Application Support state.
# ------------------------------------------------------------

print("[2/10] Relocating mutable state/config...")

livecam = RUNTIME / "bin/livecam"
s = livecam.read_text()

old = (
    'STATE_DIR="$ROOT/state"\n'
    'LOG_DIR="$STATE_DIR/logs"\n'
    'VENDOR="$ROOT/vendor"\n'
    'WEB_DIR="$ROOT/web"\n'
    'CONFIG="$ROOT/config.env"\n'
)

new = (
    'APP_SUPPORT="$HOME/Library/Application Support/HLS Livecam"\n'
    'STATE_DIR="$APP_SUPPORT/state"\n'
    'LOG_DIR="$STATE_DIR/logs"\n'
    'VENDOR="$ROOT/vendor"\n'
    'WEB_DIR="$ROOT/web"\n'
    'CONFIG="$APP_SUPPORT/config.env"\n'
)

if old not in s:
    raise SystemExit("ERROR: expected livecam state block not found")

s = s.replace(old, new, 1)

s = s.replace(
    'mkdir -p "$STATE_DIR" "$LOG_DIR"',
    'mkdir -p "$APP_SUPPORT" "$STATE_DIR" "$LOG_DIR"',
    1
)

livecam.write_text(s)
livecam.chmod(0o755)

# ------------------------------------------------------------
# 5. Preserve current live state so the installed control plane
#    recognizes the services that are already running.
# ------------------------------------------------------------

print("[3/10] Migrating current user state...")

SUPPORT.mkdir(parents=True, exist_ok=True)

src_state = ROOT / "state"
dst_state = SUPPORT / "state"

if dst_state.exists():
    shutil.rmtree(dst_state)

if src_state.exists():
    shutil.copytree(src_state, dst_state, symlinks=True)

# Never leave mutable runtime state inside the signed application bundle.
# The installed livecam launcher uses Application Support for all live state.
bundle_state = RUNTIME / "state"
if bundle_state.exists():
    shutil.rmtree(bundle_state)

config = ROOT / "config.env"
if config.exists():
    shutil.copy2(config, SUPPORT / "config.env")

# ------------------------------------------------------------
# 6. Create the actual application executable.
# ------------------------------------------------------------

print("[4/10] Creating application launcher...")

launcher = MACOS / "hls-livecam"

launcher.write_text("""#!/bin/bash
set -euo pipefail

APP_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$APP_ROOT/Resources/app"
PY="$ROOT/.venv/bin/python"

export PATH="$ROOT/bin:/usr/local/bin:/opt/homebrew/bin:$PATH"
export PYTHONPATH="$ROOT${PYTHONPATH:+:$PYTHONPATH}"

cd "$ROOT"
exec "$PY" -m gui.app "$@"
""")

launcher.chmod(0o755)

# ------------------------------------------------------------
# 7. Info.plist.
# ------------------------------------------------------------

info = {
    "CFBundleName": "HLS Livecam",
    "CFBundleDisplayName": "HLS Livecam",
    "CFBundleIdentifier": "com.livecam.hls-livecam",
    "CFBundleExecutable": "hls-livecam",
    "CFBundleIconFile": "AppIcon",
    "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": VERSION,
    "CFBundleVersion": "1.0.0",
    "LSMinimumSystemVersion": "13.0",
    "NSCameraUsageDescription": "HLS Livecam uses the camera to publish the live room video feed.",
    "NSMicrophoneUsageDescription": "HLS Livecam uses the microphone to publish sound from the room.",
    "NSHighResolutionCapable": True,
    "LSUIElement": False,
}

with open(CONTENTS / "Info.plist", "wb") as f:
    plistlib.dump(info, f)

(RES / "VERSION").write_text(VERSION + "\n")

# ------------------------------------------------------------
# 8. Install public commands + LaunchAgent.
# ------------------------------------------------------------

print("[5/10] Installing public command interface...")

BIN.mkdir(parents=True, exist_ok=True)

for name in ["camdash", "camdash-gui"]:
    target = RUNTIME / "bin" / name
    link = BIN / name

    if link.is_symlink() or link.exists():
        link.unlink()

    link.symlink_to(target)
    print(f"{link} -> {target}")

# Explicitly remove the accidental internal public wrapper.
internal_public = BIN / "_livecam"
if internal_public.exists() or internal_public.is_symlink():
    internal_public.unlink()
    print("Removed /usr/local/bin/_livecam")

# Do NOT install a public "livecam" command.

# Seed the persisted on/off flag. This is the fleet-common half of the startup
# story: registration decides whether the supervisor starts at login, the flag
# decides whether it runs the camera. Only write it if absent, so reinstalling
# never silently turns a deliberately-off box back on.
flag = SUPPORT / "state" / "enabled"
flag.parent.mkdir(parents=True, exist_ok=True)
if not flag.exists():
    flag.write_text("1\n")
    print("  seeded enabled flag -> on")
else:
    print(f"  preserved existing enabled flag -> {flag.read_text().strip()}")

print("[6/10] Configuring automatic login...")
configure_autologin(os.environ.get("USER") or Path.home().name)

print("[7/10] Installing unattended camera Login Item...")

STARTER = Path.home() / "Applications" / "HLS Livecam Startup.app"
STARTER_CONTENTS = STARTER / "Contents"
STARTER_MACOS = STARTER_CONTENTS / "MacOS"

STARTER_MACOS.mkdir(parents=True, exist_ok=True)

# The Login Item opens Terminal and runs the supervisor there. Terminal is the
# point: launchd agents start outside any GUI session, and a process with no
# GUI/TCC ancestry is refused camera access -- at which point ffmpeg's
# avfoundation input deadlocks instead of erroring, leaving a live-looking
# stack serving 404s. Running under Terminal inherits an approved ancestry.
starter_exe = STARTER_MACOS / "start"
starter_exe.write_text(f"""#!/bin/bash
exec /usr/bin/osascript <<'APPLESCRIPT'
tell application "Terminal"
    do script "exec {RUNTIME}/.venv/bin/python {RUNTIME}/bin/livecam-supervisor"
end tell
APPLESCRIPT
""")
starter_exe.chmod(0o755)

starter_info = {
    "CFBundleIdentifier": "com.livecam.hls-livecam.startup",
    "CFBundleName": "HLS Livecam Startup",
    "CFBundleDisplayName": "HLS Livecam Startup",
    "CFBundleExecutable": "start",
    "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": VERSION,
    "CFBundleVersion": "1.0.0",
    "LSUIElement": True,
}

with open(STARTER_CONTENTS / "Info.plist", "wb") as f:
    plistlib.dump(starter_info, f)

run(["codesign", "--force", "--deep", "--sign", "-", str(STARTER)])

# Remove the old LaunchAgent if an older installer left one behind.
run([
    "launchctl", "bootout",
    f"gui/{os.getuid()}/com.livecam.autostart"
], check=False)

LAUNCH.unlink(missing_ok=True)

# Replace an older managed Login Item and install ours. Deleting first matters:
# re-registering an existing name silently keeps the OLD path, so a starter app
# that has moved stays registered while pointing at nothing.
run(["osascript", "-e", """
tell application "System Events"
    if exists login item "HLS Livecam Startup" then
        delete login item "HLS Livecam Startup"
    end if
end tell
"""], check=False)

run(["osascript", "-e", f"""
tell application "System Events"
    make login item at end with properties {{name:"HLS Livecam Startup", path:"{STARTER}", hidden:false}}
end tell
"""], check=False)


def login_item_registered():
    out = subprocess.run(
        ["osascript", "-e",
         'tell application "System Events" to get name of every login item'],
        capture_output=True, text=True)
    return "HLS Livecam Startup" in out.stdout


# Verify rather than assume. Both calls above need Automation consent for
# System Events; if the operator dismisses that prompt they fail without any
# useful error, autostart never happens, and the box needs hands-on recovery
# after every reboot. That is precisely the failure this installer exists to
# prevent, so refuse to report success we have not confirmed.
if login_item_registered():
    print("  login item registered (verified)")
else:
    print()
    print("  !! LOGIN ITEM REGISTRATION FAILED")
    print("     Nothing will start at login until this is fixed.")
    print("     Most likely cause: Automation access for System Events was")
    print("     denied. Re-run the installer and allow it, or add it by hand:")
    print("       System Settings > General > Login Items > +")
    print(f"       {STARTER}")
    print()

# Do not launch the Login Item here.
# It is installed for the next GUI login. Launching it immediately
# would race the Login Item and create duplicate supervisors.

# ------------------------------------------------------------
# 9. Ad-hoc sign and verify.
# ------------------------------------------------------------

# Runtime test runs BEFORE signing, and with bytecode writing disabled.
# Importing the bundled GUI generates __pycache__ *inside* the bundle; doing
# that after signing invalidates the seal the moment it is created. That is not
# hypothetical -- a stale bin/__pycache__/broadcast-api.cpython-314.pyc was
# found breaking the signature of the installed app, alongside hand-edited
# runtime files. A broken seal means the cdhash no longer matches, which voids
# every TCC grant pinned to it.
print("[8/10] Testing installed runtime (pre-sign)...")

run([
    str(RUNTIME / ".venv/bin/python"),
    "-B",
    "-c",
    (
        "import sys, gui.app; "
        "print('Bundled Python:', sys.executable); "
        "print('Bundled GUI import: OK')"
    ),
])

# Purge any bytecode the copy or the test left behind, so the signature covers
# a tree that will not mutate itself on first run.
print("[9/10] Purging bytecode caches before signing...")
purged = 0
for cache in RUNTIME.rglob("__pycache__"):
    shutil.rmtree(cache, ignore_errors=True)
    purged += 1
print(f"  removed {purged} __pycache__ directories")

print("[10/10] Signing application...")

identity = IENV.get("CODESIGN_IDENTITY") or "-"
if identity == "-":
    print("  ad-hoc signing (no Developer ID configured)")
else:
    print(f"  signing with: {identity}")

run(["codesign", "--force", "--deep", "--sign", identity, str(APP)])
run(["codesign", "--verify", "--deep", "--strict", str(APP)])
print("  signature verified")

# Notarization is wired but inert without a real Developer ID. The service
# rejects ad-hoc binaries outright, so gate on the identity rather than only on
# the profile -- submitting an ad-hoc build would just fail confusingly.
notary_profile = IENV.get("NOTARIZE_PROFILE")
if identity != "-" and notary_profile:
    print(f"  notarizing with profile '{notary_profile}'...")
    zip_path = Path(tempfile.mkdtemp(prefix="notarize-")) / "app.zip"
    run(["ditto", "-c", "-k", "--keepParent", str(APP), str(zip_path)])
    run(["xcrun", "notarytool", "submit", str(zip_path),
         "--keychain-profile", notary_profile, "--wait"])
    run(["xcrun", "stapler", "staple", str(APP)])
    print("  notarized and stapled")
elif notary_profile:
    print("  NOTARIZE_PROFILE is set but CODESIGN_IDENTITY is not --")
    print("  notarization requires a Developer ID signature; skipping.")
else:
    print("  not notarized (ad-hoc build)")
    print("  First launch on a clean Mac will hit Gatekeeper: the operator")
    print("  must right-click the app and choose Open, once.")

# ------------------------------------------------------------
# Build a clean release DMG from the installed application.
# ------------------------------------------------------------

print("=== Building release DMG ===")

DIST.mkdir(parents=True, exist_ok=True)

stage = Path(tempfile.mkdtemp(prefix="hls-livecam-release-"))

try:
    stage_app = stage / "HLS Livecam.app"
    shutil.copytree(APP, stage_app, symlinks=True)
    (stage / "Applications").symlink_to("/Applications")

    dmg = DIST / f"HLS-Livecam-{VERSION}.dmg"
    if dmg.exists():
        dmg.unlink()

    run([
        "hdiutil", "create",
        "-volname", f"HLS Livecam {VERSION}",
        "-srcfolder", str(stage),
        "-ov",
        "-format", "UDZO",
        str(dmg),
    ])

finally:
    shutil.rmtree(stage, ignore_errors=True)

print()
print("==============================================")
print("INSTALL COMPLETE")
print("==============================================")
print("App:", APP)
print("Commands:")
print("  /usr/local/bin/camdash")
print("  /usr/local/bin/camdash-gui")
print("State:", SUPPORT)
print("Login Item:", STARTER)
print("DMG:", dmg)
print()
print("Minimum macOS: 13.0")
print("Public backend command: none")
print("Recovery DMG preserved.")
