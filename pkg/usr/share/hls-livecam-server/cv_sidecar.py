#!/usr/bin/env python3
"""cv_sidecar.py -- detection telemetry as a standalone process.

On a Linux node the CV stack has no entry point of its own: broadcast-api
imports cv_processor as a library and drives it from its own writer loop,
inside the process that already owns the frames. 7elwe's server is Rust,
so that harness does not exist here -- this file is it.

Two modes, chosen by --publish:

TELEMETRY (default, Phase 1) -- READ-ONLY. Taps the RTSP loopback the
same way video_preview.rs does, runs the detector, prints state. Never
writes to the stream, never touches the capture pipeline; if this process
dies the node keeps publishing exactly as before. Runs CVProcessor NOT AT
ALL: that class is the picture-enhancement pipeline (optical flow, CLAHE,
denoise, sharpen) measured at 121.7 ms/frame against a 66.7 ms budget on
a node stronger than tina. Its cost buys a processed *picture*, which is
only worth paying for when someone is looking at one.

CV MODE (--publish) -- runs CVProcessor over each frame and publishes the
render to a SECOND mediamtx path (default rtsp://127.0.0.1:8554/cv) via
an ffmpeg subprocess on stdin. The raw camera keeps publishing to /cam
untouched, because this process reads its input from there: a CV mode
that replaced the capture would starve itself. The viewer switches which
path it plays; concealment remains Hide's job, not CV's.

In CV Mode the detector is NOT run here. CVProcessor does its own
detection internally (CV_DETECT_*), so running ours alongside would pay
for inference twice on a pipeline that is already the expensive one.
Telemetry in that mode comes from CVProcessor.state(), which is the same
source the burned-in banner uses.

Output: one compact JSON object per line on stdout, flushed. stderr is
for diagnostics only. The reader is windows/src/cv.rs.
"""

import argparse
import json
import os
import subprocess
import sys
import threading
import time

import cv2

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cv_detect as _cvd

# Only needed for CV Mode, and genuinely optional: a node missing the
# enhancement stack can still serve telemetry. Mirrors cv_processor's own
# habit of guarding every companion import.
try:
    import cv_processor as _cvp
except Exception as _e:                                    # pragma: no cover
    _cvp = None
    _CVP_ERR = str(_e)


# --------------------------------------------------------------- frames

class FrameTap:
    """Newest-frame-wins tap on the RTSP loopback.

    A SINGLE SLOT, not a queue -- the same property broadcast-api's
    _cam_frame has, and for the same reason: when detection runs slower
    than the stream (it does, deliberately), a queue would grow without
    bound and serve steadily staler frames. Overwriting means falling
    behind costs frames, never latency or memory.
    """

    def __init__(self, url, reconnect_max=10.0):
        self.url = url
        self.reconnect_max = float(reconnect_max)
        self._frame = None
        self._at = 0.0
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self.connected = False
        self.reconnects = 0

    def start(self):
        threading.Thread(target=self._loop, daemon=True).start()
        return self

    def stop(self):
        self._stop.set()

    def latest(self):
        """The newest frame and its age, or (None, None)."""
        with self._lock:
            if self._frame is None:
                return None, None
            return self._frame, time.time() - self._at

    def _loop(self):
        backoff = 0.5
        while not self._stop.is_set():
            cap = cv2.VideoCapture(self.url, cv2.CAP_FFMPEG)
            # Ask FFmpeg for the shallowest buffer it will give us; the
            # single slot above is the real defence, this just avoids
            # handing us a pre-staled frame on connect.
            try:
                cap.set(cv2.CAP_PROP_BUFFERSIZE, 1)
            except Exception:
                pass

            if not cap.isOpened():
                cap.release()
                self.connected = False
                _warn(f"cannot open {self.url}; retry in {backoff:.1f}s")
                if self._stop.wait(backoff):
                    return
                backoff = min(self.reconnect_max, backoff * 2)
                self.reconnects += 1
                continue

            self.connected = True
            backoff = 0.5
            _warn(f"connected to {self.url}")

            while not self._stop.is_set():
                ok, frame = cap.read()
                if not ok:
                    break                      # stream ended -- reconnect
                with self._lock:
                    self._frame = frame
                    self._at = time.time()

            cap.release()
            self.connected = False
            self.reconnects += 1
            if self._stop.wait(backoff):
                return
            backoff = min(self.reconnect_max, backoff * 2)


def _warn(msg):
    print(f"cv_sidecar: {msg}", file=sys.stderr, flush=True)


# ------------------------------------------------------------- publisher

class Publisher:
    """Pipes processed RGB frames into ffmpeg, which pushes RTSP.

    Started lazily on the first frame because the encoder has to be told
    the exact frame size up front, and that is only known once the
    processing chain has produced one. Encode parameters mirror
    pipeline.rs::dshow_capture so /cv and /cam behave alike downstream --
    same codec, preset, tune, profile, bitrate and GOP.

    Video only. Audio stays on /cam, where the capture publishes it; the
    viewer keeps pulling audio from there even while watching /cv.
    """

    def __init__(self, ffmpeg, url, fps):
        self.ffmpeg = ffmpeg
        self.url = url
        self.fps = max(1, int(round(fps)))
        self.proc = None
        self.size = None
        self.dropped = 0

    def _start(self, w, h):
        cmd = [
            self.ffmpeg, "-hide_banner", "-loglevel", "error",
            "-f", "rawvideo", "-pix_fmt", "rgb24",
            "-s", f"{w}x{h}", "-r", str(self.fps),
            "-i", "pipe:0",
            "-c:v", "libx264", "-preset", "ultrafast", "-tune", "zerolatency",
            "-pix_fmt", "yuv420p",
            "-profile:v", "high", "-level", "4.0",
            "-b:v", "1500k", "-g", str(self.fps * 4),
            "-rtsp_transport", "tcp", "-f", "rtsp", self.url,
        ]
        kw = {}
        if os.name == "nt":
            # Same windowless rule every child in this codebase follows;
            # the parent is a GUI-subsystem exe with no console to inherit.
            kw["creationflags"] = 0x08000000            # CREATE_NO_WINDOW
        self.proc = subprocess.Popen(
            cmd, stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kw)
        self.size = (w, h)
        _warn(f"publishing {w}x{h}@{self.fps} -> {self.url}")

    def write(self, rgb):
        h, w = rgb.shape[:2]
        if self.proc is None:
            self._start(w, h)
        elif self.size != (w, h):
            # The chain must not change frame size mid-stream; ffmpeg was
            # told one size and would desync silently rather than fail.
            self.stop()
            self._start(w, h)
        try:
            self.proc.stdin.write(rgb.tobytes())
            return True
        except (BrokenPipeError, OSError):
            # ffmpeg died (mediamtx restart, path conflict). Drop this
            # frame and rebuild on the next one rather than taking the
            # whole sidecar down with it.
            self.dropped += 1
            _warn("publish pipe broke; restarting encoder")
            self.stop()
            return False

    def stop(self):
        if self.proc is None:
            return
        try:
            if self.proc.stdin:
                self.proc.stdin.close()
        except Exception:
            pass
        try:
            self.proc.kill()
        except Exception:
            pass
        self.proc = None
        self.size = None


# ---------------------------------------------------------------- state

def _track_summary(tracks):
    out = []
    for tr in tracks:
        x, y, w, h = tr.box
        out.append({
            "id": tr.track_id,
            "cls": tr.cls,
            "conf": round(float(tr.confidence), 3),
            "box": [int(x), int(y), int(w), int(h)],
            "state": tr.state,
            "promoted": bool(tr.promoted),
            "evidence": round(float(tr.evidence), 3),
            "age": round(float(tr.last_seen - tr.first_seen), 2),
        })
    return out


def _capability_line(detect_ms, rate):
    """7elwe's equivalent of CVProcessor._capability_line().

    Names only the faculties actually running. Phase 1 has the detector
    and the tracker; MOG2, foveal gating, acuity adaptation and scene
    registration all live in CVProcessor and are not running, so they are
    not claimed here.
    """
    bits = ["CV", "DETECT"]
    if detect_ms:
        bits.append(f"{detect_ms:.0f}MS")
    bits.append(f"{rate:.1f}FPS")
    return " / ".join(bits)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--url", default=os.environ.get(
        "HLS_CV_URL", "rtsp://127.0.0.1:8554/cam"))
    ap.add_argument("--model", default=os.environ.get("HLS_CV_MODEL", ""))
    ap.add_argument("--rate", type=float,
                    default=float(os.environ.get("HLS_CV_RATE", "2.0")),
                    help="detections per second (default 2)")
    ap.add_argument("--conf", type=float,
                    default=float(os.environ.get("HLS_CV_CONF", "0.35")))
    ap.add_argument("--size", type=int,
                    default=int(os.environ.get("HLS_CV_SIZE", "640")))
    ap.add_argument("--threads", type=int,
                    default=int(os.environ.get("HLS_CV_THREADS", "4")),
                    help="cap OpenCV worker threads (0 = leave default)")
    ap.add_argument("--publish", action="store_true",
                    default=os.environ.get("HLS_CV_PUBLISH", "") not in ("", "0"),
                    help="CV Mode: render through CVProcessor and publish")
    ap.add_argument("--publish-url", default=os.environ.get(
        "HLS_CV_PUBLISH_URL", "rtsp://127.0.0.1:8554/cv"))
    ap.add_argument("--publish-rate", type=float,
                    default=float(os.environ.get("HLS_CV_PUBLISH_RATE", "6.0")),
                    help="CV Mode frame rate. Separate knob from --rate on "
                         "purpose: --rate is how often to ASK what is in the "
                         "room, this is how often to DRAW it.")
    ap.add_argument("--ffmpeg", default=os.environ.get("HLS_FFMPEG", "ffmpeg"))
    ap.add_argument("--width", type=int,
                    default=int(os.environ.get("HLS_CV_WIDTH", "960")),
                    help="process at this width (0 = native). Resolution is "
                         "the lever that actually works on cost: tanzania "
                         "measured 121.7ms at 1280 vs 29.6ms at 640.")
    args = ap.parse_args()

    # Bounded on purpose. cv2 parallelises DNN inference across every core
    # it can see, so an unbounded detector measured 159% of a core here --
    # on a box that is simultaneously encoding and publishing video. The
    # cap trades a little detect latency, which this can afford at 2 fps,
    # for not competing with the capture pipeline.
    if args.threads > 0:
        cv2.setNumThreads(args.threads)

    if not args.model or not os.path.isfile(args.model):
        _warn(f"model not found: {args.model!r} -- nothing to detect with")
        return 2

    if args.publish:
        if _cvp is None:
            _warn(f"CV Mode requested but cv_processor is unavailable: {_CVP_ERR}")
            return 3
        return _run_publish(args)

    # .load() is NOT optional: the constructor leaves net=None and
    # detect() then silently returns [] forever, which reads exactly like
    # an empty room. Cost an hour once; never again.
    det = _cvd.OnnxDetector(args.model, size=args.size, conf=args.conf).load()
    tracker = _cvd.Tracker()
    _warn(f"detector ready ({os.path.basename(args.model)}, size={args.size}, "
          f"conf={args.conf}, rate={args.rate}/s)")

    tap = FrameTap(args.url).start()

    period = 1.0 / max(0.1, args.rate)
    seq = 0
    last_ms = 0.0
    next_at = time.time()

    try:
        while True:
            now = time.time()
            if now < next_at:
                time.sleep(min(0.05, next_at - now))
                continue
            next_at = now + period

            frame, age = tap.latest()
            seq += 1

            if frame is None:
                _emit({
                    "seq": seq,
                    "ok": False,
                    "reason": "no frame",
                    "connected": tap.connected,
                    "reconnects": tap.reconnects,
                    "mog2": False, "gated": False,
                    "scene_registered": False, "scene_stale": False,
                    "text": "", "capability_text": "",
                })
                continue

            t0 = time.time()
            dets = det.detect(frame)
            last_ms = (time.time() - t0) * 1000.0
            tracker.update(dets)

            tracks = [t for t in tracker.tracks.values() if t.state != "departed"]
            h, w = frame.shape[:2]

            _emit({
                "seq": seq,
                "ok": True,
                "connected": tap.connected,
                "reconnects": tap.reconnects,
                "frame": [int(w), int(h)],
                "frame_age": round(float(age), 3),
                "detect_ms": round(last_ms, 1),
                "rate": args.rate,
                "detections": [
                    {"cls": d.cls, "conf": round(float(d.confidence), 3),
                     "box": [int(d.x), int(d.y), int(d.w), int(d.h)]}
                    for d in dets
                ],
                "tracks": _track_summary(tracks),
                # Same function the burned-in banner uses on a Linux node,
                # so this text can never drift from the picture's own.
                "text": _cvd.hud_banner_text(tracks),
                "capability_text": _capability_line(last_ms, args.rate),
                # Faculties that belong to CVProcessor, honestly reported
                # as not running rather than omitted (a missing key and a
                # false one read differently to the lamp code).
                "mog2": False,
                "gated": False,
                "scene_registered": False,
                "scene_stale": False,
            })
    except KeyboardInterrupt:
        pass
    finally:
        tap.stop()
    return 0


def _run_publish(args):
    """CV Mode: render every frame through CVProcessor and publish it.

    Telemetry here comes from CVProcessor.state() rather than our own
    detector -- CVProcessor detects internally, and paying for inference
    twice on the expensive pipeline would be the one cost this node
    genuinely cannot spare.
    """
    # CVProcessor reads its configuration from a device.env-shaped dict.
    # The model default inside it is a Linux packaging path that does not
    # exist here, so pointing it at the resolved model is not optional.
    denv = {'CV_DETECT_MODEL': args.model}
    for key, env in (('CV_DETECT_CONF', 'HLS_CV_CONF'),
                     ('CV_DETECT_HZ', 'HLS_CV_DETECT_HZ'),
                     ('CV_EDGE_ENABLED', 'HLS_CV_EDGE_ENABLED')):
        val = os.environ.get(env)
        if val:
            denv[key] = val

    proc = _cvp.CVProcessor(denv)
    tap = FrameTap(args.url).start()
    pub = Publisher(args.ffmpeg, args.publish_url, args.publish_rate)
    _warn(f"CV Mode: width={args.width or 'native'} rate={args.publish_rate}/s "
          f"model={os.path.basename(args.model)}")

    period = 1.0 / max(0.1, args.publish_rate)
    seq = 0
    next_at = time.time()
    try:
        while True:
            now = time.time()
            if now < next_at:
                time.sleep(min(0.02, next_at - now))
                continue
            # Absolute schedule, not now+period: when a frame overruns
            # (it will -- this pipeline is over budget by design) the next
            # one is due immediately rather than compounding the delay.
            next_at = max(now, next_at + period)

            frame, age = tap.latest()
            seq += 1
            if frame is None:
                _emit({"seq": seq, "ok": False, "reason": "no frame",
                       "connected": tap.connected, "reconnects": tap.reconnects,
                       "publishing": False,
                       "mog2": False, "gated": False,
                       "scene_registered": False, "scene_stale": False,
                       "text": "", "capability_text": ""})
                continue

            if args.width and frame.shape[1] > args.width:
                h = int(round(frame.shape[0] * args.width / frame.shape[1]))
                # Even dimensions: yuv420p cannot represent odd ones and
                # ffmpeg would refuse the stream outright.
                frame = cv2.resize(frame, (args.width, h - (h & 1)),
                                   interpolation=cv2.INTER_AREA)

            t0 = time.time()
            # CVProcessor's contract is RGB in, RGB out. VideoCapture hands
            # us BGR, so without this the published render would come out
            # with red and blue swapped -- and the detector inside would be
            # reading the wrong channels too.
            rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
            out, _meta = proc.process(rgb)
            process_ms = (time.time() - t0) * 1000.0

            ok = pub.write(out)

            st = proc.state()
            st.update({
                "seq": seq, "ok": True,
                "connected": tap.connected, "reconnects": tap.reconnects,
                "publishing": bool(ok),
                "dropped": pub.dropped,
                "frame": [int(out.shape[1]), int(out.shape[0])],
                "frame_age": round(float(age), 3),
                "detect_ms": round(process_ms, 1),
                "rate": args.publish_rate,
                "achieved_fps": round(1000.0 / process_ms, 2) if process_ms else 0.0,
            })
            _emit(st)
    except KeyboardInterrupt:
        pass
    finally:
        pub.stop()
        tap.stop()
    return 0


# stdout is a STRUCTURED channel here -- cv.rs parses it as one JSON
# object per line -- but cv_processor prints human diagnostics ("SHAKEY
# DETECTOR CONFIG", "CV ACUITY: ...") straight to it. Those are not JSON
# and would interleave with real state. So the real handle is captured
# once here and sys.stdout is redirected to stderr: anything that prints
# lands in diagnostics, and only _emit reaches the reader.
_REAL_STDOUT = sys.stdout
sys.stdout = sys.stderr


def _emit(obj):
    _REAL_STDOUT.write(json.dumps(obj, separators=(",", ":")) + "\n")
    _REAL_STDOUT.flush()


if __name__ == "__main__":
    sys.exit(main())
