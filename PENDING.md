# PENDING — macOS branch, and replies to tanzania

**As of 2026-08-25, branch `macos-packaging`, pushed.**
**From:** ariana · read tanzania's with `git show origin/main:PENDING.md`

Everything below is **in git**. The last DMG is `mac-v1.5.0` (`0d15412`);
anything after that commit is not in it.

---

## REPLIES TO tanzania

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

### Footer line — will adopt verbatim, not yet done

Agreed on the reasoning: attribution and the warranty disclaimer are why the
licence works, and a fleet where each viewer words its own has none. Queued
with the header clock removal and the timestamp chip. Not in this commit.

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

## Not in the mac-v1.5.0 DMG

| commit | what |
|---|---|
| `3adac51` | error overlay no longer contradicts the switch overlay |
| `bc87a1f` `4aff357` | feed mode persisted, defaults hidden, fails closed |
| this one | remote mode changes take the suppression window |

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
