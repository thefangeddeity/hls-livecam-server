# RUN 2 (Linux) — pyfakewebcam profile

**Host:** tanzania (Arch, `~/Projects/hls-livecam-server`)
**Date:** 2026-08-13
**Brief:** `GDrive/Claude/Briefs/Linux/HLSLS/BRIEF - Linux run 2`
**Live system state:** unchanged — `feed-mode=show`, `bw-mode=false`, both
services active. **Nothing was installed or modified.** Read-only + userspace
benchmarks only.

---

## Bottom line

The brief's hypothesis is **half right, and the half that's wrong matters.**

- **Location: confirmed.** `pyfakewebcam` is the dominant per-frame cost.
- **Mechanism: wrong.** It is *not* `tobytes()` and it is *not* the `write()`
  syscall. Those together are **~1.8 ms**. It is the **RGB→YUV colour
  conversion**, at **~175–200 ms/frame**, against a 66.7 ms budget.
- **Therefore `cfakewebcam` as scoped would not fix this.** A `tobytes()` fix
  plus a C-extension `write()` targets ~1.8 ms of a ~200 ms problem — about 1%.
  It would be real work for no measurable result.

And the finding that came out of profiling, which nobody had measured:

> **The feed has never been running at 15 fps. It delivers ~4.8 unique frames
> per second, in every mode, and has since before any of this work started.**

The 15 fps in the container is the publisher duplicating each real frame ~3.2×.

---

## 1. Root cause: opencv is not installed

`pyfakewebcam.schedule_frame()` has two paths. With opencv it calls
`cv2.cvtColor(frame, cv2.COLOR_RGB2YUV)` — SIMD C. Without it, it falls back to
pure numpy:

```python
frame = np.concatenate((frame, self._ones), axis=2)   # (720,1280,4) uint8
frame = np.dot(frame, self._rgb2yuv.T)                # -> float64, 22 MB temp
self._yuv[:,:,:] = np.clip(frame, 0, 255)
```

**opencv is not installed on tanzania**, and the live service says so itself at
startup:

```
Aug 13 09:18:29 tanzania python3[316277]:
    Warning! opencv could not be imported; performace will be degraded!
```

`pacman -Q opencv python-opencv` → not found. `import cv2` → `ModuleNotFoundError`.

So the live service has been running the slow fallback the entire time.

---

## 2. The profile

CPU time (`time.process_time()`, not wall clock — the host is loaded and wall
clock was misleading), 1280×720, per `schedule_frame()` call:

| Stage | CPU ms | Note |
|---|---|---|
| `np.concatenate` → (720,1280,4) | 15.7 | 3.7 MB alloc |
| `np.dot(uint8, float64)` | **126.6** | produces a **22 MB float64** intermediate |
| `np.clip` + assign back to uint8 | 38.1 | |
| **— conversion subtotal —** | **~173–200** | **the entire problem** |
| YUYV pack (720-iteration Python loop) | 7.5 | |
| `buffer.tobytes()` | 0.3 | ← what `cfakewebcam` fixes |
| `os.write()` | ~1.5 | ← what `cfakewebcam` fixes (lower bound, /dev/null) |
| **TOTAL** | **~181–250** | vs a **66.7 ms** budget |

Run-to-run spread on the total (181/206/245/250 across runs) tracks host load;
the *shape* — conversion dwarfing everything else — is stable in every run.

---

## 3. Confirmed against the live stream, not just the bench

The bench predicted the writer loop should run at
`1000 / (work + 66.7 ms sleep)` ≈ **4–4.8 fps**, and that its thread should sit
near 70% of a core. `top -H` showed one thread at **69.5%**, matching.

To confirm independently, four consecutive live HLS segments were pulled and
decoded (60 frames, 4.00 s):

**First attempt was wrong and is worth recording.** Testing for *byte-identical*
consecutive frames found only 2 duplicates in 59 pairs — apparently 14.5 unique
fps, which would have contradicted everything above. That test was invalid:
duplicated frames are re-encoded by a lossy encoder, so they decode to
*near*-identical, not identical, pixels.

Measuring frame-to-frame mean absolute difference instead gives a clean bimodal
split:

| Cluster | n | mean diff | Interpretation |
|---|---|---|---|
| low | 40 | 0.147 | same frame, re-encode jitter |
| high | 19 | 0.779 | genuinely new camera frame |

The gap is wide — any threshold from 0.3 to 0.6 yields exactly 19 transitions.

```
19 new frames / 4.00 s  =  4.75 unique fps
60 container frames / 19 unique  =  each real frame repeated ~3.2x
```

**Measured 4.75 fps vs predicted 4–4.8 fps.** The mechanism is confirmed from
both ends.

---

## 4. Correction to run-1 addendum 5

Addendum 5 said the Blur fix's value was that render time now fits the frame
budget. That is still true, but the implication I left standing was wrong:
I framed it as Blur no longer "collapsing to ~5 fps", which implied other modes
were fine at 15. **They were not.** Every mode has been at ~4.8 fps.

What the Blur fix actually did, with the conversion cost included:

| | before run-1 | after run-1 |
|---|---|---|
| cloak frame cost | ~190 ms blur + ~200 ms conversion | ~21 ms blur + ~200 ms conversion |
| cloak rate | ~2.2 fps | ~4.4 fps |

So it roughly doubled the cloak rate and removed Blur as *a* bottleneck — real,
and worth having — but the conversion was and remains the binding constraint,
so no mode reached 15 fps. That's the part addendum 5 didn't say.

---

## 5. Candidate fixes, measured

A fused converter was written and benchmarked: it computes luma at full
resolution in **uint16** (77R+150G+29B ≤ 65280 fits), computes chroma **only at
the columns YUYV actually samples** (the current code computes U and V at full
resolution and then discards half in the packing step), and writes straight into
the output buffer from preallocated arrays, so a steady-state frame allocates
nothing. Conversion and packing become one pass.

Verified against the exact bytes the current implementation hands to
`os.write()`, across black, white, pure R/G/B, cyan, magenta, yellow, random
noise, and a gradient:

| Option | CPU ms | speedup | max err | `show` | `cloak` |
|---|---|---|---|---|---|
| current (float64 + loop pack) | ~206 | 1.0× | — | 4.9 fps | 4.4 fps |
| fused, int32 chroma | 62 | 3.3× | **1 LSB** | 15 fps | 12.0 fps |
| fused, int16 7-bit chroma | **39** | **5.3×** | 2 LSB | 15 fps | **15 fps** |
| install opencv (`cv2.cvtColor`) | not measurable here | — | — | — | — |

Both fused variants clear the 66.7 ms budget. Only the 7-bit variant leaves
enough headroom for **cloak** to also hold 15 fps once the vectorized Blur
renderer's ~21 ms is added on top.

The error is rounding only — integer fixed-point vs float64 — and lands in
chroma, which is 2:1 subsampled and then H.264-encoded at 1500 kbps. 1–2 LSB is
not visible. But it is a real difference and the PM should get to say so.

**Installing opencv is likely better than any of these** and is the smallest
change to reason about — `cv2.cvtColor` is SIMD C and would very likely beat
39 ms. It costs a new runtime dependency and needs root. It could not be
measured here because it isn't installed.

---

## 6. A bug I introduced and caught, worth recording

The first fused version put chroma accumulators in **int16** with a documented
range check. The check was wrong: `128 × 255 = 32640` fits, but the `+128`
rounding term makes it **32768** — one past int16's 32767. It wrapped to
−32768.

**Random-noise benchmarks passed it cleanly.** It only appeared on fully
saturated colours — pure red and pure blue produced `max|diff| = 255`, i.e. a
completely wrong chroma plane. It was caught by an explicit extremes stress
test, added specifically because a noise-only test proves very little about
saturation behaviour.

Anything derived from this work should keep that test. A camera pointed at
something strongly red is not an exotic input.

---

## 7. Also checked and ruled out

**CPU frequency scaling.** The governor is `powersave` and the CPU idles at
~2.0 GHz against a 4.2 GHz maximum, which looked like a possible confound for
every measurement here. It isn't: `scaling_max_freq` is the full 4.2 GHz,
`no_turbo=0`, `max_perf_pct=100`, temperatures are 27–29 °C, and spinning all
cores drives it to 3.2 GHz immediately. Not a factor, and not a finding.

---

## 8. What was NOT done

- **Nothing installed, nothing modified.** No root was needed and none was used.
- **`cfakewebcam` was not built.** The brief said to stop rather than build
  against a guess if the profile didn't support the hypothesis. The profile
  supports the *location* but contradicts the *mechanism*, so building the
  C-extension `write()` as scoped would deliver ~1% and is not the right fix.
  The correct target is the conversion.
- **No flag wiring**, since there is nothing installed to switch between.
- `pkg/` sync and any tag/push/AUR action remain gated on the PM's explicit
  go-ahead, unchanged and separate from this.

---

## 9. Recommendation

1. **Install opencv** and re-measure. Smallest change, most likely the fastest
   result, no custom numerical code to maintain. If it lands, everything below
   is unnecessary.
2. **If a new dependency isn't wanted**, take the fused converter. Prefer the
   **7-bit int16** variant — it is the only option that gets *cloak* to 15 fps
   as well as *show*, and 2 LSB of chroma rounding is not visible through
   subsampling plus lossy encode.
3. Either way, **re-measure the unique frame rate from the live stream** using
   the bimodal frame-difference method in §3 — not byte-equality, which gives a
   confidently wrong answer.
4. §2's countdown recommendation from run-1 (8–10 s) was measured while the feed
   was running at 4.8 fps. Transition timing is dominated by segment duration
   and live-edge lag, not frame rate, so it should still hold — but it is worth
   re-checking once the writer is actually delivering 15 fps.
