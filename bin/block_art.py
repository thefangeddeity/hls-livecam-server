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
    # macOS monospace fonts that carry the U+2580 ▀ upper-half-block glyph.
    "/System/Library/Fonts/Menlo.ttc",
    "/System/Library/Fonts/Supplemental/Andale Mono.ttf",
    "/System/Library/Fonts/Monaco.ttf",
    "/System/Library/Fonts/Courier.ttc",
    # Homebrew / manually-installed DejaVu, if present.
    "/usr/local/share/fonts/DejaVuSansMono.ttf",
    "/opt/homebrew/share/fonts/DejaVuSansMono.ttf",
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

# ── Vectorized renderers (run-7 §3) ──────────────────────────────────────────
# The `▀` glyph IS "top half foreground, bottom half background". The font path
# renders that shape 1760 times per frame through PIL's text engine; these build
# the same grid as an array and upscale it. Measured 5.2x faster, which is the
# difference between Blur meeting its 15fps budget and running at ~9.7fps.
#
# Deliberate difference: no font means no antialiased glyph edges, so cell
# boundaries are hard. Same math otherwise -- identical 5/95 luma percentile
# stretch, identical GAIN on fg and 1.0 on bg, identical cell geometry.
import numpy as _np


def _stretch_np(luma):
    """5/95 percentile anchors, matching contrast_stretch() on the same data."""
    flat = _np.sort(luma.reshape(-1))
    n = flat.size
    lo = float(flat[int(n * 0.05)])
    hi = float(flat[int(n * 0.95)])
    return lo, (hi - lo) or 1.0


def _grid_to_canvas(cell):
    """(rows*2, cols, 3) halfblock grid -> full-size frame, nearest-neighbour.

    np.asarray() on a PIL image wraps its internal buffer read-only. That was
    invisible here -- nothing in this module writes back into the canvas --
    until cv_processor started drawing HUD/detection boxes onto blur's output
    with cv2.putText/rectangle, which need a writable Mat and fail with
    "Bad argument ... marked as readonly" on every single call. The failure
    was swallowed by the pump's per-frame error handler, which passed the
    original unblurred frame through instead -- so Blur silently never
    rendered at all, on any frame, and looked like a no-op rather than a
    crash. np.array() (copy, not view) makes the buffer this function's own.
    """
    return _np.array(
        Image.fromarray(cell, "RGB").resize((OUTPUT_W, OUTPUT_H), Image.NEAREST))


def render_cloak_vec(rgb_bytes, width=1280, height=720, cols=COLS):
    """Colour block art, vectorized. Mirrors render_cloak_bytes()."""
    rows = int(cols * (OUTPUT_H / OUTPUT_W) * 0.5)
    src = Image.frombytes("RGB", (width, height), rgb_bytes).resize(
        (cols, rows * 2), Image.LANCZOS)
    a = _np.asarray(src, dtype=_np.float32)
    luma = a @ _np.array([0.299, 0.587, 0.114], dtype=_np.float32)
    lo, rng = _stretch_np(luma)
    fg = _np.clip((a[0::2] - lo) / rng * GAIN * 255.0, 0, 255)
    bg = _np.clip((a[1::2] - lo) / rng * 1.0 * 255.0, 0, 255)
    cell = _np.empty((rows * 2, cols, 3), _np.uint8)
    cell[0::2] = fg.astype(_np.uint8)
    cell[1::2] = bg.astype(_np.uint8)
    return _grid_to_canvas(cell)


def render_cloak_vec_bw(rgb_bytes, width=1280, height=720, cols=COLS):
    """B&W block art, vectorized. Mirrors render_cloak_bytes_bw()."""
    rows = int(cols * (OUTPUT_H / OUTPUT_W) * 0.5)
    src = Image.frombytes("RGB", (width, height), rgb_bytes).convert("L").resize(
        (cols, rows * 2), Image.LANCZOS)
    g = _np.asarray(src, dtype=_np.float32)
    lo, rng = _stretch_np(g)
    fg = _np.clip((g[0::2] - lo) / rng * GAIN * 255.0, 0, 255).astype(_np.uint8)
    bg = _np.clip((g[1::2] - lo) / rng * 1.0 * 255.0, 0, 255).astype(_np.uint8)
    cell = _np.empty((rows * 2, cols, 3), _np.uint8)
    cell[0::2] = fg[..., None]
    cell[1::2] = bg[..., None]
    return _grid_to_canvas(cell)


def render_cloak_vec_bytes(rgb_bytes, width=1280, height=720, cols=COLS):
    """PNG bytes, so it drops into the same call sites as the font path."""
    buf = io.BytesIO()
    Image.fromarray(render_cloak_vec(rgb_bytes, width, height, cols)).save(buf, "PNG")
    return buf.getvalue()


def render_cloak_vec_bw_bytes(rgb_bytes, width=1280, height=720, cols=COLS):
    buf = io.BytesIO()
    Image.fromarray(render_cloak_vec_bw(rgb_bytes, width, height, cols)).save(buf, "PNG")
    return buf.getvalue()


# ── Entry ─────────────────────────────────────────────────────────────────────
if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <input> <output.png> [cols]")
        sys.exit(1)
    cols = int(sys.argv[3]) if len(sys.argv) > 3 else COLS
    render(sys.argv[1], sys.argv[2], cols)
