# hls-livecam-win

Native Windows camera node. Serves the same HTTP surface as a Linux node so
the existing fleet cannot tell the difference — same paths, same status
codes, same content types, same CORS placement.

No WSL2, no nginx, no Flask, no systemd. One binary.

## Status: run 2 of 4 — control-plane + live capture

Shipped here:

- `:80` control-plane API and the web assets, byte-identical to the
  package's `pkg/usr/share/hls-livecam-server/` copies
- message board, buzz, feed-mode, msg-lock, bw-mode, dark, `/api/info`
- state that survives a process restart, including feed-mode -- a box
  that was hidden boots back into hidden
- live capture: dshow camera → ffmpeg → RTSP → mediamtx → HLS on
  `:8888/cam/index.m3u8`, the same path the fleet already expects
- Show/Hide actually drives the pipeline now: Hide is a source swap to a
  black-frame ffmpeg on the same RTSP path, not a client-side overlay --
  mediamtx keeps the HLS output continuous across the swap
- self-healing capture supervision (process-exit + manifest-staleness
  detection; see `pipeline.rs` for why this isn't a port of anything in
  camdash -- there was nothing live to port)

Not here yet (later runs): tray icon, dashboard GUI, autostart, installer.
Cloak/blur/bw-mode's actual pixelation pipeline is deferred (v1.1) --
`cloak` currently fails safe to the same black source as `hide` (see
`pipeline.rs`), so clicking Blur in the existing viewer never leaves the
real feed exposed, even though the visual isn't the real effect yet.

## Build and run

```
cargo build --release
target\release\hls-livecam-win.exe
```

Then open <http://localhost/>. ffmpeg.exe and mediamtx.exe are **not**
vendored into this repo (that's run 3's job) -- the binary looks for them
next to itself and fails loudly, naming the exact paths it checked, if it
can't find them. See `binaries.rs` for the resolution order. For local
runs, drop both into a `bin\` folder next to the exe:

```
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
