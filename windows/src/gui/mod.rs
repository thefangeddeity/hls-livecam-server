//! The native operator window -- egui/eframe, no webview anywhere.
//!
//! Six panels + header + footer, transcribed from the camdash reference
//! screenshot and camdash's own source (color rules in `theme.rs`, content
//! in the draw_* functions below). This is a *client* of the same
//! AppState/Pipeline the HTTP server (routes.rs) already drives -- run 3
//! merges GUI and server into one process (PM direction: "we don't need to
//! decouple them... it can be one process"), so button clicks call the
//! same state/pipeline methods the HTTP handlers call, directly, in-
//! process. No loopback HTTP calls to itself.

mod theme;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::diskhealth::{self, DiskInfo};
use crate::metrics::{Metrics, Snapshot};
use crate::pipeline::{self, Pipeline};
use crate::routes;
use crate::state::AppState;
use crate::tray::{self, Tray, TrayAction};
use crate::video_preview::SharedFrame;

const METRICS_REFRESH: Duration = Duration::from_millis(1000);
const DISK_REFRESH: Duration = Duration::from_secs(5);
const REPAINT_INTERVAL: Duration = Duration::from_millis(150);

pub struct App {
    state: Arc<AppState>,
    pipeline: Arc<Pipeline>,
    rt: tokio::runtime::Handle,

    metrics: Metrics,
    snapshot: Snapshot,
    last_metrics: Instant,

    disk: DiskInfo,
    last_disk: Instant,

    video_frame: SharedFrame,
    video_texture: Option<egui::TextureHandle>,
    video_last_gen: u64,

    hostname: String,
    tailscale: String,

    editing_msg: bool,
    edit_buffer: String,

    start: Instant,
    tray: Option<Tray>,
}

/// Bumped once at process start so a fresh build always shows a nonzero
/// uptime immediately rather than waiting a tick -- purely cosmetic.
static BOOT: AtomicU64 = AtomicU64::new(0);

impl App {
    pub fn new(
        state: Arc<AppState>,
        pipeline: Arc<Pipeline>,
        rt: tokio::runtime::Handle,
        video_frame: SharedFrame,
        tray: Option<Tray>,
    ) -> Self {
        BOOT.store(1, Ordering::Relaxed);
        let mut metrics = Metrics::new();
        let snapshot = metrics.refresh();
        Self {
            state,
            pipeline,
            rt,
            metrics,
            snapshot,
            last_metrics: Instant::now(),
            disk: diskhealth::query(),
            last_disk: Instant::now(),
            video_frame,
            video_texture: None,
            video_last_gen: 0,
            hostname: routes::hostname(),
            tailscale: routes::tailscale_ip(),
            editing_msg: false,
            edit_buffer: String::new(),
            start: Instant::now(),
            tray,
        }
    }

    fn spawn_async<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.rt.spawn(fut);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_metrics.elapsed() >= METRICS_REFRESH {
            self.snapshot = self.metrics.refresh();
            self.last_metrics = Instant::now();
        }
        if self.last_disk.elapsed() >= DISK_REFRESH {
            self.disk = diskhealth::query();
            self.last_disk = Instant::now();
        }
        self.update_video_texture(ctx);

        if let Some(tray) = &self.tray {
            match tray::poll(tray) {
                Some(TrayAction::Show) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Some(TrayAction::Quit) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                None => {}
            }
        }

        apply_console_theme(ctx);

        let pstatus = self.pipeline.status();

        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(theme::HEADER_BG)
                    .inner_margin(egui::Margin::symmetric(10, 7))
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER)),
            )
            .show(ctx, |ui| self.draw_header(ui, &pstatus));

        egui::TopBottomPanel::bottom("footer")
            .frame(
                egui::Frame::new()
                    .fill(theme::HEADER_BG)
                    .inner_margin(egui::Margin::symmetric(10, 5))
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER)),
            )
            .show(ctx, |ui| self.draw_footer(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::WINDOW_BG).inner_margin(egui::Margin::same(10)))
            .show(ctx, |ui| {
                self.draw_panel_grid(ui, &pstatus);
            });

        ctx.request_repaint_after(REPAINT_INTERVAL);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // See pipeline.rs::shutdown docs -- Windows won't do this for us.
        self.rt.block_on(self.pipeline.shutdown());
    }
}

/// Professional NVR-console chrome for widget defaults (buttons, checkboxes,
/// text fields) -- see theme.rs's console-chrome section for the palette.
/// Status colors (green/yellow/red/DIM) are untouched; this only reaches
/// widget backgrounds/strokes egui itself draws, never the status text.
fn apply_console_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.window_fill = theme::WINDOW_BG;
    v.panel_fill = theme::WINDOW_BG;
    v.extreme_bg_color = theme::PANEL_BG;
    v.faint_bg_color = theme::PANEL_BG;
    v.override_text_color = Some(theme::HEADER_TEXT);

    v.widgets.noninteractive.bg_fill = theme::PANEL_BG;
    v.widgets.noninteractive.weak_bg_fill = theme::PANEL_BG;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, theme::BORDER);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, theme::NEUTRAL_TEXT);

    v.widgets.inactive.bg_fill = theme::HEADER_BG;
    v.widgets.inactive.weak_bg_fill = theme::HEADER_BG;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, theme::BORDER);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, theme::HEADER_TEXT);

    v.widgets.hovered.bg_fill = theme::BEVEL_LIGHT;
    v.widgets.hovered.weak_bg_fill = theme::BEVEL_LIGHT;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, theme::BORDER);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, theme::HEADER_TEXT);

    v.widgets.active.bg_fill = theme::BORDER;
    v.widgets.active.weak_bg_fill = theme::BORDER;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, theme::HEADER_TEXT);

    v.selection.bg_fill = theme::GREEN.gamma_multiply(0.4);
    v.selection.stroke = egui::Stroke::new(1.0_f32, theme::GREEN_BOLD);

    ctx.set_style(style);
}

/// Panel header strip height -- a real title bar per panel, not a label
/// floating in the content area.
const HEADER_STRIP_H: f32 = 26.0;
/// Grid gutter between panels and the outer safety margin around the
/// whole grid. Deliberately looser than run 3's 6px -- PM instruction to
/// leave headroom for future controls, not pack to the pixel.
const GRID_GUTTER: f32 = 10.0;
const PANEL_CONTENT_PAD: f32 = 10.0;

impl App {
    /// Explicit rect-based 3x2 grid, not `ui.horizontal()` automatic
    /// sizing. Run 3's right-column panels had no visible right border --
    /// automatic horizontal-layout width allocation doesn't reserve exact
    /// space the way a stroke drawn at the allocated rect's edge needs;
    /// computing exact rects up front (with a safety-shrunk outer bound)
    /// guarantees every panel's border, including the rightmost column's,
    /// is fully inside the visible area.
    fn draw_panel_grid(&mut self, ui: &mut egui::Ui, pstatus: &pipeline::PipelineStatus) {
        let grid_rect = ui.available_rect_before_wrap().shrink(2.0);
        let cell_w = (grid_rect.width() - GRID_GUTTER * 2.0) / 3.0;
        let cell_h = (grid_rect.height() - GRID_GUTTER) / 2.0;

        let cell_rect = |col: usize, row: usize| {
            let x = grid_rect.left() + col as f32 * (cell_w + GRID_GUTTER);
            let y = grid_rect.top() + row as f32 * (cell_h + GRID_GUTTER);
            egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, cell_h))
        };

        self.panel_at(ui, cell_rect(0, 0), "DISK / SMART", |ui, s| s.draw_disk_smart(ui));
        self.panel_at(ui, cell_rect(1, 0), "FEED", |ui, s| s.draw_feed(ui));
        self.panel_at(ui, cell_rect(2, 0), "SYSTEM", |ui, s| s.draw_system(ui));
        self.panel_at(ui, cell_rect(0, 1), "VIDEO", |ui, s| s.draw_video(ui, pstatus));
        self.panel_at(ui, cell_rect(1, 1), "PROCESSES", |ui, s| s.draw_processes(ui));
        self.panel_at(ui, cell_rect(2, 1), "MESSAGE", |ui, s| s.draw_message(ui));
    }

    /// A framed, bordered, beveled panel at an exact rect -- console
    /// chrome, not ASCII-box transcription. Content area sized with
    /// `PANEL_CONTENT_PAD` slack on every side (headroom for future
    /// controls per the brief, not packed to the edge).
    fn panel_at(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        title: &str,
        add_contents: impl FnOnce(&mut egui::Ui, &mut Self),
    ) {
        let painter = ui.painter();
        let rounding = egui::CornerRadius::same(3);

        // Base panel fill (the "recessed" content surface).
        painter.rect_filled(rect, rounding, theme::PANEL_BG);

        // Raised header strip.
        let header_rect =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), HEADER_STRIP_H));
        painter.rect_filled(header_rect, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, theme::HEADER_BG);

        // Bevel: light line along the top+left (raised edge), dark line
        // along the bottom+right (shadow edge) -- egui::Frame can't do a
        // two-tone stroke in one call, so this is drawn directly.
        painter.hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0_f32, theme::BEVEL_LIGHT));
        painter.vline(rect.left(), rect.y_range(), egui::Stroke::new(1.0_f32, theme::BEVEL_LIGHT));
        painter.hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0_f32, theme::BEVEL_DARK));
        painter.vline(rect.right(), rect.y_range(), egui::Stroke::new(1.0_f32, theme::BEVEL_DARK));

        // The crisp, defined, full four-sided border every panel has --
        // drawn last so it sits cleanly over the bevel lines' corners.
        painter.rect_stroke(rect, rounding, egui::Stroke::new(1.0_f32, theme::BORDER), egui::StrokeKind::Inside);
        // Separator between header strip and content.
        painter.hline(
            rect.x_range(),
            header_rect.bottom(),
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );

        // Title -- small-caps-style console label: uppercase, muted, not
        // the vivid status green (that vocabulary is reserved for actual
        // status content per the brief).
        painter.text(
            header_rect.left_center() + egui::vec2(10.0, 0.0),
            egui::Align2::LEFT_CENTER,
            title.to_uppercase(),
            egui::FontId::new(12.5, egui::FontFamily::Proportional),
            theme::HEADER_TEXT,
        );

        let content_rect = egui::Rect::from_min_max(
            header_rect.left_bottom() + egui::vec2(PANEL_CONTENT_PAD, PANEL_CONTENT_PAD * 0.6),
            rect.right_bottom() - egui::vec2(PANEL_CONTENT_PAD, PANEL_CONTENT_PAD),
        );
        let mut content_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        add_contents(&mut content_ui, self);
    }

    fn update_video_texture(&mut self, ctx: &egui::Context) {
        let frame = self.video_frame.lock().unwrap();
        if let Some(f) = frame.as_ref() {
            if f.generation != self.video_last_gen {
                let image = egui::ColorImage::from_rgb([f.width, f.height], &f.rgb);
                match &mut self.video_texture {
                    Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                    None => {
                        self.video_texture = Some(ctx.load_texture(
                            "feed",
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
                self.video_last_gen = f.generation;
            }
        }
    }
}

mod header;
mod panels;
