# Working notes for Claude Code on this repo

Read by every Claude Code instance regardless of interface — CLI, Desktop, web.
These are the things that have actually cost time here, not general advice.

## Verify before you trust a document

Treat every brief, report, handoff and prior-session summary as **possibly
stale**. The product diverges from the written record between sessions.

This has bitten repeatedly:

- A handed-down account blamed the HLS layer. Capture was deadlocking at the
  *first* link — ffmpeg wedged in `avformat_open_input` with zero sockets.
- The combined A/V change orphaned the entire Python frame pipeline, so `Blur`
  and `Hide` were inert for an unknown period while still documented as working.
- The audio panel existed only in the generated `web/index.html`. The template
  it renders from never had it, so every setup run silently deleted the feature.

Each document was internally coherent and wrong. Read live state yourself —
running processes, actual endpoints, the file on disk, `git describe` — and say
plainly where it disagrees with the document. Verify with a measurement, not a
memory. **This applies to reports written by previous Claude sessions too.**

## Never patch `web/index.html`

It is *generated* from `web/index.template.html`. Patching the generated copy is
exactly how the audio panel was lost. Edit the template; keep every
`@PLACEHOLDER@` intact. Both `bin/livecam-setup` and
`scripts/build-macos-dmg.sh` substitute `@HLS_PORT@`, `@VERSION@` and
`@BUILD_LABEL@`, and the build fails if any survive.

## Git tags

Moving an annotated tag **requires** `-a` or `-F`:

```
git tag -f -a <tag> -F -   # correct
git tag -f <tag>           # WRONG — degrades to lightweight, destroys the message
```

Plain `git tag -f` silently replaces an annotated tag with a lightweight one and
permanently discards its message, tagger and date. It happened to `mac-v1.2.0`.
Check with `git cat-file -t <tag>`: `tag` = annotated, `commit` = lightweight.

Also: `git rev-parse <annotated-tag>` returns the **tag object** SHA, not the
commit. Use `<tag>^{}`. This is not a bug, but it makes a correctly-named DMG
look like it mismatches its tag.

## Don't infer state from liveness

A zombie is not a live process. `psutil.pid_exists`, `Process.is_running` and
`os.kill(pid, 0)` all report a defunct child as alive — the supervisor watched
dead components forever because of it. Reap children *and* check state.

Likewise, a capture deadlocked in `avformat_open_input` is alive, burning CPU,
and producing nothing. Report stages from something that *measures* them
(`/api/pipeline`), never from "is the process running".

If a stage cannot be measured, show it as unknown. A lamp that can never fail is
indistinguishable from a broken one — three of four sat permanently grey for
months and implied a dead pipeline on a working stack.

## macOS specifics that are load-bearing

- **TCC needs GUI ancestry.** A process started over SSH or by launchd gets no
  camera, and ffmpeg's avfoundation input *deadlocks* rather than erroring. Start
  via LaunchServices (`open` an `.app`). Do not add an AppleScript hop to get
  there — an `.app` launched by `open` is already in the GUI session, and the
  hop only adds an Automation prompt that blocks an unattended boot.
- **One capture session.** Two AVFoundation clients contending for the camera
  is what produced the deadlock. Capture is a single combined A/V ffmpeg.
- **Ad-hoc signing is permanent by decision.** No Developer ID. TCC grants are
  pinned to a cdhash, so they reset on every reinstall. Gatekeeper needs one
  right-click → Open.

## Build and release

The build refuses to package a tree with modified tracked files, untracked files
under `bin/ gui/ web/ vendor/`, or a HEAD on no remote branch — so commit and
push *before* building. `ALLOW_DIRTY_BUILD=1` overrides and stamps the label
`UNTRACKED`. Version comes from `git describe`; there is no version constant to
edit, deliberately.

## Operating preferences

- **Restarting services needs no permission.** Apply the change and restart in
  the same step, then verify with a measurement rather than reporting "done".
  This does not extend to destructive or outward-facing actions.
- **Cross-machine:** tanzania is the distribution hub and is always on. Shared
  material lives in `~/shared/hls-livecam-from-macos/` there, with a dated
  `START-HERE-*.md` in the home directory. Reports go to Google Drive under
  `Claude/Reports/<platform>/HLSLS/`, named `YYYY-MM-DD REPORT ...` so they sort
  chronologically.

## CV work

The Linux `CV Mode Phase 1..5` reports are **five phases of what did not work**.
Read them as ruled-out ground: detector thresholds cannot separate a cat from a
false human, scene registration is dead, and the regions renderer runs at
~2.5 fps. Their conclusion was that the illumination field beat the network.

On macOS, CV is additionally blocked — combined A/V removed the frame-processing
stage entirely, so there is nothing to put CV *into*. `CV` and `Foveal layer`
ship disabled with a SOON badge for that reason. `Foveal layer` reuses the Linux
name deliberately; match those semantics rather than inventing a second meaning.
