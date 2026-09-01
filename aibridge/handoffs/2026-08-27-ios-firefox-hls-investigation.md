# iPhone 14 / Firefox for iOS HLS investigation

Date: 2026-08-27 (America/Lima)  
Scope: investigation followed by a narrowly scoped Firefox-iOS-only deployed-page correction; no desktop code was changed.

> Update, 17:23 PET: Tina was a working A/B control and isolated a Firefox-iOS-only live-page difference. A checksum-backed Firefox-iOS trial was performed. The original investigation follows; the implementation record below is authoritative.

> Final update, 17:31 PET: that Firefox-iOS trial did not resolve the symptom and was fully rolled back from the verified backup. The deployed Tanzania page is again byte-for-byte identical to its pre-change version (`bf57060b…3281472f`). The actual cause is the iPhone's bare-IP/port-80 route, described below.

> Verified resolution, 21:00 PET: the iPhone owner reloaded Firefox iOS after the video-only Firefox-iOS change and confirmed playback works. This is the implemented fix.

## Implemented and verified fix: Firefox iOS video-only HLS

The exact iPhone session at `http://100.100.15.2:8080/` was captured in nginx access logs. Firefox iOS fetched the master playlist, audio/video playlists, init fragments, and media segments successfully (HTTP 200), then abandoned the native session and retried. This ruled out routing, HTTP, CORS, and basic HLS availability.

Tanzania's separate AAC rendition was the material host difference: it reported about **2.3 kb/s**, while Tina's working AAC rendition reported about **105 kb/s**. The main `<video>` is permanently muted and the page's deliberate audio control already uses its own audio element, so the safe client-specific compatibility path is to give Firefox iOS the video media playlist directly.

The deployed page now does this only for the `FxiOS` user agent:

```js
var FIREFOX_IOS_VIDEO_HLS_URL = window.location.origin + '/hls/cam/video1_stream.m3u8';
video.src = /FxiOS\//.test(navigator.userAgent)
  ? FIREFOX_IOS_VIDEO_HLS_URL : HLS_URL;
```

Desktop, Safari iOS, and every non-Firefox-iOS client continue using the original master playlist. The separate room-audio control remains intact. The deployed page hash is `4c4aa2f480d08a6b996e21eb43fc4992b1d5b2933914a10f8bc5aa0b683a75e2` and nginx was verified serving it. No service restart was needed.

Rollback: restore `shared/amicusbriefs/backups/2026-08-27T1719-05/tanzania-live-index-before-video-only-firefox-ios.html` to `/var/www/hls-livecam/index.html` as `root:root`, mode `0644`; no restart is required.

## Disproved route hypothesis

The iPhone screen capture shows a bare address: `100.100.15.2`, and Tanzania has no port-80 listener. However, the user confirmed the actual failing address was `http://100.100.15.2:8080/`. This was not the cause.

Both of these endpoints are valid, but neither is required for the fix:

- `https://tanzania.humboldt-polaris.ts.net/` — preferred; Tailscale Serve terminates HTTPS and proxies to the viewer.
- `http://100.100.15.2:8080/` — direct Tailscale endpoint.

The preferred hostname and `https://tanzania.humboldt-polaris.ts.net/hls/cam/index.m3u8` were verified from the host as HTTP/2 200. The direct `:8080` HLS endpoint was also HTTP 200. This explains why Tina can work at its no-port address while Tanzania cannot: their no-port front-end arrangements differ.

## Reverted implementation trial: Firefox iOS hls.js selection

Tina's deployed `/var/www/hls-livecam/index.html` exactly matched this repository's package viewer (`281b2dc…ce26f8f0`). Tanzania's deployed page differed by 17 untracked lines. Its iOS override forced every iOS browser into native HLS (`isIOS || !Hls.isSupported()`), unlike Tina's working conditional (`!Hls.isSupported()`).

Tanzania temporarily exempted only the Firefox-on-iOS user agent from that force-native override:

```js
var isFirefoxIOS = /FxiOS\//.test(navigator.userAgent);
if ((isIOS && !isFirefoxIOS) || !Hls.isSupported()) {
```

Effect: Firefox iOS used the same hls.js-when-available path as Tina; it still fell back to native HLS if MSE/hls.js was unavailable. Safari iOS and desktop were unchanged. The user confirmed it did not resolve the issue, so the trial was removed.

The trial's deployed hash was `db47f8dfb5a6a8785602f1a38014dc33a4de1f4718b9270d23773fc541c1108a`. It was then restored to the original `bf57060b6f1a7ca53877cdc15a1a4d1cf2e9ffe6320f76791488f7653281472f`, verified by SHA-256. No services were restarted.

Reversible backups (checksum-verified):

- `shared/amicusbriefs/backups/2026-08-27T1719-05/repository-before-ios-firefox-fix.tar.gz`
- `shared/amicusbriefs/backups/2026-08-27T1719-05/tanzania-live-index.html`
- `shared/amicusbriefs/backups/2026-08-27T1719-05/SHA256SUMS`

The rollback was performed by restoring the saved `tanzania-live-index.html` to `/var/www/hls-livecam/index.html` as `root:root`, mode `0644`; no restart was needed.

## Executive finding

The evidence does **not** support a safe mobile-only code fix yet. The iPhone is loading the responsive single-camera page and is intentionally routed to native WebKit HLS; Firefox on iOS uses WebKit. Desktop uses hls.js. Therefore the fault lies in the native-HLS delivery path or in an iOS-specific browser failure that must be captured before changing code.

I deliberately made no desktop changes and no mobile changes. No backup was needed because no implementation occurred.

## Evidence collected

- The supplied screen capture is the single-camera page, not `/cams/`: it shows the address `100.100.15.2`, the video area with static, and the HLS health lamp red while Camera, MediaMTX, and RTSP are green. It was captured at 15:04.
- The live host is `tanzania`, with LAN address `192.168.18.5` and Tailscale address `100.100.15.2`. nginx listens on `:8080`; MediaMTX listens on `:8888`; Tailscale Serve owns HTTPS `:443` and proxies `/` to `http://127.0.0.1:8080`.
- The deployed viewer sets `HLS_URL` to same-origin `/hls/cam/index.m3u8`. nginx proxies that location to local MediaMTX on port 8888 with buffering disabled. This avoids HTTPS-to-HTTP mixed content for the main viewer.
- The deployed player explicitly detects iOS and chooses native HLS (`video.src = HLS_URL`), even if hls.js reports support. Desktop takes the hls.js path. This is an existing iOS-only branch.
- Media encoding is compatible with current Apple native HLS requirements: H.264 Constrained Baseline / level 4.0 / yuv420p, 1280x720 at 15 fps, plus AAC-LC audio. The master playlist declares matching `AUDIO="audio"` and `GROUP-ID="audio"` values.
- MediaMTX is configured for Low-Latency HLS (`hlsVariant: lowLatency`) using fMP4 fragments, 1-second segments, and 200 ms parts. The playlist emitted HLS version 9/10 LL-HLS tags.
- The host logs record an actual source interruption at 15:48:43--15:48:51: MediaMTX repeatedly reported `no stream is available on path 'cam'` and then recovered. This does not align with the screen capture at 15:04, but it proves the stream had a later independent interruption.
- The iPhone 14 is an active Tailscale node (`iphone-14.humboldt-polaris.ts.net`), last seen at 21:50 UTC. The final hostname-level HTTPS probe was interrupted at the user's request and therefore is not asserted as a result.

## Important deployment finding

The checked-out repository is not the deployed single-camera viewer.

| File | SHA-256 / state |
| --- | --- |
| `pkg/usr/share/hls-livecam-server/index.html` | differs from live `/var/www/hls-livecam/index.html` |
| `pkg/usr/share/hls-livecam-server/cams/cams.html` | matches live `/var/www/hls-livecam/cams/cams.html` |

In particular, the deployed page contains the explicit iOS native-HLS branch; the tracked package file inspected in this checkout does not. A repository-only patch would consequently not repair the host presently under test, and overwriting the deployment from this checkout could remove live behavior. This is the principal reason no fix was made.

## Separate issue found: multi-camera page

`/cams/cams.html` constructs streams as `http://<camera-ip>:8888/cam/index.m3u8`. On an HTTPS page, that is mixed content and native iOS will block it. The main single-camera page in the supplied screen capture does **not** use this route, so this is not assigned as the root cause of the reported failure. It should be tracked separately.

## Most likely causes, ordered

1. Native iOS HLS rejects or stalls on this host's LL-HLS/fMP4 delivery under the exact network/Tailscale route, while desktop hls.js tolerates it. This fits the browser-path split and the screenshot's red HLS lamp.
2. An intermittent upstream publishing gap (the later 15:48 logs demonstrate this can happen). It does not by itself establish causation for the 15:04 iPhone capture.
3. A stale deployed/mobile viewer or cached asset. The live file has diverged from the repository, so its provenance and cache behavior need verification.

Codec incompatibility, a mismatched audio group, and main-viewer mixed content are currently *not* supported by the evidence.

## Required next diagnostic (no code change)

Use Safari Web Inspector for the actual iPhone session, then capture:

1. Network results for `/hls/cam/index.m3u8`, the video init fragment, and one media part/segment (URL, HTTP status, response headers, timing).
2. Console output and the video element's `error.code`, `networkState`, `readyState`, and `currentSrc` after the failure.
3. The exact URL/protocol in use (`http://100.100.15.2`, `http://...:8080`, or `https://tanzania.humboldt-polaris.ts.net`).
4. Matching MediaMTX/nginx logs for that timestamp.

If the native player is shown to fail specifically on LL-HLS, the safe remediation is server-side compatibility output (standard fMP4 or MPEG-TS HLS) or a native-only fallback playlist. Either requires explicit approval because it changes the stream delivery shared by desktop and mobile unless a separate mobile-only route is introduced and tested.

## Original no-change record (superseded)

- No repository source code changed.
- The deployed `/var/www/hls-livecam/index.html` was briefly changed for the trial, then fully restored as described above.
- No services were restarted.
- The later implementation created and checksum-verified the backups listed above.
