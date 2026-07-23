//! Live video for the FEED panel.
//!
//! camdash solves this problem by rendering into a text terminal (the
//! halfblock/palette-clustering engine read during run 1/2 research) --
//! that machinery exists solely to work around curses' color-pair ceiling
//! and has nothing to port here. A native window can just paint real
//! pixels, so this does the plain thing: a second, independent ffmpeg
//! process taps the same local RTSP publish point the fleet-facing
//! pipeline already produces (rtsp://127.0.0.1:8554/cam), decodes to raw
//! RGB frames at a reduced size/rate, and hands the latest frame to the
//! GUI thread to upload as an egui texture.
//!
//! This is a read-only consumer -- it does not touch mediamtx, the RTSP
//! publish path, or run-2's capture supervision in any way. Worst case if
//! it wedges: the FEED panel goes stale, nothing fleet-facing is affected.
//!
//! Deliberately modest: 480x270 @ 8fps is plenty for an operator glance
//! panel and keeps a second decode off the CPU budget the real capture
//! pipeline needs.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

const PREVIEW_W: usize = 480;
const PREVIEW_H: usize = 270;
/// Soft ceiling for the preview's delivered rate. The rate is NEVER set as
/// an independent number -- it is derived from the capture rate as the
/// highest integer divisor at or below this cap (see PREVIEW_DECIMATION).
/// At the current 15fps capture this resolves to 15fps (1:1, no
/// decimation). Measured cost of 15 vs the old 8 is negligible: the RTSP
/// H.264 decode dominates and is paid regardless of output rate; the extra
/// 480x270 scale/convert per frame is cheap.
const PREVIEW_FPS_CAP: u32 = 15;
/// Source frames consumed per delivered frame, N >= 1: `ceil(capture/cap)`,
/// the smallest N keeping `capture/N <= cap`. The tap then takes every Nth
/// source frame -- EVEN decimation, whose content spacing is a constant N
/// and therefore cannot judder. This replaces a hardcoded 8fps: 15/8 is
/// non-integer, so ffmpeg's fps filter spaced frames 2,2,2,...,1 with a
/// once-per-second hitch (the rhythmic freeze). Deriving the rate makes
/// that mismatch unrepresentable rather than a thing to get right by hand.
const PREVIEW_DECIMATION: u32 = {
    let cap = PREVIEW_FPS_CAP;
    let src = crate::pipeline::CAPTURE_FPS;
    (src + cap - 1) / cap // ceil(src / cap)
};
const FRAME_BYTES: usize = PREVIEW_W * PREVIEW_H * 3;
const RESTART_BACKOFF: Duration = Duration::from_secs(2);

pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
    /// Bumped on every new frame so the GUI can skip re-uploading a
    /// texture when nothing changed since the last repaint.
    pub generation: u64,
}

pub type SharedFrame = Arc<Mutex<Option<Frame>>>;

/// Shutdown handle for the preview tap. The tap is a child ffmpeg this app
/// spawns, and it must be reaped when the app exits -- pipeline.shutdown()
/// only reaps the capture + mediamtx children, so before run 7 a deliberate
/// Quit orphaned this tap at elevated integrity (an ffmpeg with a dead
/// parent the operator then couldn't kill). This gives the GUI's on_exit a
/// direct kill: it takes the current child and stops the supervise loop
/// from respawning.
#[derive(Clone)]
pub struct PreviewCtl {
    child: Arc<tokio::sync::Mutex<Option<Child>>>,
    running: Arc<AtomicBool>,
}

impl PreviewCtl {
    pub async fn shutdown(&self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
        }
    }
}

/// `repaint` is a deferred handle to the egui context: this runs on the
/// server thread before eframe has created its Context, so the slot is
/// filled later (in main's eframe creation closure). Once set, the reader
/// wakes the GUI the instant each frame lands, so the FEED panel paints
/// exactly once per arrived frame (event-driven) instead of on a fixed
/// clock that beats against the source rate and stutters -- see the cadence
/// note in gui::update.
pub fn spawn(
    ffmpeg: std::path::PathBuf,
    repaint: Arc<OnceLock<egui::Context>>,
) -> (SharedFrame, PreviewCtl) {
    let slot: SharedFrame = Arc::new(Mutex::new(None));
    let out = slot.clone();
    let ctl = PreviewCtl {
        child: Arc::new(tokio::sync::Mutex::new(None)),
        running: Arc::new(AtomicBool::new(true)),
    };
    let loop_ctl = ctl.clone();

    tokio::spawn(async move {
        let mut generation: u64 = 0;
        while loop_ctl.running.load(Ordering::Relaxed) {
            let mut cmd = Command::new(&ffmpeg);
            cmd.args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-rtsp_transport",
                "tcp",
                "-i",
                "rtsp://127.0.0.1:8554/cam",
                "-vf",
                &format!("scale={PREVIEW_W}:{PREVIEW_H}"),
                "-r",
                // Exact rational (e.g. "15/1"), not a rounded decimal, so
                // the decimation stays exactly every-Nth-frame.
                &format!("{}/{}", crate::pipeline::CAPTURE_FPS, PREVIEW_DECIMATION),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
            // Windowless, like every other child spawn (winproc::CREATE_NO_WINDOW).
            #[cfg(windows)]
            cmd.creation_flags(crate::winproc::CREATE_NO_WINDOW);
            match cmd.spawn() {
                Ok(mut child) => {
                    let mut stdout = child.stdout.take().unwrap();
                    // Hand ownership of the child to the shared slot so
                    // PreviewCtl::shutdown can kill it directly on exit.
                    *loop_ctl.child.lock().await = Some(child);
                    let mut buf = vec![0u8; FRAME_BYTES];
                    loop {
                        match stdout.read_exact(&mut buf).await {
                            Ok(_) => {
                                generation += 1;
                                *out.lock().unwrap() = Some(Frame {
                                    width: PREVIEW_W,
                                    height: PREVIEW_H,
                                    rgb: buf.clone(),
                                    generation,
                                });
                                // Wake the GUI to paint this exact frame now
                                // (event-driven cadence). No-op until the
                                // eframe closure fills the slot; after that,
                                // one repaint per arrived frame.
                                if let Some(ctx) = repaint.get() {
                                    ctx.request_repaint();
                                }
                            }
                            Err(_) => break, // pipe closed -- source not ready or ffmpeg exited
                        }
                    }
                    // Reap it (idempotent if shutdown already took it).
                    if let Some(mut child) = loop_ctl.child.lock().await.take() {
                        let _ = child.kill().await;
                    }
                }
                Err(e) => eprintln!("video_preview: failed to start ffmpeg: {e}"),
            }
            if !loop_ctl.running.load(Ordering::Relaxed) {
                break;
            }
            tokio::time::sleep(RESTART_BACKOFF).await;
        }
    });

    (slot, ctl)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of deriving the rate: it is always an even divisor of
    /// the capture rate, as close to the cap as possible without exceeding
    /// it. If someone changes CAPTURE_FPS or the cap, this guards the
    /// even-decimation invariant instead of leaving it to manual vigilance.
    #[test]
    fn preview_rate_is_even_divisor_at_or_below_cap() {
        let src = crate::pipeline::CAPTURE_FPS;
        let n = PREVIEW_DECIMATION;
        assert!(n >= 1, "decimation must consume >= 1 source frame");
        // delivered rate = src/n must not exceed the cap
        assert!(src <= PREVIEW_FPS_CAP * n, "preview rate src/{n} exceeds cap");
        // ...and n is the SMALLEST such value (n-1 would exceed the cap),
        // i.e. we deliver as close to the cap as an even divisor allows.
        if n > 1 {
            assert!(src > PREVIEW_FPS_CAP * (n - 1), "n is not minimal");
        }
    }
}
