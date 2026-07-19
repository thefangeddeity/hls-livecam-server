# hls-livecam-win

Native Windows camera node. Serves the same HTTP surface as a Linux node so
the existing fleet cannot tell the difference — same paths, same status
codes, same content types, same CORS placement.

No WSL2, no nginx, no Flask, no systemd. One binary.

## Status: run 1 of 4 — control-plane + web assets only

Shipped here:

- `:80` control-plane API and the web assets, byte-identical to the
  package's `pkg/usr/share/hls-livecam-server/` copies
- message board, buzz, feed-mode, msg-lock, bw-mode, dark, `/api/info`
- state that survives a process restart

Not here yet (later runs): ffmpeg/dshow capture, mediamtx, RTSP, HLS, the
tray icon, the dashboard GUI, autostart, and the Show/Hide feed effects.

**The viewer's video player will sit dead at "connecting".** That is
expected until run 2 brings up the capture pipeline. `feed-mode` records and
reports its value but drives nothing.

## Build and run

```
cargo build --release
target\release\hls-livecam-win.exe
```

Then open <http://localhost/>.

Binding :80 needs no elevation on Windows. The port is not negotiable: a
peer's `cams.html` fetches `http://<ip>/broadcast.txt` with no port. (A
per-cam `api_port` override does exist in `cams.json`, but using it means
editing the roster on the Linux side, which defeats "drop in with zero
fleet changes".)

| Env var | Default | Purpose |
|---|---|---|
| `HLS_BIND` | `0.0.0.0:80` | Listen address. Handy for testing off :80. |
| `HLS_STATE_DIR` | `%APPDATA%\hls-livecam-win` | Where state is persisted. |

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

## Notes

- Web assets are `include_str!`'d from `../pkg/...` at compile time, so the
  page has one source of truth shared with the Debian package and the exe
  stays self-contained. Building this crate outside the monorepo will not work.
- `windows/` is marked `export-ignore` in the repo's `.gitattributes`, so
  `git archive` (which is how the AUR tarball is cut) omits it.
