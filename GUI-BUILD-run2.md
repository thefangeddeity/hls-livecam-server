# macOS Operator GUI — Build Report (run-2)

**To:** Tech Lead
**Target:** ariana (MacBookPro13,2, i5-6287U 2C/4T, 16 GB, Sonoma 14.8.7 / OCLP)
**Date:** 2026-08-09
**Status:** Working GUI, running on the machine. Not tagged — PM visual-gates.

---

## 0. What shipped

`camdash-gui` — a PySide6 operator dashboard, 1553 lines across six files,
running alongside the curses `camdash`, which is untouched.

```
bin/camdash-gui     12   launcher (venv-aware, execs `python -m gui.app`)
gui/tokens.py      236   design system v1 as tokens + one QSS sheet
gui/probes.py      212   camdash loaded as a module; Fast/Slow metric workers
gui/video.py       138   RTSP -> VideoToolbox decode -> QImage, stale-frame rule
gui/widgets.py     312   §5 component set (Panel, Pill, StatusRow, Meter, Chip, FeedView)
gui/app.py         643   six panels, embedded Controls section, main window
```

Launch: `./bin/camdash-gui`

All of §2's ship list is present: six panels, live RTSP preview with NO-SIGNAL
placeholder, viewer-delay indicator, Show/Blur/Hide + B&W, message compose with
120-cap/lock/clear, Buzz, Repair/Pause/On-Off behind confirms, CamHub-backed
state, header with the **red pulsing** on-air pill.

---

## 1. How it was done

### The move that made it cheap: camdash is imported, not ported

`bin/camdash` has no `.py` extension, so it loads via `SourceFileLoader`:

```python
loader = importlib.machinery.SourceFileLoader("camdash", CAMDASH_PATH)
cd = importlib.util.module_from_spec(importlib.util.spec_from_loader("camdash", loader))
loader.exec_module(cd)
```

Its module level is side-effect-free (it defines functions and reads config;
threads only start inside `main()`), so importing is safe and instant — 0.25 s.

Everything behind the GUI is therefore camdash's own code, called directly:
`cpu/mem/load/uptime/disk_write_mbs`, `proc`, `port_open`, `camera_present_cached`,
`get_hls`, `read_smart`/`smart_worker`, `hls_worker`, `_api_get`,
`read_broadcast`/`write_broadcast`, `read_device_env`, `services_running`,
`services_enabled`, `_livecam`, `system_status`, `DARK_FLAG`. Zero duplicated
logic, zero modification, and camdash keeps working headless over SSH.

### What was thrown away

The ~500-line terminal color engine — `_srgb_to_lab`, `_match_240`,
`_kmeans_pairs`, `_rebuild_palette`, `_build_color_frame`, `_build_gray_frame`,
`_contain_fit`, `_blit_feed`, the xterm-240 palette, the `CELL_ASPECT`
calibration. It is the most sophisticated code in camdash and it exists solely to
force video through a character grid. Qt draws a `QPixmap`. `FeedView.paintEvent`
replaces all of it in about 20 lines, including the contain-fit that
`_contain_fit` needed 40 lines of even-height search to approximate.

### Design system

Tokens live in exactly one place (`tokens.py`); nothing else names a hex value.
Qt Style Sheets are a CSS subset, so §1's `:root` tokens and §5's `.btn` /
`.msg-box` / `.live-pill` / `.info-chip` ported close to verbatim. Three places
where QSS is not CSS, and what was done:

| CSS in the spec | QSS reality | Resolution |
|---|---|---|
| CSS variables | none | tokens substituted via f-string |
| `transform: scale(0.98)` on `:active` | no transforms | `:pressed` fill shift |
| `opacity: 0.45` on disabled | not supported on QWidget | explicit muted fg/border on `:disabled` |

**The LIVE pill flips to red** (§2/§6c), pulsing on a 650 ms timer, while the
VIDEO panel's per-service rows stay green. That was the one behavioral change
rather than a styling one, and it is in.

### Threading

Per §3, the render thread does no probing, no process scans, no shell-outs.

- `FastWorker` (500 ms) — pure psutil counter reads: cpu, mem, load, swap,
  uptime, disk write. Microseconds each.
- `SlowWorker` (2.5 s, wakeable) — everything that walks processes, opens
  sockets, spawns ffmpeg, or does HTTP.
- `VideoWorker` — owns the decoder subprocess, emits `QImage`.
- Control actions — throwaway daemon threads via `run_async`; `repair` can take
  90 s and never touches the GUI thread.

Both metric workers emit into one merged snapshot, so panels never see a
half-populated dict. After any control action the worker's `refresh_now()` event
fires so the change appears immediately rather than up to 2.5 s later.

**Why the split, rather than just calling `sample_metrics()`:** measured, it costs
**1.21 s per call**, because `proc()` walks every process on the box and
`sample_metrics()` calls it three times — plus the GUI's own top-8 scan makes
four full process walks per tick. At GUI cadence that is not affordable. The
split keeps camdash's functions verbatim and changes only *how often* each is
called.

### Stale-frame rule

`FeedView` holds either a freshly decoded frame or the OFFLINE placeholder, never
the last texture. A short read, a dead decoder, a 4 s stall, or `svc == False`
all call `clear_signal()`, which drops the pixmap outright. The decoder respawns
on its own and only emits again once a complete frame arrives.

---

## 2. Defects found and fixed during the build

1. **Checkbox band.** `QWidget { background: BG }` cascades to `QCheckBox`, which
   then painted its own darker fill across the panel behind the label. At
   dashboard scale it read as a stray horizontal rule. Isolated it by rendering a
   bare checkbox (clean) then one inside a `Panel` (reproduced). Fixed with
   `QCheckBox { background: transparent }`.
2. **"B&W" rendered as "BW".** Qt treats `&` in a widget label as a mnemonic
   marker. Escaped to `B&&W`.
3. **Message box swallowed keystrokes.** It was the first focusable widget, so it
   took focus the moment the window opened. Set to `Qt.ClickFocus`.
4. **Font alias scan.** `"SF Pro Text"` and `-apple-system` are not registered
   family names on macOS, and Qt spent ~200 ms per launch scanning aliases for
   them. `.AppleSystemUIFont` is the same typeface under the name Qt can resolve.

---

## 3. Verification performed

Rendering was verified by grabbing the window to PNG via `QWidget.grab()`, which
needs no screen-recording permission.

**Verified working:**
- Full render with live video, all six panels populated from real data
- **NO-SIGNAL path** — pointed a `VideoWorker` at a dead RTSP URL; `has_signal`
  went `True -> False`, last frame cleared, placeholder painted. This test never
  touched the real feed.
- **Control-plane POST path end-to-end** — `bw-mode` round trip
  `false -> true -> false`, with `feed-mode` confirmed unchanged at `show`
  afterwards. `bw-mode` is invisible to viewers while the feed mode is `show`, so
  this exercised the exact `_post()` helper every control button uses without
  altering the live broadcast.
- Message box populated from the server (`View from Ariana`, 16/120) without
  clobbering in-progress edits; no longer steals focus
- Clean launch, no stderr output, process stable

**NOT verified by clicking, deliberately:** Show / Blur / Hide, Buzz, Dark,
Repair, Pause, On/Off, message Save/Clear. Every one of these changes what the
family sees, makes a sound on viewers' devices, drops the stream for ~15 s, or
changes launchd state. Their transport is the same `_post()` / `cd._livecam()`
path proven above, and each destructive one sits behind a confirm — but the
button-level round trip is unproven and that is the first thing to exercise at
the visual gate.

---

## 4. Measured cost

The §1 gate was waved off, but the baseline had already run, so here is what it
found.

**Baseline, 60 s, shipped pipeline, no GUI:** load **3.80–4.79** on 4 threads.
Pipeline total ≈ 40 % of the 400 % available (capture ffmpeg ~18 %, publish
ffmpeg ~22 %, mediamtx <1.5 %, broadcast-api ~3.6 %).

**With the GUI attached** (instantaneous, `top -l 2`):

| Process | CPU |
|---|---|
| `gui.app` (Qt + probes) | 19.8 % |
| preview `ffmpeg` (VideoToolbox decode) | 7.5 % |
| `VTDecoderXPCService` | 30.2 % — but ~30 % *before* the GUI existed; preview decode adds ≈ 0 |

**GUI total ≈ 27 % of one core ≈ 7 % of the 4-thread budget.** Hardware decode is
doing its job. The preview is affordable and the stream was serving HTTP 200
throughout.

---

## 5. Corrections to the run-2 brief

Per §5's standing instruction.

1. **§1's load figure is wrong, and it was the whole basis for the gate.** The
   brief states "load ~15.4 on 4 threads … with libx264 ultrafast running alone,
   no GUI attached." Measured over 60 s with exactly that configuration, load was
   **3.80–4.79**. The high readings correlate with `WindowServer` (97–118 %) and
   `com.teamviewer.Desktop` (26–37 %) — i.e. **remote-desktop sessions, not the
   capture pipeline**. Whoever reads load on this box while connected over
   TeamViewer will over-attribute it to the stream every time.

2. **VideoToolbox decode confirmed genuinely landing on hardware**, not silently
   falling back. `VTDecoderXPCService` is active and the preview adds ≈ 0 to it.
   The §1 concern was legitimate; the answer is favorable.

3. **Newly found, inherited from camdash, not introduced here:**
   `camera_present_cached()` spawns a full `ffmpeg -f avfoundation -list_devices`
   process every 10 s to answer "is the camera still there." It costs ~16 % CPU
   while it runs and drives visible `tccd` churn (~16 % + ~9 %), since each
   enumeration triggers a TCC check. Combined that is roughly 40 % of a core spent
   on a boolean. camdash is out of scope so it was left alone, and the GUI calls
   the cached wrapper — but this is worth a ticket. A cheap fix exists: the
   capture ffmpeg dying is already the real signal that the camera went away.

4. **`sample_metrics()` costs 1.21 s per call** (four full process walks). Not
   wrong in the brief, but not stated anywhere, and it is the single fact that
   shaped the GUI's threading design.

---

## 6. Not done

- Deferred per §2, untouched: expanded/kiosk full-screen preview, menu-bar extra,
  preferences UI over `config.env`.
- No packaging, no notarization, no launchd agent for the GUI. Manual launch, as
  §5 requires.
- No `sudo` was needed at any point, so none was requested.
- The §7 "Pending" badge component is built (`widgets.Chip`) but unused — every
  control in this build is wired to a live backend, so there is nothing pending
  to mark. It is there the moment something needs it.

**Open for the visual gate:** click-through of the eight controls listed in §3,
and a judgement on whether the FEED panel wants more of the window than the
current 4/10 column share gives it.
