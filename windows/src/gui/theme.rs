//! Palette and status-coloring rules, transcribed from camdash's curses
//! color pairs and threshold functions -- not reinterpreted, not
//! "improved." Read live from pkg/usr/local/bin/camdash:
//!   init_pair(1, GREEN) (2, YELLOW) (3, RED) (4, WHITE) (5, CYAN) (6, 244 gray)
//!   status_attr / header_attr / load_attr / led() / the SMART sattr() closure
//! curses A_BOLD conventionally renders as the bright variant of the base
//! ANSI color in most terminals (including the reference screenshot), so
//! each color below has a normal and a bold/bright shade.

use egui::Color32;

pub const BG: Color32 = Color32::BLACK;

pub const GREEN: Color32 = Color32::from_rgb(0x00, 0xAA, 0x00);
pub const GREEN_BOLD: Color32 = Color32::from_rgb(0x55, 0xFF, 0x55);
pub const YELLOW: Color32 = Color32::from_rgb(0xAA, 0xAA, 0x00);
pub const YELLOW_BOLD: Color32 = Color32::from_rgb(0xFF, 0xFF, 0x55);
pub const RED: Color32 = Color32::from_rgb(0xAA, 0x00, 0x00);
pub const RED_BOLD: Color32 = Color32::from_rgb(0xFF, 0x55, 0x55);
pub const WHITE: Color32 = Color32::from_rgb(0xAA, 0xAA, 0xAA);
pub const WHITE_BOLD: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const CYAN: Color32 = Color32::from_rgb(0x00, 0xAA, 0xAA);
pub const CYAN_BOLD: Color32 = Color32::from_rgb(0x55, 0xFF, 0xFF);
pub const DIM: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);

/// camdash's `status_attr(status)`. LIVE/OK -> green bold; DEGRADED/
/// RUNNING/SHARED -> yellow bold; ERROR/DOWN/FAIL -> red bold; else plain
/// green (its `curses.color_pair(1)` fallthrough).
pub fn status_color(status: &str, dim: bool) -> Color32 {
    if dim {
        return DIM;
    }
    match status {
        "LIVE" | "OK" => GREEN_BOLD,
        "DEGRADED" | "RUNNING" | "SHARED" => YELLOW_BOLD,
        "ERROR" | "DOWN" | "FAIL" => RED_BOLD,
        _ => GREEN,
    }
}

/// camdash's `led(val)`: <50 green, <80 yellow bold, else red bold. Drives
/// the SYSTEM panel's CPU/MEM/SWAP bar fill color.
pub fn led(val: f32, dim: bool) -> Color32 {
    if dim {
        return DIM;
    }
    if val < 50.0 {
        GREEN
    } else if val < 80.0 {
        YELLOW_BOLD
    } else {
        RED_BOLD
    }
}

/// camdash's `load_attr(l, cores)`: l < cores*0.7 green; l <= cores yellow
/// bold; else red bold.
pub fn load_color(load: f64, cores: usize, dim: bool) -> Color32 {
    if dim {
        return DIM;
    }
    let cores = cores as f64;
    if load < cores * 0.7 {
        GREEN
    } else if load <= cores {
        YELLOW_BOLD
    } else {
        RED_BOLD
    }
}

/// camdash's per-field SMART `sattr(key)`: nonzero -> red bold, zero ->
/// green bold, missing -> plain text.
pub fn smart_field_color(value: Option<i64>, dim: bool) -> Color32 {
    if dim {
        return DIM;
    }
    match value {
        Some(n) if n > 0 => RED_BOLD,
        Some(_) => GREEN_BOLD,
        None => WHITE,
    }
}

/// camdash's PROCESSES row color: cpu>30 red bold, >10 yellow bold, >2
/// white bold, else dim.
pub fn proc_color(cpu_percent: f32, dim: bool) -> Color32 {
    if dim {
        return DIM;
    }
    if cpu_percent > 30.0 {
        RED_BOLD
    } else if cpu_percent > 10.0 {
        YELLOW_BOLD
    } else if cpu_percent > 2.0 {
        WHITE_BOLD
    } else {
        DIM
    }
}
