//! The six panel bodies. Content/coloring transcribed from camdash's
//! `draw_dashboard()` -- see theme.rs for the exact threshold functions
//! this calls, each annotated with the camdash function it mirrors.

use eframe::egui;

use super::theme;
use super::App;
use crate::pipeline::PipelineStatus;
use crate::state::is_valid_mode;

impl App {
    // ------------------------------------------------------- DISK / SMART
    pub(super) fn draw_disk_smart(&mut self, ui: &mut egui::Ui) {
        let d = &self.disk;
        ui.colored_label(theme::GREEN, format!("DISK: {}", d.disk_name));

        let (assess_text, assess_color) = match d.health_status.as_deref() {
            Some("Healthy") => ("PASSED".to_string(), theme::GREEN_BOLD),
            Some(other) => (other.to_uppercase(), theme::RED_BOLD),
            None => ("?".to_string(), theme::WHITE),
        };
        ui.colored_label(assess_color, format!("ASSESS: {assess_text}"));

        let (risk_text, risk_color) = match (d.health_status.as_deref(), d.operational_status.as_deref()) {
            (Some("Healthy"), Some("OK")) => ("OK", theme::GREEN_BOLD),
            (Some("Healthy"), _) => ("WARN", theme::YELLOW_BOLD),
            (Some(_), _) => ("HIGH", theme::RED_BOLD),
            (None, _) => ("?", theme::WHITE),
        };
        ui.colored_label(risk_color, format!("RISK: {risk_text}"));

        // camdash's REALLOC/PENDING are legacy ATA SMART attribute IDs
        // (5, 197). 7elwe's disk is NVMe -- those specific attributes
        // don't exist here regardless of permissions; this isn't a
        // permissions gap, NVMe uses an entirely different SMART/Health
        // log with no reallocated-sector or pending-sector concept at
        // all. Stay dimmed unconditionally.
        ui.colored_label(theme::DIM, "REALLOC: n/a (NVMe)");
        ui.colored_label(theme::DIM, "PENDING: n/a (NVMe)");

        // UNCORR and TEMP *do* have real NVMe equivalents, gated behind
        // Get-StorageReliabilityCounter, which needs admin (verified live:
        // CIM access denied on a standard token). This app now runs
        // elevated (build.rs manifest, PM decision) specifically to
        // unlock these two -- shown for real once available, still
        // honestly dimmed if the elevated query ever fails for some
        // other reason (driver quirk, etc.) rather than assumed to work
        // just because the process is admin.
        match &d.reliability {
            Some(r) => {
                let uncorr = r.read_errors_uncorrected + r.write_errors_uncorrected;
                ui.colored_label(
                    theme::smart_field_color(Some(uncorr), false),
                    format!("UNCORR: {uncorr} (R:{} W:{})", r.read_errors_uncorrected, r.write_errors_uncorrected),
                );
                match r.temperature_c {
                    Some(t) => ui.colored_label(theme::GREEN, format!("TEMP: {t:.0}\u{b0}C")),
                    None => ui.colored_label(theme::DIM, "TEMP: n/a"),
                };
            }
            None => {
                ui.colored_label(theme::DIM, "UNCORR: n/a (elevated query failed)");
                ui.colored_label(theme::DIM, "TEMP: n/a (elevated query failed)");
            }
        }
        ui.colored_label(theme::DIM, "WRITE: n/a (not wired)");
    }

    // ------------------------------------------------------------- FEED
    pub(super) fn draw_feed(&mut self, ui: &mut egui::Ui) {
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
            None => {
                ui.centered_and_justified(|ui| {
                    ui.colored_label(theme::DIM, "connecting\u{2026}");
                });
            }
        }
    }

    // ----------------------------------------------------------- SYSTEM
    pub(super) fn draw_system(&mut self, ui: &mut egui::Ui) {
        let s = &self.snapshot;
        let bar_w = ui.available_width();

        ui.colored_label(theme::GREEN, format!("CPU   {:5.1}%", s.cpu_percent));
        draw_bar(ui, s.cpu_percent, bar_w, theme::led(s.cpu_percent, false));

        ui.colored_label(theme::GREEN, format!("MEM   {:5.1}%", s.mem_percent));
        draw_bar(ui, s.mem_percent, bar_w, theme::led(s.mem_percent, false));

        ui.colored_label(theme::GREEN, format!("SWAP  {:5.1}% [{}]", s.swap_percent, s.swap_label));
        draw_bar(ui, s.swap_percent, bar_w, theme::led(s.swap_percent, false));

        // camdash's LOAD is a Unix load average -- sysinfo synthesizes an
        // approximation on Windows rather than reading a kernel value.
        // Showing that as "LOAD" would misrepresent what it is; dimmed
        // per the brief's "don't fake a number" instruction.
        match s.load {
            Some((load, cores)) => {
                ui.colored_label(theme::GREEN, format!("LOAD  {load:.2}/{cores}"));
                let pct = (load / cores as f64 * 100.0).min(100.0) as f32;
                draw_bar(ui, pct, bar_w, theme::load_color(load, cores, false));
            }
            None => {
                ui.colored_label(theme::DIM, "LOAD  n/a (no Windows equivalent)");
                draw_bar(ui, 0.0, bar_w, theme::DIM);
            }
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.colored_label(theme::GREEN, format!("RAM: {} MB", s.mem_avail_mb));
            match s.cpu_temp_c {
                Some(t) => ui.colored_label(theme::GREEN, format!("CPU TEMP: {t:.0}\u{b0}C")),
                None => ui.colored_label(theme::DIM, "CPU TEMP: n/a"),
            };
        });
    }

    // ------------------------------------------------------------ VIDEO
    pub(super) fn draw_video(&mut self, ui: &mut egui::Ui, p: &PipelineStatus) {
        let device_known = !p.device.is_empty();
        let cam_text = if device_known && p.capture_alive {
            "LIVE"
        } else if device_known {
            "FOUND"
        } else {
            "NONE"
        };
        // camdash colors this row on device-presence, independent of the
        // LIVE/FOUND/NONE text -- status_attr('LIVE' if v4l2 else 'DOWN').
        ui.colored_label(
            theme::status_color(if device_known { "LIVE" } else { "DOWN" }, false),
            format!("CAM:      {cam_text}"),
        );
        row(ui, "ffmpeg:", p.capture_alive);
        row(ui, "RTSP:", p.mediamtx_alive);
        row(ui, "mediamtx:", p.mediamtx_alive);
        ui.colored_label(
            theme::status_color(&p.hls_state, false),
            format!("HLS:      {}", p.hls_state),
        );
        // No nginx on Windows -- run-1's axum server owns :80 in-process,
        // so this row reports that instead. Relabeled ("http:" not
        // "nginx:") rather than keeping a label that would misname what's
        // actually running; same row position/semantics otherwise. Always
        // LIVE while this window is open: GUI and HTTP server share one
        // process/lifetime by design (see main.rs), so there's no state
        // where the window renders but the server died separately.
        ui.colored_label(theme::GREEN_BOLD, "http:     LIVE");

        ui.add_space(6.0);
        let feed_mode = self.state.feed_mode.lock().unwrap().clone();
        let showing = feed_mode == "show";
        let hiding = feed_mode == "hide" || feed_mode == "cloak";

        ui.horizontal(|ui| {
            if self.mode_button(ui, "Show", showing).clicked() {
                self.request_feed_mode("show");
            }
            if self.mode_button(ui, "Hide", hiding).clicked() {
                self.request_feed_mode("hide");
            }
        });
        ui.horizontal(|ui| {
            // Blur/B&W: present, visibly disabled -- PM decision, the
            // pixelation pipeline is deferred (v1.1), not implemented
            // here. Shown so the layout matches the reference without
            // advertising a capability this node doesn't have.
            ui.add_enabled(false, egui::Button::new("Blur"));
            ui.add_enabled(false, egui::Checkbox::new(&mut false, "B&W"));
        });
        if ui.button("Repair").clicked() {
            let pipeline = self.pipeline.clone();
            self.spawn_async(async move {
                pipeline.manual_repair().await;
            });
        }

        ui.add_space(6.0);
        ui.colored_label(theme::WHITE_BOLD, format!("FPS:    {}", if showing { "15" } else { "n/a" }));

        // SERVER: ON/OFF -- a real control now (camdash's [o] on/off),
        // not just a passive readout of whether mediamtx happened to be
        // alive at poll time. Reflects `enabled` (operator intent), not
        // `mediamtx_alive` (liveness) -- otherwise a legitimate crash-
        // restart cycle would flash "OFF" for a moment while `enabled`
        // was still true and mediamtx was just respawning.
        let on = p.enabled;
        let label = format!("SERVER: {}", if on { "ON" } else { "OFF" });
        let btn = egui::Button::new(egui::RichText::new(label).color(theme::BG))
            .fill(if on { theme::GREEN_BOLD } else { theme::RED_BOLD });
        if ui.add(btn).clicked() {
            let pipeline = self.pipeline.clone();
            self.spawn_async(async move {
                pipeline.set_enabled(!on).await;
            });
        }
    }

    fn mode_button(&self, ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
        // camdash: curses.A_REVERSE when selected -- reverse video reads
        // natively as a solid-fill highlighted button.
        let btn = if selected {
            egui::Button::new(egui::RichText::new(label).color(theme::BG))
                .fill(theme::GREEN_BOLD)
        } else {
            egui::Button::new(egui::RichText::new(label).color(theme::DIM))
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
        for p in &self.snapshot.top_processes {
            ui.horizontal(|ui| {
                let color = theme::proc_color(p.cpu_percent, false);
                ui.colored_label(color, &p.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.colored_label(color, format!("{:5.1}%", p.cpu_percent));
                });
            });
        }
    }

    // ---------------------------------------------------------- MESSAGE
    pub(super) fn draw_message(&mut self, ui: &mut egui::Ui) {
        ui.colored_label(theme::NEUTRAL_TEXT, "Leave a note for viewers");

        let locked = *self.state.msg_lock.lock().unwrap();
        let stored = self.state.message.lock().unwrap().clone();

        // `editing_msg` is an EXPLICIT mode flag, not derived from live
        // egui focus state. A prior version synced `edit_buffer` from
        // `stored` whenever the TextEdit lacked focus() -- which sounds
        // right, but clicking Save necessarily blurs the TextEdit in that
        // *same* frame (any click outside a focused widget blurs it), so
        // "did I just lose focus" and "did I just click Save" collapse
        // into the same event, and a focus-driven guard can race its own
        // save button. Confirmed broken against real clicks, not just
        // reasoning about it: typed text reverted to the stored value
        // instead of saving. An explicit mode -- entered on any real
        // edit, cleared only by Save or Cancel -- has no such race,
        // and is closer to what camdash itself does (an explicit edit
        // mode toggled by a key, not derived from terminal focus, which
        // doesn't really exist for a TUI anyway).
        if !self.editing_msg && !locked {
            self.edit_buffer = stored.clone();
        }

        let response = ui.add_enabled_ui(!locked, |ui| {
            let edit = egui::TextEdit::multiline(&mut self.edit_buffer)
                .desired_rows(3)
                .char_limit(120)
                .hint_text("(no message)");
            ui.add(edit)
        });
        if response.inner.changed() || response.inner.gained_focus() {
            self.editing_msg = true;
        }

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

        ui.add_space(6.0);
        let mut lock_val = locked;
        if ui.checkbox(&mut lock_val, "Lock message").changed() {
            let state = self.state.clone();
            self.spawn_async(async move {
                state.toggle_msg_lock();
            });
        }
        // Same-process design (see module docs): the HTTP server and this
        // window share one lifetime, so "is the message API up" is always
        // true while the window is open.
        ui.colored_label(theme::GREEN_BOLD, "MESSAGE API: UP");
        // Headroom: the panel is sized with slack below this point (see
        // PANEL_CONTENT_PAD / the grid's cell sizing in mod.rs) so more
        // message/broadcast controls can land here later without the
        // panel needing to grow or its neighbors needing to move.
    }
}

fn row(ui: &mut egui::Ui, label: &str, alive: bool) {
    let status = if alive { "LIVE" } else { "DOWN" };
    ui.colored_label(theme::status_color(status, false), format!("{label:<10}{status}"));
}

fn draw_bar(ui: &mut egui::Ui, percent: f32, width: f32, color: egui::Color32) {
    let height = 8.0;
    let (rect, _response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::DIM.gamma_multiply(0.15));
    let mut fill = rect;
    fill.set_width(rect.width() * (percent.clamp(0.0, 100.0) / 100.0));
    ui.painter().rect_filled(fill, 0.0, color);
    ui.add_space(2.0);
}
