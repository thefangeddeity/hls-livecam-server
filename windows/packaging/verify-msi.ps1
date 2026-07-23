<#
  Runtime verification for the win-v1.0.0 MSI. RUN ELEVATED (admin PowerShell).

    powershell -ExecutionPolicy Bypass -File verify-msi.ps1

  Exercises the full matrix and prints PASS/FAIL:
    * install v1.0.0  -> files in bin\, ONLOGON/Highest task, Start-menu
      shortcut, NO desktop shortcut, ARP entry
    * default uninstall -> everything gone, %APPDATA%\hls-livecam-win KEPT
    * purge uninstall (PURGECONFIG=1) -> %APPDATA%\hls-livecam-win GONE
    * upgrade v1.0.0 -> v1.0.1 with the app RUNNING -> old instance
      terminated, single product, version bumped (no side-by-side)

  SAFETY: your %APPDATA%\hls-livecam-win (cams.json etc.) is backed up before
  the destructive tests and restored at the end, even on error. The upgrade
  test briefly launches the camera app (feed may blink on for a few seconds).

  Leaves the machine CLEAN at the end (no MSI product installed) so you can
  then install the shipped MSI fresh for your own visual gate.
#>
param(
  [string]$Msi100 = "$PSScriptRoot\build\hls-livecam-win-1.0.0.msi",
  [string]$Msi101 = "$PSScriptRoot\build\hls-livecam-win-1.0.1.msi"
)
$ErrorActionPreference = "Continue"
$InstallDir  = "$env:ProgramFiles\hls-livecam-win"
$ConfigDir   = "$env:APPDATA\hls-livecam-win"
$StartLnk    = "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\HLS Livecam\HLS Livecam.lnk"
$logDir      = "$PSScriptRoot\build\verify-logs"
New-Item -ItemType Directory -Force $logDir | Out-Null
$results = New-Object System.Collections.ArrayList
function Check($name,$ok,$detail=""){ [void]$results.Add([pscustomobject]@{Check=$name;Result=$(if($ok){"PASS"}else{"FAIL"});Detail=$detail}); Write-Host ("[{0}] {1} {2}" -f $(if($ok){"PASS"}else{"FAIL"}),$name,$detail) }
function Msi($argline,$log){ (Start-Process msiexec -ArgumentList "$argline /qn /norestart /l*v `"$logDir\$log`"" -Wait -PassThru).ExitCode }
function DesktopLnks(){ @("$env:Public\Desktop","$env:USERPROFILE\Desktop") | ForEach-Object { Get-ChildItem $_ -Filter *.lnk -EA SilentlyContinue } | Where-Object { $_.Name -match 'livecam|HLS' } }
function ArpEntries(){ @("HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*","HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*") | ForEach-Object { Get-ItemProperty $_ -EA SilentlyContinue } | Where-Object { $_.DisplayName -like "HLS Livecam*" } }

# ---- elevation gate ----
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)) {
  Write-Host "ERROR: must run in an ELEVATED PowerShell (Run as administrator)." -ForegroundColor Red; exit 1
}
foreach($m in @($Msi100,$Msi101)){ if(-not (Test-Path $m)){ Write-Host "ERROR: missing $m" -ForegroundColor Red; exit 1 } }

# ---- back up real config; ensure a cams.json marker so preserve/purge is meaningful ----
$backup = "$env:TEMP\hls-cfg-backup-$PID"
$hadConfig = Test-Path $ConfigDir
if ($hadConfig) { Copy-Item $ConfigDir $backup -Recurse -Force }
if (-not (Test-Path "$ConfigDir\cams.json")) { New-Item -ItemType Directory -Force $ConfigDir | Out-Null; '{"cams":[]}' | Set-Content "$ConfigDir\cams.json" }
$camsMarker = (Get-FileHash "$ConfigDir\cams.json").Hash

try {
  # ---- clean any dev/prior state ----
  Get-Process hls-livecam-win -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue
  schtasks /Delete /TN hls-livecam-win /F 2>$null | Out-Null
  if (ArpEntries) { Msi "/x `"$Msi100`"" "pre-clean.log" | Out-Null }

  # ================= TEST 1: install v1.0.0 =================
  Write-Host "`n=== TEST 1: install v1.0.0 ===" -ForegroundColor Cyan
  $rc = Msi "/i `"$Msi100`"" "install-100.log"
  Check "install v1.0.0 exit 0" ($rc -eq 0) "rc=$rc"
  Check "exe installed"      (Test-Path "$InstallDir\hls-livecam-win.exe")
  Check "bin\ffmpeg.exe"     (Test-Path "$InstallDir\bin\ffmpeg.exe")
  Check "bin\mediamtx.exe"   (Test-Path "$InstallDir\bin\mediamtx.exe")
  Check "icon.ico"           (Test-Path "$InstallDir\icon.ico")
  $task = Get-ScheduledTask -TaskName hls-livecam-win -EA SilentlyContinue
  Check "scheduled task exists" ($null -ne $task)
  Check "task RunLevel=Highest" ($task.Principal.RunLevel -eq 'Highest') "RunLevel=$($task.Principal.RunLevel)"
  $logon = $task.Triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' }
  Check "task trigger=ONLOGON" ($null -ne $logon)
  $tline = schtasks /Query /TN hls-livecam-win /V /FO LIST 2>$null | Select-String 'Task To Run:'
  $target = if ($tline) { $tline.ToString().Trim() } else { "" }
  Check "task targets installed exe" ($target -match [regex]::Escape("$InstallDir\hls-livecam-win.exe")) $target
  Check "Start-menu shortcut" (Test-Path $StartLnk)
  $dtl = DesktopLnks
  Check "NO desktop shortcut" ($null -eq $dtl) $(if($dtl){"found: $($dtl.Name)"}else{"none"})
  $arp = ArpEntries
  Check "ARP entry present" ($null -ne $arp) "$($arp.DisplayName) $($arp.DisplayVersion)"

  # ================= TEST 2: default uninstall (config KEPT) =================
  Write-Host "`n=== TEST 2: default uninstall ===" -ForegroundColor Cyan
  $rc = Msi "/x `"$Msi100`"" "uninstall-default.log"
  Check "uninstall exit 0" ($rc -eq 0) "rc=$rc"
  Check "install dir removed" (-not (Test-Path $InstallDir))
  Check "task removed" ($null -eq (Get-ScheduledTask -TaskName hls-livecam-win -EA SilentlyContinue))
  Check "Start-menu entry removed" (-not (Test-Path $StartLnk))
  Check "no orphaned process" ($null -eq (Get-Process hls-livecam-win -EA SilentlyContinue))
  Check "config PRESERVED (dir)" (Test-Path $ConfigDir)
  Check "config PRESERVED (cams.json unchanged)" ((Test-Path "$ConfigDir\cams.json") -and (Get-FileHash "$ConfigDir\cams.json").Hash -eq $camsMarker)

  # ================= TEST 3: purge uninstall (config GONE) =================
  Write-Host "`n=== TEST 3: reinstall + purge uninstall ===" -ForegroundColor Cyan
  Msi "/i `"$Msi100`"" "install-forpurge.log" | Out-Null
  Check "config still present before purge" (Test-Path "$ConfigDir\cams.json")
  $rc = Msi "/x `"$Msi100`" PURGECONFIG=1" "uninstall-purge.log"
  Check "purge uninstall exit 0" ($rc -eq 0) "rc=$rc"
  Check "config REMOVED by purge" (-not (Test-Path $ConfigDir))
  Check "install dir removed (purge)" (-not (Test-Path $InstallDir))

  # ================= TEST 4: upgrade over RUNNING app =================
  Write-Host "`n=== TEST 4: upgrade v1.0.0 -> v1.0.1 with app running (camera may blink) ===" -ForegroundColor Cyan
  Msi "/i `"$Msi100`"" "install-forupgrade.log" | Out-Null
  $proc = Start-Process "$InstallDir\hls-livecam-win.exe" -PassThru
  Start-Sleep 6
  $running = Get-Process -Id $proc.Id -EA SilentlyContinue
  Check "app launches from installed location" ($null -ne $running) "pid=$($proc.Id)"
  $rc = Msi "/i `"$Msi101`"" "upgrade-101.log"
  Check "upgrade install exit 0" ($rc -eq 0) "rc=$rc"
  Start-Sleep 2
  Check "old instance terminated by upgrade" ($null -eq (Get-Process -Id $proc.Id -EA SilentlyContinue))
  $arp = @(ArpEntries)
  Check "single product (no side-by-side)" ($arp.Count -eq 1) "count=$($arp.Count)"
  Check "version bumped to 1.0.1" ($arp.DisplayVersion -eq '1.0.1') "ver=$($arp.DisplayVersion)"
  Check "exe present after upgrade" (Test-Path "$InstallDir\hls-livecam-win.exe")
  # clean up: remove the upgrade-installed product, leave machine clean
  Get-Process hls-livecam-win -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue
  Msi "/x `"$Msi101`"" "cleanup.log" | Out-Null
  Check "final cleanup uninstalled" (-not (Test-Path $InstallDir))
}
finally {
  # ---- restore real config ----
  Remove-Item $ConfigDir -Recurse -Force -EA SilentlyContinue
  if ($hadConfig) { Copy-Item $backup $ConfigDir -Recurse -Force; Remove-Item $backup -Recurse -Force -EA SilentlyContinue; Write-Host "`nRestored your original %APPDATA%\hls-livecam-win." -ForegroundColor Green }
  else { Write-Host "`n(no original config existed; test marker removed)" }

  Write-Host "`n================= SUMMARY =================" -ForegroundColor Cyan
  $results | Format-Table -AutoSize
  $fail = ($results | Where-Object Result -eq 'FAIL').Count
  Write-Host ("{0} checks, {1} FAILED. Logs: {2}" -f $results.Count,$fail,$logDir) -ForegroundColor $(if($fail){"Red"}else{"Green"})
}
