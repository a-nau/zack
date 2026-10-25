//! Schema and loading of `mappings.json` — the single shared config for all OSes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Mappings {
    #[serde(default = "default_cursor_cli")]
    pub cursor_cli: String,
    #[serde(default)]
    pub modes: BTreeMap<String, Mode>,
    #[serde(default)]
    pub folders: BTreeMap<String, Folder>,
    #[serde(default)]
    pub commands: BTreeMap<String, CommandEntry>,
    #[serde(default)]
    pub hotkeys: BTreeMap<String, Hotkey>,
    /// App-specific Hyper+key chords, active only while a matching app is
    /// focused (Windows and macOS). Take precedence over `hotkeys`.
    #[serde(default)]
    pub contexts: Vec<Context>,
}

/// One application context: which processes it applies to and what each
/// Hyper+key sends into the focused app.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// Config self-documentation only; never read by the program.
    #[serde(default)]
    #[allow(dead_code)]
    pub label: String,
    #[serde(rename = "match")]
    pub match_spec: MatchSpec,
    /// Trigger key ("1", "x", "f9") -> chord to inject or URL to open.
    pub keys: BTreeMap<String, ContextAction>,
}

/// App-context actions stay compact for the common chord case while also
/// allowing deep links such as Slack conversations.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum ContextAction {
    Chord(String),
    /// Preserve the trigger's press/release lifetime instead of tapping the
    /// chord immediately. Useful for native hold-to-talk shortcuts.
    Hold {
        hold: String,
    },
    Url {
        url: String,
    },
}

impl std::fmt::Display for ContextAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextAction::Chord(chord) => f.write_str(chord),
            ContextAction::Hold { hold } => write!(f, "hold {hold}"),
            ContextAction::Url { url } => write!(f, "open {url}"),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchSpec {
    /// Process image names, e.g. "Code.exe" (case-insensitive).
    #[serde(default)]
    pub windows: Vec<String>,
    /// macOS localized application names, e.g. "Visual Studio Code", or
    /// bundle identifiers, e.g. "com.microsoft.VSCode" (case-insensitive).
    #[serde(default)]
    pub macos: Vec<String>,
}

impl Context {
    fn matches(names: &[String], identity_lowercase: &str) -> bool {
        names.iter().any(|name| {
            name == "*"
                || identity_lowercase
                    .split('|')
                    .any(|identity| name.to_lowercase() == identity)
        })
    }

    /// Contexts are evaluated in order; "*" matches any app on that OS, so a
    /// trailing per-OS catch-all context provides global fallback chords.
    pub fn matches_current_os(&self, identity_lowercase: &str) -> bool {
        if cfg!(target_os = "macos") {
            Self::matches(&self.match_spec.macos, identity_lowercase)
        } else if cfg!(windows) {
            Self::matches(&self.match_spec.windows, identity_lowercase)
        } else {
            false
        }
    }

    pub fn available_on_current_os(&self) -> bool {
        if cfg!(target_os = "macos") {
            !self.match_spec.macos.is_empty()
        } else if cfg!(windows) {
            !self.match_spec.windows.is_empty()
        } else {
            false
        }
    }

    #[cfg(test)]
    fn matches_windows(&self, identity_lowercase: &str) -> bool {
        Self::matches(&self.match_spec.windows, identity_lowercase)
    }

    #[cfg(test)]
    fn matches_macos(&self, identity_lowercase: &str) -> bool {
        Self::matches(&self.match_spec.macos, identity_lowercase)
    }
}

fn default_cursor_cli() -> String {
    "cursor".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mode {
    pub label: String,
    /// Search URL template containing `{query}`.
    pub url: String,
}

/// A filesystem path that is either portable ("~/Downloads") or per-OS.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum PathSpec {
    Portable(String),
    PerOs {
        linux: Option<String>,
        macos: Option<String>,
        windows: Option<String>,
    },
}

impl PathSpec {
    pub fn for_current_os(&self) -> Option<&str> {
        match self {
            PathSpec::Portable(p) => Some(p),
            PathSpec::PerOs {
                linux,
                macos,
                windows,
            } => {
                let value = if cfg!(target_os = "macos") {
                    macos
                } else if cfg!(target_os = "windows") {
                    windows
                } else {
                    linux
                };
                value.as_deref()
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Folder {
    pub label: String,
    pub path: Option<PathSpec>,
    pub url: Option<String>,
    /// A `vscode-remote://` URI (opened with `cursor --folder-uri`) or a
    /// local path (opened with `cursor <path>`).
    pub cursor_uri: Option<String>,
}

/// Closed set of builtin actions selectable from the `commands` map.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinAction {
    CopyDate,
    Reload,
    OpenConfig,
    OpenConfigDir,
    /// Sanity check: dump visible windows (image/class) + context state to the log.
    Diag,
    /// Pull, build, and reinstall a Windows repo-mode installation.
    Update,
    /// Toggle macOS dictation in the app the chooser returns focus to.
    Dictation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEntry {
    pub label: String,
    pub action: BuiltinAction,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hotkey {
    pub label: String,
    pub url: Option<String>,
    pub app: Option<AppSpec>,
    /// Open a folder/file (portable or per-OS path), e.g. "~/Downloads".
    pub path: Option<PathSpec>,
}

/// Per-OS launch command. macOS: an `open -a` application name.
/// Linux: a shell command. Windows: a `cmd /C start` command.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSpec {
    pub macos: Option<String>,
    pub linux: Option<String>,
    pub windows: Option<String>,
    /// Windows activate-before-launch matcher: a process image name
    /// ("chrome.exe") or a window class ("CabinetWClass"). Defaults to the
    /// first token of `windows` + ".exe".
    #[cfg_attr(not(windows), allow(dead_code))]
    pub windows_match: Option<String>,
    /// Optional Windows chord sent after the app window is focused, including
    /// after waiting for a newly launched app to create its first window.
    pub windows_focus_chord: Option<String>,
    /// Optional macOS chord sent after `open -a` has focused the app.
    pub macos_focus_chord: Option<String>,
}

impl AppSpec {
    pub fn for_current_os(&self) -> Option<&str> {
        if cfg!(target_os = "macos") {
            self.macos.as_deref()
        } else if cfg!(target_os = "windows") {
            self.windows.as_deref()
        } else {
            self.linux.as_deref()
        }
    }

    /// What to look for among open windows before launching (Windows only;
    /// dead code elsewhere, but unit-tested on all platforms).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn windows_matcher(&self) -> Option<String> {
        if let Some(explicit) = &self.windows_match {
            return Some(explicit.clone());
        }
        let command = self.windows.as_deref()?.trim();
        // First token, honoring a quoted full path; then its basename.
        let token = if let Some(rest) = command.strip_prefix('"') {
            rest.split('"').next().unwrap_or(rest)
        } else {
            command.split_whitespace().next()?
        };
        let base = token.rsplit(['\\', '/']).next().unwrap_or(token);
        if base.is_empty() {
            return None;
        }
        if base.to_ascii_lowercase().ends_with(".exe") {
            Some(base.to_string())
        } else {
            Some(format!("{base}.exe"))
        }
    }
}

impl Hotkey {
    /// A hotkey is usable on this OS if it opens a URL (portable), has an
    /// app command for the current OS, or a path for the current OS.
    /// Unusable hotkeys are not registered.
    pub fn available_on_current_os(&self) -> bool {
        self.url.is_some()
            || self
                .app
                .as_ref()
                .is_some_and(|a| a.for_current_os().is_some())
            || self
                .path
                .as_ref()
                .is_some_and(|p| p.for_current_os().is_some())
    }
}

impl Mappings {
    /// Action for `key` in the first matching context that defines it.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn context_action_for(
        &self,
        identity_lowercase: &str,
        key: &str,
    ) -> Option<&ContextAction> {
        self.contexts
            .iter()
            .filter(|context| context.matches_current_os(identity_lowercase))
            .find_map(|context| context.keys.get(key))
    }

    #[cfg(test)]
    fn context_action_for_windows(&self, identity: &str, key: &str) -> Option<&ContextAction> {
        self.contexts
            .iter()
            .filter(|context| context.matches_windows(identity))
            .find_map(|context| context.keys.get(key))
    }

    #[cfg(test)]
    fn context_action_for_macos(&self, identity: &str, key: &str) -> Option<&ContextAction> {
        self.contexts
            .iter()
            .filter(|context| context.matches_macos(identity))
            .find_map(|context| context.keys.get(key))
    }

    /// Chord for `key` in the first matching context that defines it, so a
    /// specific app context can override individual keys of a "*" catch-all.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn context_chord_for(&self, image_lowercase: &str, key: &str) -> Option<&str> {
        match self.context_action_for(image_lowercase, key) {
            Some(ContextAction::Chord(chord)) => Some(chord.as_str()),
            _ => None,
        }
    }

    #[cfg(test)]
    fn context_chord_for_windows(&self, identity: &str, key: &str) -> Option<&str> {
        match self.context_action_for_windows(identity, key) {
            Some(ContextAction::Chord(chord)) => Some(chord.as_str()),
            _ => None,
        }
    }

    #[cfg(test)]
    fn context_chord_for_macos(&self, identity: &str, key: &str) -> Option<&str> {
        match self.context_action_for_macos(identity, key) {
            Some(ContextAction::Chord(chord)) => Some(chord.as_str()),
            _ => None,
        }
    }

    /// Parse and validate a single config in one step. Only tests use this
    /// directly now; runtime loading merges base + overlay before validating.
    #[cfg(test)]
    pub fn from_json(s: &str) -> Result<Self, String> {
        let m = Self::parse_json(s)?;
        m.validate()?;
        Ok(m)
    }

    /// Parse without validating — used so a base + private overlay can be
    /// merged first and the combined result validated once.
    fn parse_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }

    /// Overlay `other` onto `self`. Each map merges by key with the overlay
    /// winning on collision (so a private entry can override a public one).
    /// Overlay contexts are prepended so any app context they add takes
    /// precedence over the base catch-all. A non-default `cursor_cli` wins.
    pub fn merge(&mut self, other: Mappings) {
        self.modes.extend(other.modes);
        self.folders.extend(other.folders);
        self.commands.extend(other.commands);
        self.hotkeys.extend(other.hotkeys);
        let mut contexts = other.contexts;
        contexts.append(&mut self.contexts);
        self.contexts = contexts;
        if other.cursor_cli != default_cursor_cli() {
            self.cursor_cli = other.cursor_cli;
        }
    }

    /// Load the base config at `path`, then overlay an optional private
    /// `mappings.local.json` sitting beside it, and validate the result.
    pub fn load(path: &Path) -> Result<Self, String> {
        let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut base = Self::parse_json(&s)?;
        if let Some(overlay_path) = local_overlay_path(path) {
            if overlay_path.exists() {
                let os = std::fs::read_to_string(&overlay_path)
                    .map_err(|e| format!("{}: {e}", overlay_path.display()))?;
                base.merge(Self::parse_json(&os)?);
            }
        }
        base.validate()?;
        Ok(base)
    }

    fn validate(&self) -> Result<(), String> {
        for (key, mode) in &self.modes {
            if !mode.url.contains("{query}") {
                return Err(format!("mode '{key}': url must contain {{query}}"));
            }
            if key == "f" {
                return Err("mode key 'f' is reserved for the folders mode".into());
            }
        }
        for (key, folder) in &self.folders {
            let targets = [
                folder.path.is_some(),
                folder.url.is_some(),
                folder.cursor_uri.is_some(),
            ]
            .iter()
            .filter(|t| **t)
            .count();
            if targets != 1 {
                return Err(format!(
                    "folder '{key}': needs exactly one of path/url/cursor_uri, has {targets}"
                ));
            }
            if let Some(PathSpec::PerOs {
                linux,
                macos,
                windows,
            }) = &folder.path
            {
                if linux.is_none() && macos.is_none() && windows.is_none() {
                    return Err(format!("folder '{key}': per-OS path has no OS values"));
                }
            }
        }
        for (key, hotkey) in &self.hotkeys {
            let targets = [
                hotkey.url.is_some(),
                hotkey.app.is_some(),
                hotkey.path.is_some(),
            ]
            .iter()
            .filter(|t| **t)
            .count();
            if targets != 1 {
                return Err(format!(
                    "hotkey '{key}': needs exactly one of url/app/path, has {targets}"
                ));
            }
            if let Some(app) = &hotkey.app {
                if app.linux.is_none() && app.macos.is_none() && app.windows.is_none() {
                    return Err(format!("hotkey '{key}': app has no OS values"));
                }
                if let Some(chord) = &app.windows_focus_chord {
                    if app.windows.is_none() {
                        return Err(format!(
                            "hotkey '{key}': windows_focus_chord needs a windows app command"
                        ));
                    }
                    crate::chords::parse_chord(chord)
                        .map_err(|e| format!("hotkey '{key}', windows_focus_chord: {e}"))?;
                }
                if let Some(chord) = &app.macos_focus_chord {
                    if app.macos.is_none() {
                        return Err(format!(
                            "hotkey '{key}': macos_focus_chord needs a macos app command"
                        ));
                    }
                    crate::chords::parse_chord(chord)
                        .map_err(|e| format!("hotkey '{key}', macos_focus_chord: {e}"))?;
                }
            }
            if key.len() != 1
                || !key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            {
                return Err(format!(
                    "hotkey '{key}': key must be a single letter a-z or digit 0-9"
                ));
            }
        }
        for (index, context) in self.contexts.iter().enumerate() {
            if context.match_spec.windows.is_empty() && context.match_spec.macos.is_empty() {
                return Err(format!("context #{index}: match has no app names"));
            }
            for (trigger, action) in &context.keys {
                if crate::chords::trigger_code(trigger).is_none() {
                    return Err(format!(
                        "context #{index}: '{trigger}' is not a valid trigger key"
                    ));
                }
                match action {
                    ContextAction::Chord(chord) | ContextAction::Hold { hold: chord } => {
                        crate::chords::parse_chord(chord)
                            .map_err(|e| format!("context #{index}, key '{trigger}': {e}"))?;
                    }
                    ContextAction::Url { url } if url.trim().is_empty() => {
                        return Err(format!(
                            "context #{index}, key '{trigger}': url must not be empty"
                        ));
                    }
                    ContextAction::Url { .. } => {}
                }
            }
        }
        Ok(())
    }
}

/// The private overlay path for a base config: `<stem>.local.<ext>` beside
/// it (e.g. `mappings.json` -> `mappings.local.json`). Gitignored; optional.
fn local_overlay_path(base: &Path) -> Option<PathBuf> {
    let parent = base.parent()?;
    let stem = base.file_stem()?.to_str()?;
    let ext = base.extension().and_then(|e| e.to_str()).unwrap_or("json");
    Some(parent.join(format!("{stem}.local.{ext}")))
}

/// `<config_dir>/zack/mappings.json` if present, else the path from
/// `<config_dir>/zack/config-path.txt` (repo-mode pointer) if present,
/// else `mappings.json` next to the executable, else the (possibly not yet
/// existing) config path.
pub fn config_file() -> PathBuf {
    let cfg_dir = dirs::config_dir().map(|d| d.join("zack"));
    let preferred = cfg_dir.as_ref().map(|d| d.join("mappings.json"));
    if let Some(p) = &preferred {
        if p.exists() {
            return p.clone();
        }
    }
    // Repo-mode: install writes a pointer file instead of copying mappings.json.
    if let Some(dir) = &cfg_dir {
        let pointer = dir.join("config-path.txt");
        if let Ok(target) = std::fs::read_to_string(&pointer) {
            let p = PathBuf::from(target.trim());
            if p.exists() {
                return p;
            }
        }
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mappings.json")));
    match beside_exe {
        Some(p) if p.exists() => p,
        _ => preferred.unwrap_or_else(|| PathBuf::from("mappings.json")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "cursor_cli": "cursor",
      "modes": { "g": { "label": "Google", "url": "https://www.google.com/search?q={query}" } },
      "folders": {
        "down": { "label": "Downloads", "path": "~/Downloads" },
        "docs": { "label": "Documents", "path": { "linux": "~/Documents", "windows": "C:\\Users\\me\\Documents" } },
        "gem":  { "label": "Gemini", "url": "https://gemini.google.com/app" },
        "project": { "label": "Example project", "cursor_uri": "vscode-remote://ssh-remote+dev@example.com/home/dev/project" }
      },
      "commands": {
        "date": { "label": "Copy current date", "action": "copy_date" }
      },
      "hotkeys": {
        "f": { "label": "Chrome", "app": { "macos": "Google Chrome", "linux": "google-chrome", "windows": "chrome" } },
        "h": { "label": "Example dashboard", "url": "https://example.com/dashboard" },
        "n": { "label": "NoMachine", "app": { "macos": "NoMachine" } }
      }
    }"#;

    #[test]
    fn parses_sample() {
        let m = Mappings::from_json(SAMPLE).unwrap();
        assert_eq!(m.modes["g"].label, "Google");
        assert_eq!(
            m.folders["down"].path.as_ref().unwrap().for_current_os(),
            Some("~/Downloads")
        );
        assert!(matches!(m.commands["date"].action, BuiltinAction::CopyDate));
        assert_eq!(
            m.hotkeys["f"].app.as_ref().unwrap().linux.as_deref(),
            Some("google-chrome")
        );
    }

    #[test]
    fn per_os_path_resolves_or_hides() {
        let m = Mappings::from_json(SAMPLE).unwrap();
        let docs = m.folders["docs"].path.as_ref().unwrap();
        #[cfg(target_os = "linux")]
        assert_eq!(docs.for_current_os(), Some("~/Documents"));
        #[cfg(target_os = "macos")]
        assert_eq!(docs.for_current_os(), None);
    }

    #[test]
    fn hotkey_availability_respects_current_os() {
        let m = Mappings::from_json(SAMPLE).unwrap();
        assert!(m.hotkeys["h"].available_on_current_os()); // url: everywhere
        #[cfg(target_os = "linux")]
        assert!(!m.hotkeys["n"].available_on_current_os()); // macos-only app
    }

    #[test]
    fn global_hotkeys_accept_digits() {
        let m = Mappings::from_json(
            r#"{"hotkeys":{"1":{"label":"Journal","url":"https://example.com/journal"}}}"#,
        )
        .unwrap();
        assert_eq!(m.hotkeys["1"].label, "Journal");
    }

    #[test]
    fn windows_matcher_derives_or_uses_explicit() {
        let derived: AppSpec =
            serde_json::from_str(r#"{"windows":"chrome --new-window"}"#).unwrap();
        assert_eq!(derived.windows_matcher().as_deref(), Some("chrome.exe"));
        let with_ext: AppSpec = serde_json::from_str(r#"{"windows":"notepad.exe"}"#).unwrap();
        assert_eq!(with_ext.windows_matcher().as_deref(), Some("notepad.exe"));
        let quoted: AppSpec =
            serde_json::from_str(r#"{"windows":"\"C:\\Program Files\\Foo\\Bar.exe\" --flag"}"#)
                .unwrap();
        assert_eq!(quoted.windows_matcher().as_deref(), Some("Bar.exe"));
        let explicit: AppSpec =
            serde_json::from_str(r#"{"windows":"wt","windows_match":"WindowsTerminal.exe"}"#)
                .unwrap();
        assert_eq!(
            explicit.windows_matcher().as_deref(),
            Some("WindowsTerminal.exe")
        );
        let none: AppSpec = serde_json::from_str(r#"{"macos":"Finder"}"#).unwrap();
        assert_eq!(none.windows_matcher(), None);
    }

    #[test]
    fn validates_platform_focus_chords() {
        let valid = r#"{"hotkeys":{"w":{"label":"Browser first tab","app":{"windows":"chrome","windows_focus_chord":"ctrl+1"}}}}"#;
        Mappings::from_json(valid).unwrap();

        let invalid = r#"{"hotkeys":{"w":{"label":"Bad chord","app":{"windows":"chrome","windows_focus_chord":"ctrl+not-a-key"}}}}"#;
        assert!(Mappings::from_json(invalid).is_err());

        let missing_windows = r#"{"hotkeys":{"w":{"label":"Wrong OS","app":{"linux":"chrome","windows_focus_chord":"ctrl+1"}}}}"#;
        assert!(Mappings::from_json(missing_windows).is_err());

        let mac = r#"{"hotkeys":{"w":{"label":"Browser first tab","app":{"macos":"Google Chrome","macos_focus_chord":"cmd+1"}}}}"#;
        Mappings::from_json(mac).unwrap();

        let missing_macos = r#"{"hotkeys":{"w":{"label":"Wrong OS","app":{"linux":"chrome","macos_focus_chord":"cmd+1"}}}}"#;
        assert!(Mappings::from_json(missing_macos).is_err());
    }

    #[test]
    fn context_layering_specific_over_catchall() {
        let m = Mappings::from_json(include_str!("../mappings.json")).unwrap();
        // In the editor: x = close tab; elsewhere: x = close window (Alt+F4).
        assert_eq!(
            m.context_chord_for_windows("code.exe", "x"),
            Some("ctrl+f4")
        );
        assert_eq!(
            m.context_chord_for_windows("chrome.exe", "x"),
            Some("alt+f4")
        );
        // Key defined only in the specific context: no catch-all fallback noise.
        assert_eq!(
            m.context_chord_for_windows("code.exe", "9"),
            Some("ctrl+grave")
        );
        assert_eq!(m.context_chord_for_windows("chrome.exe", "9"), None);
        // Public Slack navigation stays available; workspace destinations belong
        // in the local overlay, whose URL-action support is tested below.
        assert_eq!(m.context_chord_for_macos("slack", "1"), Some("ctrl+1"));
        assert_eq!(m.context_action_for_macos("slack", "0"), None);
    }

    #[test]
    fn context_supports_url_actions() {
        let m = Mappings::from_json(
            r#"{"contexts":[{"match":{"windows":["chat.exe"]},"keys":{"8":{"url":"chat://conversation/example"}}}]}"#,
        )
        .unwrap();
        assert_eq!(
            m.context_action_for_windows("chat.exe", "8"),
            Some(&ContextAction::Url {
                url: "chat://conversation/example".into(),
            })
        );
        assert_eq!(
            m.context_action_for_windows("chat.exe", "8")
                .and_then(|action| match action {
                    ContextAction::Chord(chord) => Some(chord.as_str()),
                    _ => None,
                }),
            None
        );
    }

    #[test]
    fn context_supports_held_chords() {
        let m = Mappings::from_json(include_str!("../mappings.json")).unwrap();
        assert_eq!(
            m.context_action_for_windows("chatgpt.exe", "d"),
            Some(&ContextAction::Hold {
                hold: "ctrl+shift+d".into(),
            })
        );
        assert_eq!(
            m.context_action_for_windows("chatgpt.exe", "d")
                .and_then(|action| match action {
                    ContextAction::Chord(chord) => Some(chord.as_str()),
                    _ => None,
                }),
            None
        );
    }

    #[test]
    fn rejects_misspelled_field() {
        let bad = r#"{"modes":{},"hotkeys":{},
          "folders":{"x":{"label":"x","paht":"/a"}}}"#;
        assert!(Mappings::from_json(bad).is_err());
    }

    #[test]
    fn rejects_empty_per_os_path_and_app() {
        let bad = r#"{"modes":{},"hotkeys":{},
          "folders":{"x":{"label":"x","path":{}}}}"#;
        assert!(Mappings::from_json(bad).is_err());
        let bad = r#"{"modes":{},"folders":{},
          "hotkeys":{"x":{"label":"x","app":{}}}}"#;
        assert!(Mappings::from_json(bad).is_err());
    }

    #[test]
    fn rejects_folder_with_two_targets() {
        let bad = r#"{"modes":{},"hotkeys":{},
          "folders":{"x":{"label":"x","path":"/a","url":"https://b"}}}"#;
        assert!(Mappings::from_json(bad).is_err());
    }

    #[test]
    fn rejects_mode_without_query_placeholder() {
        let bad = r#"{"folders":{},"hotkeys":{},
          "modes":{"g":{"label":"G","url":"https://g.com/"}}}"#;
        assert!(Mappings::from_json(bad).is_err());
    }

    #[test]
    fn ships_valid_example_mappings() {
        Mappings::from_json(include_str!("../mappings.example.json")).unwrap();
    }

    #[test]
    fn ships_valid_default_mappings() {
        let m = Mappings::from_json(include_str!("../mappings.json")).unwrap();
        assert!(
            m.folders.len() >= 12,
            "expected >=12 folders, got {}",
            m.folders.len()
        );
        assert!(m.modes.len() >= 6);
        assert!(m.hotkeys.len() >= 9);
        assert!(m.commands.len() >= 4);
    }

    #[test]
    fn ships_expected_macos_navigation_bindings() {
        let m = Mappings::from_json(include_str!("../mappings.json")).unwrap();
        assert_eq!(
            m.context_chord_for_macos("terminal|com.apple.terminal", "["),
            Some("cmd+shift+lbracket")
        );
        assert_eq!(
            m.context_chord_for_macos("terminal|com.apple.terminal", "]"),
            Some("cmd+shift+rbracket")
        );
        assert_eq!(
            m.context_chord_for_macos("cursor|com.todesktop.230313mzl4w4u92", "["),
            Some("ctrl+pageup")
        );
        // Slack on macOS switches Home with Ctrl+1, while Activity has a
        // dedicated shortcut in the current desktop app.
        assert_eq!(
            m.context_chord_for_macos("slack|com.tinyspeck.slackmacgap", "1"),
            Some("ctrl+1")
        );
        assert_eq!(
            m.context_chord_for_macos("slack|com.tinyspeck.slackmacgap", "3"),
            Some("cmd+shift+m")
        );
        assert_eq!(
            m.context_chord_for_windows("slack.exe", "3"),
            Some("ctrl+shift+m")
        );
        assert_eq!(
            m.context_chord_for_macos("google chrome|com.google.chrome", "["),
            Some("ctrl+shift+tab")
        );
        assert_eq!(
            m.context_chord_for_macos("google chrome|com.google.chrome", "]"),
            Some("ctrl+tab")
        );
        // Chrome must not shadow Caps+W: private overlays use the global
        // hotkey to activate Example dashboard from every foreground app.
        assert!(m
            .context_action_for_macos("google chrome|com.google.chrome", "w")
            .is_none());
        // caps+d: system dictation toggle everywhere on macOS, except the
        // Codex app, where a held caps+d drives the app's own dictation
        // (its shortcut must be set to Ctrl+Option+Shift+D in Codex settings).
        assert_eq!(
            m.context_action_for_macos("finder|com.apple.finder", "d"),
            Some(&ContextAction::Chord("dictation".into()))
        );
        assert_eq!(
            m.context_action_for_macos("chatgpt|com.openai.codex", "d"),
            Some(&ContextAction::Hold {
                hold: "ctrl+alt+shift+d".into(),
            })
        );
        // Caps+F11 toggles native macOS full-screen mode.
        assert_eq!(
            m.context_chord_for_macos("finder|com.apple.finder", "f11"),
            Some("ctrl+cmd+f")
        );
        // Caps+F12 starts macOS's region capture and puts the result directly
        // on the clipboard instead of saving a file to the Desktop.
        assert_eq!(
            m.context_chord_for_macos("finder|com.apple.finder", "f12"),
            Some("ctrl+shift+cmd+4")
        );
    }

    #[test]
    fn ships_valid_local_overlay_example() {
        // The overlay parses on its own (partial config: no modes/commands).
        Mappings::parse_json(include_str!("../mappings.local.example.json")).unwrap();
    }

    #[test]
    fn merge_overlays_by_key_and_prepends_contexts() {
        let mut base = Mappings::from_json(include_str!("../mappings.json")).unwrap();
        let base_contexts = base.contexts.len();
        let overlay = Mappings::parse_json(
            r#"{
              "folders": { "priv": { "label": "Private", "url": "https://internal/" },
                           "docs": { "label": "Documents", "path": { "windows": "D:\\Docs" } } },
              "hotkeys": { "w": { "label": "Dash", "url": "https://internal/dash" } },
              "contexts": [ { "label": "priv app", "match": { "windows": ["Priv.exe"] },
                             "keys": { "1": "ctrl+p" } } ]
            }"#,
        )
        .unwrap();
        base.merge(overlay);
        base.validate().unwrap();
        // New private entries added.
        assert_eq!(base.folders["priv"].label, "Private");
        assert_eq!(base.hotkeys["w"].label, "Dash");
        // Overlay overrides a public key.
        assert_eq!(
            base.folders["docs"].path.as_ref().unwrap().for_current_os(),
            if cfg!(target_os = "windows") {
                Some("D:\\Docs")
            } else {
                None
            }
        );
        // Overlay context is prepended, ahead of the base catch-all.
        assert_eq!(base.contexts.len(), base_contexts + 1);
        assert_eq!(
            base.context_chord_for_windows("priv.exe", "1"),
            Some("ctrl+p")
        );
    }
}
