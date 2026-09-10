#!/usr/bin/env python3
"""
cv_scene_register.py -- one-shot CLI: build a scene model region dictionary
for one node from reference photos, and stage it for deploy.

CV Mode Phase 3. Mirrors hls-livecam-repair's shape: a manually-run tool,
not a runtime code path or an API endpoint. Not specific to any one node --
--host names the target, matching how this whole project already pulls
live frames from any reachable node over HTTP.

Two source modes:

  --source supplied <photo_dir>
      Reference photos were shot externally (a phone, typically) and
      handed off as files. Used for a node whose own camera isn't good
      enough to be its own reference source (tina: i3, older sensor).

  --source own-camera
      Capture a still directly from the target node's own camera, at its
      best available format/resolution, instead of requiring external
      photos. Only worth using if that ceiling is meaningfully better than
      a live compressed video frame -- check the node's actual capabilities
      (v4l2-ctl --list-formats-ext) before assuming this is "high-res" in
      the phone-photo sense. (Confirmed once already: tanzania's camera
      tops out at 1280x720, the SAME as its streaming resolution -- still
      a real improvement over a live frame -- no motion blur, no
      video-compression artifacts, well-exposed single capture -- but not
      high-resolution in absolute terms.)

Output: writes scene_model.json and the reference photo(s) to a local
staging directory. Deploying them to the target node's /var/lib/hls-livecam/
is a separate, explicit step (same file-copy-then-restart shape as every
other deploy this project does) -- this script does not touch the target
node's filesystem.
"""

import argparse
import json
import os
import subprocess
import sys
import time

import numpy as np
import cv2

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cv_scene


FRAME_W, FRAME_H = 1280, 720


def _api(host, endpoint, data=None):
    """Small helper for the target node's control plane, over SSH loopback
    (the API binds 127.0.0.1 on each node, not the LAN interface)."""
    if data is None:
        cmd = f"curl -s -m 3 http://127.0.0.1:5000/api/{endpoint}"
    else:
        cmd = f"curl -s -m 3 -X POST -d '{data}' http://127.0.0.1:5000/api/{endpoint}"
    out = subprocess.run(['ssh', host, cmd], stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True)
    return out.stdout.strip()


def _capture_live_frame(host):
    """One RAW frame from the target's live stream.

    CRITICAL: this must be a photographic frame, not CV Mode's processed
    output. tina runs CV_EDGE_STYLE=sharpie, which renders the feed as a
    topographic line drawing -- ORB matching a photograph against that
    finds almost nothing (measured: 6 matches, versus the hundreds a real
    photographic pair produces). The HLS stream carries whatever the
    current feed mode renders, so this temporarily switches the node to
    'show' (the unprocessed passthrough), captures, and restores the prior
    mode afterwards.

    Capturing from the camera device directly would be the obvious
    alternative, but broadcast-api's own reader holds it -- the device
    supports one reader, which is why this project has always pulled
    frames over HTTP instead.
    """
    prev_mode = _api(host, 'feed-mode') or 'cv'
    switched = False
    try:
        if prev_mode != 'show':
            _api(host, 'feed-mode', 'show')
            switched = True
            time.sleep(3.0)  # let the switch propagate through the HLS segment window

        cmd = [
            'ffmpeg', '-y', '-i', f'http://{host}:8888/cam/video1_stream.m3u8',
            '-frames:v', '1', '-f', 'rawvideo', '-pix_fmt', 'rgb24',
            '-s', f'{FRAME_W}x{FRAME_H}', 'pipe:1',
        ]
        proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        raw = proc.stdout
        expected = FRAME_W * FRAME_H * 3
        if len(raw) < expected:
            raise RuntimeError(
                f"expected {expected} bytes from {host}'s live stream, got {len(raw)} "
                "-- is the node reachable and broadcast-api running?")
        return np.frombuffer(raw[:expected], np.uint8).reshape(FRAME_H, FRAME_W, 3)
    finally:
        if switched:
            _api(host, 'feed-mode', prev_mode)


def _remote_device_env(host):
    """Read VIDEO_DEVICE off the target node via read-only SSH -- needed
    for own-camera capture to know which device to query/capture from."""
    out = subprocess.run(
        ['ssh', host, 'cat /etc/hls-livecam/device.env'],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if out.returncode != 0:
        raise RuntimeError(f"could not read device.env from {host}: {out.stderr.strip()}")
    env = {}
    for line in out.stdout.splitlines():
        line = line.strip()
        if line and not line.startswith('#') and '=' in line:
            k, v = line.split('=', 1)
            env[k] = v
    return env


def _best_still_format(host, device):
    """Query the target's actual camera capabilities and pick the largest
    resolution available -- do not assume it beats the streaming config."""
    out = subprocess.run(
        ['ssh', host, f'v4l2-ctl --device={device} --list-formats-ext'],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if out.returncode != 0:
        raise RuntimeError(f"could not query {device} on {host}: {out.stderr.strip()}")
    sizes = []
    for line in out.stdout.splitlines():
        line = line.strip()
        if line.startswith('Size: Discrete'):
            dims = line.split()[-1]
            w, h = dims.split('x')
            sizes.append((int(w), int(h)))
    if not sizes:
        raise RuntimeError(f"no discrete formats reported for {device} on {host}")
    best = max(sizes, key=lambda wh: wh[0] * wh[1])
    return best


def _capture_own_camera_still(host):
    """Capture directly from the target node's own camera at its best
    available resolution, over SSH (no sudo -- camera devices are normally
    group-readable/writable by the installing user's video group, per this
    project's existing install pattern)."""
    denv = _remote_device_env(host)
    device = denv.get('VIDEO_DEVICE')
    if not device:
        raise RuntimeError(f"no VIDEO_DEVICE in {host}'s device.env")
    w, h = _best_still_format(host, device)
    print(f"  {host}'s camera ({device}) best still format: {w}x{h}", file=sys.stderr)
    if (w, h) == (int(denv.get('VIDEO_SIZE', '0x0').split('x')[0] or 0),
                  int(denv.get('VIDEO_SIZE', '0x0').split('x')[1] or 0)):
        print(f"  NOTE: {w}x{h} is identical to {host}'s streaming resolution -- "
              "own-camera capture still helps (no motion blur, no video-"
              "compression artifacts, a well-exposed single frame), but this "
              "is not a high-resolution reference in the phone-photo sense.",
              file=sys.stderr)

    remote_tmp = '/tmp/cv_scene_register_still.jpg'
    cmd = (f"ffmpeg -y -f v4l2 -input_format mjpeg -video_size {w}x{h} "
           f"-i {device} -frames:v 1 {remote_tmp} 2>/dev/null && cat {remote_tmp} "
           f"&& rm -f {remote_tmp}")
    out = subprocess.run(['ssh', host, cmd], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if out.returncode != 0 or not out.stdout:
        raise RuntimeError(
            f"still capture failed on {host}: {out.stderr.decode(errors='replace')}")

    arr = np.frombuffer(out.stdout, np.uint8)
    frame = cv2.imdecode(arr, cv2.IMREAD_COLOR)
    if frame is None:
        raise RuntimeError(f"could not decode still captured from {host}")
    return cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)


def _load_photo_dir(photo_dir):
    photos = []
    for name in sorted(os.listdir(photo_dir)):
        if name.lower().endswith(('.jpg', '.jpeg', '.png')):
            path = os.path.join(photo_dir, name)
            bgr = cv2.imread(path)
            if bgr is None:
                print(f"  WARNING: could not read {path}, skipping", file=sys.stderr)
                continue
            photos.append((name, cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)))
    if not photos:
        raise RuntimeError(f"no readable .jpg/.jpeg/.png files in {photo_dir}")
    return photos


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--host', required=True, help='target node (e.g. tina, tanzania)')
    ap.add_argument('--source', required=True, choices=('supplied', 'own-camera'))
    ap.add_argument('photo_dir', nargs='?',
                    help='directory of reference photos (required for --source supplied)')
    ap.add_argument('--out', default='.',
                    help='local staging directory for scene_model.json + reference photo(s)')
    ap.add_argument('--model', default='/usr/share/hls-livecam-server/models/candidates/yolov8n.onnx',
                    help='detector model path (read locally, must exist on this machine)')
    args = ap.parse_args()

    if args.source == 'supplied' and not args.photo_dir:
        ap.error('--source supplied requires photo_dir')

    print(f"== Registering scene model for {args.host} (source={args.source}) ==")

    if args.source == 'supplied':
        photos = _load_photo_dir(args.photo_dir)
    else:
        frame = _capture_own_camera_still(args.host)
        photos = [(f'{args.host}_own_camera.jpg', frame)]

    primary_name, primary_frame = photos[0]
    print(f"  primary reference: {primary_name} ({primary_frame.shape[1]}x{primary_frame.shape[0]})")

    print(f"  capturing a live frame from {args.host} to register against...")
    live_frame = _capture_live_frame(args.host)

    primary_gray = cv2.cvtColor(primary_frame, cv2.COLOR_RGB2GRAY)
    live_gray = cv2.cvtColor(live_frame, cv2.COLOR_RGB2GRAY)
    H, inlier_ratio, n_matches = cv_scene.compute_homography(primary_gray, live_gray)
    if H is None:
        print(f"  FAILED: no homography found ({n_matches} matches). "
              "Reference photo may not overlap the camera's field of view enough.",
              file=sys.stderr)
        sys.exit(1)
    print(f"  homography: {n_matches} matches, inlier_ratio={inlier_ratio:.3f}")

    print(f"  detecting static objects on {primary_name} at full resolution...")
    static_objects = cv_scene.detect_static_objects(primary_frame, args.model)
    for obj in static_objects:
        print(f"    {obj['cls']}: confidence={obj['confidence']:.3f} box={obj['box']}")
    if not static_objects:
        print("    (none found)")

    model = cv_scene.build_region_dict(static_objects, H, source=args.source)
    model['host'] = args.host
    model['inlier_ratio'] = round(inlier_ratio, 3)
    model['n_matches'] = n_matches
    model['reference_resolution'] = [primary_frame.shape[1], primary_frame.shape[0]]
    model['homography'] = H.tolist()

    if len(photos) > 1:
        print(f"  {len(photos) - 1} additional reference photo(s) supplied -- "
              "checking multi-view baseline usability (scope only, not building depth)...")
        _, secondary_frame = photos[1]
        secondary_gray = cv2.cvtColor(secondary_frame, cv2.COLOR_RGB2GRAY)
        baseline = cv_scene.estimate_baseline(primary_gray, secondary_gray)
        model['multiview_baseline_check'] = baseline
        print(f"    {baseline['reason']} "
              f"(n_matches={baseline['n_matches']}, "
              f"inlier_ratio={baseline['inlier_ratio']}, "
              f"mean_reproj_error_px={baseline['mean_reproj_error_px']})")

    os.makedirs(args.out, exist_ok=True)
    model_path = os.path.join(args.out, 'scene_model.json')
    ref_path = os.path.join(args.out, 'scene_reference.jpg')
    with open(model_path, 'w') as f:
        json.dump(model, f, indent=2)
    cv2.imwrite(ref_path, cv2.cvtColor(primary_frame, cv2.COLOR_RGB2BGR))

    print(f"\n  wrote {model_path}")
    print(f"  wrote {ref_path}")
    print(f"\n  Not deployed yet -- copy both files to {args.host}:/var/lib/hls-livecam/ "
          "and restart broadcast-api there to activate (CV_SCENE_ENABLED must also be set).")


if __name__ == '__main__':
    main()
