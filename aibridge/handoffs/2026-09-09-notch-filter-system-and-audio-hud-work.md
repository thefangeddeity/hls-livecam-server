# 2026-09-09 — Notch-filter bulk-action redesign, HUD grid system, audio findings — plan for 7elwe

Written for whoever runs Claude Code on 7elwe next. Ron: "I can run Claude Code
from it, just leave plan for it in aibridge." This session's work landed on
Tanzania (this repo) and was ported to Ariana (mac-hls-livecam, separate repo,
ssh/scp). 7elwe had no viewer as of the 2026-09-02 handoff — **check that first**,
don't assume any of this already exists there.

---

## 0. First step on 7elwe: figure out what's actually there

Before porting anything, establish ground truth:
- Does 7elwe have a livecam viewer (`index.html`) and backend (`broadcast-api`
  or equivalent) at all? The 09-02 handoff says no.
- If it's a fresh/partial install, this is a **build-out**, not a port — treat
  Tanzania (this repo, `pkg/usr/share/hls-livecam-server/index.html` +
  `pkg/usr/local/bin/broadcast-api`) as the reference implementation, same way
  Ariana's ports have worked all session: pull latest Tanzania files for
  reference, read 7elwe's actual current state, adapt rather than copy blind.
- Confirm 7elwe's platform (Linux presumably, given the fleet-repo-split memory
  — check CPU/RAM before assuming it's on `hls-livecam-server` vs the
  `hls-lightcv-server` fork; i5/8GB is the cutoff) and audio backend (ALSA like
  Tanzania, or something else).

---

## 1. What shipped on Tanzania this session (reference: this repo, commits
`1cbe07f`, `ea1edff` on `tanzania-ariana-parity`)

**Abstract HUD grid system** — `.video-hud` rebuilt as a reusable CSS grid:
e%-based row pitch (`2.71828cqh`), sqrt(2)-derived column width, two
independently-tunable padding units, `inset` (not `padding`) for clipping
because padding doesn't stop overflowing content — a real bug, found and
fixed. Arial font (matches the server's burned-in `FONT_HERSHEY_SIMPLEX`
look; there was a brief monospace detour for grid-math precision, reverted).
z-index above the static/CRT overlays so it reads through a switch
transition. Errors center over the whole picture; routine status stays in
the grid corner.

**iOS Safari audio-meter fix** — `roomAudio` now loads through a dedicated
`hls.js` instance instead of native HLS. Safari's native pipeline for an
`<audio>` element doesn't reliably feed `createMediaElementSource` — confirmed
reproducing on a real iPhone, meter read flat forever despite audio playing
fine. This is very likely relevant to 7elwe too if it gets tested on iOS.

**CV telemetry on the client HUD** — `cv_detect.hud_banner_text(tracks)`
extracted as a standalone function so the server-burned banner text and the
new client-side HUD read from the *same* source, can't drift. Exposed via
`/api/cv-state`'s `text`/`capability_text` fields. Only relevant if 7elwe
runs the CV pipeline.

**Letterbox elimination** — single feed-dimensions reference (`feedAR`,
empirical off `video.videoWidth`/`videoHeight`), drives `.video-wrap`'s own
`aspect-ratio` so it matches the feed exactly when there's room, instead of
always letterboxing via `object-fit:contain`.

## 2. The notch-filter system — the big one, and the part with real lessons

### What it is now (final form, both Tanzania and Ariana)

`/api/notches`: `GET`/`POST`/`DELETE?i=N`/`PATCH?i=N` (per-entry) — standard
CRUD, each entry `{"range":[lo,hi]}` or `{"center":Hz,"width":Hz}` +
`"note"` + optional `"enabled"` (absent = enabled). Plus:

- **`PATCH` with no `?i`** = **bulk action**, sets every entry's `enabled` to
  the given value in one write. This replaced an earlier "sticky master
  switch" design that silenced everything AND made individual entries
  unclickable while off (`pointer-events:none` on the whole list) — Ron:
  *"you should still be able to hand-select which ones you want."* **Do not
  build a master switch if 7elwe needs this from scratch — build the bulk
  action from the start.**
- **`POST /api/notches/sort`** — reorders by centre frequency, persists,
  no reload triggered (order doesn't change what's audible, filters in a
  chain are independent of each other).

UI: disclosure is a bare clone of the existing Reload-style switch, **no
LED/lamp semantics on it** — this went through several rounds of visible
frustration on Tanzania before landing there ("you're thrashing; remove
button altogether and clone Reload button, no LED"). Two bulk buttons ("Turn
on all" / "Turn off all"), a "Sort by frequency" button, per-entry rows
(`.pinled`+`.bulb` toggle, freq label, note, × remove), an add-row. The whole
box gets a sunk-in recessed look (dark bg, border, inset box-shadow) reusing
whatever the node's own recessed-element design token already is.

### The load-bearing lesson: bandreject filters lie about their own width

**Confirmed empirically, twice, with real recordings (with/without A-B
comparison, spectrum diffed):** an ffmpeg `bandreject`/SoX `bandreject`
filter's real attenuation is only deep within roughly **30-50Hz of its
stated center**, regardless of how wide the `width`/`w` parameter is set.
A "wide" notch (hundreds to thousands of Hz) looks like it covers a range on
paper but the edges of that stated range are barely touched — it's a peaked
biquad response, not a rectangular band-stop.

This was tried twice on Tanzania and failed both times:
1. One wide notch spanning a source's whole range (e.g. `[220,580]` for a
   dehumidifier) — measured: only ~370-450Hz actually attenuated, rest of the
   stated range sat 10-25dB higher than the true dip.
2. A "comb" of a few *medium-width* (180-500Hz) sub-notches tiling the same
   range — **also failed**, nearly identical numbers to the single wide
   notch, because each medium sub-notch has the same weak-shoulder problem
   individually.

**What actually worked:** many genuinely narrow notches (~50Hz wide),
individually measured off real recordings and placed at the specific
frequencies found. Tanzania's final Traffic/Hiss (broadband road noise,
8-11kHz cassette-like hiss) bands were **reverted entirely** — a notch bank
fundamentally cannot flatten truly broadband/white-ish noise no matter how
many entries you throw at it; that needs a gain-reduction EQ-cut filter
(`equalizer` or `firequalizer` in ffmpeg terms), which doesn't exist in this
codebase yet. **Task #21 (this repo's task list) is queued, not started:
research adding that as a second filter primitive.** If 7elwe hits the same
broadband-noise wall, don't reinvent wide notches — check if #21 landed
first, or hit the same dead end and note it rather than spending hours on
it like this session did.

**If 7elwe needs notch entries seeded:** don't copy Tanzania's specific
measured frequencies (dehumidifier/fan/traffic — those are Tanzania's own
room, meaningless elsewhere). Record from 7elwe's own mic and measure, same
method: Welch-averaged FFT, peak-finding with local-prominence threshold,
on/off (or before/after) difference spectra to isolate what's real vs
transient. Ask Ron what's actually in that room before guessing.

## 3. What shipped on Ariana (separate repo, for cross-reference only)

Full notch-filter API+UI ported (see above design, applied to her SoX-based
`state/notches.json`/capture chain). Her reload mechanism turned out to be a
`_mode_generation` counter she already had for the CV-blur-mode path, reused
rather than inventing a new primitive — **check what 7elwe's own reload/
respawn mechanism is before assuming Tanzania's `_audio_reload_evt` pattern
applies.**

Explicitly did NOT port to Ariana (confirmed not applicable there, check
freshly for 7elwe rather than assuming the same): Settings-panel
alphabetization (no unified Settings panel exists there), the two-column
layout restructure (separately tracked, still incomplete even on Ariana).

## 4. Also fixed this session, smaller items, may or may not apply

- Mute-mic level meter now reflects mute state (was reading the raw mic tap
  regardless of mute, gave no visual confirmation mute did anything).
- "All notch filters" → bulk actions (covered above).
- Various HUD copy/positioning fixes not worth re-deriving — read the git
  log on `tanzania-ariana-parity` (`1cbe07f`, `ea1edff`) if something looks
  off and you want the reasoning.

## Needs Ron

| | |
|---|---|
| **7elwe's actual current state** | Unknown to this session — no SSH access from here. First real step is Ron running Claude Code there and reporting back what exists. |
| **Task #21 (EQ-cut filter)** | Queued, not started. Worth doing once rather than per-node if 7elwe also needs it. |
| **Notch profile for 7elwe's room** | Needs fresh recording+measurement on-site, not copied from Tanzania. |
