#!/usr/bin/env python3
"""Build a macOS app icon from the product art, cloning the existing icon's shape.

macOS icons are not full-bleed squares. They sit on a 1024 canvas inset by
100px on every side -- an 823x823 squircle -- and the corner is a continuous
superellipse rather than a circular-arc rounded rectangle. Deriving that curve
analytically gets close and looks subtly wrong beside real icons.

So the shape is not computed: it is TAKEN from the icon already installed. Its
alpha channel is the mask, which makes the new icon geometrically identical to
the one it replaces and to every other icon in the Dock. Only the artwork
changes, which is the whole intent.

Usage: make-app-icon.py --art ART.png --shape OLD_ICON.png --out NEW.png
"""
import argparse
from PIL import Image
import numpy as np


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--art', required=True, help='source product artwork')
    ap.add_argument('--shape', required=True, help='existing icon to clone the silhouette from')
    ap.add_argument('--out', required=True)
    a = ap.parse_args()

    shape = Image.open(a.shape).convert('RGBA')
    W, H = shape.size
    alpha = np.array(shape)[..., 3]

    # The art must fill exactly the region the old silhouette occupies, or the
    # tile will sit inside its own mask with a visible gap.
    rows = np.where(alpha.max(1) > 8)[0]
    cols = np.where(alpha.max(0) > 8)[0]
    top, bottom, left, right = rows[0], rows[-1], cols[0], cols[-1]
    side = max(bottom - top + 1, right - left + 1)

    art = Image.open(a.art).convert('RGB')
    # The source tile bleeds to its own edges, so scaling it to the silhouette's
    # bounding box lines the two up without cropping.
    art = art.resize((side, side), Image.LANCZOS)

    canvas = Image.new('RGBA', (W, H), (0, 0, 0, 0))
    canvas.paste(art, (left, top))
    # The old alpha is the mask, so corner curvature and antialiasing come
    # across exactly rather than being approximated.
    canvas.putalpha(Image.fromarray(alpha))
    canvas.save(a.out, optimize=True)
    print(f'{a.out}: {W}x{H}, silhouette {side}x{side} inset {top}px, '
          f'shape cloned from {a.shape}')


if __name__ == '__main__':
    main()
