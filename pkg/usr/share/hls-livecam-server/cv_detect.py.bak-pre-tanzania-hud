#!/usr/bin/env python3
"""
cv_detect.py -- detection, tracking and Shakey-style labelling for CV Mode.

Three separable pieces, per the processing brief: a Detector interface with
no framework baked in, a Tracker that persists identity between detections,
and a renderer that draws the result over the line drawing.

The detector is deliberately pluggable and defaults to NullDetector, so the
whole path -- tracking, labelling, rendering -- runs and can be tested with
no model present at all. Dropping a model in later changes one config key,
not the pipeline.

Why detection does not run every frame: on the hardware this targets a
detector pass costs far more than the frame budget. The brief's own answer
is the right one -- run detection at a low cadence, let the tracker carry
positions between passes, and render every frame from tracker state. A
missed detection shows as a track that persists briefly, not a label that
strobes.

Coat pattern is not a detection class. No general model distinguishes a
tuxedo from a tabby -- that is a coat, not an object category. It is however
trivially separable by colour statistics on the crop the detector already
produced: a tuxedo is bimodal black-and-white with almost no saturation, a
tabby is mid-luminance and brown/orange. That runs in microseconds and needs
no training data.
"""

import time

import numpy as np
import cv2

# COCO indices that matter here. A general model gives these three; the
# labels are renamed to what the operator actually calls them.
COCO_LABELS = {
    0:  'human',
    15: 'cat',
    45: 'bowl',      # the cat plate
    46: 'bowl',
}

LABEL_ALIAS = {
    'bowl': 'cat plate',
    'motion': 'motion',
}


def make_detector(kind, **kw):
    """Factory, so the pipeline never imports a specific implementation."""
    kind = (kind or 'null').strip().lower()
    if kind == 'motion':
        return MotionDetector(**{k: v for k, v in kw.items()
                                 if k in ('scale', 'threshold', 'min_area_frac',
                                          'dilate', 'max_boxes')}).load()
    if kind == 'onnx':
        return OnnxDetector(**{k: v for k, v in kw.items()
                               if k in ('model_path', 'size', 'conf', 'nms')}).load()
    return NullDetector().load()


class Detection:
    """Normalised detection, independent of whatever produced it."""

    __slots__ = ('cls', 'confidence', 'x', 'y', 'w', 'h', 'timestamp')

    def __init__(self, cls, confidence, x, y, w, h, timestamp=None):
        self.cls = cls
        self.confidence = float(confidence)
        self.x, self.y, self.w, self.h = int(x), int(y), int(w), int(h)
        self.timestamp = timestamp if timestamp is not None else time.time()

    @property
    def box(self):
        return (self.x, self.y, self.w, self.h)

    @property
    def centre(self):
        return (self.x + self.w // 2, self.y + self.h // 2)


class Detector:
    """Interface. Implementations must not import each other's frameworks."""

    def load(self):
        return self

    def detect(self, frame):
        raise NotImplementedError

    def close(self):
        pass


class NullDetector(Detector):
    """No model, no detections. The default.

    Exists so the tracker, labeller and renderer are exercised and testable
    before any model is chosen -- and so a node with no model file degrades
    to a plain line drawing rather than failing.
    """

    def detect(self, frame):
        return []


class MotionDetector(Detector):
    """Detector with no model: motion regions become boxes labelled 'motion'.

    Useful beyond testing. It exercises the whole path -- cadence, tracking,
    labelling, rendering -- on real video with nothing installed, and on a
    node too slow to run a real detector it still produces a usable HUD that
    says where something is happening, just not what it is.

    Frame differencing on a downscaled luminance image rather than MOG2: a
    background model has to be learned and re-learned after every lighting
    change, while a fixed camera watching for "what moved since a moment
    ago" needs neither the memory nor the warm-up.
    """

    def __init__(self, scale=0.25, threshold=18, min_area_frac=0.004,
                 dilate=9, max_boxes=6):
        self.scale = float(scale)
        self.threshold = int(threshold)
        self.min_area_frac = float(min_area_frac)
        self.dilate = int(dilate)
        self.max_boxes = int(max_boxes)
        self._prev = None

    def detect(self, frame):
        h, w = frame.shape[:2]
        small = cv2.resize(frame, None, fx=self.scale, fy=self.scale,
                           interpolation=cv2.INTER_AREA)
        gray = cv2.cvtColor(small, cv2.COLOR_RGB2GRAY)
        gray = cv2.GaussianBlur(gray, (0, 0), sigmaX=1.5)

        if self._prev is None or self._prev.shape != gray.shape:
            self._prev = gray
            return []

        diff = cv2.absdiff(gray, self._prev)
        self._prev = gray
        _, mask = cv2.threshold(diff, self.threshold, 255, cv2.THRESH_BINARY)
        if self.dilate > 0:
            k = cv2.getStructuringElement(
                cv2.MORPH_ELLIPSE, (self.dilate | 1, self.dilate | 1))
            # Close first: a moving subject shows as scattered patches where
            # its texture happens to differ, and those belong to one object.
            mask = cv2.morphologyEx(mask, cv2.MORPH_CLOSE, k)
            mask = cv2.dilate(mask, k)

        cont, _ = cv2.findContours(mask, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
        frame_area = gray.shape[0] * gray.shape[1]
        found = []
        for c in cont:
            area = cv2.contourArea(c)
            if area < self.min_area_frac * frame_area:
                continue
            x, y, bw, bh = cv2.boundingRect(c)
            inv = 1.0 / self.scale
            # Confidence from relative size: nothing here knows what the
            # object is, so the only honest signal is how much moved.
            conf = min(0.99, 0.4 + (area / frame_area) * 3.0)
            found.append(Detection('motion', conf, x * inv, y * inv,
                                   bw * inv, bh * inv))
        found.sort(key=lambda d: d.w * d.h, reverse=True)
        return found[:self.max_boxes]


class OnnxDetector(Detector):
    """Single-stage ONNX detector via cv2.dnn.

    cv2.dnn rather than onnxruntime: OpenCV is already a hard dependency on
    every node, so this adds a file rather than a package. Both tanzania
    (cv2 5.0) and tina (cv2 4.10) can load ONNX through it.

    Expects a YOLO-family output: (1, 84, N) or (1, N, 84).
    """

    def __init__(self, model_path, size=320, conf=0.35, nms=0.45, classes=None):
        self.model_path = model_path
        self.size = int(size)
        self.conf = float(conf)
        self.nms = float(nms)
        self.classes = classes if classes is not None else COCO_LABELS
        self.net = None

    def load(self):
        self.net = cv2.dnn.readNetFromONNX(self.model_path)
        # Explicitly CPU: the deployment target has no usable GPU and an
        # accidental fallback attempt costs startup time for nothing.
        self.net.setPreferableBackend(cv2.dnn.DNN_BACKEND_OPENCV)
        self.net.setPreferableTarget(cv2.dnn.DNN_TARGET_CPU)
        return self

    def detect(self, frame):
        if self.net is None:
            return []
        h, w = frame.shape[:2]
        blob = cv2.dnn.blobFromImage(frame, 1 / 255.0, (self.size, self.size),
                                     swapRB=False, crop=False)
        self.net.setInput(blob)
        out = self.net.forward()

        out = np.squeeze(out)
        if out.ndim != 2:
            return []
        # Accept either orientation; YOLOv8 emits (84, N), older exports (N, 84).
        if out.shape[0] < out.shape[1]:
            out = out.T

        boxes, confs, ids = [], [], []
        sx, sy = w / self.size, h / self.size
        for row in out:
            scores = row[4:]
            cid = int(np.argmax(scores))
            score = float(scores[cid])
            if score < self.conf or cid not in self.classes:
                continue
            cx, cy, bw, bh = row[:4]
            boxes.append([int((cx - bw / 2) * sx), int((cy - bh / 2) * sy),
                          int(bw * sx), int(bh * sy)])
            confs.append(score)
            ids.append(cid)

        if not boxes:
            return []
        keep = cv2.dnn.NMSBoxes(boxes, confs, self.conf, self.nms)
        if len(keep) == 0:
            return []
        dets = []
        for i in np.array(keep).flatten():
            x, y, bw, bh = boxes[i]
            dets.append(Detection(self.classes[ids[i]], confs[i], x, y, bw, bh))
        return dets

    def close(self):
        self.net = None


def classify_coat(frame, det):
    """tuxedo vs tabby, from colour statistics on the detected crop.

    Not a model: no general detector has these as classes, because a coat
    pattern is not an object category. The separation is straightforward
    though -- a tuxedo is bimodal black and white with almost no saturation,
    a tabby sits at mid luminance with brown/orange hue. Returns the refined
    label, or the original if the crop is unusable or ambiguous.
    """
    if det.cls != 'cat':
        return det.cls
    x, y, w, h = det.box
    x, y = max(0, x), max(0, y)
    crop = frame[y:y + h, x:x + w]
    if crop.size == 0 or crop.shape[0] < 8 or crop.shape[1] < 8:
        return det.cls

    hsv = cv2.cvtColor(crop, cv2.COLOR_RGB2HSV)
    sat = float(hsv[..., 1].mean())
    val = hsv[..., 2]
    dark = float((val < 70).mean())
    light = float((val > 185).mean())

    # Bimodal and desaturated: black-and-white coat.
    if sat < 60 and dark > 0.25 and light > 0.12:
        return 'tuxedo cat'
    # Warm and mid-toned: tabby.
    hue = float(np.median(hsv[..., 0][val > 40])) if (val > 40).any() else 0.0
    if sat >= 60 and 5 <= hue <= 30:
        return 'tabby cat'
    return det.cls


class Track:
    __slots__ = ('track_id', 'cls', 'confidence', 'box',
                 'first_seen', 'last_seen', 'state', 'misses')

    def __init__(self, track_id, det):
        self.track_id = track_id
        self.cls = det.cls
        self.confidence = det.confidence
        self.box = det.box
        self.first_seen = det.timestamp
        self.last_seen = det.timestamp
        self.state = 'new'
        self.misses = 0

    @property
    def centre(self):
        x, y, w, h = self.box
        return (x + w // 2, y + h // 2)


class Tracker:
    """Nearest-centre association. Deliberately simple.

    A Kalman/IOU tracker would be more accurate under occlusion, but the
    detector here runs at a low cadence on a fixed camera watching one or
    two slow subjects -- association by proximity is sufficient and costs
    nothing. Replaceable behind the same interface if that changes.
    """

    def __init__(self, max_misses=5, max_distance=180):
        self.tracks = {}
        self._next_id = 1
        self.max_misses = int(max_misses)
        self.max_distance = int(max_distance)

    def update(self, detections):
        unmatched = dict(self.tracks)
        for det in detections:
            best, best_d = None, self.max_distance + 1
            cx, cy = det.centre
            for tid, tr in unmatched.items():
                if tr.cls != det.cls:
                    continue
                tx, ty = tr.centre
                d = ((cx - tx) ** 2 + (cy - ty) ** 2) ** 0.5
                if d < best_d:
                    best, best_d = tid, d
            if best is not None:
                tr = self.tracks[best]
                tr.box = det.box
                tr.confidence = det.confidence
                tr.last_seen = det.timestamp
                tr.state = 'present'
                tr.misses = 0
                unmatched.pop(best, None)
            else:
                tr = Track(self._next_id, det)
                self.tracks[self._next_id] = tr
                self._next_id += 1

        # Anything unmatched this pass is missing, then departed. Kept for a
        # few passes so a single failed detection does not blink a label out
        # and back with a new id.
        for tid, tr in list(unmatched.items()):
            tr.misses += 1
            tr.state = 'missing'
            if tr.misses > self.max_misses:
                tr.state = 'departed'
                self.tracks.pop(tid, None)
        return list(self.tracks.values())


def draw_hud(canvas, tracks, ink=(25, 25, 25)):
    """Shakey-style: wireframe box, corner ticks, label above.

    Drawn in the same ink as the line art so the overlay belongs to the
    drawing rather than sitting on top of it as a separate UI layer.
    """
    out = canvas
    for tr in tracks:
        if tr.state == 'departed':
            continue
        x, y, w, h = tr.box
        x, y = max(0, x), max(0, y)
        dashed = tr.state == 'missing'

        cv2.rectangle(out, (x, y), (x + w, y + h), ink, 1 if dashed else 2)
        # Corner ticks -- the Shakey/SRI look, and they keep the box legible
        # against a drawing made of similar-weight strokes.
        t = max(8, min(w, h) // 6)
        for (px, py, dx, dy) in ((x, y, 1, 1), (x + w, y, -1, 1),
                                 (x, y + h, 1, -1), (x + w, y + h, -1, -1)):
            cv2.line(out, (px, py), (px + dx * t, py), ink, 3)
            cv2.line(out, (px, py), (px, py + dy * t), ink, 3)

        label = LABEL_ALIAS.get(tr.cls, tr.cls)
        text = f"{label.upper()}  {tr.confidence*100:.0f}%"
        if dashed:
            text += "  ?"
        (tw, th), _ = cv2.getTextSize(text, cv2.FONT_HERSHEY_SIMPLEX, 0.5, 1)
        ly = max(th + 6, y - 6)
        cv2.rectangle(out, (x, ly - th - 5), (x + tw + 8, ly + 3), (255, 255, 255), -1)
        cv2.rectangle(out, (x, ly - th - 5), (x + tw + 8, ly + 3), ink, 1)
        cv2.putText(out, text, (x + 4, ly), cv2.FONT_HERSHEY_SIMPLEX, 0.5, ink, 1,
                    cv2.LINE_AA)
    return out
