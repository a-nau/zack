# Install Zack and its prerequisites

Use this guide to set up a machine, build Zack, and make Caps Lock open the
launcher. Choose [macOS](#macos), [Linux](#linux-x11), or [Windows](#windows).
Commands within each section run in order. If a tool is already installed,
check its version and skip its installation step.

If you received a ready-made Zack bundle, follow the README inside it instead.
The build tools below are for installation from source. Maintainers preparing
bundles should also read the [release guide](releasing.md).

## macOS

Use Terminal on either an Apple Silicon or Intel Mac. Zack builds for the Mac
you are using. You need Apple's command-line tools and Rust; Docker and Kanata
are not required on macOS.

### 1. Install Apple's command-line tools

Run the following command, accept the installation dialog, and wait for it to
finish. This installs Git, the compiler, and the macOS SDK:

```sh
xcode-select --install
```

Verify the tools after installation:

```sh
xcode-select -p
clang --version
git --version
```

If macOS says the tools are already installed but a check fails, follow
[Apple's command-line tools instructions](https://developer.apple.com/documentation/xcode/installing-the-command-line-tools/).

### 2. Install Rust

Download and run the [official Rust installer](https://rust-lang.org/tools/install/).
Choose the default stable toolchain, then load Cargo into the current shell:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/zack-rustup.sh
sh /tmp/zack-rustup.sh
. "$HOME/.cargo/env"
rustc --version
cargo --version
```

If you already have Rust, use `rustup update stable` before building. The
current dependencies require a recent stable compiler.

### 3. Get Zack, build, and install

Choose a new checkout directory, or change into your existing checkout:

```sh
mkdir -p ~/code
cd ~/code
git clone https://github.com/a-nau/zack.git
cd zack
scripts/build-macos.sh
scripts/install/macos.sh
```

The installer asks for your administrator password to keep the Caps Lock
remapping active after restart. It also sets up local signing for Zack.
When System Settings opens, add and enable `~/.local/bin/zack` under
**Privacy & Security → Accessibility**. Then restart the launcher:

```sh
launchctl kickstart -k "gui/$(id -u)/com.zack.launcher"
```

Hold **Caps Lock** and press **Space**: the chooser should appear. If it does
not, check `~/Library/Logs/zack.log` and confirm the Accessibility toggle is on.

If `scripts/install/macos.sh` was interrupted or declined at the
administrator password prompt, zack may end up not running at all (the
script stops the existing launcher before reinstalling it). Just run
`scripts/install/macos.sh` again and let it finish.

## Linux (X11)

These steps target **Ubuntu 24.04 or Debian 12 and newer, on x86-64**, with a
systemd desktop and an X11 login session. Zack's container build produces an
x86-64 binary requiring glibc 2.36 or newer. Wayland sessions are unsupported.

Check the session and architecture in a terminal inside your desktop:

```sh
uname -m
printf '%s\n' "$XDG_SESSION_TYPE"
getconf GNU_LIBC_VERSION
```

Expect `x86_64`, `x11`, and glibc 2.36 or newer. If the session is Wayland, log
out and select your desktop's X11/Xorg session at the login screen.

### 1. Install system packages

Git and curl fetch the source and tools. The compiler and pkg-config build
Kanata. The remaining libraries support Zack's X11/OpenGL window:

```sh
sudo apt update
sudo apt install git curl ca-certificates build-essential pkg-config unzip \
  libxcb-render0 libxcb-shape0 libxcb-xfixes0 libxkbcommon0 libxkbcommon-x11-0 \
  libx11-6 libxcursor1 libxi6 libxrandr2 libgl1 libegl1
```

### 2. Install Docker Engine

If `docker info` already works as your normal user, skip this step. For a fresh
Docker installation on Ubuntu or Debian, register Docker's official package
repository and install the engine:

```sh
. /etc/os-release
case "$ID" in ubuntu|debian) ;; *) echo 'Use Docker instructions for your distribution'; exit 1 ;; esac
sudo install -m 0755 -d /etc/apt/keyrings
sudo curl -fsSL "https://download.docker.com/linux/$ID/gpg" -o /etc/apt/keyrings/docker.asc
sudo chmod a+r /etc/apt/keyrings/docker.asc
sudo tee /etc/apt/sources.list.d/docker.sources >/dev/null <<EOF_DOCKER
Types: deb
URIs: https://download.docker.com/linux/$ID
Suites: ${UBUNTU_CODENAME:-$VERSION_CODENAME}
Components: stable
Architectures: $(dpkg --print-architecture)
Signed-By: /etc/apt/keyrings/docker.asc
EOF_DOCKER
sudo apt update
sudo apt install docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
sudo systemctl enable --now docker
sudo usermod -aG docker "$USER"
```

Log out and back in so the group change takes effect. The `docker` group grants
root-level control of the machine; see Docker's
[post-installation guidance](https://docs.docker.com/engine/install/linux-postinstall/).
For an existing installation from another package source, use Docker's
[Ubuntu](https://docs.docker.com/engine/install/ubuntu/) or
[Debian](https://docs.docker.com/engine/install/debian/) migration instructions.

Verify Docker as your normal user:

```sh
docker info
docker run --rm hello-world
```

### 3. Install Rust and Kanata

Zack builds inside Docker. Kanata is installed separately with Cargo, so this
step installs Rust on the host too. Use the default stable toolchain:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/zack-rustup.sh
sh /tmp/zack-rustup.sh
. "$HOME/.cargo/env"
rustc --version
cargo --version
cargo install --locked --version 1.11.0 kanata
kanata --version
```

For an existing Rust installation, run `rustup update stable` first. The
[official Rust instructions](https://rust-lang.org/tools/install/) explain shell
setup if `cargo` is still not found. Keep this terminal open for the next step.

### 4. Get Zack, build, and install

```sh
mkdir -p ~/code
cd ~/code
git clone https://github.com/a-nau/zack.git
cd zack
scripts/build-in-container.sh
scripts/install/linux.sh
```

Run the installer as your normal user. It invokes `sudo` for keyboard-device
permissions and creates user services for Zack and Kanata. Log out and back in
once after the first installation, then check both services:

```sh
systemctl --user status zack kanata
```

Hold **Caps Lock** and press **Space** to open the chooser. If it fails, inspect
`journalctl --user -u zack -u kanata -e`. A permission error for `/dev/uinput`
usually means the new group memberships have not taken effect. Missing shared
libraries can be identified with `ldd ~/.local/bin/zack`.

## Windows

Use **64-bit Windows on an Intel/AMD PC**, with Windows PowerShell 5.1 or newer.
The build uses Docker's Linux containers; host Rust and Visual Studio are not
needed. Setting up WSL/Docker may require administrator access. Zack's own
installer runs as your normal user.

### 1. Install Git and Docker Desktop

1. Install [Git for Windows](https://git-scm.com/downloads/win), choosing the
   option that makes Git available from the command line.
2. In an **administrator PowerShell**, run `wsl --install`, then restart Windows
   if prompted. Finish the Linux distribution's first-run setup. Run
   `wsl --update` and check `wsl --version`.
3. Install [Docker Desktop for Windows](https://docs.docker.com/desktop/setup/install/windows-install/),
   selecting its WSL 2 backend. Start Docker Desktop and wait until its engine
   is running. Use **Linux containers**.

Microsoft's [WSL installation guide](https://learn.microsoft.com/en-us/windows/wsl/install)
covers virtualization and Windows-version requirements. If you already use
Rancher Desktop, select its [**dockerd (moby)** engine](https://docs.rancherdesktop.io/ui/preferences/container-engine/general/)
so the Docker CLI works.

Open a new, normal PowerShell window and verify:

```powershell
git --version
docker info
docker run --rm hello-world
```

### 2. Get Zack and download Kanata

Clone into a new directory, or change into your existing checkout:

```powershell
New-Item -ItemType Directory -Force "$env:USERPROFILE\code" | Out-Null
Set-Location "$env:USERPROFILE\code"
git clone https://github.com/a-nau/zack.git
Set-Location zack
```

Download **windows-binaries-x64.zip** and **sha256sums** from the official
[Kanata v1.11.0 release](https://github.com/jtroo/kanata/releases/tag/v1.11.0).
Save both to Downloads. Verify the archive against the release checksum before
extracting it; the following block stops if they differ:

```powershell
$ZackDownloads = Join-Path $env:USERPROFILE 'Downloads'
$ZackArchive = Join-Path $ZackDownloads 'windows-binaries-x64.zip'
$ZackExpected = @(Get-Content (Join-Path $ZackDownloads 'sha256sums') |
    Where-Object { $_ -match ' windows-binaries-x64\.zip$' })
if ($ZackExpected.Count -ne 1) { throw 'Expected exactly one Kanata archive checksum' }
$ZackActual = (Get-FileHash -Algorithm SHA256 $ZackArchive).Hash
if ($ZackActual -ne ($ZackExpected[0] -split '\s+')[0]) { throw 'Kanata checksum mismatch' }
Expand-Archive -LiteralPath $ZackArchive -DestinationPath .\dist\kanata-download -Force
New-Item -ItemType Directory -Force .\release | Out-Null
Copy-Item .\dist\kanata-download\kanata_windows_gui_winIOv2_x64.exe .\release\kanata_gui.exe
```

This GUI variant runs without a console window or an Interception driver.
The Zack installer finds `release\kanata_gui.exe` and installs it for your user.
Downloaded binaries belong in ignored build/release directories, not Git.

### 3. Build and install

If updating an older checkout that still has a top-level `install/` folder,
run `git pull --ff-only`, then use the new installer path below once.

From the Zack checkout, run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-in-container.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install\windows.ps1
Get-Process zack, kanata_gui
```

The execution-policy setting applies only to each process. If your organization
blocks scripts through Group Policy, use its approved process rather than
changing machine policy.

Hold **Caps Lock** and press **Space** to open the chooser. Both programs start
automatically at login. If `docker info` fails, start Docker Desktop and check
that it uses Linux containers. If the installer cannot find Kanata, verify
`release\kanata_gui.exe` exists. Kanata's normal-user hotkeys do not operate
inside applications launched as administrator.
