//! hls-livecam-win -- native Windows camera node.
//!
//! Run 1 scope: the control-plane HTTP surface and the web assets, nothing
//! else. There is no capture pipeline yet, so the viewer's player will sit
//! dead at "connecting" -- that is expected until run 2 brings up ffmpeg and
//! mediamtx. feed-mode is recorded and reported but drives nothing.
//!
//! Binds :80 because cams.html on a peer fetches http://<ip>/broadcast.txt
//! with no port. Binding a low port needs no elevation on Windows.

mod assets;
mod routes;
mod state;

use std::sync::Arc;

#[tokio::main]
async fn main() {
    let bind = std::env::var("HLS_BIND").unwrap_or_else(|_| "0.0.0.0:80".to_string());
    let state = Arc::new(state::AppState::load());

    println!("hls-livecam-win (run 1: control-plane only)");
    println!("  state dir : {}", state.dir().display());
    println!("  binding   : {bind}");

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

    if let Err(e) = axum::serve(listener, routes::router(state)).await {
        eprintln!("error: server stopped: {e}");
        std::process::exit(1);
    }
}
