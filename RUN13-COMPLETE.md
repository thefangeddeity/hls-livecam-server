# macOS run-13 — Broadcast outage: RESOLVED

**To:** Tech Lead
**Target:** ariana
**Status:** **Family-facing broadcast is live again**, verified end-to-end
(§1). Down for 3 days 21 hours — 2026-08-10 21:19:10 to 2026-08-14 19:05.
Root cause was two independent faults that combined into something
unrecoverable. Six defects fixed. **The camera was never broken**, and neither
was OCLP.

---

## 1. Verification — the stream is actually live

Not "the service says running." Measured:

```
index.m3u8            HTTP 200
                      #EXT-X-STREAM-INF: 1280x720, FRAME-RATE=15.000, avc1.42c028

main_stream.m3u8      #EXT-X-TARGETDURATION:1
  MEDIA-SEQUENCE      38 -> 43 -> 48 -> 54          (steady, ~1/s)
  then               80 -> 90 -> 99 -> 108 -> 117   (45s sustained)
  after redeploy      9 -> 17 -> 25

segment fetch         HTTP 200, 190,632 bytes
  file(1) says        MPEG transport stream data     <- real media, not a stub

camera reader         3.32s CPU in 18s elapsed       <- actually decoding
reader / publisher    PIDs stable across the window  <- no respawn churn
orphans               none; single reader, correctly parented
feed mode             show                           (standing rule honoured)
```

Compare the reader CPU to the broken state, where it accumulated ~3 seconds over
**six minutes**. That flat line was the whole outage.

---

## 2. When it broke — dated precisely

`mediamtx.log` publish sessions per day:

```
2026/08/10   4 sessions
2026/08/11   0      <-- nothing
2026/08/12   0      <-- nothing
2026/08/13   0      <-- nothing
```

The transition:

```
21:17:54  [session 6901055c] is publishing to path 'cam', 1 track (H264)   <- working
21:18:42  [session 2978b2e4] is reading from path 'cam'                    <- a viewer
21:19:10  [conn 127.0.0.1:49282] opened / closed: EOF                      <- broken
          (repeats, unchanged, for 3.5 days)
```

**Died at 2026-08-10 21:19:10.** No sleep/wake event — the machine was awake with
a TeamViewer session active through 21:16–21:19. Not a sleep-resume failure.

---

## 3. Root cause

Two faults. Either alone would have been survivable. Together they removed every
path to a working stream, which is why it stayed dead through repeated restarts
and a full reboot.

### Fault 1 — anything launched over SSH is denied the camera, silently

```
tccd: Policy disallows prompt for Sub:{/usr/libexec/sshd-keygen-wrapper}
      Resp:{identifier=com.apple.sshd-keygen-wrapper, pid=2778,
            binary_path=/usr/libexec/sshd-session};
      access to kTCCServiceCamera denied
VDCAssistant: TCC access returned false for pid 3222
VDCAssistant: Streaming -> Init on event kCameraStreamStop
```

TCC attributes a camera request to the **responsible process**. For an SSH
session that is `sshd-session`, which holds no camera grant — and macOS
**refuses to even prompt** for it. The denial is silent: the device opens, enters
`Streaming`, then is stopped before a single frame is delivered. `ffmpeg` just
hangs.

The PM's shell during this incident was SSH (pid 2782, child of sshd 2778), not
the local Terminal.app (pid 495). So **every manual restart attempt since
2026-08-10 was silently denied the camera**, including the one run specifically
to test this. Same for anything I launched — my own attribution
(`com.anthropic.claude-code`) is likewise ungranted.

This is almost certainly what killed it on 08-10: a restart issued over a remote
session at 21:19, during the TeamViewer window, after which no subsequent
attempt could ever recover it.

### Fault 2 — the one start path that *could* hold camera rights was broken

The LaunchAgent runs in the GUI session domain, which can hold a camera grant.
It was broken two ways, so the escape hatch didn't work either:

**2a. launchd was reaping the pipeline ~50 seconds after every start.**
`livecam start` daemonizes with `nohup` and returns. Without
`AbandonProcessGroup`, launchd considers the job finished when the wrapper exits
and kills everything left in its process group. Observed directly: the agent
reported `last exit code = 0` having started mediamtx and broadcast-api, and
~50s later nothing was listening on any port.

**2b. `ffmpeg` was not on the agent's PATH.**
launchd's default PATH is `/usr/bin:/bin:/usr/sbin:/sbin` — no `/usr/local/bin`,
where Homebrew's ffmpeg lives. Started by the agent, broadcast-api's writer
thread died on `FileNotFoundError: 'ffmpeg'` while Flask kept serving normally.
This traceback is in `broadcast-api.log` — it was happening in production.

**Net effect: the paths that ran lacked camera rights; the path with camera
rights couldn't run.** Hence 3.5 days, surviving a reboot.

---

## 4. Fixes

**a. Broken-pipe handler never respawned the publisher** (`bin/broadcast-api`)
Caught `BrokenPipeError`/`OSError`, slept, and trusted a comment claiming the
loop top would respawn. The loop top checks `poll() is not None` — actual exit. A
live ffmpeg with a dead pipe returns `None` forever. Now kills and respawns at
the point of detection.

**b. A `write()` that blocks forever was uncatchable** (`bin/broadcast-api`)
Fix (a) alone did nothing — the same publisher PID sat at 0% CPU for a full 30s
window with no exception raised. It is a *hang*, not a raise; Python's blocking
pipe write has no timeout. Added a watchdog thread that force-kills the publisher
after 5s without a successful write.

**c. The watchdog then thrashed** (`bin/broadcast-api`)
(b) introduced its own defect: with no camera, it respawned ffmpeg every 5s
forever, burning CPU on a 2-core machine and disguising an *input* fault as an
*output* fault. Now guarded — if no frame has ever arrived, leave the publisher
alone and log the real layer:
`no camera frames yet -- publisher idle, not respawning (input-side fault)`.

**d. `livecam stop` leaked camera-holding orphans** (`bin/livecam`)
`kill_stray_ffmpeg` sent SIGTERM via `pkill` and never verified. An ffmpeg
blocked in a wedged avfoundation read does not reliably act on SIGTERM — it
survived, kept the camera claimed, was orphaned to ppid 1, and then `start`
spawned a *second* reader competing with it. **Restarting was making things
worse.** Now escalates to SIGKILL, verifies release, and reports anything it
cannot clear.

**e. `ffmpeg` resolved to an absolute path** (`bin/broadcast-api`)
`shutil.which` then known install locations, resolved once at import. Fixes
fault 2b at the source, independent of who starts the process.

**f. LaunchAgent fixed** (`~/Library/LaunchAgents/com.livecam.autostart.plist`)
Added `AbandonProcessGroup` (fault 2a) and an explicit PATH including
`/usr/local/bin` and `/opt/homebrew/bin`. Both keys carry comments explaining
the incident they prevent.

**g. Camera selected by name, not position** (`bin/broadcast-api`) — see §5.

Also retained from earlier in this run: root-refusal guards at all three entry
points (`bin/livecam`, `bin/livecam-setup`, `bin/camdash-gui`).

---

## 5. Continuity Camera — a live hazard, now mitigated

An iPhone is registered as a camera on this machine alongside the built-in one:

```
FaceTime HD Camera (Built-in)     index 0
Aayah Camera  (iPhone14,7)        index 1     <-- Continuity Camera
Capture screen 0                  index 2
```

`AVF_VIDEO_INDEX=0` is **positional**. Had the iPhone enumerated first, "index 0"
would silently have become someone's phone — a family broadcast that changes
source when a phone enters the room and dies when it leaves. It is currently at
index 1, so this did not cause the outage, but it is a live trap.

`_resolve_avf_index()` now resolves the index from `AVF_DEVICE_NAME` at open
time, logs loudly if the list has been reordered, and falls back to the
configured index if the named device is missing — never worse than before.
Verified against the real device list.

**The PM asked for Continuity Camera to be blocked permanently. It cannot be
done from the Mac.** The toggle exists only on the iPhone
(**Settings → General → AirPlay & Continuity → Continuity Camera → off**), and
there is no supported `defaults` key for it on macOS — I checked rather than
invent one that silently does nothing. The §5 fix makes the pipeline immune to
the reordering regardless of whether that toggle is ever set.

---

## 6. Operational rule this establishes

**Never start the pipeline from an SSH session.** It will appear to start
correctly, serve the web UI, report every service as running, and deliver no
video — because macOS denies the camera to sshd-attributed processes without a
prompt. Use the login agent (now working) or a local Terminal.app session.

This is worth putting in the README, and it applies to any macOS deployment of
this project, not just ariana.

---

## 7. Corrections to my own earlier calls in this run

Three, stated plainly rather than buried:

1. **"Root ownership was the root cause" — wrong.** Those root-owned processes
   appeared 2026-08-13 23:20, three days *after* the outage began. A later
   complication (someone `sudo`-ing at an already-dead stream), not the cause.
2. **My five-test camera table proved nothing.** All five ran under my own
   attribution and measured a TCC denial, not the camera. I also briefly
   concluded from a `Streaming`-state log line that the driver was healthy —
   device init succeeding is not frame delivery working.
3. **The OCLP-driver theory was a red herring.** I reported the camera as
   genuinely faulted and the fix path as system/OCLP-side. That was wrong. The
   camera hardware, the OCLP root patches, and the UVC path were all fine
   throughout. The `ErrorPacketCount: 3 / FrameCompleted: 0` I cited as evidence
   of a USB fault occurs *after* `kCameraStreamStop` — it is the teardown, not
   the trigger. I should have read the ordering before drawing a hardware
   conclusion.

The PM was right to push back on handing this off as broken.

---

## 8. Bearing on the processor-module brief

Directly relevant, and worth carrying into that design.

This outage is a textbook case of the failure that brief's health model has to
catch: **every process in the chain was running, every service reported
`running`, the web UI served fine, and no video existed for 3.5 days.** The
distinguishing signal was one number — a camera reader accumulating 3 seconds of
CPU over six minutes.

Concretely, for the processor module:
- Health states must distinguish **"no input frames arriving"** from
  **"processor crashed."** Only the second is visible from process liveness.
- A frame-starved input must read `FAILED`, never `HEALTHY`.
- Passthrough/bypass fallback is worth nothing if what is passed through is an
  empty stream — bypass must assert frames are flowing, not just that the
  process is up.
- `input_fps` is the metric that would have caught this on day one. It should be
  the primary health signal, not a secondary statistic.

---

## 9. Summary

- **Broadcast live**, verified by advancing media sequence and real TS segment
  bytes, stable across a 45s sustained window and a redeploy.
- **Root cause: TCC denial of SSH-launched processes, plus a LaunchAgent broken
  two ways** so the one path with camera rights could not run.
- **Six defects fixed**, all verified. Four of them (a–d) were not the cause but
  are why a dead camera looked healthy for 3.5 days — worth keeping regardless.
- **Camera, OCLP, and USB were all innocent.** My earlier report saying otherwise
  was wrong and is corrected in §7.
- **Continuity Camera** cannot be blocked Mac-side; must be turned off on the
  iPhone. The pipeline is now immune to its reordering either way.
- **New operational rule:** never start this pipeline over SSH (§6).
