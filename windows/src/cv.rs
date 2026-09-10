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
//! Phase 2 -- CV Mode, where CV processes the published picture -- is
//! deliberately NOT here. That needs the frame path rebuilt and costs
//! ~122 ms/frame against a 66.7 ms budget on a node stronger than this
//! one; detection alone is a few frames a second and buys the telemetry
//! without touching what viewers see.

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
}

impl Cv {
    /// Never fails: a node that cannot run CV is a node with CV dark,
    /// not a node that fails to boot.
    pub fn start(state_dir: PathBuf) -> Arc<Self> {
        let cv = Arc::new(Self {
            latest: StdMutex::new(None),
            at: StdMutex::new(None),
            enabled: AtomicBool::new(false),
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
        spawn_supervisor(cv.clone(), python, script, model);
        cv
    }

    /// The /api/cv-state body. Mirrors broadcast-api's own shape --
    /// mog2/gated/scene_registered/scene_stale/text/capability_text --
    /// so a client written against a Linux node reads this unchanged.
    ///
    /// Absent or stale telemetry reports everything off, never
    /// unknown-but-probably-fine (broadcast-api's words, same rule).
    pub fn state(&self) -> Value {
        if self.enabled.load(Ordering::Relaxed) {
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

fn spawn_supervisor(cv: Arc<Cv>, python: PathBuf, script: PathBuf, model: PathBuf) {
    std::thread::spawn(move || loop {
        let mut cmd = Command::new(&python);
        cmd.arg(&script)
            .arg("--model")
            .arg(&model)
            .stdin(Stdio::null())
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
    std::fs::write(dir.join("cv_detect.py"), CV_DETECT_PY)?;
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
