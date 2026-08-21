#!/usr/bin/env python3
"""
acceptance_crippled.py -- CV Mode Phase 4 §4, the milestone experiment.

Claim under test: a persistent scene layer recovers the room's spatial
structure from TEMPORAL evidence, and therefore does not depend on a
working classifier. If true, the same architecture runs on a good camera
and a dying one, differing only in observation fidelity.

Agreement between the persistence layer and YOLO would be weak evidence --
both would be reading the same frames, and a shared bias would look like
success. So the detector is deliberately CRIPPLED and the question becomes
whether the layer reconstructs the same structure without it.

  ARM A (reference):  YOLO at normal confidence -> where is the couch?
  ARM B (crippled):   no classifier anywhere in the path. Regions are
                      proposed from temporal stability and the boundaries
                      of a long-run median, then promoted by recurrence
                      alone (cv_occupancy + cv_persist).

Scoring is spatial: does ARM B independently produce a persistent static
region covering the area ARM A calls "couch"? IoU plus containment, since
a temporally-derived region need not share the detector's box conventions
-- a region that covers the couch and some adjacent stable floor is a
success for this claim, not a failure.
"""
import argparse, json, sys, time
import numpy as np, cv2

SHARE = '/home/ron/Projects/hls-livecam-server/pkg/usr/share/hls-livecam-server'
sys.path.insert(0, SHARE)
import cv_detect as cvd
import cv_occupancy as occ
import cv_persist as cvp

W, H = 1280, 720
MODEL = '/usr/share/hls-livecam-server/models/candidates/yolov8n.onnx'
REF_CLASSES = {56: 'chair', 57: 'couch', 59: 'bed', 60: 'dining table',
               58: 'potted plant', 72: 'refrigerator', 0: 'person'}


def load_frames(path, limit=None):
    f = W * H * 3
    d = open(path, 'rb').read()
    n = len(d) // f
    if limit:
        n = min(n, limit)
    return [np.frombuffer(d[i*f:(i+1)*f], np.uint8).reshape(H, W, 3) for i in range(n)]


def iou(a, b):
    ax2, ay2 = a[0]+a[2], a[1]+a[3]
    bx2, by2 = b[0]+b[2], b[1]+b[3]
    ix1, iy1 = max(a[0], b[0]), max(a[1], b[1])
    ix2, iy2 = min(ax2, bx2), min(ay2, by2)
    if ix2 <= ix1 or iy2 <= iy1:
        return 0.0, 0.0
    inter = (ix2-ix1)*(iy2-iy1)
    union = a[2]*a[3] + b[2]*b[3] - inter
    # containment = how much of the REFERENCE box arm B covered
    return inter/union if union > 0 else 0.0, inter/max(1, a[2]*a[3])


def _acuity_scale(img, divisor=360.0, lo=0.12, hi=1.0):
    g = cv2.cvtColor(img, cv2.COLOR_RGB2GRAY)
    g = cv2.resize(g, None, fx=0.5, fy=0.5, interpolation=cv2.INTER_AREA)
    return max(lo, min(hi, float(cv2.Laplacian(g, cv2.CV_64F).var()) / divisor))


def _dsus(img, s):
    if s >= 0.999:
        return img
    sm = cv2.resize(img, None, fx=s, fy=s, interpolation=cv2.INTER_AREA)
    return cv2.resize(sm, (img.shape[1], img.shape[0]), interpolation=cv2.INTER_LINEAR)


def arm_a(frames, conf=0.25, sample=30):
    """Reference structure with the detector fully enabled and given every
    advantage: the acuity-adapted input scale that ships in 5.8.1, run over
    the temporal median AND a sample of individual frames, keeping the best
    box per class. Making this arm as strong as possible is the point --
    a weak reference would flatter ARM B."""
    det = cvd.OnnxDetector(model_path=MODEL, conf=conf,
                           classes=REF_CLASSES).load()
    med = occ.temporal_median(frames)
    cands = [med] + [frames[i] for i in
                     np.linspace(0, len(frames) - 1, min(sample, len(frames))).astype(int)]
    out = {}
    for img in cands:
        for d in det.detect(_dsus(img, _acuity_scale(img))):
            if d.cls not in out or d.confidence > out[d.cls]['confidence']:
                out[d.cls] = {'box': list(d.box), 'confidence': round(d.confidence, 3)}
    return out, med


def arm_b(frames, floor=0.25, chunks=10):
    """Crippled: no classifier is constructed or called anywhere below.

    The frame sequence is split into CHUNKS and regions are derived
    INDEPENDENTLY within each chunk. Recurrence across chunks is then
    genuine temporal evidence -- a region that reappears chunk after chunk
    from separate observations earns persistence, while one that shows up
    once does not. (An earlier version computed regions once over all
    frames and replayed them, which promoted everything and proved
    nothing: every entity trivially had zero positional variance.)
    """
    n = len(frames)
    size = max(2, n // chunks)
    # Derived once from the whole sequence: sensor validity does not vary
    # chunk to chunk, and a longer baseline makes the estimate cleaner.
    valid = occ.validity_mask(frames)
    if valid is not None:
        print(f"   sensor validity: {(~valid).mean()*100:.1f}% of frame "
              f"masked as dead")
    store = cvp.EntityStore({
        'CV_PERSIST_PATH': '/tmp/_acceptance_entities.json',
        'CV_PERSIST_RECUR_COUNT': 3,
        'CV_PERSIST_PERSIST_COUNT': int(chunks * 0.6),
        'CV_PERSIST_PERSIST_AGE_S': 0.0,
        'CV_PERSIST_STATIC_SD': 40.0,
        'CV_PERSIST_CHECKPOINT_N': 100000,
    })
    t0 = time.time()
    per_chunk = []
    for ci in range(chunks):
        seg = frames[ci*size:(ci+1)*size]
        if len(seg) < 2:
            continue
        med_c = occ.temporal_median(seg)
        stats_c = occ.occupancy_stats(seg, valid=valid)
        regs = occ.candidate_regions(stats_c, med_c)
        per_chunk.append(len(regs))
        store.observe_many([
            cvp.observation_from_detection(
                cvp.Region(*r['box']), confidence=float(r['mean_stability']),
                floor=floor, klass=None, t=t0 + ci)
            for r in regs])

    med = occ.temporal_median(frames)
    stats = occ.occupancy_stats(frames, valid=valid)
    return store, per_chunk, stats, med


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--frames', required=True)
    ap.add_argument('--limit', type=int)
    ap.add_argument('--target', default='couch')
    ap.add_argument('--ref-conf', type=float, default=0.25)
    ap.add_argument('--json-out')
    a = ap.parse_args()

    frames = load_frames(a.frames, a.limit)
    print(f"frames: {len(frames)}")

    ref, med = arm_a(frames, conf=a.ref_conf)
    print("\nARM A -- detector enabled (reference structure):")
    for k, v in sorted(ref.items(), key=lambda kv: -kv[1]['confidence']):
        print(f"   {k:16s} conf={v['confidence']:.3f} box={v['box']}")
    if not ref:
        print("   (nothing detected -- cannot score this run)")

    store, per_chunk, stats, _ = arm_b(frames)
    static = store.static_regions()
    print(f"\nARM B -- classifier crippled; regions derived independently per chunk")
    print(f"   regions per chunk: {per_chunk}")
    print(f"   {len(store.entities)} entities, {len(static)} persistent+static:")
    for e in sorted(static, key=lambda e: -e.observation_count)[:10]:
        r = e.current_region()
        print(f"   {e.display_name:16s} obs={e.observation_count:3d} "
              f"state={e.state:10s} kind={e.kind:7s} "
              f"box=({r.x},{r.y},{r.w},{r.h}) sd={e.positional_sd:.1f}")

    print(f"\nSCORING vs ARM A '{a.target}':")
    result = {'target': a.target, 'matched': False}
    if a.target in ref:
        tb = ref[a.target]['box']
        best = (0.0, 0.0, None)
        for e in static:
            r = e.current_region()
            i, cont = iou(tb, (r.x, r.y, r.w, r.h))
            if i > best[0]:
                best = (i, cont, e)
        if best[2] is not None:
            print(f"   best ARM B region: {best[2].display_name} "
                  f"IoU={best[0]:.3f} containment_of_target={best[1]:.3f}")
            ok = best[0] >= 0.30 and best[1] >= 0.50
            result.update({'iou': round(best[0], 3),
                           'containment': round(best[1], 3),
                           'entity': best[2].id,
                           'matched': ok})
            # BOTH required. Containment alone is gameable by one huge
            # region -- an early version of the proposer emitted a single
            # whole-frame blob that "contained" every object perfectly
            # while having learned nothing. IoU is what forces the region
            # to actually be the object rather than merely cover it.
            print(f"   VERDICT: {'RECONSTRUCTED' if ok else 'NOT reconstructed'} "
                  f"(needs IoU >= 0.30 AND containment >= 0.50)")
        else:
            print("   ARM B produced no persistent static regions to compare")
    else:
        print(f"   ARM A did not detect '{a.target}' -- no reference to score against")

    if a.json_out:
        json.dump({'frames': len(frames), 'arm_a': ref,
                   'arm_b_regions_per_chunk': per_chunk,
                   'arm_b_static': len(static), 'result': result},
                  open(a.json_out, 'w'), indent=2)


if __name__ == '__main__':
    main()
