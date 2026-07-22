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
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

const PREVIEW_W: usize = 480;
const PREVIEW_H: usize = 270;
const PREVIEW_FPS: &str = "8";
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

pub fn spawn(ffmpeg: std::path::PathBuf) -> (SharedFrame, PreviewCtl) {
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
                PREVIEW_FPS,
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
