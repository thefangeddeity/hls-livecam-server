//! Independent, always-on audio-only capture, decoupled from video.
//!
//! pipeline.rs used to open camera and mic together on one dshow line,
//! muxed and encoded in a single ffmpeg process (`video=X:audio=Y`).
//! That meant *any* problem on the audio side -- whatever the cause --
//! forced pipeline.rs's own stall/crash supervisor to restart the WHOLE
//! process, taking a perfectly healthy video feed down with it. Confirmed
//! live (2026-09-12): the combined capture process was cycling every
//! ~25-30s while only the audio side was the problem, and /cam 404'd on
//! every cycle as a result. `hide_command` already used a second `-i` for
//! audio under Hide, but that's still one process/one encoder loop --
//! ffmpeg's muxer still waits on both inputs together, so it didn't avoid
//! the coupling either.
//!
//! This runs audio capture as a fully separate process, publishing to
//! its own mediamtx path (`micaudio`). A crash, hang, or restart here
//! never touches video. cam_mux.rs remuxes this back together with
//! camera_capture (pipeline.rs)'s `camvideo` output for legacy /cam
//! (HLS) consumers; roomaudio.rs reads straight from here now instead of
//! bouncing through /cam, so two-way RTC audio doesn't depend on video
//! capture's health either.

use crate::state::AppState;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

pub const AUDIO_URL: &str = "rtsp://127.0.0.1:8554/micaudio";

const AUDIO_BITRATE: &str = "96k";
const AUDIO_SAMPLE_RATE: &str = "48000";
const AUDIO_CHANNELS: &str = "2";

fn command(ffmpeg: &PathBuf, device_name: &str, af: &str) -> Command {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-f", "dshow", "-i"])
        .arg(format!("audio={device_name}"))
        .args([
            "-af", af, "-c:a", "aac", "-b:a", AUDIO_BITRATE, "-ar", AUDIO_SAMPLE_RATE, "-ac", AUDIO_CHANNELS,
            "-rtsp_transport", "tcp", "-f", "rtsp", AUDIO_URL,
        ]);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::winproc::CREATE_NO_WINDOW);
    }
    cmd
}

/// Handle back to the running supervisor, so pipeline.rs's reload_audio()
/// (called after any /api/notches write) can force an immediate restart
/// with the freshly-changed filter chain -- same "kill it, the supervisor
/// respawns with current state" shape restart_capture already used for
/// the combined process, now scoped to audio alone. Killing here never
/// touches camvideo.
pub struct AudioCapture {
    current: Mutex<Option<Child>>,
}

impl AudioCapture {
    pub async fn reload(&self) {
        if let Some(mut child) = self.current.lock().await.take() {
            let _ = child.kill().await;
        }
    }
}

/// Runs for the life of the process, same shape as roomaudio.rs -- a
/// no-op if this node has no mic (matches pipeline.rs's existing "off by
/// default, a node with no microphone publishes exactly what it did
/// before" rule).
pub fn spawn_supervisor(
    ffmpeg: PathBuf,
    state: Arc<AppState>,
    device_name: Option<String>,
) -> Arc<AudioCapture> {
    let handle = Arc::new(AudioCapture { current: Mutex::new(None) });
    let Some(device_name) = device_name else {
        return handle;
    };

    crate::launch_log(&format!("audio_capture: spawning supervisor for device {device_name:?}"));
    let handle_for_loop = handle.clone();
    tokio::spawn(async move {
        crate::launch_log("audio_capture: supervisor task entered");
        loop {
            let af = state.notches.build_af_chain(&state.audio);
            crate::launch_log(&format!("audio_capture: launching ffmpeg (af chain {} chars)", af.len()));
            match command(&ffmpeg, &device_name, &af).spawn() {
                Ok(child) => {
                    crate::launch_log(&format!("audio_capture: ffmpeg spawned, pid {:?}", child.id()));
                    *handle_for_loop.current.lock().await = Some(child);
                    let exited = loop {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        let mut slot = handle_for_loop.current.lock().await;
                        match slot.as_mut() {
                            Some(child) => {
                                if matches!(child.try_wait(), Ok(Some(_))) {
                                    *slot = None;
                                    break true;
                                }
                            }
                            None => break false, // reload() took it -- restart now, no backoff needed either way
                        }
                    };
                    let _ = exited;
                }
                Err(e) => {
                    crate::launch_log(&format!("audio_capture: failed to start: {e}"));
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    handle
}
