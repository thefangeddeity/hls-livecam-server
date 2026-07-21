//! Header and footer -- camdash's `draw_dashboard()` header block and
//! `draw_footer()`, transcribed. Status pill (design doc §5/§7 item 2):
//! the shared `.live-pill` component, not plain colored text -- "single
//! indicator vocabulary" across all three surfaces.

use eframe::egui;

use super::components;
use super::fonts;
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

        // Real bug, caught from a screenshot, not by me: this used to key
        // "was this stopped on purpose" off `mediamtx_alive` (a liveness
        // signal), which predates run 4's SERVER: ON/OFF control. With
        // mediamtx down from a genuine failure (e.g. a port conflict) but
        // nobody having touched Server Off, that rendered a calm green
        // "services stopped" pill over an actual outage -- exactly
        // backwards. `p.enabled` (added in run 4) is the real operator-
        // intent signal and is what decides calm-vs-alarming now;
        // liveness alone no longer does.
        let intentional_off = !pstatus.enabled;
        let feed_mode = self.state.feed_mode.lock().unwrap().clone();
        // Run 6: Blur (cloak) is a live feed, just obscured -- distinct
        // from Hide (black). Both are "not fully exposed" for pill-color
        // purposes (neither is the red on-air state), but they get
        // different qualifier words: a blurred feed is still broadcasting
        // presence, not hidden.
        let blurred = feed_mode == "cloak";
        let feed_hidden = feed_mode == "hide";
        let obscured = blurred || feed_hidden;

        let mut qualifiers = Vec::new();
        if intentional_off {
            qualifiers.push("services stopped");
        } else if matches!(base_status, "DEGRADED" | "DOWN") {
            qualifiers.push("suggest repair");
        }
        if blurred {
            qualifiers.push("feed blurred");
        } else if feed_hidden {
            qualifiers.push("feed hidden");
        }

        // Design-system spine decision: red = LIVE/on-air, green =
        // healthy/service-up. This is the same split camdash's own
        // header_attr() already encoded (LIVE + exposed = RED) -- ported
        // faithfully in run 3, so no flip was needed here, only the
        // token values changed (design doc §2, verified against this
        // exact code before ratification). DEGRADED/DOWN while the
        // operator has NOT turned it off is always critical now -- there
        // is no "calm" reading of an unintentional failure.
        let pill_color = if intentional_off {
            if obscured { theme::warn() } else { theme::healthy() }
        } else if base_status == "LIVE" {
            if obscured { theme::warn() } else { theme::live() }
        } else {
            theme::critical()
        };

        // Pill text stays terse -- the design doc's status vocabulary is
        // one word (LIVE/DEGRADED/DOWN/OFF), not a sentence. Qualifier
        // detail ("suggest repair," "feed hidden") renders as small dim
        // text after the pill instead of crammed inside it.
        let pill_label = if intentional_off { "OFF" } else { base_status };
        let qualifier_text = qualifiers.join("  \u{2022}  ");

        let full_w = ui.available_width();
        let right_text = format!("\"{}\" {}", self.hostname, chrono_like_timestamp());
        let right_w = (text_width(ui, &right_text) + 16.0).min(full_w * 0.3);
        let left_text = format!("Webcam Server Stack  \u{2022}  uptime {up_h}h {up_m}m");
        let left_w = (text_width(ui, &left_text) + 16.0).min(full_w * 0.3);
        let mid_w = (full_w - left_w - right_w).max(0.0);
        let height = ui.available_height();

        // Title/uptime and hostname/timestamp are chrome, not status --
        // color is reserved for the status pill alone.
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(left_w, height),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    // `.strong()` does not embolden without a bold face
                    // registered (review finding) -- use the real
                    // semibold family for the app title.
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&left_text)
                                .font(fonts::semibold(theme::SIZE_BODY))
                                .color(theme::text()),
                        )
                        .truncate(),
                    );
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(mid_w, height),
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    ui.horizontal(|ui| {
                        components::status_pill(ui, pill_label, pill_color);
                        if !qualifier_text.is_empty() {
                            ui.add_space(8.0);
                            ui.colored_label(theme::text_muted(), &qualifier_text);
                        }
                    });
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(right_w, height),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&right_text).color(theme::text_dim()))
                            .truncate(),
                    );
                },
            );
        });
    }

    pub(super) fn draw_footer(&mut self, ui: &mut egui::Ui) {
        // Buzz moved to the FEED toolbar (design doc §8/A1: it's a
        // control, grouped with the feed it affects) -- footer keeps
        // chrome plus the two node-wide actions that don't belong to any
        // one panel: theme switch and the fleet-roster (Cam IPs) editor.
        ui.horizontal(|ui| {
            if self.tray.is_some() && ui.button("Minimize to tray").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            // Cam IPs (run 6): camdash's `[i]` as a modal roster editor
            // over this node's cams.json.
            if ui.button("Cam IPs").clicked() {
                self.ip_manager_open = true;
            }
            // Live theme switch -- the palette is read per-frame, so the
            // whole window follows on the next repaint; choice persists
            // in the state dir.
            let toggle_label = if theme::is_light() { "Dark theme" } else { "Light theme" };
            if ui.button(toggle_label).clicked() {
                theme::set_light(!theme::is_light());
            }
            ui.add_space(12.0);
            ui.colored_label(
                theme::text_muted(),
                format!(
                    "IP: {} (Tailscale)  \u{2022}  {}",
                    if self.tailscale.is_empty() { "n/a" } else { &self.tailscale },
                    self.hostname
                ),
            );
            // Headroom: an IP/fleet panel (camdash's [i] cam IPs) can land
            // here later without a relayout -- this row already has slack
            // and a natural slot before the trailing GPL text.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(theme::text_muted(), "GPL 3.0");
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
