#!/usr/bin/env bash
# Assemble a ready-to-install bundle for one OS into dist/zack-bundle-<os>/
# (plus an archive next to it). Bundles are flat folders the install scripts
# detect automatically — copy to the target machine and run the installer.
#
# Usage:
#   scripts/make-bundle.sh windows          # builds in Docker, bundles kanata_gui.exe
#   scripts/make-bundle.sh linux            # builds in Docker, bundles kanata
#   scripts/make-bundle.sh macos [arch]     # needs dist/zack-macos-<arch>
#                                           #   (run scripts/build-macos.sh ON the Mac first)
#                                           #   arch: arm64 (default) or x86_64
#
# Windows/Linux kanata binaries come from the official GitHub release (pinned
# version) and are verified against the release's sha256sums file. macOS uses
# native hidutil + Zack's event tap and does not bundle kanata.
set -euo pipefail

KANATA_VERSION=v1.11.0
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OS="${1:-}"
[ -n "$OS" ] || { grep '^#' "$0" | head -14; exit 1; }

[ -f "$REPO/dist/licenses/inventory.json" ] || {
    echo "Generate executable notices first: python3 scripts/generate-license-notices.py (see docs/releasing.md)." >&2
    exit 1
}

CACHE="$REPO/dist/.kanata-cache/$KANATA_VERSION"
BASE_URL="https://github.com/jtroo/kanata/releases/download/$KANATA_VERSION"

# fetch_kanata <zip-name> <path-inside-zip> <output-path>
fetch_kanata() {
    local zip_name="$1" inner="$2" out="$3"
    mkdir -p "$CACHE"
    if [ ! -f "$CACHE/$zip_name" ]; then
        echo "Downloading $zip_name ($KANATA_VERSION)..."
        curl -sSfL -o "$CACHE/$zip_name" "$BASE_URL/$zip_name"
        curl -sSfL -o "$CACHE/sha256sums" "$BASE_URL/sha256sums"
    fi
    (cd "$CACHE" && grep " $zip_name\$" sha256sums | sha256sum -c - >/dev/null) \
        || { echo "checksum mismatch for $zip_name"; exit 1; }
    unzip -o -q "$CACHE/$zip_name" -d "$CACHE/extracted-${zip_name%.zip}"
    install -m755 "$CACHE/extracted-${zip_name%.zip}/$inner" "$out"
}

make_archive() {
    local dir="$1"
    python3 "$REPO/scripts/bundle-licenses.py" "$dir" --os "$OS" \
        --kanata-version "$KANATA_VERSION"
    (cd "$REPO/dist" && rm -f "$(basename "$dir").zip" \
        && if command -v zip >/dev/null; then
               zip -r -q "$(basename "$dir").zip" "$(basename "$dir")"
           else
               python3 -m zipfile -c "$(basename "$dir").zip" "$(basename "$dir")"
           fi)
    echo "Bundle:  $dir"
    echo "Archive: $dir.zip"
}

case "$OS" in
windows)
    "$REPO/scripts/build-in-container.sh"
    BUNDLE="$REPO/dist/zack-bundle-windows"
    rm -rf "$BUNDLE" && mkdir -p "$BUNDLE"
    cp "$REPO/dist/zack-windows-x86_64.exe" "$BUNDLE/zack.exe"
    fetch_kanata windows-binaries-x64.zip kanata_windows_gui_winIOv2_x64.exe "$BUNDLE/kanata_gui.exe"
    cp "$REPO/launcher/mappings.json" "$REPO/kanata/kanata.kbd" "$BUNDLE/"
    cp "$REPO/scripts/install/windows.ps1" "$BUNDLE/install.ps1"
    cp "$REPO/scripts/install/BUNDLE-README.md" "$BUNDLE/README.md"
    make_archive "$BUNDLE"
    echo "On the target: run  powershell -ExecutionPolicy Bypass -File .\\install.ps1"
    ;;
linux)
    "$REPO/scripts/build-in-container.sh"
    BUNDLE="$REPO/dist/zack-bundle-linux"
    rm -rf "$BUNDLE" && mkdir -p "$BUNDLE"
    install -m755 "$REPO/dist/zack-linux-x86_64" "$BUNDLE/zack"
    fetch_kanata linux-binaries-x64.zip kanata_linux_x64 "$BUNDLE/kanata"
    cp "$REPO/launcher/mappings.json" "$REPO/kanata/kanata.kbd" "$BUNDLE/"
    install -m755 "$REPO/scripts/install/linux.sh" "$BUNDLE/install.sh"
    cat > "$BUNDLE/README.md" <<'EOF'
# zack — Linux install bundle (X11 sessions)

Run `./install.sh` from this folder (it sudos only for the uinput/udev
setup). It installs zack + kanata to ~/.local/bin, sets up systemd user
services with autostart, and configures CapsLock (double-tap = Esc, hold = Hyper).
Log out/in once after the first install (group membership), then:
double-tap CapsLock = Esc, hold CapsLock+Space = launcher.
Configs live in ~/.config/zack/ and ~/.config/kanata/ and survive reinstalls.
EOF
    make_archive "$BUNDLE"
    echo "On the target: run  ./install.sh"
    ;;
macos)
    ARCH="${2:-arm64}"
    BIN="$REPO/dist/zack-macos-$ARCH"
    [ -f "$BIN" ] || {
        echo "missing $BIN — macOS cannot be cross-built; run scripts/build-macos.sh ON the Mac,"
        echo "copy the binary into dist/, then re-run this script."
        exit 1
    }
    case "$ARCH" in
        arm64|x86_64) ;;
        *) echo "arch must be arm64 or x86_64"; exit 1 ;;
    esac
    BUNDLE="$REPO/dist/zack-bundle-macos-$ARCH"
    rm -rf "$BUNDLE" && mkdir -p "$BUNDLE"
    install -m755 "$BIN" "$BUNDLE/zack"
    cp "$REPO/launcher/mappings.json" "$BUNDLE/mappings.json"
    install -m755 "$REPO/scripts/install/macos.sh" "$BUNDLE/install.sh"
    install -m755 "$REPO/scripts/macos-sign-setup.sh" "$BUNDLE/macos-sign-setup.sh"
    cat > "$BUNDLE/README.md" <<'EOF'
# zack — macOS install bundle

Run `./install.sh` from this folder. It installs Zack as a user LaunchAgent
and a tiny native Caps remap job. It creates/reuses a stable local signing
identity and then opens the one required manual pane: grant Zack Accessibility.
No Kanata, Karabiner driver, or Input Monitoring permission is required.
Then: double-tap CapsLock = Esc, hold CapsLock+Space = launcher.
EOF
    make_archive "$BUNDLE"
    echo "On the target: run  ./install.sh"
    ;;
*)
    echo "unknown OS '$OS' — use windows, linux, or macos"
    exit 1
    ;;
esac
