# hls-livecam-server — macOS port

Turns a Mac's webcam into a live HLS stream with a browser-based presence
system: message board, dark-mode cloak, block-art "cloak"/"hide" feed modes,
buzz notifications, and a `camdash` terminal monitor.

This is a macOS port of [thefangeddeity/hls-livecam-server](https://github.com/thefangeddeity/hls-livecam-server)
(a Debian package). It targets **this Mac only** — no packaging, no root.

---

## What changed from the Linux original

| Concern | Linux original | macOS port |
|---|---|---|
| Camera capture | v4l2 (`/dev/video*`) | `ffmpeg -f avfoundation` |
| Studio pipeline | `pyfakewebcam` → v4l2loopback → ffmpeg | Python writes processed RGB frames **straight into the publisher ffmpeg's stdin** (no kernel module) |
| Web server | nginx (`:80`) | the Flask `broadcast-api` serves the viewer + `/api/*` + state files on `WEB_PORT` |
| Service manager | systemd units | `launchd` login agent + the `livecam` CLI (background processes + pidfiles) |
| Paths | FHS (`/var/www`, `/etc`, …) | self-contained repo dirs (`web/`, `state/`, `vendor/`) |
| Monitor | `camdash` (v3.0.0) | same, with macOS probes for swap/disk/SMART/temp/services |

> Note: the upstream README documents `camstack`, but the current monitor is
> `camdash` (v3.0.0); `camstack` is the stale predecessor and was **not** ported.

---

## Architecture

```
avfoundation → ffmpeg(reader) → broadcast-api (show/cloak/hide) → ffmpeg(publisher)
                                                                       ↓ RTSP :8554
                                                                    mediamtx
                                                                       ↓ HLS :8888 (browser-direct)
   broadcast-api (Flask, :8080) ── serves viewer + /api/* + broadcast.txt/buzz.txt/dark.png
```

- **broadcast-api** captures the camera, applies the current feed mode
  (`show` = raw, `cloak` = color block-art, `hide` = SVHS-warped B&W block-art),
  and pushes the result to mediamtx over RTSP. It also serves the web viewer,
  the JSON/text state files, and the `/api/*` endpoints.
- **mediamtx** ingests RTSP and serves HLS (with `hlsAllowOrigin: '*'` so the
  `:8080` page can pull segments from `:8888`).
- Feed capture holds the camera, so only one instance runs at a time.

---

## Requirements

- macOS with a webcam (built-in or USB)
- [Homebrew](https://brew.sh) with `ffmpeg`: `brew install ffmpeg`
- Grant your **terminal app** camera access: System Settings ▸ Privacy &
  Security ▸ Camera.
- Python 3 (a private virtualenv is created at `.venv`)

mediamtx ships in `vendor/` (darwin binary); nothing else is downloaded.

---

## Install / run

```bash
# one-time: create the venv and install deps (flask numpy pillow psutil)
python3 -m venv .venv
./.venv/bin/python -m pip install flask numpy pillow psutil

# detect the camera, write config, start, and install the login agent
./bin/livecam setup

# ...or just run without a login item:
./bin/livecam start
```

Add `bin/` to your `PATH` (or `alias livecam=$PWD/bin/livecam`) to drop the
`./bin/` prefix.

---

## Commands

| Command | What it does |
|---|---|
| `livecam setup` | Detect camera, write `config.env`, start, enable login agent |
| `livecam start` / `stop` / `restart` | Manage the two services |
| `livecam status` | Show pids, ports, HLS health, login-agent state |
| `livecam repair` | Kill stray ffmpeg and reconverge the pipeline |
| `livecam dark` | Toggle the dark-mode cloak (renders block-art `dark.png`) |
| `livecam monitor [--expanded]` | Launch the `camdash` TUI |
| `livecam logs [mediamtx\|api]` | Tail service logs |
| `livecam enable` / `disable` | Install / remove the launchd login agent |
| `livecam urls` | Print viewer / stream URLs |

### camdash keys

`f` feed preview · `x` expand · `s` show · `b` blur(cloak) · `h` hide · `w` B&W ·
`m` message · `l` lock msg · `c` clear msg · `z` buzz · `i` cam IPs · `p` pause ·
`o` on/off · `r` repair · `q` quit

---

## After setup

| What | Where |
|---|---|
| Web viewer | `http://<mac-ip>:8080` |
| HLS stream | `http://<mac-ip>:8888/cam/index.m3u8` |
| RTSP (VLC) | `rtsp://<mac-ip>:8554/cam` |
| Monitor | `livecam monitor` |

`livecam urls` prints these with your current LAN IP.

---

## Broadcast API

```bash
curl -X POST http://localhost:8080/api/broadcast --data 'Ran out of eggs! Brb!'
curl -X POST http://localhost:8080/api/dark          # toggle dark cloak
curl -X POST http://localhost:8080/api/buzz          # screen-shake + tone on viewers
curl -X POST http://localhost:8080/api/feed-mode --data 'cloak'   # show|cloak|hide
```

---

## Configuration

`config.env` (bare `KEY=VALUE`, no inline comments after values):

```
AVF_VIDEO_INDEX=0                 # ffmpeg -f avfoundation -list_devices true -i ""
AVF_DEVICE_NAME=FaceTime HD Camera (Built-in)
VIDEO_SIZE=1280x720
FRAMERATE=15
WEB_PORT=8080
RTSP_PORT=8554
HLS_PORT=8888
```

Re-run `livecam setup` to change the camera or framerate.

---

## Multi-cam grid (`web/cams`)

`web/cams/cams.json` lists other cameras to show in the `/cams` grid view.
Each entry:

```json
{
  "label": "ariana",
  "ip": "192.168.18.15",
  "stream_path": ":8888/cam/index.m3u8",
  "api_port": 8080,
  "pinned": true
}
```

- `stream_path` is appended straight to `ip` for the HLS URL — any port works,
  since it's just `http://<ip><stream_path>`.
- `api_port` (optional) is the port the message pill polls
  (`http://<ip>[:api_port]/broadcast.txt`). Omit it for boxes serving that on
  plain port 80 (the original Linux/nginx setup); set it to `8080` for another
  instance of this macOS port. Without it, the message pill just stays empty
  (`—`) — the video still plays fine either way, since that's driven by
  `stream_path`, not `api_port`.
- `camdash`'s CamHub (`i` key) only edits `label`/`ip`; `stream_path` and
  `api_port` are hand-edited in the JSON file, same as upstream.

**To add this Mac to another box's `/cams` grid**, that box's `cams.html`
needs the same `api_port`-aware `msgUrl()`/`pollMsg()` as this repo's
`web/cams/cams.html` — older copies (e.g. an unmodified upstream Linux
install) hardcode plain port 80 and will silently show `—` for this Mac's
messages even though its video works. Copy this repo's `web/cams/cams.html`
over, or hand-apply the same one-line changes, then add an entry there with
`"api_port": 8080`.

---

## Troubleshooting

| Problem | Fix |
|---|---|
| Stream broken | `livecam repair` |
| Camera "Input/output error" | Grant terminal camera access; another app may hold the camera — `livecam repair` |
| Service logs | `livecam logs mediamtx` / `livecam logs api` |
| List cameras | `ffmpeg -f avfoundation -list_devices true -i ""` |
| Cloak preview is slow | block-art renders ~9 fps on this hardware; the stream keeps up but at a lower cloak framerate |

---

## Notes

- **Sleep**: `livecam start`/`enable`/`repair` run `caffeinate -i`, tied to
  broadcast-api's pid, so the Mac won't idle/system sleep while the server is
  up (sleep would suspend the camera capture, mediamtx, and Flask entirely).
  It auto-releases the moment broadcast-api exits — `livecam stop` also kills
  it explicitly. Only idle/system sleep is blocked; the display can still
  sleep, since that doesn't affect background processes. Check the current
  state with `livecam status` (`sleep-block: active/inactive`).
- **SMART panel**: macOS internal SSDs usually don't expose SMART attributes to
  `smartctl`; the panel degrades to `N/A`/`NO ACCESS`. Install
  `brew install smartmontools` for external drives.
- **CPU temp**: unavailable without extra tooling on macOS; shown as `?`.
- Optional end-to-end smoke test: `./tests/smoke.sh` (run while services are up).

## License

GPL-3.0 (inherited from upstream).
