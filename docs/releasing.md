# Prepare executable bundles

This guide is for maintainers building a zip to install on another computer.
For installing Zack on your own machine, use the [installation guide](installation.md).
Packaging creates files under ignored `dist/`; it does not upload them.

## Prepare the tools and license materials

Use a macOS or Linux build machine with the platform build tools described in
the installation guide, plus **Python 3.11+**, Cargo, and **cargo-about 0.9.2**.
Install Python through [python.org](https://www.python.org/downloads/) on macOS
or `sudo apt install python3` on Ubuntu 24.04/Debian 12. Verify `python3 --version`
reports at least 3.11. Install and check the pinned notice generator:

```sh
cargo install --locked --version 0.9.2 cargo-about
cargo-about --version
python3 scripts/generate-license-notices.py
```

Run commands from the Zack repository root. Generation downloads the pinned
Kanata source and any missing upstream notices, verifies their checksums, and
writes the reports to `dist/licenses/`. Cargo downloads locked dependencies as
needed. Verified archives are reused from `dist/.license-source-cache/`.
An existing matching Kanata source tree can be supplied with
`--kanata-source /path/to/kanata-source`.

Review `dist/licenses/zack.html`, the Kanata HTML reports, and `inventory.json`.
The small, reviewed inputs live in `scripts/license-config/`; generated reports,
font notices, and downloaded licenses stay out of Git. Regenerate whenever
manifests, lockfiles, license inputs, or build/packaging scripts change. Review
new dependencies and missing notices before updating the recorded inputs.

## Build the package

Linux/Windows packages require Docker, Bash, curl, unzip and `sha256sum` on the
build machine. These are available after the Linux setup above. A Mac can also
assemble them with Docker Desktop and GNU coreutils (`sha256sum`) installed.
For macOS packages, build natively on a Mac first:

```sh
scripts/build-macos.sh
scripts/make-bundle.sh macos "$(uname -m)"
```

For Linux or Windows:

```sh
scripts/make-bundle.sh linux
scripts/make-bundle.sh windows
```

Each command writes `dist/zack-bundle-<platform>.zip` and its extracted folder.
The package includes installation instructions, notices, accompanying source
archives, and a `bundle-manifest.json` with hashes of its contents. Packaging
stops for stale inputs, missing/changed notices, mismatched Kanata versions,
or corrupt source archives. Keep the notices and sources with any bundle you
redistribute. See the bundle's `sources/README.md` for offline Kanata rebuilds.

Test the actual zip on each target OS before releasing it, including a fresh
installation, login startup, Caps Lock behavior, and the chooser. Generation
and packaging checks do not replace native execution tests.

## Report coverage

Zack's report covers direct, transitive and build dependencies from its locked
Cargo graph across the four targets in `scripts/license-config/about.toml`.
Development-only dependencies are excluded. This is a conservative inventory,
not a measurement of which functions survive linking. Kanata reports cover
the pinned Linux default build and Windows GUI winIOv2 build bundled by Zack.

The generator preserves original package notices and bundled-font licenses.
`scripts/license-config/upstream-notices.json` records notices recovered from
upstream source revisions with URLs and checksums. Where an exact original
notice was unavailable, its record says so; standard terms and declared authors
are retained without inventing a copyright holder or year. Bundles include the
exact Zack source crates for those cases and for MPL-2.0 components.

Linux/Windows bundles also contain Kanata's matching source and every locked
registry dependency, its LGPL/GPL terms, and supplemental Keyberon, GUI and icon
notices. Kanata remains a separate executable with its own license. Zack's MIT
license applies to Zack's original code; see [third-party credits](../THIRD-PARTY-NOTICES.md).
