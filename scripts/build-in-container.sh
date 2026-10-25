#!/usr/bin/env bash
# Build zack release binaries for Linux (x86_64) and Windows (x86_64)
# inside a Docker container — the local, no-CI build path.
#
# Usage: scripts/build-in-container.sh
# Output: dist/zack-linux-x86_64, dist/zack-windows-x86_64.exe
#
# The image is x86_64-only (--platform linux/amd64); on an ARM host this
# runs under emulation. Cargo caches persist in named Docker volumes.
set -euo pipefail

REPO="${REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
IMAGE=zack-build
DIST="$REPO/dist"
mkdir -p "$DIST"

docker build --platform linux/amd64 -t "$IMAGE" "$REPO/scripts/docker"

docker run --rm --platform linux/amd64 \
    -v "$REPO":/src \
    -v zack-cargo-registry:/usr/local/cargo/registry \
    -v zack-cargo-target:/src/launcher/target \
    -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    "$IMAGE" \
    bash -c "set -euo pipefail \
          && cargo test --locked --release --target x86_64-unknown-linux-gnu \
          && cargo build --locked --release --target x86_64-unknown-linux-gnu \
          && cargo build --locked --release --target x86_64-pc-windows-gnu \
          && install -m755 target/x86_64-unknown-linux-gnu/release/zack /src/dist/zack-linux-x86_64 \
          && install -m755 target/x86_64-pc-windows-gnu/release/zack.exe /src/dist/zack-windows-x86_64.exe \
          && chown \"\$HOST_UID:\$HOST_GID\" /src/dist/zack-linux-x86_64 /src/dist/zack-windows-x86_64.exe"

echo
echo "Built artifacts:"
ls -lh "$DIST"
echo
echo "Runtime deps of the Linux binary (must exist on the target machine):"
ldd "$DIST/zack-linux-x86_64" 2>/dev/null | grep -Ei "xcb|xkb|EGL|GL" || true
