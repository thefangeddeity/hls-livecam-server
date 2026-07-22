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

        // Qualifiers are no longer crammed next to the pill -- the header
        // center is the pill alone, dead-centre (open-source convention:
        // a three-section title bar, left/center/right; the primary
        // status indicator owns the center). The qualifier detail moved
        // to the footer status bar (ambient_status), where a bottom
        // status bar conventionally reports current state.
        let pill_label = if intentional_off { "OFF" } else { base_status };

        // Left title, right clock, and the pill placed at the TRUE window
        // centre via explicit rects -- sequential thirds accumulate
        // item-spacing drift that left the pill visibly off-centre
        // (operator: "LIVE dead in the middle"). The pill gets a rect its
        // own width, centred on rect.center().x, so it lands exactly
        // centre regardless of the side text widths.
        //
        // Allocate a real row FIRST (not available_rect_before_wrap):
        // placing content only in child UIs allocates nothing in the
        // parent, so the auto-sized header panel collapsed to its margins
        // and the text clipped against the OS title bar (operator: "header
        // too narrow, can't read anything"). allocate_exact_size reserves
        // the height so the panel sizes correctly.
        let height = 28.0;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
        let left_text = format!("Webcam Server Stack  \u{2022}  uptime {up_h}h {up_m}m");
        let right_text = format!("\"{}\"   {}", self.hostname, chrono_like_timestamp());

        let pill_w = components::status_pill_width(ui, pill_label);
        let side_w = ((rect.width() - pill_w) / 2.0 - 12.0).max(0.0);

        // Left: title + uptime (semibold; `.strong()` doesn't embolden
        // without a real bold face -- review finding).
        let mut left_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_size(rect.min, egui::vec2(side_w, rect.height())))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        left_ui.add(
            egui::Label::new(
                egui::RichText::new(&left_text)
                    .font(fonts::semibold(theme::SIZE_BODY))
                    .color(theme::text()),
            )
            .truncate(),
        );

        // Center: the status pill, dead-centre.
        let pill_rect = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.center().y),
            egui::vec2(pill_w, rect.height()),
        );
        let mut pill_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(pill_rect)
                .layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        components::status_pill(&mut pill_ui, pill_label, pill_color);

        // Right: hostname + clock, right-aligned (far right).
        let mut right_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_max(
                    egui::pos2(rect.right() - side_w, rect.top()),
                    rect.max,
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        right_ui.add(
            egui::Label::new(egui::RichText::new(&right_text).color(theme::text_dim())).truncate(),
        );
    }

    pub(super) fn draw_footer(&mut self, ui: &mut egui::Ui, p: &PipelineStatus) {
        // Bottom status bar (open-source convention): app-level action
        // buttons on the left, the live status message center, chrome
        // (GPL) far right. The old static IP text moved into the NETWORK
        // panel; this row now reports what's happening instead.
        ui.horizontal(|ui| {
            // Server on/off is an app-level power control, not a feed
            // action -- it lives here with the other node-wide buttons,
            // not in the feed toolbar (which is now feed-only). This also
            // keeps the feed toolbar within the center column at minimum
            // window width.
            let server_label = if p.enabled { "Stop server" } else { "Start server" };
            if components::button(ui, server_label).clicked() {
                let on = p.enabled;
                let pipeline = self.pipeline.clone();
                self.spawn_async(async move {
                    pipeline.set_enabled(!on).await;
                });
                self.set_status(if p.enabled { "Server stopped" } else { "Server started" });
            }
            // Cam IPs (run 6): camdash's `[i]` as a modal roster editor.
            if components::button(ui, "Cam IPs").clicked() {
                self.ip_manager_open = true;
            }
            // The footer "Minimize" button was removed in run 7: closing the
            // window (X) now minimizes to the tray itself, so a separate
            // button did the same thing. OS-minimize (the native title-bar
            // "_") still works for a taskbar-minimize; tray -> Show restores
            // a hidden window; tray -> Quit is the deliberate exit.
            // Live theme switch -- palette is read per-frame, so the whole
            // window follows next repaint; choice persists in the state dir.
            let toggle_label = if theme::is_light() { "Dark theme" } else { "Light theme" };
            if components::button(ui, toggle_label).clicked() {
                theme::set_light(!theme::is_light());
            }

            ui.add_space(16.0);
            // Status message: the last operator action for a few seconds,
            // then "Ready" at rest. This is a TRANSIENT event line, NOT a
            // liveness light -- on-air state is the header pill, service
            // state is the NODE panel's Server row. No permanent dot here
            // (it would be a redundant third liveness signal -- operator
            // correction).
            match self.active_status() {
                Some(msg) => ui.colored_label(theme::text_dim(), msg),
                None => ui.colored_label(theme::text_muted(), "Ready"),
            };

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(theme::text_muted(), "GPL 3.0");
            });
        });
    }
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
