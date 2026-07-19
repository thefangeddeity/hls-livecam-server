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
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

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

pub fn spawn(ffmpeg: std::path::PathBuf) -> SharedFrame {
    let slot: SharedFrame = Arc::new(Mutex::new(None));
    let out = slot.clone();

    tokio::spawn(async move {
        let mut generation: u64 = 0;
        loop {
            match Command::new(&ffmpeg)
                .args([
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
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    let mut stdout = child.stdout.take().unwrap();
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
                    let _ = child.kill().await;
                }
                Err(e) => eprintln!("video_preview: failed to start ffmpeg: {e}"),
            }
            tokio::time::sleep(RESTART_BACKOFF).await;
        }
    });

    slot
}
