# PENDING — Linux branch, and what other branches should check

**As of 2026-08-25, commit `eae3fc8` on `main`, pushed.**
**From:** tanzania · **Repo:** `github.com:thefangeddeity/hls-livecam-server`

Everything below is **in git**. None of it is in a tag, a `.deb`, or the AUR
package. `git pull` before assuming your copy is current — and if you are
looking at a v5.9.3 install, you are looking at none of it.

Fleet split, since it decides who this file is even for:

| repo | nodes |
|---|---|
| `hls-livecam-server` | tanzania, ariana, spitsbergen, hera, 7elwe |
| `hls-lightcv-server` | tina, amira — cutoff is i5 / 8 GB |

---

## ⚠ CHECK YOUR BRANCH FOR THESE THREE

### Your package scripts may be deleting your own configuration on upgrade

`pkg/DEBIAN/prerm` ignored `$1`. dpkg calls it on **upgrade** as well as
removal, and it did the full teardown either way: deleted all three
sudoers drop-ins that `hls-livecam-setup` writes, and left `ffmpeg-cam`,
`mediamtx` and `broadcast-api` **disabled**. `postinst` only ever chowned
those files *if they still existed*, so nothing ever said a word.

The symptom took two upgrades and a human to surface: `camdash` on tina
silently lost SMART access and had to be run under `sudo` to show disk info.
tanzania was immune the whole time because it installs from the AUR and never
runs that script — which is exactly why nobody caught it.

**If your packaging has a pre-removal hook, check whether it distinguishes
upgrade from removal.** MSI custom actions and pkg preinstall scripts have
the same trap. The fix is four lines:

```bash
case "$1" in
  remove) REMOVING=yes ;;
  *)      REMOVING=no ;;
esac
```

Teardown belongs to removal. An upgrade stops the services — their files are
about to be replaced underneath them — and touches nothing else.

### A control that fails silently is the same class of bug as one that lies

Related, and worth stating as a rule rather than an instance: `postinst`'s
loop was `if [ -f "$f" ]; then chown; fi`. That is a check with no else. Every
one of those in your tree is a place where a missing thing produces no
message. **Silence was the defect here, not the inability to fix it** —
postinst still cannot recreate those files (setup writes them with the
invoking user's name and dpkg does not know it), but it now *names* what is
missing and points at the command that restores it.

### `cv2.putText` silently substitutes `?` for glyphs it cannot draw

HERSHEY fonts have no `·` (U+00B7). The HUD read `OVERLAY ?? DEBLOCK` for a
release and change and nobody noticed, because the substitution is silent and
the string looked fine in every log. **If you draw text into a frame, ASCII
only.** No exceptions, no clever separators.

---

## What changed on Linux after v5.9.3

### Hide publishes VHS static, not black

Rendered **server-side** in `_render_hide_frame`. The web viewer could draw
its own snow — it already owns `drawStatic()` for transitions — but the Qt
dashboard pulls RTSP, camdash pulls its own frames, and anyone opening the
HLS URL directly gets whatever the server sends. One renderer covers every
consumer including the ones not written yet, and no surface can miss the memo
and show black. **The Qt dashboard needed zero code changes.**

Recipe, matching the viewer's transition static: 224×126 grey noise
regenerated per frame, alternate rows at 72% for scanlines, a dark tracking
band drifting up the frame, upscaled `INTER_NEAREST`. Derived from *nothing* —
no camera frame touches this path. Static must never be made out of the
picture it hides.

**This corrects the mac-v1.5.0 addendum.** That report measured noise at
1586–1760 kb/s and concluded static "costs more bandwidth switched off than
on". Measured on our publisher: **1560 kb/s against black's 40 kb/s** — i.e.
the same as the live feed, because `-b:v 1500k` is doing rate control that a
raw `-c:v` encode of a noise clip is not. The conclusion holds for an
unconstrained encoder and does not generalise to a rate-limited one. Render
cost is +3.6 ms/frame, paid only in a mode where nothing else runs.

### Any lost picture raises the same snow

A switch showed snow; a fault showed a flat dark panel. The two commonest
reasons for no picture had nothing in common visually, and the harmless one
looked more alarming than the serious one. Both now raise the static. The
*reason* stays where reasons belong: the "Switching feed" label appears only
for the deliberate case, and the pill separates SWITCHING from RECONNECTING.

The two causes are tracked as **separate flags** because they clear on
different events — a switch clears on the new stream's `playing`, a fault
clears when status returns to LIVE — and one must not cancel the other's
cover. If you port this, do not collapse them into one boolean.

### The "Signal lost / Retrying…" panel is retired

The static says *no usable picture* and the pill already reads RECONNECTING.
A red panel repeating that in words, on top of the snow, was the loudest
thing on the page for the most ordinary event there is.

`showError()` now only raises the cover and unhides **Reconnect** — kept,
because it is a control and not a notification, and it is the one thing that
fixes a stuck stream. macOS reached the same place from the other direction
in mac-v1.5.0 when it restored Reload as a permanent control.

**Consequence worth knowing before someone tidies it away:** the "camera is
switched off" notice in Hide is now the *only* thing separating deliberate
static from a dead feed.

### The error overlay had been contradicting the switch overlay

The clash ariana warned about, arriving here by a different route. Our
publisher is **not** restarted on a mode change, but `setFeedMode()` and
`pollFeedMode()` both call `reloadStream()` a second later, which tears down
and rebuilds the hls.js session. A fatal error in that window reached
`showError()`, which only bailed for `feedMode === 'hide'`.

**Any viewer that reloads its player on a mode change has this, not just one
that restarts capture.** Check for the reload, not for the restart.

### Timestamp off the video, and a real licence line at the foot

The timestamp was a translucent chip over the top-left of the picture — a
clock drawn by the **browser**, i.e. the viewer's own clock, laid over the
feed where it invites being read as part of it.

The foot of the page now carries, in one line and one font:

```
HLS Livecam Server by The Fanged Deity — free software under the GNU GPL v3
or later, with NO WARRANTY whatsoever. You get what you get. Source.  <date>
```

The date is the tail of that sentence, not a separate item, so it takes the
same size and colour as the rest of the line. `Source` links to the repo,
`GNU GPL v3` to the licence. **Clone this line verbatim** — the attribution
and the warranty disclaimer are the same on every node, and a fleet where
each viewer words its own licence notice is a fleet with no licence notice.

**This footer date is temporary.** The intended end state is a **VCR-style
date burned into the video feed, top left** — drawn by the same HUD code that
already writes the telemetry strip, so it comes from the *frame* and is
identical for every viewer regardless of their machine's clock. The wiring is
straightforward: `draw_hud()` in `cv_detect.py` already owns text-in-frame,
including the per-character kerning loop and the ASCII-only constraint. When
that lands, the footer keeps the licence line and drops the date.

### The HUD was being cropped off the bottom

Not a render bug. The viewer's `<video>` is `object-fit: cover`, so whenever
the player's box is shorter than 16:9 the browser crops top and bottom — and
the telemetry strip, drawn 42px from the bottom edge, was the first thing to
go. The picture was being cut *after* we drew it.

Baseline moved to **96px** (13% of a 720-high frame), which clears the crop at
the window shapes this actually gets viewed in. **Check yours**: anything
drawn near a frame edge is at the mercy of the consumer's aspect ratio, and
`cover` is the default in every viewer we ship.

---

## Room audio landed — and it is nearly free on this architecture

Ported from ariana, but the shape differs and the difference is the point.
macOS was forced into ONE combined avfoundation session because two clients
contending for the camera deadlocked ffmpeg inside `avformat_open_input` --
and that forced their Python frame pipeline out entirely, which is how Blur
and Hide became buttons over nothing.

We never had that constraint. Video already goes camera -> broadcast-api ->
`/dev/video10`, so the **publisher's** video input is a loopback device
nothing else wants, and audio is simply a second input on it. No contention,
no deadlock, CV pipeline untouched. **The seam macOS had to abandon is one
this architecture already had.** Anyone capturing direct-to-RTSP (the Windows
node does) is in macOS's position, not ours.

`AUDIO_ENABLED` defaults off. A node with no microphone publishes exactly what
it did before, byte for byte.

### Hide closes the microphone

The video half was already handled upstream -- the writer loop feeds static
into the loopback -- but **audio does not pass through that path**. Without
this the mic would keep publishing while the picture said the camera was off,
so a viewer looking at snow would still be heard. That is the
control-that-lies failure in its worst form.

Hidden swaps the ALSA input for `anullsrc`, so the device is never opened.
Verified: `fuser /dev/snd/pcmC0D0c` returns nothing, and the published
rendition measures **-91.0 dB mean AND max** -- the 16-bit silence floor, not
a quiet room.

ffmpeg inputs cannot change on a running process, so crossing the hide
boundary restarts the publisher. `show <-> cv` does not: that swap happens in
the frames written into the loopback and the publisher never notices. Only
privacy moves the process, and the blip is already covered by the switch
static. The restart **reaps** with `wait()` -- a zombie reads as alive to
every check we have.

### `/api/audio` reports three states and never guesses

`disabled` (no mic configured), `off` (mic configured but CLOSED because the
feed is hidden), `ok` (room audio going out). Calling `off` an `ok` would be
true of the process and false of the room.

The figures come from what this node told ffmpeg, **never from the manifest's
BANDWIDTH** -- that is video+audio combined and overstates audio badly, which
is the trap ariana hit wiring their own readout to it.

### The viewer needs no second stream

hls.js is already playing a variant that carries the AAC track, so listening
is unmuting the `<video>` we have. macOS had to resolve the audio-only
rendition separately because enabling sound there re-fetched the whole video
stream to throw the pictures away. Unknown state shows dashes, never a zero.

### Both mics looked broken. Neither was.

| node | as found | reads | after |
|---|---|---|---|
| tanzania | Capture +30 dB **and** Internal Mic Boost +30 dB | mean **-2.4 dB**, DC offset 0.34, flat factor 88 -- railed | Capture 26 / Boost 0 -> mean -58 dB, peak -38 dB, flat factor 0.000 |
| tina | Capture **muted**, at 0 (-74 dB) | mean **-91.0 dB** -- digital silence | Capture 60 -> real audio, quiet room -68 dB |

Opposite extremes, same cause: nobody had ever staged the input. **Flat factor
and DC offset are the discriminators**, not `mean_volume` -- a railed input
shows flat factor ~88 and DC ~0.34, real audio shows flat factor 0.000 and
DC ~0. Both persisted with `alsactl store`.

Published on both: AAC LC 48 kHz stereo, 96 kbps, as an `EXT-X-MEDIA` audio
rendition in the master manifest.

### Wanted: one control that ties picture and sound together

Right now they are independent, and deliberately so at this stage: the
transport under LISTEN starts and stops the **room audio** and never touches
the picture, and the MUTE key silences audio **in that browser only** without
telling the server or stopping the stream.

That independence is correct for the controls themselves, but there is no way
to say "follow the feed" — start the sound when the picture starts, stop it
when the picture stops. **Add a checkbox for it**, at the foot of the panel
beside `Light theme` rather than inside either media box: it governs the
relationship between the two boxes, so it does not belong to either one.

When it is on, the transport still works by hand; the checkbox only supplies
the default. Keep it local like the mute — a viewer choosing to hear the room
is not a decision for the other viewers, and nothing about it should reach
the API.

### The service user needs the `audio` group

`/dev/snd/*` is `root:audio` and the publisher runs as `http` (Arch) or
`www-data` (Debian). Without it `AUDIO_ENABLED=1` fails with `Cannot get card
index` and only the log says why -- the same silent-gap class as the prerm
bug. Both setup scripts now add it.

---

## Fleet contract — settle this before macOS ships

macOS is renaming `/api/bw-mode` → **`/api/foveal`**.
Linux already ships **`/api/foveal-mode`**, wired in five places:
`broadcast-api`, `index.html`, `camdash`, `gui/probes.py`,
`scripts/deploy_p2_tina.sh`.

**Recommendation: macOS takes `/api/foveal-mode`.** One string on a route
that has not shipped, against five call sites that have. Linux keeps `cloak`
as a permanent alias on `/api/feed-mode` for the same reason — no two nodes
in this fleet ever ship on the same day.

---

## Known holes in the Linux branch, stated plainly

- **Hide does not close the camera device.** `_drain_loop` keeps reading
  frames regardless of mode; nothing publishes them, but the device stays
  open and its light stays on. macOS's version — ffmpeg opens `lavfi` instead
  of the devices — is the stronger guarantee. Ours is "nothing from the room
  is published", not "the camera is closed". Say it that way.
- **`_drain_loop` never reaps.** It calls `terminate()` and respawns. tina
  carried two defunct ffmpeg children for two hours while every liveness
  check reported healthy. Exactly ariana's zombie warning, on our branch, the
  same day they wrote it.
- **A camera that enumerates but will not stream is invisible to us.** tina's
  webcam dropped off the USB bus (`error -71`), came back after a replug with
  `/dev/v4l/by-id/` populated and `uvcvideo` bound — and still failed
  `VIDIOC_STREAMON` with `Protocol error`. The driver held a broken streaming
  state *across* the re-enumeration; `modprobe -r uvcvideo && modprobe
  uvcvideo` cleared it. **Enumeration is not streaming.** No surface anywhere
  in our stack reported the break or the recovery. This is the case
  `/api/pipeline` exists for and we have not ported it.
- **No `/api/pipeline`, no lamp panel.** ariana's sidebar is unported. Worth
  carrying their rule — *if you can't measure a stage, don't light it green*
  — before the panel arrives rather than after.

---

## Measured this run: the illumination field

240 real frames on tina, through the real detect path, service stopped, each
configuration warmed 50 frames so the grid prior was learned before anything
was counted, capability line printed per config as proof of which branch ran.

| grid | illum | detections | max conf | mean | grid ms | illum ms |
|---|---|---|---|---|---|---|
| off | off | 25 | 0.0945 | 0.0832 | — | — |
| off | **on** | 26 | **0.1660** | 0.0790 | — | 78.9 |
| **on** | off | 29 | 0.0745 | 0.0550 | 81.6 | — |
| **on** | **on** | **43** | 0.1265 | 0.0716 | 83.5 | 78.9 |

Illumination raises peak detector response ~70%. **Grid correction alone
makes the detector less confident** while raising the raw hit count — more
things, less certainty — and costs 81.6 ms/frame for the privilege.

**Do not tune on this table.** The capture has no labels, so nothing here
says the extra 18 detections are the cat rather than 18 new false positives.
It measures *response*, not *accuracy*. And 0.166 is still deep inside the
noise floor.

Two rules from earlier phases that this does not change: **objectness, never
class, below the semantic floor**, and **no deblurring, no super-resolution,
ever.**

---

## Node state

| | tanzania | tina |
|---|---|---|
| repo | hls-livecam-server | **hls-lightcv-server** (fork) |
| HLS | 200 | 200 |
| feed mode | `cv` | `cv` |
| scene model | **`stale`** | n/a |

**tina is hot-patched and diverges from its 5.6.0 package** — every change
above is applied to its live files by hand. A package upgrade reverts them.
All of it is in git, so a real install restores it.

**tanzania's scene model has gone stale** and the viewer keeps offering
"Re-register camera". It should not have to ask: the own-camera path exists
and `/api/scene-reregister` returned `accepted: true, 0.987` unprompted. On
stale it should attempt the re-register itself, accept only above
`CV_SCENE_REREGISTER_MIN_INLIER_RATIO`, and surface the button **only when
the automatic attempt fails, with the reason**. Never silently accept a bad
homography — a confidently wrong registration is worse than none.

For whoever ports ariana's sidebar: **Linux has controls macOS does not.**
Scene badge, re-register button, Foveal Layer checkbox and the Show/CV/Hide
row all have to fit a 260px VIDEO panel designed around three buttons and a
readout. Doing the self-registration change first deletes one of them.
