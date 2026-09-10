#!/usr/bin/env python3
"""Generate the app icon assets from icon-source-2026.png.

Produces, next to this script:
  - icon.ico     : multi-res Windows icon (16, 32, 48, 256 px)
  - icon-256.png : 256px RGBA, for eframe's runtime with_icon

2026-09-09: repointed at the fleet's current canonical mark
(gui/assets/icon_1024.png on the main/macos branches, pulled via
`git show origin/main:gui/assets/icon_1024.png` -- 7elwe's old
HLS-Livecam-Server.png source predated several since-shipped art
iterations, most recently "zoom the icon art 1.15x"). Kept as the old
filename's sibling rather than overwriting it, so the previous source
stays available for comparison.

The new source adds one more layer the original crop logic didn't have
to handle: a solid BLACK outer letterbox around the navy-blue square
(the old source had no separate letterbox -- its corner pixel WAS the
navy background). strip_letterbox() removes that first; everything
after is the original two-cats-through-a-circular-opening logic
unchanged, now running against the un-letterboxed navy composition
instead of the raw source. Small sizes (16/32) get a TIGHT crop to the
opening; larger sizes (48/256) keep more of the navy composition.
Crops are centred on the opening's measured bounding box (the
non-background content), not guessed pixels, so this re-runs
deterministically if the source is ever replaced again.

Regenerate:  python make_icon.py
"""

from pathlib import Path
from PIL import Image, ImageChops

HERE = Path(__file__).parent
SRC = HERE / "icon-source-2026.png"


def strip_letterbox(img):
    """Crop out the new source's outer black margin, returning just the
    navy-blue square (ring + cats), matching the shape the rest of this
    script's cropping logic (written for the old, letterbox-free source)
    already expects.

    Scans the CENTRE row/column for the black->navy transition, not a
    corner-colour bbox diff -- a bbox diff over the whole image is fooled
    by anti-aliased/gradient edges elsewhere in the ring artwork (found
    the hard way: it picked up a stray sliver, producing a non-square,
    off-centre crop with a visible artifact). The centre row/column can
    only ever cross the letterbox border itself, so it's immune to that."""
    rgb = img.convert("RGB")
    w, h = rgb.size
    bg = rgb.getpixel((2, 2))

    def differs(px):
        return sum(abs(a - b) for a, b in zip(px, bg)) > 30

    row = [rgb.getpixel((x, h // 2)) for x in range(w)]
    col = [rgb.getpixel((w // 2, y)) for y in range(h)]
    left = next(x for x, px in enumerate(row) if differs(px))
    right = w - 1 - next(x for x, px in enumerate(reversed(row)) if differs(px))
    top = next(y for y, px in enumerate(col) if differs(px))
    bottom = h - 1 - next(y for y, px in enumerate(reversed(col)) if differs(px))
    return img.crop((left, top, right + 1, bottom + 1))

# Square side as a multiple of the opening's larger dimension.
TIGHT = 1.06  # 16/32: thin blue ring, the opening fills the tile
WIDE = 1.34   # 48/256: keeps a moderate blue border / composition


def content_bbox(img):
    """Bounding box of everything that isn't the deep-blue background
    (i.e. the opening + cats + eyes), by differencing against the corner
    colour and thresholding."""
    rgb = img.convert("RGB")
    bg = rgb.getpixel((2, 2))
    diff = ImageChops.difference(rgb, Image.new("RGB", rgb.size, bg))
    mask = diff.convert("L").point(lambda p: 255 if p > 30 else 0)
    return mask.getbbox(), bg


def square_crop(img, cx, cy, side, bg):
    """Crop a centred square, padding with the background colour if the
    box runs past the image edge (keeps the opening centred either way)."""
    side = int(round(side))
    left = int(round(cx - side / 2))
    top = int(round(cy - side / 2))
    canvas = Image.new("RGBA", (side, side), bg + (255,))
    canvas.paste(img.convert("RGBA"), (-left, -top))
    return canvas


def main():
    img = Image.open(SRC).convert("RGBA")
    img = strip_letterbox(img)
    (l, t, r, b), bg = content_bbox(img)
    cx, cy = (l + r) / 2, (t + b) / 2
    disc = max(r - l, b - t)  # the white circular opening's size
    print(f"source {img.size}, opening bbox ({l},{t})-({r},{b}) -> "
          f"centre ({cx:.0f},{cy:.0f}), opening {disc}px")

    tight = square_crop(img, cx, cy, disc * TIGHT, bg)
    wide = square_crop(img, cx, cy, disc * WIDE, bg)

    def sized(src, n):
        return src.resize((n, n), Image.LANCZOS)

    # Small sizes from the tight crop, large from the wide crop.
    imgs = {16: sized(tight, 16), 32: sized(tight, 32),
            48: sized(wide, 48), 256: sized(wide, 256)}

    ico = HERE / "icon.ico"
    imgs[256].save(
        ico, format="ICO",
        append_images=[imgs[48], imgs[32], imgs[16]],
    )
    imgs[256].save(HERE / "icon-256.png", format="PNG")
    # Same tight 32px crop the tray icon has used since "real tray icon
    # from the app mark (cropped 32px), not the placeholder" -- generated
    # here instead of hand-cropped, so it tracks the same source.
    imgs[32].save(HERE / "tray-icon-32.png", format="PNG")

    # Report what actually landed in the .ico.
    with Image.open(ico) as check:
        sizes = sorted(check.ico.sizes())
    print(f"wrote {ico.name} sizes={sizes}")
    print(f"wrote icon-256.png {imgs[256].size}")


if __name__ == "__main__":
    main()
