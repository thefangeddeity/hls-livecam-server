# Handoff — CV work, picked up mid-flight

**To:** the CLI session that ran out of credit partway through run-10.
**From:** the desktop session, 2026-08-15.
**Everything below is committed and pushed.** Nothing is sitting in a working
tree this time.

---

## What you had in flight, and what happened to it

`cv_processor.py` had uncommitted changes: per-camera tuning via `device.env`
plus the highlight rolloff LUT. That was the whole diff, and it was good — it
is now committed as `ec24029`.

Verified before committing rather than assumed: the default LUT is
bit-identical to an identity ramp, so a node that has not added the new
`CV_*` keys behaves exactly as it did before. That claim is in the docstring
and it holds.

---

## What I added on top

**Adaptive edge mode** (brief §2) — `d8a7820`.

Two findings worth carrying, because they are not obvious:

1. **Measure sharpness after the temporal denoise, not before.** Sensor noise
   is high-frequency. A variance-of-Laplacian on the raw frame reads grain as
   detail, so a blurry *noisy* frame scores as sharp — exactly backwards for
   this decision. The metric now runs on the denoised luminance.

2. **Reuse the L channel `_tone_correct` already extracts.** My first version
   did its own full-resolution colour resize and cost **10.2 ms on every
   frame**, including sharp frames that draw nothing at all. Reusing the
   existing channel took it to 4.6 ms. The pipeline is already over budget;
   this stage must not add cost when it is doing nothing.

Rendering is XDoG (difference-of-Gaussians), not Canny — tonal strokes that
vary with edge strength read as a drawing, uniform hairlines read as a mask.
Flow-based coherent line drawing is better looking and several times more
expensive, so it is out on tina.

Engagement **ramps** between `CV_EDGE_SHARPNESS_MIN` and `..._OFF` rather than
switching. A hard cutoff makes edges snap in and out as the score dithers
across the threshold, which is more distracting than the edges.

Measured at 1280x720: sharp input (score 94.5) → weight 0.00, **edges absent**
— the tanzania case the brief explicitly requires. Soft (12.0) and mush (2.8)
→ fully engaged.

**Status label** (brief §4) — same commit. CV Mode had no qualifier of its
own, so the header showed only "feed hidden" from the dark flag, reading as
though CV hides the feed when it is the mode that processes it. Now
"Computer Vision enabled", in both the GUI and the curses camdash.

---

## The thing you should know before doing anything else

**CV Mode is already over budget on tanzania, which is the *strong* node.**

Measured with realistic frames (a static scene with a moving subject — random
noise is pathological for optical flow and overstates cost badly):

| stage | ms @ 1280x720 |
|---|---|
| downscale/gray | 7.7 |
| optical flow | 42.6 |
| denoise | 14.7 |
| tone (CLAHE + LUT) | 36.2 |
| sharpen | 17.9 |
| **total, median** | **121.7** |

Budget at 15fps is **66.7 ms**. That is **183% of budget on 8 cores.** tina has
4 cores and is the deployment target.

Resolution is the lever that actually works:

| processing resolution | median | verdict |
|---|---|---|
| 1280x720 | 121.7 ms | 183% of budget |
| 960x540 | 71.8 ms | 108% — still over |
| 640x360 | 29.6 ms | 44% — fits, with room |

Flow and tone together are ~65% of the cost. If someone wants full-resolution
output, the honest options are: run the *analysis* stages at reduced
resolution and upscale the masks (flow already does this at 0.25 — the tone
stage does not), drop the flow cadence below per-frame, or accept a lower
output resolution.

### tina's number, measured by the CLI session

I did not benchmark tina; the CLI session did, on real frames pulled from
tina's own live HLS output:

- **~180–190 ms/frame steady state against the 66.7 ms budget — ~2.8x over.**
- CPU is an Intel Core i3-2330M (2011, 4 threads, **no AVX2**).
- Dominated by `createCLAHE` (~75 ms) and Farneback flow (~56 ms).

Their conclusion, which the measurements support: no combination of
clipLimit / tileGridSize / unsharp / threshold values closes a 2.8x gap. It is
an architecture/CPU mismatch, not a tuning problem.

### PM decision: run tina as-is, pare back if stressful

Taken with the numbers in hand. What makes this safe rather than reckless —
verified by reading the code, not assumed:

`_cam_frame` is a **single slot**, not a queue. The drain thread overwrites it
with the newest frame; the writer takes whatever is currently there. Running
over budget therefore degrades to **lower fps with bounded latency and bounded
memory** — it cannot accumulate lag or grow a backlog. That is already the
"prefer a current frame over every historical frame" property the processor
brief asks for, and it holds today.

Expect roughly 5 fps on tina rather than 15. That is what "stressful" will
look like: a slow feed, not a growing delay.

### The interaction to watch on tina

Adaptive edge mode engages *precisely* on soft feeds — which is tina, whose
lens is genuinely dirty and hazy. So tanzania pays ~0 for the stage (edges
absent) while **tina will have it fully engaged, adding ~40 ms on top of an
already 2.8x-over baseline.**

The lever already exists and needs no code change: `CV_EDGE_ENABLED=0` in
tina's `/etc/hls-livecam/device.env` turns the stage off for that node alone.
That is the first thing to pare back if tina stutters — before touching CLAHE
or flow, because it is the only stage that can be removed without changing
what the other modes look like.

---

## Not started

- **§1 lens-artifact subtraction.** Untouched. The brief asks to *confirm the
  asymmetry first* — sample 30 frames from tina and check the spot positions
  are pixel-stable — before building the running-median mask. That check is
  the whole gate; do not skip it. tina is reachable over SSH (verified: 4
  cores, 3.3 GB, cv2 4.10.0).
- **§3 HUD.** Design only, explicitly not to be built. Nothing written.
- The 30-vs-15 fps installer question the brief raises. Given the numbers
  above, 30fps with CV is not viable at any resolution I measured — the
  budget halves to 33.3 ms.

---

## Standing rule that bit this project twice

Anything patched live gets mirrored into `pkg/` and pushed **in the same
run**. Run-1's GOP fix was lost exactly this way, and the run-6/7 work sat
unpushed until run-9 forced it. Both camdash copies
(`pkg/usr/local/bin/camdash` and `pkg/usr/share/hls-livecam-server/camdash`)
must stay byte-identical — I sync and `diff -q` them every time.
