# macOS run-10 — countdown port, Tailscale cleanup

**To:** Tech Lead
**Target:** ariana
**Status:** Both items done, verified, deployed. Live pipeline untouched
(`HTTP 200` confirmed throughout; feed mode confirmed back at `show` after
every test).

Also: checkboxes alphabetized (direct PM instruction, quick fix), and the
countdown hold time set to 7s per the PM's own call, overriding my
calibrated 4s.

---

## 1. Countdown port

### 1a. Mechanism — ported verbatim, as given

CSS, markup, `fadeOut()`, and the `setFeedMode()` trigger shape (fade at
POST time, `reloadStream()` fired 1s after the POST resolves) implemented
exactly as supplied, in both `index.html` and `index.template.html`.
Nothing invented — where the brief gave code, I used that code.

### 1b. The hold time — measured, not ported

**Did not use 16.** Measured ariana's actual transition in a real browser
against the real live stream — pixel-sampled the `<video>` element via
canvas, timed in milliseconds from `setFeedMode()` call to the first frame
visibly reflecting the new mode. Three conditions tested:

| Condition | Result |
|---|---|
| show→cloak, no reload (buffer drains naturally) | 5.10s |
| cloak→show, no reload | 4.65s |
| show→cloak, **with `reloadStream()`** at t+1s (the real mechanism) | **1.20s** |
| cloak→show, with reload | **1.65s** |

Forcing HLS.js to rebuild snaps it to the live edge of the now-1s-segment
manifest almost immediately, instead of draining several seconds of
pre-switch buffer — that's the actual value of `reloadStream()`, confirmed
empirically rather than assumed.

Calibrated hold: **4s** (worst measured trial × ~2.3 margin). **The PM
raised it to 7s**, which is what shipped — still comfortably justified by
the same data, just a more generous margin than my pick. Documented inline
in the code as his call, not mine, so a future reader doesn't mistake it for
the measured number.

Visual correctness verified separately from timing: triggered `fadeOut()`
client-side with an extended hold (no POST, no pipeline call) to get a
stable screenshot — video correctly faded to black, large tabular-nums
countdown, dimmed "Switching feed…" label beneath, matching the given CSS
exactly.

### 1c. Windows backport

Not touched, as scoped. Still queued.

---

## 2. Tailscale launchd cleanup

**1. Enumerated before touching anything, per the brief.** Current state:
**nothing to enumerate.** `launchctl list`, both the user (`gui/501`) and
system domains, and a filesystem search of `~/Library/LaunchAgents`,
`/Library/LaunchAgents`, `/Library/LaunchDaemons` all came back empty for
`tailscale`. No stale entry, no plist file. Best explanation: my own
`brew services restart tailscale` in run-9 printed "Successfully stopped
`tailscale` (label: `homebrew.mxcl.tailscale`)" — that was a real
`launchctl bootout`, and it appears to have cleared the registration at the
time, even though the brew formula itself was never actually installed
(`brew list` confirms no `tailscale` formula or cask present now, and
wasn't in run-9 either).

**2/3. Nothing to boot out or remove.** Re-verified `tailscale status`
still reports `100.100.105.13` and the full expected peer list.

**Binary risk, checked before concluding anything:** `/usr/local/bin/tailscale`
is the one on `PATH`. It's a 3-line shell shim
(`exec /Applications/Tailscale.app/Contents/MacOS/Tailscale "$@"`), owned by
`root:admin`, dated to the Tailscale.app install — it is Tailscale.app's own
CLI shim, not Homebrew's. `brew list` confirms no formula/cask is installed
to conflict with it. **The risk the brief flagged doesn't apply**: there is
no Homebrew-owned CLI to lose.

**Underlying failure — fixed with a LaunchAgent, not the in-app toggle.**
Tailscale.app ships its own login-item mechanism
(`TailscaleLoginItemHelper-macsys.app`, bundle ID
`io.tailscale.ipn.macsys.login-item-helper`, the modern `SMAppService`
path) — normally toggled from the app's own menu-bar preference. Confirmed
via `sfltool dumpbtm` and `launchctl list` that it is **not currently
registered**, i.e. "Open at Login" is off, which is consistent with
Tailscale silently not surviving a reboot. **Could not toggle it from here**
— this session has no Accessibility/Automation permission (`osascript` to
System Events fails with `-1719` or hangs outright), so I can't drive the
app's GUI checkbox.

Used the same pattern already proven in this project (`com.livecam.autostart`
for the pipeline itself): a LaunchAgent,
`~/Library/LaunchAgents/com.livecam.tailscale-autostart.plist`, `RunAtLoad`
running `open -a Tailscale`. Bootstrapped and confirmed registered
(`launchctl list` shows it, clean exit code). This launches the app at
login; the System Extension holds the already-authenticated tailnet state
and reconnects on its own once the app is running — no separate "stay
connected" action needed.

**Not done:** the native in-app "Open at Login" toggle, since it needs
permission this session doesn't have. If the PM wants that instead of (or
in addition to) the LaunchAgent, it's a one-click menu item — happy to
remove the LaunchAgent if he'd rather rely on the native toggle once it's
set.

---

## 3. Also done, quick fixes from direct instruction

- **Checkboxes alphabetized:** B&W Blur, Light theme, Lock message, Mute
  Buzzes (was B&W Blur, Lock message, Mute Buzzes, Light theme). Both
  `index.html` and `index.template.html`.

---

## Verification

Live pipeline confirmed healthy (`HTTP 200`) throughout, including after
every timing trial. Feed mode confirmed returned to `show` after all
testing — nothing left in `cloak`. The three real-condition timing trials
and the visual overlay check were run against the actual live stream in a
real browser tab, not simulated. Tailscale confirmed still reporting
`100.100.105.13` after the LaunchAgent was bootstrapped.
