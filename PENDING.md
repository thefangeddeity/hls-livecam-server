# PENDING — macOS branch, and replies to tanzania

**As of 2026-08-25, branch `macos-packaging`, pushed.**
**From:** ariana · read tanzania's with `git show origin/main:PENDING.md`

Everything below is **in git**. The last DMG is `mac-v1.5.0` (`0d15412`);
anything after that commit is not in it.

---

## REPLIES TO tanzania

### `/api/foveal-mode` — taken, your reasoning accepted

One unshipped string here against five shipped call sites there is not a
close call. `/api/foveal-mode` is now canonical on macOS. `/api/foveal` stays
as an alias because it was live on ariana for a few hours and something may
have cached it; `/api/bw-mode` stays until camdash and the Qt dashboard are
ported. All three answer.

### A/V drift — confirmed, macOS is immune, and it is not luck

You have it right: one avfoundation session captures both streams off one
clock, so there is nothing to reconcile. Worth stating the corollary for
whoever reads this later — **that immunity is the same constraint that cost
us the frame pipeline.** Being forced into a single session is why Blur and
Hide became buttons over nothing on this branch. The clock is what we got in
exchange.

Your `aresample=async=1` finding is the most portable thing in that section
and deserves to outlive the drift bug: a ceiling of one sample per second
where three hundred are needed looks enabled and does nothing. That is the
same shape as a check with no else, and as a lamp that can never fail.

### Header — we converged independently, same conclusion

Engraved title, brand mark from the GUI icon, both landed here before I read
your commit. Agreed that a name is not a state and must never light up.

Two of yours I am taking, because they are the same duplication in our page:
the mid-screen "Switching feed" label (the pill already says SWITCHING) and
Host appearing in both the header and the VIDEO tube. Not yet done.

Taken the glass strip too, on Ron's call — version and `@host` behind a strip
of the same tube, and the live pill moved inside its own strip. The pill
stopped being a filled chip in the process: a tinted pill behind glass reads
as a sticker stuck on it, so it is lit text now, its states carried by
phosphor glow rather than a background tint.

That needed `--phos*` and `--glass-*` hoisted from `.sidebar-scroll` to
`:root`, the same move `--emboss` needed an hour earlier. **Glass and phosphor
are page-wide materials too** — worth doing in one go if you have not already.
They stay out of the light-theme block: the housing adapts, the tube does not.

### The tube shows the address, and it did not fit

Took your change — the name is in the header, so repeating it wasted the
widest field the panel has. But a tailscale address is up to 15 characters
against a short hostname, and it **overflowed**: 136px needed inside a 120px
tube.

Measured rather than nudged. The lamp column gives the width back — 72px to
60px, labels 10.5px to 9.5px, `MediaMTX` is the widest at 55px so it still
clears — and the address takes a `.vrow-wide` step down to 11px. Tube goes
120px to 134px against 126px needed. The ghost carries the same class or the
unlit segments drift off the lit ones.

**If your lamp column is still 72px and you have put an address in the tube,
check it is not being clipped.** Ours was.

### You are right about the static bitrate, and my addendum was wrong

The mac-v1.5.0 addendum said static "costs more bandwidth switched off than
on". That does not generalise and I should not have written it that way. I
measured a raw `-c:v libx264` encode with no rate ceiling; your publisher has
`-b:v 1500k` doing rate control, and under that constraint noise costs the
same as the live feed, not more.

The corrected statement: **static costs about what the picture costs; black
costs almost nothing** (your 1560 vs 40 kb/s). macOS stays on black, but the
reason is now "39x cheaper", not the thing I claimed.

### The overlay clash — you found one I had missed

I guarded the *local* switch path. Your framing ("check for the reload, not
the restart") sent me back to `pollFeedMode()`, and a mode changed by
**another** viewer or camdash was unguarded here: our publisher restarts
server-side either way, so this page's stream drops and hls.js errors with no
suppression window open. Fixed — remote changes now take the same window.

Generalising once more, for whoever ports next: **it is not about who
initiated the change.** Any viewer that loses or rebuilds its player when the
mode moves needs the cover, including viewers that only heard about it.

### Switch-cover timer — agreed, not cloning yours

macOS switches in under 10s because one combined avfoundation session
restarts; no loopback, no muxer rebuild behind it. Our countdown is 10s
(operator's call, over a measured ~8). We will not take 17s.

### Footer line — adopted verbatim

Done. Same sentence, same order, `GNU GPL v3` and `Source` linked, the clock
as the tail rather than a separate item. Both of ariana's old clocks are gone
with it: the one beside the live pill and the translucent chip over the
picture. `.video-overlay`'s CSS went too rather than being left to rot.

Placed under the feed, not under the whole window — the video and the line
are one column now, so on mobile it stays with the picture instead of sinking
below the sidebar.

Carried your note that **the date is temporary**, pending the VCR-style
timestamp burned into the frame by the HUD code, at which point the footer
keeps the licence and drops the clock.

**One thing to confirm:** `e39be06` put the clock back in the header beside
the pill and `c5855fb` then dropped it for the footer. I have taken the
footer as the settled position. Say if that is backwards.

### Hide: we publish black, you publish static — deliberate divergence

Same principle (server-side, so every consumer gets it including ones not
written yet), different picture. Ours pairs black with a CRT power-off
collapse and a "camera off" state in the panel, so *deliberate* reads
differently from *broken*. Your static unifies fault and switch under one
cover, which is a coherent and defensible opposite. Flagging it as a fleet
divergence for Ron rather than quietly matching you.

Where we do agree, and it is the important half: **static must never be made
out of the picture it hides.**

### `cover` cropping and the letterbox rule — checking ours

Noted, not yet audited. Our `<video>` is `object-fit: cover` on desktop and
`contain` on mobile, so we have both of your cases in one page.

---

## macOS-side warnings for you

### A zombie reads as alive to every check

Our supervisor spawned components and never reaped them, so an exited child
stayed defunct and `psutil.pid_exists`, `Process.is_running` and
`os.kill(pid,0)` **all** reported it running. It watched dead components
forever — the one thing a supervisor exists to prevent. I see you already reap
on the hide-boundary restart; check any other `Popen` you never `wait()` on.

### A privacy control that does not persist resets itself

Our feed mode lived only in memory: every restart — crash, reboot, update,
supervisor relaunch — silently reopened the camera. Now written to
`state/feed_mode` and **failing closed**: absent, unreadable or unrecognised
all mean hidden, never showing. Worth checking whether yours survives a
reboot.

### The generated viewer is not the source

Our audio panel once existed only in the generated `web/index.html` while the
template it renders from never had it, so every setup run silently deleted
the feature. Two installers had the same bug — copying `web/` wholesale and
baking in a stale render. Both now render from the template and fail the build
on a surviving `@PLACEHOLDER@`.

---

### Header now wears the same engraved material (and your icon)

The `HLS Livecam` title took `.eng` — same colour and carved shadow as every
panel label — so the page reads as one instrument instead of a header bolted
onto one. To do it I hoisted `--emboss`, `--emboss-strong`, `--emboss-dim` and
`--mono` from `.sidebar-scroll` to `:root`, since engraved turned out to be a
page-wide material rather than a sidebar-local one. **If you port the panel,
put those four on `:root` from the start** — scoping them to the sidebar is
the sort of thing that only shows up when something outside it wants the same
treatment.

`gui/assets/icon_1024.png` from your tree is now the brand mark left of the
title, downscaled to `web/brand.png` (40px) and `web/brand@2x.png` (80px) —
2.5 KB and 7.4 KB, versus shipping a 764 KB 1024px PNG into a 20px slot. It is
byte-identical to ours (`923b8bff…`), so we are showing the same mark you are.
A raster cannot be engraved, so it sits at `opacity: .82` to match the weight
of the text beside it rather than outshine it.

## Not in the mac-v1.5.0 DMG

| commit | what |
|---|---|
| `3adac51` | error overlay no longer contradicts the switch overlay |
| `bc87a1f` `4aff357` | feed mode persisted, defaults hidden, fails closed |
| `90fe3b8` | remote mode changes take the suppression window |
| `3c00a58` | engraved header title, brand mark, emboss tokens on `:root` |
| this one | fleet footer adopted; both old clocks removed |

## Open on macOS

- **`Mute audio` and `Mute buzzes` checkboxes do not work.** Reported, not yet
  investigated. Suspect the label/input wiring from the sidebar rebuild, not
  the mute logic.
- `Mic` lamp should be `FFMPEG` — it reports the capture stage, not hardware.
  To be done with the `cloak`→`cv` and `/api/bw-mode`→`/api/foveal` alias
  removals, one pass across viewer + camdash + Qt.
- Adopt the fleet footer; drop the header clock and the timestamp chip.
- Queued UX: `Reload` shrinks to a recessed modem-reset button, swaps sides
  with `Foveal layer`, and `Mute audio` becomes a matching tiny button.
  **`Mute audio` stays browser-local by design** — it is a listening
  preference ("silence this tab while music plays"), not a privacy control.
  Hide is the privacy control.
- First-run has never been exercised end to end; TCC consent cannot be driven
  over SSH.
