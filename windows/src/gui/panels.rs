//! The panel bodies. Content and thresholds transcribed from camdash's
//! `draw_dashboard()` originally; presentation follows the web `.stat`
//! pattern per the run-5 design review: dim sentence-case key, plain
//! right-aligned value, color on the value only and only for exceptional
//! states. VIDEO's control buttons live in the FEED panel's attached
//! toolbar (layout.rs) -- controls-vs-display split, design doc §6d.

use eframe::egui;

use super::components::{self, filled_button, stat_row, stat_row_offline};
use super::theme;
use super::App;
use crate::pipeline::PipelineStatus;
use crate::state::is_valid_mode;

impl App {
    // ------------------------------------------------------- DISK / SMART
    pub(super) fn draw_disk_smart(&mut self, ui: &mut egui::Ui) {
        dense(ui);
        let d = &self.disk;

        // A disk *name* is identity, not a status -- plain value (it was
        // green, review finding).
        stat_row(ui, "Disk", &d.disk_name, None);

        match d.health_status.as_deref() {
            Some("Healthy") => stat_row(ui, "Assess", "PASSED", None),
            Some(other) => stat_row(ui, "Assess", &other.to_uppercase(), Some(theme::critical())),
            None => stat_row_offline(ui, "Assess", "?"),
        }

        match (d.health_status.as_deref(), d.operational_status.as_deref()) {
            (Some("Healthy"), Some("OK")) => stat_row(ui, "Risk", "OK", None),
            (Some("Healthy"), _) => stat_row(ui, "Risk", "WARN", Some(theme::warn())),
            (Some(_), _) => stat_row(ui, "Risk", "HIGH", Some(theme::critical())),
            (None, _) => stat_row_offline(ui, "Risk", "?"),
        }

        // camdash's REALLOC/PENDING are legacy ATA SMART attribute IDs
        // (5, 197). 7elwe's disk is NVMe -- those reallocated/pending
        // sector concepts don't exist in the NVMe health log at all, so
        // there is nothing to worry about: rendered "None" (fine), not a
        // scary "n/a", per the humanize pass -- "None" always means fine,
        // a number always means attention.
        stat_row(ui, "Realloc", "None", None);
        stat_row(ui, "Pending", "None", None);

        // UNCORR and TEMP have real NVMe equivalents, gated behind
        // Get-StorageReliabilityCounter (needs admin -- the reason this
        // app elevates at all).
        match &d.reliability {
            Some(r) => {
                let uncorr = r.read_errors_uncorrected + r.write_errors_uncorrected;
                // 0 -> "None" (fine); non-zero -> the actual count with
                // the R/W breakdown, in critical red. So "None" always
                // reads as clean and any number means attention.
                if uncorr > 0 {
                    stat_row(
                        ui,
                        "Uncorr",
                        &format!("{uncorr} (R:{} W:{})", r.read_errors_uncorrected, r.write_errors_uncorrected),
                        Some(theme::critical()),
                    );
                } else {
                    stat_row(ui, "Uncorr", "None", None);
                }
                match r.temperature_c {
                    // NVMe operating band: warn from 60, critical from 70
                    // (typical vendor throttle points). Was green
                    // unconditionally -- a temperature is a reading, not
                    // a success (review finding).
                    Some(t) => {
                        let color = if t >= 70.0 {
                            Some(theme::critical())
                        } else if t >= 60.0 {
                            Some(theme::warn())
                        } else {
                            None
                        };
                        stat_row(ui, "Temp", &format!("{t:.0}\u{b0}C"), color);
                    }
                    None => stat_row_offline(ui, "Temp", "n/a"),
                }
            }
            None => {
                stat_row_offline(ui, "Uncorr", "n/a \u{2014} query failed");
                stat_row_offline(ui, "Temp", "n/a \u{2014} query failed");
            }
        }
        // Live disk write throughput (item 6): camdash's WRITE MB/s line,
        // now wired to the Windows LogicalDisk perf counter
        // (DiskWriteBytesPersec, sampled with the 5s disk refresh). Perf
        // counters don't need admin, so this reads on any build.
        match d.write_mb_s {
            Some(mb) => stat_row(ui, "Write", &format!("{mb:.2} MB/s"), None),
            None => stat_row_offline(ui, "Write", "n/a"),
        }
    }

    // ------------------------------------------------------------- FEED
    //
    // Three distinct states, checked in order (run 8):
    //   1. TRANSITIONING -- a feed switch is flushing the pipeline: show the
    //      VHS static animation, NOT the error state. App::feed_transition
    //      (mod.rs) is the state machine; it ends exactly when the new
    //      source's first frame is decoded, so the handoff to live video has
    //      no gap.
    //   2. NO SIGNAL -- genuine failure/off (feed_offline): server stopped or
    //      the tap has produced no fresh frame for FRAME_STALE_AFTER. This is
    //      the real error state the transition coexists with, not replaces.
    //   3. Live video.
    // Transition wins over feed_offline: during a switch the frames ARE
    // stale, but that's expected cover, not failure.
    pub(super) fn draw_feed(&mut self, ui: &mut egui::Ui, p: &PipelineStatus) {
        // 1. Switching -> static "SWITCHING" (covers the pipeline flush).
        if self.feed_transition.is_some() {
            self.draw_switching(ui, "SWITCHING");
            return;
        }
        // 2. Server deliberately off -> static "STANDBY" holds the frame
        //    instead of a bare NO SIGNAL (operator request): the feed is
        //    off on purpose, not failed.
        if !p.enabled {
            self.draw_switching(ui, "STANDBY");
            return;
        }
        // 3. Genuine failure (server on, no fresh frame) -> the real error.
        if self.feed_offline(p) {
            components::placeholder(ui, "NO SIGNAL");
            return;
        }

        match &self.video_texture {
            Some(tex) => {
                let avail = ui.available_size();
                let aspect = tex.size()[0] as f32 / tex.size()[1] as f32;
                let mut size = avail;
                if size.x / size.y > aspect {
                    size.x = size.y * aspect;
                } else {
                    size.y = size.x / aspect;
                }
                ui.centered_and_justified(|ui| {
                    ui.add(egui::Image::new((tex.id(), size)));
                });
            }
            None => components::placeholder(ui, "CONNECTING"),
        }
    }

    /// Black-and-white analog VHS snow with scanlines and a slow rolling
    /// tracking band, plus a small mono `label` (e.g. "SWITCHING" during a
    /// feed switch, "STANDBY" when the server is off). B&W only (retro
    /// surveillance, not broken-TV).
    ///
    /// Cheap by construction: a small grey noise field (NOISE_W x NOISE_H)
    /// regenerated each frame from a persistent xorshift RNG (so it crawls)
    /// into ONE reused texture, scaled up NEAREST -- no full-res per-pixel
    /// loop. The dominant cost is the repaint itself (a full-window
    /// re-render), so it self-schedules at STATIC_FPS_INTERVAL (15fps) only
    /// while it's actually on screen, and stops the moment live video or
    /// NO SIGNAL takes over.
    fn draw_switching(&mut self, ui: &mut egui::Ui, label: &str) {
        // The repaint cadence (and the hidden-window gate that stops this
        // busy-looping at ~97% CPU when minimized) is centralized in
        // update() -- see there. Field size barely affects cost (the
        // repaint dominates, not the noise gen), so keep it fine enough for
        // crisp snow rather than blocky. 15fps is the real cost lever.
        const NOISE_W: usize = 224;
        const NOISE_H: usize = 126;
        self.noise_frame = self.noise_frame.wrapping_add(1);

        // A dark tracking band that scrolls slowly up the frame (classic VHS
        // roll). Position in noise rows, wrapping.
        let band_pos = (self.noise_frame / 2) % (NOISE_H as u64);

        let mut pixels = Vec::with_capacity(NOISE_W * NOISE_H);
        for y in 0..NOISE_H {
            // Scanlines: every other row dimmed. Rolling band: a few rows
            // near band_pos strongly dimmed.
            let scan = if y % 2 == 0 { 1.0 } else { 0.72 };
            let dist = (y as i64 - band_pos as i64).unsigned_abs();
            let band = if dist < 3 { 0.35 } else { 1.0 };
            let row_mul = scan * band;
            for _ in 0..NOISE_W {
                let n = (next_noise(&mut self.noise_rng) & 0xFF) as f32;
                pixels.push(egui::Color32::from_gray((n * row_mul) as u8));
            }
        }
        let image = egui::ColorImage {
            size: [NOISE_W, NOISE_H],
            pixels,
        };
        match &mut self.noise_texture {
            Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
            None => {
                self.noise_texture =
                    Some(ui.ctx().load_texture("vhs_noise", image, egui::TextureOptions::NEAREST))
            }
        }
        let rect = ui.max_rect();
        let painter = ui.painter();
        if let Some(tex) = &self.noise_texture {
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }

        // Label: a pulsing dot + `label` in mono, over a subtle dark plate
        // for legibility against the snow. Small, lower-center.
        let t = self.start.elapsed().as_secs_f32();
        let pulse = 0.55 + 0.45 * (t * 3.0).sin();
        let center = egui::pos2(rect.center().x, rect.bottom() - 34.0);
        let font = egui::FontId::monospace(13.0);
        let text = label;
        let galley = painter.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE);
        let dot_r = 4.0;
        let gap = 8.0;
        let content_w = dot_r * 2.0 + gap + galley.size().x;
        let plate = egui::Rect::from_center_size(
            center,
            egui::vec2(content_w + 28.0, galley.size().y + 14.0),
        );
        painter.rect_filled(plate, egui::CornerRadius::same(6), egui::Color32::from_black_alpha(150));
        let dot_c = egui::pos2(plate.left() + 14.0 + dot_r, center.y);
        painter.circle_filled(
            dot_c,
            dot_r,
            egui::Color32::from_white_alpha((pulse * 235.0) as u8),
        );
        painter.text(
            egui::pos2(dot_c.x + dot_r + gap, center.y),
            egui::Align2::LEFT_CENTER,
            text,
            font,
            egui::Color32::from_rgb(0xD0, 0xD0, 0xD0),
        );
    }

    /// The action toolbar attached under the feed (design doc §8, A1).
    /// Naming (§6b): button reads "Blur," API value stays "cloak," mono
    /// modifier reads "B&W." Run 6: Blur and B&W are live controls -- the
    /// cloak pipeline exists now (pipeline.rs), so the old disabled +
    /// PENDING-badge half-state is gone. Show/Blur/Hide are one
    /// mutually-exclusive feed-mode group; B&W is a modifier that only
    /// bites while Blur is active.
    pub(super) fn draw_feed_toolbar(&mut self, ui: &mut egui::Ui, _p: &PipelineStatus) {
        // Live-view control bar, the way NVR/camera UIs (Frigate, Blue
        // Iris, Viewtron) and this project's own web viewer lay it out:
        // the feed-mode buttons fill the left, and the momentary action
        // (Buzz -- the analog of a two-way-talk / siren press) is a
        // COMPACT button set apart on the right, past the divider. Not
        // equal-width-cramped next to B&W (operator: "too close"), not a
        // huge full-width button (operator: "huge"). Show/Blur/Hide
        // absorb the leftover width so the row fills exactly and Buzz
        // sits flush at the right edge on any window size.
        let feed_mode = self.state.feed_mode.lock().unwrap().clone();
        let showing = feed_mode == "show";
        let blurring = feed_mode == "cloak";
        let hiding = feed_mode == "hide";

        let spacing = ui.spacing().item_spacing.x;
        let bw_w = 62.0; // B&W checkbox natural width (box + "B&W" label)
        let buzz_w = 96.0; // compact Buzz, not full-width
        let div_pad = 12.0; // breathing room each side of the divider
        let sep_w = 8.0;
        // The 3 mode buttons absorb everything the fixed items don't use,
        // so the row always fills and Buzz stays flush right.
        let fixed = bw_w + div_pad * 2.0 + sep_w + buzz_w + spacing * 4.0;
        let btn_w = ((ui.available_width() - fixed) / 3.0).max(theme::BUTTON_MIN_W);

        if self.mode_button(ui, "Show", showing, btn_w).clicked() {
            self.request_feed_mode("show");
            self.set_status("Feed live");
        }
        if self.mode_button(ui, "Blur", blurring, btn_w).clicked() {
            self.request_feed_mode("cloak");
            self.set_status("Feed blurred");
        }
        if self.mode_button(ui, "Hide", hiding, btn_w).clicked() {
            self.request_feed_mode("hide");
            self.set_status("Feed hidden");
        }

        // B&W modifier. Enabled only while Blur is active -- it has no
        // effect on a plain or hidden feed, so a live-but-inert checkbox
        // would misrepresent it. Toggling drives the real filter via the
        // same path the /api/bw-mode handler uses (state flag +
        // pipeline.refresh_cloak).
        let mut bw = *self.state.bw_mode.lock().unwrap();
        let bw_resp = ui.add_enabled(blurring, egui::Checkbox::new(&mut bw, "B&W"));
        if !blurring {
            bw_resp.on_disabled_hover_text("B&W applies while Blur is active");
        } else if bw_resp.changed() {
            let state = self.state.clone();
            let pipeline = self.pipeline.clone();
            self.spawn_async(async move {
                state.toggle_bw_mode();
                pipeline.refresh_cloak().await;
            });
            self.set_status(if bw { "Feed blurred (B&W)" } else { "Feed blurred" });
        }

        // Divider (operator likes the visual break) with breathing room,
        // then the compact Buzz on the right.
        ui.add_space(div_pad);
        ui.separator();
        ui.add_space(div_pad);
        let buzz_text = egui::RichText::new("Buzz")
            .font(super::fonts::bold(theme::SIZE_BUTTON))
            .color(egui::Color32::WHITE);
        if filled_button(ui, buzz_text, theme::buzz(), theme::buzz_hover(), egui::vec2(buzz_w, theme::MIN_BUTTON_HEIGHT)).clicked() {
            let state = self.state.clone();
            self.spawn_async(async move {
                let _ = state.buzz_now();
            });
            self.set_status("Buzz sent");
        }
    }

    // ----------------------------------------------------------- SYSTEM
    pub(super) fn draw_system(&mut self, ui: &mut egui::Ui) {
        dense(ui);
        let s = &self.snapshot;
        let bar_w = ui.available_width();

        stat_row(ui, "CPU", &format!("{:.1}%", s.cpu_percent), exceptional(theme::value_color(s.cpu_percent)));
        draw_bar(ui, s.cpu_percent, bar_w, theme::meter_fill(s.cpu_percent));

        stat_row(ui, "Memory", &format!("{:.1}%", s.mem_percent), exceptional(theme::value_color(s.mem_percent)));
        draw_bar(ui, s.mem_percent, bar_w, theme::meter_fill(s.mem_percent));

        stat_row(
            ui,
            "Swap",
            &format!("{:.1}% [{}]", s.swap_percent, s.swap_label),
            exceptional(theme::value_color(s.swap_percent)),
        );
        draw_bar(ui, s.swap_percent, bar_w, theme::meter_fill(s.swap_percent));

        // PQL (Processor Queue Length): the honest Windows analog to
        // camdash's Unix LOAD -- threads waiting for a core, from the OS
        // perf counter, labeled as what it actually is. The bar is scaled
        // against cores*2 (a sustained queue past ~2 threads/core is the
        // usual "CPU-bound" line), and colored on the same band.
        match s.pql {
            Some(q) => {
                let pct = (q / (s.cores as f64 * 2.0) * 100.0).min(100.0) as f32;
                stat_row(ui, "PQL", &format!("{q:.0}"), exceptional(theme::value_color(pct)));
                draw_bar(ui, pct, bar_w, theme::meter_fill(pct));
            }
            None => {
                stat_row_offline(ui, "PQL", "n/a");
                draw_bar(ui, 0.0, bar_w, theme::offline());
            }
        }

        ui.add_space(2.0);
        stat_row(ui, "RAM free", &format!("{} MB", s.mem_avail_mb), None);
        // CPU temperature (item 7): the row is shown ONLY when a reading
        // is actually available, so there's no dead "n/a" line. On this
        // box it's always None: real CPU temp on Windows needs a
        // hardware-monitor kernel driver (LibreHardwareMonitor / WinRing0
        // class) that sysinfo can't read on its own. Deferred -- Ron is
        // not investing in shipping a signed kernel driver now. If such a
        // driver is ever present and sysinfo surfaces a value, this row
        // appears automatically.
        if let Some(t) = s.cpu_temp_c {
            stat_row(ui, "CPU temp", &format!("{t:.0}\u{b0}C"), None);
        }
    }

    // ------------------------------------------------------------ VIDEO
    //
    // Status-only -- controls live in the FEED toolbar (§6d). Repair
    // stays here: a secondary/rare action, next to the rows it diagnoses.
    pub(super) fn draw_video(&mut self, ui: &mut egui::Ui, p: &PipelineStatus) {
        dense(ui);
        let device_known = !p.device.is_empty();
        let cam_text = if device_known && p.capture_alive {
            "LIVE"
        } else if device_known {
            "FOUND"
        } else {
            "NONE"
        };
        stat_row(ui, "Camera", cam_text, exceptional(theme::status_color(if device_known { "LIVE" } else { "NONE" })));
        service_row(ui, "ffmpeg", p.capture_alive);
        service_row(ui, "RTSP", p.mediamtx_alive);
        service_row(ui, "mediamtx", p.mediamtx_alive);
        stat_row(ui, "HLS", &p.hls_state, exceptional(theme::status_color(&p.hls_state)));
        // No nginx on Windows -- run-1's axum server owns :80 in-process,
        // always up while this window exists (one process, one lifetime).
        stat_row(ui, "HTTP", "LIVE", None);

        let feed_mode = self.state.feed_mode.lock().unwrap().clone();
        let showing = feed_mode == "show";
        if showing {
            stat_row(ui, "FPS", "15", None);
        } else {
            stat_row_offline(ui, "FPS", "n/a");
        }

        ui.add_space(6.0);
        // Repair is a control, not a stat row -- restore the 32px control
        // minimum that dense() lowered for the rows above it.
        ui.spacing_mut().interact_size.y = theme::MIN_BUTTON_HEIGHT;
        if components::button(ui, "Repair").clicked() {
            let pipeline = self.pipeline.clone();
            self.spawn_async(async move {
                pipeline.manual_repair().await;
            });
            self.set_status("Repair triggered");
        }
    }

    fn mode_button(&self, ui: &mut egui::Ui, label: &str, selected: bool, width: f32) -> egui::Response {
        // The viewer's `.dark-btn.is-dark` toggle-on pattern: raised
        // fill, brighter border, when active. Width is passed in so the
        // feed toolbar can size all its buttons to fill the row.
        let btn = if selected {
            egui::Button::new(egui::RichText::new(label).color(theme::text()))
                .fill(theme::border_strong())
                .stroke(egui::Stroke::new(1.0_f32, theme::text_muted()))
        } else {
            egui::Button::new(egui::RichText::new(label).color(theme::text()))
        };
        ui.add(btn.min_size(egui::vec2(width, theme::MIN_BUTTON_HEIGHT)))
    }

    fn request_feed_mode(&self, mode: &str) {
        if !is_valid_mode(mode) {
            return;
        }
        self.state.set_feed_mode(mode);
        let pipeline = self.pipeline.clone();
        let mode = mode.to_string();
        self.spawn_async(async move {
            pipeline.apply_feed_mode(&mode).await;
        });
    }

    // -------------------------------------------------------- PROCESSES
    pub(super) fn draw_processes(&mut self, ui: &mut egui::Ui) {
        dense(ui);
        // Fill the box (run 6: PROCESSES owns the tall bottom-left slot):
        // draw as many rows as fit the available height rather than a
        // fixed count, so the panel neither clips (set_clip_rect guards
        // that too) nor floats a short list in an oversized empty box on
        // a big monitor. metrics collects up to 15; whatever's left after
        // that is simply below the fold.
        let row_h = theme::STAT_ROW_MIN_H + theme::STAT_ROW_GAP;
        // Name is a key (dim), the % is the value; color on the value
        // only, per thresholds -- not the whole row (review finding).
        for p in self.snapshot.top_processes.iter() {
            if ui.available_height() < row_h {
                break;
            }
            ui.horizontal(|ui| {
                ui.colored_label(theme::text_dim(), &p.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.colored_label(theme::proc_color(p.cpu_percent), format!("{:.1}%", p.cpu_percent));
                });
            });
        }
    }

    // ---------------------------------------------------------- MESSAGE
    pub(super) fn draw_message(&mut self, ui: &mut egui::Ui) {
        let locked = *self.state.msg_lock.lock().unwrap();
        let stored = self.state.message.lock().unwrap().clone();

        // Hint on the left, live character count on the right (the web
        // spec's `.char-count`; the 120 cap was invisible until typing
        // silently stopped -- review finding). Warn color near the cap.
        ui.horizontal(|ui| {
            ui.colored_label(theme::text_dim(), "Leave a note for viewers");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let count = self.edit_buffer.chars().count();
                let color = if count >= 110 { theme::warn() } else { theme::text_muted() };
                ui.label(egui::RichText::new(format!("{count}/120")).size(theme::SIZE_SMALL).color(color));
            });
        });

        // `editing_msg` is an EXPLICIT mode flag, not derived from live
        // egui focus state -- see run-4 notes on why a focus-derived
        // guard raced its own Save button. Set on any real edit, cleared
        // only by Save/Cancel.
        if !self.editing_msg && !locked {
            self.edit_buffer = stored.clone();
        }

        let response = ui.add_enabled_ui(!locked, |ui| {
            let edit = egui::TextEdit::multiline(&mut self.edit_buffer)
                .desired_rows(4)
                .char_limit(120)
                .hint_text("(no message)");
            ui.add(edit)
        });
        if response.inner.changed() || response.inner.gained_focus() {
            self.editing_msg = true;
        }

        // All three are plain `.btn`s -- in the ground truth the web
        // viewer's own Save is not `.primary` (accent is reserved for
        // Reconnect there), and the previous grey->accent morph on dirty
        // state was an invented treatment (review finding). Dirty state
        // is carried by enablement, same as the web.
        ui.horizontal(|ui| {
            let changed = self.edit_buffer != stored;
            if components::button_enabled(ui, !locked && changed, "Save").clicked() {
                let state = self.state.clone();
                let msg = self.edit_buffer.clone();
                self.spawn_async(async move {
                    let _ = state.set_message(&msg);
                });
                self.editing_msg = false;
                self.set_status("Message saved");
            }
            if components::button_enabled(ui, !locked && !stored.is_empty(), "Clear").clicked() {
                self.edit_buffer.clear();
                let state = self.state.clone();
                self.spawn_async(async move {
                    let _ = state.set_message("");
                });
                self.editing_msg = false;
                self.set_status("Message cleared");
            }
            if components::button_enabled(ui, !locked && changed, "Cancel").clicked() {
                self.edit_buffer = stored.clone();
                self.editing_msg = false;
            }
        });

        // Lock checkbox and the API fact share one row -- the API note
        // is a plain muted value, not a green banner (it's true whenever
        // the window exists; permanent green is noise, review finding).
        ui.horizontal(|ui| {
            let mut lock_val = locked;
            if ui.checkbox(&mut lock_val, "Lock message").changed() {
                let state = self.state.clone();
                self.spawn_async(async move {
                    state.toggle_msg_lock();
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(theme::text_muted(), "API up");
            });
        });
    }

    // -------------------------------------------------------------- NODE
    //
    // Run 6.1: the node's identity + reachability at a glance -- the
    // IP/host text that used to sit in the footer, promoted to its own
    // panel so the footer can carry transient status instead (operator
    // request). Titled "Node" (not "Network"): it's this box's identity,
    // not a network-config panel. Fleet-facing ports are the fixed
    // contract values (:80 control, :8888 HLS), not the possibly-
    // overridden local bind.
    pub(super) fn draw_node(&mut self, ui: &mut egui::Ui, p: &PipelineStatus) {
        dense(ui);
        let na = |s: &str| if s.is_empty() { "n/a".to_string() } else { s.to_string() };
        stat_row(ui, "Tailscale", &na(&self.tailscale), None);
        stat_row(ui, "Local IP", &na(&self.local_ip), None);
        stat_row(ui, "Hostname", &na(&self.hostname), None);
        stat_row(ui, "HTTP", ":80", None);
        stat_row(ui, "HLS", ":8888/cam", None);
        if p.enabled {
            stat_row(ui, "Server", "running", Some(theme::healthy()));
        } else {
            stat_row(ui, "Server", "stopped", Some(theme::text_muted()));
        }
    }
}

/// Dense-list spacing for the status panels: the stat-row gap AND a
/// lowered row minimum -- `horizontal()` treats `interact_size.y` as the
/// minimum row height, so at the 32px control default every stat row
/// ballooned to 32px no matter what the item gap said (caught from a
/// screenshot). Scoped to the panel's Ui; MESSAGE and the toolbar keep
/// the 32px control metrics.
fn dense(ui: &mut egui::Ui) {
    let s = ui.spacing_mut();
    s.item_spacing.y = theme::STAT_ROW_GAP;
    s.interact_size.y = theme::STAT_ROW_MIN_H;
}

/// Tiny xorshift64 PRNG for the VHS-static snow -- no `rand` dependency
/// for a few thousand grey values a frame. The caller threads the state
/// across frames so successive frames get fresh noise (the snow crawls).
fn next_noise(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// One VIDEO service row: plain "up", critical "DOWN".
fn service_row(ui: &mut egui::Ui, key: &str, alive: bool) {
    if alive {
        stat_row(ui, key, "up", None);
    } else {
        stat_row(ui, key, "DOWN", Some(theme::critical()));
    }
}

/// Collapses "plain text" to None so stat_row's healthy-default applies;
/// keeps call sites reading as "color only if exceptional."
fn exceptional(c: egui::Color32) -> Option<egui::Color32> {
    (c != theme::text()).then_some(c)
}

fn draw_bar(ui: &mut egui::Ui, percent: f32, width: f32, color: egui::Color32) {
    let height = 6.0;
    let (rect, _response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, theme::panel_2());
    let mut fill = rect;
    fill.set_width(rect.width() * (percent.clamp(0.0, 100.0) / 100.0));
    ui.painter().rect_filled(fill, 3.0, color);
    ui.add_space(2.0);
}
