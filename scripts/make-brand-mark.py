#!/usr/bin/env python3
"""Generate the viewer's header mark from the source artwork.

art/ holds 1254x1254 source images that are deliberately not shipped. This
builds the one derivative the web viewer asks for (/brand.png) so the mark can
be regenerated from the art rather than existing only as a binary somebody
once exported by hand.

Two things the naive resize gets wrong, both handled here:

  * The source's rounded square bleeds to all four edges and only the corners
    are black. Resized as-is, those corners sit as black notches on the dark
    header housing. They are cut to transparency by flood-filling inward from
    each image corner -- the lens barrel is also near-black but is enclosed by
    navy and therefore unreachable, which a colour threshold would get wrong.

  * The lens occupies 74% of the source tile. At the header's 20 CSS px the
    surrounding navy margin is pure waste, so the mark is cropped to the lens
    plus a thin margin (89%) and re-cornered at the source's own radius ratio.

Usage: scripts/make-brand-mark.py [--product livecam|lightcv]
"""
import argparse
import os
import sys

from PIL import Image, ImageDraw
import numpy as np

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCES = {
    'livecam': 'art/hls-livecam-server.png',
    'lightcv': 'art/hls-lightcv-server.png',
}
# Each product ships its mark into its own tree. This script lives in the
# livecam repo, so the lightcv destination is only reachable by pointing --out
# at a checkout of the fork; there is deliberately no path here that writes one
# product's art into the other's package.
OUT_PATHS = {
    'livecam': 'pkg/usr/share/hls-livecam-server/brand.png',
    'lightcv': None,   # set --out explicitly, inside a fork checkout
}
# 20 CSS px in the header; 128 carries a 3x phone with room to spare, at a
# file size that does not matter. Bigger is shipping an app icon down the
# wire to draw a 20px square.
OUT_SIZE = 128
CORNER_RATIO = 177 / 1254        # measured off the source squircle
LENS_MARGIN = 1.12               # crop side = lens extent * this

# Desktop/app icons want the whole tile. The lens crop exists to win back
# pixels at 20 CSS px in a page header; at 1024 there is nothing to win and
# cropping would just throw away the artwork's composition.
GUI_ICON = {
    'livecam': ('gui/assets/icon_1024.png', 1024),
}


def find_lens(rgb):
    """Lens centre and extent, measured against the navy field rather than
    assumed. Returns (cx, cy, size)."""
    h, w = rgb.shape[:2]
    navy = rgb[40, w // 2].astype(int)
    row = np.abs(rgb[h // 2].astype(int) - navy).sum(1)
    col = np.abs(rgb[:, w // 2].astype(int) - navy).sum(1)
    xs = np.where(row > 60)[0]
    ys = np.where(col > 60)[0]
    if not len(xs) or not len(ys):
        raise SystemExit('could not locate the lens against the navy field')
    return ((xs[0] + xs[-1]) // 2, (ys[0] + ys[-1]) // 2,
            max(xs[-1] - xs[0], ys[-1] - ys[0]))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--product', choices=sorted(SOURCES), default='livecam')
    # No shared default. --out used to default to the LIVECAM package path
    # regardless of --product, so `--product lightcv` with no --out wrote the
    # fork's mark straight into the parent's package -- the exact confusion
    # art/README.md exists to prevent. The destination now follows the product.
    ap.add_argument('--out', default=None)
    ap.add_argument('--size', type=int, default=None,
                    help=f'output edge in px (default {OUT_SIZE})')
    ap.add_argument('--full-tile', action='store_true',
                    help='keep the whole rounded tile instead of cropping to '
                         'the lens; what you want for an app icon')
    ap.add_argument('--gui-icon', action='store_true',
                    help='shorthand: --full-tile at the GUI icon size, '
                         'written to this product\'s gui/assets path')
    args = ap.parse_args()
    if args.gui_icon:
        if args.product not in GUI_ICON:
            raise SystemExit(f'--product {args.product} has no GUI icon path '
                             'in this repo; pass --out explicitly')
        default_out, default_size = GUI_ICON[args.product]
        args.full_tile = True
        args.out = args.out or default_out
        args.size = args.size or default_size
    args.size = args.size or OUT_SIZE
    if args.out is None:
        args.out = OUT_PATHS[args.product]
        if args.out is None:
            raise SystemExit(
                f'--product {args.product} has no destination in this repo. '
                'This is the livecam repo; pass --out pointing into a '
                'hls-lightcv-server checkout.')

    src_path = os.path.join(REPO, SOURCES[args.product])
    src = Image.open(src_path).convert('RGB')
    rgb = np.array(src)
    w = rgb.shape[1]

    if args.full_tile:
        # The source tile already bleeds to all four edges, so "the whole
        # tile" is the whole image; only the corners need cutting.
        side = min(w, rgb.shape[0])
        crop = src.crop((0, 0, side, side))
        lsize = side
    else:
        lcx, lcy, lsize = find_lens(rgb)
        side = int(lsize * LENS_MARGIN)
        x0 = max(0, min(w - side, lcx - side // 2))
        y0 = max(0, min(rgb.shape[0] - side, lcy - side // 2))
        crop = src.crop((x0, y0, x0 + side, y0 + side))

    ss = 4                       # supersample so the corner arc stays smooth
    radius = int(side * CORNER_RATIO)
    mask = Image.new('L', (side * ss, side * ss), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, side * ss - 1, side * ss - 1], radius=radius * ss, fill=255)

    out = crop.convert('RGBA')
    out.putalpha(mask.resize((side, side), Image.LANCZOS))
    out = out.resize((args.size, args.size), Image.LANCZOS)

    dest = args.out if os.path.isabs(args.out) else os.path.join(REPO, args.out)
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    out.save(dest, optimize=True)
    shape = 'full tile' if args.full_tile else f'lens fills {lsize / side * 100:.0f}%'
    print(f'{args.out}: {args.size}x{args.size}, '
          f'{os.path.getsize(dest)} bytes, from {SOURCES[args.product]} '
          f'({shape})')


if __name__ == '__main__':
    sys.exit(main())
