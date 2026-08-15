# Changelog
## [5.4.0] - 2026-08-14
### Fixed
- **Feed mode persists across restarts, and defaults to `cloak`.** A restart
  previously reset the feed to `show`, exposing the clear camera after the
  operator had deliberately chosen cloak. `show` is now never reached by
  fallback -- only by explicit choice.
- **GOP regression.** Run-1's 1s keyframe interval was lost when a package
  install overwrote a live-only fix. Now sourced from `device.env` as
  `GOP_SECONDS` (default 1) and committed in `pkg/`.
- **broadcast-api now stops cleanly.** Its supervisor loops respawned the
  children that SIGTERM had just killed, so the replacement ffmpeg outlived
  the process, systemd waited out its full stop timeout, and an orphaned
  ffmpeg kept the camera device open. 90s-and-failed is now 47ms and clean.
- **Stalled streams are detected.** `hls_worker` only checked the master
  playlist for HTTP 200 -- a static file that answers 200 while the pipeline
  behind it is stopped. It now watches `EXT-X-MEDIA-SEQUENCE` and reports
  STALE, with the threshold derived from the stream's own TARGETDURATION so
  slow hardware is not misreported as broken.
- **Hide no longer derives from the camera image.** It row-shuffled and
  rolled the live frame; it now publishes black, matching the Windows node.
- Header no longer reads DEGRADED for a deliberately stopped server, or for
  a healthy one whose HLS state has not been polled yet.

### Changed
- Feed switches are covered by VHS static instead of a 16s countdown, and
  the cover is now shown to **every** viewer -- previously only the browser
  that clicked saw it.
- Hide shows viewers "No signal / The camera is switched off" rather than an
  unexplained black frame.

## [5.3.0] - 2026-08-13
### Added
- camdash-gui: PySide6 operator dashboard for Linux, ported from the macOS build.
  Installed alongside camdash — the curses monitor is unchanged and still the
  headless/SSH surface; the GUI is the desktop one.
- camdash: additive GUI-support layer (API_BASE/RTSP_PORT/HLS_PORT/API_PORT,
  `camera_present_cached()`, `_cpu_temp()` via x86_pkg_temp, and a `_livecam()`
  dispatcher over the existing run_* service functions). The curses UI does not
  reference any of it.
- setup (Arch): registers camdash-gui.desktop and prepares the per-user config
  directory the GUI persists window geometry into, chowned to the invoking user
  (root-owned would be silently unwritable from the user's session).

### Notes
- GUI is Arch-only. PySide6 has no Debian bookworm packages (the split pyside6
  packages start at trixie) and the Debian node is headless, so `gui/` lives at
  the repo root rather than under `pkg/`, and the .deb payload is unchanged.

## [3.0.3] - 2026-05-06
### Fixed
- camdash: VIDEO STACK shows N/A for MSSG API, DARK, FPS, REPAIR when server is off ([o] toggle)
- camdash: CAM shows FOUND (not LIVE) when server is off but camera is present
- camdash: CAM shows LIVE only when server is on and camera is present
- setup (Arch): mediamtx restarted after config write to avoid "path not configured" on fresh installs
- setup (Arch): sudoers entries use `disable --now` / `enable --now` to match camdash [o] toggle


## [2.8.7] - 2026-05-01
### Fixed
- camstack: `getdata()` → `tobytes()` (Pillow deprecation, Python 3.14)
- Arch setup: move nginx default server from port 80 to 8080 so conf.d config wins
- Arch setup: add `include conf.d/*.conf;` to nginx.conf if missing
- Arch setup: create `/var/log/nginx/` if missing

## [2.8.6] - 2026-04-26
### Fixed
- Arch setup: `User=www-data` → `User=http` in broadcast-api.service
- Arch setup: sudoers entry updated from www-data to http
- deb: version bumped, ffmpeg-cam.service synced to pkg

## [2.8.5] - 2026-04-25
### Fixed
- Arch setup: pkg copy of hls-livecam-setup-arch had stale `hostname -I` — now matches installed fix from v2.8.4

## [2.8.4] - 2026-04-25
### Fixed
- Arch setup: IP address empty in completion message — replaced `hostname -I` with `ip route get` fallback

## [2.8.3] - 2026-04-25
### Fixed
- Arch setup: `hostname` command not found — replaced with `hostnamectl hostname` with `/etc/hostname` fallback

## [2.8.2] - 2026-04-25
### Added
- Feed mode controls: Show / Cloak / Hide replacing single Hide Feed toggle
- Cloak mode: client-side halfblock pixelation canvas overlay (color, live)
- Cloak bridges to Hide: grayscale pixelation shown immediately on Hide to prevent flash of raw video while snapshot generates
- confirm() dialog on Clear message button

### Changed
- Forced dark theme (no longer follows system preference)
- Sidebar sections styled as cards
- Button state logic: Save and Cancel blue only when changes pending; Clear blue when message exists; all always visible
- block_art COLS reduced to 80 to match Cloak pixelation density
- dark.png cache-busted on Hide with timestamp query string

### Notes
- Cloak is client-side only — a determined user can bypass via dev tools.
  Future goal: server-side pixelated HLS stream via ffmpeg-cam-dark.service.

## [2.8.1] - 2026-04-24
### Added
- camstack SYSTEM panel: CPU temperature display (coretemp sensor, falls back to "?" if unavailable)
- snap_interval config: `/var/lib/hls-livecam/snap_interval` sets widget refresh (1/2/5s), falls back to 2s default

### Changed
- SMART box: colon spacing normalized, TEMP renamed to DISK TEMP
- SYSTEM panel bottom row: equidistant thirds layout for RAM / USB / CPU TEMP

## [2.8.0] - 2026-04-24
### Added
- camstack MESSAGE box: live pixelated webcam widget using halfblock ▀ rendering with oceanic/earth-tone palette (color when feed live, grayscale when hidden)
- camstack broadcast editor: multiline support with Enter for newline, Ctrl+D to save & deliver, left/right cursor navigation, mid-text insert/delete
- camstack footer: dynamic word wrap at • boundaries, two-row layout
- Mute Buzzes button: now permanently blue (primary) for visual consistency
- Blinking • bullet on LIVE status in camstack header

### Changed
- snap_worker refresh interval: 5s (tuned from 2s for CPU stability)
- Widget capture size increased to 80×26 for terminal resize support

## [2.7.9] - 2026-04-24
### Fixed
- Repair completion status "OK" overriding header sys_status, causing "OK | feed hidden" to display in red instead of falling back to base status (LIVE/CONNECTING/DOWN)
- Removed dead color branch in `header_attr()` handling "OK" — unreachable after above fix

## [2.7.8] - 2026-04-24
### Fixed
- Packaging and setup audit: corrected dependency declarations, sudoers, service paths, and postinst to reach clean installable state on Debian and Arch

## [2.8.8] - 2026-05-02
- index.html: hostname now dynamic in title and header (works on any host)
- index.html: feed mode buttons inverted logic fixed (active=gray, inactive=blue)
- index.html: cancel button no longer lit on page load
- ffmpeg-cam.service template: User=root (was mediamtx, cross-contamination fix)
- camstack: version string updated to 2.8.8

## [2.8.9] - 2026-05-02
- index.html: feed mode button logic fixed (active=gray, inactive=blue)
- index.html: cancel button no longer lit on page load
- index.html: renamed CAM-01 to HLS Livecam

## [2.8.10] - 2026-05-04
- setup-arch: fix nginx log dir, http user, conf.d handling (Arch-specific)
- broadcast-api.service: revert DynamicUser, use www-data (setup substitutes http on Arch)

## [2.8.11] - 2026-05-04
- broadcast-api.service: revert DynamicUser, use www-data (setup substitutes http on Arch)

## [2.8.12] - 2026-05-04
- setup: use template for ffmpeg-cam.service (wrong file — patched hls-livecam-setup instead of hls-livecam-setup-arch, no effect)

## [2.8.13] - 2026-05-04
- setup-arch: use @PLACEHOLDER@ template for ffmpeg-cam.service ExecStart, eliminating ${VAR} word-splitting bug

## [2.8.14] - 2026-05-04
- index.html: remove hardcoded primary class from showBtn
- index.html: rename Cloak to Blur
