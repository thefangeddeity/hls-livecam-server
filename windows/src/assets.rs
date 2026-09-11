//! Web assets, embedded at compile time from the canonical package tree.
//!
//! These are the same files `hls-livecam-setup` deploys into /var/www on a
//! Linux node -- served verbatim, no @HOSTNAME@ substitution (index.html
//! resolves its own hostname/tailscale display at runtime via /api/info).
//!
//! Paths reach up into pkg/ deliberately: one source of truth for the page,
//! shared with the Debian package. Building windows/ outside the monorepo is
//! not supported.

use std::sync::OnceLock;

const INDEX_RAW: &str = include_str!("../../pkg/usr/share/hls-livecam-server/index.html");
const CAMS_RAW: &str = include_str!("../../pkg/usr/share/hls-livecam-server/cams/cams.html");

/// The fleet's actual brand mark (pulled 2026-09-09 from
/// `origin/main:pkg/usr/share/hls-livecam-server/brand.png` -- same file
/// Tanzania serves at this path, already cropped/sized for exactly this
/// use: a small header logo and browser tab icon, not the wider crop
/// windows/assets/icon-256.png uses for the desktop app's window/taskbar
/// icon at a different scale).
pub const BRAND_PNG: &[u8] = include_bytes!("../../pkg/usr/share/hls-livecam-server/brand.png");

/// The vendored HLS player -- index.html's own `<script src="/vendor/hls.min.js">`
/// depends on this existing at that exact path (see index.html's comment on
/// that tag: vendored deliberately, not cdnjs, so a phone on a captive or
/// filtered network still gets a working player). Never actually embedded
/// here until now -- the route for it didn't exist at all, so this tag has
/// 404'd on every browser on this node since the port began. Silently
/// survivable on Safari/iOS specifically (native-first video, and audio's
/// own `window.Hls &&` guard falls through to a native <audio src> either
/// way) but NOT on Chrome/Firefox/Android, which have no native HLS
/// support and need this file to play anything at all. Found while
/// chasing "Unable to hear inbound audio from iOS" -- turned out to be
/// real but unrelated to iOS specifically; this is a bigger, older gap
/// the iOS report just happened to surface first.
pub const HLS_MIN_JS: &[u8] =
    include_bytes!("../../pkg/usr/share/hls-livecam-server/vendor/hls.min.js");

/// Line endings are normalised to LF before serving.
///
/// git's core.autocrlf is true on a stock Windows checkout, which rewrites
/// these files to CRLF on disk. Embedding them as-is served an index.html
/// 973 bytes larger than tina's -- one \r per line -- so the page a peer
/// fetched from this node was byte-different from every Linux sibling for
/// no reason. Normalising here keeps the fix inside windows/ and holds
/// regardless of how a given machine has autocrlf configured.
fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

pub fn index_html() -> &'static str {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| lf(INDEX_RAW)).as_str()
}

pub fn cams_html() -> &'static str {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| lf(CAMS_RAW)).as_str()
}

/// The fleet list. A camera node serves an empty list; the aggregator role
/// lives on whichever box actually has the roster. `hls-livecam-setup` seeds
/// this with `[]` on Linux, so an unconfigured node answers `[]` too.
pub const CAMS_JSON_DEFAULT: &str = "[]";
