# Zack launcher

The Rust daemon registers hotkeys, displays the chooser, and runs configured actions.

- [src/](src/README.md): application code and unit tests.
- [Cargo.toml](Cargo.toml) and [Cargo.lock](Cargo.lock): dependencies and locked versions.
- [.cargo/](.cargo/README.md): platform build configuration.
- [mappings.json](mappings.json): public default shortcuts.
- [mappings.example.json](mappings.example.json): minimal starter configuration.
- [mappings.local.example.json](mappings.local.example.json): private overlay example;
  copy to ignored `mappings.local.json` for personal settings.

Run `cargo test --locked` from this directory. Follow the
[installation guide](../docs/installation.md) for platform build requirements,
or the [configuration guide](../README.md) to edit shortcuts.
