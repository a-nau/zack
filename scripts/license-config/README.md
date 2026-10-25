# Inputs for executable notices

- [about.toml](about.toml): reviewed license choices and Zack build targets.
- [kanata.json](kanata.json): pinned Kanata source and GPL notice checksums.
- [upstream-notices.json](upstream-notices.json): original notice locations,
  checksums, and recorded gaps for dependency packages.
- [SOURCES.md](SOURCES.md): source and rebuild instructions copied into bundles.

[generate-license-notices.py](../generate-license-notices.py) reads these inputs
and writes the full reports to ignored `dist/licenses/`. See the
[release guide](../../docs/releasing.md) for regeneration and review steps.
