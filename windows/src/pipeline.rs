//! Capture pipeline: mediamtx + a swappable ffmpeg source, both supervised.
//!
//! dshow camera --[ffmpeg capture]--> rtsp://127.0.0.1:8554/cam --[mediamtx]--> :8888/cam/index.m3u8
//!
//! "Show" and "Hide" are two different ffmpeg command lines pushing to the
//! *same* RTSP path; swapping between them just means killing one child and
//! starting the other. mediamtx keeps the HLS output continuous across that
//! swap because, from its point of view, a publisher disconnected and a new
//! one connected to the same path -- consumers never see the stream drop.
//!
//! Self-healing note: camdash was supposed to be the reference for a
//! staleness/auto-repair heuristic to port. Reading it live, that mechanism
//! does not exist -- hls_worker() only checks whether GET /cam/index.m3u8
//! returns HTTP 200 every 5s (it doesn't look at whether the manifest is
//! actually advancing), and run_repair() has exactly one call site, the
//! interactive [r] key. There is no auto-repair loop anywhere in the
//! current source, despite the README's claim. So the detector below is not
//! a port -- there was nothing live to port. It's sized off mediamtx's own
//! segment timing (TARGETDURATION=4s; 8s is two full segments, comfortably
//! past any single missed cycle) rather than the README's number.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::state::AppState;

const RTSP_URL: &str = "rtsp://127.0.0.1:8554/cam";
const HLS_MASTER_PATH: &str = "/cam/index.m3u8";
const RESTART_BACKOFF: Duration = Duration::from_secs(2);
const STALL_POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Two full mediamtx HLS segments (TARGETDURATION=4s) with no manifest
/// change at all is "definitely stuck," not "unlucky poll timing."
const STALL_THRESHOLD: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    Show,
    /// Also where "cloak" lands this run -- see state.rs / run-2 report.
    Hidden,
}

fn source_for_mode(mode: &str) -> Source {
    match mode {
        "show" => Source::Show,
        _ => Source::Hidden, // "hide" and "cloak" (fail-safe) both land here
    }
}

pub struct Pipeline {
    ffmpeg: PathBuf,
    mediamtx: PathBuf,
    state: Arc<AppState>,
    capture: Mutex<Option<Child>>,
    target: Mutex<Source>,
    device: Mutex<String>,
    running: AtomicBool,
}

impl Pipeline {
    pub async fn start(
        ffmpeg: PathBuf,
        mediamtx: PathBuf,
        state: Arc<AppState>,
    ) -> Arc<Self> {
        let device = load_or_pick_device(&ffmpeg, &state).await;
        let initial = source_for_mode(&state.feed_mode.lock().unwrap().clone());

        let p = Arc::new(Self {
            ffmpeg,
            mediamtx,
            state,
            capture: Mutex::new(None),
            target: Mutex::new(initial),
            device: Mutex::new(device),
            running: AtomicBool::new(true),
        });

        spawn_mediamtx_supervisor(p.clone());
        // Give mediamtx a moment to open its RTSP listener before the first
        // publisher tries to connect.
        tokio::time::sleep(Duration::from_millis(800)).await;
        p.swap_to(initial).await;
        spawn_stall_supervisor(p.clone());

        p
    }

    /// Drives the actual swap. "cloak" fails safe to the same black source
    /// as "hide" -- see AskUserQuestion decision recorded in the run-2
    /// report; the real pixelation effect is v1.1 scope.
    pub async fn apply_feed_mode(&self, mode: &str) {
        self.swap_to(source_for_mode(mode)).await;
    }

    async fn swap_to(&self, target: Source) {
        *self.target.lock().await = target;
        self.restart_capture().await;
    }

    async fn restart_capture(&self) {
        let target = *self.target.lock().await;
        let device = self.device.lock().await.clone();

        let mut slot = self.capture.lock().await;
        if let Some(mut child) = slot.take() {
            let _ = child.kill().await;
        }

        let cmd = match target {
            Source::Show => capture_command(&self.ffmpeg, &device),
            Source::Hidden => hide_command(&self.ffmpeg),
        };
        match spawn(cmd) {
            Ok(child) => {
                println!("pipeline: capture -> {target:?} ({device})");
                *slot = Some(child);
            }
            Err(e) => eprintln!("pipeline: failed to start capture ({target:?}): {e}"),
        }
    }
}

// --------------------------------------------------------- mediamtx

fn spawn_mediamtx_supervisor(p: Arc<Pipeline>) {
    tokio::spawn(async move {
        let config_path = p.state.dir().join("mediamtx.yml");
        // Minimal config -- verified against the pinned v1.15.2 binary to
        // produce output identical to hls-livecam-setup's pristine-default-
        // plus-appended-paths-block file. mediamtx fills in every omitted
        // key from its own built-in defaults, so there's no doc-sized file
        // to keep in sync with the binary version.
        let _ = std::fs::write(&config_path, "paths:\n  cam:\n  all_others:\n");

        while p.running.load(Ordering::Relaxed) {
            let mut cmd = Command::new(&p.mediamtx);
            cmd.arg(&config_path);
            match spawn(cmd) {
                Ok(mut child) => {
                    println!("pipeline: mediamtx started");
                    let _ = child.wait().await;
                    eprintln!("pipeline: mediamtx exited, restarting");
                }
                Err(e) => eprintln!("pipeline: failed to start mediamtx: {e}"),
            }
            tokio::time::sleep(RESTART_BACKOFF).await;
        }
    });
}

// --------------------------------------------------------- self-healing

fn spawn_stall_supervisor(p: Arc<Pipeline>) {
    tokio::spawn(async move {
        let mut last_body: Option<String> = None;
        let mut unchanged_since: Option<std::time::Instant> = None;

        loop {
            tokio::time::sleep(STALL_POLL_INTERVAL).await;

            // Fast path: the capture child exited on its own (camera
            // unplugged and ffmpeg errored out, crash, etc). Don't wait for
            // the manifest to go stale -- restart now.
            let exited = {
                let mut slot = p.capture.lock().await;
                match slot.as_mut() {
                    Some(child) => matches!(child.try_wait(), Ok(Some(_))),
                    None => true,
                }
            };
            if exited {
                eprintln!("pipeline: capture process gone, restarting");
                p.restart_capture().await;
                last_body = None;
                unchanged_since = None;
                continue;
            }

            // Slow path: process alive but manifest not advancing (e.g.
            // ffmpeg blocked on a dead capture pin instead of exiting).
            match fetch_media_playlist_body().await {
                Some(body) => {
                    if last_body.as_deref() == Some(body.as_str()) {
                        let since = *unchanged_since.get_or_insert_with(std::time::Instant::now);
                        if since.elapsed() >= STALL_THRESHOLD {
                            eprintln!(
                                "pipeline: manifest unchanged for {}s, restarting capture",
                                since.elapsed().as_secs()
                            );
                            p.restart_capture().await;
                            unchanged_since = None;
                        }
                    } else {
                        unchanged_since = None;
                    }
                    last_body = Some(body);
                }
                None => {
                    // mediamtx itself not answering is mediamtx's own
                    // supervisor's problem, not capture's -- don't also
                    // restart capture off a signal capture can't fix.
                    last_body = None;
                    unchanged_since = None;
                }
            }
        }
    });
}

/// Resolves the master playlist's media playlist and returns its body, or
/// None if either fetch fails. Minimal hand-rolled HTTP/1.1 GET -- this is
/// two tiny localhost requests a second; pulling in a full HTTP client
/// dependency for that isn't worth it.
async fn fetch_media_playlist_body() -> Option<String> {
    let master = http_get_local(HLS_MASTER_PATH).await?;
    let media_path = master
        .lines()
        .rev()
        .find(|l| !l.is_empty() && !l.starts_with('#'))?;
    http_get_local(&format!("/cam/{media_path}")).await
}

async fn http_get_local(path: &str) -> Option<String> {
    let mut stream = TcpStream::connect("127.0.0.1:8888").await.ok()?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.ok()?;
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut buf))
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&buf);
    let (status_line, rest) = text.split_once("\r\n")?;
    if !status_line.contains(" 200 ") {
        return None;
    }
    let body = rest.split_once("\r\n\r\n")?.1;
    Some(body.to_string())
}

// --------------------------------------------------------- ffmpeg commands

/// Anchored on the loopback publisher inside broadcast-api's _writer_loop --
/// that's the process actually producing today's live RTSP output on tina/
/// tanzania (ffmpeg-cam.service is installed, then immediately deleted and
/// masked by hls-livecam-setup; it does not run in steady state). Its encode
/// parameters are identical to ffmpeg-cam.service's ExecStart anyway
/// (libx264/ultrafast/zerolatency/high/4.0/1500k/g=FR*4), so there's no
/// ambiguity about which one to match -- only the input side differs
/// (dshow direct capture here vs. a v4l2 loopback relay there), which
/// doesn't touch what ends up in the HLS segments.
fn capture_command(ffmpeg: &PathBuf, device_name: &str) -> Command {
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "dshow",
        "-vcodec",
        "mjpeg",
        "-video_size",
        "1280x720",
        "-framerate",
        "30",
        "-i",
    ])
    .arg(format!("video={device_name}"))
    .args([
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-tune",
        "zerolatency",
        "-vf",
        "fps=15,format=yuv420p",
        "-profile:v",
        "high",
        "-level",
        "4.0",
        "-b:v",
        "1500k",
        "-g",
        "60",
        "-rtsp_transport",
        "tcp",
        "-f",
        "rtsp",
        RTSP_URL,
    ]);
    cmd
}

/// Ported verbatim from pkg/etc/systemd/system/ffmpeg-cam-dark.service.
/// That unit is never enabled by hls-livecam-setup -- confirmed by reading
/// setup end to end, it's fossil config, same class as the nginx conf.d
/// file from run 1. There is no live node to compare Hide's manifest
/// characteristics against as a result (r=30 here vs. the real 15fps a
/// consumer sees from Show -- that's inherited from the unit file, not a
/// bug). See the run-2 report for the full note.
fn hide_command(ffmpeg: &PathBuf) -> Command {
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=black:s=1280x720:r=30",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-tune",
        "zerolatency",
        "-vf",
        "format=yuv420p",
        "-profile:v",
        "high",
        "-level",
        "4.0",
        "-b:v",
        "500k",
        "-g",
        "60",
        "-rtsp_transport",
        "tcp",
        "-f",
        "rtsp",
        RTSP_URL,
    ]);
    cmd
}

fn spawn(mut cmd: Command) -> std::io::Result<Child> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn()
}

// --------------------------------------------------------- device enumeration

/// Equivalent of v4l2-ctl --list-devices. Parses ffmpeg's dshow device-list
/// dump off stderr -- there's no structured output mode, this is how every
/// ffmpeg dshow enumeration tool does it.
async fn list_dshow_video_devices(ffmpeg: &PathBuf) -> Vec<String> {
    let output = Command::new(ffmpeg)
        .args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await;

    let Ok(output) = output else { return Vec::new() };
    let text = String::from_utf8_lossy(&output.stderr);

    // Lines look like:  [in#0 @ 0x...] "Device Name" (video)
    text.lines()
        .filter(|l| l.trim_end().ends_with("(video)"))
        .filter_map(|l| {
            let start = l.find('"')?;
            let rest = &l[start + 1..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .collect()
}

/// device.txt in the state dir is this project's /etc/hls-livecam/device.env
/// -- a persisted friendly name, consumed the same way on every restart.
/// Re-enumerated (not just trusted) each time we need to start Show, since
/// friendly names are stable across replug but the set of attached cameras
/// can change between runs.
async fn load_or_pick_device(ffmpeg: &PathBuf, state: &AppState) -> String {
    let persisted = std::fs::read_to_string(state.dir().join("device.txt"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let devices = list_dshow_video_devices(ffmpeg).await;

    if let Some(name) = persisted {
        if devices.iter().any(|d| d == &name) {
            return name;
        }
        eprintln!(
            "pipeline: configured device {name:?} not currently present; devices seen: {devices:?}"
        );
        return name; // still try it -- ffmpeg's own error is the honest signal
    }

    match devices.first() {
        Some(first) => {
            let _ = std::fs::write(state.dir().join("device.txt"), first);
            println!("pipeline: no device configured, selected first found: {first:?}");
            first.clone()
        }
        None => {
            eprintln!("pipeline: no dshow video devices found");
            String::new()
        }
    }
}
