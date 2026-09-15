//! Automatic recovery for a real, reproducible hardware/driver fault on
//! this machine's mic array (Intel Smart Sound Technology for Digital
//! Microphones): confirmed by direct testing (2026-09-11) that the
//! capture goes silent -- and comes back silent again within single-digit
//! seconds even fully isolated, with zero app code, zero filters, and no
//! other audio activity anywhere on the box running -- so this is not a
//! bug in dshow_capture/roomaudio/talk, it is the physical device/driver
//! itself. The one thing that reliably restores real signal (repeatedly
//! verified) is a full disable+enable cycle of the parent SST controller
//! device node (NOT the "Microphone Array" MMDEVAPI endpoint alone --
//! that was tried first and does nothing; it has to be the controller).
//! There is no known non-disruptive fix at the OS level, so this watches
//! /cam's actual audio level the same way a human would (volumedetect
//! over the RTSP loopback, same technique used to diagnose this) and
//! kicks the controller when it has been convincingly silent for a
//! sustained period -- long enough that a real quiet room won't trigger
//! it, short enough that a call isn't dead for long before it recovers.
//!
//! pnputil, not the Disable-PnpDevice/Enable-PnpDevice PowerShell
//! cmdlets: same underlying operation, but pnputil is a single plain
//! process with clean exit-success text, no PowerShell quoting/escaping
//! risk from the device's own instance ID (which contains '&' -- fatal
//! to unquoted cmd.exe parsing, confirmed the hard way while debugging
//! this by hand). Needs admin -- fine, main.rs already runs this app
//! elevated (RunLevel Highest) for exactly this class of device control.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// Own tiny append-log, independent of main.rs's private launch_log --
/// this runs for the life of the process and needs a running history
/// (launch_log truncates once per run and main.rs doesn't expose it),
/// so a silent bug here can actually be diagnosed after the fact instead
/// of only living in ffmpeg output nobody's watching.
fn log(msg: &str) {
    let Ok(appdata) = std::env::var("APPDATA") else {
        return;
    };
    let path = std::path::PathBuf::from(appdata)
        .join("hls-livecam-win")
        .join("mic_watchdog.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{epoch}] {msg}");
    }
}

const SOURCE_URL: &str = "rtsp://127.0.0.1:8554/cam";

/// Below this, and it isn't "a quiet room" -- a live mic array picking up
/// nothing but electrical noise floor measures here. Confirmed empirically:
/// good captures (TV/room noise actually present) read -50 to -52 dB max;
/// the stuck-silent state reads -74 to -90 dB max, every time.
const SILENT_MAX_DB: f32 = -65.0;
/// Two consecutive bad probes (~30s of confirmed silence) before acting,
/// so one unlucky quiet moment doesn't trigger a reset mid-call.
const BAD_PROBES_BEFORE_RESET: u32 = 2;
const PROBE_INTERVAL: Duration = Duration::from_secs(15);
/// After a reset, give the app's own capture supervisor time to reopen
/// the device and stabilize before judging it again.
const POST_RESET_COOLDOWN: Duration = Duration::from_secs(45);

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(crate::winproc::CREATE_NO_WINDOW);
}
#[cfg(not(windows))]
fn no_window(_cmd: &mut Command) {}

/// Finds the SST controller's PnP instance ID by parsing `pnputil
/// /enum-devices /connected` for the "Digital Microphones" controller
/// function specifically (not the Verification/OED/Bluetooth siblings
/// that share the same VEN_8086 audio DSP, and not the MMDEVAPI
/// "Microphone Array" endpoint -- that one has no effect when reset).
/// Resolved by description text rather than hardcoded, so a driver
/// reinstall renumbering the instance suffix doesn't silently break this.
fn find_sst_controller_id() -> Option<String> {
    let mut cmd = Command::new("pnputil");
    cmd.args(["/enum-devices", "/connected"]);
    no_window(&mut cmd);
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut pending_id: Option<&str> = None;
    for line in text.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("Instance ID:") {
            pending_id = Some(rest.trim());
        } else if line.trim_start().starts_with("Device Description:")
            && line.trim_end().ends_with("for Digital Microphones")
        {
            if let Some(id) = pending_id {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn reset_controller(instance_id: &str) {
    let mut disable = Command::new("pnputil");
    disable.args(["/disable-device", instance_id]);
    no_window(&mut disable);
    let _ = disable.status();

    std::thread::sleep(Duration::from_secs(2));

    let mut enable = Command::new("pnputil");
    enable.args(["/enable-device", instance_id]);
    no_window(&mut enable);
    let _ = enable.status();

    // The controller reset alone does nothing for a call already in
    // progress: pipeline.rs's dshow capture process opened its handle to
    // the device before the reset and keeps it -- confirmed by hand,
    // resetting the controller with that process still running leaves
    // its output bit-identical, frozen on the same stale buffer. Killing
    // it here is safe: pipeline.rs supervises /cam the same way every
    // other leg in this codebase does (roomaudio.rs, talk.rs) and
    // respawns on exit within about a second, this time opening a fresh
    // handle against the just-reset hardware.
    kill_cam_capture();
}

fn kill_cam_capture() {
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-Command",
        "Get-CimInstance Win32_Process -Filter \"Name='ffmpeg.exe'\" \
         | Where-Object { $_.CommandLine -like '*dshow*' -and $_.CommandLine -like '*8554/cam*' } \
         | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }",
    ]);
    no_window(&mut cmd);
    let _ = cmd.status();
}

/// Probes /cam's current audio level the same way manual diagnosis did:
/// a short read over the RTSP loopback (mediamtx serves any number of
/// readers fine -- HLS, roomaudio.rs, and this all read the same path
/// concurrently without conflict; the actual exclusive-mode contention
/// risk is only ever at the physical dshow capture, which this never
/// touches). Returns None if /cam isn't up yet or didn't answer in time,
/// which is treated as "unknown," never as "silent."
fn probe_max_db(ffmpeg: &PathBuf) -> Option<f32> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "info",
        "-rtsp_transport",
        "tcp",
        "-i",
        SOURCE_URL,
        "-t",
        "2",
        "-map",
        "0:a",
        "-af",
        "volumedetect",
        "-f",
        "null",
        "-",
    ]);
    no_window(&mut cmd);
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    for line in text.lines() {
        if let Some(idx) = line.find("max_volume:") {
            let rest = &line[idx + "max_volume:".len()..];
            let db_str = rest.trim().trim_end_matches("dB").trim();
            if let Ok(db) = db_str.parse::<f32>() {
                return Some(db);
            }
        }
    }
    None
}

/// Keeps the mic array usable across this hardware's known silent-drop
/// fault (see module docs) by watching real captured level and kicking
/// the SST controller when it has been convincingly dead for a sustained
/// stretch. Not a fix for the underlying driver bug -- Task queue also
/// carries the real fix (Intel SST driver update via HP Support
/// Assistant) -- this just keeps the node usable in the meantime.
pub fn spawn_supervisor(ffmpeg: PathBuf) {
    std::thread::spawn(move || {
        let mut instance_id: Option<String> = None;
        let mut bad_streak: u32 = 0;
        log("mic_watchdog: started");

        loop {
            std::thread::sleep(PROBE_INTERVAL);

            if instance_id.is_none() {
                instance_id = find_sst_controller_id();
                match &instance_id {
                    Some(id) => log(&format!("resolved SST controller: {id}")),
                    None => log("SST controller not found yet, will retry"),
                }
            }
            let Some(id) = instance_id.as_deref() else {
                continue;
            };

            match probe_max_db(&ffmpeg) {
                Some(db) if db <= SILENT_MAX_DB => {
                    bad_streak += 1;
                    log(&format!("probe: {db:.1} dB (silent, streak {bad_streak})"));
                    if bad_streak >= BAD_PROBES_BEFORE_RESET {
                        bad_streak = 0;
                        log("resetting SST controller");
                        reset_controller(id);
                        log("reset done, cooling down");
                        std::thread::sleep(POST_RESET_COOLDOWN);
                    }
                }
                Some(db) => {
                    if bad_streak > 0 {
                        log(&format!("probe: {db:.1} dB (recovered)"));
                    }
                    bad_streak = 0;
                }
                None => log("probe: no /cam audio track (pipeline not up yet?)"),
            }
        }
    });
}
