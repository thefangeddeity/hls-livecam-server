//! Republishes micaudio (audio_capture.rs) as Opus on a dedicated
//! "roomaudio" mediamtx path, for the two-way WHEP inbound leg (the
//! room, heard by the caller). Mirrors broadcast-api's
//! `_start_roomaudio`/`_roomaudio_supervise`: WHEP requires Opus and
//! mediamtx does not transcode, but micaudio is AAC, so without this leg
//! the two-way inbound WHEP subscription has no track any browser can
//! actually decode -- silent inbound, nothing to explain it client-side.
//!
//! Derived from audio_capture.rs's independent `micaudio` output over the
//! RTSP loopback -- mediamtx re-serving its own already-published stream
//! to a second client -- NOT a second dshow capture of the physical mic.
//! index.html used to carry a comment (and this module's absence matched
//! it) reasoning that a dedicated roomaudio republish would need a second
//! concurrent open of the same physical mic device, which dshow cannot
//! reliably do (unlike ALSA's dmix) -- a real constraint, but one that
//! does not apply here: this reads an RTSP *output* from mediamtx, the
//! same way the HLS viewer already does, so it never touches the capture
//! device a second time. That comment was the actual reason this was
//! left unbuilt; it was wrong.
//!
//! Reads `micaudio` rather than `/cam`: pipeline.rs's video and
//! audio_capture.rs's audio are fully independent processes now (see
//! pipeline.rs's header comment), and this leg has no reason to depend
//! on video/cam_mux's health just to get at the room's audio.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const ROOM_URL: &str = "rtsp://127.0.0.1:8554/roomaudio";

fn start(ffmpeg: &PathBuf) -> std::io::Result<std::process::Child> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-rtsp_transport",
        "tcp",
        "-i",
        crate::audio_capture::AUDIO_URL,
        "-c:a",
        "libopus",
        "-b:a",
        "64k",
        "-ar",
        "48000",
        "-ac",
        "1",
        "-application",
        "voip",
        "-rtsp_transport",
        "tcp",
        "-f",
        "rtsp",
        ROOM_URL,
    ]);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::winproc::CREATE_NO_WINDOW);
    }
    cmd.spawn()
}

/// Keep the roomaudio Opus leg alive for the life of the process. Fails
/// until micaudio exists (audio_capture still starting) and whenever
/// that leg restarts (e.g. a notch/gain reload); a short backoff between
/// respawns keeps this from busy-looping while it's momentarily absent.
/// Persistent, not bounded like the talkback leg -- WHEP subscribers
/// expect it ready, not started on demand.
pub fn spawn_supervisor(ffmpeg: PathBuf) {
    std::thread::spawn(move || loop {
        if let Ok(mut child) = start(&ffmpeg) {
            let _ = child.wait();
        }
        std::thread::sleep(Duration::from_secs(1));
    });
}
