# Human Mail — Project State
Date: 2026-08-28
Machine: tanzania

## Stopped here intentionally

Tanzania's live VFD has been corrected.

The visible phosphor "shadow" is an intentional part of the VFD design. It was
not supposed to be removed. The problem was that some fields still displayed
the placeholder glyphs used by the ghost layer.

Fixed and visually confirmed:

- FPS
- Audio Rate
- Audio Bits

The repository package source has also been updated for the Audio fix.

## Repository organization

AI collaboration:

- `aibridge/cc`
- `aibridge/codex`
- `aibridge/handoffs`

Models:

- `models/`

Server-common material:

- `servercommon/`

The old `amicusbriefs` naming was eliminated.

Generated `.deb` files were removed from the repository. CC can generate a new
package after the repository work is committed/pushed.

## Cleanup

Only useful HLSLS Linux v6 screenshots remain:

`HLSLS_linux-v6.0.0*`

The pile of obsolete screenshots and old `.deb` artifacts was intentionally
removed.

## Next machine

Spitsbergen is going to be reinstalled.

After reinstall, the VFD implementation should be compared against the now-known
good Tanzania behavior. The goal is to preserve the phosphor effect and make
the ghost layer track the live values correctly.

## Current instruction

Pause implementation here.

Let Claude Code pick up the repository state, inspect it, document/reconcile
its existing changes, and handle the eventual commit/push.

Do not rebuild or package prematurely.
