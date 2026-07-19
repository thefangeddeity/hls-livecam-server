//! Web assets, embedded at compile time from the canonical package tree.
//!
//! These are the same files `hls-livecam-setup` deploys into /var/www on a
//! Linux node -- served verbatim, no @HOSTNAME@ substitution (index.html
//! resolves its own hostname/tailscale display at runtime via /api/info).
//!
//! Paths reach up into pkg/ deliberately: one source of truth for the page,
//! shared with the Debian package. Building windows/ outside the monorepo is
//! not supported.

pub const INDEX_HTML: &str =
    include_str!("../../pkg/usr/share/hls-livecam-server/index.html");

pub const CAMS_HTML: &str =
    include_str!("../../pkg/usr/share/hls-livecam-server/cams/cams.html");

/// The fleet list. A camera node serves an empty list; the aggregator role
/// lives on whichever box actually has the roster. `hls-livecam-setup` seeds
/// this with `[]` on Linux, so an unconfigured node answers `[]` too.
pub const CAMS_JSON_DEFAULT: &str = "[]";
