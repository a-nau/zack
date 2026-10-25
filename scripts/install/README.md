# Installation scripts

These scripts install an already-built Zack binary and configure startup.
Follow the [setup guide](../../docs/installation.md) to install prerequisites
and build first. Run the appropriate command from the repository root:

| Script | Purpose |
|---|---|
| `scripts/install/macos.sh` | Install the signed launcher, native Caps remap and user LaunchAgent |
| `scripts/install/linux.sh` | Install Zack and Kanata, keyboard permissions and systemd user services |
| `.\scripts\install\windows.ps1` | Install Zack and Kanata for the current Windows user |
| `.\scripts\install\uninstall-windows.ps1` | Remove the Windows installation; prompts before removal |

[BUNDLE-README.md](BUNDLE-README.md) becomes the README in a Windows bundle.
The installers also support flat bundles, where the executable and configs sit
beside the installer. [make-bundle.sh](../make-bundle.sh) creates that layout.
