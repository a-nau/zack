# Third-party components

The [MIT license](LICENSE) applies to Zack's original code. Dependencies and
upstream material retain their own licenses and copyright notices.

- Zack uses Rust libraries including egui/eframe, global-hotkey and
  nucleo-matcher. Exact versions are locked in `launcher/Cargo.lock`.
- egui's bundled fonts include Hack/Bitstream Vera, Noto Emoji, Ubuntu and the
  emoji icon font, each with its own license.
- `nucleo-matcher` and `option-ext` use MPL-2.0. Executable bundles include their
  exact source archives.
- On Windows/Linux, [Kanata](https://github.com/jtroo/kanata/tree/v1.11.0) runs as
  a separate LGPL-3.0 executable. Bundles include its notices, matching source,
  locked dependency sources and rebuild instructions.

Dependency reports are generated into ignored `dist/licenses/` when preparing
executable bundles. They are not checked into the source repository. The
[release guide](docs/releasing.md) explains generation and report coverage;
`scripts/make-bundle.sh` attaches the resulting notices and verified sources.

## Earlier launcher

Zack follows the interaction model of the author's earlier
[AutoHotkey launcher](https://github.com/a-nau/AutoHotKey-GUI-and-Commands), a fork
of [plul/Public-AutoHotKey-Scripts](https://github.com/plul/Public-AutoHotKey-Scripts).
The source distribution contains no AutoHotkey or Lua files. The earlier
project's MIT notice is retained below to acknowledge that lineage.

MIT License

Copyright (c) 2021 Asger Juul Brunshøj

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
