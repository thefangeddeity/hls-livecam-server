#!/usr/bin/env python3
"""
cv_processor.py -- stateful temporal CV pipeline for CV Mode.

RGB frame -> temporal buffer -> motion estimation (optical flow)
    -> motion-aware temporal denoise -> highlight rolloff -> tone/exposure
    correction -> controlled sharpening -> CV display frame

Stays in NumPy/OpenCV array form throughout -- no PNG/PIL round trip. That
cycle (NumPy -> bytes -> PNG -> PIL -> bytes -> NumPy) is what the cloak
render path in block_art.py does, and it measured ~190ms/frame on tanzania
against a 66.7ms budget at 15fps. CV Mode cannot afford it.

Bounded history only (HISTORY frames). A live feed has no use for stale
frames -- broadcast-api's own drain loop already discards backlog and hands
this only the newest frame, so this never sees more than it can use.

The camera is fixed (not panning), so "alignment" between frames is the
identity for the static background -- optical flow here locates *where*
motion is, to gate blending, not to warp frames into registration.

Performance note: an earlier version of this module did the motion-weighted
blend with plain NumPy float32 broadcasting (frame.astype(float32) * alpha
+ ...). It was correct but measured 130-250ms/frame on tanzania -- 3x+ over
budget. The cost was the manual float32 elementwise math, not OpenCV or the
optical flow call. Rewritten below to use a binary (not continuous) motion
mask with OpenCV's native fused ops (addWeighted, copyTo), which stay in
uint8 and run through OpenCV's own multithreaded/SIMD paths. See the
profiling harness this shipped alongside for the before/after numbers.

Per-camera tuning: every constant below that affects visible output is
read from device.env, not hardcoded -- tanzania and tina have different
cameras (and very different CPUs) and do not share good default values.
Passing no denv (or a denv missing these keys) reproduces the exact
pre-tuning defaults, so this is a no-op change for any node that hasn't
added the new keys yet.
"""

import time
from collections import deque

import numpy as np
import cv2

# Flow is computed on a downscaled grayscale pair purely to build a coarse
# motion mask -- a full-resolution dense flow field is not needed to answer
# "is this pixel neighborhood moving," and it costs several times more.
_FLOW_SCALE = 0.25

_DEFAULTS = {
    'CV_MOTION_THRESHOLD':    1.5,   # flow px/frame (at _FLOW_SCALE res) counted as "moving"
    'CV_CLAHE_CLIP':          2.0,   # cv2.createCLAHE clipLimit
    'CV_CLAHE_TILES':         8,     # cv2.createCLAHE tileGridSize, square (N x N)
    'CV_UNSHARP_AMOUNT':      0.6,   # unsharp mask strength
    'CV_UNSHARP_SIGMA':       1.2,   # unsharp mask Gaussian blur sigma ("radius")
    'CV_HIGHLIGHT_THRESHOLD': 200,   # L channel (0-255) above which rolloff engages
    'CV_HIGHLIGHT_CEILING':   255,   # 255 = disabled; <255 caps the L channel there, freeing
                                      # numeric headroom below white for CLAHE to work with
    'CV_HIGHLIGHT_GAMMA':     1.0,   # curve shape of the compression; 1.0 = linear
}


def _read_float(denv, key):
    try:
        return float(denv.get(key, _DEFAULTS[key]))
    except (TypeError, ValueError):
        return _DEFAULTS[key]


def _read_int(denv, key):
    try:
        return int(float(denv.get(key, _DEFAULTS[key])))
    except (TypeError, ValueError):
        return _DEFAULTS[key]


def _highlight_rolloff_lut(threshold, ceiling, gamma):
    """Soft-knee curve for the L channel: identity at and below `threshold`.
    Above it, remaps input range [threshold, 255] onto output range
    [threshold, ceiling] via t**gamma. With ceiling < 255 this genuinely
    compresses highlights -- a pixel clipped at 255 comes out at `ceiling`
    instead, leaving real numeric headroom below white for CLAHE's local
    contrast stretch to use, rather than a plateau with nothing left to
    redistribute. ceiling=255 and/or gamma=1.0 is the identity (disabled).
    Precomputed once per parameter set, not per frame: applying it is a
    single cv2.LUT call, not per-pixel math.
    """
    x = np.arange(256, dtype=np.float32)
    in_span = max(1e-6, 255.0 - threshold)
    out_span = max(0.0, ceiling - threshold)
    t = np.clip((x - threshold) / in_span, 0.0, 1.0)
    y = threshold + out_span * np.power(t, gamma)
    y = np.where(x <= threshold, x, y)
    return np.clip(y, 0, 255).astype(np.uint8)


class CVProcessor:
    """Stateful temporal enhancer. process(frame) -> (frame, metadata).

    frame is HxWx3 uint8 RGB. Retains up to HISTORY frames of history
    internally; callers do not manage state between calls.

    denv: the device.env dict (see broadcast-api's _read_device_env()).
    Optional -- omitting it, or omitting individual CV_* keys, reproduces
    the original hardcoded defaults exactly.
    """

    HISTORY = 3

    def __init__(self, denv=None):
        denv = denv or {}
        self._frames = deque(maxlen=self.HISTORY)  # raw RGB frames, newest last
        self._gray_small = deque(maxlen=self.HISTORY)  # matching downscaled gray, for flow

        self._motion_threshold = _read_float(denv, 'CV_MOTION_THRESHOLD')
        self._unsharp_amount = _read_float(denv, 'CV_UNSHARP_AMOUNT')
        self._unsharp_sigma = _read_float(denv, 'CV_UNSHARP_SIGMA')

        clip = _read_float(denv, 'CV_CLAHE_CLIP')
        tiles = _read_int(denv, 'CV_CLAHE_TILES')
        self._clahe = cv2.createCLAHE(clipLimit=clip, tileGridSize=(tiles, tiles))

        hi_threshold = _read_int(denv, 'CV_HIGHLIGHT_THRESHOLD')
        hi_ceiling = _read_int(denv, 'CV_HIGHLIGHT_CEILING')
        hi_gamma = _read_float(denv, 'CV_HIGHLIGHT_GAMMA')
        self._highlight_lut = _highlight_rolloff_lut(hi_threshold, hi_ceiling, hi_gamma)

    def process(self, frame):
        """frame: HxWx3 uint8 RGB. Returns (frame, metadata); output frame
        is always the same HxWx3 uint8 RGB shape as the input."""
        t = {}
        t0 = time.perf_counter()

        small_gray = cv2.cvtColor(
            cv2.resize(frame, None, fx=_FLOW_SCALE, fy=_FLOW_SCALE, interpolation=cv2.INTER_AREA),
            cv2.COLOR_RGB2GRAY,
        )
        t['downscale_gray'] = time.perf_counter() - t0

        motion_mean = 0.0
        moving_mask = None  # HxW uint8, 255 where moving, else 0
        t1 = time.perf_counter()
        if self._gray_small:
            flow = cv2.calcOpticalFlowFarneback(
                self._gray_small[-1], small_gray, None,
                pyr_scale=0.5, levels=2, winsize=13,
                iterations=2, poly_n=5, poly_sigma=1.1, flags=0,
            )
            mag_small = cv2.magnitude(flow[..., 0], flow[..., 1])
            motion_mean = float(mag_small.mean())
            _, moving_small = cv2.threshold(mag_small, self._motion_threshold, 255, cv2.THRESH_BINARY)
            moving_mask = cv2.resize(moving_small.astype(np.uint8),
                                      (frame.shape[1], frame.shape[0]),
                                      interpolation=cv2.INTER_NEAREST)
        t['flow'] = time.perf_counter() - t1

        t2 = time.perf_counter()
        denoised = self._temporal_denoise(frame, moving_mask)
        t['denoise'] = time.perf_counter() - t2

        t3 = time.perf_counter()
        toned = self._tone_correct(denoised)
        t['tone'] = time.perf_counter() - t3

        t4 = time.perf_counter()
        sharpened = self._sharpen(toned, moving_mask)
        t['sharpen'] = time.perf_counter() - t4

        self._frames.append(frame)
        self._gray_small.append(small_gray)

        t['total'] = time.perf_counter() - t0
        metadata = {
            'motion': motion_mean,
            'timings_ms': {k: round(v * 1000, 3) for k, v in t.items()},
            'history': len(self._frames),
        }
        return sharpened, metadata

    def _temporal_denoise(self, frame, moving_mask):
        """Temporally average against up to two prior frames, then restore
        the crisp current frame wherever the binary motion mask says a
        pixel is moving -- static background gets real noise reduction, a
        moving subject is composited back in untouched, never blended."""
        if not self._frames:
            return frame

        if len(self._frames) >= 2:
            avg = cv2.addWeighted(frame, 0.44, self._frames[-1], 0.28, 0)
            avg = cv2.addWeighted(avg, 1.0, self._frames[-2], 0.28, 0)
        else:
            avg = cv2.addWeighted(frame, 0.5, self._frames[-1], 0.5, 0)

        if moving_mask is None or not moving_mask.any():
            return avg

        result = avg.copy()
        cv2.copyTo(frame, moving_mask, result)
        return result

    def _tone_correct(self, frame):
        """Highlight rolloff, then CLAHE, both on the luminance channel only
        -- fixes exposure/contrast without shifting color balance. Rolloff
        runs first so CLAHE's local histogram equalization has real headroom
        to work with in a tile that's mostly blown out, instead of a flat
        plateau of saturated values with nothing left to redistribute."""
        lab = cv2.cvtColor(frame, cv2.COLOR_RGB2LAB)
        l, a, b = cv2.split(lab)
        l = cv2.LUT(l, self._highlight_lut)
        l2 = self._clahe.apply(l)
        return cv2.cvtColor(cv2.merge((l2, a, b)), cv2.COLOR_LAB2RGB)

    def _sharpen(self, frame, moving_mask):
        """Unsharp mask via OpenCV's fused addWeighted, skipped wherever the
        motion mask is set. Motion blur there is a real optical effect from
        the exposure window, not missing detail -- sharpening it would
        amplify noise, not the subject."""
        blurred = cv2.GaussianBlur(frame, (0, 0), sigmaX=self._unsharp_sigma)
        amount = self._unsharp_amount
        sharpened = cv2.addWeighted(frame, 1.0 + amount, blurred, -amount, 0)

        if moving_mask is None or not moving_mask.any():
            return sharpened

        result = sharpened.copy()
        cv2.copyTo(frame, moving_mask, result)
        return result
