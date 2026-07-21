// Release builds are a GUI subsystem app (no console window pops up on
// launch); debug builds keep the console so stdout/stderr stay visible
// while developing. Because release silences stdout/stderr, the launch
// path also logs to %APPDATA%\hls-livecam-win\launch.log (see
// launch_log) so a silent startup failure is still diagnosable.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! hls-livecam-win -- native Windows camera node + operator dashboard.
//!
//! Run 3 scope: a native egui/eframe operator window (no webview, no HTML
//! anywhere in the GUI), autostart, and binary bundling. Builds on runs
//! 1-2 (HTTP control-plane + capture pipeline), unregressed.
//!
//! Architecture note (PM direction mid-run-3): the GUI and the run-1/2
//! server do NOT need to be decoupled over a network hop just because
//! that's how camdash and the Linux stack relate -- on 7elwe they're the
//! same desktop, so this is one process. A background OS thread owns a
//! tokio runtime running the axum server and the capture pipeline; the
//! main thread owns eframe's blocking native-window event loop. Button
//! clicks in the GUI call the same AppState/Pipeline methods the HTTP
//! handlers call, directly -- no loopback HTTP client, no second copy of
//! any logic. The external :80/:8888 contract is unchanged; only the
//! GUI's own path to driving it got shorter.
//!
//! This process runs elevated (see build.rs). PM decision, so DISK/SMART
//! can read Get-StorageReliabilityCounter -- that WMI class denies a
//! standard token. Windows enforces the manifest at launch (there is no
//! code path that runs before UAC has already granted an admin token),
//! so by the time main() executes, everything below -- binding :80,
//! spawning ffmpeg/mediamtx, registering autostart -- already has admin
//! rights it didn't strictly need before. Autostart correspondingly
//! switched from a Registry Run key to a Scheduled Task (see
//! autostart.rs) -- a Run-key launch of a requireAdministrator exe still
//! prompts UAC every login; a task set to run with highest privileges
//! doesn't.

mod assets;
mod autostart;
mod binaries;
mod cams;
mod diskhealth;
mod gui;
mod metrics;
mod pipeline;
mod routes;
mod state;
mod tray;
mod video_preview;
mod winproc;

use std::sync::Arc;

fn main() {
    let ffmpeg = match binaries::resolve_ffmpeg() {
        Ok(p) => p,
        Err(e) => {
            launch_log(&format!("fatal: {e}"));
            std::process::exit(1);
        }
    };
    let mediamtx = match binaries::resolve_mediamtx() {
        Ok(p) => p,
        Err(e) => {
            launch_log(&format!("fatal: {e}"));
            std::process::exit(1);
        }
    };
    launch_log(&format!("ffmpeg   : {}", ffmpeg.display()));
    launch_log(&format!("mediamtx : {}", mediamtx.display()));

    match autostart::ensure_installed() {
        Ok(true) => launch_log("autostart: registered"),
        Ok(false) => launch_log("autostart: already registered"),
        Err(e) => launch_log(&format!("autostart: could not register ({e}) -- continuing without it")),
    }

    // Handoff from the background server thread to the GUI (main) thread:
    // state/pipeline/a runtime handle, once the async bootstrap completes.
    let (tx, rx) = std::sync::mpsc::channel();
    let ffmpeg_for_video = ffmpeg.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let state = Arc::new(state::AppState::load());
            launch_log(&format!("state dir : {}", state.dir().display()));

            let pipeline = pipeline::Pipeline::start(ffmpeg, mediamtx, state.clone()).await;
            let video_frame = video_preview::spawn(ffmpeg_for_video);

            let handle = tokio::runtime::Handle::current();
            if tx.send((state.clone(), pipeline.clone(), handle, video_frame)).is_err() {
                return; // GUI thread gone before we finished booting
            }

            let ctx = Arc::new(routes::Ctx { state, pipeline });
            let bind = std::env::var("HLS_BIND").unwrap_or_else(|_| "0.0.0.0:80".to_string());
            launch_log(&format!("binding   : {bind}"));

            let listener = match tokio::net::TcpListener::bind(&bind).await {
                Ok(l) => l,
                Err(e) => {
                    launch_log(&format!(
                        "fatal: cannot bind {bind}: {e} -- another process is probably holding \
                         the port (check: netstat -ano | findstr :80)"
                    ));
                    std::process::exit(1);
                }
            };
            launch_log("listening. viewer: http://localhost/  HLS: http://localhost:8888/cam/index.m3u8");

            if let Err(e) = axum::serve(listener, routes::router(ctx)).await {
                launch_log(&format!("fatal: server stopped: {e}"));
                std::process::exit(1);
            }
        });
    });

    let (state, pipeline, rt_handle, video_frame) = match rx.recv() {
        Ok(v) => v,
        Err(_) => {
            launch_log("fatal: server thread failed to start");
            std::process::exit(1);
        }
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Floor derived from the layout's content-driven panel
            // heights (layout.rs): below ~800px of window height the
            // right column cannot hold SYSTEM + PROCESSES + a usable
            // MESSAGE panel, and below ~1100px width the fixed side
            // columns squeeze the feed under its 400px floor.
            .with_inner_size([1240.0, 840.0])
            .with_min_inner_size([1100.0, 800.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Webcam Server Stack",
        native_options,
        Box::new(move |cc| {
            // Fonts MUST be installed here, before the first frame:
            // set_fonts() mid-frame only takes effect the *next* frame,
            // and egui panics outright when layout asks for a named
            // family (segoe_semibold) that isn't bound yet -- which is
            // exactly frame 1 if installation waits until update()
            // (crashed on launch; caught from Ron's console screenshot).
            gui::init_fonts(&cc.egui_ctx);
            // Built here, not earlier in main(): Windows tray APIs are
            // thread-affine like window handles, so this has to happen on
            // the thread eframe's event loop actually runs on.
            let tray = tray::build();
            if tray.is_none() {
                launch_log("tray: could not create a tray icon -- window minimize/close still work normally");
            }
            Ok(Box::new(gui::App::new(state, pipeline, rt_handle, video_frame, tray)))
        }),
    );

    if let Err(e) = result {
        launch_log(&format!("fatal: GUI failed: {e}"));
        std::process::exit(1);
    }
}

/// Launch-path logging. Release builds are a GUI subsystem app with no
/// console, so println!/eprintln! go nowhere -- without this, a silent
/// startup failure (missing binary, port already held, GUI init error)
/// would leave the user with a window that never appears and no clue why.
/// Every line also prints to stdout, which is visible under a debug build
/// or when launched from a terminal. The log is truncated once per
/// process (Once) so it holds just the current session, not an unbounded
/// history.
fn launch_log(msg: &str) {
    println!("{msg}");
    let Ok(appdata) = std::env::var("APPDATA") else {
        return;
    };
    let path = std::path::PathBuf::from(appdata)
        .join("hls-livecam-win")
        .join("launch.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    static LOG_INIT: std::sync::Once = std::sync::Once::new();
    LOG_INIT.call_once(|| {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = std::fs::write(&path, format!("=== launch (unix {epoch}) ===\n"));
    });
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{msg}");
    }
}

use eframe::egui;
