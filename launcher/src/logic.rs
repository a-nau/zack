//! Pure chooser logic: screens, rows, fuzzy filtering, URL building.
//! No I/O and no egui types — everything here is unit-testable.

use std::collections::{BTreeMap, BTreeSet};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

use crate::mappings::{BuiltinAction, ContextAction, Hotkey, Mappings};

#[derive(Clone, PartialEq)]
pub enum Screen {
    Root,
    /// Search mode; the String is the mode key ("g", "gs", ...).
    Search(String),
    /// General finder: everything launchable (folders + direct hotkeys),
    /// tagged by kind. Entered with `f<space>`.
    Find,
    /// Overview of all direct Caps key bindings (type "?" at the root).
    Help,
    /// Visual keyboard showing effective bindings for the previously focused
    /// application.
    Keyboard,
}

#[derive(Clone, Debug)]
pub enum Action {
    EnterMode(String),
    EnterFind,
    ShowHelp,
    ShowKeyboard,
    OpenPath(String),
    OpenUrl(String),
    OpenCursor(String),
    /// Launch a direct hotkey by its key ("f", "s", …) from the finder.
    LaunchHotkey(String),
    Builtin(BuiltinAction),
}

pub struct Row {
    /// Short kind tag ("dir", "web", "ide", "app"); "" = no chip.
    pub tag: String,
    pub title: String,
    pub subtext: String,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardBinding {
    pub label: String,
    pub detail: String,
    pub contextual: bool,
}

pub fn new_matcher() -> Matcher {
    Matcher::new(Config::DEFAULT)
}

/// "g " -> Some("g"): exactly one word followed by exactly one trailing space.
pub fn mode_prefix(query: &str) -> Option<&str> {
    let prefix = query.strip_suffix(' ')?;
    if prefix.is_empty() || prefix.contains(' ') {
        return None;
    }
    Some(prefix)
}

/// Percent-encode everything outside RFC 3986 unreserved characters.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn build_search_url(template: &str, query: &str) -> String {
    template.replace("{query}", &urlencode(query))
}

/// Kind tag + target text for a direct hotkey, for display and finder rows.
fn hotkey_tag_target(hotkey: &Hotkey) -> (&'static str, String) {
    if let Some(url) = &hotkey.url {
        ("web", url.clone())
    } else if let Some(app) = hotkey.app.as_ref().and_then(|a| a.for_current_os()) {
        ("app", app.to_string())
    } else if let Some(path) = hotkey.path.as_ref().and_then(|p| p.for_current_os()) {
        ("dir", path.to_string())
    } else {
        ("", String::new())
    }
}

/// The general finder list: every launchable target, tagged by kind. Folders
/// (dir/web/ide) plus the direct Caps hotkeys (web/app/dir) so `find` is a
/// superset you can fuzzy-search in one place.
fn find_rows(mappings: &Mappings) -> Vec<Row> {
    let mut rows: Vec<Row> = mappings
        .folders
        .iter()
        .filter_map(|(key, folder)| {
            let (tag, subtext, action) = if let Some(path) = &folder.path {
                // Hide entries with no path for this OS.
                let p = path.for_current_os()?;
                ("dir", p.to_string(), Action::OpenPath(p.to_string()))
            } else if let Some(url) = &folder.url {
                ("web", url.clone(), Action::OpenUrl(url.clone()))
            } else {
                let uri = folder.cursor_uri.clone().unwrap_or_default();
                ("ide", format!("Cursor: {uri}"), Action::OpenCursor(uri))
            };
            Some(Row {
                tag: tag.into(),
                title: format!("{key} — {}", folder.label),
                subtext,
                action,
            })
        })
        .collect();
    for (key, hotkey) in &mappings.hotkeys {
        if !hotkey.available_on_current_os() {
            continue;
        }
        let (tag, subtext) = hotkey_tag_target(hotkey);
        rows.push(Row {
            tag: tag.into(),
            title: format!("{key} — {}", hotkey.label),
            subtext,
            action: Action::LaunchHotkey(key.clone()),
        });
    }
    rows
}

fn fuzzy_filter(rows: Vec<Row>, query: &str, matcher: &mut Matcher) -> Vec<Row> {
    if query.trim().is_empty() {
        return rows;
    }
    let pattern = Pattern::parse(query.trim(), CaseMatching::Ignore, Normalization::Smart);
    let mut scored: Vec<(u32, Row)> = rows
        .into_iter()
        .filter_map(|row| {
            let haystack = format!("{} {} {}", row.tag, row.title, row.subtext);
            let mut buf = Vec::new();
            let utf32 = nucleo_matcher::Utf32Str::new(&haystack, &mut buf);
            pattern.score(utf32, matcher).map(|score| (score, row))
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, row)| row).collect()
}

/// One row per direct Caps binding: hotkeys, platform builtins, app-context
/// chords. Rows are informational (their action just keeps the help open).
fn help_rows(mappings: &Mappings) -> Vec<Row> {
    let info = |tag: &str, title: String, subtext: String| Row {
        tag: tag.into(),
        title,
        subtext,
        action: Action::ShowHelp,
    };
    let mut rows = vec![info(
        "",
        "Space — open/close this launcher".into(),
        "single-tap Caps = nothing; double-tap = Esc".into(),
    )];
    for (key, hotkey) in &mappings.hotkeys {
        if !hotkey.available_on_current_os() {
            continue;
        }
        let (tag, target) = hotkey_tag_target(hotkey);
        rows.push(info(
            tag,
            format!("{} — {}", key.to_uppercase(), hotkey.label),
            target,
        ));
    }
    if cfg!(any(windows, target_os = "macos")) {
        for (key, description) in [
            (
                "Up / Down",
                "fill / exit full screen or minimize the active window",
            ),
            (
                "Left / Right",
                "move the active window to the previous/next monitor",
            ),
            (
                "O",
                if cfg!(target_os = "macos") {
                    "Command-Tab app switcher"
                } else {
                    "Alt-Tab switcher: tap to cycle, release Caps to pick"
                },
            ),
            ("P", "Ctrl-Tab (next tab in the focused app)"),
        ] {
            rows.push(info("", format!("{key} — {description}"), "builtin".into()));
        }
        for context in mappings
            .contexts
            .iter()
            .filter(|context| context.available_on_current_os())
        {
            let scope = if context.label.is_empty() {
                if cfg!(target_os = "macos") {
                    context.match_spec.macos.join(", ")
                } else {
                    context.match_spec.windows.join(", ")
                }
            } else {
                context.label.clone()
            };
            for (key, chord) in &context.keys {
                rows.push(info(
                    "",
                    format!("{} — {chord}", key.to_uppercase()),
                    scope.clone(),
                ));
            }
        }
    }
    rows
}

/// Effective Caps bindings for the visual keyboard. App-context actions
/// replace global hotkeys on the same key, matching runtime dispatch.
pub fn keyboard_bindings(
    mappings: &Mappings,
    identity: Option<&str>,
) -> BTreeMap<String, KeyboardBinding> {
    let mut bindings = BTreeMap::new();
    bindings.insert(
        "caps".into(),
        KeyboardBinding {
            label: "Hyper / ×2 Esc".into(),
            detail: "Hold for Zack shortcuts; double-tap for Escape".into(),
            contextual: false,
        },
    );
    bindings.insert(
        "space".into(),
        KeyboardBinding {
            label: "Launcher".into(),
            detail: "Open or close Zack".into(),
            contextual: false,
        },
    );
    for (key, hotkey) in &mappings.hotkeys {
        if !hotkey.available_on_current_os() {
            continue;
        }
        let (_, target) = hotkey_tag_target(hotkey);
        bindings.insert(
            key.clone(),
            KeyboardBinding {
                label: hotkey.label.clone(),
                detail: target,
                contextual: false,
            },
        );
    }
    if cfg!(any(windows, target_os = "macos")) {
        for (key, label, detail) in [
            ("up", "Fill", "Fill or maximize the active window"),
            (
                "down",
                "Exit / Min",
                "Exit full screen or minimize the active window",
            ),
            ("left", "Prev display", "Move to the previous display"),
            ("right", "Next display", "Move to the next display"),
            ("o", "App switcher", "Switch applications"),
            ("p", "Next tab", "Switch to the next tab"),
        ] {
            bindings.insert(
                key.into(),
                KeyboardBinding {
                    label: label.into(),
                    detail: detail.into(),
                    contextual: false,
                },
            );
        }
        if let Some(identity) = identity {
            let keys: BTreeSet<&str> = mappings
                .contexts
                .iter()
                .flat_map(|context| context.keys.keys().map(String::as_str))
                .collect();
            for key in keys {
                let Some(action) = mappings.context_action_for(identity, key) else {
                    continue;
                };
                let label = match action {
                    ContextAction::Chord(chord) => chord.clone(),
                    ContextAction::Hold { hold } => format!("hold {hold}"),
                    ContextAction::Url { .. } => "Open link".into(),
                };
                bindings.insert(
                    key.into(),
                    KeyboardBinding {
                        label,
                        detail: format!("App override: {action}"),
                        contextual: true,
                    },
                );
            }
        }
    }
    bindings
}

pub fn rows_for(
    screen: &Screen,
    mappings: &Mappings,
    query: &str,
    matcher: &mut Matcher,
) -> Vec<Row> {
    match screen {
        Screen::Root => {
            let mut rows: Vec<Row> = mappings
                .modes
                .iter()
                .map(|(key, mode)| Row {
                    tag: String::new(),
                    title: format!("{key} — {} search", mode.label),
                    subtext: format!("type {key}<space>"),
                    action: Action::EnterMode(key.clone()),
                })
                .collect();
            rows.push(Row {
                tag: String::new(),
                title: "f — Find (folders, apps, links…)".into(),
                subtext: "type f<space>".into(),
                action: Action::EnterFind,
            });
            rows.push(Row {
                tag: String::new(),
                title: "? — All Caps key bindings".into(),
                subtext: "type ?".into(),
                action: Action::ShowHelp,
            });
            rows.push(Row {
                tag: String::new(),
                title: "kbd — Keyboard view".into(),
                subtext: "effective bindings for the active app".into(),
                action: Action::ShowKeyboard,
            });
            rows.extend(mappings.commands.iter().map(|(key, command)| Row {
                tag: String::new(),
                title: format!("{key} — {}", command.label),
                subtext: String::new(),
                action: Action::Builtin(command.action),
            }));
            fuzzy_filter(rows, query, matcher)
        }
        Screen::Find => fuzzy_filter(find_rows(mappings), query, matcher),
        Screen::Help => fuzzy_filter(help_rows(mappings), query, matcher),
        Screen::Keyboard => Vec::new(),
        Screen::Search(mode_key) => {
            let Some(mode) = mappings.modes.get(mode_key) else {
                return Vec::new();
            };
            let trimmed = query.trim();
            if trimmed.is_empty() {
                return vec![Row {
                    tag: String::new(),
                    title: format!("{}: type a query…", mode.label),
                    subtext: String::new(),
                    action: Action::EnterMode(mode_key.clone()),
                }];
            }
            vec![Row {
                tag: String::new(),
                title: format!("{}: {trimmed}", mode.label),
                subtext: "Enter to search".into(),
                action: Action::OpenUrl(build_search_url(&mode.url, trimmed)),
            }]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::Mappings;

    fn mappings() -> Mappings {
        Mappings::from_json(include_str!("../mappings.json")).unwrap()
    }

    #[test]
    fn mode_prefix_needs_single_trailing_space() {
        assert_eq!(mode_prefix("g "), Some("g"));
        assert_eq!(mode_prefix("gs "), Some("gs"));
        assert_eq!(mode_prefix("g"), None);
        assert_eq!(mode_prefix("g  "), None);
        assert_eq!(mode_prefix(" g "), None);
        assert_eq!(mode_prefix(""), None);
    }

    #[test]
    fn urlencodes_reserved_chars() {
        assert_eq!(urlencode("a b&c=ä"), "a%20b%26c%3D%C3%A4");
        assert_eq!(urlencode("safe-._~AZaz09"), "safe-._~AZaz09");
    }

    #[test]
    fn builds_search_url() {
        assert_eq!(
            build_search_url("https://g.com/?q={query}", "rust egui"),
            "https://g.com/?q=rust%20egui"
        );
    }

    #[test]
    fn root_rows_contain_modes_commands_and_filter() {
        let m = mappings();
        let mut matcher = new_matcher();
        let all = rows_for(&Screen::Root, &m, "", &mut matcher);
        assert!(all.iter().any(|r| r.title.starts_with("g —")));
        assert!(all.iter().any(|r| r.title.starts_with("date —")));
        let filtered = rows_for(&Screen::Root, &m, "dat", &mut matcher);
        assert!(filtered.iter().any(|r| matches!(
            r.action,
            Action::Builtin(crate::mappings::BuiltinAction::CopyDate)
        )));
        assert!(filtered.len() < all.len());
    }

    #[test]
    fn find_rows_filter_fuzzily_and_tag_by_kind() {
        let m = mappings();
        let mut matcher = new_matcher();
        let rows = rows_for(&Screen::Find, &m, "dwn", &mut matcher);
        assert!(rows
            .iter()
            .any(|r| r.title.contains("down —") && r.tag == "dir"));
    }

    #[test]
    fn find_includes_launchable_hotkeys() {
        let m = mappings();
        let mut matcher = new_matcher();
        let rows = rows_for(&Screen::Find, &m, "chrome", &mut matcher);
        // The Chrome hotkey ("f") is launchable from the finder, tagged "app".
        assert!(rows
            .iter()
            .any(|r| matches!(&r.action, Action::LaunchHotkey(k) if k == "f") && r.tag == "app"));
    }

    #[test]
    fn folders_without_current_os_path_are_hidden() {
        let m = mappings();
        let mut matcher = new_matcher();
        let rows = rows_for(&Screen::Find, &m, "", &mut matcher);
        // "apps" has only macos+windows paths, so it must be absent on Linux.
        #[cfg(target_os = "linux")]
        assert!(!rows.iter().any(|r| r.title.contains("apps —")));
        // Portable entries are always present.
        assert!(rows.iter().any(|r| r.title.contains("down —")));
    }

    #[test]
    fn search_screen_yields_single_query_row() {
        let m = mappings();
        let mut matcher = new_matcher();
        let rows = rows_for(&Screen::Search("g".into()), &m, "hello world", &mut matcher);
        assert_eq!(rows.len(), 1);
        match &rows[0].action {
            Action::OpenUrl(u) => assert!(u.ends_with("q=hello%20world")),
            other => panic!("unexpected action {other:?}"),
        }
    }

    #[test]
    fn help_rows_list_direct_bindings() {
        let m = mappings();
        let mut matcher = new_matcher();
        let rows = rows_for(&Screen::Help, &m, "", &mut matcher);
        assert!(rows.iter().any(|r| r.title.starts_with("Space —")));
        // url hotkey (S — ChatGPT): present on every OS, tagged "web".
        assert!(rows
            .iter()
            .any(|r| r.title.contains("S — ChatGPT") && r.tag == "web"));
        #[cfg(any(windows, target_os = "macos"))]
        assert!(rows.iter().any(|r| r.title.contains("Tab")));
    }

    #[test]
    fn root_offers_help_entry() {
        let m = mappings();
        let mut matcher = new_matcher();
        let rows = rows_for(&Screen::Root, &m, "", &mut matcher);
        assert!(rows.iter().any(|r| matches!(r.action, Action::ShowHelp)));
        assert!(rows
            .iter()
            .any(|r| matches!(r.action, Action::ShowKeyboard)));
    }

    #[test]
    fn keyboard_bindings_apply_context_overrides() {
        let m = mappings();
        let bindings = keyboard_bindings(&m, Some("google chrome|com.google.chrome"));
        assert_eq!(bindings["f"].label, "Chrome");
        #[cfg(target_os = "macos")]
        {
            assert_eq!(bindings["]"].label, "ctrl+tab");
            assert!(bindings["]"].contextual);
            assert_eq!(bindings["f11"].label, "ctrl+cmd+f");
        }
    }
}
