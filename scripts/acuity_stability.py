#!/usr/bin/env python3
"""
acuity_stability.py -- paired acuity-scale test (CV Mode Phase 4 §2).

The Phase 3b result (cat 0.0008 -> 0.026 on tina) was a single-frame peak.
A peak is not a usable observation: a persistent estimator needs the same
physical object to produce a CONSISTENT response over time far more than
it needs one good reading. This measures repeatability, not maxima.

Per node: capture a real frame sequence, run several fixed scales plus the
adaptive setting over every frame, and report per-scale mean / stdev /
coefficient-of-variation / detection-rate, plus per-frame latency.

Reports raw detector output only -- no class claims below the semantic
floor (Phase 4 rule: objectness, never class, under CV_DETECT_CONF).
"""
import argparse, json, subprocess, sys, time
import numpy as np, cv2

sys.path.insert(0, '/home/ron/Projects/hls-livecam-server/pkg/usr/share/hls-livecam-server')
import cv_detect as cvd

W, H = 1280, 720
MODEL = '/usr/share/hls-livecam-server/models/candidates/yolov8n.onnx'


def api(host, ep, data=None):
    c = (f"curl -s -m 3 http://127.0.0.1:5000/api/{ep}" if data is None
         else f"curl -s -m 3 -X POST -d '{data}' http://127.0.0.1:5000/api/{ep}")
    cmd = ['ssh', host, c] if host != 'localhost' else ['bash', '-c', c]
    return subprocess.run(cmd, stdout=subprocess.PIPE, text=True).stdout.strip()


def capture(host, seconds, fps=1):
    """Continuous pull sampled at fps -- NOT repeated single-frame pulls,
    which re-fetch the same HLS segment and yield duplicate frames."""
    url = (f"http://{'127.0.0.1' if host=='localhost' else host}:8888"
           f"/cam/video1_stream.m3u8")
    prev = api(host, 'feed-mode') or 'cv'
    switched = False
    try:
        if prev != 'show':
            api(host, 'feed-mode', 'show')
            switched = True
            time.sleep(12)   # HLS segment window; short waits still yield CV output
        out = subprocess.run(
            ['ffmpeg', '-y', '-i', url, '-t', str(seconds), '-vf', f'fps={fps}',
             '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-s', f'{W}x{H}', 'pipe:1'],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL).stdout
    finally:
        if switched:
            api(host, 'feed-mode', prev)
    n = len(out) // (W * H * 3)
    frames = [np.frombuffer(out[i*W*H*3:(i+1)*W*H*3], np.uint8).reshape(H, W, 3)
              for i in range(n)]
    uniq = len({f.tobytes().__hash__() for f in frames})
    return frames, uniq


def scores(det, img):
    h, w = img.shape[:2]
    s = min(640/w, 640/h); nw, nh = int(w*s), int(h*s)
    c = np.zeros((640, 640, 3), np.uint8)
    c[(640-nh)//2:(640-nh)//2+nh, (640-nw)//2:(640-nw)//2+nw] = cv2.resize(img, (nw, nh))
    det.net.setInput(cv2.dnn.blobFromImage(
        c, 1/255.0, (640, 640), (0, 0, 0), swapRB=False, crop=False, ddepth=cv2.CV_32F))
    o = np.squeeze(det.net.forward())
    if o.shape[0] < o.shape[1]:
        o = o.T
    return o[:, 4:]


def dsus(img, s):
    if s >= 0.999:
        return img
    sm = cv2.resize(img, None, fx=s, fy=s, interpolation=cv2.INTER_AREA)
    return cv2.resize(sm, (img.shape[1], img.shape[0]), interpolation=cv2.INTER_LINEAR)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--host', required=True)
    ap.add_argument('--seconds', type=int, default=60)
    ap.add_argument('--classes', default='15,0',
                    help='COCO ids to track (default cat,person)')
    ap.add_argument('--divisor', type=float, default=360.0)
    ap.add_argument('--json-out')
    a = ap.parse_args()

    cls_ids = [int(x) for x in a.classes.split(',')]
    names = {0: 'person', 15: 'cat', 56: 'chair', 57: 'couch', 60: 'table'}

    print(f"capturing {a.seconds}s from {a.host} ...", file=sys.stderr)
    frames, uniq = capture(a.host, a.seconds)
    print(f"  {len(frames)} frames, {uniq} unique", file=sys.stderr)
    if uniq < len(frames) * 0.8:
        print("  WARNING: many duplicate frames -- stability numbers will be "
              "optimistic", file=sys.stderr)

    det = cvd.OnnxDetector(model_path=MODEL, conf=0.001).load()
    acu = [float(cv2.Laplacian(cv2.cvtColor(f, cv2.COLOR_RGB2GRAY), cv2.CV_64F).var())
           for f in frames]
    adaptive = max(0.12, min(1.0, float(np.median(acu)) / a.divisor))

    print(f"\nhost={a.host}  acuity: mean={np.mean(acu):.1f} sd={np.std(acu):.1f}"
          f"  -> adaptive scale {adaptive:.3f}")

    results = {}
    for label, s in [('1.00', 1.0), ('0.50', 0.5), ('0.35', 0.35),
                     ('0.25', 0.25), ('0.18', 0.18), ('0.12', 0.12),
                     (f'ADAPT({adaptive:.2f})', adaptive)]:
        per_frame = {c: [] for c in cls_ids}
        t0 = time.perf_counter()
        for f in frames:
            sc = scores(det, dsus(f, s))
            for c in cls_ids:
                per_frame[c].append(float(sc[:, c].max()))
        lat = (time.perf_counter() - t0) / max(1, len(frames)) * 1000
        results[label] = {'scale': s, 'latency_ms': round(lat, 1),
                          'classes': {}}
        row = [f"{label:>13}  {lat:6.1f}ms"]
        for c in cls_ids:
            v = np.array(per_frame[c])
            cv_ = float(v.std()/v.mean()) if v.mean() > 0 else float('nan')
            rate = float((v >= 0.01).mean())
            results[label]['classes'][names.get(c, c)] = {
                'mean': round(float(v.mean()), 4), 'sd': round(float(v.std()), 4),
                'cv': round(cv_, 3), 'max': round(float(v.max()), 4),
                'rate_over_0.01': round(rate, 3)}
            row.append(f"{names.get(c,c)}: mean={v.mean():.4f} sd={v.std():.4f} "
                       f"CoV={cv_:.2f} rate>0.01={rate:.2f}")
        print("  " + "  |  ".join(row))

    if a.json_out:
        json.dump({'host': a.host, 'frames': len(frames), 'unique': uniq,
                   'acuity_mean': float(np.mean(acu)),
                   'adaptive_scale': adaptive, 'results': results},
                  open(a.json_out, 'w'), indent=2)
        print(f"\nwrote {a.json_out}", file=sys.stderr)


if __name__ == '__main__':
    main()
