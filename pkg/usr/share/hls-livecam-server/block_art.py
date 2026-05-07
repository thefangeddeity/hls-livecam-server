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

import sys, os, io
from PIL import Image, ImageDraw, ImageFont

# ── Config ────────────────────────────────────────────────────────────────────
OUTPUT_W   = 1280
OUTPUT_H   = 720
COLS       = 80
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

    src = Image.open(input_path).convert("RGB")
    src = src.resize((cols, rows * 2), Image.LANCZOS)
    data = list(src.getdata())

    luma = [0.299*r + 0.587*g + 0.114*b for r,g,b in data]
    lo, rng = contrast_stretch(luma)

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

            fg = tuple(level(c, lo, rng, GAIN) for c in top)
            bg = tuple(level(c, lo, rng, 1.0)  for c in bot)

            x = int(tc * cell_w)
            y = int(tr * cell_h)

            draw.rectangle([x, y, x + int(cell_w), y + int(cell_h)], fill=bg)
            ox = x + int((cell_w - glyph_w) / 2)
            oy = y + int((cell_h - glyph_h) / 2)
            draw.text((ox, oy), "▀", font=font, fill=fg)

    canvas.save(output_path, "PNG")
    print(f"Saved {output_path} ({OUTPUT_W}x{OUTPUT_H})")

# ── Render cache (for streaming) ────────────────────────────────────────────
_render_cache = {}

def _get_render_params(cell_h):
    pt = max(4, int(cell_h * 0.85))
    if pt not in _render_cache:
        font = find_font(pt)
        probe = Image.new("RGB", (200, 200))
        pd = ImageDraw.Draw(probe)
        bbox = pd.textbbox((0, 0), "▀", font=font)
        _render_cache[pt] = (font, bbox[2]-bbox[0], bbox[3]-bbox[1])
    return pt, *_render_cache[pt]

# ── GoL renderer ─────────────────────────────────────────────────────────────
ALIVE_FG = (210, 210, 210)
ALIVE_BG = (100, 100, 100)
DEAD_FG  = (18,  18,  18)
DEAD_BG  = (10,  10,  10)

def render_gol_to_bytes(grid, cols, rows):
    """Render a GoL grid (flat list of 0/1, cols*rows) to PNG bytes."""
    cell_w = OUTPUT_W / cols
    cell_h = OUTPUT_H / rows
    pt, font, glyph_w, glyph_h = _get_render_params(cell_h)

    canvas = Image.new("RGB", (OUTPUT_W, OUTPUT_H), BG_CANVAS)
    draw = ImageDraw.Draw(canvas)

    for r in range(rows):
        for col in range(cols):
            alive = grid[r * cols + col]
            fg = ALIVE_FG if alive else DEAD_FG
            bg = ALIVE_BG if alive else DEAD_BG
            x = int(col * cell_w)
            y = int(r   * cell_h)
            draw.rectangle([x, y, x + int(cell_w), y + int(cell_h)], fill=bg)
            ox = x + int((cell_w - glyph_w) / 2)
            oy = y + int((cell_h - glyph_h) / 2)
            draw.text((ox, oy), "▀", font=font, fill=fg)

    buf = io.BytesIO()
    canvas.save(buf, "PNG")
    return buf.getvalue()


# ── Studio renderers ─────────────────────────────────────────────────────────

def render_grace_bytes(cols=COLS):
    """Static B&W noise frame — covers HLS gap during mode switch."""
    import random as _random
    rows = int(cols * (OUTPUT_H / OUTPUT_W) * 0.5)
    grid = [_random.randint(60, 180) for _ in range(cols * rows * 2)]
    cell_w = OUTPUT_W / cols
    cell_h = OUTPUT_H / rows
    pt, font, glyph_w, glyph_h = _get_render_params(cell_h)
    canvas = Image.new("RGB", (OUTPUT_W, OUTPUT_H), BG_CANVAS)
    draw = ImageDraw.Draw(canvas)
    for tr in range(rows):
        for tc in range(cols):
            fg_v = grid[(tr * 2)     * cols + tc]
            bg_v = grid[(tr * 2 + 1) * cols + tc]
            fg = (fg_v, fg_v, fg_v)
            bg = (bg_v, bg_v, bg_v)
            x = int(tc * cell_w)
            y = int(tr * cell_h)
            draw.rectangle([x, y, x + int(cell_w), y + int(cell_h)], fill=bg)
            ox = x + int((cell_w - glyph_w) / 2)
            oy = y + int((cell_h - glyph_h) / 2)
            draw.text((ox, oy), "▀", font=font, fill=fg)
    buf = io.BytesIO()
    canvas.save(buf, "PNG")
    return buf.getvalue()


def render_cloak_bytes_bw(rgb_bytes, width=1280, height=720, cols=COLS):
    """B&W block-art from raw RGB24 frame bytes."""
    rows = int(cols * (OUTPUT_H / OUTPUT_W) * 0.5)
    src = Image.frombytes('RGB', (width, height), rgb_bytes).convert('L')
    src = src.resize((cols, rows * 2), Image.LANCZOS)
    data = list(src.getdata())
    lo, rng = contrast_stretch(data)
    cell_w = OUTPUT_W / cols
    cell_h = OUTPUT_H / rows
    pt, font, glyph_w, glyph_h = _get_render_params(cell_h)
    canvas = Image.new('RGB', (OUTPUT_W, OUTPUT_H), BG_CANVAS)
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
            draw.text((ox, oy), '▀', font=font, fill=fg)
    buf = io.BytesIO()
    canvas.save(buf, 'PNG')
    return buf.getvalue()


def render_cloak_bytes(rgb_bytes, width=1280, height=720, cols=COLS):
    """Color block-art from raw RGB24 frame bytes (from ffmpeg pipe)."""
    rows = int(cols * (OUTPUT_H / OUTPUT_W) * 0.5)
    src = Image.frombytes("RGB", (width, height), rgb_bytes)
    src = src.resize((cols, rows * 2), Image.LANCZOS)
    data = list(src.getdata())
    luma = [0.299 * r + 0.587 * g + 0.114 * b for r, g, b in data]
    lo, rng = contrast_stretch(luma)
    cell_w = OUTPUT_W / cols
    cell_h = OUTPUT_H / rows
    pt, font, glyph_w, glyph_h = _get_render_params(cell_h)
    canvas = Image.new("RGB", (OUTPUT_W, OUTPUT_H), BG_CANVAS)
    draw = ImageDraw.Draw(canvas)
    for tr in range(rows):
        for tc in range(cols):
            top = data[(tr * 2)     * cols + tc]
            bot = data[(tr * 2 + 1) * cols + tc]
            fg = tuple(level(ch, lo, rng, GAIN) for ch in top)
            bg = tuple(level(ch, lo, rng, 1.0)  for ch in bot)
            x = int(tc * cell_w)
            y = int(tr * cell_h)
            draw.rectangle([x, y, x + int(cell_w), y + int(cell_h)], fill=bg)
            ox = x + int((cell_w - glyph_w) / 2)
            oy = y + int((cell_h - glyph_h) / 2)
            draw.text((ox, oy), "▀", font=font, fill=fg)
    buf = io.BytesIO()
    canvas.save(buf, "PNG")
    return buf.getvalue()

# ── Entry ─────────────────────────────────────────────────────────────────────
if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <input> <output.png> [cols]")
        sys.exit(1)
    cols = int(sys.argv[3]) if len(sys.argv) > 3 else COLS
    render(sys.argv[1], sys.argv[2], cols)
