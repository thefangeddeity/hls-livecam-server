# macOS run-11 — Pause relocation (shipped) + DMG scoping (research only)

**To:** Tech Lead
**Target:** ariana
**Status:** §1 shipped and verified. §2 is a scoping report as instructed —
**nothing built, no DMG produced.** Live pipeline untouched throughout
(`HTTP 200` confirmed after deployment).

---

## 1. Pause relocation — shipped

Moved `Pause` out of the feed strip into the footer, next to `Stop server`;
renamed to `Pause server` / `Resume server`. `Repair` stayed under the feed
— confirmed correct, it's a genuine pipeline-reconvergence action, not a
server toggle.

The shared `action_busy` lock on `Dashboard` already covered all three
actions generically (`Dashboard.set_action_busy()` refreshes both
`p_feed` and `bottom` regardless of which one triggered it), so moving
Pause's widget location required no change to the locking mechanism itself
— confirmed by reading the code, not assumed. Repair, Pause, and On/Off
still cannot run concurrently.

Verified: `FeedPanel` no longer has a `b_pause` attribute; `BottomBar` does,
reads "Pause server"; screenshot confirms layout (attached, and in
`~/Downloads`). Deployed via `./bin/camdash-gui`, stream confirmed `HTTP 200`
after.

---

## 2. DMG — scoping report

### 2a. Scope — what each actually requires

**Operator app only** (bundles the GUI, points at an existing node):

- Needs PySide6 + the `gui/` package. Does **not** need mediamtx, the
  Flask control-plane, or a launchd agent.
- **Still needs `ffmpeg`** — the preview panel decodes the node's RTSP
  stream locally (`VideoWorker` spawns `ffmpeg -i rtsp://...`). Not
  optional for this shape.
- **A real finding, not assumed:** the current codebase is not built for
  this shape. `gui/probes.py` reads `cd.API_BASE`, which resolves through
  camdash's own config to `127.0.0.1` — hardcoded to being colocated with
  the node. Worse, `SlowWorker`'s health checks (`cd.proc("ffmpeg")`,
  `cd.proc("mediamtx")`, `cd.services_running()`) scan the **local** process
  table. Point this app at a remote node today and every VIDEO-panel row
  reads `DOWN` regardless of the remote node's actual health, because it's
  checking for processes that were never going to be on this machine.
  "Operator app only" is not a packaging change — it's a real code change
  (a configurable target host, and probes that ask the remote node's own
  `/api/*` for health instead of scanning local processes).
- A family member with no node of their own gains nothing this can't
  already do in a browser at `http://<node>:8080`.

**Full node** (recipient's Mac becomes a camera peer):

- Needs: Python 3.14 runtime, the full venv (Flask, numpy, Pillow, psutil,
  PySide6), `ffmpeg` (Homebrew or vendored), `mediamtx` (already vendored
  in this repo — no separate install), a launchd agent, camera TCC consent,
  and something equivalent to `bin/livecam-setup` to detect the camera
  index and write `config.env` on a machine that isn't ariana.
- `livecam-setup` today is a technical CLI tool assuming an operator who
  can read its prompts, not a non-technical family member. Making this
  installer-grade (detect the camera, pick free ports, handle a name
  collision with another peer) is real, unscoped work, separate from the
  DMG/signing question this brief asked about.
- Matches the "first among equals" direction stated previously. Costs
  substantially more than the operator-only shape.

**Not deciding between these — reporting the actual weight of each, as
asked.**

### 2b. Gatekeeper — tested, not summarized from documentation

No second Mac is reachable to test on (`tailscale status` — the only
macOS peer is ariana itself). Tested the actual mechanism on ariana instead:
quarantine enforcement is a Gatekeeper/`spctl` decision keyed to the
`com.apple.quarantine` xattr and the code signature, not to which physical
machine is asking — so this reproduces the recipient's exact experience.

**Test 1 — fully unsigned, quarantine flag set exactly as a browser
download would set it:**
```
xattr -w com.apple.quarantine "0081;<hex-time>;Safari;" TestApp.app
spctl -a -vvv TestApp.app
  → TestApp.app: rejected
    source=no usable signature
```
`open TestApp.app` did not silently fail — it triggered a real modal block
(confirmed via `CoreServicesUIAgent`, the actual process that hosts the
"...is damaged and can't be opened" dialog, appearing and holding a
window). This is the refusal the brief described, reproduced for real, not
assumed from Apple's docs.

**Test 2 — ad-hoc signed (`codesign -s -`), same quarantine flag:**
```
codesign -dv TestApp2.app
  → Signature=adhoc, TeamIdentifier=not set
spctl -a -vvv TestApp2.app
  → TestApp2.app: rejected
```
**Confirms the brief's own suspicion exactly: ad-hoc signing does not clear
the quarantine block.** Verified, not taken on faith.

**Developer ID + notarization — not tested.** This needs a paid Apple
Developer Program membership and credentials I don't have access to. I
can't produce an empirical result for the one path that's supposed to
actually work; I can only report that it's the documented mechanism Apple
provides specifically to satisfy the check that just rejected both tests
above, and that no other path in this test cleared that check.

**Conclusion, from what was actually run:** of the three options, two are
now proven not to work (unsigned, ad-hoc). The PM's choice is narrower than
"pick one of three" — it's "pay for Developer ID, or ship with the one
Terminal-command workaround instructions, accepting that defeats shipping
to non-technical family without a walkthrough call."

### 2c. Build path — evaluated with real builds, not just install checks

| Path | Installs on 3.14? | Actually builds/runs? | Needs CLT? |
|---|---|---|---|
| Hand-built `.app` (already proven) | n/a | **Yes — this is `camdash-gui.app`, shipped and working today** | **No** |
| `py2app` | Yes, clean wheel install | Alias-mode build succeeded and ran (`hello from py2app test`, exit 0) | No, for alias mode |
| `PyInstaller` | Yes, clean wheel install | **Failed** — see below | **Yes, and fails** |

**PyInstaller is disqualified on this machine as-is.** A real build
(`pyinstaller --windowed`) fails at the bootloader-assembly step:
```
SystemError: lipo command (...) failed with error code 1!
xcrun: error: invalid active developer path (/Library/Developer/CommandLineTools),
missing xcrun at: /Library/Developer/CommandLineTools/usr/bin/xcrun
```
Confirmed `lipo` itself is broken standalone (same `xcrun` error, unrelated
to PyInstaller) — PyInstaller needs it to thin its prebuilt universal
bootloader to a single architecture. **Direct answer to the brief's own
question: yes, this path needs CLT, and CLT is the thing this machine has
been missing since run-1.** Installing CLT to unblock PyInstaller is itself
a real prerequisite someone has to do first.

**py2app's easy test passed; the real one is still open.** Alias mode
doesn't freeze dependencies — it's a thin shim pointing back at this
venv's `site-packages`, useful for confirming basic 3.14 compatibility (which
it did) but **not something you could hand to a family member**; it breaks
the moment the venv it points at isn't there. The real question — does
py2app's standalone mode correctly freeze PySide6's native Qt frameworks
and plugins — is untested. That's real build work (not "run pip install"),
and building it now would mean starting the DMG this brief said not to
build yet. Flagging as the next concrete validation step if py2app is the
chosen path.

**Recommendation, with reasoning:** the hand-built `.app` approach is not
a fallback — it's the only one of the three with a working, running proof
on this exact machine today (`camdash-gui.app`), needs zero additional
tooling, and there is nothing py2app's standalone mode could do that a
hand-assembled `Contents/{MacOS,Resources,Frameworks}` with `PySide6`
copied in cannot also do, for the cost of writing the copy step once. Would
only recommend py2app over it if the standalone-mode test above is run and
reveals real savings — currently unproven.

### Icon and TCC — reasoned from what's already built, one nuance worth stating

A real bundle's `Info.plist` (`CFBundleIconFile`) is what actually makes an
icon persist when the app isn't running — confirmed already, this is
exactly the run-9 fix. `camdash-gui.app` already has this; **no rewiring
needed**, it carries forward into a DMG unchanged.

TCC nuance worth being precise about: the **operator GUI itself never
touches the camera** — `broadcast-api` does (established back in run-2).
So "one clean TCC prompt instead of the Terminal-attribution mess" is a
real benefit **only in the full-node shape**, where the bundle also runs
capture. In the operator-only shape, the GUI's bundle identity mainly
matters for Dock/Finder icon persistence, not for a camera prompt — it will
never see one.

---

## 3. Order followed

§1 shipped first (small, mechanical). §2 stayed research-only — no DMG,
no `.app` beyond the disposable test copies used for the Gatekeeper checks
(deleted after use).

---

## Verification

§1: live pipeline `HTTP 200` after deployment, screenshot confirms correct
button placement and labels, programmatic check confirms `FeedPanel` no
longer holds a `b_pause` reference.

§2: every claim above that could be tested, was — Gatekeeper behavior,
ad-hoc signing, PyInstaller's actual failure, py2app's actual (alias-mode)
build and run. The two claims that could not be tested (Developer ID
notarization, py2app standalone with real PySide6) are named as such, not
presented as verified.
