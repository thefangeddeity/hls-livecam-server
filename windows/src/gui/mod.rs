//! The native operator window -- egui/eframe, no webview anywhere.
//!
//! Run 5: feed-hero layout + Apple-HIG palette per
//! `hls-livecam-design-system.md` (ratified) -- replacing run 4's even
//! six-panel console grid. The feed dominates the center with its action
//! toolbar attached directly below it; the five status/data panels flank
//! it in two margin columns. This is a *client* of the same AppState/
//! Pipeline the HTTP server (routes.rs) already drives -- one process, no
//! loopback HTTP calls to itself (run-3 PM direction, unchanged).

mod components;
mod fonts;
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
/// Display-only staleness fallback for the FEED panel -- if the pipeline
/// itself reports healthy but video_preview's own independent decode tap
/// has stopped producing frames (a failure the pipeline status can't see,
/// since it's a separate read-only process), don't keep painting the last
/// frame. Short on purpose: this only gates what's drawn, not any restart
/// action, so being reactive costs nothing. Distinct from run-2's 8s
/// pipeline-restart threshold, which gates an actual restart and is
/// sized off mediamtx's segment timing -- this is a different decision
/// with different stakes.
const FRAME_STALE_AFTER: Duration = Duration::from_secs(3);

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
    last_frame_at: Instant,

    hostname: String,
    tailscale: String,

    editing_msg: bool,
    edit_buffer: String,

    // IP manager (run 6): the node's cam roster + the modal's form state.
    // Loaded once at startup; the modal edits this copy and writes
    // cams.json on each mutation (which /cams/cams.json then serves).
    ip_manager_open: bool,
    cams: crate::cams::CamStore,
    ip_form: IpForm,

    start: Instant,
    tray: Option<Tray>,
}

/// The IP-manager modal's transient form state -- the three edit fields,
/// which existing row (if any) is selected for editing, and the last
/// validation/save note to show under the fields.
#[derive(Default)]
pub(crate) struct IpForm {
    pub name: String,
    pub ip: String,
    pub port: String,
    pub selected: Option<usize>,
    pub error: Option<String>,
    pub note: Option<String>,
}

impl IpForm {
    fn clear(&mut self) {
        *self = IpForm::default();
    }
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
            last_frame_at: Instant::now(),
            hostname: routes::hostname(),
            tailscale: routes::tailscale_ip(),
            editing_msg: false,
            edit_buffer: String::new(),
            ip_manager_open: false,
            cams: crate::cams::CamStore::load(),
            ip_form: IpForm::default(),
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

    /// The stale-frame fix's decision function. True whenever the FEED
    /// should show NO SIGNAL instead of the last decoded texture.
    /// Primary signal: the pipeline-down state that already drives the
    /// VIDEO panel's rows red/critical (server off, capture or mediamtx
    /// not alive, or HLS not reporting LIVE) -- reusing that existing
    /// signal rather than inventing a new one, per the brief. Secondary,
    /// belt-and-suspenders signal: video_preview's own independent RTSP
    /// tap can die without the main pipeline noticing at all (it's a
    /// separate read-only process) -- frame-staleness catches that case
    /// specifically.
    fn feed_offline(&self, p: &pipeline::PipelineStatus) -> bool {
        if !p.enabled || !p.capture_alive || !p.mediamtx_alive || p.hls_state != "LIVE" {
            return true;
        }
        self.last_frame_at.elapsed() > FRAME_STALE_AFTER
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
                self.last_frame_at = Instant::now();
            }
        }
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

        apply_theme(ctx);

        let pstatus = self.pipeline.status();

        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(theme::panel())
                    .inner_margin(egui::Margin::symmetric(14, 9))
                    .stroke(egui::Stroke::new(1.0_f32, theme::border())),
            )
            .show(ctx, |ui| self.draw_header(ui, &pstatus));

        egui::TopBottomPanel::bottom("footer")
            .frame(
                egui::Frame::new()
                    .fill(theme::panel())
                    .inner_margin(egui::Margin::symmetric(14, 6))
                    .stroke(egui::Stroke::new(1.0_f32, theme::border())),
            )
            .show(ctx, |ui| self.draw_footer(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::bg()).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                self.draw_layout(ui, &pstatus);
            });

        // Modal is drawn last so its backdrop overlays everything above.
        if self.ip_manager_open {
            self.draw_ip_manager(ctx);
        }

        ctx.request_repaint_after(REPAINT_INTERVAL);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // See pipeline.rs::shutdown docs -- Windows won't do this for us.
        self.rt.block_on(self.pipeline.shutdown());
    }
}

/// Called from main.rs's creation closure, BEFORE the first frame --
/// set_fonts() only takes effect at the next frame boundary, and egui
/// panics if layout references a named family (semibold/bold) that
/// isn't bound yet. Installing during update() is one frame too late.
/// Also restores the persisted light/dark choice so the first frame
/// already renders in the operator's theme.
pub fn init_fonts(ctx: &egui::Context) {
    theme::load_persisted();
    fonts::install(ctx);
}

/// Apple-HIG theme per the design doc, light or dark per the operator's
/// toggle, applied to egui's widget defaults (buttons, checkboxes, text
/// fields). Runs every frame, which is what makes the footer's theme
/// switch take effect live -- the palette functions read the mode flag.
fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    *v = if theme::is_light() { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.window_fill = theme::bg();
    v.panel_fill = theme::bg();
    v.extreme_bg_color = theme::panel_2();
    v.faint_bg_color = theme::panel();
    v.override_text_color = Some(theme::text());

    // `.btn`: panel-2 fill, border-strong border.
    v.widgets.inactive.bg_fill = theme::panel_2();
    v.widgets.inactive.weak_bg_fill = theme::panel_2();
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, theme::border_strong());
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, theme::text());
    v.widgets.inactive.corner_radius = egui::CornerRadius::same(theme::RADIUS_SM);

    // `.btn:hover`: bg fill, text-muted border.
    v.widgets.hovered.bg_fill = theme::bg();
    v.widgets.hovered.weak_bg_fill = theme::bg();
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, theme::text_muted());
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, theme::text());
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(theme::RADIUS_SM);

    v.widgets.active.bg_fill = theme::border_strong();
    v.widgets.active.weak_bg_fill = theme::border_strong();
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, theme::text());
    v.widgets.active.corner_radius = egui::CornerRadius::same(theme::RADIUS_SM);

    v.widgets.noninteractive.bg_fill = theme::panel();
    v.widgets.noninteractive.weak_bg_fill = theme::panel();
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, theme::border());
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, theme::text_dim());

    // Disabled affordance (design doc §5 / §7 item 3): egui 0.31 has no
    // single Visuals field for this (checked -- `disabled_alpha` doesn't
    // exist in this version); `add_enabled(false, ...)` already renders
    // widgets faded via its own noninteractive-style path using the
    // palette above, which reads as the same "greyed out" affordance the
    // doc specifies without needing an extra knob here.

    v.selection.bg_fill = theme::accent().gamma_multiply(0.35);
    v.selection.stroke = egui::Stroke::new(1.0_f32, theme::accent());

    // Type ramp + control metrics from the Windows Fluent Design spec
    // (see theme.rs docs): text and control sizes move together. egui's
    // defaults (12.5px text, 4x1 button padding, 3px stack gap) match
    // neither the design doc nor any desktop standard -- and bumping
    // only the button metrics without the type ramp produced big
    // buttons full of tiny text (caught from a screenshot).
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::proportional(theme::SIZE_HEADING)),
        (egui::TextStyle::Body, egui::FontId::proportional(theme::SIZE_BODY)),
        (egui::TextStyle::Button, egui::FontId::proportional(theme::SIZE_BUTTON)),
        (egui::TextStyle::Small, egui::FontId::proportional(theme::SIZE_SMALL)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(theme::SIZE_BODY)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, theme::PANEL_GAP);
    style.spacing.button_padding = theme::BUTTON_PADDING;
    style.spacing.interact_size.y = theme::MIN_BUTTON_HEIGHT;

    ctx.set_style(style);
}

mod header;
mod ipmanager;
mod layout;
mod panels;
