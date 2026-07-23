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

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::diskhealth::{self, DiskInfo};
use crate::metrics::{Metrics, Snapshot};
use crate::pipeline::{self, Pipeline};
use crate::routes;
use crate::state::AppState;
use crate::tray::{self, Tray};
use crate::video_preview::{self, SharedFrame};

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
/// How long a footer status message stays up before the bar falls back to
/// the ambient feed state.
const STATUS_HOLD: Duration = Duration::from_secs(5);

// --- Feed-switch transition (run 8) ---
/// No new preview frame for this long during a transition = the old source
/// has stopped (the tap lost its input). Well below the tap's 2s reconnect
/// backoff, well above the ~125ms preview frame interval, so it trips
/// reliably on every switch without false-firing on a single dropped frame.
/// Once this gap is seen, the NEXT frame is accepted as the new source.
const SWITCH_GAP: Duration = Duration::from_millis(700);
/// Safety ceiling: if no new-source frame arrives within this, the switch
/// is treated as failed and the FEED falls through to the real NO SIGNAL
/// error state. The animation must never spin forever. The web viewer's
/// cover is ~16s; this gives margin over a worst-case pipeline flush.
const TRANSITION_CAP: Duration = Duration::from_secs(22);
/// Repaint cadence while the static animation is running. Each repaint is
/// a full-window egui re-render, so this is the dominant cost of the snow
/// -- at 30fps it measured ~12-21% CPU, enough to contend with the capture
/// ffmpeg (and a remote-desktop re-encode of the ever-changing snow) and
/// snag the feed on a switch (operator). 15fps still reads as live snow at
/// roughly half the cost. draw_switching self-schedules this whenever the
/// static is on screen (transition OR feed-off standby).
const STATIC_FPS_INTERVAL: Duration = Duration::from_millis(66);

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
    video_preview: video_preview::PreviewCtl,
    video_texture: Option<egui::TextureHandle>,
    video_last_gen: u64,
    last_frame_at: Instant,

    // Feed-switch transition (run 8): while a switch is flushing the
    // pipeline, the FEED shows a VHS-static animation instead of NO SIGNAL.
    feed_transition: Option<FeedTransition>,
    last_switch_seq: u64,
    noise_texture: Option<egui::TextureHandle>,
    noise_rng: u64,
    noise_frame: u64,

    hostname: String,
    tailscale: String,
    local_ip: String,

    // Footer status line (replaces the old static IP text): the last
    // operator action, shown for a few seconds then fading back to the
    // ambient feed state. Set via set_status() at each action's click.
    status_text: String,
    status_at: Instant,

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
    /// True while the window is minimized to the tray. Shared with the
    /// tray thread (which clears it on Show). Minimizing (not hiding) is
    /// what actually parks the loop at ~0% -- winit stops delivering
    /// redraws to an iconic window (measured 0.4% minimized vs 95% for a
    /// Visible(false)-hidden window, which eframe/glow busy-loops). This
    /// flag is a cheap belt-and-suspenders gate on update() in case a
    /// stray repaint slips through while minimized.
    window_hidden: Arc<AtomicBool>,
}

/// One in-progress feed switch. Two-phase (see SWITCH_GAP): first we wait
/// for the preview tap's frames to freeze (`gap_gen` records the frozen
/// generation), then we accept the first frame past that as the new source
/// -- which rejects any trailing old-source frames still in flight when the
/// switch began. `started` bounds the whole thing against TRANSITION_CAP.
struct FeedTransition {
    started: Instant,
    gap_gen: Option<u64>,
}

/// Plain-value inputs to the transition decision, so the core logic is
/// pure and unit-testable without an App/egui/pipeline (the live path is
/// otherwise only reachable with a real camera + free pipeline ports).
struct TransitionInputs {
    /// A transition is currently active.
    active: bool,
    /// The frozen-frame generation recorded once the gap was seen.
    gap_gen: Option<u64>,
    /// Server on (deliberate-off cancels any transition).
    enabled: bool,
    /// switch_seq changed this frame (a capture (re)start happened).
    switch_changed: bool,
    /// The preview was showing live video when the switch fired.
    was_live: bool,
    /// Frames have been frozen long enough (> SWITCH_GAP) -- old source gone.
    gap_seen: bool,
    /// Current preview-tap frame generation.
    video_last_gen: u64,
    /// The active transition has exceeded TRANSITION_CAP.
    timed_out: bool,
}

/// Outcome of one transition step.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    /// Not transitioning -- draw live video or NO SIGNAL (per feed_offline).
    Idle,
    /// Start a fresh transition this frame.
    Begin,
    /// Stay in the transition; carry this (possibly newly-set) gap_gen.
    Continue(Option<u64>),
}

/// The pure feed-switch decision. Two-phase: begin only from a live feed;
/// then wait for the frames to freeze (records gap_gen), then accept the
/// first frame past that freeze as the new source (rejects trailing
/// old-source frames). Timeout or server-off end it to the real error state.
fn transition_next(i: TransitionInputs) -> Next {
    // Server off is a deliberate stop -- real OFF/NO SIGNAL always wins.
    if !i.enabled {
        return Next::Idle;
    }
    if i.active {
        // A failed switch must eventually surface the genuine error.
        if i.timed_out {
            return Next::Idle;
        }
        // Phase 1: record the frozen generation once the old source stops.
        let gap_gen = if i.gap_gen.is_none() && i.gap_seen {
            Some(i.video_last_gen)
        } else {
            i.gap_gen
        };
        // Phase 2: the first frame past the freeze is the new source.
        if let Some(g) = gap_gen {
            if i.video_last_gen > g {
                return Next::Idle;
            }
        }
        return Next::Continue(gap_gen);
    }
    // Not transitioning: a switch off a live feed begins one. A restart into
    // an already-broken feed is a recovery, not a switch -- stay Idle so the
    // FEED keeps showing NO SIGNAL rather than a reassuring "SWITCHING".
    if i.switch_changed && i.was_live {
        Next::Begin
    } else {
        Next::Idle
    }
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
        video_preview: video_preview::PreviewCtl,
        tray: Option<Tray>,
        window_hidden: Arc<AtomicBool>,
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
            video_preview,
            video_texture: None,
            video_last_gen: 0,
            last_frame_at: Instant::now(),
            feed_transition: None,
            last_switch_seq: 0,
            noise_texture: None,
            // Any nonzero seed; xorshift state persists across frames so the
            // noise keeps crawling instead of repeating.
            noise_rng: 0x9E37_79B9_7F4A_7C15,
            noise_frame: 0,
            hostname: routes::hostname(),
            tailscale: routes::tailscale_ip(),
            local_ip: routes::local_ip(),
            status_text: String::new(),
            // Start already-expired so the footer shows ambient state, not
            // a stale message. checked_sub avoids an underflow panic if the
            // monotonic clock's zero is < STATUS_HOLD ago at startup.
            status_at: Instant::now()
                .checked_sub(STATUS_HOLD)
                .unwrap_or_else(Instant::now),
            editing_msg: false,
            edit_buffer: String::new(),
            ip_manager_open: false,
            cams: crate::cams::CamStore::load(),
            ip_form: IpForm::default(),
            start: Instant::now(),
            tray,
            window_hidden,
        }
    }

    fn spawn_async<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.rt.spawn(fut);
    }

    /// Post a transient status message to the footer status bar. Shown
    /// for STATUS_HOLD, after which the footer falls back to the ambient
    /// feed state. Called synchronously at each action's click site (not
    /// from the spawned async task) so it registers immediately.
    pub(super) fn set_status(&mut self, msg: impl Into<String>) {
        self.status_text = msg.into();
        self.status_at = Instant::now();
    }

    /// The current footer status: a fresh action message if one is still
    /// within its hold window, otherwise None (the footer then shows the
    /// ambient feed state instead).
    pub(super) fn active_status(&self) -> Option<&str> {
        if self.status_at.elapsed() < STATUS_HOLD && !self.status_text.is_empty() {
            Some(&self.status_text)
        } else {
            None
        }
    }

    /// Real NO SIGNAL for the FEED preview. Run 8 keys this purely on
    /// server-on + actual frame flow, NOT the pipeline liveness rows
    /// (hls_state/capture_alive). Those rows lag reality by up to a poll
    /// (~1s) and flip DOWN on every switch, so keying NO SIGNAL off them
    /// both (a) flashed the error on a normal switch and (b) risked a tail
    /// flash right as a switch completed but before hls_state caught up.
    /// The preview tap's own frame flow is the ground truth for "is video
    /// actually showing"; a genuinely dead pipeline stops those frames, so
    /// staleness still catches real failure (just ~FRAME_STALE_AFTER
    /// slower, which is fine -- the VIDEO panel rows still report the
    /// pipeline state immediately for diagnosis). The transition animation
    /// (draw_feed) takes precedence over this during a switch.
    fn feed_offline(&self, p: &pipeline::PipelineStatus) -> bool {
        !p.enabled || self.last_frame_at.elapsed() > FRAME_STALE_AFTER
    }

    /// Advance the feed-switch transition state machine once per frame.
    /// Reads the wall-clock/frame inputs off `self`, delegates the pure
    /// decision to `transition_next` (unit-tested), applies the result, and
    /// drives the ~30fps repaint while a transition is live.
    fn update_feed_transition(&mut self, ctx: &egui::Context, p: &pipeline::PipelineStatus) {
        let switch_changed = p.switch_seq != self.last_switch_seq;
        if switch_changed {
            self.last_switch_seq = p.switch_seq;
        }
        let frame_gap = self.last_frame_at.elapsed();
        let was_live = frame_gap < FRAME_STALE_AFTER;
        let gap_seen = frame_gap > SWITCH_GAP;
        let (active, gap_gen, timed_out) = match &self.feed_transition {
            Some(t) => (true, t.gap_gen, t.started.elapsed() > TRANSITION_CAP),
            None => (false, None, false),
        };

        match transition_next(TransitionInputs {
            active,
            gap_gen,
            enabled: p.enabled,
            switch_changed,
            was_live,
            gap_seen,
            video_last_gen: self.video_last_gen,
            timed_out,
        }) {
            Next::Idle => self.feed_transition = None,
            Next::Begin => {
                self.feed_transition = Some(FeedTransition {
                    started: Instant::now(),
                    gap_gen: None,
                })
            }
            Next::Continue(g) => {
                if let Some(t) = self.feed_transition.as_mut() {
                    t.gap_gen = g;
                }
            }
        }
        // The static's repaint cadence is self-scheduled by draw_switching
        // (it also covers the feed-off standby case), so nothing to do here.
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
        // Minimized to the tray: winit already parks the loop (an iconic
        // window gets no redraws -- measured 0.4% CPU), so update() normally
        // isn't even called here. This gate is defensive: if a stray repaint
        // does land while minimized, skip all render work and throttle rather
        // than spin. (An earlier design HID the window with Visible(false)
        // instead of minimizing; eframe/glow busy-looped that invisible
        // surface at ~95% CPU regardless of the repaint schedule, starving
        // the capture -- minimizing is what fixed it.) The tray menu runs on
        // its own thread, so nothing interactive is lost here.
        if self.window_hidden.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
            return;
        }

        if self.last_metrics.elapsed() >= METRICS_REFRESH {
            self.snapshot = self.metrics.refresh();
            self.last_metrics = Instant::now();
        }
        if self.last_disk.elapsed() >= DISK_REFRESH {
            self.disk = diskhealth::query();
            self.last_disk = Instant::now();
        }
        self.update_video_texture(ctx);

        // Window-X policy (run 7): closing the window minimizes to the tray
        // and keeps the server running -- a naive family member must not be
        // able to kill the camera by clicking X. Tray menu clicks (Show/
        // Quit) are handled off-thread now (tray::spawn_menu_handler, run
        // 8): a hidden window stops repainting, so update() can't be relied
        // on to poll them -- that's why tray Quit didn't work once
        // minimized. Fallback: with no tray there's no Show/Quit affordance,
        // so X must still quit (on_exit reaps the children) or the app would
        // be unclosable except via Task Manager.
        if ctx.input(|i| i.viewport().close_requested()) && self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            // OS-minimize, NOT Visible(false)/hide. A hidden eframe window
            // busy-loops its GL present at ~95% CPU (eframe skips update()
            // for it but keeps spinning -- measured, single thread pegged);
            // a MINIMIZED window is parked by winit and drops to ~0%. Trade:
            // the window stays as a taskbar button rather than vanishing to
            // tray-only, but it's out of the way, the server keeps running,
            // and tray Show / the taskbar button both restore it.
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            self.window_hidden.store(true, Ordering::Relaxed);
        }

        apply_theme(ctx);

        let pstatus = self.pipeline.status();
        // Advance the feed-switch transition before drawing the FEED so the
        // panel renders the right state (static vs live vs NO SIGNAL) this
        // frame. Runs after update_video_texture so last_frame_at/
        // video_last_gen reflect this frame's tap output.
        self.update_feed_transition(ctx, &pstatus);

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
            .show(ctx, |ui| self.draw_footer(ui, &pstatus));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::bg()).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                self.draw_layout(ui, &pstatus);
            });

        // Modal is drawn last so its backdrop overlays everything above.
        if self.ip_manager_open {
            self.draw_ip_manager(ctx);
        }

        // Repaint cadence (we only reach here while visible -- hidden
        // early-returns above). Repaint fast (STATIC_FPS_INTERVAL, 15fps)
        // whenever there's motion to show: the static animation
        // (switching/standby) OR live video -- the preview was jerky at the
        // old 6.7fps idle cadence (an 8fps source shown at 6.7fps). NO
        // SIGNAL / idle stays slow.
        let showing_motion =
            self.feed_transition.is_some() || !pstatus.enabled || !self.feed_offline(&pstatus);
        let interval = if showing_motion {
            STATIC_FPS_INTERVAL
        } else {
            REPAINT_INTERVAL
        };
        ctx.request_repaint_after(interval);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // See pipeline.rs::shutdown docs -- Windows won't do this for us.
        // Reap ALL children: capture + mediamtx (pipeline.shutdown) AND the
        // video_preview tap (preview.shutdown). The tap used to be left
        // running -- a deliberate Quit orphaned an elevated ffmpeg the
        // operator then couldn't kill (run-6 diagnosis, pid evidence).
        let pipeline = self.pipeline.clone();
        let preview = self.video_preview.clone();
        self.rt.block_on(async move {
            pipeline.shutdown().await;
            preview.shutdown().await;
        });
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

#[cfg(test)]
mod transition_tests {
    use super::{transition_next, Next, TransitionInputs};

    /// Builder with sensible defaults so each test states only what it cares
    /// about.
    fn inp() -> TransitionInputs {
        TransitionInputs {
            active: false,
            gap_gen: None,
            enabled: true,
            switch_changed: false,
            was_live: false,
            gap_seen: false,
            video_last_gen: 100,
            timed_out: false,
        }
    }

    #[test]
    fn idle_stays_idle_without_a_switch() {
        assert_eq!(transition_next(inp()), Next::Idle);
    }

    #[test]
    fn switch_from_live_begins() {
        let i = TransitionInputs { switch_changed: true, was_live: true, ..inp() };
        assert_eq!(transition_next(i), Next::Begin);
    }

    #[test]
    fn switch_while_broken_does_not_begin() {
        // A restart into an already-dead feed is recovery, not a switch:
        // stay Idle so the FEED keeps showing NO SIGNAL, not "SWITCHING".
        let i = TransitionInputs { switch_changed: true, was_live: false, ..inp() };
        assert_eq!(transition_next(i), Next::Idle);
    }

    #[test]
    fn active_waits_for_the_freeze() {
        // Transitioning, frames haven't frozen yet -> keep waiting, no gap.
        let i = TransitionInputs { active: true, gap_seen: false, ..inp() };
        assert_eq!(transition_next(i), Next::Continue(None));
    }

    #[test]
    fn trailing_old_frames_do_not_exit_early() {
        // Active, no gap recorded yet, and a frame bumps the generation
        // (a trailing old-source frame). Must NOT exit -- gap not seen.
        let i = TransitionInputs { active: true, gap_gen: None, gap_seen: false, video_last_gen: 105, ..inp() };
        assert_eq!(transition_next(i), Next::Continue(None));
    }

    #[test]
    fn freeze_records_the_gap_generation() {
        // Active, frames frozen this frame -> record gap_gen, don't exit yet.
        let i = TransitionInputs { active: true, gap_gen: None, gap_seen: true, video_last_gen: 108, ..inp() };
        assert_eq!(transition_next(i), Next::Continue(Some(108)));
    }

    #[test]
    fn holds_after_freeze_until_a_new_frame() {
        // gap recorded at 108, generation hasn't advanced past it -> hold.
        let i = TransitionInputs { active: true, gap_gen: Some(108), video_last_gen: 108, ..inp() };
        assert_eq!(transition_next(i), Next::Continue(Some(108)));
    }

    #[test]
    fn new_source_frame_ends_the_transition() {
        // gap recorded at 108, a fresh frame past it -> new source is live.
        let i = TransitionInputs { active: true, gap_gen: Some(108), video_last_gen: 109, ..inp() };
        assert_eq!(transition_next(i), Next::Idle);
    }

    #[test]
    fn timeout_falls_through_to_error() {
        // Failed switch: capped out with no new frame -> real NO SIGNAL.
        let i = TransitionInputs { active: true, gap_gen: Some(108), video_last_gen: 108, timed_out: true, ..inp() };
        assert_eq!(transition_next(i), Next::Idle);
    }

    #[test]
    fn server_off_cancels_any_transition() {
        let i = TransitionInputs { active: true, gap_gen: Some(108), enabled: false, ..inp() };
        assert_eq!(transition_next(i), Next::Idle);
    }
}
