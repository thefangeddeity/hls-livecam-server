#requires -RunAsAdministrator
<#
.SYNOPSIS
  Build+deploy camdash.exe: stop the service, copy the freshly-built exe
  into Program Files, restart it, report what's running.

  Registered as a Scheduled Task ("hls-livecam-deploy", run with highest
  privileges) so it can be triggered without a UAC prompt -- Stop-Process
  on the elevated camdash.exe process, and writing into Program Files,
  both need an elevated token; this script IS that elevated token,
  bootstrapped once at task-registration time rather than re-prompted per
  run. See autostart.rs's module doc for the same mechanism used for the
  app's own logon autostart.
#>

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\Ron\Projects\hls-livecam-server\windows'
$installed = 'C:\Program Files\hls-livecam-win\camdash.exe'
$built = "$repo\target\release\camdash.exe"
# Triggered via schtasks /Run -- no console attached, so this is the only
# way to see what happened afterward (a build failure here would
# otherwise be silent: the old exe just keeps running, looking fine).
$log = "$repo\deploy.log"

function Log($msg) { $msg | Out-File -Append -FilePath $log -Encoding utf8 }

"=== deploy $(Get-Date -Format o) ===" | Out-File -FilePath $log -Encoding utf8
Set-Location $repo
$buildOutput = cargo build --release --bin camdash 2>&1 | Out-String
Log $buildOutput
if ($LASTEXITCODE -ne 0) {
    Log "BUILD FAILED (exit $LASTEXITCODE) -- not touching the running service."
    exit 1
}

Stop-ScheduledTask -TaskName 'hls-livecam-win' -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2
Get-Process camdash, ffmpeg, mediamtx -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Copy-Item $built $installed -Force
Log "deployed: $((Get-Item $installed).LastWriteTime)"

Start-ScheduledTask -TaskName 'hls-livecam-win'
Start-Sleep -Seconds 4
$procs = Get-Process camdash, ffmpeg, mediamtx -ErrorAction SilentlyContinue |
    Select-Object ProcessName, Id, StartTime | Format-Table | Out-String
Log $procs
Log "=== done ==="
