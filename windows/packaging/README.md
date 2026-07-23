# Windows node MSI packaging

Builds the installer for the Windows camera node (tag stream `win-vX.Y.Z`,
independent of the Linux `vX.Y.Z` / macOS `mac-vX.Y.Z` lines).

## Build

```powershell
powershell -ExecutionPolicy Bypass -File build-msi.ps1 -Version 1.0.0
```

Output: `build\hls-livecam-win-<version>.msi`.

## Inputs and where they come from

| Input | Source | In git? |
|-------|--------|---------|
| `hls-livecam-win.exe` | `cargo build --release` in `windows\` (embeds the `requireAdministrator` manifest) | no (build artifact) |
| `bin\ffmpeg.exe` | gyan.dev static FFmpeg build | **no — never vendored** |
| `bin\mediamtx.exe` | mediamtx GitHub release (v1.15.2) | **no — never vendored** |
| `assets\icon.ico` | tracked in `windows\assets\` | yes |
| WiX v3.14.1 | fetched by `build-msi.ps1` into `tools\` | no |

**ffmpeg/mediamtx are deliberately not committed** (repo bloat — ~150 MB
combined). The build stages them from `windows\target\release\bin\`, which is
where the app's own run-2 bootstrap already places them on 7elwe. If that dir
is empty on a fresh checkout, drop the two binaries there before building
(gyan.dev FFmpeg; mediamtx release). `build-msi.ps1` fails loudly with these
instructions if either is missing.

Everything under `tools/` and `build/` (and any `*.msi`) is git-ignored.

## What the MSI does

- Installs to `C:\Program Files\hls-livecam-win\`, with `ffmpeg.exe` /
  `mediamtx.exe` in `bin\` next to the exe (where the resolver looks).
- Registers an **elevated `ONLOGON` Scheduled Task** `hls-livecam-win`
  (`/RL HIGHEST` = HighestAvailable) — the same autostart mechanism the app
  self-registers (`src/autostart.rs`), created here so it exists at install
  time rather than only after first launch.
- Adds a **Start-menu entry** with the icon. **No desktop shortcut** (by PM
  decision — none is authored).
- Force-terminates a running instance before replacing files, so an
  **upgrade** over a running app doesn't fail on a locked exe. (The app
  intercepts `WM_CLOSE` as *minimize*, so the MSI terminates rather than
  asks it to close.)
- **Major-upgrade**: a newer MSI replaces an older install in place; a
  downgrade is blocked.

## Uninstall — two tiers

**Default** (Add/Remove Programs, or `msiexec /x <ProductCode>`): removes the
program files, the Scheduled Task, and the Start-menu entry. **Keeps**
`%APPDATA%\hls-livecam-win` (message, feed-mode, device selection, theme,
`cams.json`) — that dir is created by the app at runtime, is not tracked by
the MSI, and is left untouched, so a reinstall picks the operator's config
back up.

**Purge** (opt-in, never default, never silent):

```powershell
msiexec /x <ProductCode> PURGECONFIG=1
```

Does everything the default does **and** deletes `%APPDATA%\hls-livecam-win`.
Use only for a genuinely clean removal — this loses the cam list.

> The purge is a documented command rather than an uninstaller checkbox: a
> custom WiX maintenance-dialog was judged higher-risk than its value for a
> single-operator tool. The default path already preserves config, so config
> loss cannot happen by accident — the purge is strictly opt-in.
