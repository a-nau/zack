# Build zack release binaries for Linux (x86_64) and Windows (x86_64)
# inside a Docker container. Requires Docker Desktop (or Rancher Desktop) running on Windows.
#
# Usage: scripts\build-in-container.ps1
# Output: dist\zack-linux-x86_64, dist\zack-windows-x86_64.exe
$ErrorActionPreference = "Stop"

$Repo  = Split-Path -Parent $PSScriptRoot
$Image = "zack-build"
$Dist  = Join-Path $Repo "dist"
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

try { & docker info *>$null } catch {}
if ($LASTEXITCODE -ne 0) {
    Write-Error "Docker daemon is not reachable. Start Docker Desktop (or Rancher Desktop) and wait for it to be ready, then retry."
    exit 1
}

docker build --platform linux/amd64 -t $Image (Join-Path $Repo "scripts\docker")
if ($LASTEXITCODE -ne 0) { throw "Docker image build failed (exit $LASTEXITCODE)." }

docker run --rm --platform linux/amd64 `
    -v "${Repo}:/src" `
    -v "zack-cargo-registry:/usr/local/cargo/registry" `
    -v "zack-cargo-target:/src/launcher/target" `
    -e REPO=/src `
    $Image `
    bash -c @'
set -euo pipefail
cargo test  --locked --release --target x86_64-unknown-linux-gnu
cargo build --locked --release --target x86_64-unknown-linux-gnu
cargo build --locked --release --target x86_64-pc-windows-gnu
mkdir -p /src/dist
install -m755 target/x86_64-unknown-linux-gnu/release/zack   /src/dist/zack-linux-x86_64
install -m755 target/x86_64-pc-windows-gnu/release/zack.exe  /src/dist/zack-windows-x86_64.exe
'@
if ($LASTEXITCODE -ne 0) { throw "Docker build/test run failed (exit $LASTEXITCODE)." }

Write-Host ""
Write-Host "Built artifacts:"
Get-ChildItem $Dist | Format-Table Name, Length, LastWriteTime
