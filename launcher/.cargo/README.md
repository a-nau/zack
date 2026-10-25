# Cargo configuration

[config.toml](config.toml) selects the MinGW linker for Windows GNU builds.
The [Docker build image](../../scripts/docker/README.md) supplies that linker.
Native macOS builds use the default system toolchain.
