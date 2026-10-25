# Launcher source

| File | Contents |
|---|---|
| [main.rs](main.rs) | Application entry point, event loop, chooser UI and hotkey registration |
| [mappings.rs](mappings.rs) | Configuration schema, validation and private-overlay merging |
| [actions.rs](actions.rs) | Launching apps, opening links and paths, and platform actions |
| [chords.rs](chords.rs) | Trigger keys and injected key chords |
| [logic.rs](logic.rs) | Chooser commands and fuzzy filtering |
| [placement.rs](placement.rs) | Monitor selection and chooser placement |

Unit tests live alongside the code. Run `cargo test --locked` from `launcher/`.
