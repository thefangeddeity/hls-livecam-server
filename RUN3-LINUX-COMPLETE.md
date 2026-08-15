# RUN 3 (Linux) — opencv verify, re-measure, declare the dependency

**Host:** tanzania (Arch, `~/Projects/hls-livecam-server`)
**Date:** 2026-08-13
**Brief:** `GDrive/Claude/Briefs/Linux/HLSLS/BRIEF - Linux run 3`
**Live system state:** `feed-mode=show`, `bw-mode=false`, `broadcast-api` and
`mediamtx` both active. Left in `show` as required.

---

## Bottom line — the brief's premise does not hold yet

> "opencv has been installed on tanzania."

**The C++ library is installed. The Python binding is not.** `cv2` cannot be
imported, so `pyfakewebcam` is still taking the slow numpy fallback and
**nothing has actually changed.**

```
$ pacman -Q opencv python-opencv
opencv 5.0.0-9
error: package 'python-opencv' was not found

$ python3 -c "import cv2"
ModuleNotFoundError: No module named 'cv2'

$ python3 -c "import pyfakewebcam.pyfakewebcam as p; print(p.cv2_imported)"
Warning! opencv could not be imported; performace will be degraded!
False
```

The `opencv` package ships headers and shared libraries only —
`/usr/include/opencv5/...` — and nothing named `cv2` exists anywhere on disk.
The bindings are a separate package, `python-opencv 5.0.0-9`, present in `extra`
and not installed.

The service also has not been restarted since **09:18:27**, i.e. before any of
this, so its startup log still carries the same warning run-2 found.

**The one command that finishes this:**

```bash
sudo pacman -S python-opencv
```

That needs root. `pacman` is not in the NOPASSWD set (only `smartctl`,
`hls-livecam-dark`, and specific `systemctl` verbs are), so it is the one step
here I could not do. Everything else in the brief is done.

---

## 1. Re-measurement — confirms nothing improved

Measured with run-2 §3's bimodal frame-difference method (byte-equality gives a
confidently wrong answer). Four live HLS segments per mode, 60 frames, 4.00 s.

| Mode | unique fps | note |
|---|---|---|
| `show` | **5.50** | vs run-2's 4.75 — same ballpark, no improvement |
| `cloak` colour | **~6.0** | see threshold caveat below |
| `cloak` B&W | ~6 (0.25 at the wrong threshold) | see caveat |
| `hide` | 0.00 | **correct** — `hide` renders once and reuses `_hide_frame` |

`hide` reading 0.00 is not a fault. The writer caches a single frame on entry to
that mode and re-sends it, so zero unique frames is exactly right, and it is a
useful control: it confirms the measurement is reading real content change.

### A correction to my own method from run-2

Run-3's brief adopts run-2 §3's method as trustworthy. It is — **for natural
video only.** The fixed 0.45 threshold I used is tuned to camera content and
**undercounts block art**, which is heavily quantised into 80×22 cells and so
changes by much smaller amounts frame to frame. Sweeping the threshold on the
same cloak capture:

| threshold | transitions | implied fps |
|---|---|---|
| > 0.05 | 34 | 8.50 |
| > 0.10 | 24 | **6.00** |
| > 0.20 | 24 | **6.00** |
| > 0.45 | 4 | 1.00 |

The stable plateau across 0.10–0.20 is the real answer (~6 fps). At 0.45 the
same stream reports 1.0 fps, and my first pass reported cloak B&W as 0.25 fps —
which is wrong, and which I would have filed as a dramatic regression if I had
not swept it.

**The threshold must be chosen per content type**: ~0.45 for natural video,
~0.15 for block art. Anyone reusing this method needs to sweep and look for the
plateau, not inherit the constant.

---

## 2. Dependency declared — three files, all staged

This is the part the brief said must not be skipped, and it is complete.

### `aur-hls-livecam/PKGBUILD`

```diff
-depends=(... 'python-numpy' 'python-pyfakewebcam' ...)
+depends=(... 'python-numpy' 'python-opencv' 'python-pyfakewebcam' ...)
```

### `aur-hls-livecam/.SRCINFO`

```diff
 	depends = python-numpy
+	depends = python-opencv
 	depends = python-pyfakewebcam
```

Edited line-wise in Python, tab indentation preserved and asserted — not sed.

### `hls-livecam-server/pkg/DEBIAN/control`

```diff
-Depends: ..., python3-numpy, wget, ca-certificates
+Depends: ..., python3-numpy, python3-opencv, wget, ca-certificates
```

**`python3-opencv` verified to exist**, not assumed —
`packages.debian.org/bookworm/python3-opencv` returns HTTP 200 with the correct
package page. (The `sources.debian.org` API returns nothing for it because that
indexes *source* packages; this is a binary package built from the `opencv`
source.)

`pkgver`/`pkgrel` untouched, AUR untouched, nothing pushed or tagged.

### Nowhere else tracks dependencies

Checked and confirmed absent: no `requirements.txt`, no `setup.py`, no
`pyproject.toml`, no dependency list in `README.md` or `HANDOFF.md`, and
`hls-livecam-setup-arch` installs no packages (it relies on PKGBUILD `depends`).
The three files above are the complete set.

### Will it actually be pulled in on tina? — yes, and here is why that needed checking

The `.deb` **never declared `pyfakewebcam` at all.** It is pip-installed at
runtime instead:

```
pkg/DEBIAN/postinst:13:  python3 -m pip install pyfakewebcam --break-system-packages --quiet
```

So the Debian side was already getting its webcam library outside the package
manager. That matters here because **pip-installing `pyfakewebcam` does not pull
in opencv** — opencv is an optional import in that library, which is the whole
reason the silent fallback exists.

Adding `python3-opencv` to `Depends:` is the correct fix, and the ordering
works: apt resolves `Depends:` *before* running `postinst`, so `cv2` is
importable by the time `pyfakewebcam` is installed and first used. tina will get
it on the next `.deb` install.

Worth flagging separately: `pyfakewebcam` being pip-installed in `postinst`
rather than declared is a pre-existing packaging weakness, not something this
run introduced. It is out of scope here but should not stay that way forever.

---

## 3. Countdown re-check — still holds

Two trials, same harness and discriminator as run-1 §2:

| Direction | measured |
|---|---|
| cloak → show | 3.78 s |
| show → cloak | 4.15 s |

Both land inside run-1's measured 3.55–4.49 s range. As run-2 predicted,
transition timing is set by segment duration (1 s, unchanged) and
`liveSyncDurationCount` (3, unchanged), not by feed frame rate — so the
**8–10 s hold recommendation stands.** Verified rather than assumed, though the
prediction was right.

---

## 4. Fused converter and cfakewebcam — unchanged

Per the brief: the fused converter stays documented in run-2, not wired in.
`cfakewebcam` stays closed. Neither was touched. Once `python-opencv` is
installed and `cv2.cvtColor` is measured, the fused converter should only be
revisited if opencv comes back *worse* than its 39 ms benchmark, which is
unlikely.

---

## 5. What is left

1. `sudo pacman -S python-opencv` — the only blocked step.
2. Restart `broadcast-api` (`systemctl stop` + `start` are both NOPASSWD, so
   this one does not need a password) and confirm the startup warning is gone
   **and** that `pyfakewebcam.cv2_imported` is `True` — check the flag, not just
   the absence of the warning.
3. Re-run `~/Projects/hls-livecam-run1-staging/measure_fps.sh`, sweeping the
   threshold per §1 rather than trusting the 0.45 default for cloak modes.
   Expect ~15 fps in `show` and `cloak`; `hide` stays 0.00 by design.
4. `pkg/` sync and any tag/push/AUR action remain gated, unchanged.

---

## 6. What was wrong in this brief

- **§0's premise.** "opencv has been installed" is true only of the C++ library;
  the Python binding that actually matters is not installed, so §1 and §2 could
  not confirm a fast path that isn't live. Reported rather than worked around.
- **§2's method, as generalised.** Run-2's fixed threshold is not
  content-agnostic and undercounts block art by up to 6×. Corrected above.
