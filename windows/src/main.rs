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
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let mediamtx = match binaries::resolve_mediamtx() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("ffmpeg   : {}", ffmpeg.display());
    println!("mediamtx : {}", mediamtx.display());

    match autostart::ensure_installed() {
        Ok(true) => println!("autostart: registered"),
        Ok(false) => println!("autostart: already registered"),
        Err(e) => eprintln!("autostart: could not register ({e}) -- continuing without it"),
    }

    // Handoff from the background server thread to the GUI (main) thread:
    // state/pipeline/a runtime handle, once the async bootstrap completes.
    let (tx, rx) = std::sync::mpsc::channel();
    let ffmpeg_for_video = ffmpeg.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let state = Arc::new(state::AppState::load());
            println!("state dir : {}", state.dir().display());

            let pipeline = pipeline::Pipeline::start(ffmpeg, mediamtx, state.clone()).await;
            let video_frame = video_preview::spawn(ffmpeg_for_video);

            let handle = tokio::runtime::Handle::current();
            if tx.send((state.clone(), pipeline.clone(), handle, video_frame)).is_err() {
                return; // GUI thread gone before we finished booting
            }

            let ctx = Arc::new(routes::Ctx { state, pipeline });
            let bind = std::env::var("HLS_BIND").unwrap_or_else(|_| "0.0.0.0:80".to_string());
            println!("binding   : {bind}");

            let listener = match tokio::net::TcpListener::bind(&bind).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("error: cannot bind {bind}: {e}");
                    eprintln!("       another process is probably holding the port.");
                    eprintln!("       check with: netstat -ano | findstr :80");
                    std::process::exit(1);
                }
            };
            println!("listening. viewer: http://localhost/  aggregator: http://localhost/cams/");
            println!("HLS: http://localhost:8888/cam/index.m3u8");

            if let Err(e) = axum::serve(listener, routes::router(ctx)).await {
                eprintln!("error: server stopped: {e}");
                std::process::exit(1);
            }
        });
    });

    let (state, pipeline, rt_handle, video_frame) = match rx.recv() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("error: server thread failed to start");
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
                eprintln!("tray: could not create a tray icon -- window minimize/close still work normally");
            }
            Ok(Box::new(gui::App::new(state, pipeline, rt_handle, video_frame, tray)))
        }),
    );

    if let Err(e) = result {
        eprintln!("error: GUI failed: {e}");
        std::process::exit(1);
    }
}

use eframe::egui;
