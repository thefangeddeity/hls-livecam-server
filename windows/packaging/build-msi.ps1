<#
  Builds the win-v1.0.0 MSI for the Windows camera node.

  Sources (NOT in git -- see README):
    * windows\target\release\camdash.exe           (cargo build --release, manifest build,
                                                    static CRT via .cargo\config.toml)
    * windows\target\release\bin\ffmpeg.exe        (gyan.dev static build)
    * windows\target\release\bin\mediamtx.exe      (mediamtx GitHub release)
    * windows\assets\icon.ico                      (tracked)

  Toolchain: WiX v3.14 standalone under packaging\tools\wix314 (auto-fetched
  if absent). Output: packaging\build\hls-livecam-win-<version>.msi.

  Usage:  powershell -ExecutionPolicy Bypass -File build-msi.ps1 [-Version 1.0.0]
#>
param(
  # One product version across the fleet -- the .deb, .dmg and .msi all
  # carry the release version rather than each platform keeping its own
  # line. Keep this in step with windows\Cargo.toml, which feeds the
  # viewer's own windows-v<version> build label.
  [string]$Version = "7.0.0"
)
$ErrorActionPreference = "Stop"
$pkg   = $PSScriptRoot
$win   = Split-Path $pkg -Parent
$tools = Join-Path $pkg "tools\wix314"
$build = Join-Path $pkg "build"
New-Item -ItemType Directory -Force $build | Out-Null

# --- 1. WiX toolchain (documented fetch, never committed) ---
$candle = Join-Path $tools "candle.exe"
$light  = Join-Path $tools "light.exe"
if (-not (Test-Path $candle)) {
  Write-Host "WiX not found -- fetching WiX v3.14.1 binaries..."
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  $zip = Join-Path $pkg "tools\wix314-binaries.zip"
  New-Item -ItemType Directory -Force $tools | Out-Null
  Invoke-WebRequest "https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip" -OutFile $zip -UseBasicParsing
  Expand-Archive $zip $tools -Force
}

# --- 2. Stage inputs; fail loudly if the un-vendored binaries are missing ---
$exe      = Join-Path $win "target\release\camdash.exe"
$ffmpeg   = Join-Path $win "target\release\bin\ffmpeg.exe"
$mediamtx = Join-Path $win "target\release\bin\mediamtx.exe"
$ico      = Join-Path $win "assets\icon.ico"
# The CV detector's weights (tracked in git, unlike ffmpeg/mediamtx).
# cv.rs looks in <exe>\models first, so this is where it has to land or
# CV stays dark on a fresh machine.
$model    = Join-Path (Split-Path $win -Parent) "models\yolov8n.onnx"
foreach ($f in @($exe,$ffmpeg,$mediamtx,$ico,$model)) {
  if (-not (Test-Path $f)) {
    throw "Missing build input: $f`n" +
          "  camdash.exe : run ``cargo build --release`` in windows\`n" +
          "  ffmpeg.exe/mediamtx.exe : NOT in git -- place the gyan.dev ffmpeg and`n" +
          "    the mediamtx release binary in windows\target\release\bin\ (see README)."
  }
}
Write-Host ("Staging: exe={0:N1}MB ffmpeg={1:N1}MB mediamtx={2:N1}MB" -f `
  ((Get-Item $exe).Length/1MB), ((Get-Item $ffmpeg).Length/1MB), ((Get-Item $mediamtx).Length/1MB))

# --- 3. Compile + link ---
$wxs    = Join-Path $pkg "hls-livecam-win.wxs"
$wixobj = Join-Path $build "hls-livecam-win.wixobj"
$msi    = Join-Path $build "hls-livecam-win-$Version.msi"

& $candle -nologo -arch x64 -ext WixUtilExtension `
  "-dProductVersion=$Version" "-dExePath=$exe" "-dFfmpegPath=$ffmpeg" `
  "-dMediamtxPath=$mediamtx" "-dIcoPath=$ico" "-dModelPath=$model" `
  -out $wixobj $wxs
if ($LASTEXITCODE -ne 0) { throw "candle failed ($LASTEXITCODE)" }

& $light -nologo -ext WixUtilExtension -spdb -out $msi $wixobj
if ($LASTEXITCODE -ne 0) { throw "light failed ($LASTEXITCODE)" }

Write-Host ("`nBuilt: {0}  ({1:N1} MB)" -f $msi, ((Get-Item $msi).Length/1MB))
