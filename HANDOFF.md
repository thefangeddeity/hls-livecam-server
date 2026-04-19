# hls-livecam-server — Handoff to Fresh Instance

## Project in one paragraph

`hls-livecam-server` is a Debian package that streams a USB webcam via HLS using MediaMTX and ffmpeg, served through nginx with a browser viewer. Ron (user, github: thefangeddeity) runs it on his home server `tina` (Ubuntu, ffmpeg without libv4l2). `camstack` is a curses TUI monitor. A Flask broadcast API (`broadcast-api`) allows the web viewer to send/receive messages and control dark mode and buzz. The family uses it as a shared whiteboard and presence system. As of v2.6.3, the feature set is: live stream, message board, hide/show feed (dark mode), buzz (MSN-style screen shake + tone), fullscreen overlay button, and responsive mobile layout.

## Repository

https://github.com/thefangeddeity/hls-livecam-server

Current committed version on main: **2.6.3**. GitHub tag `v2.6.3` exists.
AUR repo lives at `~/aur-hls-livecam` on tina — current at v2.6.3.

Ron's workflow:
```bash
cd ~/hls-livecam-server
sudo cp /usr/local/bin/camstack pkg/usr/share/hls-livecam-server/camstack
sudo cp /usr/local/bin/camstack pkg/usr/local/bin/camstack
git add -A
git commit -m "vX.X.X: description"
git push
git tag vX.X.X && git push origin vX.X.X
```

AUR bump workflow (no makepkg on Ubuntu — write .SRCINFO manually):
```bash
cd ~/aur-hls-livecam
sed -i 's/pkgver=.*/pkgver=X.X.X/' PKGBUILD
sed -i 's/pkgrel=.*/pkgrel=1/' PKGBUILD
curl -sL https://github.com/thefangeddeity/hls-livecam-server/archive/refs/tags/vX.X.X.tar.gz | sha256sum
# update sha256sums= in PKGBUILD, write .SRCINFO manually, then:
git add PKGBUILD .SRCINFO && git commit -m "vX.X.X: ..." && git push
```

## Architecture (load-bearing, do not "simplify")

- **Peer services, not parent/child.** ffmpeg captures from `/dev/v4l/by-id/...`, pushes RTSP to `127.0.0.1:8554`. MediaMTX serves HLS on `:8888`. nginx proxies on `:80`.
- **No `-use_libv4l2 1` flag.** Ubuntu ffmpeg isn't built with it.
- `/etc/hls-livecam/device.env` contains `VIDEO_DEVICE=<path>`, `FRAMERATE=<n>`.
- **`@HOSTNAME@`** in `index.html` substituted at install time by hls-livecam-setup via sed.
- **`broadcast-api`** Flask service runs as `www-data` on `127.0.0.1:5000`, managed by systemd (`broadcast-api.service`). nginx proxies `/api/broadcast`, `/api/dark`, and `/api/buzz` to it.
- **Dark mode** flag: `/var/lib/hls-livecam/dark`. Toggle script: `/usr/local/bin/hls-livecam-dark`. The live stream keeps running underneath the overlay — no black stream service. Static cloak image: `/var/www/hls-livecam/dark.png`. Original blank dark.png backed up at `/var/lib/hls-livecam/dark_orig.png`.
- **Buzz** timestamp: `/var/www/hls-livecam/buzz.txt` (owned `root:www-data`, mode `0664`). Browser polls every 1s, triggers shake + sawtooth tone on change.
- **Framerate** is controlled via `-vf "fps=@FRAMERATE@,format=yuv420p"` in the ffmpeg unit. The `-framerate` v4l2 input flag is also present but the camera ignores it. `device.env` stores the chosen value. Setup prompts for High (30) or Low (15). **Currently running at 15fps.**

## Full service chain

```
v4l2 device → ffmpeg → RTSP (:8554) → mediamtx → HLS (:8888) → nginx (:80) → browser
                                                                      ↑
                                                              broadcast-api (:5000)
```

nginx proxies: `/api/broadcast`, `/api/dark`, `/api/buzz` → `127.0.0.1:5000`

## camstack current state (v2.6.3)

### Layout:
- **Header:** uptime, compound status pill (center), no separate Feed indicator (folded into status)
- **Top row:** MESSAGE box (left) | SYSTEM (right)
- **Bottom row:** STACK (left, narrow) | PROCESSES (right) | DISK/SMART (far right)
- **Footer:** `[m]` message · `[h]` show/hide feed · `[s]` start/stop services · `[r]` repair · `[q]` quit

### SYSTEM box:
```
CPU   XX.X%  [bar]
MEM   XX.X%  [bar]
SWAP  XX.X%  [bar]
LOAD  X.XX/4 [bar]
RAM: XXXX MB    USB: AUTO
```

### STACK box:
```
ffmpeg:   LIVE/DOWN
RTSP:     LIVE/DOWN
mediamtx: LIVE/DOWN
HLS:      LIVE/DOWN
nginx:    LIVE/DOWN
MSSG API: LIVE/DOWN
FPS:      15
REPAIR:   IDLE/RUNNING/OK/FAIL
```

### Compound header status logic:
- `• LIVE` → **RED** (exposed, camera streaming)
- `• LIVE | feed hidden` → **YELLOW** (streaming but overlay on)
- `• DEGRADED | services stopped` → **GREEN** (intentional, all clear)
- `• DEGRADED | services stopped | feed hidden` → **YELLOW** (intentional, mindful)
- `• DEGRADED | suggest repair` → **RED** (something actually broke)
- `• DOWN | suggest repair` → **RED**
- `• ERROR | suggest repair` → **RED**

### Service toggle `[s]`:
- Prompts confirm in footer (blocks on input, not a flash)
- Toggles: mediamtx, ffmpeg-cam, nginx, broadcast-api
- On stop: `systemctl stop` + `systemctl mask ffmpeg-cam` (prevents auto-restart)
- On start: `systemctl unmask ffmpeg-cam` + `systemctl start`
- `proc("ffmpeg")` scoped to processes with `8554` in cmdline — avoids matching unrelated ffmpeg processes (e.g. audio streamers)

### Color logic:
- Header uses `header_attr()` — see compound status logic above
- LIVE/OK → yellow (stack indicators)
- DOWN/ERROR/DEGRADED → red
- Everything else → green
- Load bar uses `load_attr` (green/yellow/red by load vs cores)

### Key bindings:
- `[m]` — message edit (Enter=newline, Esc=cancel, Ctrl+U=clear)
- `[h]` — toggle feed hide/show
- `[s]` — start/stop all services (confirm prompt, blocks on input)
- `[r]` — force repair (confirm prompt)

## broadcast-api endpoints

- `POST /api/broadcast` — writes broadcast.txt, max 140 chars, newlines preserved
- `GET /api/dark` → `true`/`false`
- `POST /api/dark` → calls `sudo -n hls-livecam-dark`, returns new state
- `POST /api/buzz` → writes timestamp to buzz.txt, returns timestamp

## broadcast-api systemd service

Unit file at `/etc/systemd/system/broadcast-api.service`. Runs as `www-data`, `Restart=on-failure`. Enabled at boot. Previously ran as a bare process (PID 999) — now properly managed. Also lives in `pkg/etc/systemd/system/broadcast-api.service` in the repo.

## Web viewer (index.html) — v2.6.3 state

**Do not touch unless Ron asks.**

### Desktop:
- Video left, sidebar right (260px, flex stretch, no fixed height)
- Sidebar: MESSAGE (textarea 120px) + Save/Cancel/Clear in 3-col row + Controls (Buzz, Mute Buzzes, Hide Feed) + Stream stats (Host, Resolution, Framerate) + Audio
- Stats shown: Host, Resolution, Framerate. Protocol and Uptime removed.
- Fullscreen: overlay button bottom-right of video, appears on hover

### Mobile (≤720px):
- Video 16:9 top, sidebar flows below
- Stats/audio hidden via `.desktop-only`
- Fullscreen overlay button always visible at 0.7 opacity
- Buttons: Save/Cancel/Clear 3-col grid
- iOS: native HLS, tap-to-play fallback, `touchend` for keyboard, `liftDarkOverlay()` on `loadedmetadata`

### Dark mode overlay behavior:
- Overlay image: `/dark.png` with cache-busting `?t=Date.now()` on activation
- Live stream keeps running underneath — no black stream service
- `liftDarkOverlay()` is guarded with `if (!isDark)` in ALL call sites:
  - `loadedmetadata` event handler
  - `setStatus()` when mode === 'live'
- This fixes a race condition where the video initializing before `pollDark()` returns would lift the overlay prematurely (reproducible on Safari)
- `pollDark()` called immediately on page load + every 5s

### Buzz behavior:
- Browser polls `/buzz.txt` every 1s
- On new timestamp: full-screen red/orange flash + violent shake (0.9s) + three sawtooth tones (880→660→440 Hz, stacked oscillators + compressor)
- Mute toggle stored in `localStorage` per device
- iOS mute switch blocks audio; visual shake still fires
- `navigator.vibrate()` not supported on iOS Safari

### Message behavior:
- Polls `/broadcast.txt` every 1s
- Tap textarea to edit, Save to send, Cancel to discard, Clear to erase
- Save blue only when content differs from saved; Clear blue when message exists
- Newlines supported; Enter = newline (not send)

## Hardware notes (tina)

- `/dev/sda`: 541 reallocated sectors, RISK HIGH. Replacement needed eventually.
- Webcam: physically dying — geometric frame split + green cast. Hardware, not FFmpeg.
- CPU: Intel i3-2330M (2011, 4 logical cores). Runs hot (~79°C under load). `idle_inject` throttling is normal.
- ffmpeg runs at 15fps via `-vf fps=15` to reduce CPU load. Camera ignores `-framerate` flag.
- There is an unrelated ffmpeg audio streaming process running (port 8082) — camstack correctly ignores it.

## Ideas backlog

- **Block-art freeze frame for dark.png** — attempted, shelved. Plan: on hide, grab RTSP frame with ffmpeg, run through `block_art.py` (already written, lives in repo), save as `dark.png`. Core blocker was overlay lift race condition (now fixed). Should be straightforward to retry. `block_art.py` is at `/usr/share/hls-livecam-server/block_art.py`, needs Pillow (`pip3 install Pillow --break-system-packages`).
- `[z]` buzz binding in camstack
- Fix un-hide feed not reinitializing video on mobile
- Two-camera support
- Private web admin page
- Telegram bot for remote broadcast
- Tailscale invite link generator
- Docker image
- Raspberry Pi support
- WebRTC mode
- Post to r/selfhosted, r/homelab, awesome-selfhosted
- **Test/debug on real Arch environment**
- Live FPS measurement in camstack (currently reads from device.env)
- Framerate selector in browser/camstack (arbitrary fps via `-vf fps=N`)

## The team

- **Engineer** — writes code, owns working output
- **Verifier** — fresh eyes, syntax checks
- **UX reviewer** — TUI and mobile web decisions; keep active
- **ADD angel** — Ron has adult ADHD; tight, one thing at a time, no walls of text

## How to work with Ron

- **Terse.** No apologies, no preamble.
- **Ron does not edit files manually.** Always provide scripts or sed commands.
- **Ron SCPs files from `C:\Users\Ron\Desktop\hls-livecam\` to `ron@192.168.18.60:/home/ron/Downloads`**
- **Long patches → `/tmp/fix.py`.** Heredocs get garbled for anything >10 lines.
- **One clarifying question** when design decision needed.
- **Version bump every ship:** bug-fix → patch, feature → minor.

## Mistakes to never repeat

1. Long heredocs get garbled — use a temp file for anything substantial.
2. Patch tina without syncing repo — always sync both directions.
3. Verify exact content with `sed -n 'X,Yp'` before patching — match failures waste time.
4. Unicode em-dashes in comments break string matching — use line numbers or sed instead.
5. Drop UX reviewer hat.
6. Forget `@HOSTNAME@` sed substitution after copying index.html.
7. Repeated patch failures on the same target — stop and rewrite cleanly.
8. `makepkg` not available on Ubuntu/tina — always write `.SRCINFO` manually.
9. Don't add `padding` to `overflow: hidden` containers — it breaks video layout.
10. iOS Safari: `navigator.vibrate()` not supported, Web Audio blocked by mute switch.
11. Don't use `height: calc(100vh - Xpx)` on the sidebar — breaks the entire layout.
12. Don't set `align-self: stretch` and `calc` height together — catastrophic.
13. Heredocs >10 lines get garbled on tina — always use `/tmp/fix.py` pattern.
14. Bash history expansion breaks `!` inside double-quoted strings — use `/tmp/fix.py` with single-quoted heredoc instead of inline `-c` python3 calls containing `!`.
15. `proc("ffmpeg")` is too broad — always scope to port 8554 in cmdline to avoid matching unrelated ffmpeg processes.
16. `scr.timeout(500)` causes confirm prompts to flash and self-cancel — switch to `scr.timeout(-1)` while a confirm is active.
17. Don't start a new patch chain from a stale local copy — always verify the live file on tina first with `grep` or `sed -n`.
18. Dark overlay lift is called from multiple sites — guard ALL of them with `if (!isDark)`, not just one.

## First message to Ron

Read the handoff. What's next?
