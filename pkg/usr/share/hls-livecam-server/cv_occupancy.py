#!/usr/bin/env python3
"""
cv_occupancy.py -- static structure from temporal evidence alone
(CV Mode Phase 4 §5).

No classifier is involved anywhere in this module, by design. It answers
only "what tends to stay spatially stable", which is a question a degraded
sensor can still answer, and deliberately not "what is this", which it
cannot.

Everything is computed on a long-run temporal median rather than live
frames. On a static camera the median is simply a better image than any
frame it was built from: independent noise falls as sqrt(N), so a few
hundred frames materially raises the contrast of exactly the large,
low-frequency region boundaries that survive a bad lens. It cannot restore
spatial frequencies the optics never passed -- blur is deterministic, not
noise -- and nothing here pretends otherwise.

MOG2 is used strictly as a change/occupancy sensor. No notion of what a
moving thing might be is encoded here; that decision belongs to the
persistence layer, which has the temporal evidence to make it.
"""

import numpy as np
import cv2

# Everything runs on a heavily downsampled grid. Structure at this scale
# is what a degraded lens can actually support, and it keeps the whole
# module off the frame budget -- this is long-run bookkeeping, not a
# per-frame stage.
GRID_W, GRID_H = 80, 45


def validity_mask(frames, max_frames=120, sd_floor=1.0, dark_luma=40.0):
    """Per-pixel sensor validity, derived from long-run temporal statistics.

    A pixel that never changes across a live scene is not reporting the
    room -- it is dead. Measured on tina, whose sensor film has peeled:
    x=[0,201] over the full height, 12.3% of the frame, temporal sd
    exactly 0.00 and luminance exactly 0. Nothing there is data.

    That matters because every stage downstream treats invariance as
    evidence of STRUCTURE. Left unmasked, the dead strip is the single
    most temporally stable thing in the frame, so it is promoted to the
    most confident persistent entity in the room -- which is precisely
    what happened before this existed: the top-ranked static entity on
    tina was a 224x720 block of nothing.

    Returns a bool array, True where the pixel is usable. Derived rather
    than configured, so it needs no per-node calibration and tracks a
    sensor that degrades further.
    """
    if not frames:
        return None
    sel = frames if len(frames) <= max_frames else [
        frames[i] for i in np.linspace(0, len(frames) - 1, max_frames).astype(int)]
    L = np.stack([cv2.cvtColor(f, cv2.COLOR_RGB2GRAY) for f in sel]).astype(np.float32)
    sd = L.std(axis=0)
    mean = L.mean(axis=0)
    # Dark AND invariant only. Saturation is deliberately NOT treated as
    # death: a blown-out highlight is an exposure problem, not a broken
    # sensor, and it sits on a real object. Including a `mean > 250` clause
    # here masked 15.4% of tina's frame and 26% of the couch itself -- the
    # very surface the scene model exists to find -- because that couch is
    # a bright clipped white.
    dead = (sd < sd_floor) & (mean < dark_luma)
    # Grow slightly: the boundary of a dead region is a hard synthetic edge
    # and the few pixels beside it are contaminated by it.
    dead = cv2.dilate(dead.astype(np.uint8), np.ones((5, 5), np.uint8)).astype(bool)
    return ~dead


def _grid(mask):
    """Downsample a full-frame bool mask onto the occupancy grid."""
    if mask is None:
        return np.ones((GRID_H, GRID_W), bool)
    m = cv2.resize(mask.astype(np.uint8), (GRID_W, GRID_H),
                   interpolation=cv2.INTER_AREA)
    return m > 0


def temporal_median(frames, max_frames=300):
    """Long-run median. Median rather than mean so a subject that parks in
    frame for a while cannot drag the estimate of what is behind it."""
    if not frames:
        return None
    sel = frames if len(frames) <= max_frames else [
        frames[i] for i in np.linspace(0, len(frames) - 1, max_frames).astype(int)]
    return np.median(np.stack(sel).astype(np.float32), axis=0).astype(np.uint8)


def occupancy_stats(frames, var_threshold=150.0, scale=0.25, valid=None):
    """Per-cell long-run statistics on the GRID_W x GRID_H grid.

    Returns dict of HxW float arrays:
      fg_fraction     -- fraction of observations where this cell changed.
                         Its COMPLEMENT is the interesting signal: cells
                         that never change are structure. Free space is
                         where things move; occupied space is where they
                         do not.
      appearance_sd   -- temporal standard deviation of cell brightness.
                         Low = stable appearance.
      stability       -- fraction of observations within 1 sd of the
                         cell's own median: "how reliably does this cell
                         look like itself".
    """
    if not frames:
        return None
    mog = cv2.createBackgroundSubtractorMOG2(
        history=max(50, len(frames)), varThreshold=var_threshold,
        detectShadows=False)

    fg_hits = np.zeros((GRID_H, GRID_W), np.float32)
    lum = []
    for f in frames:
        small = cv2.resize(f, None, fx=scale, fy=scale, interpolation=cv2.INTER_AREA)
        m = mog.apply(small)
        m = cv2.resize(m, (GRID_W, GRID_H), interpolation=cv2.INTER_AREA)
        fg_hits += (m > 0).astype(np.float32)
        g = cv2.cvtColor(cv2.resize(f, (GRID_W, GRID_H), interpolation=cv2.INTER_AREA),
                         cv2.COLOR_RGB2GRAY)
        lum.append(g.astype(np.float32))

    n = float(len(frames))
    L = np.stack(lum)
    vg = _grid(valid)
    med = np.median(L, axis=0)
    sd = L.std(axis=0)
    within = (np.abs(L - med) <= np.maximum(sd, 1e-6)).mean(axis=0)
    # Invalid cells are reported as maximally unstable so no downstream
    # stage can mistake dead silicon for a stable surface.
    stability = within.astype(np.float32)
    stability[~vg] = 0.0
    return {
        'fg_fraction': fg_hits / n,
        'appearance_sd': sd,
        'stability': stability,
        'median_luma': med.astype(np.float32),
        'valid': vg,
        'n_frames': int(n),
    }


def candidate_regions(stats, median_img, max_regions=12,
                      min_area_frac=0.006, max_area_frac=0.60,
                      max_fg_fraction=0.35, bands=6):
    """Propose static-structure regions from stability and boundaries only.

    Segmentation is by LUMINANCE BANDING of the temporal median, not by
    edge detection. That choice is forced by the hardware: on a degraded
    lens there is no reliable fine detail, and an edge-based segmenter
    either finds nothing or -- as measured during development -- floods
    into a single region spanning the whole frame, which then "explains"
    every object trivially. What such an image still has is large areas of
    distinct brightness, so banding traces the boundaries that actually
    survive. This is the same reasoning sharpie mode already rests on.

    Both bounds matter. `max_area_frac` is not tidiness: a region covering
    most of the frame contains every object by construction and would make
    any containment-based evaluation pass without the layer having learned
    anything. A proposer that can emit one is not a proposer.

    Returns boxes in FULL-FRAME pixel coordinates.
    """
    if stats is None or median_img is None:
        return []
    h, w = median_img.shape[:2]
    cells = GRID_W * GRID_H

    vg = stats.get('valid')
    if vg is None:
        vg = np.ones_like(stats['stability'], bool)
    # Percentile over VALID cells only -- a large dead area would otherwise
    # drag the threshold down and let genuinely unstable regions through.
    valid_stab = stats['stability'][vg]
    thresh = np.percentile(valid_stab, 35) if valid_stab.size else 0.0
    stable = ((stats['stability'] > thresh) &
              (stats['fg_fraction'] < max_fg_fraction) & vg)

    g = cv2.cvtColor(cv2.resize(median_img, (GRID_W, GRID_H),
                                interpolation=cv2.INTER_AREA), cv2.COLOR_RGB2GRAY)
    g = cv2.medianBlur(g, 3)
    lo, hi = float(g.min()), float(g.max())
    if hi - lo < 1.0:
        return []
    band = np.clip(((g - lo) / (hi - lo) * bands).astype(np.int32), 0, bands - 1)

    out = []
    for b in range(bands):
        mask = ((band == b) & stable).astype(np.uint8)
        if mask.sum() == 0:
            continue
        mask = cv2.morphologyEx(mask, cv2.MORPH_OPEN, np.ones((2, 2), np.uint8))
        mask = cv2.morphologyEx(mask, cv2.MORPH_CLOSE, np.ones((3, 3), np.uint8))
        nlab, labels, st, _ = cv2.connectedComponentsWithStats(mask, 8)
        for i in range(1, nlab):
            x, y, bw, bh, area = st[i]
            frac = area / float(cells)
            if frac < min_area_frac or frac > max_area_frac:
                continue
            # A real object can legitimately fill half the frame -- tina's
            # couch does. What must still be rejected is the degenerate
            # blob that spans essentially the WHOLE frame in both axes and
            # therefore "contains" every object trivially. Size alone
            # cannot separate those two cases; spanning both dimensions
            # can.
            if bw >= 0.92 * GRID_W and bh >= 0.92 * GRID_H:
                continue
            sel = (labels[y:y+bh, x:x+bw] == i)
            sx, sy = w / float(GRID_W), h / float(GRID_H)
            out.append({
                'box': (int(x * sx), int(y * sy),
                        max(1, int(bw * sx)), max(1, int(bh * sy))),
                'grid_box': (int(x), int(y), int(bw), int(bh)),
                'area_cells': int(area),
                'area_frac': round(frac, 4),
                'band': b,
                'mean_fg_fraction': float(stats['fg_fraction'][y:y+bh, x:x+bw][sel].mean()),
                'mean_stability': float(stats['stability'][y:y+bh, x:x+bw][sel].mean()),
            })
    out.sort(key=lambda r: r['area_cells'], reverse=True)
    return out[:max_regions]
