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

try:
    import cv_detect as _cvd
except Exception:      # detection is optional; the pipeline runs without it
    _cvd = None

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
    'CV_SHADOW_LIFT':         0.0,   # 0 = off. Raises dark values so detail in
                                      # shadow survives; a dark subject against a
                                      # bright surface is otherwise crushed to a
                                      # silhouette while the rolloff only helps
                                      # the bright end.
    'CV_SHADOW_RANGE':        110,   # L value below which the lift applies

    # Adaptive edge mode. Below a sharpness threshold, enhancement stops
    # helping -- CLAHE on mush is still mush -- so the pipeline switches to
    # drawing the structure it can still find. Threshold-driven and
    # automatic, so the same config behaves correctly on a sharp camera and
    # a soft one without per-install tuning.
    'CV_EDGE_ENABLED':        1,     # 0 disables the stage entirely
    'CV_EDGE_SHARPNESS_MIN':  35.0,  # variance-of-Laplacian below which edges engage
    'CV_EDGE_SHARPNESS_OFF':  70.0,  # ...and at/above which they are fully absent
    'CV_EDGE_STRENGTH':       0.75,  # opacity of the darkest strokes at full engagement
    'CV_EDGE_SIGMA':          1.0,   # base stroke width; scaled up as the image softens
    'CV_EDGE_SIGMA_MAX':      3.5,   # cap, so a very soft frame does not draw slabs
    'CV_EDGE_PERCENTILE':     99.0,  # DoG normalisation reference; lower = heavier ink
    'CV_EDGE_GAMMA':          0.65,  # <1 lifts mid-strength edges toward full ink

    # Edge style. 'overlay' inks strokes over the photo. 'sharpie' discards
    # the photo entirely and returns a line drawing on white.
    #
    # Sharpie exists because on a genuinely defocused lens there is no fine
    # detail to recover -- enhancement either does nothing or manufactures
    # halos. What such a frame still has is large regions of distinct
    # brightness, so the drawing traces region *boundaries* rather than
    # texture. Tracing texture is what makes a blurry frame come out as
    # scattered dashes instead of strokes.
    'CV_EDGE_STYLE':          'overlay',
    'CV_SHARPIE_LEVELS':      4,     # luminance bands; more = concentric clutter
    'CV_SHARPIE_THICK':       3,     # stroke width in px
    'CV_SHARPIE_MIN_LEN':     140,   # discard contours shorter than this
    'CV_SHARPIE_SMOOTH':      13,    # median kernel; flattens speckle pre-banding
    'CV_SHARPIE_CLOSE':       7,     # morphological close/open, for unbroken strokes
    # Detection + Shakey HUD. Runs every Nth frame, never per frame: a
    # detector pass costs far more than the frame budget on this hardware.
    # The tracker carries boxes between passes and the HUD is drawn every
    # frame from tracker state, so labels persist smoothly instead of
    # strobing at the detection cadence.
    'CV_DETECT_ENABLED':      1,     # off by default
    'CV_DETECT_BACKEND':      'onnx',   # motion | onnx | null
    'CV_DETECT_INTERVAL':     8,     # run the detector every N frames
    'CV_DETECT_MODEL':        '/usr/share/hls-livecam-server/models/candidates/yolov8n.onnx',
    'CV_DETECT_CONF':         0.12,
    'CV_DETECT_MIN_AREA':     0.004, # motion backend: ignore blobs smaller than this
    'CV_HUD_ENABLED':         1,     # draw boxes/labels when detection is on

    'CV_SHARPIE_SCALE':       0.5,   # compute the drawing at this scale. Strokes are
                                      # thick by design, so the detail lost is detail
                                      # the drawing discards anyway -- and contour
                                      # finding plus morphology is superlinear in
                                      # pixel count, so this is the difference between
                                      # ~150ms and ~40ms.

    # Enhancement on moving subjects. The pipeline skips denoise and sharpen
    # wherever the motion mask is set, on the reasoning that motion blur is
    # an optical fact rather than missing detail. That holds for a fast pan;
    # it is wrong for a pet ambling across a soft, dirty lens, where the blur
    # is mostly focus and the moving subject is the whole point of watching.
    # Enabled by default: on this camera the subject matters more than the
    # theoretical risk of sharpening genuine motion blur.
    'CV_SHARPEN_MOTION':      1,     # sharpen moving regions too
    'CV_DENOISE_MOTION':      0,     # but do NOT temporally blend them (ghosting)

    # Lens-artifact subtraction. Water spots and dirt on the lens are static,
    # sharp and high-contrast while the scene behind them is soft and moving.
    # That asymmetry is what makes them separable: sample across frames that
    # contain motion, and anything high-frequency that never moves is on the
    # glass, not in the room.
    'CV_ARTIFACT_ENABLED':    0,     # off by default; opt in per node
    'CV_ARTIFACT_SAMPLES':    30,    # frames sampled to learn the mask
    'CV_ARTIFACT_MIN_MOTION': 0.4,   # mean flow required for a frame to count
    'CV_ARTIFACT_THRESHOLD':  18,    # high-pass response counted as an artifact
    'CV_ARTIFACT_DILATE':     2,     # grow the mask so stroke edges are covered
    'CV_ARTIFACT_MAX_AREA':   0.04,  # refuse a mask covering more than this
                                      # fraction of frame -- that is a wrong
                                      # answer, not a very dirty lens
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


def _tone_curve_lut(threshold, ceiling, gamma, shadow_lift=0.0, shadow_range=110):
    """Full tone curve: shadow lift at the dark end, highlight rolloff at the
    bright end, identity through the middle.

    The rolloff alone only ever helps a blown-out surface. A dark subject in
    the same frame -- black fur on white bedding -- is at the other end of the
    histogram and is crushed to a silhouette regardless of what the highlights
    do. The lift raises those values with a gamma curve, weighted so it fades
    out by `shadow_range` and leaves midtones alone.
    """
    lut = _highlight_rolloff_lut(threshold, ceiling, gamma).astype(np.float32)
    if shadow_lift > 0.0:
        x = np.arange(256, dtype=np.float32)
        # Gamma < 1 raises dark values; strength tapers to zero at shadow_range
        # so the curve stays continuous instead of stepping at the boundary.
        lifted = 255.0 * np.power(x / 255.0, 1.0 / (1.0 + shadow_lift))
        w = np.clip((shadow_range - x) / max(shadow_range, 1), 0.0, 1.0)
        lut = lut * (1.0 - w) + lifted * w
    return np.clip(lut, 0, 255).astype(np.uint8)


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
        self._last_luma = None  # denoised L channel, kept by _tone_correct
        self._frames = deque(maxlen=self.HISTORY)  # raw RGB frames, newest last
        self._gray_small = deque(maxlen=self.HISTORY)  # matching downscaled gray, for flow

        self._motion_threshold = _read_float(denv, 'CV_MOTION_THRESHOLD')
        self._unsharp_amount = _read_float(denv, 'CV_UNSHARP_AMOUNT')
        self._unsharp_sigma = _read_float(denv, 'CV_UNSHARP_SIGMA')

        clip = _read_float(denv, 'CV_CLAHE_CLIP')
        tiles = _read_int(denv, 'CV_CLAHE_TILES')
        self._clahe = cv2.createCLAHE(clipLimit=clip, tileGridSize=(tiles, tiles))

        self._edge_enabled = _read_int(denv, 'CV_EDGE_ENABLED') != 0
        self._edge_sharp_min = _read_float(denv, 'CV_EDGE_SHARPNESS_MIN')
        self._edge_sharp_off = _read_float(denv, 'CV_EDGE_SHARPNESS_OFF')
        self._edge_strength = _read_float(denv, 'CV_EDGE_STRENGTH')
        self._edge_sigma = _read_float(denv, 'CV_EDGE_SIGMA')
        self._edge_sigma_max = _read_float(denv, 'CV_EDGE_SIGMA_MAX')
        self._edge_percentile = _read_float(denv, 'CV_EDGE_PERCENTILE')
        self._edge_gamma = _read_float(denv, 'CV_EDGE_GAMMA')
        self._edge_style = str((denv or {}).get('CV_EDGE_STYLE',
                                                _DEFAULTS['CV_EDGE_STYLE'])).strip().lower()
        self._sharpie_levels = max(2, _read_int(denv, 'CV_SHARPIE_LEVELS'))
        self._sharpie_thick = max(1, _read_int(denv, 'CV_SHARPIE_THICK'))
        self._sharpie_min_len = _read_int(denv, 'CV_SHARPIE_MIN_LEN')
        self._sharpie_smooth = _read_int(denv, 'CV_SHARPIE_SMOOTH')
        self._sharpie_close = _read_int(denv, 'CV_SHARPIE_CLOSE')
        self._sharpie_scale = _read_float(denv, 'CV_SHARPIE_SCALE')

        self._detect_enabled = _read_int(denv, 'CV_DETECT_ENABLED') != 0
        self._detect_interval = max(1, _read_int(denv, 'CV_DETECT_INTERVAL'))
        self._hud_enabled = _read_int(denv, 'CV_HUD_ENABLED') != 0
        self._frame_n = 0
        self._detector = None
        self._tracker = None
        if self._detect_enabled and _cvd is not None:
            backend = str((denv or {}).get('CV_DETECT_BACKEND',
                                           _DEFAULTS['CV_DETECT_BACKEND']))
            try:
                _raw_model = (denv or {}).get('CV_DETECT_MODEL')
                _effective_model = str(
                    _raw_model or _DEFAULTS['CV_DETECT_MODEL']
                )

                print(
                    "SHAKEY DETECTOR CONFIG:"
                    f" backend={backend!r}"
                    f" raw_model={_raw_model!r}"
                    f" effective_model={_effective_model!r}"
                    f" exists={__import__('os').path.exists(_effective_model)!r}",
                    flush=True,
                )

                self._detector = _cvd.make_detector(
                    backend,
                    model_path=_effective_model,
                    conf=_read_float(denv, 'CV_DETECT_CONF'),
                    min_area_frac=_read_float(denv, 'CV_DETECT_MIN_AREA'))
                self._tracker = _cvd.Tracker()
            except Exception as exc:
                print(
                    f"SHAKEY DETECTOR INIT ERROR: "
                    f"{type(exc).__name__}: {exc}",
                    flush=True,
                )
                self._detector = None
        self._sharpen_motion = _read_int(denv, 'CV_SHARPEN_MOTION') != 0
        self._denoise_motion = _read_int(denv, 'CV_DENOISE_MOTION') != 0

        self._artifact_enabled = _read_int(denv, 'CV_ARTIFACT_ENABLED') != 0
        self._artifact_samples = _read_int(denv, 'CV_ARTIFACT_SAMPLES')
        self._artifact_min_motion = _read_float(denv, 'CV_ARTIFACT_MIN_MOTION')
        self._artifact_threshold = _read_int(denv, 'CV_ARTIFACT_THRESHOLD')
        self._artifact_dilate = _read_int(denv, 'CV_ARTIFACT_DILATE')
        self._artifact_max_area = _read_float(denv, 'CV_ARTIFACT_MAX_AREA')
        self._artifact_mask = None      # HxW uint8, 255 where lens is dirty
        self._artifact_samples_buf = []  # luminance frames collected while learning
        self._artifact_learning = self._artifact_enabled

        hi_threshold = _read_int(denv, 'CV_HIGHLIGHT_THRESHOLD')
        hi_ceiling = _read_int(denv, 'CV_HIGHLIGHT_CEILING')
        hi_gamma = _read_float(denv, 'CV_HIGHLIGHT_GAMMA')
        self._highlight_lut = _tone_curve_lut(
            hi_threshold, hi_ceiling, hi_gamma,
            _read_float(denv, 'CV_SHADOW_LIFT'),
            _read_int(denv, 'CV_SHADOW_RANGE'))

    def process(self, frame):
        t = {}
        t0 = time.perf_counter()

        # Sharpie discards the photo, so everything that exists to improve the
        # photo is wasted work: optical flow, the temporal blend and the
        # unsharp pass are ~70ms of the frame and none of it survives into a
        # line drawing. Tone still runs -- the drawing needs its equalised
        # luminance, and without equalisation the bands all land inside the
        # blown-out end of the histogram.
        if self._edge_enabled and self._edge_style == 'sharpie':
            toned = self._tone_correct(frame)
            t['tone'] = time.perf_counter() - t0
            t5 = time.perf_counter()
            out = self._draw_sharpie(toned)
            t['edges'] = time.perf_counter() - t5
            t6 = time.perf_counter()
            out, tracks = self._detect_and_hud(frame, out)
            t['detect'] = time.perf_counter() - t6
            t['total'] = time.perf_counter() - t0
            return out, {
                'motion': 0.0,
                'sharpness': 0.0,
                'tracks': len(tracks),
                'edge_weight': 1.0,
                'artifact_mask': self._artifact_mask is not None,
                'artifact_learning': False,
                'timings_ms': {k: round(v * 1000, 3) for k, v in t.items()},
                'history': len(self._frames),
            }

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

        # Sharpness is measured on the *denoised* luminance, not the raw
        # frame. Sensor noise is high-frequency, so a variance-of-Laplacian
        # taken before denoising reads grain as detail and a blurry, noisy
        # frame scores as sharp -- exactly backwards. Measuring after the
        # temporal blend removes that confound. (Tenengrad scores better in
        # comparative focus studies but is documented as having weak noise
        # immunity, which is the wrong trade for an old webcam.)
        # Sampling uses the denoised luminance the tone stage kept, and runs
        # only while learning -- once the mask exists this costs nothing.
        if self._artifact_learning:
            self._collect_artifact_sample(self._last_luma, motion_mean)
        if self._artifact_enabled and self._artifact_mask is not None:
            toned = self._subtract_artifacts(toned)
            sharpened = self._sharpen(toned, moving_mask)

        t5 = time.perf_counter()
        sharpness = self._measure_sharpness(self._last_luma)
        edged, edge_weight = self._draw_edges(sharpened, sharpness)
        t['edges'] = time.perf_counter() - t5

        t6 = time.perf_counter()
        edged, hud_tracks = self._detect_and_hud(frame, edged)
        t['detect'] = time.perf_counter() - t6

        self._frames.append(frame)
        self._gray_small.append(small_gray)

        # SHAKey HUD: render on the FINAL display frame, immediately before
        # returning it. This is deliberately after all visual processing so
        # the HUD cannot be consumed by the enhancement/edge pipeline.
        if self._hud_enabled:
            edged = _cvd.draw_hud(edged, hud_tracks)
            if self._frame_n % self._detect_interval == 0:
                print(
                    f"SHAKEY HUD RENDER: frame={self._frame_n} "
                    f"tracks={len(hud_tracks)} enabled=1",
                    flush=True,
                )

        t['total'] = time.perf_counter() - t0
        metadata = {
            'motion': motion_mean,
            'sharpness': round(sharpness, 2),
            'edge_weight': round(edge_weight, 3),
            'artifact_mask': self._artifact_mask is not None,
            'artifact_learning': self._artifact_learning,
            'tracks': len(hud_tracks),
            'timings_ms': {k: round(v * 1000, 3) for k, v in t.items()},
            'history': len(self._frames),
        }
        return edged, metadata

    def _measure_sharpness(self, luma):
        """Variance of the Laplacian on a quarter-scale luminance image.

        Takes the L channel the tone stage has already extracted, rather
        than converting and downscaling a colour frame again -- measured at
        10.2ms per frame when it did its own full-resolution colour resize,
        paid on *every* frame including sharp ones where no edges are drawn.
        Downscaling a single channel that already exists is a fraction of
        that.

        Quarter scale also suppresses, via INTER_AREA averaging, the sensor
        grain that would otherwise inflate the score. Absolute values are
        therefore lower than a full-resolution figure, which is why the
        thresholds are configuration rather than constants.
        """
        small = cv2.resize(luma, None, fx=_FLOW_SCALE, fy=_FLOW_SCALE,
                           interpolation=cv2.INTER_AREA)
        return float(cv2.Laplacian(small, cv2.CV_64F).var())

    def _edge_weight(self, sharpness):
        """0 when the image is sharp enough to leave alone, 1 when it is
        thoroughly soft, ramped between. A ramp rather than a hard switch:
        a binary cutoff makes edges snap in and out as the score dithers
        across the threshold, which is far more distracting than the edges
        themselves."""
        lo, hi = self._edge_sharp_min, self._edge_sharp_off
        if hi <= lo:
            return 1.0 if sharpness <= lo else 0.0
        if sharpness >= hi:
            return 0.0
        if sharpness <= lo:
            return 1.0
        return float((hi - sharpness) / (hi - lo))

    def _draw_edges(self, frame, sharpness):
        """XDoG-style line overlay, engaged only as the image goes soft.

        Difference-of-Gaussians rather than Canny: it yields tonal strokes
        that vary with edge strength instead of uniform binary hairlines,
        which is what reads as a drawing rather than a mask. Flow-based
        (coherent) line drawing is the better-looking method but costs
        several times more, and this has to run on the weakest node in the
        fleet.

        Stroke width scales with how soft the image is, per the brief: a
        heavily blurred frame has no fine structure left to trace, so a
        hairline would follow noise. Returns (frame, weight_applied).
        """
        if not self._edge_enabled:
            return frame, 0.0
        w = self._edge_weight(sharpness)
        if w <= 0.0:
            # Sharp feed: absent entirely, and costing nothing beyond the
            # metric itself. This is the tanzania case.
            return frame, 0.0

        if self._edge_style == 'sharpie':
            return self._draw_sharpie(frame), w

        sigma = min(self._edge_sigma * (1.0 + 2.0 * w), self._edge_sigma_max)
        gray = cv2.cvtColor(frame, cv2.COLOR_RGB2GRAY)
        g1 = cv2.GaussianBlur(gray, (0, 0), sigmaX=sigma)
        g2 = cv2.GaussianBlur(gray, (0, 0), sigmaX=sigma * 1.6)
        dog = cv2.subtract(g1, g2)

        # Normalise against a high percentile, not the maximum. On a noisy
        # frame the max is a single outlier pixel, so dividing by it scales
        # every genuine stroke down to nothing -- which is exactly why the
        # first version drew almost nothing on a soft, grainy feed. A
        # percentile puts real edges at full range and lets the outliers
        # clip, which is what they should do.
        ref = float(np.percentile(dog, self._edge_percentile))
        if ref <= 1e-6:
            return frame, 0.0
        strokes = cv2.convertScaleAbs(dog, alpha=255.0 / ref)

        # Gamma below 1 lifts mid-strength edges toward full ink instead of
        # leaving the drawing dominated by only the few strongest contours.
        if self._edge_gamma != 1.0:
            lut = np.clip(255.0 * (np.arange(256) / 255.0) ** self._edge_gamma,
                          0, 255).astype(np.uint8)
            strokes = cv2.LUT(strokes, lut)

        # Darken along strokes, scaled by engagement.
        amount = self._edge_strength * w
        ink = cv2.cvtColor(strokes, cv2.COLOR_GRAY2RGB)
        return cv2.addWeighted(frame, 1.0, ink, -amount, 0), w

    def _detect_and_hud(self, source_frame, canvas):
        """Run detector on cadence; always render SHAKey telemetry."""
        self._frame_n += 1
        tracks = []

        if self._detector is not None and self._tracker is not None:
            if self._frame_n % self._detect_interval == 0:
                try:
                    dets = self._detector.detect(source_frame)

                    for d in dets:
                        d.cls = _cvd.classify_coat(source_frame, d)

                    self._tracker.update(dets)

                    print(
                        "SHAKEY DETECT:"
                        f" frame={self._frame_n}"
                        f" detections={len(dets)}"
                        f" tracks={len(self._tracker.tracks)}"
                        f" detail={[f"{d.cls}:{d.confidence:.3f}" for d in dets]}",
                        flush=True,
                    )

                except Exception as exc:
                    print(
                        f"SHAKEY DETECT ERROR:"
                        f" frame={self._frame_n}"
                        f" {type(exc).__name__}: {exc}",
                        flush=True,
                    )

            tracks = [
                t for t in self._tracker.tracks.values()
                if t.state != 'departed'
            ]

        if self._hud_enabled:
            canvas = _cvd.draw_hud(canvas, tracks)

        return canvas, tracks

    def _draw_sharpie(self, frame):
        """Line drawing on white: quantise luminance, outline the bands.

        Reuses the luminance the tone stage already produced -- it has been
        denoised and CLAHE-equalised, which is exactly the input this wants,
        and re-deriving it would double the cost of the most expensive part.
        Equalisation matters: without it the bands all land inside the
        blown-out end of the histogram and the drawing loses the bright half
        of the scene.
        """
        h, w_px = frame.shape[:2]
        g = self._last_luma if self._last_luma is not None else \
            cv2.cvtColor(frame, cv2.COLOR_RGB2GRAY)
        sc = self._sharpie_scale
        if 0.1 < sc < 1.0:
            g = cv2.resize(g, None, fx=sc, fy=sc, interpolation=cv2.INTER_AREA)
        gh, gw = g.shape[:2]

        k = self._sharpie_smooth | 1
        g = cv2.medianBlur(g, k if k <= 9 else 9)

        step = max(1, 256 // self._sharpie_levels)
        ink = np.zeros((gh, gw), np.uint8)
        close_k = cv2.getStructuringElement(
            cv2.MORPH_ELLIPSE,
            (self._sharpie_close | 1, self._sharpie_close | 1))

        for lv in range(1, self._sharpie_levels):
            band = ((g >= lv * step).astype(np.uint8)) * 255
            # Close then open: joins a band broken by noise into one region,
            # then drops the specks that would otherwise each get outlined.
            band = cv2.morphologyEx(band, cv2.MORPH_CLOSE, close_k)
            band = cv2.morphologyEx(band, cv2.MORPH_OPEN, close_k)
            cont, _ = cv2.findContours(band, cv2.RETR_LIST, cv2.CHAIN_APPROX_NONE)
            for c in cont:
                if cv2.arcLength(c, False) >= self._sharpie_min_len:
                    cv2.drawContours(ink, [c], -1, 255, self._sharpie_thick)

        if ink.shape[:2] != (h, w_px):
            ink = cv2.resize(ink, (w_px, h), interpolation=cv2.INTER_NEAREST)

        canvas = np.full_like(frame, 255)
        canvas[ink > 0] = (25, 25, 25)
        return canvas

    # ── lens-artifact subtraction ───────────────────────────────────────
    def relearn_artifacts(self):
        """Discard the current mask and start sampling again."""
        self._artifact_mask = None
        self._artifact_samples_buf = []
        self._artifact_learning = self._artifact_enabled

    def _collect_artifact_sample(self, luma, motion_mean):
        """Sample luminance while the scene is moving.

        Motion is the requirement, not an optimisation: the method separates
        lens from scene by exploiting that the scene changes and the glass
        does not. Sampling a still scene would bake furniture into the mask.
        """
        if not self._artifact_learning or luma is None:
            return
        if motion_mean < self._artifact_min_motion:
            return
        self._artifact_samples_buf.append(luma.copy())
        if len(self._artifact_samples_buf) >= self._artifact_samples:
            self._build_artifact_mask()

    def _build_artifact_mask(self):
        """Median across samples, high-passed, thresholded.

        Median rather than mean: a mean is dragged by whatever passed through
        frame, while a median discards anything not present in most samples.
        What survives is what was there the whole time -- the glass.
        """
        self._artifact_learning = False
        stack = np.stack(self._artifact_samples_buf, axis=0)
        self._artifact_samples_buf = []

        median = np.median(stack, axis=0).astype(np.uint8)
        # High-pass: artifacts are small and sharp; the scene's persistent
        # structure is broad. Subtracting a blurred copy keeps only the fine
        # detail that survived the median.
        low = cv2.GaussianBlur(median, (0, 0), sigmaX=4.0)
        high = cv2.absdiff(median, low)
        _, mask = cv2.threshold(high, self._artifact_threshold, 255, cv2.THRESH_BINARY)

        if self._artifact_dilate > 0:
            k = cv2.getStructuringElement(
                cv2.MORPH_ELLIPSE,
                (2 * self._artifact_dilate + 1, 2 * self._artifact_dilate + 1))
            mask = cv2.dilate(mask, k)

        # A mask covering a large fraction of the frame means the separation
        # failed -- a static camera on a still scene, or a threshold far too
        # low. Repairing that much of every frame would do more damage than
        # the dirt. Refuse it and leave the feed alone.
        area = float((mask > 0).sum()) / mask.size
        if area > self._artifact_max_area:
            self._artifact_mask = None
            return
        self._artifact_mask = mask

    def _subtract_artifacts(self, frame):
        """Fill masked pixels from their surroundings.

        A blur-and-composite rather than cv2.inpaint: inpainting solves a PDE
        per region and is far too expensive per frame on the hardware this
        targets, while the mask is sparse specks whose neighbourhoods are
        genuinely representative. One blur and one masked copy.
        """
        if self._artifact_mask is None:
            return frame
        filled = cv2.GaussianBlur(frame, (0, 0), sigmaX=3.0)
        result = frame.copy()
        cv2.copyTo(filled, self._artifact_mask, result)
        return result

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

        if self._denoise_motion or moving_mask is None or not moving_mask.any():
            return avg

        # Still composite the crisp frame back over motion here: temporal
        # blending a moving subject ghosts it, which is a different and much
        # more visible failure than leaving it un-denoised.
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
        # Kept for the sharpness metric: this is the denoised luminance,
        # before CLAHE stretches local contrast. Measuring after CLAHE would
        # report the enhancement rather than the underlying focus, and the
        # decision to draw edges has to be made about the source image.
        self._last_luma = l
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

        # With CV_SHARPEN_MOTION the moving subject is sharpened like
        # everything else. That is the point on this camera: the cat is what
        # someone is watching, and excluding it left the subject the softest
        # thing in frame.
        if self._sharpen_motion or moving_mask is None or not moving_mask.any():
            return sharpened

        result = sharpened.copy()
        cv2.copyTo(frame, moving_mask, result)
        return result
