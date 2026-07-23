<#
  Runtime verification for win-v1.0.1 (camdash.exe rename + static CRT).
  RUN ELEVATED (admin PowerShell).

    powershell -ExecutionPolicy Bypass -File verify-msi.ps1

  Matrix:
    TEST 1  fresh install v1.0.1 -> camdash.exe in place, bin\ resolved,
            ONLOGON/Highest task targeting camdash.exe, Start-menu shortcut,
            NO desktop shortcut, ARP entry
    TEST 2  default uninstall -> everything gone, %APPDATA%\hls-livecam-win KEPT
    TEST 3  purge uninstall (PURGECONFIG=1) -> %APPDATA%\hls-livecam-win GONE
    TEST 4  *** UPGRADE ACROSS THE RENAME *** v1.0.0 (hls-livecam-win.exe)
            -> v1.0.1 (camdash.exe) WITH THE OLD APP RUNNING:
              - old hls-livecam-win.exe GONE (not orphaned beside the new one)
              - camdash.exe present
              - Scheduled Task RETARGETED to camdash.exe (a task pointing at a
                deleted exe is a silent failure that only shows at next logon)
              - exactly ONE task (not one per name)
              - single ARP product, version 1.0.1, no side-by-side
              - old instance terminated; app runs after upgrade
              - config preserved across the upgrade
              - autostart.rs self-registration CONVERGES (launching the new exe
                must not add a second task or re-point it elsewhere)

  SAFETY: %APPDATA%\hls-livecam-win is backed up first and restored at the end,
  even on error. The upgrade test launches the app briefly (camera may blink).
  Ends with the machine clean (no product installed).
#>
param(
  [string]$MsiOld = "$PSScriptRoot\build\hls-livecam-win-1.0.0.msi",   # contains hls-livecam-win.exe
  [string]$MsiNew = "$PSScriptRoot\build\hls-livecam-win-1.0.1.msi"    # contains camdash.exe
)
$ErrorActionPreference = "Continue"
$InstallDir = "$env:ProgramFiles\hls-livecam-win"      # install dir intentionally unchanged
$OldExe     = "$InstallDir\hls-livecam-win.exe"
$NewExe     = "$InstallDir\camdash.exe"
$ConfigDir  = "$env:APPDATA\hls-livecam-win"           # config dir intentionally unchanged
$StartLnk   = "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\HLS Livecam\HLS Livecam.lnk"
$TaskName   = "hls-livecam-win"                        # task name intentionally unchanged
$logDir     = "$PSScriptRoot\build\verify-logs"
New-Item -ItemType Directory -Force $logDir | Out-Null
$results = New-Object System.Collections.ArrayList
function Check($name,$ok,$detail=""){ [void]$results.Add([pscustomobject]@{Check=$name;Result=$(if($ok){"PASS"}else{"FAIL"});Detail=$detail}); Write-Host ("[{0}] {1} {2}" -f $(if($ok){"PASS"}else{"FAIL"}),$name,$detail) }
function Msi($argline,$log){ (Start-Process msiexec -ArgumentList "$argline /qn /norestart /l*v `"$logDir\$log`"" -Wait -PassThru).ExitCode }
function DesktopLnks(){ @("$env:Public\Desktop","$env:USERPROFILE\Desktop") | ForEach-Object { Get-ChildItem $_ -Filter *.lnk -EA SilentlyContinue } | Where-Object { $_.Name -match 'livecam|HLS|camdash' } }
function ArpEntries(){ @("HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*","HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*") | ForEach-Object { Get-ItemProperty $_ -EA SilentlyContinue } | Where-Object { $_.DisplayName -like "HLS Livecam*" } }
function TaskTarget(){ $l = schtasks /Query /TN $TaskName /V /FO LIST 2>$null | Select-String 'Task To Run:'; if($l){ $l.ToString().Trim() } else { "" } }
function RelatedTasks(){ @(Get-ScheduledTask -EA SilentlyContinue | Where-Object { $_.TaskName -match 'livecam|camdash' }) }
function KillAll(){ Get-Process camdash,hls-livecam-win -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue }

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)) {
  Write-Host "ERROR: must run in an ELEVATED PowerShell (Run as administrator)." -ForegroundColor Red; exit 1
}
foreach($m in @($MsiOld,$MsiNew)){ if(-not (Test-Path $m)){ Write-Host "ERROR: missing $m" -ForegroundColor Red; exit 1 } }

$backup = "$env:TEMP\hls-cfg-backup-$PID"
$hadConfig = Test-Path $ConfigDir
if ($hadConfig) { Copy-Item $ConfigDir $backup -Recurse -Force }
if (-not (Test-Path "$ConfigDir\cams.json")) { New-Item -ItemType Directory -Force $ConfigDir | Out-Null; '{"cams":[]}' | Set-Content "$ConfigDir\cams.json" }
$camsMarker = (Get-FileHash "$ConfigDir\cams.json").Hash

try {
  KillAll
  schtasks /Delete /TN $TaskName /F 2>$null | Out-Null
  foreach($m in @($MsiOld,$MsiNew)){ if (ArpEntries) { Msi "/x `"$m`"" "pre-clean.log" | Out-Null } }

  # ================= TEST 1: fresh install v1.0.1 =================
  Write-Host "`n=== TEST 1: fresh install v1.0.1 (camdash.exe) ===" -ForegroundColor Cyan
  $rc = Msi "/i `"$MsiNew`"" "install-101.log"
  Check "install v1.0.1 exit 0" ($rc -eq 0) "rc=$rc"
  Check "camdash.exe installed"  (Test-Path $NewExe)
  Check "old exe NOT present"    (-not (Test-Path $OldExe))
  Check "bin\ffmpeg.exe"         (Test-Path "$InstallDir\bin\ffmpeg.exe")
  Check "bin\mediamtx.exe"       (Test-Path "$InstallDir\bin\mediamtx.exe")
  Check "icon.ico"               (Test-Path "$InstallDir\icon.ico")
  $task = Get-ScheduledTask -TaskName $TaskName -EA SilentlyContinue
  Check "scheduled task exists"  ($null -ne $task)
  Check "task RunLevel=Highest"  ($task.Principal.RunLevel -eq 'Highest') "RunLevel=$($task.Principal.RunLevel)"
  Check "task trigger=ONLOGON"   ($null -ne ($task.Triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' }))
  $t = TaskTarget
  Check "task targets camdash.exe" ($t -match [regex]::Escape($NewExe)) $t
  Check "Start-menu shortcut"    (Test-Path $StartLnk)
  $dtl = DesktopLnks
  Check "NO desktop shortcut"    ($null -eq $dtl) $(if($dtl){"found: $($dtl.Name)"}else{"none"})
  $arp = ArpEntries
  Check "ARP entry present"      ($null -ne $arp) "$($arp.DisplayName) $($arp.DisplayVersion)"

  # ================= TEST 2: default uninstall =================
  Write-Host "`n=== TEST 2: default uninstall (config KEPT) ===" -ForegroundColor Cyan
  $rc = Msi "/x `"$MsiNew`"" "uninstall-default.log"
  Check "uninstall exit 0" ($rc -eq 0) "rc=$rc"
  Check "install dir removed" (-not (Test-Path $InstallDir))
  Check "task removed" ($null -eq (Get-ScheduledTask -TaskName $TaskName -EA SilentlyContinue))
  Check "Start-menu entry removed" (-not (Test-Path $StartLnk))
  Check "no orphaned process" ($null -eq (Get-Process camdash,hls-livecam-win -EA SilentlyContinue))
  Check "config PRESERVED (dir)" (Test-Path $ConfigDir)
  Check "config PRESERVED (cams.json unchanged)" ((Test-Path "$ConfigDir\cams.json") -and (Get-FileHash "$ConfigDir\cams.json").Hash -eq $camsMarker)

  # ================= TEST 3: purge uninstall =================
  Write-Host "`n=== TEST 3: reinstall + purge uninstall ===" -ForegroundColor Cyan
  Msi "/i `"$MsiNew`"" "install-forpurge.log" | Out-Null
  Check "config present before purge" (Test-Path "$ConfigDir\cams.json")
  $rc = Msi "/x `"$MsiNew`" PURGECONFIG=1" "uninstall-purge.log"
  Check "purge uninstall exit 0" ($rc -eq 0) "rc=$rc"
  Check "config REMOVED by purge" (-not (Test-Path $ConfigDir))
  Check "install dir removed (purge)" (-not (Test-Path $InstallDir))
  # restore a config so the upgrade test can prove preservation
  New-Item -ItemType Directory -Force $ConfigDir | Out-Null
  '{"cams":[]}' | Set-Content "$ConfigDir\cams.json"
  $camsMarker = (Get-FileHash "$ConfigDir\cams.json").Hash

  # ========== TEST 4: UPGRADE ACROSS THE RENAME (the critical one) ==========
  Write-Host "`n=== TEST 4: upgrade v1.0.0 (hls-livecam-win.exe) -> v1.0.1 (camdash.exe), app RUNNING ===" -ForegroundColor Cyan
  $rc = Msi "/i `"$MsiOld`"" "install-100-forupgrade.log"
  Check "v1.0.0 baseline installed" ($rc -eq 0 -and (Test-Path $OldExe)) "rc=$rc"
  $t0 = TaskTarget
  Check "baseline task targets OLD exe" ($t0 -match [regex]::Escape($OldExe)) $t0
  $proc = Start-Process $OldExe -PassThru
  Start-Sleep 6
  Check "old app running (holds file lock)" ($null -ne (Get-Process -Id $proc.Id -EA SilentlyContinue)) "pid=$($proc.Id)"

  $rc = Msi "/i `"$MsiNew`"" "upgrade-across-rename.log"
  Check "upgrade exit 0" ($rc -eq 0) "rc=$rc"
  Start-Sleep 3
  Check ">>> OLD hls-livecam-win.exe GONE" (-not (Test-Path $OldExe))
  Check ">>> NEW camdash.exe present" (Test-Path $NewExe)
  $t1 = TaskTarget
  Check ">>> task RETARGETED to camdash.exe" ($t1 -match [regex]::Escape($NewExe)) $t1
  $rt = RelatedTasks
  Check ">>> exactly ONE task (no duplicate)" ($rt.Count -eq 1) "count=$($rt.Count): $($rt.TaskName -join ',')"
  Check "old instance terminated" ($null -eq (Get-Process -Id $proc.Id -EA SilentlyContinue))
  $arp = @(ArpEntries)
  Check "single product (no side-by-side)" ($arp.Count -eq 1) "count=$($arp.Count)"
  Check "version is 1.0.1" ($arp.DisplayVersion -eq '1.0.1') "ver=$($arp.DisplayVersion)"
  Check "config preserved across upgrade" ((Test-Path "$ConfigDir\cams.json") -and (Get-FileHash "$ConfigDir\cams.json").Hash -eq $camsMarker)

  # app runs after upgrade + autostart self-registration must CONVERGE
  $p2 = Start-Process $NewExe -PassThru
  Start-Sleep 8
  Check "app runs after upgrade (camdash.exe)" ($null -ne (Get-Process -Id $p2.Id -EA SilentlyContinue)) "pid=$($p2.Id)"
  $rt2 = RelatedTasks
  $t2 = TaskTarget
  Check ">>> autostart converges: still ONE task" ($rt2.Count -eq 1) "count=$($rt2.Count): $($rt2.TaskName -join ',')"
  Check ">>> autostart converges: still targets camdash.exe" ($t2 -match [regex]::Escape($NewExe)) $t2
  KillAll; Start-Sleep 2
  Msi "/x `"$MsiNew`"" "cleanup.log" | Out-Null
  Check "final cleanup uninstalled" (-not (Test-Path $InstallDir))
}
finally {
  KillAll
  Remove-Item $ConfigDir -Recurse -Force -EA SilentlyContinue
  if ($hadConfig) { Copy-Item $backup $ConfigDir -Recurse -Force; Remove-Item $backup -Recurse -Force -EA SilentlyContinue; Write-Host "`nRestored your original %APPDATA%\hls-livecam-win." -ForegroundColor Green }
  else { Write-Host "`n(no original config existed; test marker removed)" }
  Write-Host "`n================= SUMMARY =================" -ForegroundColor Cyan
  $results | Format-Table -AutoSize
  $fail = ($results | Where-Object Result -eq 'FAIL').Count
  Write-Host ("{0} checks, {1} FAILED. Logs: {2}" -f $results.Count,$fail,$logDir) -ForegroundColor $(if($fail){"Red"}else{"Green"})
}
