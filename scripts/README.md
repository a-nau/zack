# Build, installation and release scripts

Run scripts from the repository root; see the [setup guide](../docs/installation.md)
for prerequisites.

| Path | Purpose |
|---|---|
| [install/](install/README.md) | Installers and the Windows uninstaller |
| [build-macos.sh](build-macos.sh) | Native macOS build |
| [build-in-container.sh](build-in-container.sh) / [build-in-container.ps1](build-in-container.ps1) | Linux and Windows builds through Docker |
| [docker/](docker/README.md) | Container build environment |
| [update-windows.ps1](update-windows.ps1) | Pull, build and reinstall a Windows checkout |
| [macos-sign-setup.sh](macos-sign-setup.sh) | Create the local signing identity used by the Mac installer |
| [make-bundle.sh](make-bundle.sh) | Assemble a portable installation zip |
| [license-config/](license-config/README.md) | Reviewed inputs for executable notices |
| [generate-license-notices.py](generate-license-notices.py) / [bundle-licenses.py](bundle-licenses.py) | Generate notices and attach verified source archives |
| [unpack-kanata-source.py](unpack-kanata-source.py) | Reconstruct Kanata's offline source tree from a bundle |
| [test-bundle-licenses.py](test-bundle-licenses.py) | Offline packaging guard checks |

Build outputs and generated notices go to ignored `dist/`. See the
[release guide](../docs/releasing.md) before assembling executable bundles.
