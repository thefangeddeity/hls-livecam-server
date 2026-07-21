//! Palette, typography constants, and status-coloring rules -- the Windows
//! rendering of `hls-livecam-design-system.md` (ratified), now in two
//! modes. The dark palette is the ratified token set, extracted verbatim
//! from the web viewers' `:root` CSS. The light palette is its Apple-HIG
//! counterpart: each dark token swapped for the *same system color's*
//! documented light-mode variant (iOS/macOS publish both -- e.g. green
//! #30d158 dark / #34c759 light, blue #0a84ff dark / #007aff light), plus
//! Apple's standard light surfaces (#f5f5f7 page, white panels). So the
//! light view is theme-colored by construction, not an inverted dark.
//!
//! Palette entries are functions, not consts: they read a process-wide
//! light/dark flag so the operator can switch live. The flag persists in
//! the state dir next to everything else this app remembers.

use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, Vec2};

// ------------------------------------------------------------- mode switch

static LIGHT: AtomicBool = AtomicBool::new(false);
const THEME_FILE: &str = "theme.txt";

pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// Set and persist. Persistence is best-effort -- a failed write costs
/// the preference across restarts, nothing else.
pub fn set_light(on: bool) {
    LIGHT.store(on, Ordering::Relaxed);
    let _ = std::fs::write(
        crate::state::state_dir().join(THEME_FILE),
        if on { "light" } else { "dark" },
    );
}

/// Load the persisted choice at startup (before the first frame).
pub fn load_persisted() {
    if let Ok(s) = std::fs::read_to_string(crate::state::state_dir().join(THEME_FILE)) {
        LIGHT.store(s.trim() == "light", Ordering::Relaxed);
    }
}

#[inline]
fn pick(dark: Color32, light: Color32) -> Color32 {
    if is_light() {
        light
    } else {
        dark
    }
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

// --------------------------------------------------------------- surfaces

pub fn bg() -> Color32 {
    pick(rgb(0x111113), rgb(0xF5F5F7))
}
pub fn panel() -> Color32 {
    pick(rgb(0x1C1C1E), rgb(0xFFFFFF))
}
pub fn panel_2() -> Color32 {
    pick(rgb(0x242426), rgb(0xF2F2F7))
}
pub fn border() -> Color32 {
    pick(rgb(0x38383A), rgb(0xD2D2D7))
}
pub fn border_strong() -> Color32 {
    pick(rgb(0x48484A), rgb(0xC7C7CC))
}

// ------------------------------------------------------------------- text

pub fn text() -> Color32 {
    pick(rgb(0xF5F5F7), rgb(0x1D1D1F))
}
pub fn text_dim() -> Color32 {
    pick(rgb(0x98989D), rgb(0x6E6E73))
}
pub fn text_muted() -> Color32 {
    pick(rgb(0x6E6E73), rgb(0x86868B))
}

// ---------------------------------------------------------------- accent

pub fn accent() -> Color32 {
    pick(rgb(0x0A84FF), rgb(0x007AFF))
}
#[allow(dead_code)] // spec token; Save lost its accent treatment in review
pub fn accent_hover() -> Color32 {
    pick(rgb(0x409CFF), rgb(0x0071E3))
}

// ------------------------------------------------------- semantic status
//
// LIVE and CRITICAL share a hex by design -- the design doc's point is
// that the *word* "danger" was overloaded across two facts (feed on-air
// vs. service failed), not that they need distinct colors. Two names so
// call sites read as which fact they're stating.

/// Feed on-air / recording. Broadcast convention: red.
pub fn live() -> Color32 {
    pick(rgb(0xFF453A), rgb(0xFF3B30))
}
/// A service/process failed or is down.
pub fn critical() -> Color32 {
    live()
}
/// Degraded / reconnecting / a risk that isn't clean but isn't failed.
pub fn warn() -> Color32 {
    pick(rgb(0xFF9F0A), rgb(0xFF9500))
}
/// A service/process is up and good.
pub fn healthy() -> Color32 {
    pick(rgb(0x30D158), rgb(0x34C759))
}
/// Intentionally stopped / absent / disabled -- reads as muted on purpose.
pub fn offline() -> Color32 {
    text_muted()
}

/// Buzz-specific red (`.buzz-btn`) -- its own token in the source CSS.
pub fn buzz() -> Color32 {
    rgb(0xFF3B30)
}
pub fn buzz_hover() -> Color32 {
    rgb(0xE0352B)
}

// --------------------------------------------------------------- radius

pub const RADIUS: u8 = 12;
pub const RADIUS_SM: u8 = 8;

// ---------------------------------------------------- section-title style
//
// The panel-header convention (design doc §3): 600 weight, uppercase,
// +0.06em letter-spacing, text-dim. Size is 13px here, not the web's
// 11px: Fluent's group-header style is Body Strong (14px/600, sentence
// case), and uppercase tracked text at 13px carries roughly the same
// visual weight -- 11px caps that read fine at browser distance read
// undersized in a desktop panel header (operator feedback).
pub const SECTION_TITLE_SIZE: f32 = 13.0;
pub fn section_title_color() -> Color32 {
    text_dim()
}

// -------------------------------------------------------------- spacing

pub const PANEL_GAP: f32 = 8.0;
/// Tighter row rhythm for the dense status panels -- the web sidebar's
/// `.stat` rows are a list rhythm, not the 8px section gap.
pub const STAT_ROW_GAP: f32 = 4.0;
/// Minimum height of one stat row. egui's `horizontal()` uses
/// `interact_size.y` as the minimum row height, so without lowering it
/// inside dense panels every key/value row balloons to the 32px control
/// minimum (caught from a screenshot).
pub const STAT_ROW_MIN_H: f32 = 20.0;

// Control sizing follows the Windows Fluent Design standard for desktop
// apps: standard button = 32px min height, ~11px/5px padding, with the
// type ramp below from the same spec so text and control sizes are
// proportioned to each other by design.
pub const BUTTON_PADDING: Vec2 = Vec2 { x: 11.0, y: 5.0 };
pub const MIN_BUTTON_HEIGHT: f32 = 32.0;

// Fluent type ramp (the subset this app uses): Body 14, control text
// Body-size, Small 11 (captions/badges), Heading 16.
pub const SIZE_BODY: f32 = 14.0;
pub const SIZE_BUTTON: f32 = 14.0;
pub const SIZE_SMALL: f32 = 11.0;
pub const SIZE_HEADING: f32 = 16.0;

// ============================================================ status logic
//
// Thresholds are camdash's, untouched since run 3. The HEALTHY/normal
// band returns *uncolored* text per the web `.stat` pattern -- color is
// a signal precisely because most rows don't have one (design review).

/// Value color for a named status. Healthy states (LIVE/OK) are plain
/// text; only degraded/failed states get color.
pub fn status_color(status: &str) -> Color32 {
    match status {
        "DEGRADED" | "RUNNING" | "SHARED" => warn(),
        "ERROR" | "DOWN" | "FAIL" | "NONE" => critical(),
        _ => text(),
    }
}

/// SYSTEM meter fill: neutral accent in the normal band, colored only at
/// camdash's warn/critical thresholds (<50 / <80).
pub fn meter_fill(val: f32) -> Color32 {
    if val < 50.0 {
        accent()
    } else if val < 80.0 {
        warn()
    } else {
        critical()
    }
}

/// SYSTEM value-text color at the same thresholds.
pub fn value_color(val: f32) -> Color32 {
    if val < 50.0 {
        text()
    } else if val < 80.0 {
        warn()
    } else {
        critical()
    }
}

/// camdash's PROCESSES thresholds, value-only: >30 critical, >10 warn,
/// >2 plain, idle dim.
pub fn proc_color(cpu_percent: f32) -> Color32 {
    if cpu_percent > 30.0 {
        critical()
    } else if cpu_percent > 10.0 {
        warn()
    } else if cpu_percent > 2.0 {
        text()
    } else {
        offline()
    }
}
