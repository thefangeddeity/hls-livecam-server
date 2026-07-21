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
        // (5, 197). 7elwe's disk is NVMe -- those attributes don't exist
        // here regardless of permissions. Muted unconditionally.
        stat_row_offline(ui, "Realloc", "n/a (NVMe)");
        stat_row_offline(ui, "Pending", "n/a (NVMe)");

        // UNCORR and TEMP have real NVMe equivalents, gated behind
        // Get-StorageReliabilityCounter (needs admin -- the reason this
        // app elevates at all).
        match &d.reliability {
            Some(r) => {
                let uncorr = r.read_errors_uncorrected + r.write_errors_uncorrected;
                stat_row(
                    ui,
                    "Uncorr",
                    &format!("{uncorr} (R:{} W:{})", r.read_errors_uncorrected, r.write_errors_uncorrected),
                    (uncorr > 0).then_some(theme::critical()),
                );
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
        stat_row_offline(ui, "Write", "n/a (not wired)");
    }

    // ------------------------------------------------------------- FEED
    //
    // Stale-frame fix (design doc §2/§7 item 1): don't paint the last
    // decoded texture once the feed is OFFLINE -- App::feed_offline()
    // decides. Both placeholder states use the shared `.ph` pattern.
    pub(super) fn draw_feed(&mut self, ui: &mut egui::Ui, p: &PipelineStatus) {
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

    /// The action toolbar attached under the feed (design doc §8, A1).
    /// Naming (§6b): button reads "Blur," API value stays "cloak," mono
    /// modifier reads "B&W." Run 6: Blur and B&W are live controls -- the
    /// cloak pipeline exists now (pipeline.rs), so the old disabled +
    /// PENDING-badge half-state is gone. Show/Blur/Hide are one
    /// mutually-exclusive feed-mode group; B&W is a modifier that only
    /// bites while Blur is active.
    pub(super) fn draw_feed_toolbar(&mut self, ui: &mut egui::Ui, p: &PipelineStatus) {
        let feed_mode = self.state.feed_mode.lock().unwrap().clone();
        let showing = feed_mode == "show";
        let blurring = feed_mode == "cloak";
        let hiding = feed_mode == "hide";

        if self.mode_button(ui, "Show", showing).clicked() {
            self.request_feed_mode("show");
        }
        if self.mode_button(ui, "Blur", blurring).clicked() {
            self.request_feed_mode("cloak");
        }
        if self.mode_button(ui, "Hide", hiding).clicked() {
            self.request_feed_mode("hide");
        }

        ui.add_space(10.0);
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
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(14.0);

        // `.buzz-btn`: its own red, #fff text, 700 weight -- with real
        // hover feedback (`.buzz-btn:hover #e0352b`).
        let buzz_text = egui::RichText::new("Buzz")
            .font(super::fonts::bold(theme::SIZE_BUTTON))
            .color(egui::Color32::WHITE);
        if filled_button(ui, buzz_text, theme::buzz(), theme::buzz_hover()).clicked() {
            let state = self.state.clone();
            self.spawn_async(async move {
                let _ = state.buzz_now();
            });
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(14.0);

        // A standard `.btn` verb, not a green/red filled status-object --
        // the previous SERVER: ON/OFF button invented a filled style that
        // exists nowhere in the design system and conflated state (green
        // = running) with action (click = stop). State lives in the
        // header's status pill; this is just the verb (review finding).
        let on = p.enabled;
        let label = if on { "Stop server" } else { "Start server" };
        if ui.button(label).clicked() {
            let pipeline = self.pipeline.clone();
            self.spawn_async(async move {
                pipeline.set_enabled(!on).await;
            });
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

        // camdash's LOAD is a Unix load average -- sysinfo synthesizes an
        // approximation on Windows rather than reading a kernel value.
        // Showing that as "Load" would misrepresent what it is; muted per
        // the brief's "don't fake a number" instruction.
        match s.load {
            Some((load, cores)) => {
                let pct = (load / cores as f64 * 100.0).min(100.0) as f32;
                stat_row(ui, "Load", &format!("{load:.2}/{cores}"), exceptional(theme::value_color(pct)));
                draw_bar(ui, pct, bar_w, theme::meter_fill(pct));
            }
            None => {
                stat_row_offline(ui, "Load", "n/a");
                draw_bar(ui, 0.0, bar_w, theme::offline());
            }
        }

        ui.add_space(2.0);
        stat_row(ui, "RAM free", &format!("{} MB", s.mem_avail_mb), None);
        match s.cpu_temp_c {
            Some(t) => stat_row(ui, "CPU temp", &format!("{t:.0}\u{b0}C"), None),
            None => stat_row_offline(ui, "CPU temp", "n/a"),
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
        if ui.button("Repair").clicked() {
            let pipeline = self.pipeline.clone();
            self.spawn_async(async move {
                pipeline.manual_repair().await;
            });
        }
    }

    fn mode_button(&self, ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
        // The viewer's `.dark-btn.is-dark` toggle-on pattern: raised
        // fill, brighter border, when active.
        let btn = if selected {
            egui::Button::new(egui::RichText::new(label).color(theme::text()))
                .fill(theme::border_strong())
                .stroke(egui::Stroke::new(1.0_f32, theme::text_muted()))
        } else {
            egui::Button::new(egui::RichText::new(label).color(theme::text()))
        };
        ui.add(btn)
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
            if ui.add_enabled(!locked && changed, egui::Button::new("Save")).clicked() {
                let state = self.state.clone();
                let msg = self.edit_buffer.clone();
                self.spawn_async(async move {
                    let _ = state.set_message(&msg);
                });
                self.editing_msg = false;
            }
            if ui.add_enabled(!locked && !stored.is_empty(), egui::Button::new("Clear")).clicked() {
                self.edit_buffer.clear();
                let state = self.state.clone();
                self.spawn_async(async move {
                    let _ = state.set_message("");
                });
                self.editing_msg = false;
            }
            if ui.add_enabled(!locked && changed, egui::Button::new("Cancel")).clicked() {
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
        // Headroom: the panel is sized with slack below this point (right
        // column gets the largest share of the three, see layout.rs) so
        // more message/broadcast controls can land here later without
        // the panel needing to grow or its neighbors needing to move.
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
