//! hls-livecam-win -- native Windows camera node.
//!
//! Run 2 scope: live capture from a dshow camera, through mediamtx, out as
//! HLS on the fleet path, self-healing when the camera drops, and a working
//! Hide (source-swap to black). No tray icon, no autostart, no dashboard
//! GUI, no installer -- that's run 3. No Cloak/blur/bw-mode pipeline --
//! that's the deferred raw-frame-pipe work; "cloak" fails safe to the same
//! black source as "hide" until then (see pipeline.rs / the run-2 report).
//!
//! Binds :80 because cams.html on a peer fetches http://<ip>/broadcast.txt
//! with no port. Binding a low port needs no elevation on Windows.

mod assets;
mod binaries;
mod pipeline;
mod routes;
mod state;

use std::sync::Arc;

#[tokio::main]
async fn main() {
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

    let state = Arc::new(state::AppState::load());
    println!("state dir : {}", state.dir().display());

    let pipeline = pipeline::Pipeline::start(ffmpeg, mediamtx, state.clone()).await;
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
}
