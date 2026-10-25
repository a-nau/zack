# Sources accompanying executable bundles

Zack bundles include the exact, unmodified crates.io source archives for the
MPL-2.0 components `nucleo-matcher` and `option-ext`, and for Zack dependencies
whose original standalone license notice was unavailable. Their licenses remain
in force for those files. They do not change the MIT license on Zack's own code.
Extract any `.crate` with a tar-compatible archive tool to inspect or modify it.

Windows/Linux bundles additionally contain the unmodified Kanata v1.11.0 source
at commit `4e6bec4d52d044bd13cfa01cea4e02dc2d246c65` and every registry dependency
in its Cargo.lock. The upstream archive includes its build scripts, manifests,
resources and CI build commands. `manifest.json` records every included archive's
SHA-256 checksum. No network source download is needed after unpacking.

Kanata is a separate LGPL-3.0 executable. You may modify it and replace the
bundled or installed Kanata executable. Zack does not link Kanata into its own
binary. The original LGPL and referenced GPL terms are in `licenses/upstream/`.

## Rebuild Kanata

With Python 3.9+, run from this bundle's `sources` directory, choosing a directory
that does not already exist:

```sh
python3 unpack-kanata-source.py /path/to/new-directory
```

The helper verifies all archive checksums, extracts the original project, and
creates a Cargo vendor directory with an offline source configuration. Change
into the printed `kanata-<commit>` directory. Use Rust and the native compiler/SDK
for the target platform; system toolchains are not included in the source bundle.

For the Linux x86-64 executable:

```sh
cargo build --release --locked --offline --target x86_64-unknown-linux-gnu
```

For the Windows x86-64 GUI winIOv2 executable, use the MSVC toolchain:

```sh
cargo build --release --locked --offline --target x86_64-pc-windows-msvc --features gui,win_manifest,win_sendinput_send_scancodes,win_llhook_read_scancodes
```

The generated executable is in `target/<target>/release/`. Preserve the source
and license materials alongside bundles you redistribute. A modified Kanata or
different build requires its matching source, build details and notices; do not
reuse this pinned inventory for a different binary. These commands reproduce
the selected target/features, not a guarantee of byte-for-byte identity with
upstream release executables built with another compiler environment.
