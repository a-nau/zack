# kanata layer for zack

On Windows and Linux, held CapsLock becomes **Hyper (Ctrl+Alt+Shift)** and
a quick double-tap becomes **Escape**; a single tap does nothing. The
[zack daemon](../README.md) listens for Hyper hotkeys. macOS uses native
`hidutil` plus Zack's Accessibility event tap because Kanata's device grab
suppresses the MacBook function/media row.

## Install kanata

The [prerequisite setup guide](../docs/installation.md) has the full installation
steps, including the exact Windows download, checksum check and install path.

| OS | Command | Notes |
|---|---|---|
| Linux | `cargo install --locked --version 1.11.0 kanata` or [GitHub release](https://github.com/jtroo/kanata/releases) | needs `uinput` setup — `scripts/install/linux.sh` does it |
| Windows | [GitHub release](https://github.com/jtroo/kanata/releases): use **`kanata_gui.exe`** (LL-hook, no console window) | no driver/admin needed; hotkeys do NOT work while an *elevated* window is focused |

## Test it in the foreground

```sh
kanata -c kanata.kbd        # then: double-tap Caps = Esc, hold Caps + key = Ctrl+Alt+Shift+key
kanata --check -c kanata.kbd  # syntax check only
```

## Per-OS permissions

- **Linux**: kanata needs `/dev/uinput` access → `input` + `uinput` groups,
  udev rule, `uinput` kernel module. `scripts/install/linux.sh` follows the
  [official setup](https://github.com/jtroo/kanata/blob/main/docs/setup-linux.md);
  log out/in once afterwards.
- **Windows**: none, but keys are not remapped inside elevated (admin) windows.

## Known caveats

- If tap-hold misbehaves with key repeat on Linux, wrap the alias as
  `(multi f24 (tap-hold-press ...))` — documented kanata workaround.
- AltGr on Windows/German layouts: `process-unmapped-keys (all-except lctl ralt)`
  in the config handles this; if AltGr still misfires see the
  [`windows-altgr` options](https://github.com/jtroo/kanata/blob/main/docs/config.adoc#windows-only-windows-altgr).
