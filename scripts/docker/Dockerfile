# Build image for zack: Linux native + Windows (GNU) cross builds.
# Digest-pinned base (= rust:1.96-bookworm as of 2026-07) for reproducible
# builds; bump deliberately. bookworm => the produced Linux binary needs
# glibc >= 2.36 (Debian 12 / Ubuntu 23.04+) at runtime.
FROM rust:1.96-bookworm@sha256:7d0723df719e7f213b69dc7c8c595985c3f4b060cfbee4f7bc0e347a86fe3b6a
RUN apt-get update && apt-get install -y --no-install-recommends \
        mingw-w64 pkg-config \
        libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
        libxkbcommon-dev \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-pc-windows-gnu
WORKDIR /src/launcher
