# hls-livecam-win

Native Windows camera node. Serves the same HTTP surface as a Linux node so
the existing fleet cannot tell the difference — same paths, same status
codes, same content types, same CORS placement.

No WSL2, no nginx, no Flask, no systemd. One binary.

## Status: run 4 of 4 — professional console restyle + fixes, done

This closes the Windows node v1. Run 3 shipped a working but
terminal-flat egui window; run 4 restyled it to a professional NVR-
console look (Blue Iris class -- framed, bordered, beveled panels on
neutral steel/slate chrome) and fixed real bugs found by actually running
the thing:

- **Restyle** (`gui/theme.rs`'s console-chrome section, `gui/mod.rs`'s
  `panel_at`): every panel is a fully bordered, beveled box with a raised
  header strip, on neutral dark-slate chrome. Fixes run 3's edge-bleed
  bug (the rightmost column had no visible right border) by replacing
  automatic `ui.horizontal()` sizing with an explicit rect-based grid.
  Status colors (green/yellow/red/dim) are byte-for-byte unchanged --
  only the chrome around them changed.
- **Message save was actually broken** -- found by Ron clicking it
  himself, not by my own testing. The "don't clobber while editing"
  guard checked egui's live focus state, which necessarily goes false
  the instant you click Save (any click outside a focused field blurs
  it), racing the save itself. Replaced with an explicit edit-mode flag
  that's only cleared by Save/Cancel, never by focus loss.
- **Hide mode was burning 300-400% CPU** -- also found by Ron, not me.
  The black-frame `lavfi` source had no `-re` (real-time pacing) flag,
  so ffmpeg generated and encoded frames as fast as the CPU allowed
  instead of pinned to 30fps. A real camera paces itself at the hardware
  rate; a synthetic source doesn't unless told to. Now ~5% CPU per
  process in Hide.
- **SERVER: ON/OFF is now a real control**, not a passive readout --
  `Pipeline::set_enabled()` stops/starts mediamtx and capture together,
  both supervisor loops idle-poll instead of respawning while off.
- **Runs elevated** (PM decision) so DISK/SMART can read
  `Get-StorageReliabilityCounter` for real NVMe UNCORR/TEMP data,
  previously permanently dimmed. This has a real cost: autostart moved
  from the Registry Run key to a Scheduled Task (`RunLevel:
  HighestAvailable`) since a Run-key launch of a `requireAdministrator`
  exe still prompts UAC every login. **More importantly: once elevated,
  I can no longer test this app's GUI myself** -- see the UIPI note
  below. This is the single biggest process change this run forced.
- minimize-to-tray (best-effort; falls back to a normal minimizable
  window if tray construction fails on a given machine)
- clean shutdown: closing the window kills mediamtx.exe/ffmpeg.exe
  first -- Windows does not do this automatically when a parent process
  exits (see `pipeline.rs::shutdown`)

Deferred, unchanged from run 2: Cloak/blur/bw-mode's actual pixelation
pipeline (v1.1). `cloak` fails safe to the same black source as `hide`;
the GUI's Blur/B&W controls stay visibly disabled, present in the layout
so nothing advertises a capability this node doesn't have.

### UIPI: elevation means I can no longer click-test this app myself

Windows blocks synthetic input (`SendInput`/`mouse_event`, and
`SendMessage`/`PostMessage` for input-shaped messages including
`WM_CLOSE`) from a standard-integrity process to a higher-integrity
window -- User Interface Privilege Isolation, the same mechanism that
puts UAC's consent prompt on the Secure Desktop. Before this app required
admin, my automation and the app ran at the same integrity level and this
didn't apply; every prior run's click/keystroke verification worked
because of that, not despite it.

Confirmed empirically this run, not assumed: precisely-measured
`mouse_event` clicks at the correct on-screen coordinates for Show,
Buzz, and a `SendMessage(WM_CLOSE)` all silently did nothing once the
window was running elevated, while the *identical* techniques worked
throughout runs 1-3. Screenshots (`CopyFromScreen`, which reads the
physical framebuffer rather than asking the target process for its
content) and the HTTP API (a network socket, unrelated to window
elevation) are unaffected -- I can still verify visually and can still
drive `/api/feed-mode` etc. directly to prove the underlying pipeline
logic works. What I cannot do anymore is prove a GUI *button* is wired
to that logic by actually clicking it. Ron clicking the app himself is
now load-bearing for GUI verification, not optional -- flagged plainly
in the run-4 report rather than quietly narrowing what "verified" means.

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
- Run 3's message-save fix was incomplete -- the MESSAGE panel's edit
  buffer was still getting silently overwritten by the stored value on
  focus loss, because clicking Save necessarily blurs the field in the
  same frame the click needs to register on. Ron caught this manually,
  clicking the actual app; my own automated testing that run had missed
  it (a coordinate-precision problem in the click automation masked the
  real bug -- the clicks looked like they'd failed for a boring reason
  when the field itself was the issue). Fixed properly in run 4: an
  explicit edit-mode flag, cleared only by Save/Cancel, never by focus.
- Run 3's Hide-mode ffmpeg command had no `-re` flag on its synthetic
  `lavfi` source, so it encoded flat out instead of pinned to 30fps --
  300-400% sustained CPU, also caught by Ron running the app, not by me.
  See the run-4 status section above.
