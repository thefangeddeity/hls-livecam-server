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
//!   * /hls/ carries an EXTRA `Cache-Control: no-store` alongside whatever
//!     mediamtx's own header already says (nginx's `add_header ... always`
//!     ADDS rather than replaces, so the wire shape genuinely is two
//!     Cache-Control lines -- see proxy_hls). This one wasn't in the repo's
//!     conf.d file's intent either, it was just never carried into this
//!     proxy at all until "Unable to hear inbound audio from iOS" traced
//!     back to iOS Safari's HTTP cache serving a stale audio-only rendition
//!     without it. Verified against a live node the same way as the rest
//!     of this file's fidelity notes.

use axum::{
    body::{Body, Bytes},
    extract::{OriginalUri, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
    routing::{any, get, post},
    Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::assets;
use crate::notches::NotchError;
use crate::pipeline::Pipeline;
use crate::state::{is_valid_mode, AppState};
use crate::talk::Talk;

/// Bundles the two things a handler might need. Most only touch state;
/// only feed-mode also has to reach the pipeline to actually drive a swap.
pub struct Ctx {
    pub state: Arc<AppState>,
    pub pipeline: Arc<Pipeline>,
    pub talk: Arc<Talk>,
    pub cv: Arc<crate::cv::Cv>,
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
        .route("/brand.png", get(brand_png))
        .route("/vendor/hls.min.js", get(hls_min_js))
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
        .route("/api/foveal-mode", get(foveal_mode_get).post(foveal_mode_post))
        .route("/api/dark", get(dark_get).post(dark_post))
        .route(
            "/api/notches",
            get(notches_get).post(notches_post).delete(notches_delete).patch(notches_patch),
        )
        .route("/api/notches/sort", post(notches_sort))
        .route("/api/talk", get(talk_get).post(talk_post))
        .route("/api/pipeline", get(api_pipeline))
        .route("/api/cv-state", get(api_cv_state))
        .route(
            "/api/audio-settings",
            get(audio_settings_get).post(audio_settings_post),
        )
        // Same-origin reverse proxy to mediamtx -- see the proxy module
        // docs above proxy_hls. /hls mirrors broadcast-api's nginx
        // location verbatim; /talk and /cam are 7elwe's own WHIP/WHEP
        // paths (cam, not roomaudio -- see talk.rs's device-contention
        // note) proxied the same way for the same reason.
        .route("/hls/{*rest}", get(proxy_hls))
        .route("/talk/{*rest}", any(proxy_talk))
        .route("/cam/{*rest}", any(proxy_cam))
        // roomaudio.rs republishes /cam's audio as Opus for the two-way
        // WHEP inbound leg; proxy_cam's logic is already path-generic
        // (recomputes est from the real URI each call), so the same
        // handler proxies this path too without any change to it.
        .route("/roomaudio/{*rest}", any(proxy_cam))
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

/// The header logo + browser tab icon -- see assets::BRAND_PNG docs.
/// Static/embedded, unlike dark_png's on-disk file, so no None case.
async fn brand_png() -> Response {
    build(StatusCode::OK, "image/png", assets::BRAND_PNG.to_vec(), false, true)
}

async fn hls_min_js() -> Response {
    // "text/javascript", verified against a live Tanzania node rather than
    // assumed -- nginx's mime.types maps .js there, not
    // application/javascript, which was this function's first guess.
    build(StatusCode::OK, "text/javascript", assets::HLS_MIN_JS.to_vec(), false, true)
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
    match ctx.state.buzz_now() {
        Ok(ts) => flask_text(ts),
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
    let v = ctx.state.toggle_bw_mode();
    // Run 6: B&W is now a live filter modifier on Blur, not an inert
    // flag. Re-spawn the capture if Blur is active so the effect actually
    // changes; a no-op otherwise. The HTTP response (the new bool) is
    // unchanged -- contract preserved.
    ctx.pipeline.refresh_cloak().await;
    bool_text(v)
}

async fn foveal_mode_get(State(ctx): State<Arc<Ctx>>) -> Response {
    bool_text(ctx.cv.get_foveal())
}

async fn foveal_mode_post(State(ctx): State<Arc<Ctx>>, body: String) -> Response {
    let enabled = body.trim() == "true";
    ctx.cv.set_foveal(enabled);
    bool_text(enabled)
}

async fn dark_get(State(ctx): State<Arc<Ctx>>) -> Response {
    let v = *ctx.state.dark.lock().unwrap();
    bool_text(v)
}

async fn dark_post(State(ctx): State<Arc<Ctx>>) -> Response {
    bool_text(ctx.state.toggle_dark())
}

// ----------------------------------------------------------------- talk
//
// See talk.rs module docs. GET doubles as the client's 4s heartbeat poll;
// POST's body is the app-level signal ("two-way" starts/refreshes a call,
// anything else -- including the explicit "false" hangup -- ends it).
// Both directions of the actual audio (WHIP publish, WHEP subscribe) talk
// straight to mediamtx and never reach this route.

async fn talk_get(State(ctx): State<Arc<Ctx>>) -> Response {
    bool_text(ctx.talk.poll())
}

async fn talk_post(State(ctx): State<Arc<Ctx>>, body: String) -> Response {
    bool_text(ctx.talk.set(&body))
}

// -------------------------------------------------- audio settings
//
// High-pass / low-pass / gain, the Settings panel's "Audio filters"
// block. Values are clamped server-side and the ACCEPTED value is
// echoed back, so overshooting a bound snaps visibly in the field
// rather than silently doing nothing. See audio_settings.rs for the
// per-key bounds and why the outbound gain's ceiling is lower.

async fn audio_settings_get(State(ctx): State<Arc<Ctx>>) -> Response {
    let body = serde_json::to_vec(&ctx.state.audio.all()).unwrap_or_default();
    build(StatusCode::OK, "application/json", body, false, false)
}

async fn audio_settings_post(State(ctx): State<Arc<Ctx>>, body: String) -> Response {
    let Some(obj) = parse_json_body(&body).and_then(|v| v.as_object().cloned()) else {
        return bad_request();
    };

    let mut accepted = serde_json::Map::new();
    let mut room_dirty = false;
    for (key, value) in obj.iter() {
        match ctx.state.audio.set(key, value) {
            Ok(v) => {
                accepted.insert(key.clone(), v);
                room_dirty |= crate::audio_settings::AudioSettings::affects_room(key);
            }
            Err(()) => return bad_request(),
        }
    }

    // Only the room/HLS-facing keys need the publisher restarted. A
    // talk-only tweak (outbound gain, speaker mute) restarting it too
    // would bounce HLS audio for every listener for no reason -- a real
    // bug hit on Tanzania, avoided here by scoping the trigger.
    if room_dirty {
        ctx.pipeline.reload_audio().await;
    }

    let body = serde_json::to_vec(&Value::Object(accepted)).unwrap_or_default();
    build(StatusCode::OK, "application/json", body, false, false)
}

// ------------------------------------------------------------- cv-state
//
// What the CV faculties are actually doing, measured rather than
// configured -- same field names broadcast-api answers with, so a client
// written against a Linux node reads this one unchanged. An absent or
// stale sidecar reports everything off, never unknown-but-probably-fine.
// See cv.rs for why Phase 1 honestly reports mog2/gated/scene_* false:
// those faculties live in CVProcessor, which is Phase 2.

async fn api_cv_state(State(ctx): State<Arc<Ctx>>) -> Response {
    let body = serde_json::to_vec(&ctx.cv.state()).unwrap_or_default();
    build(StatusCode::OK, "application/json", body, false, true)
}

// -------------------------------------------------------------- pipeline
//
// Per-stage health for the video/audio panel LED banks (paintLamps() /
// paintAudioModes() in index.html). Every lamp there reads from a real
// measurement, never a decorative default -- 'ok'/'down' are genuinely
// distinct from an absent field (dark, unmeasured/not-applicable), so
// this only ever reports a stage as 'ok' or 'down', never invents a
// third value the client would have to special-case.

async fn api_pipeline(State(ctx): State<Arc<Ctx>>) -> Response {
    let p = ctx.pipeline.status();
    let hide = ctx.state.feed_mode.lock().unwrap().as_str() == "hide";
    let camera = if hide {
        "off"
    } else if p.capture_alive {
        "ok"
    } else {
        "down"
    };
    let mediamtx = if p.mediamtx_alive { "ok" } else { "down" };
    // No separate RTSP-connection probe on this node (unlike broadcast-
    // api's _tcp_established_to) -- capture_alive already means our own
    // encoder has an open RTSP push to mediamtx, so both alive together
    // is the honest signal an actual connection exists.
    let rtsp = if p.capture_alive && p.mediamtx_alive { "ok" } else { "down" };
    let mic = ctx.pipeline.audio_status();

    let body = json!({
        "camera": camera,
        "mediamtx": mediamtx,
        "rtsp": rtsp,
        "mic": mic,
    });
    build(
        StatusCode::OK,
        "application/json",
        serde_json::to_vec(&body).unwrap_or_default(),
        false,
        false,
    )
}

// ------------------------------------------------------------- notches
//
// Mirrors broadcast-api's /api/notches* exactly -- see notches.rs module
// docs. `application/json`, not FLASK_TEXT: broadcast-api sets that
// Content-Type explicitly on this envelope, unlike its bare-string routes.

fn notches_response(entries: Vec<Value>) -> Response {
    let body = serde_json::to_vec(&json!({ "notches": entries })).unwrap_or_default();
    build(StatusCode::OK, "application/json", body, false, false)
}

fn notch_err_response(e: NotchError) -> Response {
    match e {
        NotchError::Invalid => build(StatusCode::BAD_REQUEST, FLASK_TEXT, FLASK_400.as_bytes().to_vec(), false, false),
        NotchError::NotFound => build(StatusCode::NOT_FOUND, FLASK_TEXT, FLASK_404.as_bytes().to_vec(), false, false),
        NotchError::Io => build(StatusCode::INTERNAL_SERVER_ERROR, FLASK_TEXT, Vec::new(), false, false),
    }
}

fn bad_request() -> Response {
    build(StatusCode::BAD_REQUEST, FLASK_TEXT, FLASK_400.as_bytes().to_vec(), false, false)
}

/// body or empty-string fallback to `{}`, matching broadcast-api's
/// `_json.loads(request.get_data(as_text=True) or '{}')`.
fn parse_json_body(body: &str) -> Option<Value> {
    if body.trim().is_empty() {
        Some(json!({}))
    } else {
        serde_json::from_str(body).ok()
    }
}

async fn notches_get(State(ctx): State<Arc<Ctx>>) -> Response {
    notches_response(ctx.state.notches.list())
}

async fn notches_post(State(ctx): State<Arc<Ctx>>, body: String) -> Response {
    let Some(body) = parse_json_body(&body) else {
        return bad_request();
    };
    match ctx.state.notches.add(&body) {
        Ok(entries) => {
            ctx.pipeline.reload_audio().await;
            notches_response(entries)
        }
        Err(e) => notch_err_response(e),
    }
}

async fn notches_delete(State(ctx): State<Arc<Ctx>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(raw) = q.get("i") else {
        return bad_request();
    };
    let Ok(i) = raw.parse::<i64>() else {
        return bad_request();
    };
    match ctx.state.notches.delete(i) {
        Ok(entries) => {
            ctx.pipeline.reload_audio().await;
            notches_response(entries)
        }
        Err(e) => notch_err_response(e),
    }
}

async fn notches_patch(
    State(ctx): State<Arc<Ctx>>,
    Query(q): Query<HashMap<String, String>>,
    body: String,
) -> Response {
    let Some(body) = parse_json_body(&body) else {
        return bad_request();
    };
    let Some(enabled) = body.get("enabled").and_then(Value::as_bool) else {
        return bad_request();
    };

    let outcome = match q.get("i") {
        None => ctx.state.notches.set_enabled_all(enabled),
        Some(raw) => match raw.parse::<i64>() {
            Ok(i) => ctx.state.notches.set_enabled_one(i, enabled),
            Err(_) => return bad_request(),
        },
    };
    match outcome {
        Ok(entries) => {
            ctx.pipeline.reload_audio().await;
            notches_response(entries)
        }
        Err(e) => notch_err_response(e),
    }
}

async fn notches_sort(State(ctx): State<Arc<Ctx>>) -> Response {
    // No reload_audio -- order doesn't change what's audible.
    match ctx.state.notches.sort() {
        Ok(entries) => notches_response(entries),
        Err(e) => notch_err_response(e),
    }
}

async fn api_info() -> Response {
    let (host, ts) = tokio::task::spawn_blocking(|| (hostname(), tailscale_ip()))
        .await
        .unwrap_or_else(|_| (String::new(), String::new()));

    // Flask's jsonify spacing, trailing newline included. `version` feeds
    // the viewer's header/tab build label (windows-v<version>) -- see
    // index.html's buildLabel wiring; CARGO_PKG_VERSION so this can never
    // drift from what actually got built (Cargo.toml is the one place
    // the version is written down).
    let body = format!(
        "{{\"hostname\": \"{}\", \"tailscale\": \"{}\", \"version\": \"{}\"}}\n",
        json_escape(&host),
        json_escape(&ts),
        env!("CARGO_PKG_VERSION"),
    );
    build(
        StatusCode::OK,
        "application/json",
        body.into_bytes(),
        false,
        false,
    )
}

// ------------------------------------------------------------- proxy
//
// Same-origin reverse proxy to mediamtx (:8888 HLS, :8889 WHIP/WHEP),
// mirroring broadcast-api's nginx config verbatim (pkg/etc/nginx/conf.d/
// hls-livecam.conf: proxy_http_version 1.1, Host passed through,
// proxy_buffering off). Exists because this server is reachable over
// HTTPS (Tailscale serve terminates TLS on 443 -> this process's :80);
// a browser refuses a plain-http fetch to another port from an https
// page as mixed content, which broke both video playback and two-way
// calls under the tailnet HTTPS URL (found live, 2026-09-09, from an
// actual "NetworkError when attempting to fetch resource" on a real
// WHIP POST). /hls strips its prefix (nginx's trailing-slash
// proxy_pass behavior); /talk and /cam keep theirs (mediamtx's own
// paths already include them). /cam, not /roomaudio -- 7elwe's WHEP
// return leg reuses the cam path rather than a dedicated roomaudio
// publish (see talk.rs's device-contention note), so it needs its own
// proxy prefix nginx's config doesn't have.
//
// Confirmed empirically before writing this (not assumed): mediamtx's
// own WHIP response already returns a relative Location header
// (`/talk/whip/<id>`, no host:port baked in), so this proxy is
// deliberately transparent -- no Location-rewrite step, headers/status/
// body pass through as received.

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

async fn proxy(method: Method, upstream_url: String, headers: HeaderMap, body: Bytes) -> Response {
    let mut req = http_client().request(method, &upstream_url);
    // Forward headers verbatim except Host (reqwest sets its own for the
    // upstream target) and hop-by-hop ones a proxy must not pass as-is.
    for (name, value) in headers.iter() {
        let n = name.as_str();
        if n.eq_ignore_ascii_case("host")
            || n.eq_ignore_ascii_case("content-length")
            || n.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        req = req.header(name, value);
    }
    if !body.is_empty() {
        req = req.body(body);
    }

    let upstream = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("proxy: upstream request to {upstream_url} failed: {e}");
            return build(StatusCode::BAD_GATEWAY, FLASK_TEXT, Vec::new(), false, false);
        }
    };

    let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut resp_headers = HeaderMap::new();
    for (name, value) in upstream.headers().iter() {
        let n = name.as_str();
        if n.eq_ignore_ascii_case("connection")
            || n.eq_ignore_ascii_case("transfer-encoding")
            || n.eq_ignore_ascii_case("content-length")
        {
            continue; // hop-by-hop / let axum recompute framing itself
        }
        resp_headers.insert(name.clone(), value.clone());
    }

    // Streamed through, not buffered into memory first -- matches nginx's
    // proxy_buffering off (load-bearing for live HLS segments and
    // WHIP/WHEP's low-latency SDP/ICE exchange, not a stylistic choice).
    let body = Body::from_stream(upstream.bytes_stream());

    let mut builder = Response::builder().status(status);
    if let Some(h) = builder.headers_mut() {
        *h = resp_headers;
    }
    builder
        .body(body)
        .unwrap_or_else(|_| build(StatusCode::BAD_GATEWAY, FLASK_TEXT, Vec::new(), false, false))
}

fn upstream_url(port: u16, prefix: &str, uri: &axum::http::Uri) -> String {
    let rest = uri.path().strip_prefix(prefix).unwrap_or("");
    match uri.query() {
        Some(q) => format!("http://127.0.0.1:{port}/{rest}?{q}"),
        None => format!("http://127.0.0.1:{port}/{rest}"),
    }
}

async fn proxy_hls(method: Method, OriginalUri(uri): OriginalUri, headers: HeaderMap, body: Bytes) -> Response {
    let mut resp = proxy(method, upstream_url(8888, "/hls/", &uri), headers, body).await;
    // Matches Tanzania's nginx `/hls/` location, which adds this via
    // `add_header Cache-Control "no-store" always` -- ADDS, does not
    // replace, so the wire behaviour there is genuinely two Cache-Control
    // header lines (mediamtx's own max-age/no-cache, plus this). Confirmed
    // empirically (not assumed) by diffing this proxy's headers against
    // Tanzania's real ones: this route was passing mediamtx's header
    // through unchanged with no no-store at all, which iOS Safari's HTTP
    // cache is known to treat more aggressively than desktop/Android --
    // dev: "Unable to hear inbound audio from iOS" on 7elwe traced to
    // exactly this gap (video kept working because its own rendition
    // fetch cycle happened to revalidate anyway; the audio-only rendition
    // is what iOS was serving stale). append(), not insert(), to match
    // nginx's own two-header wire shape rather than guess at a "cleaner"
    // single-header alternative that hasn't actually been proven to work
    // on real iOS hardware the way this exact shape has.
    resp.headers_mut()
        .append(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

async fn proxy_talk(method: Method, OriginalUri(uri): OriginalUri, headers: HeaderMap, body: Bytes) -> Response {
    // Keeps the /talk prefix (unlike /hls) -- mediamtx's own WHIP path is
    // already /talk/whip, matching nginx's proxy_pass .../talk/ target.
    let rest = uri.path().strip_prefix('/').unwrap_or(uri.path());
    let url = match uri.query() {
        Some(q) => format!("http://127.0.0.1:8889/{rest}?{q}"),
        None => format!("http://127.0.0.1:8889/{rest}"),
    };
    proxy(method, url, headers, body).await
}

async fn proxy_cam(method: Method, OriginalUri(uri): OriginalUri, headers: HeaderMap, body: Bytes) -> Response {
    // Same shape as proxy_talk -- keeps the /cam prefix (mediamtx's own
    // WHEP path is /cam/whep).
    let rest = uri.path().strip_prefix('/').unwrap_or(uri.path());
    let url = match uri.query() {
        Some(q) => format!("http://127.0.0.1:8889/{rest}?{q}"),
        None => format!("http://127.0.0.1:8889/{rest}"),
    };
    proxy(method, url, headers, body).await
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

pub(crate) fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

/// Primary LAN IPv4 for the NETWORK panel. The standard no-traffic trick:
/// "connect" a UDP socket toward a public address (no packet is actually
/// sent -- connect on UDP just fixes the socket's default route) and read
/// back which local interface the OS chose. Returns empty on any failure
/// (e.g. no network), which the panel renders as n/a.
pub(crate) fn local_ip() -> String {
    use std::net::UdpSocket;
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_default()
}

/// Mirrors broadcast-api: first 100.x.x.x address found, empty string if the
/// lookup fails for any reason.
pub(crate) fn tailscale_ip() -> String {
    const CANDIDATES: [&str; 2] = ["tailscale", r"C:\Program Files\Tailscale\tailscale.exe"];
    for exe in CANDIDATES {
        // winproc::hidden sets CREATE_NO_WINDOW -- without it the tailscale
        // CLI flashes a console window on every /api/info call and at startup.
        if let Ok(out) = crate::winproc::hidden(exe).args(["ip", "-4"]).output() {
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
