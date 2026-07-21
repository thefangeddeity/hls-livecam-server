//! The IP-manager modal (run-6 brief item 3) -- surfaces camdash's
//! `[i] cam IPs` as a professional dialog in the ratified theme, backed
//! by this node's `cams.json` (see cams.rs). Lists entries, adds/edits/
//! removes them with real input boxes and button controls (no
//! keybindings), validates IP format, treats port as optional, and
//! writes the file on every mutation so `/cams/cams.json` reflects edits
//! immediately.

use eframe::egui;

use super::components::{filled_button, stat_row_offline};
use super::{fonts, theme, App};

impl App {
    pub(super) fn draw_ip_manager(&mut self, ctx: &egui::Context) {
        // egui::Modal gives the dimmed backdrop + escape/click-out close
        // for free. Framed in the ratified panel style so it reads as the
        // same design system as the dashboard behind it.
        let modal = egui::Modal::new(egui::Id::new("ip_manager"))
            .backdrop_color(egui::Color32::from_black_alpha(160))
            .frame(
                egui::Frame::new()
                    .fill(theme::panel())
                    .stroke(egui::Stroke::new(1.0_f32, theme::border()))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .inner_margin(egui::Margin::same(18)),
            )
            .show(ctx, |ui| {
                ui.set_width(440.0);
                self.ip_manager_body(ui);
            });

        // Backdrop click / Escape, or the Close button (which sets the
        // flag false directly), dismisses. Clear the form on close so the
        // next open starts fresh.
        if modal.should_close() {
            self.ip_manager_open = false;
            self.ip_form.clear();
        }
    }

    fn ip_manager_body(&mut self, ui: &mut egui::Ui) {
        // Title row.
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Fleet cameras")
                    .font(fonts::semibold(theme::SIZE_HEADING))
                    .color(theme::text()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} entries", self.cams.cams.len()))
                        .color(theme::text_muted()),
                );
            });
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("cams.json \u{2014} this node's roster served at /cams/cams.json")
                .size(theme::SIZE_SMALL)
                .color(theme::text_muted()),
        );

        if self.cams.parse_failed {
            ui.add_space(6.0);
            ui.colored_label(
                theme::warn(),
                "\u{26a0}  Existing cams.json didn't parse as a JSON array; saving will replace it.",
            );
        }

        ui.add_space(12.0);
        self.ip_manager_list(ui);
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(12.0);
        self.ip_manager_form(ui);
    }

    /// The current entries, each selectable (loads it into the form) with
    /// a per-row Delete.
    fn ip_manager_list(&mut self, ui: &mut egui::Ui) {
        if self.cams.cams.is_empty() {
            stat_row_offline(ui, "No cameras", "add one below");
            return;
        }

        // Column header.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("NAME").size(theme::SIZE_SMALL).color(theme::text_muted()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(64.0); // reserve for the Delete button column
                ui.label(egui::RichText::new("IP  :  PORT").size(theme::SIZE_SMALL).color(theme::text_muted()));
            });
        });
        ui.add_space(2.0);

        let mut to_delete: Option<usize> = None;
        let mut to_select: Option<usize> = None;
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (i, cam) in self.cams.cams.iter().enumerate() {
                    let selected = self.ip_form.selected == Some(i);
                    ui.horizontal(|ui| {
                        let name = if cam.label.is_empty() { "(unnamed)" } else { cam.label.as_str() };
                        // Clicking the name selects the row for editing.
                        let label = egui::RichText::new(name)
                            .color(if selected { theme::accent() } else { theme::text() });
                        if ui.selectable_label(selected, label).clicked() {
                            to_select = Some(i);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Delete").clicked() {
                                to_delete = Some(i);
                            }
                            let port = cam.port.map(|p| p.to_string()).unwrap_or_else(|| "\u{2014}".into());
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(format!("{}  :  {}", cam.ip, port))
                                    .color(theme::text_dim()),
                            );
                        });
                    });
                }
            });

        if let Some(i) = to_select {
            let cam = &self.cams.cams[i];
            self.ip_form.name = cam.label.clone();
            self.ip_form.ip = cam.ip.clone();
            self.ip_form.port = cam.port.map(|p| p.to_string()).unwrap_or_default();
            self.ip_form.selected = Some(i);
            self.ip_form.error = None;
            self.ip_form.note = None;
        }
        if let Some(i) = to_delete {
            self.cams.remove(i);
            self.persist("Deleted.");
            // If the deleted row (or a later one) was selected, drop the
            // selection so Save doesn't target a shifted/absent index.
            self.ip_form.clear();
        }
    }

    /// The add/edit fields + Add / Save / Delete / Close controls.
    fn ip_manager_form(&mut self, ui: &mut egui::Ui) {
        let editing = self.ip_form.selected.is_some();
        ui.label(
            egui::RichText::new(if editing { "Edit camera" } else { "Add camera" })
                .font(fonts::semibold(theme::SIZE_BODY))
                .color(theme::text()),
        );
        ui.add_space(8.0);

        field_row(ui, "Name", &mut self.ip_form.name, "Front door");
        ui.add_space(6.0);
        field_row(ui, "IP", &mut self.ip_form.ip, "100.100.17.1");
        ui.add_space(6.0);
        field_row(ui, "Port", &mut self.ip_form.port, "optional");

        // Validation / save feedback line (fixed slot so buttons don't
        // jump as it appears).
        ui.add_space(8.0);
        match (&self.ip_form.error, &self.ip_form.note) {
            (Some(e), _) => ui.colored_label(theme::critical(), e),
            (None, Some(n)) => ui.colored_label(theme::healthy(), n),
            (None, None) => ui.colored_label(theme::text_muted(), "IP required \u{2022} port optional"),
        };

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            // Add: always available -- creates a new entry from the fields.
            if ui.button("Add").clicked() {
                self.commit_form(false);
            }
            // Save: only when a row is selected -- updates it in place.
            if ui.add_enabled(editing, egui::Button::new("Save")).clicked() {
                self.commit_form(true);
            }
            // Delete: only when a row is selected.
            if ui.add_enabled(editing, egui::Button::new("Delete")).clicked() {
                if let Some(i) = self.ip_form.selected {
                    self.cams.remove(i);
                    self.persist("Deleted.");
                    self.ip_form.clear();
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Accent Close so the dismiss action is the visually
                // dominant control, matching `.btn.primary`.
                if filled_button(ui, egui::RichText::new("Close").color(egui::Color32::WHITE), theme::accent(), theme::accent_hover())
                    .clicked()
                {
                    self.ip_manager_open = false;
                    self.ip_form.clear();
                }
                if editing && ui.button("New").clicked() {
                    // Drop the selection to switch from edit-mode back to
                    // add-mode without closing.
                    self.ip_form.clear();
                }
            });
        });
    }

    /// Validate the form and either add a new entry or update the selected
    /// one, then persist. `save` true = update selected; false = add new.
    fn commit_form(&mut self, save: bool) {
        let name = self.ip_form.name.trim().to_string();
        let ip = self.ip_form.ip.trim().to_string();

        if !crate::cams::valid_ip(&ip) {
            self.ip_form.note = None;
            self.ip_form.error = Some(if ip.is_empty() {
                "IP is required.".into()
            } else {
                format!("\u{201c}{ip}\u{201d} is not a valid IP address.")
            });
            return;
        }
        let port = match crate::cams::parse_port(&self.ip_form.port) {
            Ok(p) => p,
            Err(()) => {
                self.ip_form.note = None;
                self.ip_form.error = Some("Port must be a number 1\u{2013}65535, or blank.".into());
                return;
            }
        };

        if save {
            if let Some(i) = self.ip_form.selected {
                self.cams.update(i, name, ip, port);
                self.persist("Saved.");
            }
        } else {
            self.cams.add(name, ip, port);
            // Select the just-added row so the user can immediately tweak
            // it, and so a follow-up Save targets it rather than adding a
            // duplicate.
            self.ip_form.selected = Some(self.cams.cams.len() - 1);
            self.persist("Added.");
        }
        self.ip_form.error = None;
    }

    /// Write cams.json and record the outcome in the form's note line.
    fn persist(&mut self, ok_note: &str) {
        match self.cams.save() {
            Ok(()) => {
                self.ip_form.error = None;
                self.ip_form.note = Some(ok_note.to_string());
            }
            Err(e) => {
                self.ip_form.note = None;
                self.ip_form.error = Some(format!("Could not write cams.json: {e}"));
            }
        }
    }
}

/// One labelled input row: a fixed-width dim key, then a full-width text
/// box in the component style.
fn field_row(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [56.0, theme::MIN_BUTTON_HEIGHT],
            egui::Label::new(egui::RichText::new(label).color(theme::text_dim())),
        );
        ui.add(
            egui::TextEdit::singleline(value)
                .hint_text(hint)
                .desired_width(f32::INFINITY),
        );
    });
}
