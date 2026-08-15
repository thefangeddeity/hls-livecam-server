"""
Probe layer — imports the shipped camdash and reuses its functions verbatim.

camdash is loaded as a module via SourceFileLoader (it has no .py extension).
Nothing here modifies it: run-2 brief §2 puts camdash explicitly out of scope,
and it must keep working headless over SSH.

What is reused unchanged: sample_metrics, system_status, read_smart/smart_worker,
hls_worker, read_broadcast/write_broadcast, read_cams/write_cams, services_enabled,
_livecam, and the /api POST helpers' target constants.

Threading contract (run-2 brief §3, non-negotiable): sample_metrics() measures at
~1.2s per call because it walks every process twice. It must never be called from
the render thread. MetricsWorker owns it and hands finished snapshots across by
signal.
"""
import importlib.machinery
import importlib.util
import json
import os
import re
import subprocess
import threading
import time
import urllib.request

from PySide6.QtCore import QObject, QThread, Signal

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
_INSTALLED_CAMDASH = "/usr/local/bin/camdash"
_STAGED_CAMDASH = os.path.join(ROOT, "pkg", "usr", "local", "bin", "camdash")


def _load_from(path):
    loader = importlib.machinery.SourceFileLoader("camdash", path)
    spec = importlib.util.spec_from_loader("camdash", loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


def _load_camdash():
    """Installed camdash is ground truth once it actually carries the GUI
    support this port needs (API_BASE etc.) -- but installing it requires a
    root-owned file write this process cannot make (no typed passwords,
    ever). Until that install happens, fall back to the repo's staged copy
    rather than failing outright, so the port can be run and tested from a
    checkout in the meantime."""
    if os.path.exists(_INSTALLED_CAMDASH):
        mod = _load_from(_INSTALLED_CAMDASH)
        if hasattr(mod, "API_BASE"):
            return mod
    return _load_from(_STAGED_CAMDASH)


cd = _load_camdash()
CAMDASH_PATH = cd.__file__

API_BASE = cd.API_BASE
RTSP_URL = f"rtsp://127.0.0.1:{cd.RTSP_PORT}/cam"
HLS_URL = f"http://127.0.0.1:{cd.HLS_PORT}/cam/index.m3u8"
MAX_MSG = 120

_workers_started = False


def start_background_workers():
    """Start camdash's own daemon workers so hls/smart state populates.

    These are the same threads camdash's main() starts; the snapshot worker is
    NOT started (it renders terminal rasters, which the GUI has no use for).
    """
    global _workers_started
    if _workers_started:
        return
    threading.Thread(target=cd.hls_worker, daemon=True).start()
    threading.Thread(target=cd.smart_worker, daemon=True).start()
    _workers_started = True


# ── control-plane POSTs (off the render thread; callers use run_async) ──
def _post(endpoint, data=b""):
    req = urllib.request.Request(f"{API_BASE}/api/{endpoint}", data=data, method="POST")
    with urllib.request.urlopen(req, timeout=2) as r:
        return r.read().decode().strip()


def set_feed_mode(mode):
    return _post("feed-mode", mode.encode())


def toggle_msg_lock():
    return _post("msg-lock")


def buzz():
    return _post("buzz")


def smart_lines():
    with cd.smart_lock:
        return list(cd.smart_info)


# ── NODE panel data (run-3 §4a) ──────────────────────────────────
_ip_cache = ["", 0.0]


def local_ip():
    """LAN address. Cached for 60s — this shells out, and it rarely changes."""
    now = time.time()
    if not _ip_cache[0] or now - _ip_cache[1] > 60:
        found = ""
        try:
            out = subprocess.run(
                ["ip", "-4", "route", "get", "1.1.1.1"],
                capture_output=True, text=True, timeout=2,
            ).stdout
            m = re.search(r"\bsrc (\S+)", out)
            if m:
                found = m.group(1)
        except Exception:
            pass
        _ip_cache[0] = found or "—"
        _ip_cache[1] = now
    return _ip_cache[0]


_disk_cache = ["", 0.0]


def disk_model():
    """Human disk name, e.g. 'TOSHIBA MQ04UBF100'.

    Windows shows the drive model; `/dev/sda` is the same fact in a worse
    format. Cached for 5 min — the disk does not change.
    """
    now = time.time()
    if not _disk_cache[0] or now - _disk_cache[1] > 300:
        name = ""
        try:
            dev = cd.get_main_disk()
            name = subprocess.run(["lsblk", "-no", "MODEL", dev],
                                  capture_output=True, text=True, timeout=4).stdout.strip()
        except Exception:
            pass
        _disk_cache[0] = name or "—"
        _disk_cache[1] = now
    return _disk_cache[0]


def node_info():
    """hostname + tailscale, straight from the control-plane's own /api/info."""
    try:
        with urllib.request.urlopen(f"{API_BASE}/api/info", timeout=1) as r:
            return json.loads(r.read().decode())
    except Exception:
        return {"hostname": cd.socket.gethostname(), "tailscale": ""}


def read_feed_state(default=False):
    """Shared with camdash via state/camdash-feed.state.

    camdash treats a missing file as ON; the GUI defaults it OFF per the PM's
    run-7 decision, and writes the file so both surfaces agree from then on.
    """
    try:
        with open(cd.FEED_STATE_FILE) as f:
            return f.read().strip() == "on"
    except FileNotFoundError:
        write_feed_state(default)
        return default
    except Exception:
        return default


def write_feed_state(val):
    try:
        with open(cd.FEED_STATE_FILE, "w") as f:
            f.write("on" if val else "off")
    except Exception:
        pass


def run_async(fn, *args, done=None):
    """Fire-and-forget on a plain daemon thread, for control actions.

    Used for anything that shells out to `livecam` (repair takes up to 90s) or
    POSTs to the control-plane. Never blocks the GUI thread.
    """
    def _run():
        try:
            result = fn(*args)
        except Exception as exc:
            result = exc
        if done is not None:
            done(result)

    threading.Thread(target=_run, daemon=True).start()


class FastWorker(QThread):
    """Cheap metrics only — pure psutil counter reads, microseconds each.

    Deliberately does NOT call camdash's sample_metrics(). That function is
    correct but calls proc() three times, and each proc() walks every process on
    the box; with the GUI's own top-8 scan that is four full process walks per
    tick, which measured at ~1.2s and ~22% CPU when driven at GUI cadence. The
    expensive half lives in SlowWorker instead. Both reuse camdash's own
    functions — the split is about cadence, not about duplicating logic.
    """

    snapshot = Signal(dict)

    def __init__(self, interval_ms=500, parent=None):
        super().__init__(parent)
        self._interval = interval_ms
        self._running = True

    def run(self):
        import psutil

        while self._running:
            try:
                self.snapshot.emit({
                    "c": cd.cpu(),
                    "m": cd.mem(),
                    "l": cd.load(),
                    "up": cd.uptime(),
                    "cores": psutil.cpu_count(logical=True),
                    "swap": psutil.swap_memory(),
                    "stype": cd.swap_type(),
                    "avail": psutil.virtual_memory().available // (1024 * 1024),
                    "disk": cd.disk_write_mbs(),
                })
            except Exception:
                pass
            self.msleep(self._interval)

    def stop(self):
        self._running = False
        self.wait(3000)


class SlowWorker(QThread):
    """Everything that walks processes, opens sockets, spawns ffmpeg, or does HTTP.

    Wakeable: a control action calls refresh_now() so a mode change shows up
    immediately instead of waiting out the interval.
    """

    snapshot = Signal(dict)

    def __init__(self, interval_ms=2500, parent=None):
        super().__init__(parent)
        self._interval = interval_ms / 1000.0
        self._running = True
        self._wake = threading.Event()

    def refresh_now(self):
        self._wake.set()

    def run(self):
        start_background_workers()
        while self._running:
            try:
                self.snapshot.emit({
                    "ff": cd.proc("ffmpeg"),
                    "mm": cd.proc("mediamtx"),
                    "nginx": cd.proc("broadcast-api"),
                    "svc": cd.services_running(),
                    "hls": cd.get_hls(),
                    "rtsp": cd.port_open("127.0.0.1", int(cd.RTSP_PORT)),
                    "api": cd.port_open("127.0.0.1", int(cd.API_PORT)),
                    "v4l2": cd.camera_present_cached(),
                    "v4l2_dev": cd.read_device_env().get("VIDEO_DEVICE", "?"),
                    "dark": os.path.exists(cd.DARK_FLAG),
                    "feed_mode": cd._api_get("feed-mode", "show"),
                    "msg_lock": cd._api_get("msg-lock", "false"),
                    "cputemp": cd._cpu_temp(),
                    "smart": smart_lines(),
                    "broadcast": cd.read_broadcast(),
                    "enabled": cd.services_enabled(),
                    # ProcessPanel's row count is now derived from the panel's
                    # actual height rather than fixed at 24, so this over-fetches
                    # a comfortable ceiling rather than the exact display count --
                    # the process_iter() walk this does is the expensive part
                    # regardless of how many rows get sliced off the result.
                    "procs": _top_processes(60),
                    "node": node_info(),
                    "lan_ip": local_ip(),
                    "disk_model": disk_model(),
                })
            except Exception:
                pass
            self._wake.wait(self._interval)
            self._wake.clear()

    def stop(self):
        self._running = False
        self._wake.set()
        self.wait(4000)


def _top_processes(n):
    import psutil

    rows = []
    for p in psutil.process_iter(["name", "cpu_percent"]):
        try:
            cpu = p.info["cpu_percent"] or 0.0
            # Idle processes are dropped rather than listed at 0.0%. The panel
            # sizes its row count to the available height, so anything not
            # filtered here pads the list out with dozens of sleeping kernel
            # threads -- the fixed 24-row version simply never reached them.
            if cpu <= 0.0:
                continue
            rows.append((p.info["name"] or "", cpu))
        except Exception:
            pass
    rows.sort(key=lambda r: r[1], reverse=True)
    return rows[:n]
