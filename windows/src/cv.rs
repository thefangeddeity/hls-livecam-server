//! CV Phase 1: detection telemetry from a supervised Python sidecar.
//!
//! The fleet's CV stack is 5,000 lines of measured, tuned Python against
//! cv2 -- reimplementing it in Rust would take months and diverge from
//! the other nodes immediately. So this does what the codebase already
//! does for ffmpeg and mediamtx: runs it as a child process and
//! supervises it. Ron's own framing for the split -- Rust for the GUI,
//! Python for server plumbing.
//!
//! Strictly optional, by construction. If Python or the model is
//! missing, this logs once and stays dark; every other faculty on the
//! node is unaffected. Nothing in the capture or publish path depends on
//! it -- the sidecar is a READ-ONLY consumer of the same RTSP loopback
//! video_preview.rs taps.
//!
//! IPC is line-delimited JSON on the child's stdout, which is the shape
//! video_preview.rs already established for talking to a child (it reads
//! raw frames the same way). One mechanism, not two.
//!
//! CV exists ONLY while the picture is in CV mode, matching Tanzania:
//! there, detection and the HUD are side effects of the render, not an
//! independent service. So the sidecar is spawned on entering `cv` and
//! killed on leaving it, and telemetry reads dark the rest of the time.
//! An earlier build ran detection continuously in the background; that
//! was a local invention, and it also paid ~130% of a core permanently
//! for readings nobody was looking at.
//!
//! One unavoidable divergence from Tanzania, worth stating plainly: its
//! Python owns the camera device directly and mutates frames in-process,
//! so CV Mode simply changes what the single publisher emits. Here
//! ffmpeg owns the device and publishes to RTSP, so the sidecar has to
//! read from somewhere -- it consumes /cam and publishes its render to a
//! SECOND path, /cv, which the viewer plays instead. The raw camera
//! therefore keeps publishing throughout CV Mode. Concealment is Hide's
//! job, not CV's, so that is acceptable -- but it is a real difference,
//! not an implementation detail.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Embedded rather than installed beside the exe: the scripts then
/// cannot drift from the binary that drives them, and the installer has
/// one less payload to place. Same one-source-of-truth reasoning as
/// assets.rs's include_str! of the web page, and the same pkg/ tree, so
/// a Linux node and this node run byte-identical detector code.
const CV_DETECT_PY: &str =
    include_str!("../../pkg/usr/share/hls-livecam-server/cv_detect.py");
const CV_SIDECAR_PY: &str =
    include_str!("../../pkg/usr/share/hls-livecam-server/cv_sidecar.py");
/// CV Mode's enhancement stack. cv_processor imports the rest, each in
/// its own try/except, so a missing one disables that faculty instead of
/// breaking the pipeline -- but they are all shipped, because a silently
/// degraded renderer is worse than an absent one. cv_scene_register is
/// deliberately absent: it is a standalone CLI, not part of the loop.
const CV_PROCESSOR_PY: &str =
    include_str!("../../pkg/usr/share/hls-livecam-server/cv_processor.py");
const CV_SCENE_PY: &str =
    include_str!("../../pkg/usr/share/hls-livecam-server/cv_scene.py");
const CV_PERSIST_PY: &str =
    include_str!("../../pkg/usr/share/hls-livecam-server/cv_persist.py");
const CV_OCCUPANCY_PY: &str =
    include_str!("../../pkg/usr/share/hls-livecam-server/cv_occupancy.py");
const CV_NOTIFY_PY: &str =
    include_str!("../../pkg/usr/share/hls-livecam-server/cv_notify.py");

/// A reading older than this counts as stale and the lamps go dark --
/// same idea as CVProcessor.state(fresh=2.0): a wedged sidecar must not
/// leave telemetry lit over a frozen picture. Generous relative to the
/// default 2/s cadence so ordinary jitter never blinks it.
const FRESH: Duration = Duration::from_secs(5);
const RESTART_BACKOFF: Duration = Duration::from_secs(3);

pub struct Cv {
    latest: StdMutex<Option<Value>>,
    at: StdMutex<Option<Instant>>,
    /// False when the sidecar could not be started at all (no Python, no
    /// model). Distinct from "running but reporting nothing".
    enabled: AtomicBool,
    /// Read in state() so telemetry goes dark the INSTANT the picture
    /// leaves CV mode, rather than lingering for the staleness window
    /// while the child is still winding down.
    state: StdMutex<Option<Arc<crate::state::AppState>>>,
}

impl Cv {
    /// Never fails: a node that cannot run CV is a node with CV dark,
    /// not a node that fails to boot.
    pub fn start(
        state_dir: PathBuf,
        state: Arc<crate::state::AppState>,
        ffmpeg: PathBuf,
    ) -> Arc<Self> {
        let cv = Arc::new(Self {
            latest: StdMutex::new(None),
            at: StdMutex::new(None),
            enabled: AtomicBool::new(false),
            state: StdMutex::new(Some(state.clone())),
        });

        let python = match resolve_python() {
            Some(p) => p,
            None => {
                crate::launch_log(
                    "cv: no python (py.exe/python.exe) on PATH or HLS_PYTHON -- CV stays dark",
                );
                return cv;
            }
        };
        let model = match resolve_model(&state_dir) {
            Some(m) => m,
            None => {
                crate::launch_log(
                    "cv: yolov8n.onnx not found (set HLS_CV_MODEL, or place it in \
                     <exe>\\models or <state>\\models) -- CV stays dark",
                );
                return cv;
            }
        };
        let script = match extract_scripts(&state_dir) {
            Ok(s) => s,
            Err(e) => {
                crate::launch_log(&format!("cv: could not write sidecar scripts: {e} -- CV stays dark"));
                return cv;
            }
        };

        crate::launch_log(&format!(
            "cv: {} {} (model {})",
            python.display(),
            script.display(),
            model.display()
        ));
        cv.enabled.store(true, Ordering::Relaxed);
        spawn_supervisor(cv.clone(), python, script, model, state, ffmpeg);
        cv
    }

    /// The /api/cv-state body. Mirrors broadcast-api's own shape --
    /// mog2/gated/scene_registered/scene_stale/text/capability_text --
    /// so a client written against a Linux node reads this unchanged.
    ///
    /// Absent or stale telemetry reports everything off, never
    /// unknown-but-probably-fine (broadcast-api's words, same rule).
    pub fn state(&self) -> Value {
        // Not in CV mode means not seeing, so report nothing rather than
        // the last thing seen. Matches Tanzania, where the telemetry does
        // not exist outside the render.
        let in_cv = self
            .state
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.feed_mode.lock().unwrap().as_str() == "cv")
            .unwrap_or(false);
        if in_cv && self.enabled.load(Ordering::Relaxed) {
            let fresh = self
                .at
                .lock()
                .unwrap()
                .map(|t| t.elapsed() < FRESH)
                .unwrap_or(false);
            if fresh {
                if let Some(v) = self.latest.lock().unwrap().clone() {
                    return v;
                }
            }
        }
        json!({
            "mog2": false,
            "gated": false,
            "scene_registered": false,
            "scene_stale": false,
            "text": "",
            "capability_text": "",
        })
    }
}

fn spawn_supervisor(
    cv: Arc<Cv>,
    python: PathBuf,
    script: PathBuf,
    model: PathBuf,
    state: Arc<crate::state::AppState>,
    ffmpeg: PathBuf,
) {
    std::thread::spawn(move || loop {
        // CV runs only while the picture is in CV mode. Outside it there
        // is no child at all -- no detector, no encoder, no cost.
        if state.feed_mode.lock().unwrap().as_str() != "cv" {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }

        let mut cmd = Command::new(&python);
        cmd.arg(&script)
            .arg("--model")
            .arg(&model)
            .arg("--publish")
            .arg("--ffmpeg")
            .arg(&ffmpeg);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            // The sidecar's stderr is diagnostics (connect/reconnect
            // notices); it would otherwise land in a console this
            // process does not have.
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(crate::winproc::CREATE_NO_WINDOW);
        }

        match cmd.spawn() {
            Ok(mut child) => {
                // Watchdog: ends the child when the feed mode stops
                // matching what it was started for. The read loop below
                // blocks on stdout, so the flip cannot be noticed there;
                // killing the child breaks that read and the outer loop
                // respawns with the right argv. Polls rather than being
                // pushed to, because a mode change is rare and a second
                // of latency on it is imperceptible.
                // `finished` is set by THIS thread once the child is
                // reaped, so the watchdog exits with it. Without that
                // shared flag a normally-exiting child would leave its
                // watchdog polling forever -- one leaked thread per
                // restart, which over a long uptime is not nothing.
                let finished = Arc::new(AtomicBool::new(false));
                let killer = Killer { pid: child.id(), done: finished.clone() };
                let st = state.clone();
                let watch = std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_millis(500));
                    if killer.done() {
                        return;
                    }
                    if st.feed_mode.lock().unwrap().as_str() != "cv" {
                        killer.kill();
                        return;
                    }
                });

                if let Some(out) = child.stdout.take() {
                    for line in BufReader::new(out).lines() {
                        let Ok(line) = line else { break };
                        if line.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<Value>(&line) {
                            Ok(v) => {
                                *cv.latest.lock().unwrap() = Some(v);
                                *cv.at.lock().unwrap() = Some(Instant::now());
                            }
                            // A malformed line is not worth killing the
                            // sidecar over; skip it and keep reading.
                            Err(_) => continue,
                        }
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
                finished.store(true, Ordering::Relaxed);
                let _ = watch.join();
            }
            Err(e) => {
                crate::launch_log(&format!("cv: failed to start sidecar: {e}"));
            }
        }
        // Reached on child exit or pipe close. Stale readings age out via
        // FRESH on their own, so there is nothing to clear here.
        std::thread::sleep(RESTART_BACKOFF);
    });
}

/// A kill handle for a spawned child, usable from the watchdog thread.
///
/// std's Child cannot be shared across threads for killing, so this holds
/// the raw OS pid and shells out to the same mechanism the rest of the
/// codebase uses. Crude, but it keeps ownership of the Child (and its
/// stdout) firmly in the reading thread.
struct Killer {
    pid: u32,
    done: Arc<AtomicBool>,
}

impl Killer {
    fn kill(&self) {
        self.done.store(true, Ordering::Relaxed);
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", &self.pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(crate::winproc::CREATE_NO_WINDOW);
        }
        let _ = cmd.status();
    }
    fn done(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }
}

/// Resolves the real interpreter, deliberately NOT the `py.exe` launcher.
///
/// py.exe is a shim that spawns python.exe as a CHILD, so supervising it
/// means supervising the wrong process: killing the shim on restart
/// leaves the interpreter (and its ~120 MB of loaded OpenCV and model)
/// running with no parent. Asking the launcher for `sys.executable` once
/// at startup gets the real path, and everything after that is a single
/// process this code actually owns. python.exe is not on PATH on this
/// box -- only the launcher is -- which is why the indirection exists at
/// all.
fn resolve_python() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HLS_PYTHON") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(direct) = which_on_path("python.exe") {
        return Some(direct);
    }
    let launcher = which_on_path("py.exe")?;
    let mut cmd = Command::new(&launcher);
    cmd.args(["-c", "import sys; print(sys.executable)"])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::winproc::CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
    if path.is_file() {
        Some(path)
    } else {
        // Better the shim than nothing -- CV still works, restarts just
        // leave a short-lived orphan that dies on its next blocked write.
        Some(launcher)
    }
}

fn resolve_model(state_dir: &PathBuf) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HLS_CV_MODEL") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("models").join("yolov8n.onnx"));
        }
    }
    candidates.push(state_dir.join("models").join("yolov8n.onnx"));
    candidates.into_iter().find(|p| p.is_file())
}

/// Rewritten on every start so the scripts always match this binary --
/// an upgrade must never leave a stale sidecar behind.
fn extract_scripts(state_dir: &PathBuf) -> std::io::Result<PathBuf> {
    let dir = state_dir.join("cv");
    std::fs::create_dir_all(&dir)?;
    for (name, body) in [
        ("cv_detect.py", CV_DETECT_PY),
        ("cv_processor.py", CV_PROCESSOR_PY),
        ("cv_scene.py", CV_SCENE_PY),
        ("cv_persist.py", CV_PERSIST_PY),
        ("cv_occupancy.py", CV_OCCUPANCY_PY),
        ("cv_notify.py", CV_NOTIFY_PY),
    ] {
        std::fs::write(dir.join(name), body)?;
    }
    let sidecar = dir.join("cv_sidecar.py");
    std::fs::write(&sidecar, CV_SIDECAR_PY)?;
    Ok(sidecar)
}

fn which_on_path(exe_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(exe_name))
        .find(|p| p.is_file())
}
