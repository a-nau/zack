# Uninstall zack + kanata on Windows: undo everything scripts\install\windows.ps1 set up.
#
# Removes, in order: the background processes, the wake-from-sleep scheduled task,
# the HKCU autostart Run keys, and the binaries/configs that were copied under
# %LOCALAPPDATA% and %APPDATA%.
#
# It DELIBERATELY never touches your repo checkout: in repo mode the live configs
# are launcher\mappings.json and kanata\kanata.kbd inside the repo, and those are
# your source files - this script only removes the AppData *pointer* to them, not
# the repo. Only the fixed, installer-owned AppData/LocalAppData paths are deleted.
param(
    # Skip the confirmation prompt (for scripted/unattended removal).
    [switch]$Force
)
$ErrorActionPreference = "Stop"

if (-not $Force) {
    Write-Host "This removes zack + kanata: processes, autostart, the wake-restart task,"
    Write-Host "and the copies under %LOCALAPPDATA%\{zack,kanata} and %APPDATA%\{zack,kanata}."
    Write-Host "Your repo checkout (mappings.json / kanata.kbd) is NOT touched."
    $ans = Read-Host "Proceed? [y/N]"
    if ($ans -notmatch '^(y|yes)$') { Write-Host "Aborted - nothing was changed."; return }
}

# 1. Stop the background processes (both kanata binary names + the legacy name).
Get-Process -Name zack, capsule, kanata, kanata_gui -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300

# 2. Remove the wake-from-sleep scheduled task (ignore if it isn't there).
try {
    Unregister-ScheduledTask -TaskName "zack-wake-restart" -Confirm:$false -ErrorAction Stop
    Write-Host "Removed scheduled task: zack-wake-restart"
} catch {
    Write-Host "No wake-restart task to remove."
}

# 3. Remove HKCU autostart entries (current names + legacy 'capsule').
$Run = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
foreach ($name in @("zack", "kanata", "capsule")) {
    Remove-ItemProperty -Path $Run -Name $name -ErrorAction SilentlyContinue
}
Write-Host "Removed autostart Run keys."

# 4. Remove ONLY the specific files this installer creates, then remove each
#    directory only if it is now empty. This deliberately preserves anything the
#    installer does not own - e.g. a pre-existing kanata install (PATH-fallback
#    mode never copies kanata here) or a kanata.kbd / mappings.json you keep in
#    %APPDATA%. Wholesale recursive deletion could destroy that unrelated data.
$Files = @(
    (Join-Path $env:LOCALAPPDATA "zack\zack.exe"),
    (Join-Path $env:LOCALAPPDATA "zack\wake-restart.ps1"),
    (Join-Path $env:LOCALAPPDATA "zack\wake-restart.cmd"),   # legacy name from an older installer
    (Join-Path $env:LOCALAPPDATA "kanata\kanata_gui.exe"),
    (Join-Path $env:APPDATA      "zack\config-path.txt"),
    (Join-Path $env:APPDATA      "zack\zack.log")
)
foreach ($f in $Files) {
    if (Test-Path -LiteralPath $f) {
        Remove-Item -LiteralPath $f -Force -ErrorAction SilentlyContinue
        if (-not (Test-Path -LiteralPath $f)) { Write-Host "Removed $f" }
        else { Write-Warning "Could not remove $f (in use?). Remove it manually." }
    }
}

# Remove the installer's AppData dirs only when empty; otherwise keep + report them
# so we never delete files the installer did not create.
$Dirs = @(
    (Join-Path $env:LOCALAPPDATA "zack"),
    (Join-Path $env:LOCALAPPDATA "kanata"),
    (Join-Path $env:APPDATA      "zack"),
    (Join-Path $env:APPDATA      "kanata")
)
foreach ($dir in $Dirs) {
    if (Test-Path -LiteralPath $dir) {
        $remaining = @(Get-ChildItem -LiteralPath $dir -Force -ErrorAction SilentlyContinue)
        if ($remaining.Count -eq 0) {
            Remove-Item -LiteralPath $dir -Force -ErrorAction SilentlyContinue
            Write-Host "Removed empty dir $dir"
        } else {
            Write-Host "Kept $dir ($($remaining.Count) file(s) not created by the installer)."
        }
    }
}

Write-Host ""
Write-Host "Uninstall complete. Your repo checkout was not touched."
Write-Host "(Repo mode: launcher\mappings.json and kanata\kanata.kbd remain as your source configs.)"
