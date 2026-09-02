"""
Design tokens — HLS Livecam Unified Design System v1 (RATIFIED, §1-§5 binding).

Token NAMES are the interface; raw hex appears exactly once, here. Everything
else in the GUI references these names, per design system §1.

Qt Style Sheets are a CSS subset, so the viewers' `:root` tokens and the
.btn / .msg-box / .live-pill / .info-chip component definitions port close to
verbatim. Differences from CSS that matter, and how they're handled:
  * no CSS variables    -> QSS is built by .format() substitution below
  * no `transform`      -> §5's active `scale(0.98)` becomes a :pressed fill shift
  * no `opacity` on QWidget in QSS -> §7's 0.45 disabled affordance is expressed
    as explicit muted fg/border colors on :disabled
"""

# ── §1 Color tokens ──────────────────────────────────────────────
# Surfaces
BG            = "#111113"
PANEL         = "#1c1c1e"
PANEL_2       = "#242426"
BORDER        = "#38383a"
BORDER_STRONG = "#48484a"

# Text
TEXT       = "#f5f5f7"
TEXT_DIM   = "#98989d"
TEXT_MUTED = "#6e6e73"

# Accent / interactive
ACCENT       = "#0a84ff"
ACCENT_HOVER = "#409cff"

# Semantic status (§2 model)
LIVE     = "#ff453a"   # on-air / recording
CRITICAL = "#ff453a"   # error / down  (same hex, different role)
WARN     = "#ff9f0a"   # degraded / reconnecting
HEALTHY  = "#30d158"   # service up / OK
OFFLINE  = "#6e6e73"   # stopped / no-signal / disabled

# Amber, not red -- the grammar the web panel settled on and this surface
# must not contradict, because it is the same operator looking at the same
# node: RED withdraws a signal (Hide, Stop), AMBER interrupts everyone (Buzz),
# GREEN opens a channel (Call). Buzz interrupts; it does not withdraw.
#
# Contrast measured, not eyeballed: white on the old red was 3.15:1, under the
# 4.5:1 floor, and white on amber would be worse at 2.06:1. Near-black on amber
# is 9.08:1. Bright faces take dark legends.
BUZZ = "#ff9f0a"       # §5 buzz button fill -- interrupts, so amber
BUZZ_INK = "#1a1102"   # 9.08:1 on the above

# Dimmed LIVE, for the pill's pulse trough (§2: "red, pulsing")
LIVE_DIM = "#7a2019"

# Radius (§1)
RADIUS    = 12         # panels
RADIUS_SM = 8          # inputs, buttons, chips

# ── Light theme (run-8/9 §5, derived — not in the ratified design system) ──
# Derivation rule, as specified: surfaces lighten, text darkens, semantic status
# colors hold hue and shift only where contrast against the new surfaces demands
# it. Dark tokens stay exactly as ratified; this is an additive second set
# selected by _apply_theme(), never a replacement.
#
# Surfaces: the dark ramp bg/panel/panel-2/border/border-strong runs
# #111113 -> #48484a, roughly evenly spaced steps of a near-black ramp toward
# mid-grey. Lightened is the same ramp reflected toward white, using Apple's own
# light-mode system greys as anchors (NSColor windowBackgroundColor family)
# rather than a blind per-channel invert, which would have produced a bluish
# cast instead of a neutral grey scale.
LIGHT_BG            = "#f2f2f7"   # app background
LIGHT_PANEL         = "#ffffff"   # panel fill
LIGHT_PANEL_2       = "#e9e9ee"   # raised/inset element fill
LIGHT_BORDER        = "#d1d1d6"   # standard border
LIGHT_BORDER_STRONG = "#c7c7cc"   # emphasized border

LIGHT_TEXT       = "#1c1c1e"   # primary
LIGHT_TEXT_DIM   = "#6e6e73"   # secondary — same token value as dark's TEXT_MUTED;
                                # the ramp midpoint is roughly self-inverse
LIGHT_TEXT_MUTED = "#8e8e93"   # tertiary / placeholder

# Accent and every semantic status color HOLD HUE, unchanged from dark, per
# run-8 §4's explicit instruction. Re-exported here only for the QSS builder's
# uniform access pattern -- the values are identical to the dark set above.
LIGHT_ACCENT       = ACCENT
LIGHT_ACCENT_HOVER = ACCENT_HOVER
LIGHT_LIVE         = LIVE
LIGHT_CRITICAL     = CRITICAL
LIGHT_WARN         = WARN
LIGHT_HEALTHY      = HEALTHY
LIGHT_OFFLINE      = "#8e8e93"   # was text-muted's dark value #6e6e73; the token
                                  # IS text-muted by definition (§1), so it moves
                                  # with LIGHT_TEXT_MUTED rather than holding hue
LIGHT_BUZZ = BUZZ
LIGHT_LIVE_DIM = "#f5b8b3"       # LIVE pill's pulse trough against a light panel

# ── §4 Spacing ───────────────────────────────────────────────────
PANEL_PAD   = 14       # panel padding
GAP_INTERNAL = 8       # internal element gap (§4 says 6-8)
GAP_REGION  = 16       # between major regions

# ── §3 Typography ────────────────────────────────────────────────
# §3's stack is `-apple-system, ..., "SF Pro Text", ...`. On macOS that resolves
# to SF Pro Text, which Qt reaches as ".AppleSystemUIFont" — the literal string
# "SF Pro Text" is not a registered family name and only costs a slow alias scan
# at startup, so it is deliberately omitted here. Same typeface, named the way Qt
# can find it.
FAMILY = '".AppleSystemUIFont"'

# Type scale: (px, weight)
T_OVERLAY_LG  = (48, 700)   # large transient (switch-count overlay)
T_OVERLAY     = (15, 600)   # overlay titles ("Signal lost")
T_BRAND       = (14, 600)   # brand / primary
T_BODY        = (14, 400)   # message body
T_STAT        = (13, 500)   # stat values, buttons-large
T_BUTTON      = (12, 500)   # buttons, chips, status bar
T_SECTION     = (11, 600)   # section titles — uppercase, +0.06em
T_HINT        = (11, 500)   # hints, char count
T_BADGE       = (10, 600)   # badges, micro-labels


def status_color(state: str) -> str:
    """§2 state -> token. Never used without a label word alongside it.

    Case-insensitive: background services render the word lowercase (`up`) to
    keep LIVE reserved for on-air facts, per §2 and the Windows node.
    """
    return {
        "LIVE":     HEALTHY,   # a *service* being up is green (§2)
        "UP":       HEALTHY,
        "OK":       HEALTHY,
        "FOUND":    HEALTHY,
        "PASSED":   HEALTHY,
        "WARN":     WARN,
        "DEGRADED": WARN,
        "RUNNING":  WARN,
        "RECONNECTING": WARN,
        "DOWN":     CRITICAL,
        "ERROR":    CRITICAL,
        "FAIL":     CRITICAL,
        "HIGH":     CRITICAL,
        "OFF":      OFFLINE,
        "NO SIGNAL": OFFLINE,
        "NONE":     OFFLINE,
        "N/A":      OFFLINE,
        "UNKNOWN":  OFFLINE,
        "PENDING":  OFFLINE,
    }.get((state or "").strip().upper(), TEXT_DIM)


def meter_color(pct: float) -> str:
    """Usage ramp: accent -> warn -> critical, at camdash's led() thresholds.

    Normal is `accent`, not `healthy`. Under the §2 spine decision green means
    "a service is up"; spending it on a 23%-busy CPU bar overloads the one token
    the status model depends on. The Windows node reads the same way.
    """
    if pct < 50:
        return ACCENT
    if pct < 80:
        return WARN
    return CRITICAL


# ── Theme switching ───────────────────────────────────────────────
# _DARK is the ratified §1 palette, captured here (after all the assignments
# above have run) so it survives being overwritten. _LIGHT is the derived set.
# THEME is the only new state; every color name below (BG, PANEL, TEXT, ...)
# stays a plain module-level string, reassigned in place by set_theme(). Every
# call site reads T.BG / T.status_color() etc. at paint/update time rather than
# importing a bound copy, so flipping these here is sufficient -- nothing else
# needs to know a theme exists.
_DARK = dict(
    BG=BG, PANEL=PANEL, PANEL_2=PANEL_2, BORDER=BORDER, BORDER_STRONG=BORDER_STRONG,
    TEXT=TEXT, TEXT_DIM=TEXT_DIM, TEXT_MUTED=TEXT_MUTED,
    ACCENT=ACCENT, ACCENT_HOVER=ACCENT_HOVER,
    BUZZ_INK=BUZZ_INK, LIVE=LIVE, CRITICAL=CRITICAL, WARN=WARN, HEALTHY=HEALTHY, OFFLINE=OFFLINE,
    BUZZ=BUZZ, LIVE_DIM=LIVE_DIM,
)
_LIGHT = dict(
    BG=LIGHT_BG, PANEL=LIGHT_PANEL, PANEL_2=LIGHT_PANEL_2,
    BORDER=LIGHT_BORDER, BORDER_STRONG=LIGHT_BORDER_STRONG,
    TEXT=LIGHT_TEXT, TEXT_DIM=LIGHT_TEXT_DIM, TEXT_MUTED=LIGHT_TEXT_MUTED,
    ACCENT=LIGHT_ACCENT, ACCENT_HOVER=LIGHT_ACCENT_HOVER,
    LIVE=LIGHT_LIVE, CRITICAL=LIGHT_CRITICAL, WARN=LIGHT_WARN,
    HEALTHY=LIGHT_HEALTHY, OFFLINE=LIGHT_OFFLINE,
    BUZZ=LIGHT_BUZZ, LIVE_DIM=LIGHT_LIVE_DIM,
)
THEME = "dark"


def set_theme(name: str) -> str:
    """Switch the active palette. Returns the rebuilt QSS to reapply.

    Caller's job: `self.setStyleSheet(tokens.set_theme('light'))` on the window,
    then trigger a repaint on the custom-painted widgets (StatusPill, Meter,
    FeedView) that cache a QColor between updates -- they pick up the new
    module values on their next scheduled snapshot/paint regardless, since the
    dashboard re-renders every 500ms.
    """
    global THEME, QSS
    global BG, PANEL, PANEL_2, BORDER, BORDER_STRONG
    global TEXT, TEXT_DIM, TEXT_MUTED, ACCENT, ACCENT_HOVER
    global LIVE, CRITICAL, WARN, HEALTHY, OFFLINE, BUZZ, LIVE_DIM
    THEME = "light" if name == "light" else "dark"
    active = _LIGHT if THEME == "light" else _DARK
    (BG, PANEL, PANEL_2, BORDER, BORDER_STRONG, TEXT, TEXT_DIM, TEXT_MUTED,
     ACCENT, ACCENT_HOVER, LIVE, CRITICAL, WARN, HEALTHY, OFFLINE, BUZZ,
     LIVE_DIM) = (
        active["BG"], active["PANEL"], active["PANEL_2"], active["BORDER"],
        active["BORDER_STRONG"], active["TEXT"], active["TEXT_DIM"],
        active["TEXT_MUTED"], active["ACCENT"], active["ACCENT_HOVER"],
        active["LIVE"], active["CRITICAL"], active["WARN"], active["HEALTHY"],
        active["OFFLINE"], active["BUZZ"], active["LIVE_DIM"],
    )
    QSS = _build_qss()
    return QSS


def _build_qss() -> str:
    return f"""
QWidget {{
    background: {BG};
    color: {TEXT};
    font-family: {FAMILY};
    font-size: {T_BUTTON[0]}px;
}}

/* §5 Section/panel: panel fill, border, radius, 14px padding */
QFrame#Panel {{
    background: {PANEL};
    border: 1px solid {BORDER};
    border-radius: {RADIUS}px;
}}

QLabel#SectionTitle {{
    color: {TEXT_DIM};
    font-size: {T_SECTION[0]}px;
    font-weight: {T_SECTION[1]};
    text-transform: uppercase;
    letter-spacing: 0.06em;
    background: transparent;
}}

QLabel {{ background: transparent; }}

QLabel#Hint      {{ color: {TEXT_MUTED}; font-size: {T_HINT[0]}px; font-weight: {T_HINT[1]}; }}
QLabel#Stat      {{ font-size: {T_STAT[0]}px; font-weight: {T_STAT[1]}; }}
QLabel#Brand     {{ font-size: {T_BRAND[0]}px; font-weight: {T_BRAND[1]}; }}
QLabel#Mono      {{ font-family: "SF Mono", Menlo, monospace; font-size: {T_HINT[0]}px; }}

/* §5 Button */
QPushButton {{
    background: {PANEL_2};
    border: 1px solid {BORDER_STRONG};
    border-radius: {RADIUS_SM}px;
    color: {TEXT};
    font-size: {T_BUTTON[0]}px;
    font-weight: {T_BUTTON[1]};
    padding: 6px 10px;
}}
QPushButton:hover  {{ background: {BG}; border-color: {TEXT_MUTED}; }}
QPushButton:pressed {{ background: {BORDER}; }}
QPushButton:disabled {{
    color: {TEXT_MUTED};
    border-color: {BORDER};
    background: {PANEL};
}}

/* §5 toggle-on: raised fill, brighter border (the .dark-btn.is-dark pattern) */
QPushButton[toggled="true"] {{
    background: {BORDER};
    border-color: {TEXT_DIM};
    color: {TEXT};
    font-weight: 600;
}}

/* Compact density: the FEED strip packs Feed/mode/B&W/Pause/Repair/Buzz into
   one row, more controls than the standard 6px/10px padding comfortably
   holds at anything narrower than a wide window. Toolbar-style tighter
   controls here only -- same pattern as Xcode/Photos.app toolbars -- rather
   than shrinking the window's other buttons or dropping a control. */
QPushButton[density="compact"] {{
    padding: 4px 7px;
    font-size: {T_HINT[0]}px;
}}
QCheckBox[density="compact"] {{
    font-size: {T_HINT[0]}px;
    spacing: 3px;
}}
QCheckBox[density="compact"]::indicator {{
    width: 11px; height: 11px;
}}

QPushButton#Primary {{
    background: {ACCENT}; border-color: {ACCENT}; color: #ffffff;
}}
QPushButton#Primary:hover {{ background: {ACCENT_HOVER}; border-color: {ACCENT_HOVER}; }}
QPushButton#Primary:disabled {{
    background: {PANEL}; border-color: {BORDER}; color: {TEXT_MUTED};
}}

QPushButton#Buzz {{
    background: {BUZZ}; border-color: {BUZZ}; color: {BUZZ_INK}; font-weight: 700;
}}
QPushButton#Buzz:hover {{ background: #ffb340; border-color: #ffb340; }}

/* Green opens a channel. Dark face with a light legend -- the same
   construction Hide's red uses, measured 7.48:1. Disabled until a call
   backend exists: present as a name, not faked as a working control. */
QPushButton#Call {{
    background: #1f5c2e; border-color: #123f1d; color: #eafcef; font-weight: 700;
}}
QPushButton#Call:hover {{ background: #2d7a3f; border-color: #1f5c2e; }}
QPushButton#Call:disabled {{
    background: {PANEL}; border-color: {BORDER}; color: {TEXT_MUTED};
}}

/* §5 Input / textarea (.msg-box) */
QPlainTextEdit#MsgBox {{
    background: {PANEL_2};
    border: 1px solid {BORDER};
    border-radius: {RADIUS_SM}px;
    color: {TEXT};
    font-size: {T_BODY[0]}px;
    padding: 8px;
    selection-background-color: {ACCENT};
}}
QPlainTextEdit#MsgBox:focus {{
    border: 1px solid {ACCENT};
    background: {PANEL};
}}
QPlainTextEdit#MsgBox:disabled {{
    color: {TEXT_MUTED};
    border-color: {BORDER};
}}

/* transparent, or the inherited QWidget fill paints a darker band across the
   panel behind the label — reads as a stray rule at dashboard scale */
QCheckBox {{
    background: transparent;
    color: {TEXT_DIM};
    font-size: {T_BUTTON[0]}px;
    spacing: 6px;
}}
QCheckBox:disabled {{ color: {TEXT_MUTED}; }}
QCheckBox::indicator {{
    width: 13px; height: 13px;
    border: 1px solid {BORDER_STRONG};
    border-radius: 3px;
    background: {PANEL_2};
}}
QCheckBox::indicator:checked {{
    background: {ACCENT};
    border-color: {ACCENT};
}}
QCheckBox::indicator:disabled {{ border-color: {BORDER}; background: {PANEL}; }}

QLineEdit {{
    background: {PANEL_2};
    border: 1px solid {BORDER};
    border-radius: {RADIUS_SM}px;
    color: {TEXT};
    padding: 4px 6px;
}}
QLineEdit:focus {{ border-color: {ACCENT}; }}

QScrollBar:vertical {{ background: {PANEL}; width: 8px; border: none; }}
QScrollBar::handle:vertical {{ background: {BORDER_STRONG}; border-radius: 4px; }}
QScrollBar::add-line, QScrollBar::sub-line {{ height: 0; }}

QToolTip {{
    background: {PANEL_2};
    color: {TEXT};
    border: 1px solid {BORDER_STRONG};
    padding: 4px;
}}
"""


QSS = _build_qss()
