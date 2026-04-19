#!/usr/bin/env python3
"""
block_art.py — Halfblock terminal renderer → PNG
Grab one RTSP frame via ffmpeg, render as ▀ halfblock art, save as dark.png.

Usage:
    python3 block_art.py <input_image> <output.png> [cols]

Algorithm mirrors render_halfblock() from sub-block-ascii-cam:
  - Scale input to (cols x rows*2) grayscale
  - Per-frame contrast stretch: 5th/95th percentile
  - Each cell: top pixel = fg gray, bottom pixel = bg gray
  - Character ▀ rendered via PIL with fg color on bg rect
"""

import sys, os
from PIL import Image, ImageDraw, ImageFont

# ── Config ────────────────────────────────────────────────────────────────────
OUTPUT_W   = 1280
OUTPUT_H   = 720
COLS       = 160
GAIN       = 1.4   # fg contrast boost, mirrors ascii_cam default
BG_CANVAS  = (10, 10, 10)

FONT_CANDIDATES = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
]

def find_font(pt):
    for path in FONT_CANDIDATES:
        if os.path.exists(path):
            return ImageFont.truetype(path, pt)
    return ImageFont.load_default()

# ── Brightness math (mirrors _level / render_halfblock) ───────────────────────
def contrast_stretch(pixels):
    s = sorted(pixels)
    n = len(s)
    lo = s[int(n * 0.05)]
    hi = s[int(n * 0.95)]
    rng = (hi - lo) or 1
    return lo, rng

def level(val, lo, rng, gain=1.0):
    """Map pixel brightness to 0–255 gray with gain and percentile stretch."""
    t = (val - lo) / rng * gain
    return int(min(255, max(0, t * 255)))

# ── Core ──────────────────────────────────────────────────────────────────────
def render(input_path, output_path, cols=COLS):
    rows = int(cols * (OUTPUT_H / OUTPUT_W) * 0.5)  # cell_aspect ~0.45, /2 for halfblock

    src = Image.open(input_path).convert("L")
    src = src.resize((cols, rows * 2), Image.LANCZOS)
    data = list(src.getdata())

    lo, rng = contrast_stretch(data)

    cell_w = OUTPUT_W / cols
    cell_h = OUTPUT_H / rows
    pt = max(4, int(cell_h * 0.85))
    font = find_font(pt)

    probe = Image.new("RGB", (200, 200))
    pd = ImageDraw.Draw(probe)
    bbox = pd.textbbox((0, 0), "▀", font=font)
    glyph_w = bbox[2] - bbox[0]
    glyph_h = bbox[3] - bbox[1]

    canvas = Image.new("RGB", (OUTPUT_W, OUTPUT_H), BG_CANVAS)
    draw = ImageDraw.Draw(canvas)

    for tr in range(rows):
        for tc in range(cols):
            top = data[(tr * 2)     * cols + tc]
            bot = data[(tr * 2 + 1) * cols + tc]

            fg_v = level(top, lo, rng, GAIN)
            bg_v = level(bot, lo, rng, 1.0)

            fg = (fg_v, fg_v, fg_v)
            bg = (bg_v, bg_v, bg_v)

            x = int(tc * cell_w)
            y = int(tr * cell_h)

            draw.rectangle([x, y, x + int(cell_w), y + int(cell_h)], fill=bg)
            ox = x + int((cell_w - glyph_w) / 2)
            oy = y + int((cell_h - glyph_h) / 2)
            draw.text((ox, oy), "▀", font=font, fill=fg)

    canvas.save(output_path, "PNG")
    print(f"Saved {output_path} ({OUTPUT_W}x{OUTPUT_H})")

# ── Entry ─────────────────────────────────────────────────────────────────────
if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <input> <output.png> [cols]")
        sys.exit(1)
    cols = int(sys.argv[3]) if len(sys.argv) > 3 else COLS
    render(sys.argv[1], sys.argv[2], cols)
