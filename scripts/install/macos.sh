#!/usr/bin/env bash
# Install Zack on macOS. Caps is remapped natively (hidutil) to an inert F18
# sentinel; Zack's Accessibility event tap turns held Caps into Hyper. No
# Kanata, Karabiner driver, or Input Monitoring permission is required.
# Usage: scripts/install/macos.sh   (run ON the Mac, after scripts/build-macos.sh)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -f "$SCRIPT_DIR/zack" ]; then
    # Flat bundle layout (scripts/make-bundle.sh output).
    BIN="$SCRIPT_DIR/zack"
    SRC_MAP="$SCRIPT_DIR/mappings.json"
else
    REPO="$(cd "$SCRIPT_DIR/../.." && pwd)"
    BIN="$REPO/dist/zack-macos-$(uname -m)"
    SRC_MAP="$REPO/launcher/mappings.json"
fi
for src in "$BIN" "$SRC_MAP"; do
    [ -f "$src" ] || { echo "missing $src — incomplete bundle/build"; exit 1; }
done

# Get admin auth up front. If this is skipped, a declined/non-interactive sudo
# prompt during the LaunchDaemon step below (after the running launcher has
# already been booted out) aborts the script via `set -e` and leaves zack
# not running at all until the script is re-run to completion.
sudo -v || { echo "sudo access is required (for the CapsLock remap LaunchDaemon)"; exit 1; }

# A stable local identity makes Accessibility a one-time grant across updates.
if ! security find-identity -v -p codesigning 2>/dev/null | grep -q zack-codesign; then
    if [ -x "$SCRIPT_DIR/macos-sign-setup.sh" ]; then
        "$SCRIPT_DIR/macos-sign-setup.sh"
    elif [ -x "$SCRIPT_DIR/../macos-sign-setup.sh" ]; then
        "$SCRIPT_DIR/../macos-sign-setup.sh"
    else
        echo "missing zack-codesign identity and macos-sign-setup.sh"
        exit 1
    fi
fi

# --- zack binary -------------------------------------------------------------
mkdir -p ~/.local/bin
launchctl bootout "gui/$(id -u)/com.zack.launcher" 2>/dev/null || true
install -m755 "$BIN" ~/.local/bin/zack
codesign --force --sign zack-codesign --identifier com.zack.launcher ~/.local/bin/zack

# --- mappings (only if absent) ----------------------------------------------
ZACK_CFG_DIR="$HOME/Library/Application Support/zack"
mkdir -p "$ZACK_CFG_DIR"
if [ -f "$SCRIPT_DIR/zack" ]; then
    [ -f "$ZACK_CFG_DIR/mappings.json" ] || cp "$SRC_MAP" "$ZACK_CFG_DIR/"
elif [ ! -f "$ZACK_CFG_DIR/mappings.json" ]; then
    # Repo mode: edit launcher/mappings.json and type `rel` to reload it live.
    printf '%s\n' "$SRC_MAP" > "$ZACK_CFG_DIR/config-path.txt"
fi

# --- native Caps remap -------------------------------------------------------
# macOS 26 requires root for UserKeyMapping on the built-in keyboard. This
# tiny job reapplies CapsLock (HID 0x39) -> F18 (0x6d) at boot and periodically
# in case the keyboard service is recreated. Zack consumes F18; no device is
# grabbed, so brightness/volume/media keys remain completely native.
ROOT_STAGE="$(mktemp -d "${TMPDIR:-/tmp}/zack-install.XXXXXX")"
trap 'rm -rf "$ROOT_STAGE"' EXIT
tee "$ROOT_STAGE/com.zack.caps-remap.plist" >/dev/null <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.zack.caps-remap</string>
  <key>ProgramArguments</key><array>
    <string>/usr/bin/hidutil</string><string>property</string><string>--set</string>
    <string>{"UserKeyMapping":[{"HIDKeyboardModifierMappingSrc":30064771129,"HIDKeyboardModifierMappingDst":30064771181}]}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>StartInterval</key><integer>60</integer>
  <key>StandardOutPath</key><string>/dev/null</string>
  <key>StandardErrorPath</key><string>/dev/null</string>
</dict></plist>
EOF

# One sudo session. Also disable/remove Zack's obsolete macOS Kanata jobs from
# older installs; the third-party driver itself is left installed but unused.
sudo /bin/bash -c '
set -euo pipefail
stage="$1"
/bin/launchctl bootout system/com.zack.kanata 2>/dev/null || true
/bin/launchctl bootout system/com.zack.karabiner-vhid 2>/dev/null || true
/bin/launchctl bootout system/com.zack.caps-remap 2>/dev/null || true
/bin/rm -f /Library/LaunchDaemons/com.zack.kanata.plist
/bin/rm -f /Library/LaunchDaemons/com.zack.karabiner-vhid.plist
/usr/bin/install -o root -g wheel -m 644 "$stage/com.zack.caps-remap.plist" /Library/LaunchDaemons/com.zack.caps-remap.plist
/bin/launchctl bootstrap system /Library/LaunchDaemons/com.zack.caps-remap.plist
' _ "$ROOT_STAGE"

# --- zack user LaunchAgent ---------------------------------------------------
mkdir -p ~/Library/LaunchAgents ~/Library/Logs
tee ~/Library/LaunchAgents/com.zack.launcher.plist >/dev/null <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.zack.launcher</string>
  <key>ProgramArguments</key><array><string>$HOME/.local/bin/zack</string></array>
  <key>EnvironmentVariables</key>
  <dict><key>PATH</key><string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin</string></dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/zack.log</string>
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/zack.log</string>
</dict></plist>
EOF
launchctl bootstrap "gui/$(id -u)" ~/Library/LaunchAgents/com.zack.launcher.plist

cat <<'EOF'

Done. One manual permission remains:
1. System Settings -> Privacy & Security -> Accessibility:
   add and enable ~/.local/bin/zack.

Optional Caps+D dictation:
2. System Settings -> Keyboard -> Dictation -> enable it and set Shortcut to
   "Press Right Command Key Twice".

No Kanata, Karabiner driver, or Input Monitoring permission is required.
EOF
open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility" 2>/dev/null || true

echo "Status:  launchctl print gui/$(id -u)/com.zack.launcher | head -5"
echo "         sudo launchctl print system/com.zack.caps-remap | head -5"
echo "Log:     ~/Library/Logs/zack.log"
