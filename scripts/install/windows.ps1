# Install zack + kanata on Windows: CapsLock remapping (kanata) and
# background autostart at logon for both (HKCU Run keys). No admin needed.
#
# Two layouts:
#   1. REPO checkout (preferred): run scripts\install\windows.ps1 from a git clone.
#      The binary comes from release\, and the LIVE config is the repo's
#      launcher\mappings.json (via a pointer file) — edit, 'rel', commit.
#      The 'update' command then pulls, builds/tests in Docker, and reinstalls.
#   2. Flat bundle: zack.exe, mappings.json, kanata.kbd, kanata_gui.exe
#      next to this script; configs are COPIED to %APPDATA% (only if absent;
#      -ForceConfig overwrites with timestamped backups).
param(
    # Bundle mode only: overwrite existing %APPDATA% configs (backup kept).
    [switch]$ForceConfig,
    # Accepted for compatibility with the self-updater; no prompts exist today.
    [switch]$NoPrompt
)
$ErrorActionPreference = "Stop"

# Start a process that SURVIVES closing this shell: Windows Terminal puts
# Start-Process children in its job object and kills them with the tab;
# WMI-created processes live outside that job.
function Start-Detached([string]$CommandLine) {
    $result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = $CommandLine }
    if ($result.ReturnValue -ne 0) { Write-Warning "failed to start: $CommandLine (code $($result.ReturnValue))" }
}

# --- locate sources ----------------------------------------------------------
$BundleMode = Test-Path -LiteralPath (Join-Path $PSScriptRoot "zack.exe")
if ($BundleMode) {
    $SrcBin    = Join-Path $PSScriptRoot "zack.exe"
    $SrcMap    = Join-Path $PSScriptRoot "mappings.json"
    $SrcKbd    = Join-Path $PSScriptRoot "kanata.kbd"
    $SrcKanata = Join-Path $PSScriptRoot "kanata_gui.exe"
} else {
    $Repo      = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    # Prefer a locally built binary; fall back to the committed zip.
    $SrcDist = Join-Path $Repo "dist\zack-windows-x86_64.exe"
    $SrcZip  = Join-Path $Repo "release\zack-windows-x86_64.zip"
    if (Test-Path -LiteralPath $SrcDist) {
        $SrcBin = $SrcDist
    } elseif (Test-Path -LiteralPath $SrcZip) {
        $Extract = Join-Path $env:TEMP "zack-install-bin"
        Remove-Item -Recurse -Force $Extract -ErrorAction SilentlyContinue
        Expand-Archive -LiteralPath $SrcZip -DestinationPath $Extract -Force
        $SrcBin = Join-Path $Extract "zack-windows-x86_64.exe"
    } else {
        throw "no binary found - run scripts\build-in-container.ps1 first"
    }
    $SrcMap    = Join-Path $Repo "launcher\mappings.json"
    $SrcKbd    = Join-Path $Repo "kanata\kanata.kbd"
    $SrcKanata = Join-Path $Repo "release\kanata_gui.exe"
}

# Preflight EVERY required source before touching processes or files.
foreach ($src in @($SrcBin, $SrcMap, $SrcKbd)) {
    if (-not (Test-Path -LiteralPath $src)) { throw "missing $src - incomplete bundle/checkout" }
}

# --- stop running instances BEFORE replacing binaries (Windows locks
# --- loaded executables; both kanata binary names are covered) --------------
Get-Process -Name zack, capsule, kanata, kanata_gui -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300

# One-time migration from the old "capsule" name: drop its autostart entry.
Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "capsule" -ErrorAction SilentlyContinue

# --- zack binary ----------------------------------------------------------
$ZackDir = Join-Path $env:LOCALAPPDATA "zack"
New-Item -ItemType Directory -Force -Path $ZackDir | Out-Null
Copy-Item -LiteralPath $SrcBin -Destination (Join-Path $ZackDir "zack.exe") -Force

# --- kanata binary -----------------------------------------------------------
$KanataExe = ""
if (Test-Path -LiteralPath $SrcKanata) {
    $KanataDir = Join-Path $env:LOCALAPPDATA "kanata"
    New-Item -ItemType Directory -Force -Path $KanataDir | Out-Null
    Copy-Item -LiteralPath $SrcKanata -Destination (Join-Path $KanataDir "kanata_gui.exe") -Force
    $KanataExe = Join-Path $KanataDir "kanata_gui.exe"
} else {
    $cmd = Get-Command kanata_gui.exe, kanata.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) { $KanataExe = $cmd.Source }
}

# --- configs -------------------------------------------------------------------
$CfgDir = Join-Path $env:APPDATA "zack"
$KanataCfgDir = Join-Path $env:APPDATA "kanata"
New-Item -ItemType Directory -Force -Path $CfgDir, $KanataCfgDir | Out-Null
$Pointer = Join-Path $CfgDir "config-path.txt"

if ($BundleMode) {
    Remove-Item -LiteralPath $Pointer -Force -ErrorAction SilentlyContinue
    $MapDst = Join-Path $CfgDir "mappings.json"
    $KbdDst = Join-Path $KanataCfgDir "kanata.kbd"
    if ($ForceConfig) {
        $Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
        foreach ($dst in @($MapDst, $KbdDst)) {
            if (Test-Path -LiteralPath $dst) { Copy-Item -LiteralPath $dst -Destination "$dst.$Stamp.bak" -Force }
        }
        Copy-Item -LiteralPath $SrcMap -Destination $MapDst -Force
        Copy-Item -LiteralPath $SrcKbd -Destination $KbdDst -Force
        Write-Host "Configs overwritten (-ForceConfig); previous versions kept with timestamps."
    } else {
        if (-not (Test-Path -LiteralPath $MapDst)) { Copy-Item -LiteralPath $SrcMap -Destination $CfgDir }
        if (-not (Test-Path -LiteralPath $KbdDst)) { Copy-Item -LiteralPath $SrcKbd -Destination $KanataCfgDir }
    }
    $KanataCfg = $KbdDst
} else {
    # REPO mode: the repo files ARE the live configs. zack reads
    # mappings.json through the pointer; kanata gets the repo path directly.
    Set-Content -LiteralPath $Pointer -Value $SrcMap -NoNewline
    Write-Host "Live config: $SrcMap  (edit + 'rel'; commit to sync machines)"
    $KanataCfg = $SrcKbd
}

# --- autostart at logon (quoted absolute paths) --------------------------------
$Run = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
Set-ItemProperty -Path $Run -Name "zack" -Value ('"' + (Join-Path $ZackDir "zack.exe") + '"')
if ($KanataExe) {
    Set-ItemProperty -Path $Run -Name "kanata" -Value ('"' + $KanataExe + '" -c "' + $KanataCfg + '"')
    Start-Detached ('"' + $KanataExe + '" -c "' + $KanataCfg + '"')
    Write-Host "kanata installed + autostart registered: $KanataExe"
} else {
    Write-Warning "kanata_gui.exe not found (release\, bundle, or PATH) - CapsLock stays unmapped."
}

Start-Detached ('"' + (Join-Path $ZackDir "zack.exe") + '"')

# --- logon + wake-from-sleep recovery -----------------------------------------
# On Windows resume, kanata's low-level keyboard hook silently dies (the process
# keeps running but stops remapping CapsLock) and zack's eframe/OpenGL context is
# lost, crashing it. The HKCU Run keys above only fire at logon, so neither comes
# back after a mere sleep/wake. Register a scheduled task that fires on the resume
# events in the System log (including Modern Standby) or on session unlock and
# restarts both. kanata is killed + relaunched unconditionally: it's the HOOK
# that died, not the process, so "start if missing" would not fix it.
$ZackExe = Join-Path $ZackDir "zack.exe"
# The recovery logic runs as a PowerShell -EncodedCommand (built here, attached to
# the task action below), NOT a script file. Rationale: a .ps1 file is subject to
# execution policy - Group Policy Restricted/AllSigned would silently block it on a
# locked-down machine even with process-level -ExecutionPolicy Bypass - and a .cmd is
# read in the console OEM code page (mangling non-ASCII paths). -EncodedCommand is
# exempt from execution policy and carries the whole command as base64 UTF-16, so it
# is robust on both counts. It kills BY PROCESS NAME (covers kanata_gui.exe AND the
# PATH-fallback kanata.exe), settles, then relaunches via WMI Win32_Process.Create -
# NOT Start-Process. WMI-created processes inherit no console and have no shell parent,
# so (a) they reliably re-acquire kanata's low-level keyboard hook and (b) they are
# immune to a closing terminal. This mirrors Start-Detached used earlier in the file.
# Each CommandLine is a single-quoted PS literal (single quotes doubled); the exe and
# config paths are wrapped in embedded double quotes for the child's own arg parsing
# ('"' is illegal in Windows paths, so double-quoting is always safe).
$KCmd = "'`"" + ($KanataExe -replace "'", "''") + "`" -c `"" + ($KanataCfg -replace "'", "''") + "`"'"
$ZCmd = "'`"" + ($ZackExe -replace "'", "''") + "`"'"
$WakeLines = @(
    'Stop-Process -Name kanata_gui, kanata, zack -Force -ErrorAction SilentlyContinue',
    'Start-Sleep -Seconds 2'
)
if ($KanataExe) { $WakeLines += "Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = $KCmd } | Out-Null" }
$WakeLines += "Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = $ZCmd } | Out-Null"
$WakeEnc = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes(($WakeLines -join "`n")))

# Register via the ScheduledTasks module (not schtasks.exe): the action is passed
# through COM with NO shell quoting, so it stays correct even if the profile path
# contains spaces. The event trigger handles classic S3 resume (Kernel-Power 107),
# Modern Standby exit (Kernel-Power 507), and Power-Troubleshooter 1 as a fallback.
# A separate session-unlock trigger repairs zack after lock/unlock even when Windows
# never enters a sleep state. -AllowStartIfOnBatteries /
# -DontStopIfGoingOnBatteries so it also fires when a laptop wakes on battery (Task
# Scheduler skips battery starts by default). Register-ScheduledTask throws on
# failure, which we catch so a task problem never aborts the install. The logon
# trigger is delayed so the HKCU Run entries remain a fast path when Windows runs
# them, while the task reliably repairs startup when Windows skips those entries.
$WakeSub = '<QueryList><Query Id="0" Path="System"><Select Path="System">*[System[(Provider[@Name=''Microsoft-Windows-Kernel-Power''] and (EventID=107 or EventID=507)) or (Provider[@Name=''Microsoft-Windows-Power-Troubleshooter''] and EventID=1)]]</Select></Query></QueryList>'
try {
    # -EncodedCommand (base64 UTF-16) is exempt from execution policy and Unicode-safe;
    # -WindowStyle Hidden avoids a console flash on wake.
    $WakeArg       = "-NoProfile -WindowStyle Hidden -EncodedCommand $WakeEnc"
    $WakeAction    = New-ScheduledTaskAction -Execute "powershell.exe" -Argument $WakeArg
    $WakeTrigClass = Get-CimClass -Namespace Root/Microsoft/Windows/TaskScheduler -ClassName MSFT_TaskEventTrigger -ErrorAction Stop
    $WakeTrigger   = New-CimInstance -CimClass $WakeTrigClass -ClientOnly
    $WakeTrigger.Enabled      = $true
    $WakeTrigger.Subscription = $WakeSub
    $WakeUser      = ([Security.Principal.WindowsIdentity]::GetCurrent()).Name
    $UnlockTrigClass = Get-CimClass -Namespace Root/Microsoft/Windows/TaskScheduler -ClassName MSFT_TaskSessionStateChangeTrigger -ErrorAction Stop
    $UnlockTrigger   = New-CimInstance -CimClass $UnlockTrigClass -ClientOnly
    $UnlockTrigger.Enabled     = $true
    $UnlockTrigger.StateChange = 8 # TASK_SESSION_STATE_CHANGE_TYPE.SessionUnlock
    $UnlockTrigger.UserId      = $WakeUser
    $LogonTrigger  = New-ScheduledTaskTrigger -AtLogOn -User $WakeUser
    $LogonTrigger.Delay = "PT10S"
    $WakePrincipal = New-ScheduledTaskPrincipal -UserId $WakeUser -LogonType Interactive -RunLevel Limited
    $WakeSettings  = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew `
        -ExecutionTimeLimit (New-TimeSpan -Minutes 1) `
        -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
    Register-ScheduledTask -TaskName "zack-wake-restart" -Action $WakeAction -Trigger @($LogonTrigger, $WakeTrigger, $UnlockTrigger) `
        -Principal $WakePrincipal -Settings $WakeSettings -Force -ErrorAction Stop | Out-Null
    Write-Host "Logon + wake/unlock recovery task registered (zack-wake-restart)."
} catch {
    Write-Warning "Could not register logon/wake restart task: $($_.Exception.Message)"
    Write-Warning "Logon and sleep/wake may need a manual restart (re-run scripts\install\windows.ps1 to retry)."
}

# --- `zack` restart alias on PATH ---------------------------------------------
# Convenience command: typing `zack` in any shell (re)starts kanata + zack by
# running the wake task (WMI-detached, no console) so they survive window closes.
# It lives in its OWN dir - NOT $ZackDir, which holds zack.exe: PATHEXT resolves
# .exe before .cmd, so a `zack` on PATH next to zack.exe would launch the daemon
# instead. The alias dir is appended to the user PATH (idempotent).
$ZackBinDir = Join-Path $ZackDir "bin"
New-Item -ItemType Directory -Force -Path $ZackBinDir | Out-Null
$AliasLines = @(
    '@echo off',
    'REM Auto-generated by scripts\install\windows.ps1. `zack` = restart kanata + zack via the',
    'REM wake-restart task (WMI-detached, no console) so they survive closing a window.',
    'schtasks /Run /TN "zack-wake-restart" >nul 2>&1',
    'if errorlevel 1 (echo [zack] wake-restart task missing - re-run scripts\install\windows.ps1) else (echo [zack] restarting kanata + zack ... ~3s)'
)
Set-Content -LiteralPath (Join-Path $ZackBinDir "zack.cmd") -Value $AliasLines -Encoding Oem
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$ZackBinDir*") {
    $NewPath = if ($UserPath) { "$UserPath;$ZackBinDir" } else { $ZackBinDir }
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    Write-Host "'zack' alias installed; added $ZackBinDir to PATH (open a NEW terminal to use it)."
} else {
    Write-Host "'zack' alias installed ($ZackBinDir already on PATH)."
}

Write-Host ""
Write-Host "=============================================================="
Write-Host " Install complete."
Write-Host ""
Write-Host " Both programs are now running detached in the background and"
Write-Host " will start automatically at every logon."
Write-Host ""
Write-Host " >> You can safely CLOSE THIS WINDOW. <<"
Write-Host ""
Write-Host " Try it:   double-tap CapsLock     = Esc (single-tap does nothing)"
Write-Host "           hold CapsLock + Space   = launcher"
Write-Host "           type 'update' in it     = git pull + Docker build/test + reinstall (repo mode)"
Write-Host "           type '?' in it          = all key bindings"
Write-Host ""
Write-Host " Log: $CfgDir\zack.log   (hotkeys don't reach admin windows)"
Write-Host "=============================================================="
