//! Remuxes pipeline.rs's independent `camvideo` output back together with
//! audio_capture.rs's independent `micaudio` output into the legacy
//! `/cam` path, for existing consumers that expect one combined
//! video+audio stream (HLS playback, the desktop preview tap). `-c copy`
//! both tracks -- everything is already encoded correctly by its own
//! capture leg, this is a pure remux over the RTSP loopback (mediamtx
//! re-serving its own already-published streams to a second client, the
//! same trick roomaudio.rs uses), never a device open.
//!
//! This is deliberately the ONLY place video and audio are recombined.
//! If this process itself hangs or dies, only /cam's remux needs a
//! restart -- camvideo and micaudio keep running and keep being
//! individually consumable throughout (the desktop preview tap could be
//! pointed at camvideo directly in a future pass; left on /cam for now,
//! zero behavior change beyond fixing the coupling).

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const VIDEO_URL: &str = "rtsp://127.0.0.1:8554/camvideo";
const CAM_URL: &str = "rtsp://127.0.0.1:8554/cam";

fn command(ffmpeg: &PathBuf, has_audio: bool) -> Command {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-rtsp_transport", "tcp", "-i", VIDEO_URL]);
    if has_audio {
        cmd.args(["-rtsp_transport", "tcp", "-i", crate::audio_capture::AUDIO_URL])
            .args(["-map", "0:v", "-map", "1:a", "-c", "copy"]);
    } else {
        cmd.args(["-map", "0:v", "-c", "copy"]);
    }
    cmd.args(["-rtsp_transport", "tcp", "-f", "rtsp", CAM_URL]);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::winproc::CREATE_NO_WINDOW);
    }
    cmd
}

/// Persistent, not bounded -- /cam is expected to exist for the life of
/// the app, same as roomaudio.rs's WHEP leg.
pub fn spawn_supervisor(ffmpeg: PathBuf, has_audio: bool) {
    tokio::spawn(async move {
        loop {
            if let Ok(mut child) = command(&ffmpeg, has_audio).spawn() {
                let _ = child.wait().await;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}
