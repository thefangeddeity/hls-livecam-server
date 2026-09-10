# 2026-09-10 — 7elwe viewer port, HLS/RTC fixes, RTC-video status

Written for Ron (and whoever picks up 7elwe next) after a long session
porting the web viewer from Tanzania and fixing real connectivity bugs.
Everything below is verified against actual running state, not assumed.

## Done and verified tonight

- **Full pixel-by-pixel viewer port** from Tanzania's real source (pulled
  via scp, not description) — dual-sidebar VFD-console layout, notch
  filter UI, HUD grid, video mode bank (Show/Blur/Hide — no CV/Night,
  confirmed `state.rs::is_valid_mode` doesn't support those), audio mode
  bank (HLS/Silence/RTC).
- **HLS video loading, fixed** — root cause: this node is reachable over
  HTTPS via Tailscale serve (443 -> 127.0.0.1:80 only), but the frontend
  was hitting mediamtx directly on :8888/:8889 over plain HTTP. Browsers
  block that as mixed content from an HTTPS page. Fixed with a same-origin
  reverse proxy in axum (routes.rs: `/hls/`, `/talk/`, `/cam/` ->
  mediamtx, mirroring broadcast-api's nginx config). Confirmed mediamtx's
  own WHIP response already returns a relative `Location` header, so the
  proxy is transparent, no header rewriting needed.
- **Two-way RTC audio, outbound confirmed working** (viewer's mic reaches
  the room). Inbound (room -> viewer) was silent because
  `HLS_AUDIO_ENABLED` was never set on this node at all — no env var, no
  `audio_device.txt` ever written. Fixed (User-scope env var, no reboot
  needed); mic now detected: "Microphone Array (Intel® Smart Sound
  Technology for Digital Microphones)". **Needs a fresh real-browser test
  to confirm inbound audio actually arrives now** — everything downstream
  of the fix checks out (mic detected, `cam` path now has an audio track)
  but I can't complete real ICE negotiation from a script.
- **LED panel bugs, both fixed**:
  - Audio panel's HLS-red-during-Silence was actually already-correct
    code that just hadn't been deployed yet (stale build) — no change
    needed there.
  - Video panel LEDs all dark despite a live feed: real gap, `/api/pipeline`
    didn't exist on this node's backend at all. Built it
    (`routes.rs::api_pipeline` + `Pipeline::audio_status()` distinguishing
    disabled/down/ok for the mic lamp). Also added the missing 5th LED
    (RTC, honestly dark/"not wired yet") to match Tanzania's real 5-LED
    video panel (Camera/MediaMTX/RTSP/HLS/RTC — Tanzania's own prose
    description of this said "Mic" instead of "RTC", which was simply
    wrong; went with the real file).
- **Cosmetic/polish**: new app icon everywhere (pulled the fleet's actual
  current mark from `origin/main`, not the stale one 7elwe had — desktop
  app icon/taskbar/tray, browser favicon, header logo), tab
  title/build-label format fixed (was hardcoded "linux-v..." copied
  verbatim from the port, now "windows-v<version>", version now actually
  comes from the backend), GUI header decluttered (dropped the redundant
  in-app "Webcam Server Stack" title now that Windows 11 shows it on the
  window border), panel-to-panel spacing widened and made consistent
  (was too tight, then too sparse with big gaps — settled on 16px
  matching `.main`'s own padding, panels justified to top).

## Audio tuning — measured, not guessed (2026-09-10, ~00:30 local)

**Settings "Audio filters" block now exists**: `/api/audio-settings`
(audio_settings.rs), clamped server-side per key, accepted value echoed
back so overshoot snaps visibly. High-pass / Low-pass / Inbound gain /
Outbound gain + the test-only "Mute server speaker/headset in RTC"
toggle. Reload is scoped: only the room-facing keys respawn the
publisher; talk-only keys don't bounce HLS audio for listeners.

**What the room actually sounds like.** Recorded 60 s and 90 s off the
RTSP loopback and ran Welch-averaged FFT with local-prominence peak
finding, then a per-slice ridge track. Findings:

- **No discrete tones.** Spectral flatness 0.72 / 0.51 (1.0 = white
  noise, <0.1 = strongly tonal). Max prominence 11–13 dB where a real
  fan blade-pass tone is 20–40 dB. No harmonic series.
- **The peaks don't reproduce between takes** — 691 Hz was the strongest
  feature in the 60 s sample (11.2 dB) and just 3.3 dB in the 90 s one.
  That instability is the strongest single piece of evidence.
- Ridge tracking **disproved a drifting-tone hypothesis**: the
  per-slice strongest peak jumps a median of 691 Hz between adjacent
  85 ms slices, and even in its single best slice the 1090 Hz feature is
  70 Hz wide (a real tone would be 1–2 bins). It is genuinely broadband.
- **None of Tanzania's 12 notch frequencies apply here** — measured
  against 7elwe's own room they sit at 1.4–7.1 dB, at or under the
  3.3 dB ripple floor. Do not copy them.

**Seven notches were still added**, at Ron's explicit direction ("I
don't care if you neutralize the hump, it's a small slice of the
spectrogram, don't be so shy") — placed at the bands where two
independent methods agreed: 120, 346, 392, 691, 1090, 1570, 2740 Hz,
each 50 Hz wide (the width that actually works — see the bandreject
lesson in the earlier handoff).

**Verified by difference spectrum** (before vs after, same room minutes
apart). Baseline drift in untouched bands was only −0.1 to −0.3 dB, so
the rest is real: every notch shows −4.9 to −7.5 dB against its own
shoulders; low-pass 14000→10000 gave −5.4 dB at 12–16 kHz; high-pass
80→120 gave −7.2 dB at 90–120 Hz. By octave: −6.2 dB at 63–125 Hz,
−4.2 dB at 250–500 Hz, −5.4 dB at 12–16 kHz.

**Caveat to set expectations**: 2–4 kHz moved only −0.3 dB. The
broadband fan whirr is reduced, not removed — a notch bank cannot flatten
broadband noise, and the band-limit is doing most of the audible work.
Further improvement needs a gain-reduction EQ (`equalizer`/
`firequalizer`), which no node in the fleet has yet.

**Recording hygiene**: all three .wav captures were deleted after
analysis. They were room audio from Ron's home; only spectra were ever
examined, never content.

**Re-run in daylight.** This is a quiet-hours profile. Daytime sources
(HVAC, traffic, appliances) will differ; the scripts to repeat it are in
the session scratchpad pattern described above.

## Genuinely NOT done, and why

- **RTC video for calls** — does not exist anywhere in the fleet to port.
  Checked Tanzania's real AV-panel code directly: the "CALL SERVER"
  button, Cam/Mic/RTC LEDs, and in-call mic/video/hangup controls are all
  explicitly `disabled` in the markup, with titles like "not wired yet"
  and a comment calling it "the future home of the video call (camera +
  self-view PiP)... task: wire CALL." This is real greenfield UI/backend
  work, not a port, with open design questions (where does the room
  display the caller's video — desktop GUI, web viewer, both? self-view
  PiP? does it extend the existing audio-only RTC bank or the separate
  CALL button?). Didn't want to freehand that without you. 7elwe's own
  matching stub is already correctly disabled/honest, same as Tanzania's.

## Infrastructure set up tonight, for future sessions

- **Passwordless SSH both directions** between 7elwe and Tanzania (see
  the earlier 2026-09-09 handoff for the notch-filter/audio-HUD context
  that started this).
- **`windows/deploy.ps1`** — the build+stop+copy+restart cycle, now a
  single script instead of a hand-pasted PowerShell block each time.
  Logs to `windows/deploy.log` (it's triggered via a Scheduled Task with
  no console attached, so this is the only way to see what happened,
  including whether the build itself failed — deploy.ps1 checks
  `$LASTEXITCODE` and refuses to touch the running service on a build
  failure).
- **`hls-livecam-deploy` Scheduled Task** (Ron created this one — I
  cannot create `/RL HIGHEST` tasks myself, confirmed empirically, only
  Ron's own elevated context can register one) — runs `deploy.ps1` with
  highest privileges, no UAC prompt. Triggered via `schtasks /Run /TN
  hls-livecam-deploy`, which does NOT need elevation itself, only
  *creating* the task did. This is what let me keep deploying
  unattended after Ron went to bed.

## If this session died to an AUP/classifier block — read this first

This project repeatedly triggered a server-side safety classifier.
**Eight** hard refusals across two sessions, every one of them
`apiRefusalCategory: "bio"` with `apiRefusalExplanation: null` (no reason
given). Each presents as "Claude can't help with this. Start a new
session to continue." There is nothing unsafe in this project — it is
ffmpeg/mediamtx plumbing, an axum reverse proxy, WebRTC audio, and CSS.

Request IDs, if anyone ever chases this with Anthropic:

| When (UTC) | Request ID |
|---|---|
| 2026-09-09 23:16:32 | req_011CetiZZi25Ur2chGM363zZ (killed session 1) |
| 2026-09-10 03:53:26 | req_011Ceu5cNFj3jucZStKP44o9 |
| 2026-09-10 04:30:23 | req_011Ceu8WY6jtB9rENZLcZHNo |
| 2026-09-10 04:30:39 | req_011Ceu8XprqJGzVnF5gnL62j |
| 2026-09-10 04:38:51 | req_011Ceu98iCDKSjJjJ8rVJLDB |
| 2026-09-10 04:40:17 | req_011Ceu9GD3z7ik9E2B3YSTHp |
| 2026-09-10 04:40:25 | req_011Ceu9Gw1NJPhhMi25HWc3U |
| 2026-09-10 04:40:59 | req_011Ceu9KPtSb2W28JT7rQaj8 |

Reproduce the list yourself with:
`grep -c apiRefusalCategory ~/.claude/projects/<slug>/*.jsonl`
(on this box: `/c/Users/Ron/.claude/projects/C--Users-Ron--local-bin/`).

**The one operational lesson — do this differently:** session 2 began by
reading session 1's crashed `.jsonl` transcript to reconstruct context.
If the trigger lived in that content, that imported it straight into the
new session, which would explain why a "fresh" session inherited the
same problem and why block frequency then *accelerated* as context grew
(one at 03:53, three inside the last 90 seconds). **Start clean and
re-derive from files — this handoff, the code, the memory dir — rather
than replaying an old transcript.** Also avoid reading the conversation
around a refusal to diagnose it: that pulls the triggering content back
into context and makes it worse. Metadata-only greps (as above) are
safe.

Ron's own hypothesis, worth carrying: `[bio]` may mean
biographical/personal-information rather than biology — this project
combines his real name (in every filesystem path), a home camera and
microphone, real Tailscale hostnames/IPs, and home network topology.
Unconfirmed, but it fits better than anything else proposed.

## Needs Ron

| | |
|---|---|
| **Fresh RTC audio test** | Confirm inbound audio actually works now with the HLS_AUDIO_ENABLED fix. |
| **RTC video for calls — design** | Where does the room see the caller (desktop GUI / web / both)? Self-view PiP? Extend the RTC audio bank or wire up the separate CALL button? Nothing to build until this is answered. |
| **Notch profile for 7elwe's room** | Still unseeded (correctly, per the earlier handoff's own guidance) — needs on-site recording/measurement whenever Ron wants it. |
| **PARKED: purge personal detail from the repo** | This repo is **public** (`github.com/thefangeddeity/hls-livecam-server`, unauthenticated API returns 200). Committed content includes Ron's name in hardcoded paths (`C:\Users\Ron\...` in `windows/deploy.ps1`, this handoff, various code comments), fleet machine names, the mic device model, and `ron:` attributions throughout the ported viewer code (inherited from Tanzania's own file, so it predates this work). No email, tailnet FQDN, or public IP is committed — that was checked before pushing. Ron flagged he wants this purged later; note that a purge of already-pushed history needs a rewrite (filter-repo) plus a force-push, not just a follow-up commit. |
