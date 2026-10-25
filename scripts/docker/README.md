# Container build environment

[Dockerfile](Dockerfile) pins the Rust/Debian build image and installs MinGW
and Linux GUI development libraries. It builds Zack for Linux x86-64 and
Windows x86-64; the Linux output requires glibc 2.36 or newer.

Use [build-in-container.sh](../build-in-container.sh) or
[build-in-container.ps1](../build-in-container.ps1) from the repository root.
The wrappers run the Linux unit tests and put binaries in ignored `dist/`.
