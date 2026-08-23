# run-9 addendum — app icon + feed strip layout

**To:** Tech Lead
**Target:** ariana
**Status:** Done, verified, deployed. GUI-only changes; live pipeline
untouched throughout (confirmed `HTTP 200` on `index.m3u8` after every
restart in this stretch of work).

Covers a run of direct PM-in-chat fixes after the run-9 report was filed:
the app icon (six iterations) and the feed-mode strip layout (spacing +
final control order). Filed as its own addendum since none of it was in a
numbered brief.

---

## 1. App icon — six passes, each fixing a real measured defect

The PM supplied a Windows 11 taskbar tile (two cats over a moon) as the
source. Getting the actual pixels off him took two false starts worth
recording: pasted chat images aren't reachable from this session's file
tools, and the macOS clipboard bridge (`osascript ... the clipboard as
«class PNGf»`) returned **stale data twice** before he saved the file to
`~/Downloads` directly, which is what actually worked. ImageMagick was
tried and dropped — needs Xcode Command Line Tools, missing on this machine
since the original run-1 brief. Used Pillow instead, already in the venv.

Every fix below was a **measured** correction, not a re-guess:

1. **First crop was hand-eyeballed and wrong** — asymmetric single-ear
   blobs, not the source art. Fixed by pulling the clipboard PNG for real
   and cropping from actual pixel data.
2. **Crop centered on the cat silhouette bbox, not the moon** — clipped the
   moon's top edge, producing a broken arc instead of a full circle. Fixed
   by measuring the moon's true bounding circle (color-threshold scan:
   center `(627,593)`, radius `399px` in the 1254px source) and centering
   the crop on *that*.
3. **96% canvas fill looked oversized in the Dock** next to sibling icons.
   Traced to conflating two separate things: how tightly to crop the
   *source* vs. how much *canvas margin* the finished artwork should carry.
   First correction (80% fill with transparent padding) overcorrected the
   other way —
4. **— rounding became nearly invisible**, because that transparent-padding
   approach shrank the whole shape, making the corner curve proportionally
   tiny against the full tile.
5. **Full-bleed fix (art to the true canvas edges) then read as "huge again"** —
   correctly rounded, wrong scale, because full bleed applies to the
   *background*, not to how much of the frame the *subject* should occupy.
6. **Final fix: measured a real system icon rather than guessing a fourth
   time.** Extracted `Firefox.icns`, read its alpha channel directly: content
   (background + subject together) occupies **80.4%** of the 1024 canvas
   with genuine transparent padding around it — not full bleed — and the
   corner radius is **16.5% of that content box**, a plain round-rect curve.
   Rebuilt to match those two numbers exactly. This is the version that
   shipped.

**Separately, a real architectural gap: the icon didn't persist after
quitting the app — reverted to Python's default rocket.** Cause:
`QApplication.setWindowIcon()` only paints the Dock tile while the process
is alive; it can't supply what Dock/Finder show for a pinned, non-running
item, because that's read from a bundle's `Info.plist`, and a bare
`python -m gui.app` invocation has none. Fixed properly rather than working
around it: built `camdash-gui.app` at the project root — real
`Contents/Info.plist` (`CFBundleIconFile` → `AppIcon.icns`,
`CFBundleExecutable` → a launcher that execs the same venv entry point as
before), icon in `Contents/Resources/`, registered with LaunchServices.
`mdls` confirms macOS sees it as `kMDItemKind = "Application"`, not a script.

**Action needed from the PM:** whatever's currently pinned to the Dock
points at the raw Python interpreter (that's the rocket). Unpin it and drag
`camdash-gui.app` (in the project folder) to the Dock instead — icon
persistence only works once the pinned item *is* the bundle.

## 2. Feed strip — spacing fix, then reorder

**Spacing.** At the PM's actual window width the row (Feed, three mode
buttons, B&W, Pause, Repair, Buzz — six buttons, a checkbox, and one more
button in a single line) was consuming nearly all available width with
almost no margin between groups, reading as buttons crashing together even
though nothing technically overlapped. Confirmed by measuring real
`sizeHint()` widths against the actual panel width at a narrower test
window (1100px) before touching anything, rather than guessing at spacing
numbers again.

Fix: a toolbar-density treatment for this row specifically — tighter
padding and font (`density="compact"` QSS property, applied via a small
helper on each control) rather than shrinking buttons elsewhere in the app.
Same pattern real macOS toolbars use for control-dense rows (Xcode, Photos).
Gutter spacing between logical groups normalized to a consistent 10px,
replacing spacing that had drifted to mixed 4/6/10px values across earlier
edits. Verified comfortable margins at both a narrow (1100px) and the
default (1400px) window, not just technically fitting.

**Order.** Per this instruction: `Feed | Pause Repair | Show Blur Hide B&W
| ... | Buzz` — supervision actions (Pause/Repair) now sit immediately
after Feed, ahead of the mode buttons. Confirmed by screenshot.

---

## Verification

All GUI-only; the live pipeline was never touched. `HTTP 200` on
`http://127.0.0.1:8888/cam/index.m3u8` confirmed after every one of the
several `camdash-gui` relaunches in this stretch. Final layout and icon
screenshotted at both narrow and default window widths; icon legibility
checked at actual Dock/menu scale (32px), not just the 1024px master.
