//! Shared components (design doc §5): the status pill, the pending badge,
//! the `.stat` key/value row, and colored action buttons. One definition
//! each -- "single indicator vocabulary" (§7 item 2), not a separate look
//! per panel.

use eframe::egui;

use super::fonts;
use super::theme;

/// `.stat`: dim key on the left, plain-text value right-aligned, with
/// color applied to the VALUE only, and only for exceptional states --
/// pass None for the healthy/normal case and it renders `--text` white.
/// This replaces two patterns the review flagged as the app's biggest
/// visual defects: whole rows painted green for ordinary healthy states
/// (green stops meaning anything when it's everywhere), and columns
/// "aligned" with format!-padded spaces, which cannot line up in a
/// proportional font -- right-alignment does the aligning here, and as a
/// bonus hides most live-value digit jitter (no tabular figures in egui).
pub fn stat_row(ui: &mut egui::Ui, key: &str, value: &str, value_color: Option<egui::Color32>) {
    ui.horizontal(|ui| {
        ui.colored_label(theme::text_dim(), key);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(value).color(value_color.unwrap_or(theme::text())),
                )
                .truncate(),
            );
        });
    });
}

/// A `.stat` row that is entirely muted -- for facts that are absent /
/// not applicable (REALLOC on NVMe, WRITE not wired) rather than healthy.
pub fn stat_row_offline(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.colored_label(theme::text_muted(), key);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(value).color(theme::text_muted())).truncate(),
            );
        });
    });
}

/// A standard action button at the uniform minimum size (theme::
/// button_min_size). Use this instead of `ui.button(..)` for every
/// operator-facing button so widths are consistent across the app.
pub fn button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(theme::button_min_size()))
}

/// Enable-gated variant of `button`.
pub fn button_enabled(
    ui: &mut egui::Ui,
    enabled: bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(text).min_size(theme::button_min_size()))
}

/// A filled action button with real hover/pressed feedback, at the same
/// uniform minimum size. Plain `Button::fill()` pins one fill across
/// every interact state, so the colored buttons (Buzz, accent Close)
/// gave zero hover response while stock buttons next to them did (review
/// finding) -- this routes the fill through the widget-visuals machinery
/// instead so egui's own hover/press handling drives it, using the
/// spec's hover tokens.
pub fn filled_button(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    fill: egui::Color32,
    hover_fill: egui::Color32,
    min_size: egui::Vec2,
) -> egui::Response {
    ui.scope(|ui| {
        let w = &mut ui.style_mut().visuals.widgets;
        w.inactive.weak_bg_fill = fill;
        w.inactive.bg_fill = fill;
        w.inactive.bg_stroke = egui::Stroke::new(1.0_f32, fill);
        w.hovered.weak_bg_fill = hover_fill;
        w.hovered.bg_fill = hover_fill;
        w.hovered.bg_stroke = egui::Stroke::new(1.0_f32, hover_fill);
        w.active.weak_bg_fill = hover_fill;
        w.active.bg_fill = hover_fill;
        w.active.bg_stroke = egui::Stroke::new(1.0_f32, hover_fill);
        ui.add(egui::Button::new(text).min_size(min_size))
    })
    .inner
}

/// `.live-pill`: rounded pill, dot + label, colored per state. Doc: 999px
/// radius, `panel-2` fill, a border blended toward the state color, a
/// 600-weight label colored to match, a 6px dot before it.
pub fn status_pill(ui: &mut egui::Ui, label: &str, color: egui::Color32) {
    let dot_r = 3.0;
    let pad_h = 10.0;
    let pad_v = 4.0;
    let gap = 6.0;

    let font = fonts::semibold(11.0);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), color);
    let content_w = dot_r * 2.0 + gap + galley.size().x;
    let content_h = galley.size().y.max(dot_r * 2.0);
    let size = egui::vec2(content_w + pad_h * 2.0, content_h + pad_v * 2.0);

    let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    let border = mix(theme::border(), color, 0.3);
    let bg = mix(theme::panel_2(), color, 0.08);
    painter.rect(
        rect,
        egui::CornerRadius::same(255), // fully rounded (999px equivalent)
        bg,
        egui::Stroke::new(1.0_f32, border),
        egui::StrokeKind::Inside,
    );

    let dot_center = rect.left_center() + egui::vec2(pad_h + dot_r, 0.0);
    painter.circle_filled(dot_center, dot_r, color);

    let text_pos = dot_center + egui::vec2(dot_r + gap, -galley.size().y / 2.0);
    painter.galley(text_pos, galley, color);
}

/// The exact rendered width of `status_pill` for `label` -- so the header
/// can place the pill in a rect that width, centered on the true window
/// center (sequential-thirds drift left the pill slightly off-center;
/// operator wanted it dead-centre). Mirrors the size math above:
/// dot(6) + gap(6) + text + horizontal padding(20).
pub fn status_pill_width(ui: &egui::Ui, label: &str) -> f32 {
    let galley =
        ui.fonts(|f| f.layout_no_wrap(label.to_string(), fonts::semibold(11.0), theme::text()));
    galley.size().x + 32.0
}

/// `.info-chip .badge`: small muted chip (600 weight per spec). Was used
/// to mark Blur/B&W as PENDING; run 6 made those live, so it's currently
/// unused, but kept as the ratified badge component for the next deferred
/// affordance rather than deleted and re-derived.
#[allow(dead_code)]
pub fn pending_badge(ui: &mut egui::Ui) {
    let text = "PENDING";
    let font = fonts::semibold(10.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, theme::text_muted());
    let pad_h = 6.0;
    let pad_v = 2.0;
    let size = galley.size() + egui::vec2(pad_h * 2.0, pad_v * 2.0);
    let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(4),
        theme::panel(),
        egui::Stroke::new(1.0_f32, theme::border()),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.left_top() + egui::vec2(pad_h, pad_v), galley, theme::text_muted());
}

/// The web placeholder pattern (`.ph` / `.ph-icon`): centered dim icon
/// over a small tracked-caps label. Used for both FEED placeholder states
/// (NO SIGNAL, CONNECTING) so they read as one vocabulary -- previously
/// "connecting…" was a bare lowercase label in a different style from NO
/// SIGNAL's icon treatment (review finding). Letter-spacing ~0.04em like
/// the web's placeholder label, not the 2px the first attempt used.
pub fn placeholder(ui: &mut egui::Ui, label: &str) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("\u{25CE}").size(32.0).color(theme::text_muted()));
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(label)
                    .size(11.0)
                    .color(theme::text_muted())
                    .extra_letter_spacing(0.5),
            );
        });
    });
}

/// Approximates CSS `color-mix(in srgb, color P%, base)` -- linear
/// per-channel blend. Close enough for a border/background tint; this
/// isn't going through a perceptual color space the way real color-mix
/// does, but the visual difference at these small percentages (8-30%) is
/// not meaningful.
fn mix(base: egui::Color32, tint: egui::Color32, t: f32) -> egui::Color32 {
    let lerp = |a: u8, b: u8| -> u8 { (a as f32 + (b as f32 - a as f32) * t).round() as u8 };
    egui::Color32::from_rgb(lerp(base.r(), tint.r()), lerp(base.g(), tint.g()), lerp(base.b(), tint.b()))
}
