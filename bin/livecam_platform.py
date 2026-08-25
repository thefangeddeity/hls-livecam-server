"""Fleet-portable paths, state, and platform shims.

The camera stack runs on macOS, Windows 11, and Linux. Everything that differs
between those three lives here, so the supervisor and the components can be
written once. Callers should import from this module rather than branching on
sys.platform themselves -- when a fourth platform shows up, or when macOS moves
its Application Support conventions again, this is the only file that changes.

Two things in here are load-bearing for the fleet:

  * `state_dir()` -- mutable state never lives inside the installed application
    bundle/directory. On macOS that directory is code-signed, and writing into
    it breaks the seal, which silently voids every TCC grant the app holds.

  * the enabled flag -- "should this box be streaming?" is persisted as a file,
    not as launchd/systemd/Task Scheduler registration state. Registration only
    decides whether the *supervisor* starts at login; the flag decides whether
    the supervisor runs the camera. Keeping them separate is what makes on/off
    survive reboots, reinstalls, and OS-specific service plumbing identically
    on all three platforms.
"""
import os
import sys
import subprocess

APP_NAME = "HLS Livecam"
SLUG     = "hls-livecam"

BIN_DIR = os.path.dirname(os.path.abspath(__file__))
ROOT    = os.path.dirname(BIN_DIR)

IS_MACOS   = sys.platform == "darwin"
IS_WINDOWS = os.name == "nt"
IS_LINUX   = sys.platform.startswith("linux")


# ── Locations ────────────────────────────────────────────────────

def _base_dir():
    """Per-user, writable, survives reboots, outside any signed bundle."""
    if IS_MACOS:
        return os.path.expanduser(f"~/Library/Application Support/{APP_NAME}")
    if IS_WINDOWS:
        root = os.environ.get("LOCALAPPDATA") or os.path.expanduser("~")
        return os.path.join(root, APP_NAME)
    xdg = os.environ.get("XDG_STATE_HOME") or os.path.expanduser("~/.local/state")
    return os.path.join(xdg, SLUG)


def state_dir():
    # The bash CLI computes its own state path (repo-relative in a checkout,
    # Application Support once installed) and exports it. Honour that rather
    # than recomputing, so a dev checkout and the components it launches never
    # disagree about where the pidfiles and the enabled flag live.
    return os.environ.get("LIVECAM_STATE_DIR") or os.path.join(_base_dir(), "state")


def log_dir():
    return os.path.join(state_dir(), "logs")


def config_path():
    return os.environ.get("LIVECAM_CONFIG") or os.path.join(_base_dir(), "config.env")


def ensure_dirs():
    for d in (state_dir(), log_dir()):
        os.makedirs(d, exist_ok=True)


def runtime_bin(name):
    """Path to a sibling component, with the platform's executable suffix."""
    if IS_WINDOWS and not name.endswith(".exe"):
        exe = os.path.join(BIN_DIR, name + ".exe")
        if os.path.exists(exe):
            return exe
    return os.path.join(BIN_DIR, name)


def mediamtx_bin():
    name = "mediamtx.exe" if IS_WINDOWS else "mediamtx"
    return os.path.join(ROOT, "vendor", name)


# ── Config ───────────────────────────────────────────────────────

def read_config():
    """Parse the KEY=VALUE config. Tolerates inline '# comment' on every key
    except AVF_DEVICE_NAME, whose values legitimately contain '#'-free but
    space-bearing device names and must be taken to end-of-line."""
    env = {}
    try:
        with open(config_path()) as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue
                k, v = line.split("=", 1)
                k = k.strip()
                if "#" in v and k != "AVF_DEVICE_NAME":
                    v = v.split("#", 1)[0]
                env[k] = v.strip()
    except Exception:
        pass
    return env


# ── The enabled flag ─────────────────────────────────────────────
#
# Absent means enabled. A fresh install that has run setup is expected to
# stream; requiring an explicit opt-in file would mean a box that silently
# does nothing after a reinstall, which is the harder failure to notice.

def enabled_path():
    return os.path.join(state_dir(), "enabled")


def is_enabled():
    try:
        with open(enabled_path()) as f:
            return f.read().strip() not in ("0", "false", "off", "no")
    except FileNotFoundError:
        return True
    except Exception:
        return True


def set_enabled(on):
    ensure_dirs()
    tmp = enabled_path() + ".tmp"
    with open(tmp, "w") as f:
        f.write("1\n" if on else "0\n")
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, enabled_path())


# ── Process liveness ─────────────────────────────────────────────

def pid_alive(pid):
    if not pid or pid <= 0:
        return False
    try:
        import psutil
        return psutil.pid_exists(pid)
    except ImportError:
        pass
    if IS_WINDOWS:
        try:
            out = subprocess.run(["tasklist", "/FI", f"PID eq {pid}", "/NH"],
                                 capture_output=True, text=True, timeout=10).stdout
            return str(pid) in out
        except Exception:
            return False
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    except Exception:
        return False


def read_pidfile(path):
    try:
        with open(path) as f:
            return int(f.read().strip())
    except Exception:
        return None


def write_pidfile(path, pid):
    with open(path, "w") as f:
        f.write(str(pid) + "\n")


def terminate(pid, grace=1.0):
    """Signal a pid to stop, escalating to a hard kill.

    A capture process wedged in a blocking device-open syscall will not act on
    a polite signal, so the escalation is not optional -- see the watchdog in
    broadcast-api for the failure this exists to survive.
    """
    import signal
    import time
    if not pid_alive(pid):
        return
    try:
        if IS_WINDOWS:
            subprocess.run(["taskkill", "/PID", str(pid)],
                           capture_output=True, timeout=10)
        else:
            os.kill(pid, signal.SIGTERM)
    except Exception:
        pass
    deadline = time.monotonic() + grace
    while time.monotonic() < deadline:
        if not pid_alive(pid):
            return
        time.sleep(0.1)
    try:
        if IS_WINDOWS:
            subprocess.run(["taskkill", "/F", "/PID", str(pid)],
                           capture_output=True, timeout=10)
        else:
            os.kill(pid, signal.SIGKILL)
    except Exception:
        pass
