"""
RTSP preview pipeline.

Transport is RTSP (`rtsp://127.0.0.1:8554/cam`, ~1s), ratified in run-2 brief §0.
The family-facing HLS path is ~4-7s behind; the FEED panel shows that delta
explicitly so the operator never mistakes the preview for what viewers see.

Decode runs through VideoToolbox (`-hwaccel videotoolbox`). Hardware *encode* is
dead on this box (-12908, OCLP casualty), which is why the capture pipeline uses
libx264; decode is unaffected and is what this path needs.

Stale-frame rule (run-2 brief §2/§3, and the Windows lesson): when the pipeline
dies, the last decoded frame is CLEARED, not held. `signal_lost` fires and the
widget paints the NO-SIGNAL placeholder. A frame is only ever emitted when it is
freshly decoded.
"""
import subprocess
import time

from PySide6.QtCore import QThread, Signal
from PySide6.QtGui import QImage

# Preview is deliberately smaller than the 1280x720 source: the panel is ~a third
# of the window, and decoding to panel size rather than full res keeps this well
# inside the headroom measured on this machine.
PREVIEW_W = 640
PREVIEW_H = 360
PREVIEW_FPS = 15

FRAME_BYTES = PREVIEW_W * PREVIEW_H * 3
STALL_SECONDS = 4.0     # no fresh frame for this long -> NO SIGNAL
RESTART_DELAY = 2.0


class VideoWorker(QThread):
    """Decodes RTSP to RGB frames on its own thread and emits QImages."""

    frame = Signal(QImage)
    signal_lost = Signal()

    def __init__(self, rtsp_url, parent=None):
        super().__init__(parent)
        self.rtsp_url = rtsp_url
        self._running = True
        self._proc = None
        self._live = False

    def _spawn(self):
        return subprocess.Popen(
            [
                "ffmpeg", "-hide_banner", "-loglevel", "error",
                "-hwaccel", "videotoolbox",
                "-rtsp_transport", "tcp",
                "-i", self.rtsp_url,
                "-an",
                "-vf", f"scale={PREVIEW_W}:{PREVIEW_H}",
                "-r", str(PREVIEW_FPS),
                "-f", "rawvideo", "-pix_fmt", "rgb24",
                "pipe:1",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            # No visible window or terminal artifact (run-2 brief §3). On macOS a
            # pipe-attached child is already windowless; start_new_session also
            # detaches it from the GUI's controlling terminal so a stray SIGINT
            # can't reach it.
            start_new_session=True,
        )

    def _mark_lost(self):
        if self._live:
            self._live = False
            self.signal_lost.emit()

    def run(self):
        buf = bytearray(FRAME_BYTES)
        last_frame_at = time.monotonic()

        while self._running:
            if self._proc is None or self._proc.poll() is not None:
                self._mark_lost()
                try:
                    self._proc = self._spawn()
                except Exception:
                    self._proc = None
                    self.msleep(int(RESTART_DELAY * 1000))
                    continue
                last_frame_at = time.monotonic()

            try:
                view = memoryview(buf)
                got = 0
                while got < FRAME_BYTES and self._running:
                    n = self._proc.stdout.readinto(view[got:])
                    if not n:
                        break
                    got += n

                if got == FRAME_BYTES:
                    # Copy: QImage must not alias the buffer we reuse next loop.
                    img = QImage(
                        bytes(buf), PREVIEW_W, PREVIEW_H,
                        PREVIEW_W * 3, QImage.Format_RGB888,
                    )
                    self._live = True
                    last_frame_at = time.monotonic()
                    self.frame.emit(img)
                else:
                    # Short read = producer died mid-frame. Never emit a partial
                    # or stale frame; drop it and let the restart path run.
                    self._kill_proc()
                    self._mark_lost()
                    self.msleep(int(RESTART_DELAY * 1000))
                    continue
            except Exception:
                self._kill_proc()
                self._mark_lost()
                self.msleep(int(RESTART_DELAY * 1000))
                continue

            if time.monotonic() - last_frame_at > STALL_SECONDS:
                self._mark_lost()

        self._kill_proc()

    def _kill_proc(self):
        if self._proc is not None:
            try:
                self._proc.kill()
                self._proc.wait(timeout=2)
            except Exception:
                pass
            self._proc = None

    def stop(self):
        self._running = False
        self._kill_proc()
        self.wait(3000)
