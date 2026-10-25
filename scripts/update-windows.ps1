# Update a repo-mode installation in a separate console after Zack exits.
param(
    [Parameter(Mandatory = $true)][string]$Repo,
    [int]$LauncherPid = 0,
    [switch]$NoPrompt
)
$ErrorActionPreference = 'Stop'

try {
    $Repo = (Resolve-Path -LiteralPath $Repo).Path
    $Builder = Join-Path $Repo 'scripts\build-in-container.ps1'
    $Installer = Join-Path $Repo 'scripts\install\windows.ps1'
    $PowerShell = (Get-Process -Id $PID).Path
    if ($LauncherPid -gt 0 -and (Get-Process -Id $LauncherPid -ErrorAction SilentlyContinue)) {
        Write-Host 'Waiting for Zack to exit...'
        Wait-Process -Id $LauncherPid -Timeout 15 -ErrorAction Stop
    }
    Write-Host 'Pulling the configured upstream...'
    & git -C $Repo pull --ff-only
    if ($LASTEXITCODE -ne 0) { throw "git pull failed (exit $LASTEXITCODE)." }
    Write-Host 'Building and testing the updated source (Docker must be running)...'
    & $PowerShell -NoProfile -ExecutionPolicy Bypass -File $Builder
    if ($LASTEXITCODE -ne 0) { throw "Build failed (exit $LASTEXITCODE); installation skipped." }
    & $PowerShell -NoProfile -ExecutionPolicy Bypass -File $Installer -NoPrompt
    if ($LASTEXITCODE -ne 0) { throw "Installation failed (exit $LASTEXITCODE)." }
    Write-Host 'Update finished.'
} catch {
    Write-Host "UPDATE FAILED: $_" -ForegroundColor Red
    Write-Host 'Fix the error, then rebuild and rerun scripts\install\windows.ps1 from the repository.'
    if (-not $NoPrompt) { Read-Host 'Press Enter to close' | Out-Null }
    exit 1
}
if (-not $NoPrompt) { Start-Sleep 3 }
