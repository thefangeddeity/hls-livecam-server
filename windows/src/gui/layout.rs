//! Feed-hero layout (design doc §8, ratified) -- run-6 rearrangement:
//!
//!   left column  = VIDEO status (top, fixed)  + PROCESSES (tall, bottom)
//!   center       = FEED + attached toolbar (top, dominant) + SYSTEM (bottom)
//!   right column = DISK/SMART (top, fixed)    + MESSAGE (bottom, absorbs)
//!
//! Rationale (Ron): PROCESSES was cramped in its old short right-column
//! slot while DISK/SMART was oversized for its fixed 8 rows, so they
//! swap roles -- PROCESSES gets the long vertical box (and fills it, see
//! draw_processes) and DISK/SMART becomes a fixed top-right panel. SYSTEM
//! moves directly under the feed it relates to. The feed keeps the visual
//! weight: it's the top of the center column with SYSTEM a fixed strip
//! beneath it.
//!
//! Panel heights are CONTENT-driven, not window-percentage-driven: fixed
//! panels are sized from the type ramp's row rhythm; the one absorber per
//! column (PROCESSES left, MESSAGE right) takes the remainder. This
//! avoids both the oversized-empty-box and the amputated-row failure
//! modes an even percentage split produced (review finding).

use eframe::egui;

use super::fonts;
use super::theme;
use super::App;
use crate::pipeline::PipelineStatus;

/// Left/right margin column widths. Left sized so DISK's longest fallback
/// value fits without clipping (240 didn't -- review finding); right is
/// wider still: MESSAGE needs room for a real textarea + button row.
const LEFT_COL_W: f32 = 260.0;
const RIGHT_COL_W: f32 = 320.0;
const GUTTER: f32 = 16.0; // design doc §4: 16px between major regions
const TOOLBAR_H: f32 = 56.0;
const HEADER_STRIP_H: f32 = 28.0;
const PANEL_PAD: f32 = 14.0; // design doc §4: panel padding 14px

/// One `.stat` row at the Fluent ramp: ~20px of 14px text + the 4px
/// stat-row gap.
const STAT_ROW_H: f32 = 24.0;
/// Per-panel chrome: header strip + 4px offset + bottom pad.
const CHROME_H: f32 = HEADER_STRIP_H + 4.0 + PANEL_PAD;
/// Fixed MESSAGE panel height: hint/count row + 4-row textarea + button
/// row + lock row + gaps + chrome. Keeps the textarea a sane size instead
/// of stretching to fill the column.
const MESSAGE_H: f32 = 250.0;
/// Never let the feed column collapse below this even on a narrow
/// window -- the side columns are fixed-width, so without a floor the
/// center rect inverts.
const MIN_FEED_W: f32 = 400.0;

impl App {
    pub(super) fn draw_layout(&mut self, ui: &mut egui::Ui, pstatus: &PipelineStatus) {
        let full = ui.available_rect_before_wrap();

        let left = egui::Rect::from_min_size(full.min, egui::vec2(LEFT_COL_W, full.height()));
        let right = egui::Rect::from_min_size(
            egui::pos2(full.right() - RIGHT_COL_W, full.top()),
            egui::vec2(RIGHT_COL_W, full.height()),
        );
        let center = egui::Rect::from_min_max(
            egui::pos2(left.right() + GUTTER, full.top()),
            egui::pos2(
                (right.left() - GUTTER).max(left.right() + GUTTER + MIN_FEED_W),
                full.bottom(),
            ),
        );

        // ---- left column: VIDEO (fixed) over PROCESSES (tall, fills) ----
        // VIDEO: 7 stat rows + 6px space + 32px Repair button.
        let video_h = 7.0 * STAT_ROW_H + 6.0 + 32.0 + CHROME_H;
        let left_video = egui::Rect::from_min_size(left.min, egui::vec2(LEFT_COL_W, video_h));
        let left_proc = egui::Rect::from_min_max(
            egui::pos2(left.left(), left_video.bottom() + GUTTER),
            left.max,
        );
        self.panel_at(ui, left_video, "Video", |ui, s| s.draw_video(ui, pstatus));
        self.panel_at(ui, left_proc, "Processes", |ui, s| s.draw_processes(ui));

        // ---- center: FEED (dominant, with toolbar) over SYSTEM (fixed) --
        // SYSTEM: 4 label+bar pairs (row + 6px bar + 2px space + 4px gap)
        // + 2px space + 2 info rows.
        let sys_h = 4.0 * (STAT_ROW_H + 12.0) + 2.0 + 2.0 * STAT_ROW_H + CHROME_H;
        let center_feed = egui::Rect::from_min_max(
            center.min,
            egui::pos2(center.right(), center.bottom() - sys_h - GUTTER),
        );
        let center_sys = egui::Rect::from_min_size(
            egui::pos2(center.left(), center_feed.bottom() + GUTTER),
            egui::vec2(center.width(), sys_h),
        );
        self.panel_at(ui, center_feed, "Feed", |ui, s| s.draw_feed_with_toolbar(ui, pstatus));
        self.panel_at(ui, center_sys, "System", |ui, s| s.draw_system(ui));

        // ---- right column: DISK/SMART (fixed), MESSAGE (fixed), NODE
        // (absorbs) ----
        // DISK/SMART: 8 stat rows. MESSAGE: fixed modest height. NODE
        // takes the remainder -- reference info that reads fine with a
        // little air, so it's the right absorber here.
        let disk_h = 8.0 * STAT_ROW_H + CHROME_H;
        let right_disk = egui::Rect::from_min_size(right.min, egui::vec2(RIGHT_COL_W, disk_h));
        let right_msg = egui::Rect::from_min_size(
            egui::pos2(right.left(), right_disk.bottom() + GUTTER),
            egui::vec2(RIGHT_COL_W, MESSAGE_H),
        );
        let right_node = egui::Rect::from_min_max(
            egui::pos2(right.left(), right_msg.bottom() + GUTTER),
            right.max,
        );
        self.panel_at(ui, right_disk, "Disk / SMART", |ui, s| s.draw_disk_smart(ui));
        self.panel_at(ui, right_msg, "Message", |ui, s| s.draw_message(ui));
        self.panel_at(ui, right_node, "Node", |ui, s| s.draw_node(ui, pstatus));
    }

    /// A framed, bordered panel at an exact rect with a raised header
    /// strip. Section-title convention (design doc §3): 11px, 600 weight,
    /// uppercase, text-dim, letter-spacing 0.06em -- rendered with the
    /// real semibold face and real letter-spacing (0.06em of 11px =
    /// 0.66px) via LayoutJob, replacing the earlier thin-space-insertion
    /// hack that tracked ~3x too loose and corrupted the string (review
    /// finding). Panel radius is `--radius` 12 like the web `.section`;
    /// RADIUS_SM stays for buttons/inputs only.
    pub(super) fn panel_at(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        title: &str,
        add_contents: impl FnOnce(&mut egui::Ui, &mut Self),
    ) {
        let rounding = egui::CornerRadius::same(theme::RADIUS);

        ui.painter().rect_filled(rect, rounding, theme::panel());
        ui.painter().rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(1.0_f32, theme::border()),
            egui::StrokeKind::Inside,
        );

        let mut job = egui::text::LayoutJob::default();
        job.append(
            &title.to_uppercase(),
            0.0,
            egui::TextFormat {
                font_id: fonts::semibold(theme::SECTION_TITLE_SIZE),
                color: theme::section_title_color(),
                extra_letter_spacing: theme::SECTION_TITLE_SIZE * 0.06,
                ..Default::default()
            },
        );
        let galley = ui.fonts(|f| f.layout_job(job));
        let header_rect =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), HEADER_STRIP_H));
        let title_pos = egui::pos2(
            header_rect.left() + PANEL_PAD,
            header_rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(title_pos, galley, theme::section_title_color());

        let content_rect = egui::Rect::from_min_max(
            header_rect.left_bottom() + egui::vec2(PANEL_PAD, 4.0),
            rect.right_bottom() - egui::vec2(PANEL_PAD, PANEL_PAD),
        );
        let mut content_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        // `max_rect` is a sizing hint, not a clip -- without a clip, a
        // panel with more rows than its height allows paints straight
        // through its own border (caught from a screenshot). But clipping
        // exactly at content_rect cropped the LEFT border of a focused
        // field: an input's focus stroke paints centred on the widget
        // edge, so ~1px falls just outside content_rect and got cut
        // (MESSAGE field, operator photo -- only three sides drew). Give
        // the clip a few px of horizontal + top slack (still well inside
        // the panel's 14px padding, so nothing bleeds past the border) so
        // an edge stroke draws whole; keep the BOTTOM flush so over-tall
        // row lists are still cut at the panel border, which is the whole
        // reason this clip exists.
        let clip = egui::Rect::from_min_max(
            egui::pos2(content_rect.left() - 4.0, content_rect.top() - 4.0),
            egui::pos2(content_rect.right() + 4.0, content_rect.bottom()),
        );
        content_ui.set_clip_rect(clip);
        add_contents(&mut content_ui, self);
    }

    /// FEED content: the video area plus its attached toolbar, drawn as
    /// one continuous surface inside the FEED panel's content rect (not
    /// two separate boxes) -- "a horizontal strip attached to the video."
    pub(super) fn draw_feed_with_toolbar(&mut self, ui: &mut egui::Ui, p: &PipelineStatus) {
        let full = ui.available_rect_before_wrap();
        let toolbar_rect = egui::Rect::from_min_max(
            egui::pos2(full.left(), full.bottom() - TOOLBAR_H),
            full.max,
        );
        let video_rect = egui::Rect::from_min_max(
            full.min,
            egui::pos2(full.right(), toolbar_rect.top() - 8.0),
        );

        // Separator between video area and its toolbar.
        ui.painter().hline(
            full.x_range(),
            toolbar_rect.top() - 4.0,
            egui::Stroke::new(1.0_f32, theme::border()),
        );

        let mut video_ui = ui.new_child(egui::UiBuilder::new().max_rect(video_rect));
        self.draw_feed(&mut video_ui, p);

        // Left-aligned (fills from the left edge): the toolbar sizes its
        // buttons to the panel width so the row spans the whole box, so
        // there's nothing to center -- centering would fight the fill and
        // reintroduce dead edges.
        let mut toolbar_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(toolbar_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        self.draw_feed_toolbar(&mut toolbar_ui, p);
    }
}
