---
name: unattended-7elwe
description: Build, deploy, and verify the 7elwe Windows node without the operator present. Use when working on hls-livecam-server/windows and Ron is away, asleep, or has said to proceed autonomously — covers the elevated deploy task, and how to prove a change actually works rather than assuming it did.
---

# Working unattended on 7elwe

## Deploy

Do **not** hand-paste a stop/copy/restart block, and do not try to elevate
your own shell — it cannot be done (confirmed: `schtasks /Create /RL
HIGHEST` returns Access Denied from a non-elevated session, and
`Start-Process -Verb RunAs` raises a UAC dialog nobody is awake to click).

Instead trigger the pre-registered elevated task, which needs no
elevation to *run*:

```
schtasks /Run /TN "hls-livecam-deploy"
```

It executes `windows/deploy.ps1` (build → stop service → copy exe →
restart → report), which refuses to touch the running service if the
build failed. Wait for it, then read the log:

```bash
until powershell -Command "(schtasks /Query /TN 'hls-livecam-deploy' /FO LIST | Select-String 'Status').ToString()" | grep -q "Ready"; do sleep 5; done
tail -40 windows/deploy.log
```

If the task is ever missing, Ron must recreate it himself — that one step
needs his elevated context:

```
schtasks /Create /TN "hls-livecam-deploy" /TR "powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\Ron\Projects\hls-livecam-server\windows\deploy.ps1" /SC ONCE /ST 23:59 /RL HIGHEST /F
```

## Verify — evidence, not assumption

**Confirm what is actually running before debugging anything.** Multiple
hours were lost tonight to diagnosing symptoms produced by a build that
had never been deployed. Compare timestamps first:

```powershell
(Get-Item 'C:\Program Files\hls-livecam-win\camdash.exe').LastWriteTime
(Get-Item '.\target\release\camdash.exe').LastWriteTime
```

Then prove the change:

- **Backend** — hit the endpoint directly:
  `Invoke-WebRequest -Uri 'http://127.0.0.1/api/pipeline' -UseBasicParsing`
- **Appearance** — headless screenshot, then actually look at it:
  ```bash
  "/c/Program Files/Google/Chrome/Application/chrome.exe" --headless=new --disable-gpu --no-sandbox \
    --window-size=1600,1000 --virtual-time-budget=8000 --screenshot="/c/Users/Ron/AppData/Local/Temp/v.png" "http://127.0.0.1/"
  ```
  Use Git Bash, not PowerShell — PowerShell's call-operator quoting
  mangles the `--screenshot=` argument. This caught a real bug (a call
  bar visible on page load) that a passing build would never have shown.
- **Wiring** — drive the live page over DevTools Protocol rather than
  guessing whether handlers fire: launch Chrome with
  `--remote-debugging-port=9222`, take the page's `webSocketDebuggerUrl`
  from `http://127.0.0.1:9222/json`, and `Runtime.evaluate` against it
  (Node 24 has a built-in `WebSocket`, so no npm install). Await promises
  in the evaluated expression or you get `[object Promise]` back.

Video frames often do not paint in headless screenshots. A black player
is not evidence of a broken feed — check `/api/pipeline` and the HLS
manifest instead.

## Judgement while alone

- **Port from the real file, never from a description of it.** Both
  machines are reachable by passwordless SSH; `scp` the actual source.
  Descriptions drift — a peer's own summary of its LED layout was wrong
  about its own file tonight.
- **Ship the fix, flag the design.** A wiring bug with one right answer
  is fair game unattended. A feature with open product questions (where
  a caller's video should appear, what a redesign should look like) is
  not — leave it flagged in the handoff rather than inventing an answer
  nobody asked for.
- **Never fake a backend.** If an endpoint does not exist, let the UI
  degrade honestly (dark lamp, caught 404) and record the gap. Do not
  stub a plausible-looking response.
- **Write the handoff before the session dies, not after.** This project
  trips a server-side classifier repeatedly (see the memory entry); a
  session can end without warning. `aibridge/handoffs/<date>-<topic>.md`
  is the convention, and it is what the next session should read
  *instead of* replaying an old transcript.
