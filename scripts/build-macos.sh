#!/usr/bin/env bash
# Build the zack release binary natively ON the Mac.
# macOS cannot be cross-built from a Linux container (no redistributable SDK).
#
# Prereq: rustup (https://rustup.rs). Usage: scripts/build-macos.sh
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
command -v cargo >/dev/null || { echo "install rust first: https://rustup.rs"; exit 1; }

cd "$REPO/launcher"
cargo build --locked --release
mkdir -p "$REPO/dist"
install -m755 target/release/zack "$REPO/dist/zack-macos-$(uname -m)"
echo "Built: $REPO/dist/zack-macos-$(uname -m)"
