//! The HTTP surface.
//!
//! Fidelity notes -- this file exists to be indistinguishable from a Linux
//! node's :80, so the odd-looking bits are deliberate:
//!
//!   * CORS is on broadcast.txt and cams.json ONLY -- NOT buzz.txt, and not
//!     /api/ (a bare proxy_pass). This was verified against live nodes rather
//!     than taken from the repo: pkg/etc/nginx/conf.d/hls-livecam.conf claims
//!     buzz.txt carries Access-Control-Allow-Origin, but hls-livecam-setup --
//!     which generates the config that actually deploys -- emits no buzz.txt
//!     block at all, and neither tina (nginx/1.28.3) nor tanzania (1.30.3)
//!     sends the header. Nothing fetches buzz.txt cross-origin anyway;
//!     cams.html only reaches for broadcast.txt. Match the live fleet.
//!   * GET /index.html serves the page, same bytes and same headers as GET /.
//!     The repo's conf.d file implies a 404 here (its catch-all is
//!     `location / { return 404; }`), but that file is the same dead config
//!     described above; tina and tanzania both answer 200. Verified by
//!     fetching both paths from a live node and comparing -- identical.
//!   * /api/ replies carry Flask's default text/html; charset=utf-8, not
//!     text/plain, because they are bare `return "string", 200` values.
//!   * POST /api/broadcast answers 204 with an empty body, not 200.
//!   * The no-cache header block is only on the locations nginx puts it on;
//!     /cams/ deliberately lacks it.

use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use std::sync::Arc;

use crate::assets;
use crate::pipeline::Pipeline;
use crate::state::{is_valid_mode, AppState};

/// Bundles the two things a handler might need. Most only touch state;
/// only feed-mode also has to reach the pipeline to actually drive a swap.
pub struct Ctx {
    pub state: Arc<AppState>,
    pub pipeline: Arc<Pipeline>,
}

/// Flask's content type for a bare `return "text", 200`.
const FLASK_TEXT: &str = "text/html; charset=utf-8";

const NGINX_404: &str = "<html>\r\n<head><title>404 Not Found</title></head>\r\n<body>\r\n<center><h1>404 Not Found</h1></center>\r\n<hr><center>nginx</center>\r\n</body>\r\n</html>\r\n";

const NGINX_301: &str = "<html>\r\n<head><title>301 Moved Permanently</title></head>\r\n<body>\r\n<center><h1>301 Moved Permanently</h1></center>\r\n<hr><center>nginx</center>\r\n</body>\r\n</html>\r\n";

/// Werkzeug's abort(400) page, as broadcast-api would emit it.
const FLASK_400: &str = "<!doctype html>\n<html lang=en>\n<title>400 Bad Request</title>\n<h1>Bad Request</h1>\n<p>The browser (or proxy) sent a request that this server could not understand.</p>\n";

const FLASK_404: &str = "<!doctype html>\n<html lang=en>\n<title>404 Not Found</title>\n<h1>Not Found</h1>\n<p>The requested URL was not found on the server. If you entered the URL manually please check your spelling and try again.</p>\n";

pub fn router(ctx: Arc<Ctx>) -> Router {
    Router::new()
        // -- served straight off disk by nginx on Linux --
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/broadcast.txt", get(broadcast_txt))
        .route("/buzz.txt", get(buzz_txt))
        .route("/dark.png", get(dark_png))
        .route("/cams", get(cams_redirect))
        .route("/cams/", get(cams_html))
        .route("/cams/cams.html", get(cams_html))
        .route("/cams/cams.json", get(cams_json))
        // -- proxied to broadcast-api on Linux --
        .route("/api/info", get(api_info))
        .route("/api/broadcast", post(api_broadcast))
        .route("/api/buzz", post(api_buzz))
        .route("/api/feed-mode", get(feed_mode_get).post(feed_mode_post))
        .route("/api/msg-lock", get(msg_lock_get).post(msg_lock_post))
        .route("/api/bw-mode", get(bw_mode_get).post(bw_mode_post))
        .route("/api/dark", get(dark_get).post(dark_post))
        // An unknown /api/ path reaches Flask and gets Flask's 404 page;
        // anything else is refused by nginx itself. Different bodies.
        .route("/api/{*rest}", get(flask_not_found).post(flask_not_found))
        .fallback(nginx_not_found)
        .with_state(ctx)
}

fn build(status: StatusCode, ctype: &str, body: Vec<u8>, cors: bool, nocache: bool) -> Response {
    let mut b = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, ctype);
    if nocache {
        b = b
            .header(
                header::CACHE_CONTROL,
                "no-store, no-cache, must-revalidate, max-age=0",
            )
            .header(header::PRAGMA, "no-cache");
    }
    if cors {
        b = b.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    }
    b.body(Body::from(body)).unwrap()
}

/// A bare string return from a Flask view.
fn flask_text(s: String) -> Response {
    build(StatusCode::OK, FLASK_TEXT, s.into_bytes(), false, false)
}

fn bool_text(v: bool) -> Response {
    flask_text(if v { "true".into() } else { "false".into() })
}

// ---------------------------------------------------------------- static

async fn index() -> Response {
    build(
        StatusCode::OK,
        "text/html",
        assets::index_html().as_bytes().to_vec(),
        false,
        true,
    )
}

async fn cams_html() -> Response {
    // location /cams carries no cache-control block in the nginx config.
    build(
        StatusCode::OK,
        "text/html",
        assets::cams_html().as_bytes().to_vec(),
        false,
        false,
    )
}

async fn cams_redirect() -> Response {
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(header::LOCATION, "/cams/")
        .header(header::CONTENT_TYPE, "text/html")
        .body(Body::from(NGINX_301))
        .unwrap()
}

async fn cams_json(State(ctx): State<Arc<Ctx>>) -> Response {
    build(
        StatusCode::OK,
        "application/json",
        ctx.state.cams_json().into_bytes(),
        true,
        true,
    )
}

async fn broadcast_txt(State(ctx): State<Arc<Ctx>>) -> Response {
    let msg = ctx.state.message.lock().unwrap().clone();
    build(StatusCode::OK, "text/plain", msg.into_bytes(), true, true)
}

async fn buzz_txt(State(ctx): State<Arc<Ctx>>) -> Response {
    let ts = ctx.state.buzz.lock().unwrap().clone();
    // No CORS here on purpose -- see module docs.
    build(StatusCode::OK, "text/plain", ts.into_bytes(), false, true)
}

async fn dark_png(State(ctx): State<Arc<Ctx>>) -> Response {
    match ctx.state.dark_png() {
        Some(bytes) => build(StatusCode::OK, "image/png", bytes, false, true),
        // No cloak image generated yet -- nginx 404s the same way.
        None => nginx_not_found().await,
    }
}

// ------------------------------------------------------------------- api

async fn api_broadcast(State(ctx): State<Arc<Ctx>>, body: String) -> Response {
    // Python: request.get_data(as_text=True).strip()[:MAX_LEN]
    // [:120] slices characters, not bytes, so take() over chars.
    let msg: String = body.trim().chars().take(120).collect();
    match ctx.state.set_message(&msg) {
        Ok(()) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header(header::CONTENT_TYPE, FLASK_TEXT)
            .body(Body::empty())
            .unwrap(),
        Err(_) => build(
            StatusCode::INTERNAL_SERVER_ERROR,
            FLASK_TEXT,
            Vec::new(),
            false,
            false,
        ),
    }
}

async fn api_buzz(State(ctx): State<Arc<Ctx>>) -> Response {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string());
    match ctx.state.set_buzz(&ts) {
        Ok(()) => flask_text(ts),
        Err(_) => build(
            StatusCode::INTERNAL_SERVER_ERROR,
            FLASK_TEXT,
            Vec::new(),
            false,
            false,
        ),
    }
}

async fn feed_mode_get(State(ctx): State<Arc<Ctx>>) -> Response {
    let m = ctx.state.feed_mode.lock().unwrap().clone();
    flask_text(m)
}

async fn feed_mode_post(State(ctx): State<Arc<Ctx>>, body: String) -> Response {
    let mode = body.trim().to_string();
    if !is_valid_mode(&mode) {
        return build(
            StatusCode::BAD_REQUEST,
            FLASK_TEXT,
            FLASK_400.as_bytes().to_vec(),
            false,
            false,
        );
    }
    // Persist first (contract-visible immediately even if the swap is
    // still in flight), then drive the actual source swap. "cloak" fails
    // safe to the same black source as "hide" this run -- see pipeline.rs.
    ctx.state.set_feed_mode(&mode);
    ctx.pipeline.apply_feed_mode(&mode).await;
    flask_text(mode)
}

async fn msg_lock_get(State(ctx): State<Arc<Ctx>>) -> Response {
    let v = *ctx.state.msg_lock.lock().unwrap();
    bool_text(v)
}

async fn msg_lock_post(State(ctx): State<Arc<Ctx>>) -> Response {
    bool_text(ctx.state.toggle_msg_lock())
}

async fn bw_mode_get(State(ctx): State<Arc<Ctx>>) -> Response {
    let v = *ctx.state.bw_mode.lock().unwrap();
    bool_text(v)
}

async fn bw_mode_post(State(ctx): State<Arc<Ctx>>) -> Response {
    bool_text(ctx.state.toggle_bw_mode())
}

async fn dark_get(State(ctx): State<Arc<Ctx>>) -> Response {
    let v = *ctx.state.dark.lock().unwrap();
    bool_text(v)
}

async fn dark_post(State(ctx): State<Arc<Ctx>>) -> Response {
    bool_text(ctx.state.toggle_dark())
}

async fn api_info() -> Response {
    let (host, ts) = tokio::task::spawn_blocking(|| (hostname(), tailscale_ip()))
        .await
        .unwrap_or_else(|_| (String::new(), String::new()));

    // Flask's jsonify spacing, trailing newline included.
    let body = format!(
        "{{\"hostname\": \"{}\", \"tailscale\": \"{}\"}}\n",
        json_escape(&host),
        json_escape(&ts)
    );
    build(
        StatusCode::OK,
        "application/json",
        body.into_bytes(),
        false,
        false,
    )
}

// --------------------------------------------------------------- 404s

async fn nginx_not_found() -> Response {
    build(
        StatusCode::NOT_FOUND,
        "text/html",
        NGINX_404.as_bytes().to_vec(),
        false,
        false,
    )
}

async fn flask_not_found() -> Response {
    build(
        StatusCode::NOT_FOUND,
        FLASK_TEXT,
        FLASK_404.as_bytes().to_vec(),
        false,
        false,
    )
}

// -------------------------------------------------------------- helpers

fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

/// Mirrors broadcast-api: first 100.x.x.x address found, empty string if the
/// lookup fails for any reason.
fn tailscale_ip() -> String {
    const CANDIDATES: [&str; 2] = ["tailscale", r"C:\Program Files\Tailscale\tailscale.exe"];
    for exe in CANDIDATES {
        if let Ok(out) = std::process::Command::new(exe).args(["ip", "-4"]).output() {
            if out.status.success() {
                if let Ok(s) = String::from_utf8(out.stdout) {
                    for line in s.lines() {
                        let t = line.trim();
                        if t.starts_with("100.") {
                            return t.to_string();
                        }
                    }
                }
            }
        }
    }
    String::new()
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
