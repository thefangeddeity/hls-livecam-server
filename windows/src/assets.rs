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
