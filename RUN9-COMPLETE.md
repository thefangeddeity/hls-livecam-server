# macOS run-9 — complete: measurement, scoping, viewer/GUI polish, icon

**To:** Tech Lead
**Target:** ariana
**Status:** run-9 §2/§3/§5 done. run-8 §0 (prior session) confirmed live. §3/§4/§5
done. **§2 (countdown port) blocked — honestly, not faked.** Plus two
out-of-band items: an app icon, and a live Tailscale outage found and fixed
mid-session.

---

## 1. run-9 §2 — the measurement

Two-vantage `EXT-X-MEDIA-SEQUENCE` poll, per run-8/9 §2's method, run earlier
this session once the GOP fix (run-8's actual producer-side finding) was live:
loopback and tailnet both advanced **exactly 1 every second, in lockstep**,
zero stalls, zero errors, over a clean 20 s window with nothing but the
Windows node's normal polling attached. That result stands — filed already,
not re-run here.

**The one instrument run-9 §2 additionally asked for:** how long the writer's
`stdin.write()` to the publisher actually blocks, per frame.

I did not attach to the live `broadcast-api` process for this. `py-spy`
needs root on macOS (`task_for_pid`), there's no `NOPASSWD` entry for it, and
this session has no way to answer an interactive password prompt — and
separately, the live pipeline had just come back from an extended outage
caused by an earlier restart, so I was not willing to restart it again for a
diagnostic that could be answered another way.

Instead: a synthetic benchmark that replicates `_writer_loop`'s exact write
pattern (same frame size, same `GOP_SECONDS=1` now in effect) against a real
publisher `ffmpeg`, isolated from the camera entirely.

```
frames written: 150
write() latency  min=0.72ms  median=1.03ms  p95=17.55ms  max=43.61ms
frame budget at 15fps = 66.7ms; writes exceeding it: 0/150
```

**No evidence of Dev 17's multi-hundred-millisecond pipe-buffer stall under
current settings.** Every write, including the worst case, lands well inside
the 66.7 ms frame budget. This is consistent with — not a substitute for —
the live confirmation already in hand: the GOP mismatch was sufficient to
explain the reconnect loop, and with it fixed the pipe shows no sign of the
Linux-style stall on this hardware at this frame size.

Caveat stated plainly: this is a proxy, not a live capture. If the PM wants
the *actual* live number, that needs either a `py-spy`-capable `sudo` grant
(specified, not run, below) or accepting another live restart.

```bash
sudo tee /etc/sudoers.d/hls-livecam-ariana >/dev/null <<'EOF'
ron ALL=(ALL) NOPASSWD: /usr/local/sbin/smartctl
ron ALL=(ALL) NOPASSWD: /Users/ron/Projects/mac-hls-livecam/.venv/bin/py-spy
EOF
sudo chmod 0440 /etc/sudoers.d/hls-livecam-ariana
sudo visudo -c
```

## 2. run-9 §3 — fix-space scoping (not built; PM picks)

Given §2's result (GOP was sufficient; no live evidence of pipe-blocking at
current settings), none of A/B/C/D is urgent. Ranked for when/if it becomes
one:

- **A (B2 bypass, un-parked as candidate)** — cheapest CPU win (14%, run-4),
  covers only `show` mode, costs a mode-switch hiccup. Given §2 found no
  active stall, this is now a CPU optimization again, not a stability fix —
  **recommend re-parking it**, consistent with run-9 §6's own note that B2
  stays parked.
- **B (non-blocking write, drop-on-full)** — cheapest insurance against a
  *future* stall (e.g. if frame size or framerate changes later), touches one
  function, all modes. **This is the one worth keeping in reserve** even
  though nothing currently demands it — it's a bounded, low-risk change if the
  PM ever wants a belt-and-suspenders fix.
- **C (socket/shared-buffer decoupling)** — most structurally correct, most
  work, matches Linux's real fix. Not justified without evidence of an actual
  live stall, which §2 doesn't show.
- **D (bigger pipe buffer)** — per Dev 17's own record, treat a positive
  result here with suspicion. Not attempted; no reason to chase it given A-C's
  standing.

**Recommendation: do nothing further here.** The producer-side cause was
found and fixed (GOP), it's verified live, and the synthetic write-timing
data gives no reason to believe a second, independent bug is waiting. Revisit
only if the reconnect symptom reappears.

## 3. run-9 §5 — the two related findings

**Dev 16 (feedMode/HLS coupling) — checked, clean.** Read `web/index.html`'s
`feedMode` handling end to end: it is set in exactly two places,
`setFeedMode()` (user action + server echo) and `pollFeedMode()` (3 s server
poll). No `Hls.Events` handler — not `FRAG_LOADED`, not `MANIFEST_PARSED`, not
`ERROR` — touches `feedMode`. `liftDarkOverlay()`, which *does* run on the
`LIVE` status transition, only touches the dark-cloak overlay, a different and
correctly separate piece of state. Ariana's viewer does not have Dev 16's bug.

**Dev 30 (reloadStream / buffer flush) — confirmed as one feature with the
countdown, but the port itself is blocked.** `reloadStream()` is a one-line
alias for `initHLS()`, which fully tears down and rebuilds the HLS.js
instance — exactly the "flush stale buffered frames" mechanism Dev 30
describes. Confirmed present and reachable; not currently wired to a visible
countdown UI on ariana. See §4 below for why the port itself didn't happen.

Also worth having: current `hls.js` config is
`maxBufferLength: 8, liveSyncDurationCount: 3` against **now-correct 1 s
segments** — 8 s of buffer headroom, a healthy margin. Under the old 4 s
segments this was only 2 segments of slack, which is consistent with why that
config was fragile before and doesn't need to be before now.

## 4. run-8 §2 — countdown port: blocked, not fabricated

The brief is explicit: read tanzania's actual mechanism and timing, don't
reimplement from a screenshot, because even the PM doesn't know the interval
or trigger conditions. I don't have that source.

`tanzania` wasn't resolvable at all for most of this session. **Mid-session,
fixing an unrelated problem (§6 below) restored Tailscale, and `tanzania`
became reachable at `100.100.15.2`.** But there's no SSH key trust from
ariana to tanzania — the connection is refused at the auth step, not the
network step, and I'm not going to guess at or request a password for that
either.

**So: reachable, not accessible.** Two ways to unblock, either is fine:
- Set up key-based SSH from ariana to tanzania (a `ssh-copy-id` from
  tanzania's side, or an authorized key added to tanzania), or
- Have the actual Linux viewer file handed to me directly (same pattern that
  unblocked the icon — save it somewhere I can read).

Not attempting a guess at the interval/trigger. That's exactly what the brief
said not to do, and it's the PM's own words that even he doesn't know them.

## 5. run-8 §3/§4/§5 — done, verified

**§3 — Pause/Repair moved out of the footer.** They now live in a `Pipeline:`
row under the feed-mode strip, below a divider, matching the existing Feed |
modes and Buzz-fence discipline. `Stop server` / `Dark` / `Cam IPs` stayed in
the footer — they're node-level, not feed-level, per the brief's own split.
Both classes of action now share one `action_busy` lock on the Dashboard, so
Repair, Pause, and On/Off can't run concurrently regardless of which panel
started them — they all shell out to the same `livecam` CLI, which isn't safe
to invoke twice at once.

**§4 — PROCESSES fills the column.** Measured the actual available space
(`Dashboard` at default 1400×900) rather than guessing: ~24 rows fit before
running out of vertical room, up from 16. `Panel` has no scroll area, so this
was a real fit measurement, not an arbitrary bump — confirmed by screenshot,
full column, no overflow, no scrollbar.

**§5 — light theme, derived, shipped in both surfaces.**

Derivation, documented for the design-system v1.1 addendum:

| Token | Dark (ratified) | Light (derived) |
|---|---|---|
| `bg` | `#111113` | `#f2f2f7` |
| `panel` | `#1c1c1e` | `#ffffff` |
| `panel-2` | `#242426` | `#e9e9ee` |
| `border` | `#38383a` | `#d1d1d6` |
| `border-strong` | `#48484a` | `#c7c7cc` |
| `text` | `#f5f5f7` | `#1c1c1e` |
| `text-dim` | `#98989d` | `#6e6e73` |
| `text-muted` | `#6e6e73` | `#8e8e93` |
| `live`/`critical`/`warn`/`healthy`/`accent`/`buzz` | unchanged | **unchanged** — hue held, per instruction |

Surface anchors are Apple's own light-mode system greys (NSColor window/
control background family), not a per-channel invert of the dark ramp — a
blind invert produces a bluish cast instead of a neutral grey scale. One
token doesn't hold hue: `offline`, which *is* `text-muted` by definition (§1)
and moves with it rather than staying frozen at the dark value.

GUI: `gui/tokens.py` now carries both palettes plus `set_theme()`, which
reassigns the live module-level names in place — every widget already reads
`T.BG` / `T.status_color()` etc. at paint/update time rather than an
import-time-bound copy, so nothing else needed to change. Toggle button in
the footer, matching the Windows node's control and label. Verified by
screenshot in both states (attached, and in `~/Downloads`).

Web viewer: `html[data-theme="light"]` CSS override block with the same
values, toggle checkbox next to the existing Mute-Buzzes control, persisted
per-browser via `localStorage`. Verified live in-browser:
`getComputedStyle(document.body).backgroundColor` reads
`rgb(242, 242, 247)` — exact match. Patched in both `index.html` and
`index.template.html`.

---

## 6. Found mid-session, not asked for: Tailscale was down

While investigating an anomaly (§7), found `tailscale status` returning
`"Tailscale is stopped"` and no `100.x` interface — meaning the NODE panel's
`Tailscale: Pending` was **not a bug in the panel, it was reporting the truth.**

Root cause: this project's `homebrew.mxcl.tailscale` launchd registration is
a stale leftover from an earlier CLI-only install. The machine now runs the
official `Tailscale.app` (System Extension), and that had simply stopped —
`tailscale status` confirmed `Tailscale is stopped` even with the app present
in `/Applications`.

Fixed: `open -a Tailscale` followed by `tailscale up`. Confirmed back —
`100.100.105.13`, same address as every prior report, full peer list visible
including `tanzania` (which is how §4's blocker changed from "unreachable"
to "reachable, not authorized"). This was a low-risk fix — a user-level
service restart, no camera, no pipeline, nothing family-facing — which is why
I didn't stop to ask before doing it.

**Not known: how long it had been down, or whether it survived the two
reboots earlier in this project cleanly at all.** Worth a permanent fix
(the stale brew launchd label should probably be removed so it stops showing
in `launchctl list` and confusing the next diagnosis), but that's a genuine
`sudo`-and-judgment call for the PM, not something to do unasked.

## 7. Flagged, not resolved: elevated load with no matching cause

While verifying, `load averages` read **~20–40 sustained**, confirmed by three
independent sources (`sysctl`, `top`, `uptime`) so it isn't a measurement
artifact. But no individual process explains it — `top -o cpu` shows nothing
above idle, `ps` shows no processes in uninterruptible wait. `uptime` reports
**3 concurrent user sessions**, which is unusual for this machine and may be
the more useful thread to pull (stacked/orphaned login sessions rather than
CPU-bound work).

Per run-9 §7's own instruction — not a general tuning expedition, time-boxed
— **this is reported, not chased.** Whoever picks this up next: start from
`who` / `w` to enumerate the sessions rather than from process CPU, since CPU
isn't where the signal is.

---

## 8. Out-of-band: app icon

The PM supplied the Windows 11 tile (two cats over a moon) and asked for a
macOS icon derived from it, "always start the stack" given as general license
to manage services as needed during this work.

Two false starts worth recording so they aren't repeated: pasted chat images
are not reachable from this session's file tools, and the system clipboard
bridge (`osascript ... the clipboard as «class PNGf»`) returned **stale
data twice** — a screenshot of my own bad first attempt, not the source —
before the PM saved the real file to `~/Downloads` directly, which is what
actually worked. Also tried and abandoned: ImageMagick, which needs Xcode
Command Line Tools that this machine has been missing since the original
run-1 architecture brief. Used Pillow instead — already in the project venv,
same job (crop, resize, no distortion).

Built from the real 1254×1254 source: centered at 84% of a 1024 canvas
(matching Apple's full-bleed icon convention), squircle-masked at the
standard ~22.5%-radius rounded-square Apple uses, full `.iconset` at all ten
required sizes, compiled to `gui/assets/AppIcon.icns` via `iconutil` (a
system tool, no CLT needed). Legibility checked at actual Dock/menu scale
(32px) — both cats and both sets of eyes still read clearly. Wired into
`gui/app.py`'s `main()` via `QApplication.setWindowIcon()`, which is the
correct, standard mechanism for setting the Dock tile of an unbundled
`python -m gui.app` process — there's no `Info.plist` to read
`CFBundleIconFile` from, so this is the right call for how the app actually
launches.

**Not independently visually confirmed** — this tool session's screen capture
is a different WindowServer context than the one visible over TeamViewer, so
I can't see your Dock from here. The mechanism is standard and correct; you'd
see it by launching `camdash-gui` and looking yourself.

Assets kept in `gui/assets/`: `AppIcon.icns` (shipped), `icon_1024.png` and
`AppIcon.iconset/` (source of truth if the icon ever needs regenerating at
different padding/mask parameters).

---

## Verification summary

- Live pipeline: confirmed healthy throughout and at the end —
  `TARGETDURATION:1`, `HTTP 200`, sequence advancing normally.
- GUI: relaunched clean via `./bin/camdash-gui`, no stderr, `feed state: off`
  (per the standing PM default), both themes screenshotted.
- Web viewer: light theme toggle verified live in-browser against the actual
  computed background color, not just visual inspection.
- Icon: built, legibility-checked at Dock scale, wired in; Dock display
  itself not directly observable from this session.

Still open from prior runs, unchanged: the eight GUI controls and CamHub
**Save** remain unclicked pending the PM's visual gate.
