# HANDOFF — CV Mode packaging + AUR ship (v5.6.0 → v5.6.2)

**Date:** 2026-08-17
**From:** the desktop-chat session that shipped v5.6.0–v5.6.2 to AUR
**To:** the next Claude Tech Lead instance
**Read this in full before touching anything.**

---

## TL;DR

- GitHub is at `v5.6.2`. AUR is published at `5.6.2-1` (or will be shortly — propagation loop was handed off, not confirmed complete).
- **`yay -S hls-livecam-server` has NOT been run on tanzania.** Ron is hesitant to run it. Don't push him to — see "Why Ron is hesitant" below, it's a legitimate concern, not indecision.
- **Tina is out of the fleet for now.** Camera hardware can't support reliable detection (confidence down to 0.01 just to see cats, and even then scores are 0.02–0.07). Handle tina ad-hoc, not through packaging, until further notice.
- Detection + tracking + HUD are confirmed working on tanzania's live (hand-patched, pre-upgrade) system today — screenshot evidence below.

---

## Session started with zero conversational context

This instance had no prior-turn memory of this project. Documents pasted into the chat at session start (an "Addendum for instance 20B" AUR crisis doc, a "Handoff from 19 to 20" doc, v4.4.2-era source files) turned out to be **40+ releases stale** — real GitHub HEAD was already at v5.5.1→v5.6.0 territory with an active CV Mode effort. Don't trust pasted/uploaded handoff docs as current truth — verify against the live GitHub repo first (`git ls-remote --tags`, shallow clone, `git log`). This cost nothing and caught the staleness immediately; skipping it would have wasted the whole session chasing an AUR bug (a stale `sha256sum` mismatch from v4.4.2) that was ancient history.

---

## What shipped this session

### v5.6.0
Tagged existing HEAD (commit `50e2163`). Diffing `v5.5.1..v5.6.0` file trees caught that a new file, `cv_detect.py` (the detection/tracking module), had no install line in `PKGBUILD` — same failure class as a prior AUR incident (hash matches, install "succeeds," package is missing a file). Added the install line before publishing.

### v5.6.1
Two real fixes to `pkg/usr/share/hls-livecam-server/hls-livecam-setup-arch`:
1. **Model auto-download at install time.** `yolov8n.onnx` (12.85 MB) is hosted as a GitHub Release asset under tag `models-v1` (deliberately separate from source version tags, so it isn't re-uploaded on every bump). Setup now `wget`s + sha256-verifies it, mirroring the existing MediaMTX download pattern exactly. Asset hash: `b2bc52f40e8e1c532427d5bde3575a5d5b571b739fab2c6df443733ed1589cbd`.
2. **device.env write is no longer destructive.** It used to be an unconditional `cat > device.env <<EOF` — any re-run of setup silently wiped hand-tuned settings (e.g. `CV_DETECT_CONF`). Now it merges: setup's own managed keys (`VIDEO_DEVICE`, `V4L2_INPUT_FMT`, `VIDEO_SIZE`, `FRAMERATE`, `GOP_SECONDS`, `CV_DETECT_MODEL`) get rewritten fresh; everything else already in the file (custom `CV_*` tuning, etc.) is preserved verbatim under a `# --- preserved from a previous config ---` marker. Verified by dry-run against a mock device.env before handing to Ron (see note on process below).

Also investigated whether `broadcast-api`'s `CVProcessor()` call (missing `denv`, so it silently ignored `device.env` and used hardcoded defaults) needed fixing. **False alarm — already fixed as of v5.6.0.** `git show 50e2163:pkg/usr/local/bin/broadcast-api` already had `CVProcessor(denv)`; it was part of the original CV-mode commit batch. Ron's live hand-edit on tanzania matched what was already in git. Worth stating plainly: always verify with `git show <tag>:<path>` before assuming a gap — the correct instinct to double-check saved a wasted "hold the release" decision.

### v5.6.2
Ron modified `cv_detect.py` directly on tanzania. Two things happened in one commit, described as "just a HUD text tweak" but actually two distinct changes — worth reading the diff yourself before trusting a one-line commit message, same lesson as above:
1. HUD label text: `"DETECTING {n} ITEMS"` → `"DETECTING {n} TARGETS"`.
2. **`COCO_LABELS` dict rewritten.** Six previously-active classes (`bowl`, `banana`, `chair`, `couch`, `bed`, `vase`) were removed. A different set (`handbag`, `backpack`, `chair`, `couch`, `bed`, `scissors`) was added but left **commented out**, annotated "helps find lost items" — reads like scratch work for a future feature, not something ready to ship.
   - Confirmed via code read (`cid not in self.classes` filters at detection time, `cv_detect.py:279`) that removed classes are silently dropped before tracking/display — no crash, just gone.
   - Confirmed **intentional** — Ron: "My changes are truth." Do not relitigate.
   - Flagging for whoever uncomments the "lost items" block later: it currently has `24: 'handbag'` and `24: 'backpack'` on the same key (a dict can't hold both — only the second survives), and standard COCO id for `handbag` is `26`, not `24` (`backpack` at `24` is correct). Small thing, but it'll silently misbehave if enabled as-is.

---

## Current AUR package state

`PKGBUILD` / `.SRCINFO` in `~/Projects/aur-hls-livecam`:
- `pkgver=5.6.2`, `pkgrel=1`
- `sha256sums[0] = a447ef77cc9b5905faae67c5aa4dc96dde4dd9d32e362726c8e0990934745465`, `[1] = SKIP` (the `.install` file, unverified by design per project convention)
- Pushed to `ssh://aur.archlinux.org/hls-livecam-server.git` (commit `477e637`)

Propagation-check loop was handed to Ron; completion not confirmed in this session. If picking this up, check first:
```bash
curl -s 'https://aur.archlinux.org/rpc/v5/info/hls-livecam-server' | grep -o '"Version":"[^"]*"'
```

---

## Why Ron is hesitant to run `yay -S`

This is legitimate, not just nerves — treat it as an open risk, not something to talk him past.

**Never resolved this session: `hls-livecam-server.install` file content.** Asked for it four separate times across the session; never received it. It's not in the GitHub source repo (only in the AUR repo), and `aur.archlinux.org` isn't reachable from this instance's sandbox — could not fetch it independently. **This is the single highest-priority open item.** It determines whether `post_upgrade` auto-invokes `hls-livecam-setup`, which is the actual mechanism that would exercise (or not exercise) all the setup-script fixes shipped this session.

Separately: tanzania's live system right now is a **hand-patched hybrid**, not a clean install — the live `broadcast-api`, live `device.env` (with `CV_DETECT_CONF=0.35`), and the manually-placed model file were never deployed through the package manager to begin with. An actual `yay -S` reinstall against exactly this hybrid state has never been exercised end-to-end. The device.env merge fix and model-download logic are written and syntax-checked, but **not proven against tanzania's real current state.**

Recommendation: get the `.install` file before the next `yay -S` attempt, or if Ron wants to just try it, make sure a config backup is fresh first (`cp -r /etc/hls-livecam /etc/hls-livecam.bak-preinstall` at minimum). Don't push him to run it before he's ready — this is his call.

---

## Tina: excluded from the fleet

Two screenshots from Ron this session, both attached to this handoff for reference (not preserved in this file — ask Ron if they matter and aren't findable):

- **tanzania**, Aug 17 2026 20:20:43 — `HUMAN 0.71` correctly detected and boxed on a real live frame. CV Mode detection + tracking + HUD are demonstrably working end-to-end on tanzania's hardware today, just not yet via the packaged/AUR-installed path.
- **tina**, Aug 17 2026 20:21:49 — sharpie-mode render, two labels: `CAT 0.07` and `CAT 0.02`. Ron: had to drop `CV_DETECT_CONF` to `0.01` just to get any detections to register at all on tina's camera.

Decision this session: **tina is out of the packaged fleet for now.** Not a target for AUR/deb CV Mode rollout. Handle it ad-hoc/manually until the camera situation changes or the dual-model plan below lands. Don't build tina-specific packaging work without checking this is still the case.

---

## Parked / explicitly not done this session

- **Debian side.** Audited, not fixed: `pkg/DEBIAN/control` version is still stuck at `5.5.1` (same drift AUR had before this session). The Debian `hls-livecam-setup` script has the *identical* two gaps Arch's had before v5.6.1 (destructive device.env overwrite, zero CV_/model handling) — same fix pattern would apply, just not ported over. Explicit call from Ron: "Debian can wait."
- **Dual-model / "high CPU, low CPU" mode.** Planned to replace the current high/low-fps setup choice. Once built and tested, stronger nodes get a larger model from `models/candidates/`; weaker nodes (tina, maybe) keep `yolov8n`. This is also the trigger to reconsider tina's fleet status. `yolov4-tiny.cfg` sitting in `models/candidates/` on tanzania has no matching weights file and isn't referenced anywhere in code (`cv_detect.py` only loads ONNX) — confirmed dead/scratch, not packaged, skip it.
- **CHANGELOG.md** has no entries for v5.6.0/5.6.1/5.6.2. Worth catching up next natural pause.
- **Model license.** `yolov8n.onnx` is (presumably) a stock Ultralytics export — Ultralytics weights are typically AGPL-3.0-licensed. This repo is GPL-3.0-or-later. Compatibility was flagged early this session and never actually confirmed. Resolve before any wider/public distribution.
- **"Find lost items" detection labels** (`handbag`/`backpack`/`scissors` in `cv_detect.py`) — inert, commented out, not reviewed as a real feature. Has the duplicate-key/wrong-id bug noted above if someone enables it later.

---

## Process notes for whoever picks this up

- The AUR ship discipline (tag → wait → hash → patch with `assert`-checked Python heredocs → diff file trees between tags before touching `PKGBUILD`) caught a real bug again this session (the `cv_detect.py` install-line gap). Keep doing the diff-before-patch step every release — cheap, and it's now paid for itself at least twice in this project's history.
- `git status`/`git diff` before every commit, even ones described as trivial — the v5.6.2 "HUD text tweak" turned out to also silently drop six detection classes. Turned out to be intentional, but it needed surfacing, not assuming.
- This instance did some unprompted sandbox dry-run testing of a patch before handing it to Ron (copying the target file, running the patch, checking `bash -n` and a mock functional run). Ron's feedback: that burned tokens he didn't ask for. The instinct to verify before handing over risky multi-level-nested-heredoc patches was reasonable, but calibrate it down — a syntax check is enough for most things; save deeper verification for changes where the failure mode is expensive or hard to diagnose after the fact.
