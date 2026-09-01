# HLSLS — VFD Phosphor Ghost Fix / Repository State
Date: 2026-08-28
Machine: tanzania
Status: PAUSED FOR CC HANDOFF

## What was discovered

The VFD uses two visual layers:

- `.vghost` — dim phosphor underlay / ghost layer
- `.vlit` — live illuminated value

CSS confirms that `.vghost` is intentionally absolutely positioned underneath `.vlit`.
The ghost effect itself is NOT to be removed.

The bug was that several VFD fields retained their static placeholder glyphs after
the live value was populated.

Examples of stale placeholders:

- FPS: `88.8`
- Audio Rate: `88.8`
- Audio Bits: `888`

These were visible behind correct live values.

## FPS fix

Tanzania live `/var/www/hls-livecam/index.html` was fixed first.

`fpsGhost` was added to the FPS ghost layer.

The FPS updater now writes the same current reading to both:

- `fpsVal`
- `fpsGhost`

This preserves the phosphor-depth effect while eliminating the stale `88.8`.

The fix was visually confirmed working.

Backup:
`/var/www/hls-livecam/index.html.bak-fps-ghost-20260828-115354`

## Audio fix

The same problem was found in Audio.

Markup now has:

- `audioRateGhost`
- `audioKbpsGhost`

`pollAudio()` updates the ghost layers from the same values used for:

- `audioRate`
- `audioKbps`

When the audio API is unavailable, both live fields and ghost fields are cleared.

The Audio fix was visually confirmed working on Tanzania.

Backup:
`/var/www/hls-livecam/index.html.bak-audio-ghost-20260828-120209`

## Repository source

The actual package/source HTML is NOT `web/index.html`.

Correct repository source:

`pkg/usr/share/hls-livecam-server/index.html`

The Audio ghost fix has been copied into that repository source.

Verified identifiers:

- `audioRateGhost`
- `audioKbpsGhost`

## HLS/iOS context

Earlier Firefox iOS problems were caused by an HLS standard mismatch, not by
MediaMTX configuration.

The working HLS implementation on Tanzania was compared against the failing
machine and the mismatch provided the clue.

Do not "fix" this by changing the server HLS configuration without evidence.

## Feed initialization

The HTML was previously changed so the page polls `/api/feed-mode` before
starting HLS.

The important invariant is:

- do not attach/start HLS until the initial feed mode is known
- if the initial mode is `hide`, do not start the player
- subsequent mode changes use the existing transition logic

This was deliberate and should not be casually reverted.

## Repository cleanup

Generated `.deb` artifacts were removed from the repository.

The previous package files under `releases/` were deleted.

CC should generate a fresh package after the current work is committed/pushed.

Repository now uses:

`models/`
for model assets such as `yolov8n.onnx`.

AI collaboration structure:

`aibridge/cc/`
`aibridge/codex/`
`aibridge/handoffs/`

Shared server-facing collaboration material:

`servercommon/`

The former `amicusbriefs` directory was eliminated.

## Screenshot cleanup

Only screenshots matching:

`HLSLS_linux-v6.0.0*`

were considered useful and retained.

Other screenshots were removed from the working tree.

`camdash-tina.png` was tracked and is now intentionally deleted.

## Current important Git state

Expected intentional changes include:

- modified `pkg/usr/share/hls-livecam-server/index.html`
- existing CV changes in `cv_detect.py`
- existing CV changes in `cv_processor.py`
- new `aibridge/`
- new `models/`
- existing reference/art material as appropriate
- deletion of obsolete generated `.deb` files
- deletion of obsolete screenshot material
- deletion of `ffmpeg-cam.service.working-reference`

DO NOT blindly discard these changes.

## Spitsbergen

Spitsbergen is a separate machine.

It is expected to be reinstalled.

After reinstall, compare its fresh repository/live HTML against the known-good
Tanzania implementation and repair the VFD ghost binding there.

Do not assume Tanzania and Spitsbergen have identical source/runtime state.

## Important conceptual conclusion

The phosphor ghost effect was intentional.

The error was inconsistent data binding.

The correct implementation is:

LIVE VALUE
    +
matching dim PHOSPHOR GHOST
    =
intended VFD appearance

Do not eliminate `.vghost` merely because it looks like duplicated text.

## Handoff state

PAUSE HERE.

Let Claude Code inspect, reconcile, commit, and push the accumulated repository
changes.

Do not rebuild the `.deb` until the repository state is intentionally committed.
