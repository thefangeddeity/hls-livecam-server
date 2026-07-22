#!/usr/bin/env python3
"""Generate the app icon assets from HLS-Livecam-Server.png.

Produces, next to this script:
  - icon.ico     : multi-res Windows icon (16, 32, 48, 256 px)
  - icon-256.png : 256px RGBA, for eframe's runtime with_icon

The source is two cats against a white moon on a deep-blue field with a
wide blue margin and the moon sitting off-centre. A straight downscale
would leave the moon a small off-centre blob at 16px, so the small sizes
(16/32) get a TIGHT crop to the moon; the larger sizes (48/256) keep more
of the blue composition. Crops are centred on the moon's measured
bounding box (the non-background content), not guessed pixels, so this
re-runs deterministically if the source is ever replaced.

Regenerate:  python make_icon.py
"""

from pathlib import Path
from PIL import Image, ImageChops

HERE = Path(__file__).parent
SRC = HERE / "HLS-Livecam-Server.png"

# Square side as a multiple of the moon's larger dimension.
TIGHT = 1.06  # 16/32: thin blue ring, moon fills the tile
WIDE = 1.34   # 48/256: keeps a moderate blue border / composition


def content_bbox(img):
    """Bounding box of everything that isn't the deep-blue background
    (i.e. the moon + cats + eyes), by differencing against the corner
    colour and thresholding."""
    rgb = img.convert("RGB")
    bg = rgb.getpixel((2, 2))
    diff = ImageChops.difference(rgb, Image.new("RGB", rgb.size, bg))
    mask = diff.convert("L").point(lambda p: 255 if p > 30 else 0)
    return mask.getbbox(), bg


def square_crop(img, cx, cy, side, bg):
    """Crop a centred square, padding with the background colour if the
    box runs past the image edge (keeps the moon centred either way)."""
    side = int(round(side))
    left = int(round(cx - side / 2))
    top = int(round(cy - side / 2))
    canvas = Image.new("RGBA", (side, side), bg + (255,))
    canvas.paste(img.convert("RGBA"), (-left, -top))
    return canvas


def main():
    img = Image.open(SRC).convert("RGBA")
    (l, t, r, b), bg = content_bbox(img)
    cx, cy = (l + r) / 2, (t + b) / 2
    moon = max(r - l, b - t)
    print(f"source {img.size}, moon bbox ({l},{t})-({r},{b}) -> "
          f"centre ({cx:.0f},{cy:.0f}), moon {moon}px")

    tight = square_crop(img, cx, cy, moon * TIGHT, bg)
    wide = square_crop(img, cx, cy, moon * WIDE, bg)

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

    # Report what actually landed in the .ico.
    with Image.open(ico) as check:
        sizes = sorted(check.ico.sizes())
    print(f"wrote {ico.name} sizes={sizes}")
    print(f"wrote icon-256.png {imgs[256].size}")


if __name__ == "__main__":
    main()
