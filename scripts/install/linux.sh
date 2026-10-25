#!/usr/bin/env bash
# Install zack + kanata on Linux (X11 sessions only — see launcher/README.md).
# Idempotent; never overwrites an existing user-edited config.
# Usage: scripts/install/linux.sh   (asks for sudo for the uinput/udev steps)
set -euo pipefail

if [ "$(id -u)" -eq 0 ]; then
    echo "Run as your normal user (the script sudos where needed), not as root." >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -f "$SCRIPT_DIR/zack" ]; then
    # Flat bundle layout (scripts/make-bundle.sh output).
    BIN="$SCRIPT_DIR/zack"
    SRC_MAP="$SCRIPT_DIR/mappings.json"
    SRC_KBD="$SCRIPT_DIR/kanata.kbd"
    BUNDLED_KANATA="$SCRIPT_DIR/kanata"
else
    REPO="$(cd "$SCRIPT_DIR/../.." && pwd)"
    BIN="$REPO/dist/zack-linux-x86_64"
    SRC_MAP="$REPO/launcher/mappings.json"
    SRC_KBD="$REPO/kanata/kanata.kbd"
    BUNDLED_KANATA=""
fi
for src in "$BIN" "$SRC_MAP" "$SRC_KBD"; do
    [ -f "$src" ] || { echo "missing $src — incomplete bundle/build"; exit 1; }
done

# --- zack binary (stop a running instance before replacing) -------------
mkdir -p ~/.local/bin
systemctl --user stop zack.service 2>/dev/null || true
install -m755 "$BIN" ~/.local/bin/zack

# --- configs (only if absent) ----------------------------------------------
mkdir -p ~/.config/zack ~/.config/kanata
[ -f ~/.config/zack/mappings.json ] || cp "$SRC_MAP" ~/.config/zack/
[ -f ~/.config/kanata/kanata.kbd ]     || cp "$SRC_KBD" ~/.config/kanata/

# --- kanata binary: bundled copy first, then PATH ----------------------------
if [ -n "$BUNDLED_KANATA" ] && [ -f "$BUNDLED_KANATA" ]; then
    install -m755 "$BUNDLED_KANATA" ~/.local/bin/kanata
    KANATA="$HOME/.local/bin/kanata"
else
    KANATA="$(command -v kanata || true)"
fi
if [ -z "$KANATA" ]; then
    echo "kanata not found (bundle or PATH). Install it first, e.g.:  cargo install kanata"
    echo "then re-run this script."
    exit 1
fi

# --- uinput permissions (official kanata setup: module, groups, udev) -------
echo "Setting up uinput access for kanata (needs sudo)..."
getent group uinput >/dev/null || sudo groupadd uinput
sudo usermod -aG input,uinput "$USER"
echo uinput | sudo tee /etc/modules-load.d/uinput.conf >/dev/null
sudo modprobe uinput || true
echo 'KERNEL=="uinput", MODE="0660", GROUP="uinput", OPTIONS+="static_node=uinput"' \
    | sudo tee /etc/udev/rules.d/99-kanata-uinput.rules >/dev/null
sudo udevadm control --reload-rules && sudo udevadm trigger

# --- systemd user units ------------------------------------------------------
mkdir -p ~/.config/systemd/user
cat > ~/.config/systemd/user/kanata.service <<EOF
[Unit]
Description=kanata keyboard remapper (zack layer)

[Service]
ExecStart=$KANATA -c %h/.config/kanata/kanata.kbd
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
EOF

cat > ~/.config/systemd/user/zack.service <<'EOF'
[Unit]
Description=zack launcher daemon
After=graphical-session.target
PartOf=graphical-session.target

[Service]
ExecStart=%h/.local/bin/zack
Restart=on-failure
RestartSec=3

[Install]
WantedBy=graphical-session.target
EOF

systemctl --user daemon-reload
systemctl --user enable zack.service kanata.service
if ! systemctl --user restart zack.service; then
    echo "WARNING: zack did not start (no graphical session?). It will start at next login." >&2
fi

# Only start kanata now if BOTH group memberships are already effective;
# a fresh install needs one logout/login first.
if id -nG | tr ' ' '\n' | grep -qx uinput && id -nG | tr ' ' '\n' | grep -qx input; then
    systemctl --user restart kanata.service
    echo "kanata started."
else
    echo
    echo "NOTE: you were just added to the 'input' and 'uinput' groups."
    echo "Log out and back in once, then run:  systemctl --user start kanata.service"
fi

echo
echo "Done. Status:   systemctl --user status zack kanata"
echo "Logs:           journalctl --user -u zack -u kanata -e"
