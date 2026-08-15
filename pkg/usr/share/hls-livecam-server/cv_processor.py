#!/usr/bin/env python3
"""
cv_processor.py -- stateful temporal CV pipeline for CV Mode.

RGB frame -> temporal buffer -> motion estimation (optical flow)
    -> motion-aware temporal denoise -> tone/exposure correction
    -> controlled sharpening -> CV display frame

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
"""

import time
from collections import deque

import numpy as np
import cv2

# Flow is computed on a downscaled grayscale pair purely to build a coarse
# motion mask -- a full-resolution dense flow field is not needed to answer
# "is this pixel neighborhood moving," and it costs several times more.
_FLOW_SCALE = 0.25

# Flow magnitude (px/frame, measured at _FLOW_SCALE resolution) above which
# a pixel neighborhood counts as "moving": skip temporal blend and sharpen
# damping there, so a moving cat is never ghosted or double-sharpened.
_MOTION_THRESHOLD = 1.5


class CVProcessor:
    """Stateful temporal enhancer. process(frame) -> (frame, metadata).

    frame is HxWx3 uint8 RGB. Retains up to HISTORY frames of history
    internally; callers do not manage state between calls.
    """

    HISTORY = 3

    def __init__(self):
        self._frames = deque(maxlen=self.HISTORY)  # raw RGB frames, newest last
        self._gray_small = deque(maxlen=self.HISTORY)  # matching downscaled gray, for flow
        self._clahe = cv2.createCLAHE(clipLimit=2.0, tileGridSize=(8, 8))

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
            _, moving_small = cv2.threshold(mag_small, _MOTION_THRESHOLD, 255, cv2.THRESH_BINARY)
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
        """CLAHE on the luminance channel only -- fixes exposure/contrast
        without shifting color balance."""
        lab = cv2.cvtColor(frame, cv2.COLOR_RGB2LAB)
        l, a, b = cv2.split(lab)
        l2 = self._clahe.apply(l)
        return cv2.cvtColor(cv2.merge((l2, a, b)), cv2.COLOR_LAB2RGB)

    def _sharpen(self, frame, moving_mask):
        """Unsharp mask via OpenCV's fused addWeighted, skipped wherever the
        motion mask is set. Motion blur there is a real optical effect from
        the exposure window, not missing detail -- sharpening it would
        amplify noise, not the subject."""
        blurred = cv2.GaussianBlur(frame, (0, 0), sigmaX=1.2)
        sharpened = cv2.addWeighted(frame, 1.6, blurred, -0.6, 0)

        if moving_mask is None or not moving_mask.any():
            return sharpened

        result = sharpened.copy()
        cv2.copyTo(frame, moving_mask, result)
        return result
