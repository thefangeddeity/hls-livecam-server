"""
Shared components — the design system's §5 component set, as Qt widgets.

Every one of these exists in the ratified spec: Section/panel, Pill, Button,
Input/textarea, Chip. Nothing new is invented here; where the spec names a style,
this file implements it and nothing more.
"""
from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtGui import QColor, QFont, QImage, QPainter, QPen, QPixmap
from PySide6.QtWidgets import (
    QFrame, QHBoxLayout, QLabel, QSizePolicy, QVBoxLayout, QWidget,
)

from . import tokens as T


def _font(spec, upper=False):
    f = QFont()
    f.setPixelSize(spec[0])
    f.setWeight(QFont.Weight(spec[1]))
    if upper:
        f.setCapitalization(QFont.AllUppercase)
        f.setLetterSpacing(QFont.PercentageSpacing, 106)  # §3: +0.06em
    return f


def tabular(label: QLabel) -> QLabel:
    """§3: tabular-nums on every live-updating numeric. Non-negotiable."""
    f = label.font()
    f.setStyleHint(QFont.SansSerif)
    f.setFixedPitch(True)
    label.setFont(f)
    return label


class Panel(QFrame):
    """§5 Section/panel: panel fill, border, radius, 14px padding, section title."""

    def __init__(self, title, parent=None):
        super().__init__(parent)
        self.setObjectName("Panel")
        outer = QVBoxLayout(self)
        outer.setContentsMargins(T.PANEL_PAD, T.PANEL_PAD, T.PANEL_PAD, T.PANEL_PAD)
        outer.setSpacing(T.GAP_INTERNAL)

        self.title = QLabel(title)
        self.title.setObjectName("SectionTitle")
        self.title.setFont(_font(T.T_SECTION, upper=True))
        outer.addWidget(self.title)

        self.body = QVBoxLayout()
        self.body.setSpacing(3)   # 6 on ariana; tightened for tanzania's shorter screen
        # Body takes all surplus height. Without the stretch factor Qt shares it
        # between the title and the body, and a stretched panel ends up with its
        # section title floating in the middle of empty space.
        outer.addLayout(self.body, 1)
        self._outer = outer

    def add(self, w):
        self.body.addWidget(w)
        return w

    def add_layout(self, lay):
        self.body.addLayout(lay)
        return lay

    def add_stretch(self):
        self.body.addStretch(1)


class StatusPill(QLabel):
    """§5 Pill — dot + label, colored per the §2 state model.

    §2/§6c (ratified): the on-air state is RED and pulsing. A *service* being up
    is green, and that is a different widget (StatusRow). camdash's lineage has
    these backwards; this is the corrected behavior.
    """

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFont(_font(T.T_BUTTON))
        self._state = "OFF"
        self._pulse_on = True
        self._timer = QTimer(self)
        self._timer.timeout.connect(self._pulse)
        self.setAlignment(Qt.AlignCenter)
        self.set_state("OFF")

    def set_state(self, state, label=None):
        self._state = state
        text = label or state
        on_air = state == "LIVE"
        if on_air and not self._timer.isActive():
            self._timer.start(650)
        elif not on_air and self._timer.isActive():
            self._timer.stop()
            self._pulse_on = True

        color = T.LIVE if on_air else T.status_color(state)
        self._color = color
        self._text = text
        self._repaint()

    def _pulse(self):
        self._pulse_on = not self._pulse_on
        self._repaint()

    def _repaint(self):
        dot = self._color
        if self._state == "LIVE" and not self._pulse_on:
            dot = T.LIVE_DIM
        # Never color alone (§2) — the label word always travels with it.
        self.setText(f"  ●  {self._text}  ")
        self.setStyleSheet(
            f"color: {self._color};"
            f"background: {T.PANEL_2};"
            f"border: 1px solid {dot};"
            f"border-radius: 999px;"
            f"padding: 3px 10px;"
            f"font-weight: 600;"
        )


class StatusRow(QWidget):
    """A `LABEL   VALUE` line where VALUE carries a §2 status color + word."""

    def __init__(self, label, parent=None):
        super().__init__(parent)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(6)
        self.k = QLabel(label)
        self.k.setFont(_font(T.T_BUTTON))
        self.k.setStyleSheet(f"color: {T.TEXT_DIM};")
        self.v = QLabel("—")
        self.v.setFont(_font(T.T_BUTTON))
        tabular(self.v)
        self.v.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        lay.addWidget(self.k)
        lay.addStretch(1)
        lay.addWidget(self.v)

    def set(self, value, state=None):
        self.v.setText(str(value))
        color = T.status_color(state if state is not None else str(value))
        self.v.setStyleSheet(f"color: {color}; font-weight: 600;")


class StatRow(QWidget):
    """A plain `LABEL   VALUE` line with no status semantics."""

    def __init__(self, label, parent=None):
        super().__init__(parent)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(6)
        self.k = QLabel(label)
        self.k.setFont(_font(T.T_BUTTON))
        self.k.setStyleSheet(f"color: {T.TEXT_DIM};")
        self.v = QLabel("—")
        self.v.setFont(_font(T.T_BUTTON))
        tabular(self.v)
        self.v.setStyleSheet(f"color: {T.TEXT};")
        self.v.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        lay.addWidget(self.k)
        lay.addStretch(1)
        lay.addWidget(self.v)

    def set(self, value, color=None):
        self.v.setText(str(value))
        self.v.setStyleSheet(f"color: {color or T.TEXT};")


class Meter(QWidget):
    """Labelled bar for CPU / MEM / SWAP / LOAD, colored by the camdash ramp."""

    def __init__(self, label, parent=None):
        super().__init__(parent)
        self._pct = 0.0
        self._color = T.HEALTHY
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(3)

        head = QHBoxLayout()
        head.setContentsMargins(0, 0, 0, 0)
        self.k = QLabel(label)
        self.k.setFont(_font(T.T_BUTTON))
        self.k.setStyleSheet(f"color: {T.TEXT_DIM};")
        self.v = QLabel("—")
        self.v.setFont(_font(T.T_STAT))
        tabular(self.v)
        self.v.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        head.addWidget(self.k)
        head.addStretch(1)
        head.addWidget(self.v)
        lay.addLayout(head)

        self.bar = _Bar()
        lay.addWidget(self.bar)

    def set(self, pct, text=None, color=None):
        pct = max(0.0, min(100.0, float(pct)))
        color = color or T.meter_color(pct)
        self.v.setText(text if text is not None else f"{pct:.1f}%")
        self.v.setStyleSheet(f"color: {color}; font-weight: 500;")
        self.bar.set(pct, color)


class _Bar(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self._pct = 0.0
        self._color = T.HEALTHY
        self.setFixedHeight(5)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)

    def set(self, pct, color):
        self._pct, self._color = pct, color
        self.update()

    def paintEvent(self, _):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        p.setPen(Qt.NoPen)
        p.setBrush(QColor(T.PANEL_2))
        p.drawRoundedRect(self.rect(), 2.5, 2.5)
        w = int(self.width() * self._pct / 100.0)
        if w > 0:
            p.setBrush(QColor(self._color))
            r = self.rect()
            r.setWidth(w)
            p.drawRoundedRect(r, 2.5, 2.5)


class Divider(QFrame):
    """Hairline rule. Horizontal by default; vertical isolates Buzz (run-3 §2)."""

    def __init__(self, vertical=False, parent=None):
        super().__init__(parent)
        self.setFrameShape(QFrame.VLine if vertical else QFrame.HLine)
        self.setStyleSheet(f"background: {T.BORDER}; border: none;")
        if vertical:
            self.setFixedWidth(1)
        else:
            self.setFixedHeight(1)


class Chip(QLabel):
    """§5 Chip (.info-chip) — used for the §7 'Pending' badge on unwired controls."""

    def __init__(self, text, parent=None):
        super().__init__(text, parent)
        self.setFont(_font(T.T_BADGE))
        self.setStyleSheet(
            f"background: {T.PANEL_2}; color: {T.TEXT_MUTED};"
            f"border: 1px solid {T.BORDER}; border-radius: {T.RADIUS_SM}px;"
            f"padding: 1px 5px;"
        )


class FeedView(QWidget):
    """The FEED surface.

    Holds exactly one of two things: a freshly decoded frame, or the OFFLINE
    NO-SIGNAL placeholder (§2, and the stale-frame lesson). It never holds the
    last decoded texture after the pipeline dies — `clear_signal()` drops the
    pixmap outright.
    """

    def __init__(self, parent=None):
        super().__init__(parent)
        self._pix = None
        self._off = False
        self.setMinimumSize(320, 180)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)

    def set_off(self, off):
        """FEED OFF is a different fact from NO SIGNAL and must not look like it.

        An operator who sees NO SIGNAL starts debugging a stream that is fine.
        Off is deliberate and says so.
        """
        self._off = off
        if off:
            self._pix = None
        self.update()

    def set_frame(self, img: QImage):
        self._pix = QPixmap.fromImage(img)
        self.update()

    def clear_signal(self):
        self._pix = None
        self.update()

    @property
    def has_signal(self):
        return self._pix is not None

    def paintEvent(self, _):
        p = QPainter(self)
        p.setRenderHint(QPainter.SmoothPixmapTransform)
        p.fillRect(self.rect(), QColor(T.BG))

        if self._pix is None:
            self._paint_placeholder(p)
            return

        # Contain-fit, centered: letterbox/pillarbox, never stretch or crop.
        scaled = self._pix.scaled(
            self.size(), Qt.KeepAspectRatio, Qt.SmoothTransformation
        )
        x = (self.width() - scaled.width()) // 2
        y = (self.height() - scaled.height()) // 2
        p.drawPixmap(x, y, scaled)

    def _paint_placeholder(self, p):
        """OFFLINE state — the web viewer's `.ph` no-signal pattern."""
        p.setPen(QPen(QColor(T.BORDER), 1, Qt.DashLine))
        p.setBrush(Qt.NoBrush)
        r = self.rect().adjusted(1, 1, -2, -2)
        p.drawRoundedRect(r, T.RADIUS_SM, T.RADIUS_SM)

        icon = QFont()
        icon.setPixelSize(26)
        p.setFont(icon)
        p.setPen(QColor(T.TEXT_MUTED))
        p.drawText(
            self.rect().adjusted(0, -14, 0, -14), Qt.AlignCenter,
            "◍" if not self._off else "◌"
        )

        f = QFont()
        f.setPixelSize(T.T_SECTION[0])
        f.setWeight(QFont.Weight(T.T_SECTION[1]))
        f.setCapitalization(QFont.AllUppercase)
        f.setLetterSpacing(QFont.PercentageSpacing, 106)
        p.setFont(f)
        p.setPen(QColor(T.TEXT_MUTED))
        p.drawText(self.rect().adjusted(0, 26, 0, 26), Qt.AlignCenter,
                   "Feed Off" if self._off else "No Signal")
        if self._off:
            h = QFont()
            h.setPixelSize(T.T_HINT[0])
            p.setFont(h)
            p.drawText(self.rect().adjusted(0, 48, 0, 48), Qt.AlignCenter,
                       "decoder stopped — press Feed to view")
