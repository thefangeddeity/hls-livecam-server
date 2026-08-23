# macOS Operator GUI — Layout + Parity Report (run-3)

**To:** Tech Lead
**Target:** ariana (MacBookPro13,2, Sonoma 14.8.7 / OCLP)
**Date:** 2026-08-09
**Status:** All four run-3 items landed. Running on the machine. Not tagged — PM
visual-gates.

1842 lines across six files (was 1553).

---

## 1. §1 — The feed is now the hero

The rigid 3×2 grid is gone. Layout is now three columns:

- **Left (3/14):** SYSTEM, DISK/SMART — content-height, stacked to the top, with
  a trailing stretch that soaks up the slack.
- **Centre (8/14):** FEED, stretching. Then PROCESSES.
- **Right (3/14):** NODE, VIDEO, MESSAGE — content-height, same pattern.
- **Bottom:** the action toolbar, full width.

**One thing the brief didn't anticipate, and it mattered.** Breaking the grid the
obvious way — feed panel stretches, side columns go content-height — grew the
*panel* but not the *picture*. The source is 16:9; a tall narrow slot contain-fits
it and leaves roughly 35 % of the panel as dead black bars above and below. The
feed was bigger and looked worse.

The fix was to put the surplus height to work: **PROCESSES moved out of the left
column and under the feed.** It takes the vertical slack the video can't use, so
contain-fit is now width-limited and the picture fills its panel edge to edge.
Feed is unmistakably the subject, with no dead bars. Left column is shorter as a
result, which is what content-height panels are supposed to look like.

---

## 2. §2 — Controls under the feed, Buzz isolated

Show / Blur / Hide / B&W now sit in a strip directly beneath the video, inside
the centre column. **Buzz is fenced off**: stretch, then a vertical divider, then
Buzz right-aligned in its danger fill. It reads as a separate class of action,
which is the point — it makes a noise in someone's living room.

Repair / Pause / On-Off left the feed area entirely and are now supervision
actions in the bottom toolbar, alongside Dark and Cam IPs.

---

## 3. §3 — LIVE vs up

Corrected in the VIDEO panel:

| Row | Word | Why |
|---|---|---|
| CAM | `LIVE` | on-air fact |
| HLS | `LIVE` | on-air fact |
| web | `LIVE` | the surface the family actually hits |
| ffmpeg | `up` | background service |
| RTSP | `up` | background service |
| mediamtx | `up` | background service |

Green on both — the word carries the distinction, not the colour, exactly as §2
intends. `status_color()` is now case-insensitive so `up` resolves without a
second token.

---

## 4. §4 — Missing surfaces

**4a. NODE panel — built.** HOSTNAME, LAN IP, TAILSCALE, HTTP, HLS, SERVER.
Hostname and Tailscale come from the control-plane's own `/api/info`; the LAN
address shells out to `ipconfig` behind a 60-second cache.

**4b. Bottom action toolbar — built.** Start/Stop server, Pause/Resume, Repair,
Dark, Cam IPs on the left; a status readout and `GPL 3.0` on the right. The
readout shows `Ready`, or the in-flight action (`Repairing…`, `Stopping…`) with
the destructive buttons disabled for the duration. Button labels track state —
it reads `Start server` right now because the launchd agent isn't installed and
the services are running manually.

**4c. CamHub — built and reachable.** `Cam IPs` opens a modal over camdash's own
`read_cams`/`write_cams`: four slots, editable label and IP, pin toggle,
reorder, clear, Save/Cancel. Scope deliberately matches camdash — `stream_path`
and `api_port` stay hand-edited in `cams.json`, so the two front-ends can't
disagree about what they own.

*Owning the miss:* run-2 listed this as shipping in v1 and it did not, and my
run-2 report said "CamHub-backed state," which read like it had. It hadn't. The
brief caught it correctly.

**4d. MESSAGE Cancel — added.** Reverts the box to the server's current text.
Enabled only when there are unsaved changes, and disabled with Save/Clear when
the message is locked.

---

## 5. §5 — Kept

The `PREVIEW ~1s · VIEWERS ~4–7s` chip is unchanged, still under the feed.
Flagged for backport to the Windows node.

---

## 6. What sudo would unlock

Per the PM's note — specifying, not running, per §7.

### SMART (DISK/SMART panel)

**The first blocker is not permissions — `smartctl` is not installed at all.**
`brew list` shows no smartmontools and `command -v smartctl` finds nothing. That
is why the panel reads `NO ACCESS`. There is also no `/etc/sudoers.d` drop-in on
this machine (the directory is empty).

Step 1 needs no sudo:

```bash
brew install smartmontools
```

Step 2 is the sudo part. **camdash already calls `sudo -n smartctl`** in
`read_smart()`, so once this lands the SSH CLI viewer gains the SMART panel with
**zero code change** — exactly the htop-style arrangement you described:

```bash
sudo tee /etc/sudoers.d/hls-livecam-ariana >/dev/null <<'EOF'
ron ALL=(ALL) NOPASSWD: /usr/local/sbin/smartctl
EOF
sudo chmod 0440 /etc/sudoers.d/hls-livecam-ariana
sudo visudo -c
```

Verify the binary path first (`command -v smartctl`) and match it exactly — a
sudoers rule with the wrong path silently grants nothing.

Worth knowing before spending the effort: this is an internal Apple SSD, and
those usually don't expose SMART attributes to smartctl at all. The likely
outcome is the panel moving from `NO ACCESS` to `N/A` — a more honest answer, but
not a populated panel. It pays off properly for external drives.

### CPU temperature — needs sudo *and* a code change

`powermetrics` is present at `/usr/bin/powermetrics` and is root-only, so a
sudoers entry would make it reachable. **But the sudoers entry alone changes
nothing**, because camdash's `_cpu_temp()` only tries
`psutil.sensors_temperatures()`, which does not exist on macOS — it never shells
out to anything. Reaching parity on CPU temp needs camdash edited, and §7 puts
camdash out of scope.

So this one is a scope call, not a permissions call. Three options:

1. Leave it. CPU TEMP stays `—` on both front-ends.
2. Add a powermetrics probe **to the GUI only** — fixes the dashboard, leaves the
   SSH viewer showing `—`. Divergence between the two front-ends.
3. Edit camdash's `_cpu_temp()` so both gain it. Correct, but it's the one file
   the brief protects.

I'd want your call before touching camdash. Also unverified: whether SMC
sampling reports die temp at all on 2016 Intel hardware under OCLP — worth a
one-shot `sudo powermetrics --samplers smc -n1 -i1000` before committing to any
of the three.

---

## 6b. Addendum — side-by-side parity pass against win-v1.0.1

After the PM supplied a Windows screenshot, a second pass aligned the two.

**Adopted from Windows:**

| Change | Was | Now |
|---|---|---|
| Column assignment | left SYSTEM+DISK, centre FEED+PROCESSES, right NODE+VIDEO+MESSAGE | left VIDEO+PROCESSES, centre FEED+SYSTEM, right DISK+MESSAGE+NODE |
| Row label case | `REALLOC`, `MEM`, `LAN IP` | `Realloc`, `Memory`, `Local IP` — only section titles shout (§3) |
| Row names | `CAM`, `web`, `RAM AVAIL` | `Camera`, `HTTP`, `RAM free` |
| Disk identity | `/dev/disk2` | `APPLE SSD AP0512J`, via `diskutil`, cached 5 min |
| NODE field order | Hostname first | Tailscale, Local IP, Hostname, HTTP, HLS, Server |
| Server state | `ON` / `OFF` | `running` / `stopped` |
| PROCESSES depth | 8 rows | 16 rows, filling the left column |
| MESSAGE API state | full `MESSAGE API — UP` row | compact `API up`, sharing the Lock row |
| Swap suffix | `[NONE]` | `[none]` |

**Meter colour ramp changed, and it's a spec fix as much as a parity one.** Bars
were green below 50 %. Windows uses its accent blue there, and it's right: under
the §2 spine decision green means *a service is up*, so spending it on a
23 %-busy CPU bar overloads the one token the whole status model rests on. Normal
is now `accent`; warn and critical are unchanged.

**Two bugs this pass surfaced:**

1. `Panel` added its body layout with no stretch factor, so a *stretched* panel
   shared surplus height between the section title and the body — PROCESSES
   rendered with its title floating in the middle of an empty box. Only visible
   once a panel was told to expand, which never happened before this layout.
   Fixed with `outer.addLayout(self.body, 1)`.
2. The 8→16 process row change was applied to `app.py`, but the count that
   matters (`_top_processes(8)`) lives in `probes.py`. The panel had 16 slots and
   was handed 8 rows.

**Where macOS is now ahead**, and worth backporting:

- The feed occupies roughly half the window width; on Windows it is closer to a
  quarter. This is the "feed is the hero" instruction actually carried through.
- The `PREVIEW ~1s · VIEWERS ~4–7s` chip (already flagged in §5).
- A `SHOW` / `BLUR` / `HIDE` mode word under the feed — Windows shows the mode
  only through button state.
- B&W is a **checkbox**; Windows renders it as a radio button, which is the wrong
  control for a toggle that isn't part of a mutually exclusive set.

**Deliberately not matched:** Windows keeps `Repair` inside the VIDEO panel. It
stays in the bottom toolbar here because run-3 §2 explicitly reclassified
Repair/Pause/On-Off as supervision rather than feed actions — a considered
deviation, not an oversight.

---

## 7. Corrections to the run-3 brief

1. **§4a: "no Tailscale on ariana yet" is wrong.** It's up and reporting
   **100.100.105.13**, straight from `/api/info`. The NODE panel shows the real
   address. The §7 `Pending` path is implemented for when a field is genuinely
   absent, but it isn't triggered by this one.

2. **Theme toggle — and a correction to my own earlier correction.** I first
   reported that no light theme could exist because design system §1 ratifies
   only a dark palette. The Windows screenshot shows a working **`Light theme`**
   button, so light tokens demonstrably exist in the Windows implementation —
   they are simply **absent from the ratified design system document**. That
   makes this a gap in the spec, not in this build.

   It is the one remaining parity gap. I have not invented a light palette,
   because §1 says the palette is ratified and not to be re-opened, and deriving
   one unilaterally would put the two platforms out of sync in a way that is
   harder to unpick than a missing button. **Needed: the Windows light token
   values, or clearance to derive them.** With either it is short work — every
   colour already routes through `tokens.py`, so it becomes a second token set
   and a QSS reapply, not a rewrite.

---

## 8. Verification

Same method as run-2: `QWidget.grab()` to PNG, no screen-recording permission
needed.

**Verified:** full render of the new three-column layout with live video; feed
fills its panel with no letterbox bars; LIVE/up wording correct per row; NODE
populated with real hostname, LAN IP and Tailscale address; bottom toolbar
present with correct state-tracking labels; CamHub dialog opens, loads and pads
to 4 slots, and renders its controls; MESSAGE shows Save/Clear/Cancel with
correct enable states; clean launch, no stderr.

**Not verified by clicking, deliberately — unchanged from run-2:** Show / Blur /
Hide, Buzz, Dark, Repair, Pause, On-Off, message Save/Clear/Cancel, and CamHub
**Save**. Each changes what the family sees, makes a noise, drops the stream,
changes launchd state, or writes `cams.json`. I opened the CamHub dialog and
dismissed it without saving; `cams.json` is still `[]`, untouched.

That click-through is still the first thing to exercise at the visual gate.
