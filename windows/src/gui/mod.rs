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
use crate::pipeline::Pipeline;
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

        apply_dark_theme(ctx);

        let pstatus = self.pipeline.status();

        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(8, 6)))
            .show(ctx, |ui| self.draw_header(ui, &pstatus));

        egui::TopBottomPanel::bottom("footer")
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(8, 4)))
            .show(ctx, |ui| self.draw_footer(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(6)))
            .show(ctx, |ui| {
                let avail = ui.available_size();
                let spacing = 6.0;
                let row_h = (avail.y - spacing) / 2.0;
                let col_w = (avail.x - spacing * 2.0) / 3.0;
                let cell = egui::vec2(col_w, row_h);

                ui.spacing_mut().item_spacing = egui::vec2(spacing, spacing);
                ui.horizontal(|ui| {
                    self.panel(ui, "DISK / SMART", cell, |ui, s| s.draw_disk_smart(ui));
                    self.panel(ui, "FEED", cell, |ui, s| s.draw_feed(ui));
                    self.panel(ui, "SYSTEM", cell, |ui, s| s.draw_system(ui));
                });
                ui.horizontal(|ui| {
                    self.panel(ui, "VIDEO", cell, |ui, s| s.draw_video(ui, &pstatus));
                    self.panel(ui, "PROCESSES", cell, |ui, s| s.draw_processes(ui));
                    self.panel(ui, "MESSAGE", cell, |ui, s| s.draw_message(ui));
                });
            });

        ctx.request_repaint_after(REPAINT_INTERVAL);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // See pipeline.rs::shutdown docs -- Windows won't do this for us.
        self.rt.block_on(self.pipeline.shutdown());
    }
}

fn apply_dark_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.window_fill = theme::BG;
    style.visuals.panel_fill = theme::BG;
    style.visuals.extreme_bg_color = theme::BG;
    style.visuals.override_text_color = Some(theme::GREEN_BOLD);
    ctx.set_style(style);
}

impl App {
    /// A bordered, titled box -- the native stand-in for camdash's
    /// box-drawing-character panel borders. Not literal ASCII art; a
    /// titled frame reads as "the same cockpit" without pretending to be
    /// a terminal.
    fn panel(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        size: egui::Vec2,
        add_contents: impl FnOnce(&mut egui::Ui, &mut Self),
    ) {
        let border = egui::Stroke::new(1.0_f32, theme::DIM);
        egui::Frame::new()
            .stroke(border)
            .fill(theme::BG)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);
                ui.vertical(|ui| {
                    ui.colored_label(theme::DIM, egui::RichText::new(title).strong());
                    ui.add_space(4.0);
                    add_contents(ui, self);
                });
            });
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
