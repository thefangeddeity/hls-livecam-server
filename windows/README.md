# hls-livecam-win

Native Windows camera node. Serves the same HTTP surface as a Linux node so
the existing fleet cannot tell the difference — same paths, same status
codes, same content types, same CORS placement.

No WSL2, no nginx, no Flask, no systemd. One binary.

## Status: run 3 of 4 — native operator dashboard, done

This closes the Windows node v1. Shipped here:

- a native egui/eframe operator window -- no webview, no HTML in the GUI
  anywhere -- reproducing camdash's six-panel cockpit (DISK/SMART, FEED,
  SYSTEM, VIDEO, PROCESSES, MESSAGE) with the same green-on-black palette
  and status-color thresholds, transcribed from camdash's source (see
  `gui/theme.rs`) and the reference screenshot
- live video in the FEED panel -- a second, independent ffmpeg tap on the
  local RTSP stream, decoded to raw frames and uploaded as an egui
  texture (see `video_preview.rs`)
- GUI buttons drive the *same* AppState/Pipeline methods the HTTP handlers
  call -- one process, no loopback HTTP client (PM direction: GUI and
  server don't need to be decoupled just because that's how camdash and
  the Linux stack relate)
- autostart via the per-user Registry Run key, idempotent across restarts
- minimize-to-tray (best-effort; falls back to a normal minimizable
  window if tray construction fails on a given machine)
- clean shutdown: closing the window kills mediamtx.exe/ffmpeg.exe
  first -- Windows does not do this automatically when a parent process
  exits, unlike what `taskkill /T`-based testing in run 2 made it easy to
  assume (see `pipeline.rs::shutdown`)

Deferred, unchanged from run 2: Cloak/blur/bw-mode's actual pixelation
pipeline (v1.1). `cloak` fails safe to the same black source as `hide`,
now also true for the GUI's disabled Blur/B&W controls -- present in the
layout, non-interactive, so nothing advertises a capability this node
doesn't have.

## Build and run

```
cargo build --release
target\release\hls-livecam-win.exe
```

The window opens directly -- no separate server process to start first,
it's the same binary. ffmpeg.exe and mediamtx.exe are **not** vendored
into this repo -- the binary looks for them next to itself and fails
loudly, naming the exact paths it checked, if it can't find them. See
`binaries.rs` for the resolution order. Bundle layout:

```
target\release\hls-livecam-win.exe
target\release\bin\ffmpeg.exe
target\release\bin\mediamtx.exe
```

Versions used this run: ffmpeg 8.1.2-essentials_build (gyan.dev Windows
static build), mediamtx v1.15.2 (matches the version `hls-livecam-setup`
pins on Linux).

Binding :80 needs no elevation on Windows. The port is not negotiable: a
peer's `cams.html` fetches `http://<ip>/broadcast.txt` with no port. (A
per-cam `api_port` override does exist in `cams.json`, but using it means
editing the roster on the Linux side, which defeats "drop in with zero
fleet changes".)

| Env var | Default | Purpose |
|---|---|---|
| `HLS_BIND` | `0.0.0.0:80` | Listen address. Handy for testing off :80. |
| `HLS_STATE_DIR` | `%APPDATA%\hls-livecam-win` | Where state is persisted. |
| `HLS_FFMPEG` | *(see binaries.rs)* | Explicit override path to ffmpeg.exe. |
| `HLS_MEDIAMTX` | *(see binaries.rs)* | Explicit override path to mediamtx.exe. |

## HTTP surface

| Method | Path | Notes |
|---|---|---|
| GET | `/`, `/index.html` | Viewer page, same bytes both ways |
| GET | `/cams/`, `/cams/cams.html` | Aggregator page (`/cams` → 301) |
| GET | `/cams/cams.json` | Fleet roster, `[]` unless one is placed in the state dir |
| GET | `/broadcast.txt` | Current message. **CORS `*`** — peers read this cross-origin |
| GET | `/buzz.txt` | Last buzz timestamp, ms. No CORS (matches live nodes) |
| POST | `/api/broadcast` | Set message. Trimmed, 120 chars. **204** on success |
| POST | `/api/buzz` | Records a buzz, returns the timestamp |
| GET/POST | `/api/feed-mode` | `show` \| `cloak` \| `hide`; anything else → **400** |
| GET/POST | `/api/msg-lock` | Defaults to `true` when no state file exists |
| GET/POST | `/api/bw-mode` | In-memory only, resets on restart (matches Linux) |
| GET/POST | `/api/dark` | Legacy flag. Nothing in the current viewer calls it |
| GET | `/api/info` | `{"hostname": ..., "tailscale": ...}` |

Anything else is a 404 — with nginx's 404 body outside `/api/` and Flask's
inside it, because on Linux those two are different servers.

## Where the contract came from

The reference is a *running* Linux node, not the repo. Where the two
disagreed, the live fleet won:

- `pkg/etc/nginx/conf.d/hls-livecam.conf` says `/buzz.txt` carries
  `Access-Control-Allow-Origin` and that `/index.html` should 404. Neither
  is true on tina or tanzania. That file is shadowed by the config
  `hls-livecam-setup` generates into `sites-available`, and appears to be
  dead. It is left alone here — it is a Linux-package concern.
- `pkg/etc/systemd/system/ffmpeg-cam-dark.service` is never enabled by
  `hls-livecam-setup` -- same class of fossil. Hide's black-frame command
  is ported verbatim from it anyway (run-2 brief's explicit instruction),
  so there's no live node to compare Hide's manifest characteristics
  against. Its GOP/framerate (`r=30`, `g=60` → 2s keyframe interval) don't
  match Show's real production values (`g=60` at 15fps output → 4s), so a
  consumer will see `TARGETDURATION` change (4 → 2) and segment/sequence
  numbers advance faster while hidden. Stream never drops across the
  swap; only its segment cadence changes.
- camdash's README claims "Auto-repair — detects stream down for 8s and
  triggers repair automatically." Reading the actual source: `hls_worker()`
  only checks `GET /cam/index.m3u8` for HTTP 200 every 5s (doesn't look at
  whether the manifest is advancing), and `run_repair()` has exactly one
  call site, the interactive `[r]` key. There's no auto-repair loop
  anywhere in the current source. This node's self-healing supervisor is
  therefore not a port of anything -- it's sized off mediamtx's own
  `TARGETDURATION` (4s default; 8s stall threshold = two full segments).

## Notes

- Web assets are `include_str!`'d from `../pkg/...` at compile time, so the
  page has one source of truth shared with the Debian package and the exe
  stays self-contained. Building this crate outside the monorepo will not work.
- `windows/` is marked `export-ignore` in the repo's `.gitattributes`, so
  `git archive` (which is how the AUR tarball is cut) omits it.
- The FEED panel's live preview runs a **second** ffmpeg decode process
  independent of the fleet-facing capture pipeline. It's a read-only tap
  (doesn't touch mediamtx or the RTSP publish path), but it's real CPU:
  observed two ffmpeg processes and noticeably higher CPU with the window
  open versus run 2's headless server. Deliberately kept modest (480x270
  @8fps) to bound it, but it is not free.
- DISK/SMART, SYSTEM (CPU temp, LOAD), and PROCESSES pull from
  Windows-native sources (`Get-PhysicalDisk`, `sysinfo`), not ports of
  camdash's psutil/smartctl calls -- there's nothing to port, the
  underlying OS facilities differ entirely. Fields with no honest Windows
  equivalent (NVMe has no ATA REALLOC/PENDING/UNCORR attributes; this
  laptop's CPU temp isn't exposed cleanly; Windows has no Unix load
  average) are shown dimmed/n-a rather than approximated.
- Found via actually driving the window with real clicks and keystrokes,
  not by reading the code: the MESSAGE panel's edit buffer was getting
  silently overwritten every repaint by the stored value, because the
  "don't clobber while editing" guard checked a field that was never set
  to true anywhere. Typed text could never be saved. Fixed by tracking the
  text field's actual focus state instead. Screenshotting the running app
  at each step is what caught this -- it read correctly in the source.
