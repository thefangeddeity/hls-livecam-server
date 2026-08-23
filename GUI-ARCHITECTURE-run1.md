# macOS Operator GUI — Architecture Recommendation (run-1)

**Target:** ariana (MacBookPro13,2, Intel i5-6287U 2C/4T, 16 GB)
**Date:** 2026-08-09
**Status:** Recommendation. No code written, no scaffolding created.

---

## 0. Headline

The brief asks which architecture to build. The answer is smaller than the brief
expects, because **the macOS node is already built and running.** ariana is not a
dumb push node — it is a complete, self-contained Shape A peer: local capture,
local mediamtx, local Flask control-plane, local viewer, launchd login agent,
sleep-block. It was serving HLS on `:8888` throughout this analysis.

What is missing is exactly one thing: a graphical shell over an API that already
exists and already works. The PM confirmed this during the session — *"just needs
a GUI."*

**Recommendation: Python + PySide6 (Qt 6), in-process with the existing code.**
The shared-Windows-Rust-crate hypothesis is dead — see §2. Estimated 6–8 focused
days, versus the weeks the brief's framing implies.

---

## 1. Verified ground truth (measured this session, supersedes brief §2)

Running processes on ariana at time of writing:

```
mediamtx        vendor/mediamtx  state/mediamtx.yml
broadcast-api   Python 3.14 Flask, port 8080
ffmpeg (reader) avfoundation 1280x720@15 uyvy422 → rawvideo → pipe
ffmpeg (publisher) rawvideo → libx264 ultrafast → rtsp://127.0.0.1:8554/cam
caffeinate -i -w <broadcast-api pid>
```

Live endpoint checks: `:8888/cam/index.m3u8` → **HTTP 200**. `:8080/` → **HTTP
200**. `/api/feed-mode` → **`show`**.

Toolchain inventory — this is what drives the framework decision:

| Tool | State |
|---|---|
| Python | **3.14.6** (Homebrew), venv with flask, numpy, pillow, psutil |
| PySide6 | wheel available, `cp310-abi3-macosx_13_0_universal2` — **compatible** |
| tkinter | **missing** (`_tkinter` not built; needs `brew install python-tk@3.14`) |
| Rust / cargo | **not installed** |
| Swift / swiftc | **unavailable** — `xcrun` missing at the active developer path |
| Xcode | **not installed** (CLT only, and that CLT is broken) |
| Node | **not installed** |

---

## 2. Verdict on the shared-Windows-crate hypothesis: **dead**

The brief (§4) asks to prove or kill this with the actual Windows source. It is
killed, on three independent grounds — any one is sufficient.

**The source could not be located — checked three independent ways.**
`github.com/thefangeddeity` has four public repos: `hls-livecam-server` (Python),
`arch-sdcard-updater` (Shell), `ele-messenger` (HTML), `sub-block-ascii-cam`
(Python). No Rust, nothing Windows. **`Claude/Briefs/Windows/` in Drive is
empty.** **`Claude/Reports/Windows/` contains exactly one file**,
`msi-packaging-win-v1.0.0.md` — the packaging report, not the architecture
recommendation the brief says to read first. If `camdash.exe` win-v1.0.1 exists,
neither its source nor its architecture doc is reachable from here.

The brief's instruction was to prove the hypothesis with source, not argument. The
source is unreachable, so it cannot be proven, and committing to an unverifiable
premise is strictly worse than committing to a verified one.

**The thing worth sharing does not exist on this side.** The sharing argument
assumes a Rust backend that a Rust GUI would sit on top of. There isn't one.
macOS's capture pipeline, supervision, and control-plane are Python, complete, and
running. A Rust GUI would reach that backend over HTTP on `127.0.0.1:8080` — which
is precisely what a Python GUI would do, in three lines, without a second
language or a toolchain that isn't installed.

**The real overlap is the contract, and it is already honored.** What genuinely
should be shared between the Windows and macOS nodes is `/api/broadcast`,
`/api/buzz`, `/api/feed-mode`, `/api/msg-lock`, `/api/info`, the 120-char cap, and
the LIVE/DOWN/DEGRADED vocabulary. All of it is already implemented here. Sharing
GUI widget code on top of that buys very little and costs a language boundary.

---

## 3. Framework recommendation

### Recommended: Python + PySide6 (Qt 6)

- **Zero backend work.** camdash's probe layer is already a clean client:
  `sample_metrics()` (psutil, local), `_api_get()` / POST helpers (HTTP to
  `:8080`), `_livecam()` (subprocess to the control CLI), plus direct reads of
  `state/*.txt`. A PySide6 GUI imports these functions unchanged. No IPC, no
  reimplementation, no second process.
- **Verified installable.** The `cp310-abi3` universal2 wheel requires macOS 13+.
  This machine is 14.8.7 → fine. *(Worth noting: on the Monterey 12.7.6 the brief
  assumed, this wheel would not have installed at all. The version correction in
  §6 is load-bearing, not cosmetic.)*
- **Real video, finally.** camdash's preview is a 2-second ffmpeg JPEG snapshot
  because a terminal cannot do better. Qt can: either `QMediaPlayer` against the
  HLS/RTSP URL, or an ffmpeg `rawvideo` pipe blitted into a `QImage`. The pipe
  path is the safer default — predictable, no dependence on Qt's HLS support, and
  it reuses the exact ffmpeg invocation pattern already in `broadcast-api`.
- **The Windows threading lesson is native here.** `QThread` + signals gives
  precisely the "metrics on a background thread, snapshot handoff to the render
  thread" discipline the brief's §6 demands. Qt makes the correct pattern the
  easy one.
- **The design system is expressed in CSS, and Qt consumes CSS.** The ratified
  spec is a set of CSS `:root` tokens extracted from the web viewers — colors,
  `radius`/`radius-sm`, the `.btn` / `.msg-box` / `.live-pill` / `.info-chip`
  component definitions. Qt Style Sheets are a CSS subset, so those tokens and
  component styles port nearly verbatim. egui (immediate-mode, no stylesheet
  layer) and SwiftUI would both require hand-translating every token into a
  different styling model. This was not a factor I could weigh before the design
  system surfaced; it independently reinforces the same recommendation.
- **Weak point:** distribution (a PySide6 app bundle is large and fiddly). The
  brief scopes packaging out, and this is a single-machine install, so it does
  not bite in v1.

### Rejected: Swift + SwiftUI

Best *feel* of the three, and AVKit would give real video playback for free. It
loses on toolchain cost, not merit: there is no Xcode, and the Command Line Tools
install is broken (`xcrun` missing at the active developer path), so there is no
working `swiftc` today. That is a multi-GB install plus an Xcode-vs-OCLP-Sonoma
compatibility question — to reach an HTTP API that Python already reaches
trivially, at the price of a second codebase in a second language.

Note the brief's stated objection to SwiftUI was **wrong**: there is no Monterey
API ceiling, because this machine is not on Monterey. The option still loses, but
for different reasons than the brief gives.

### Rejected: Rust + egui

See §2. Additionally: no Rust toolchain installed, and no Rust code on this
machine to share with.

### Considered, ranked second: Toga (BeeWare)

Native Cocoa widgets via PyObjC, same in-process reuse benefit as PySide6, far
smaller footprint, much better macOS packaging story. Loses on video: no mature
video surface, so the preview widget would be custom work — which is the single
highest-risk part of this build. Revisit if PySide6's footprint or Qt licensing
becomes a real constraint.

### Rejected: tkinter

Needs `brew install python-tk@3.14` first, then offers no video widget at all
(per-frame `PhotoImage` blitting) and cannot reasonably hit the design system's
type and spacing spec.

---

## 4. What of camdash actually ports

camdash is 1769 lines. The split is unintuitive and worth stating plainly,
because it is the reason this build is cheap:

**Discard entirely (~500 lines) — the most sophisticated code in the file.**
The whole terminal color engine: `_srgb_to_lab`, `_match_240`, `_kmeans_pairs`,
`_rebuild_palette`, `_build_color_frame`, `_build_gray_frame`, `_contain_fit`,
`_blit_feed`, the xterm-240 palette, the `CELL_ASPECT` calibration procedure, the
curses pair budget. Every line of it exists to force a video frame through a
character grid. A GUI decodes a frame and draws it. None of this survives — and
none of it needs replacing.

Also discarded: `layout()`, `box()`, `safe()`, footer text-wrapping, the termios
IXON handling.

**Reuse unchanged (~700 lines of logic).** `sample_metrics()`, `system_status()`,
`proc()`, `read_smart()`, `get_main_disk()`, `camera_present()`, `port_open()`,
`hls_worker()`, `_api_get()` and every POST helper, `_livecam()`,
`read_cams()` / `write_cams()`, `read_broadcast()` / `write_broadcast()`, the feed
state file handling.

**Rewrite in Qt idiom (~550 lines).** The panels themselves, the confirm-then-act
flows, the message editor, CamHub's list editing.

**Keep as settled design, do not rediscover.** The six-panel model (DISK/SMART,
FEED, SYSTEM, VIDEO, PROCESSES, MESSAGE), the status vocabulary, the
confirm-before-destructive-action pattern, the 500 ms tick, the HLS-stall
down-detection heuristic.

---

## 5. v1 feature cut

**Ships in v1:**
- All six panels, design-system styled
- Live video preview — real video, not 2 s snapshots — with a NO-SIGNAL
  placeholder that **clears the last frame** on pipeline death (brief §6)
- Feed mode: Show / Blur (cloak) / Hide, plus the B&W checkbox
- Message compose (120-char cap), lock, clear
- Buzz, dark-cloak toggle
- Repair · Pause (stop/start) · On/Off (launchd enable/disable), each behind a
  confirm
- CamHub slot editing
- Header: hostname, uptime, system status, qualifiers — on-air pill **red**, per
  design system §2/§6c (see §9)
- Unified status pill, consistent disabled affordance (`opacity 0.45` +
  not-allowed), section-title convention applied uniformly, and the "Pending"
  badge for unwired controls — all design system §7 v1 requirements

**Defers to v1.1:**
- Expanded / kiosk full-screen preview (`--expanded` equivalent)
- Menu-bar extra
- A preferences UI over `config.env` — keep `livecam setup` for now

**Out of scope entirely:**
- Packaging and notarization
- Any replacement of the Python backend
- Any change to the API contract
- Retiring camdash (see §7)

---

## 6. Corrections to the brief

Per §8.7. Several of these change decisions, not just facts.

1. **macOS version — decision-changing.** Not Monterey 12.7.6 (21H1320). This
   machine runs **Sonoma 14.8.7 (23J520)** on a MacBookPro13,2 — hardware Apple
   never supported past Monterey, so presumably OCLP. The brief's "only macOS 12
   APIs, forever" ceiling does not exist. It matters concretely: the recommended
   PySide6 wheel requires macOS 13+ and would have been unusable under the
   brief's premise.

2. **"ariana currently runs as a dumb push node" — false.** Shape A is built and
   running. Local mediamtx (RTSP `:8554`, HLS `:8888`), local Flask control-plane
   and viewer (`:8080`), local capture supervision, launchd login agent
   (`com.livecam.autostart`), `caffeinate` sleep-block tied to the API pid. The
   §3 scope fork is moot; the PM has confirmed it.

3. **Push target.** Not `rtsp://192.168.18.3:8554/ariana` (tina). It is
   `rtsp://127.0.0.1:8554/cam` — loopback into ariana's own mediamtx. ariana
   serves its own viewer and is not dependent on tina.

4. **h264_videotoolbox does not work on this machine — correction to a measured
   claim.** The brief reports hardware encode at ~1.0x realtime with CPU largely
   idle. It fails outright:
   `Cannot create compression session: -12908` (kVTCouldNotFindVideoEncoderErr).
   The encoder is compiled in (`--enable-videotoolbox`) but no hardware encode
   session can be created — consistent with an OCLP-patched Sonoma on 2016 Intel
   hardware. The shipped pipeline correctly uses `libx264 -preset ultrafast -tune
   zerolatency`. Measured 300 frames of 720p: 1.51 s real / 2.01 s user. Fine, but
   it is CPU, not silicon — so do not plan CPU budget assuming free encode.
   Hardware **decode** does work (`VTDecoderXPCServer` is active), which is the
   path the GUI preview actually needs.

5. **Port layout.** Not the fleet layout in §5. This port uses **`:8080`** for
   control-plane and viewer (not `:80` — it runs without root), `:8888` HLS,
   `:8554` RTSP. The brief calls the `cams.html` port-80 assumption immovable;
   it was already moved, via an `api_port` field in `cams.json`. §5 is stale here.

6. **Capture config.** 15 fps, not 30 (`FRAMERATE=15`). The brief's ~27 fps
   observation does not describe the current configuration.

7. **Cloak/Blur and B&W are not out of scope — they are shipped.** §8 defers them
   as "real new engineering." `block_art.py` implements color block-art, B&W
   block-art, an SVHS-warp hide mode, and a Game-of-Life renderer; `/api/feed-mode`
   and `/api/bw-mode` are live; camdash binds them to `s`/`b`/`h`/`w`. The GUI
   must expose them or it ships a regression.

8. **The §5 "confirmed blockers" are already solved.**
   `psutil.sensors_temperatures()` → degrades to `?`; `systemctl` → replaced by
   the `livecam` CLI; the v4l2 device-node check → replaced by ffmpeg avfoundation
   device enumeration; FHS paths → repo-relative. None of these are blockers; they
   are done.

9. **SMART panel is ported**, not skipped, and degrades to `N/A` on this machine's
   internal SSD. Keep the panel — it already exists and costs nothing.

10. **camdash is 1769 lines, version 3.0.0** with macOS probes — not the 1754-line
    Linux 5.x file the brief describes.

---

## 7. Lifecycle recommendation

**Dock app, launched manually. Not headless, not a launchd-managed service.**

The critical constraint: the backend **already owns its own lifecycle** via
`com.livecam.autostart` and the `livecam` CLI. The GUI must not become a second
supervisor, or two things will fight over the same pipeline. The GUI is a viewer
and controller that can be quit and relaunched while the stream keeps running —
which is exactly camdash's current contract, and it is the right one.

- **Do not add a second launchd agent** for the GUI. If the PM later wants it at
  login, use a Login Item (`SMAppService`) — but manual launch should be the
  default.
- **Menu-bar extra: v1.1.** It is the right long-term shape for an always-available
  operator control, but it is not on the critical path.

**What Business IT must provide: nothing new.** Two findings worth recording:

- **The GUI needs no camera TCC grant.** It never touches the camera —
  `broadcast-api` holds it, and the GUI reads video over HTTP/RTSP from
  `127.0.0.1`. A GUI app is a distinct TCC subject from Terminal, so this would
  otherwise have been a new permission prompt. It isn't one.
- Sleep prevention is already handled (`caffeinate -i` tied to the API pid) and
  belongs to the backend, not the GUI.

---

## 8. Complexity estimate (solo dev)

Only the GUI remains, so this is the whole job:

| Work | Estimate |
|---|---|
| Data layer: wrap existing probes in QThread + signals | ~1 day |
| Video preview widget (ffmpeg rawvideo pipe → QImage) | 1–2 days |
| Controls, confirm flows, message editor | 1–2 days |
| Design system application | ~1 day |
| Integration, NO-SIGNAL states, polish | 1–2 days |
| **Total** | **~6–8 focused days** |

The gap between this and the brief's implied scope is entirely the backend that
already exists.

---

## 9. Design system — obtained, and one behavioral consequence

`HLS Livecam Unified Design System v1` landed in `Claude/Briefs/macOS/HLSLS`
during this session. It is **RATIFIED**: §1–§5 are binding spec. Nothing is
blocked; styling can proceed.

**The one thing that is not merely styling — the LIVE pill flips to red.**
Design system §2 and §6c (explicitly ratified): *red = on-air, green =
service-up*. camdash's lineage uses **green** for the on-air state
(`header_attr()` returns `color_pair(1)`/green paths). Under the ratified model
the header on-air pill becomes **red and pulsing**, while the VIDEO panel's
per-service rows (ffmpeg / RTSP / mediamtx / HLS / web) **stay green**. These are
different facts wearing the same word. Porting camdash's colors verbatim would
ship a ratified-spec violation on the most visible element in the app.

**Binding tokens** (use names, never raw hex): surfaces `bg #111113`,
`panel #1c1c1e`, `panel-2 #242426`, `border #38383a`, `border-strong #48484a`;
text `#f5f5f7` / `text-dim #98989d` / `text-muted #6e6e73`; `accent #0a84ff`;
status `live`/`critical #ff453a`, `warn #ff9f0a`, `healthy #30d158`,
`offline #6e6e73`. Radius **12 px panels / 8 px elements**. Never color alone —
every state pairs a color with a label word.

**Design system §7 adds five v1 requirements** beyond the brief's cut, all cheap
and all folded into §5 above: the OFFLINE/NO-SIGNAL feed state (this is the
stale-frame fix's ratified home), a unified status pill across surfaces, a
consistent disabled affordance (`opacity 0.45` + not-allowed), the section-title
convention applied uniformly, and a "Pending" badge on any control whose backing
pipeline is not wired. Sparklines are explicitly **parked** — do not build them.

**Decisions to make, not resolved here:**

1. **Feed-mode naming (design system §6b, marked "PM's pick").** Proposal on the
   table: user-facing word is **Blur** everywhere, `cloak` stays the internal API
   value only, **B&W** is a modifier on Blur. This port currently says
   "Blur" + "B&W" in camdash and posts `cloak` — already consistent with the
   proposal, but it is not ratified.
2. **Preview transport — a real UX call.** RTSP gives ~1 s latency (what is
   actually happening); HLS gives ~4–7 s (what viewers actually see). These are
   different products. Recommendation: RTSP, with an indicator showing viewer
   delay — but this is the PM's.
3. **Does the GUI replace camdash, or ship alongside it?** Recommendation:
   alongside. camdash works headless over SSH; the GUI cannot. Killing camdash
   would lose remote operation.
4. **Controls in a dedicated strip vs embedded in panels** — design system §8,
   explicitly still open. The design system leans toward a Controls *section*
   rather than a separate toolbar, for cohesion with the web viewer's sidebar.
   It does not settle it, and neither does this document.
