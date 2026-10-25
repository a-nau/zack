# zack — Windows install bundle

CapsLock-driven launcher. After installing: **single-tap CapsLock = nothing**,
**double-tap CapsLock = Esc**, **hold CapsLock = Hyper**, `Caps+Space` = fuzzy
chooser (searches, folders, commands), `Caps+letter` = open/focus apps and URLs
(press again while the app is focused to minimize it), `Caps+arrows` = window management
(Up = maximize/restore, Down = minimize, Left/Right = move across monitors), `Caps+O` = Alt-Tab window switcher (hold Caps, tap O to cycle, release to pick), `Caps+P` = Ctrl-Tab (next tab).

## What's in this folder

| File | Purpose |
|---|---|
| `install.ps1` | one-shot installer (run this) |
| `zack.exe` | the launcher daemon |
| `kanata_gui.exe` | kanata v1.11.0 keyboard remapper, tray build (official release, checksum-verified) |
| `kanata.kbd` | CapsLock config: hold = Ctrl+Alt+Shift, double-tap = Esc |
| `mappings.json` | your searches / folders / hotkeys |

## Install

1. **Copy this whole folder somewhere local** (e.g. `Downloads\zack`) —
   running from a network share can trip PowerShell policies.
2. If your AutoHotkey setup is running, **exit it** (tray icon → Exit) — it
   also owns CapsLock and the two will fight.
3. Open a normal (non-admin) PowerShell in the folder and run:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\install.ps1
   ```

That's everything — and you can **close the PowerShell window afterwards**;
both programs are started detached and keep running. The script:

- installs `zack.exe` → `%LOCALAPPDATA%\zack`, `kanata_gui.exe` → `%LOCALAPPDATA%\kanata`
- puts the configs in `%APPDATA%\zack\mappings.json` and `%APPDATA%\kanata\kanata.kbd`
  (**only if not already there** — your edits survive reinstalls)
- registers both for **autostart at logon** (`HKCU\...\Run`, no admin needed)
- starts both immediately (zack is invisible; kanata sits in the tray)

Re-running the script later is safe — it stops the old processes, replaces
the binaries, and leaves your configs alone. That's also how you update.
**Important:** because your configs are never touched, updates that add new
config sections (like app-specific shortcuts) won't activate until you run
`install.ps1 -ForceConfig` (your old mappings.json is saved as .bak) or merge
the new sections into `%APPDATA%\zack\mappings.json` yourself.

## Quick test

1. Double-tap `CapsLock` → acts as Esc; a single tap does nothing.
2. Hold `CapsLock` + `Space` → chooser appears centered; type `g test` + Enter → Google search.
3. `Caps+Space`, `f `, type `dwn`, Enter → Downloads folder.
4. Hold `CapsLock` + `F` with Chrome already open → focuses the existing
   window; press again → minimizes it.
5. Hold `CapsLock` + `Right` → active window jumps to the next monitor.

Forgot a binding? `Caps+Space`, type `?` — full searchable overview.

## Editing shortcuts

`Caps+Space`, type `config`, Enter → opens `mappings.json`. Edit, save, then
`Caps+Space`, `rel`, Enter → reloaded (errors show in the chooser itself).

## Troubleshooting / uninstall

- Log: `%APPDATA%\zack\zack.log`
- Hotkeys don't work while an **elevated (admin) window** is focused — Windows limitation.
- A second `zack.exe` start exits instantly on purpose (single instance).
- Uninstall: delete the `zack` and `kanata` values under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, end both processes in
  Task Manager, delete `%LOCALAPPDATA%\zack`, `%LOCALAPPDATA%\kanata`,
  `%APPDATA%\zack`, `%APPDATA%\kanata`.

Source: the Zack source repository
