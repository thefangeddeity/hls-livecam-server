//! Segoe UI, loaded from the OS at runtime -- not shipped. The design
//! doc's font stack (`-apple-system, ..., "Segoe UI", ...`) resolves to
//! Segoe UI on Windows already; this just makes egui (which otherwise uses
//! its own bundled font) render with the same system font the web viewers
//! get for free, so all three surfaces read as one family.
//!
//! Three faces, not one: the design system's weights are load-bearing --
//! section titles and pill labels are 600, Buzz is 700 -- and egui's
//! `.strong()` does NOT synthesize bold; with only the regular face
//! registered, every "600" in the spec silently rendered regular (review
//! finding). Semibold and bold are registered as named families so call
//! sites pick a weight explicitly via `semibold(size)`/`bold(size)`.
//!
//! Every face falls back gracefully (missing file -> that family maps to
//! whatever regular resolved to, ultimately egui's default font) -- a
//! slightly-off font is a cosmetic regression, not a reason to fail to
//! start. Note egui panics on layout with an *unregistered* named family,
//! so both named families are always registered, even in fallback.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;

const REGULAR_PATH: &str = r"C:\Windows\Fonts\segoeui.ttf";
const SEMIBOLD_PATH: &str = r"C:\Windows\Fonts\seguisb.ttf";
const BOLD_PATH: &str = r"C:\Windows\Fonts\segoeuib.ttf";

const SEMIBOLD_FAMILY: &str = "segoe_semibold";
const BOLD_FAMILY: &str = "segoe_bold";

/// 600-weight FontId -- section titles, pill labels, stat emphasis.
pub fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(SEMIBOLD_FAMILY.into()))
}

/// 700-weight FontId -- Buzz (`.buzz-btn` specifies 700).
pub fn bold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(BOLD_FAMILY.into()))
}

/// Idempotent; guarded because apply_theme() runs every frame and font
/// installation is not free (file reads + full glyph-atlas rebuild).
static INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn install(ctx: &egui::Context) {
    if INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }

    let mut fonts = egui::FontDefinitions::default();

    // Regular replaces the default proportional face. Monospace is left
    // as egui's genuinely-monospace default (it used to get hijacked with
    // Segoe UI too, which made FontId::monospace a lie -- review finding).
    let mut base = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if let Ok(bytes) = std::fs::read(REGULAR_PATH) {
        fonts
            .font_data
            .insert("segoe_ui".to_owned(), egui::FontData::from_owned(bytes).into());
        base.insert(0, "segoe_ui".to_owned());
        fonts.families.insert(egui::FontFamily::Proportional, base.clone());
    } else {
        eprintln!("fonts: {REGULAR_PATH} not found -- using egui's default font");
    }

    let mut register_weight = |family: &str, path: &str, data_key: &str| {
        let mut chain = base.clone();
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(data_key.to_owned(), egui::FontData::from_owned(bytes).into());
            chain.insert(0, data_key.to_owned());
        } else {
            eprintln!("fonts: {path} not found -- {family} falls back to regular");
        }
        fonts
            .families
            .insert(egui::FontFamily::Name(family.into()), chain);
    };
    register_weight(SEMIBOLD_FAMILY, SEMIBOLD_PATH, "segoe_ui_semibold");
    register_weight(BOLD_FAMILY, BOLD_PATH, "segoe_ui_bold");

    ctx.set_fonts(fonts);
}
