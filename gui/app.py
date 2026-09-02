"""
camdash-gui — macOS operator GUI for hls-livecam-server.

A client, not a supervisor (run-2 §5, unchanged in run-3). `com.livecam.autostart`
and the `livecam` CLI own the pipeline's lifecycle; this window can be quit and
reopened while the stream keeps running. It ships alongside camdash, which keeps
working headless over SSH.

Layout follows run-3: three columns, the centre one owned by the feed. Side
columns are content-height and stack to the top, so the feed takes every pixel
they aren't using. Feed-mode controls sit directly under the video; supervision
actions live in the bottom toolbar. Those are different classes of action and the
layout says so.

Threading: the render thread does no probing, no process scans, no shell-outs.
FastWorker/SlowWorker own those; control actions run via probes.run_async.
"""
import os
import sys
import time

from PySide6.QtCore import QEvent, Qt, QTimer
from PySide6.QtGui import QFont, QGuiApplication, QIcon
from PySide6.QtWidgets import (
    QApplication, QCheckBox, QDialog, QFrame, QHBoxLayout, QLabel, QLineEdit,
    QMessageBox, QPlainTextEdit, QPushButton, QVBoxLayout, QWidget,
)

from . import probes, tokens as T
from .video import VideoWorker
from .widgets import (
    Chip, Divider, FeedView, Meter, Panel, StatRow, StatusPill, StatusRow,
    _font, tabular,
)

cd = probes.cd


def _toggle_style(btn, on):
    """Apply §5 toggle-on state via a QSS property; repolish to make it take.

    Skips when unchanged — a repolish is a full style recompute, and this runs on
    every snapshot.
    """
    want = "true" if on else "false"
    if btn.property("toggled") == want:
        return
    btn.setProperty("toggled", want)
    btn.style().unpolish(btn)
    btn.style().polish(btn)


def confirm(parent, title, text):
    box = QMessageBox(parent)
    box.setWindowTitle(title)
    box.setText(text)
    box.setIcon(QMessageBox.NoIcon)
    box.setStandardButtons(QMessageBox.Yes | QMessageBox.Cancel)
    box.setDefaultButton(QMessageBox.Cancel)
    box.setStyleSheet(T.QSS)
    return box.exec() == QMessageBox.Yes


# ── Left column ──────────────────────────────────────────────────
class SystemPanel(Panel):
    """Row labels are Title Case, matching the Windows node. Only section titles
    are uppercase (§3); shouting every label costs legibility for nothing."""

    def __init__(self):
        super().__init__("System")
        self.cpu = Meter("CPU")
        self.mem = Meter("Memory")
        self.swap = Meter("Swap")
        self.load = Meter("Load")
        for m in (self.cpu, self.mem, self.swap, self.load):
            self.add(m)
        self.ram = StatRow("RAM free")
        self.temp = StatRow("CPU temp")
        self.add(self.ram)
        self.add(self.temp)

    def update_from(self, snap):
        cores = snap.get("cores", 1) or 1
        self.cpu.set(snap.get("c", 0.0))
        self.mem.set(snap.get("m", 0.0))
        swap = snap.get("swap")
        spct = getattr(swap, "percent", 0.0)
        self.swap.set(spct, text=f"{spct:.1f}% [{snap.get('stype', '?').lower()}]")
        l = snap.get("l", 0.0)
        self.load.set(min(l / cores * 100, 100), text=f"{l:.2f}/{cores}")
        self.ram.set(f"{snap.get('avail', 0)} MB")
        t = snap.get("cputemp", "?")
        # No SMC access on macOS without extra tooling — shown as unavailable,
        # not faked.
        self.temp.set("—" if t == "?" else f"{t}°C",
                      T.TEXT_MUTED if t == "?" else T.TEXT)


class DiskPanel(Panel):
    def __init__(self):
        super().__init__("Disk / SMART")
        self.disk = StatRow("Disk")
        self.assess = StatusRow("Assess")
        self.risk = StatusRow("Risk")
        self.realloc = StatRow("Realloc")
        self.pending = StatRow("Pending")
        self.uncorr = StatRow("Uncorr")
        self.temp = StatRow("Temp")
        self.write = StatRow("Write")
        for w in (self.disk, self.assess, self.risk, self.realloc,
                  self.pending, self.uncorr, self.temp, self.write):
            self.add(w)

    def update_from(self, snap):
        si = {}
        for entry in snap.get("smart", []):
            if ":" in entry:
                k, _, v = entry.partition(":")
                si[k.strip()] = v.strip()

        # Model name, not the device node — `/dev/disk2` is the same fact in a
        # worse format, and Windows shows the drive.
        self.disk.set(snap.get("disk_model") or si.get("DISK", "?"))
        na = si.get("SMART", "") in ("N/A", "NO ACCESS")

        if na:
            # macOS internal SSDs don't expose SMART to smartctl; degrade to the
            # OFFLINE token rather than showing a false PASSED.
            self.assess.set(si.get("SMART", "N/A"), "N/A")
            self.risk.set("N/A", "N/A")
        else:
            assess = si.get("HEALTH", "?")
            self.assess.set(assess, "PASSED" if assess == "PASSED" else "ERROR")

            def _n(key):
                try:
                    return int(si.get(key, "x"))
                except ValueError:
                    return 0

            realloc, pending, uncorr = _n("REALLOC"), _n("PENDING"), _n("UNCORR")
            if realloc > 100 or pending > 0 or uncorr > 0:
                self.risk.set("HIGH", "HIGH")
            elif realloc > 0:
                self.risk.set("WARN", "WARN")
            else:
                self.risk.set("OK", "OK")

        self.realloc.set(si.get("REALLOC", "N/A"))
        self.pending.set(si.get("PENDING", "N/A"))
        self.uncorr.set(si.get("UNCORR", "N/A"))
        t = si.get("DISK TEMP", "?")
        self.temp.set(t if t not in ("?", "UNKNOWN") else "—")
        self.write.set(f"{snap.get('disk', 0.0):.2f} MB/s")


class ProcessPanel(Panel):
    def __init__(self):
        super().__init__("Processes")
        # run-8 §4: fill the column at default geometry (measured ~24 rows fit
        # in the 900px-tall default window without a scrollbar; Panel has none).
        self.rows = []
        for _ in range(24):
            r = StatRow("—")
            self.add(r)
            self.rows.append(r)
        self.add_stretch()

    def update_from(self, snap):
        procs = snap.get("procs", [])
        for i, row in enumerate(self.rows):
            if i < len(procs):
                name, cpu = procs[i]
                row.k.setText(name[:22])
                color = (T.CRITICAL if cpu > 30 else
                         T.WARN if cpu > 10 else
                         T.TEXT if cpu > 2 else T.TEXT_MUTED)
                row.set(f"{cpu:5.1f}%", color)
            else:
                row.k.setText("")
                row.set("")


# ── Centre column: the feed is the hero ──────────────────────────
class FeedPanel(Panel):
    """Video, then the feed-mode strip directly beneath it (run-3 §2).

    Mode buttons sit where the operator's eye already is. Buzz is fenced off
    behind a divider and right-aligned: it makes a noise in someone's living
    room and must not read as the sixth item in a button block.
    """

    def __init__(self, win):
        super().__init__("Feed")
        self.win = win

        self.view = FeedView()
        self.body.addWidget(self.view, 1)

        foot = QHBoxLayout()
        foot.setContentsMargins(0, 0, 0, 0)
        self.mode = QLabel("—")
        self.mode.setFont(_font(T.T_BADGE))
        self.mode.setStyleSheet(f"color: {T.TEXT_DIM}; font-weight: 600;")
        # Kept verbatim per run-3 §5 — macOS-only, and flagged for backport to
        # the Windows node.
        self.delay = Chip("PREVIEW ~1s  ·  VIEWERS ~4–7s")
        foot.addWidget(self.mode)
        foot.addStretch(1)
        foot.addWidget(self.delay)
        self.add_layout(foot)

        self.add(Divider())

        # Toolbar density: this row packs six buttons, a checkbox, and Buzz
        # into one line -- the panel's normal 6px/10px button padding only
        # fits that comfortably at the widest window sizes. Every control
        # below gets density="compact" (tighter QSS padding, see tokens.py)
        # rather than shrinking the window's other buttons.
        GUTTER = 10  # consistent gap between logical groups, was 4/6/10 mixed

        def _compact(w):
            w.setProperty("density", "compact")
            return w

        strip = QHBoxLayout()
        strip.setSpacing(4)
        # Feed toggle owns the decoder, not just the picture. Off by default.
        self.b_feed = _compact(QPushButton("Feed"))
        self.b_feed.setToolTip("Show the preview and start the decoder")
        self.b_feed.clicked.connect(lambda: self.win.toggle_feed())
        strip.addWidget(self.b_feed)

        # run-8 §3 put Pause here too; run-11 §1 corrects that -- Pause is a
        # server control (mediamtx/broadcast-api on/off), not a feed-mode
        # action, and now lives in the footer next to Stop server. Repair
        # stays: it genuinely reconverges the feed pipeline.
        strip.addWidget(Divider(vertical=True))
        strip.addSpacing(GUTTER)
        self.b_repair = _compact(QPushButton("Repair"))
        self.b_repair.clicked.connect(self._repair)
        strip.addWidget(self.b_repair)

        strip.addSpacing(GUTTER)
        strip.addWidget(Divider(vertical=True))
        strip.addSpacing(GUTTER)
        self.b_show = _compact(QPushButton("Show"))
        self.b_blur = _compact(QPushButton("Blur"))
        self.b_hide = _compact(QPushButton("Hide"))
        for b, m in ((self.b_show, "show"), (self.b_blur, "cloak"), (self.b_hide, "hide")):
            b.clicked.connect(lambda _=False, mode=m: self._set_mode(mode))
            strip.addWidget(b)

        # "&&" — Qt eats a single & as a mnemonic marker and would render "BW".
        self.bw = _compact(QCheckBox("B&&W"))
        self.bw.clicked.connect(self._toggle_bw)
        strip.addSpacing(GUTTER)
        strip.addWidget(self.bw)

        self.sup_status = QLabel("")
        self.sup_status.setObjectName("Hint")
        self.sup_status.setFont(_font(T.T_HINT))
        strip.addSpacing(6)
        strip.addWidget(self.sup_status)

        strip.addStretch(1)
        strip.addWidget(Divider(vertical=True))
        strip.addSpacing(GUTTER)
        # Call and Buzz sit together: the same gesture at two volumes. Buzz
        # shakes a viewer already open, Call summons someone who is not.
        # Disabled until a call backend exists -- named, not faked.
        self.b_call = _compact(QPushButton("Call"))
        self.b_call.setObjectName("Call")
        self.b_call.setEnabled(False)
        self.b_call.setToolTip("Call a viewer — not wired yet")
        strip.addWidget(self.b_call)
        self.b_buzz = _compact(QPushButton("Buzz"))
        self.b_buzz.setObjectName("Buzz")
        self.b_buzz.clicked.connect(self._buzz)
        strip.addWidget(self.b_buzz)
        self.add_layout(strip)

    def _refresh(self, _=None):
        self.win.slow.refresh_now()

    def _set_mode(self, mode):
        probes.run_async(probes.set_feed_mode, mode, done=self._refresh)

    def _toggle_bw(self):
        probes.run_async(probes.toggle_bw, done=self._refresh)

    def _buzz(self):
        QGuiApplication.beep()
        probes.run_async(probes.buzz)

    # -- supervision actions (moved from the footer, run-8 §3) --
    def _repair(self):
        if not confirm(self.win, "Repair",
                       "Kill stray ffmpeg and reconverge the pipeline?\n"
                       "The stream will drop for about 15 seconds."):
            return
        self.win.set_action_busy(True, "Repairing…")
        probes.run_async(cd._livecam, "repair",
                         done=lambda _: self.win.set_action_busy(False))

    def update_from(self, snap):
        mode = snap.get("feed_mode", "show")
        bw = snap.get("bw_mode", "false") == "true"
        svc = snap.get("svc", False)

        # §6b naming: user-facing word is Blur; `cloak` is the API value only.
        label = {"show": "SHOW", "cloak": "BLUR", "hide": "HIDE"}.get(mode, mode.upper())
        if mode == "cloak" and bw:
            label += " · B&W"
        self.mode.setText(label)

        _toggle_style(self.b_show, svc and mode == "show")
        _toggle_style(self.b_blur, svc and mode == "cloak")
        _toggle_style(self.b_hide, svc and mode == "hide")

        self.bw.blockSignals(True)
        self.bw.setChecked(bw)
        self.bw.blockSignals(False)
        # B&W only means anything while Blur is active (§6b: it's a modifier).
        self.bw.setEnabled(svc and mode == "cloak")

        for b in (self.b_show, self.b_blur, self.b_hide, self.b_buzz):
            b.setEnabled(svc)
        _toggle_style(self.b_feed, self.win.feed_on)

        busy = self.win.action_busy
        self.b_repair.setEnabled(svc and not busy)
        self.sup_status.setText(self.win.action_label if busy else "")
        self.sup_status.setStyleSheet(f"color: {T.WARN if busy else T.TEXT_MUTED};")


# ── Right column ─────────────────────────────────────────────────
class NodePanel(Panel):
    """Where this node is and how to reach it (run-3 §4a)."""

    def __init__(self):
        super().__init__("Node")
        # Field order matches the Windows node.
        self.ts = StatRow("Tailscale")
        self.lan = StatRow("Local IP")
        self.host = StatRow("Hostname")
        self.http = StatRow("HTTP")
        self.hls = StatRow("HLS")
        self.server = StatusRow("Server")
        for w in (self.ts, self.lan, self.host, self.http, self.hls, self.server):
            self.add(w)

    def update_from(self, snap):
        node = snap.get("node") or {}
        self.host.set(node.get("hostname", "—"))
        self.lan.set(snap.get("lan_ip", "—"))

        ts = node.get("tailscale", "")
        if ts:
            self.ts.set(ts)
        else:
            # §7 'Pending' rather than omitting the row — an absent field and an
            # unconfigured one are different facts.
            self.ts.set("Pending", T.TEXT_MUTED)

        self.http.set(f":{cd.API_PORT}")
        self.hls.set(f":{cd.HLS_PORT}/cam")
        self.server.set("running" if snap.get("svc") else "stopped",
                        "UP" if snap.get("svc") else "DOWN")


class StackPanel(Panel):
    """Per-service stack rows.

    run-3 §3: LIVE is reserved for on-air facts — the camera, the HLS stream, the
    HTTP surface the family actually hits. Background services read `up`. Same
    green on both; the word is what carries the distinction.
    """

    def __init__(self):
        super().__init__("Video")
        self.cam = StatusRow("Camera")
        self.ffmpeg = StatusRow("ffmpeg")
        self.rtsp = StatusRow("RTSP")
        self.mediamtx = StatusRow("mediamtx")
        self.hls = StatusRow("HLS")
        self.web = StatusRow("HTTP")
        for w in (self.cam, self.ffmpeg, self.rtsp, self.mediamtx, self.hls, self.web):
            self.add(w)
        self.fps = StatRow("FPS")
        self.add(self.fps)

    def update_from(self, snap):
        svc = snap.get("svc", False)
        v4l2 = snap.get("v4l2", False)

        # on-air facts -> LIVE
        self.cam.set("LIVE" if (v4l2 and svc) else "FOUND" if v4l2 else "NONE")
        hls = snap.get("hls", "UNKNOWN")
        self.hls.set(hls)
        self.web.set("LIVE" if snap.get("nginx") else "DOWN")

        # background services -> up
        self.ffmpeg.set("up" if snap.get("ff") else "DOWN")
        self.rtsp.set("up" if snap.get("rtsp") else "DOWN")
        self.mediamtx.set("up" if snap.get("mm") else "DOWN")

        self.fps.set(cd.read_device_env().get("FRAMERATE", "?") if svc else "N/A")


class MessagePanel(Panel):
    def __init__(self, win):
        super().__init__("Message")
        self.win = win

        head = QHBoxLayout()
        hint = QLabel("Leave a note for viewers")
        hint.setObjectName("Hint")
        hint.setFont(_font(T.T_HINT))
        self.count = QLabel("0/120")
        self.count.setObjectName("Hint")
        self.count.setFont(_font(T.T_HINT))
        tabular(self.count)
        head.addWidget(hint)
        head.addStretch(1)
        head.addWidget(self.count)
        self.add_layout(head)

        self.box = QPlainTextEdit()
        self.box.setObjectName("MsgBox")
        self.box.setFont(_font(T.T_BODY))
        self.box.setPlaceholderText("(no message)")
        self.box.setMinimumHeight(96)
        # ClickFocus, not the default StrongFocus: otherwise this is the first
        # focusable widget in the window and silently swallows keystrokes aimed
        # at the dashboard the moment it opens.
        self.box.setFocusPolicy(Qt.ClickFocus)
        self.box.textChanged.connect(self._on_change)
        self.add(self.box)

        btns = QHBoxLayout()
        btns.setSpacing(6)
        self.b_save = QPushButton("Save")
        self.b_save.setObjectName("Primary")
        self.b_save.clicked.connect(self._save)
        self.b_clear = QPushButton("Clear")
        self.b_clear.clicked.connect(self._clear)
        # run-3 §4d: editing with no way to abandon changes is a real gap.
        self.b_cancel = QPushButton("Cancel")
        self.b_cancel.clicked.connect(self._cancel)
        for b in (self.b_save, self.b_clear, self.b_cancel):
            btns.addWidget(b)
        btns.addStretch(1)
        self.add_layout(btns)

        # Lock and the API health share a row, as on Windows — the API state is a
        # one-word reassurance, not a headline that needs its own line.
        foot = QHBoxLayout()
        foot.setContentsMargins(0, 0, 0, 0)
        self.lock = QCheckBox("Lock message")
        self.lock.clicked.connect(self._toggle_lock)
        self.api = QLabel("API —")
        self.api.setObjectName("Hint")
        self.api.setFont(_font(T.T_HINT))
        foot.addWidget(self.lock)
        foot.addStretch(1)
        foot.addWidget(self.api)
        self.add_layout(foot)

        self._server_text = ""
        self._locked = False

    def _on_change(self):
        text = self.box.toPlainText()
        if len(text) > probes.MAX_MSG:
            # Enforce the 120-char contract at the widget, not just server-side.
            cur = self.box.textCursor()
            pos = cur.position()
            self.box.blockSignals(True)
            self.box.setPlainText(text[:probes.MAX_MSG])
            cur.setPosition(min(pos, probes.MAX_MSG))
            self.box.setTextCursor(cur)
            self.box.blockSignals(False)
            text = self.box.toPlainText()
        n = len(text)
        remaining = probes.MAX_MSG - n
        color = (T.HEALTHY if remaining > 20 else
                 T.WARN if remaining > 0 else T.CRITICAL)
        self.count.setText(f"{n}/{probes.MAX_MSG}")
        self.count.setStyleSheet(f"color: {color};")
        self._sync_buttons()

    def _sync_buttons(self):
        dirty = self.box.toPlainText() != self._server_text
        self.b_save.setEnabled(not self._locked and dirty)
        self.b_clear.setEnabled(not self._locked and bool(self.box.toPlainText()))
        self.b_cancel.setEnabled(not self._locked and dirty)

    def _save(self):
        text = self.box.toPlainText()[:probes.MAX_MSG]

        def done(_):
            self._server_text = text

        probes.run_async(cd.write_broadcast, text, done=done)

    def _clear(self):
        if not confirm(self.win, "Clear message", "Clear the viewer message?"):
            return
        self.box.setPlainText("")
        probes.run_async(cd.write_broadcast, "")

    def _cancel(self):
        """Abandon unsaved edits — revert to what the server currently holds."""
        self.box.blockSignals(True)
        self.box.setPlainText(self._server_text)
        self.box.blockSignals(False)
        self._on_change()

    def _toggle_lock(self):
        probes.run_async(probes.toggle_msg_lock,
                         done=lambda _: self.win.slow.refresh_now())

    def update_from(self, snap):
        self._locked = snap.get("msg_lock", "false") == "true"
        self.lock.blockSignals(True)
        self.lock.setChecked(self._locked)
        self.lock.blockSignals(False)

        server = snap.get("broadcast", "")
        self._server_text = server
        # Never clobber an in-progress edit: only adopt the server's text when
        # the operator isn't typing and hasn't got unsaved changes.
        if not self.box.hasFocus() and self.box.toPlainText() != server:
            self.box.blockSignals(True)
            self.box.setPlainText(server)
            self.box.blockSignals(False)
            self._on_change()

        self.box.setReadOnly(self._locked)
        self._sync_buttons()
        up = snap.get("api")
        self.api.setText("API up" if up else "API down")
        self.api.setStyleSheet(f"color: {T.HEALTHY if up else T.CRITICAL};")


# ── CamHub (run-3 §4c) ───────────────────────────────────────────
class CamHubDialog(QDialog):
    """Edit the /cams grid slots. Backed by camdash's read_cams/write_cams.

    Matches camdash's scope deliberately: label and ip are editable, pinned is a
    toggle, and `stream_path`/`api_port` stay hand-edited in cams.json — same as
    upstream, so the two front-ends can't disagree about what they own.
    """

    SLOTS = 4

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Cam IPs")
        self.setStyleSheet(T.QSS)
        self.setMinimumWidth(440)

        lay = QVBoxLayout(self)
        lay.setContentsMargins(T.GAP_REGION, T.GAP_REGION, T.GAP_REGION, T.GAP_REGION)
        lay.setSpacing(T.GAP_INTERNAL)

        title = QLabel("Cams")
        title.setObjectName("SectionTitle")
        title.setFont(_font(T.T_SECTION, upper=True))
        lay.addWidget(title)

        hint = QLabel("Slots shown in the /cams grid. stream_path and api_port "
                      "stay hand-edited in cams.json.")
        hint.setObjectName("Hint")
        hint.setFont(_font(T.T_HINT))
        hint.setWordWrap(True)
        lay.addWidget(hint)

        self.slots = cd.read_cams() or []
        while len(self.slots) < self.SLOTS:
            self.slots.append({"label": "", "ip": "", "stream_path": "", "pinned": False})

        self.rows = []
        for i in range(self.SLOTS):
            row = QHBoxLayout()
            row.setSpacing(6)
            pin = QCheckBox()
            pin.setChecked(bool(self.slots[i].get("pinned")))
            pin.setToolTip("Pinned")
            label = QLineEdit(self.slots[i].get("label", ""))
            label.setPlaceholderText("label")
            label.setMaxLength(12)
            ip = QLineEdit(self.slots[i].get("ip", ""))
            ip.setPlaceholderText("ip")
            ip.setMaxLength(15)
            up = QPushButton("↑")
            up.setFixedWidth(30)
            up.clicked.connect(lambda _=False, idx=i: self._move(idx, -1))
            down = QPushButton("↓")
            down.setFixedWidth(30)
            down.clicked.connect(lambda _=False, idx=i: self._move(idx, +1))
            clear = QPushButton("✕")
            clear.setFixedWidth(30)
            clear.clicked.connect(lambda _=False, idx=i: self._clear(idx))
            for w in (pin, label, ip, up, down, clear):
                row.addWidget(w)
            row.setStretch(1, 2)
            row.setStretch(2, 3)
            lay.addLayout(row)
            self.rows.append((pin, label, ip))

        lay.addWidget(Divider())
        btns = QHBoxLayout()
        btns.addStretch(1)
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        save = QPushButton("Save")
        save.setObjectName("Primary")
        save.clicked.connect(self._save)
        btns.addWidget(cancel)
        btns.addWidget(save)
        lay.addLayout(btns)

    def _harvest(self):
        for i, (pin, label, ip) in enumerate(self.rows):
            self.slots[i]["pinned"] = pin.isChecked()
            self.slots[i]["label"] = label.text().strip()
            self.slots[i]["ip"] = ip.text().strip()

    def _repaint_rows(self):
        for i, (pin, label, ip) in enumerate(self.rows):
            pin.setChecked(bool(self.slots[i].get("pinned")))
            label.setText(self.slots[i].get("label", ""))
            ip.setText(self.slots[i].get("ip", ""))

    def _move(self, idx, delta):
        j = idx + delta
        if not (0 <= j < self.SLOTS):
            return
        self._harvest()
        self.slots[idx], self.slots[j] = self.slots[j], self.slots[idx]
        self._repaint_rows()

    def _clear(self, idx):
        self._harvest()
        self.slots[idx] = {"label": "", "ip": "", "stream_path": "", "pinned": False}
        self._repaint_rows()

    def _save(self):
        self._harvest()
        err = cd.write_cams(self.slots)
        if err:
            QMessageBox.warning(self, "Cam IPs", f"Could not save: {err}")
            return
        self.accept()


# ── Bottom action toolbar (run-3 §4b) ────────────────────────────
class BottomBar(QFrame):
    """Node-level actions + status readout.

    run-11 §1: Pause server moved here from the feed strip -- it toggles
    mediamtx/broadcast-api, the server function, and has nothing to do with
    the feed's visual mode; it sits next to Stop server, the other control
    on this same layer. Repair stayed under the feed (run-8 §3 was right
    about that one): it's a genuine pipeline-reconvergence action. Dark /
    Cam IPs are node-level too -- no obvious feed-adjacent home -- and On/Off
    toggles the launchd login agent, which outlives any single feed session.
    """

    def __init__(self, win):
        super().__init__()
        self.win = win
        self.setObjectName("Panel")
        lay = QHBoxLayout(self)
        lay.setContentsMargins(T.PANEL_PAD, 8, T.PANEL_PAD, 8)
        lay.setSpacing(6)

        self.b_onoff = QPushButton("Stop server")
        self.b_onoff.clicked.connect(self._onoff)
        self.b_pause = QPushButton("Pause server")
        self.b_pause.clicked.connect(self._pause)
        self.b_dark = QPushButton("Dark")
        self.b_dark.clicked.connect(self._dark)
        self.b_cams = QPushButton("Cam IPs")
        self.b_cams.clicked.connect(self._cams)
        # run-8/9 §5: derived light theme, matching the Windows node's control.
        self.b_theme = QPushButton("Light theme")
        self.b_theme.clicked.connect(lambda: self.win.toggle_theme())
        for b in (self.b_onoff, self.b_pause, self.b_dark, self.b_cams, self.b_theme):
            lay.addWidget(b)

        lay.addStretch(1)

        self.status = QLabel("Ready")
        self.status.setObjectName("Hint")
        self.status.setFont(_font(T.T_HINT))
        lay.addWidget(self.status)

        lay.addSpacing(10)
        lay.addWidget(Divider(vertical=True))
        lay.addSpacing(10)

        # Build label sits with the licence badge rather than the status
        # readout: it is fixed identity, not changing state.
        build = QLabel(probes.version_label(short=True))
        build.setObjectName("Hint")
        build.setFont(_font(T.T_BADGE))
        build.setToolTip(f"Build: {probes.version_label()}")
        lay.addWidget(build)

        lay.addSpacing(10)
        lay.addWidget(Divider(vertical=True))
        lay.addSpacing(10)

        lic = QLabel("GPL 3.0")
        lic.setObjectName("Hint")
        lic.setFont(_font(T.T_BADGE))
        lay.addWidget(lic)

    def set_status(self, text, color=None):
        self.status.setText(text)
        self.status.setStyleSheet(f"color: {color or T.TEXT_MUTED};")

    def _refresh(self, _=None):
        self.win.slow.refresh_now()

    def _onoff(self):
        enabled = self.win.last.get("enabled", False)
        action = "disable" if enabled else "enable"
        if not confirm(self.win, "Server On/Off",
                       f"{'Disable' if enabled else 'Enable'} the launchd login agent?\n"
                       f"This persists across reboots."):
            return
        self.win.set_action_busy(True, "Disabling…" if enabled else "Enabling…")
        probes.run_async(cd._livecam, action,
                         done=lambda _: self.win.set_action_busy(False))

    def _dark(self):
        probes.run_async(cd._livecam, "dark", done=self._refresh)

    def _cams(self):
        CamHubDialog(self.win).exec()

    # Semantics unchanged from run-8 §3, only the label and location moved
    # (run-11 §1): manual on/off toggle for deliberate downtime, confirmation
    # -gated, session-only. Does not touch the login agent -- Stop server does.
    def _pause(self):
        running = self.win.last.get("svc", False)
        action = "stop" if running else "start"
        if not confirm(self.win, "Pause server",
                       f"{'Stop' if running else 'Start'} mediamtx and broadcast-api?"):
            return
        self.win.set_action_busy(True, "Stopping…" if running else "Starting…")
        probes.run_async(cd._livecam, action,
                         done=lambda _: self.win.set_action_busy(False))

    def update_from(self, snap):
        enabled = snap.get("enabled", False)
        svc = snap.get("svc", False)
        busy = self.win.action_busy
        self.b_onoff.setText("Stop server" if enabled else "Start server")
        self.b_onoff.setEnabled(not busy)
        self.b_pause.setText("Pause server" if svc else "Resume server")
        self.b_pause.setEnabled(not busy)
        self.b_dark.setEnabled(svc)
        self.set_status(self.win.action_label if busy else "Ready",
                        T.WARN if busy else None)


# ── Main window ──────────────────────────────────────────────────
class Dashboard(QWidget):
    def __init__(self):
        super().__init__()
        # The build goes in the title so it is readable from the window list
        # and from a screenshot, without needing the window focused.
        self.setWindowTitle(
            f"camdash — HLS Livecam Operator — {probes.version_label(short=True)}")
        self.resize(1400, 900)
        self.setStyleSheet(T.QSS)
        self.last = {}
        # Shared across FeedPanel (Pause/Repair) and BottomBar (On/Off): all
        # three ultimately shell out to `livecam`, which is not safe to run
        # concurrently with itself, so only one supervision action runs at a time
        # regardless of which panel started it.
        self.action_busy = False
        self.action_label = ""

        root = QVBoxLayout(self)
        root.setContentsMargins(T.GAP_REGION, T.GAP_REGION, T.GAP_REGION, T.GAP_REGION)
        root.setSpacing(T.GAP_REGION)
        root.addLayout(self._build_header())

        self.p_system = SystemPanel()
        self.p_disk = DiskPanel()
        self.p_procs = ProcessPanel()
        self.p_feed = FeedPanel(self)
        self.p_node = NodePanel()
        self.p_stack = StackPanel()
        self.p_msg = MessagePanel(self)

        cols = QHBoxLayout()
        cols.setSpacing(T.GAP_REGION)

        # Side columns: content-height panels stacked to the top, trailing
        # stretch soaking up the slack. That slack is what the feed reclaims.
        left = QVBoxLayout()
        left.setSpacing(T.GAP_REGION)
        left.addWidget(self.p_stack)
        left.addWidget(self.p_procs, 1)

        # PROCESSES sits under the feed rather than in a side column. The video is
        # 16:9; a tall narrow slot letterboxes it and the feed ends up surrounded
        # by dead bars — growing the panel without growing the picture. Parking a
        # content-height panel below takes the surplus height, so contain-fit
        # lands close to filling the width instead.
        centre = QVBoxLayout()
        centre.setSpacing(T.GAP_REGION)
        centre.addWidget(self.p_feed, 1)
        centre.addWidget(self.p_system)

        right = QVBoxLayout()
        right.setSpacing(T.GAP_REGION)
        for p in (self.p_disk, self.p_msg, self.p_node):
            right.addWidget(p)
        right.addStretch(1)

        cols.addLayout(left, 3)
        cols.addLayout(centre, 8)
        cols.addLayout(right, 3)
        root.addLayout(cols, 1)

        self.bottom = BottomBar(self)
        root.addWidget(self.bottom)

        self.panels = (self.p_system, self.p_disk, self.p_procs, self.p_feed,
                       self.p_node, self.p_stack, self.p_msg, self.bottom)

        # Split cadence: cheap counters at 500ms, everything that walks processes
        # or does I/O at 2.5s. See probes.FastWorker for why.
        self.fast = probes.FastWorker(interval_ms=500)
        self.fast.snapshot.connect(self.on_snapshot)
        self.fast.start()

        self.slow = probes.SlowWorker(interval_ms=2500)
        self.slow.snapshot.connect(self.on_snapshot)
        self.slow.start()

        # PM decision (run-7): the preview starts OFF and the decoder starts
        # stopped. This is not a mission-critical device -- the stack panels
        # report health, and web viewing is the primary use. Nothing here
        # affects the family-facing stream, which is served by mediamtx
        # regardless of whether this window decodes a preview.
        self.video = None
        self.feed_on = probes.read_feed_state(default=False)
        self._apply_video_state()

        self.clock = QTimer(self)
        self.clock.timeout.connect(self._tick_clock)
        self.clock.start(1000)
        self._tick_clock()

    def _build_header(self):
        bar = QHBoxLayout()
        bar.setContentsMargins(2, 0, 2, 0)

        self.brand = QLabel("Webcam Server Stack")
        self.brand.setObjectName("Brand")
        self.brand.setFont(_font(T.T_BRAND))
        self.uptime = QLabel("—")
        self.uptime.setObjectName("Hint")
        self.uptime.setFont(_font(T.T_HINT))
        tabular(self.uptime)

        left = QVBoxLayout()
        left.setSpacing(1)
        left.addWidget(self.brand)
        left.addWidget(self.uptime)

        self.pill = StatusPill()
        self.qualifiers = QLabel("")
        self.qualifiers.setObjectName("Hint")
        self.qualifiers.setFont(_font(T.T_HINT))

        mid = QHBoxLayout()
        mid.setSpacing(8)
        mid.addWidget(self.pill)
        mid.addWidget(self.qualifiers)

        self.host = QLabel(cd.socket.gethostname())
        self.host.setFont(_font(T.T_BUTTON))
        self.host.setStyleSheet(f"color: {T.HEALTHY}; font-weight: 600;")
        self.time = QLabel("")
        self.time.setObjectName("Hint")
        self.time.setFont(_font(T.T_HINT))
        tabular(self.time)

        right = QVBoxLayout()
        right.setSpacing(1)
        right.addWidget(self.host, alignment=Qt.AlignRight)
        right.addWidget(self.time, alignment=Qt.AlignRight)

        bar.addLayout(left)
        bar.addStretch(1)
        bar.addLayout(mid)
        bar.addStretch(1)
        bar.addLayout(right)
        return bar

    def _tick_clock(self):
        self.time.setText(time.strftime("%H:%M:%S  %d-%b-%y"))

    def toggle_theme(self):
        want = "light" if T.THEME == "dark" else "dark"
        self.setStyleSheet(T.set_theme(want))
        self.bottom.b_theme.setText("Dark theme" if want == "light" else "Light theme")
        # Custom-painted widgets cache colors between calls; force one repaint
        # each rather than waiting out the next 500ms tick.
        self.pill._repaint()
        self.p_feed.view.update()
        for panel in (self.p_system,):
            for m in (panel.cpu, panel.mem, panel.swap, panel.load):
                m.bar.update()

    def set_action_busy(self, busy, label=""):
        self.action_busy = busy
        self.action_label = label
        self.p_feed.update_from(self.last)
        self.bottom.update_from(self.last)

    def on_snapshot(self, partial):
        # Two workers on different cadences both feed this; merge, then render
        # from the merged view so panels never see a half-populated snapshot.
        self.last.update(partial)
        snap = self.last
        if "svc" not in snap:
            return   # nothing from SlowWorker yet; don't paint a false OFF state

        up = snap.get("up", 0)
        self.uptime.setText(f"uptime {up // 3600}h {(up % 3600) // 60}m")

        svc = snap.get("svc", False)
        base = cd.system_status(snap.get("hls", "DOWN"), snap.get("ff"), snap.get("mm"))

        # §2/§6c: the on-air pill is RED and pulsing. Service-up rows stay green.
        if not svc:
            self.pill.set_state("OFF", "OFF")
        elif base == "LIVE":
            self.pill.set_state("LIVE", "LIVE")
        else:
            self.pill.set_state(base)

        quals = []
        if not svc:
            quals.append("services stopped")
        elif base in ("DEGRADED", "ERROR", "DOWN"):
            quals.append("suggest repair")
        if snap.get("dark"):
            quals.append("feed hidden")
        self.qualifiers.setText(" | ".join(quals))

        for p in self.panels:
            p.update_from(snap)

        # Server down means no live source; drop the frame rather than let the
        # decoder's last texture linger (stale-frame rule).
        if not svc and self.feed_on:
            self.p_feed.view.clear_signal()

    # ── preview decoder lifecycle ────────────────────────────────
    def _want_video(self):
        """Decode only when the operator asked for it AND someone can see it."""
        return (self.feed_on
                and self.isVisible()
                and not self.isMinimized()
                and not self.window().windowState() & Qt.WindowMinimized)

    def _apply_video_state(self):
        want = self._want_video()
        if want and self.video is None:
            # QThread can't be restarted once stopped; make a fresh one.
            self.video = VideoWorker(probes.RTSP_URL)
            self.video.frame.connect(self.p_feed.view.set_frame)
            self.video.signal_lost.connect(self.p_feed.view.clear_signal)
            self.video.start()
        elif not want and self.video is not None:
            self.video.stop()
            self.video = None
            self.p_feed.view.clear_signal()
        self.p_feed.view.set_off(not self.feed_on)

    def toggle_feed(self):
        self.feed_on = not self.feed_on
        probes.write_feed_state(self.feed_on)   # keep camdash in step
        self._apply_video_state()

    def changeEvent(self, event):
        # Minimise/restore is the cheap, reliable visibility signal on macOS;
        # true occlusion isn't exposed to Qt.
        super().changeEvent(event)
        if event.type() == QEvent.WindowStateChange:
            self._apply_video_state()

    def showEvent(self, event):
        super().showEvent(event)
        self._apply_video_state()

    def hideEvent(self, event):
        super().hideEvent(event)
        self._apply_video_state()

    def closeEvent(self, event):
        if self.video is not None:
            self.video.stop()
        self.fast.stop()
        self.slow.stop()
        super().closeEvent(event)


def main():
    app = QApplication(sys.argv)
    app.setApplicationName("camdash")
    icon_path = os.path.join(os.path.dirname(__file__), "assets", "AppIcon.icns")
    if os.path.exists(icon_path):
        # Not a bundled .app -- launched as `python -m gui.app` -- so there is
        # no Info.plist to read CFBundleIconFile from. Qt sets the Dock tile
        # for the running process directly from this at runtime instead.
        app.setWindowIcon(QIcon(icon_path))
    f = QFont()
    f.setPixelSize(T.T_BUTTON[0])
    app.setFont(f)
    win = Dashboard()
    win.show()
    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
