//! Header and footer -- camdash's `draw_dashboard()` header block and
//! `draw_footer()`, transcribed. Read live: `system_status()`,
//! `header_attr()`, the qualifiers list (`services stopped` /
//! `suggest repair` / `feed hidden`).

use eframe::egui;

use super::theme;
use super::App;
use crate::pipeline::PipelineStatus;

impl App {
    pub(super) fn draw_header(&mut self, ui: &mut egui::Ui, pstatus: &PipelineStatus) {
        // System uptime (since boot), matching camdash's psutil.boot_time()
        // semantics -- not this process's own start time.
        let up = self.snapshot.uptime_secs;
        let up_h = up / 3600;
        let up_m = (up % 3600) / 60;

        // camdash's system_status(hls, ff, mm) -- our hls_state only has
        // LIVE/DOWN (see pipeline.rs), so there's no ERROR branch here.
        let base_status = if pstatus.hls_state == "LIVE" {
            "LIVE"
        } else if pstatus.capture_alive || pstatus.mediamtx_alive {
            "DEGRADED"
        } else {
            "DOWN"
        };

        // "svc" in camdash means "is the core stack up." mediamtx_alive is
        // the closest single Windows-side signal for that.
        let svc = pstatus.mediamtx_alive;
        // camdash's `dark` header qualifier is the *legacy* dark flag,
        // which nothing in the current viewer JS drives (confirmed dead
        // in run-1 research). feed_mode == hide/cloak is the practical
        // "is the feed actually hidden right now" signal, so that's what
        // drives this qualifier instead.
        let feed_mode = self.state.feed_mode.lock().unwrap().clone();
        let hidden = feed_mode == "hide" || feed_mode == "cloak";

        let mut qualifiers = Vec::new();
        if !svc {
            qualifiers.push("services stopped");
        } else if matches!(base_status, "DEGRADED" | "DOWN") {
            qualifiers.push("suggest repair");
        }
        if hidden {
            qualifiers.push("feed hidden");
        }

        let header_color = if !svc {
            if hidden {
                theme::YELLOW_BOLD
            } else {
                theme::GREEN_BOLD
            }
        } else if base_status == "LIVE" {
            if hidden {
                theme::YELLOW_BOLD
            } else {
                theme::RED_BOLD
            }
        } else {
            theme::RED_BOLD
        };

        // Explicit width split, not three stacked ui.horizontal() sections:
        // sequential sections don't reserve space against each other in
        // egui, so the middle "centered_and_justified" block was eating
        // into the right-aligned timestamp's space and clipping it
        // (caught by screenshotting the actual window, not just reading
        // the code -- the clipped "26" was only visible that way).
        let full_w = ui.available_width();
        let right_text = {
            let ts = chrono_like_timestamp();
            format!("\"{}\" {}", self.hostname, ts)
        };
        let right_w = (text_width(ui, &right_text) + 16.0).min(full_w * 0.35);
        let left_text = format!("Webcam Server Stack  \u{2022}  uptime {up_h}h {up_m}m");
        let left_w = (text_width(ui, &left_text) + 16.0).min(full_w * 0.35);
        let mid_w = (full_w - left_w - right_w).max(0.0);
        let height = ui.available_height();

        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(left_w, height),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.colored_label(theme::GREEN_BOLD, &left_text);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(mid_w, height),
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    let mut mid = format!("\u{2022} {base_status}");
                    if !qualifiers.is_empty() {
                        mid.push_str(" | ");
                        mid.push_str(&qualifiers.join(" | "));
                    }
                    ui.colored_label(header_color, egui::RichText::new(mid).strong());
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(right_w, height),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.colored_label(theme::WHITE, &right_text);
                },
            );
        });
    }

    pub(super) fn draw_footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Buzz").clicked() {
                let state = self.state.clone();
                self.spawn_async(async move {
                    let _ = state.buzz_now();
                });
            }
            if self.tray.is_some() && ui.button("Minimize to tray").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            ui.add_space(12.0);
            ui.colored_label(
                theme::DIM,
                format!(
                    "IP: {} (Tailscale)  \u{2022}  {}",
                    if self.tailscale.is_empty() { "n/a" } else { &self.tailscale },
                    self.hostname
                ),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(theme::DIM, "GPL 3.0");
            });
        });
    }
}

/// Measures rendered text width at the UI's current font/size so the
/// header's left/right column widths fit their content instead of using
/// guessed pixel budgets.
fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    ui.fonts(|f| f.layout_no_wrap(text.to_string(), font_id, egui::Color32::WHITE).size().x)
}

/// No chrono dependency for one timestamp -- std::time plus a fixed civil-
/// calendar conversion is enough for "HH:MM DD-Mon-YY", camdash's exact
/// format (`time.strftime("%H:%M %d-%b-%y")`).
fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let secs_of_day = secs % 86400;
    let (h, m) = (secs_of_day / 3600, (secs_of_day % 3600) / 60);

    // Civil-from-days (Howard Hinnant's algorithm), proleptic Gregorian.
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_num = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m_num <= 2 { y + 1 } else { y };

    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mon = MONTHS[(m_num as usize - 1).min(11)];
    format!("{h:02}:{m:02} {d:02}-{mon}-{:02}", y % 100)
}
