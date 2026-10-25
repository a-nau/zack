//! App-context key parsing: trigger key names ("1", "x", "f9") and target
//! chords ("ctrl+shift+e"). Pure and unit-tested; injection lives in actions.

use global_hotkey::hotkey::Code;

/// A parsed target chord to inject into the focused app.
#[derive(Clone, Debug, PartialEq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// Final key name, lowercase (letter, digit, f1-f12, grave, tab, …).
    pub key: String,
}

/// Hyper+<name> registration code for a trigger key: a-z, 0-9, f1-f12, [, ].
pub fn trigger_code(name: &str) -> Option<Code> {
    let lower = name.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix('f') {
        if let Ok(n) = rest.parse::<u8>() {
            return Some(match n {
                1 => Code::F1,
                2 => Code::F2,
                3 => Code::F3,
                4 => Code::F4,
                5 => Code::F5,
                6 => Code::F6,
                7 => Code::F7,
                8 => Code::F8,
                9 => Code::F9,
                10 => Code::F10,
                11 => Code::F11,
                12 => Code::F12,
                13 => Code::F13,
                14 => Code::F14,
                15 => Code::F15,
                16 => Code::F16,
                17 => Code::F17,
                18 => Code::F18,
                19 => Code::F19,
                20 => Code::F20,
                21 => Code::F21,
                22 => Code::F22,
                23 => Code::F23,
                24 => Code::F24,
                _ => return None,
            });
        }
    }
    let mut chars = lower.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    Some(match c {
        'a' => Code::KeyA,
        'b' => Code::KeyB,
        'c' => Code::KeyC,
        'd' => Code::KeyD,
        'e' => Code::KeyE,
        'f' => Code::KeyF,
        'g' => Code::KeyG,
        'h' => Code::KeyH,
        'i' => Code::KeyI,
        'j' => Code::KeyJ,
        'k' => Code::KeyK,
        'l' => Code::KeyL,
        'm' => Code::KeyM,
        'n' => Code::KeyN,
        'o' => Code::KeyO,
        'p' => Code::KeyP,
        'q' => Code::KeyQ,
        'r' => Code::KeyR,
        's' => Code::KeyS,
        't' => Code::KeyT,
        'u' => Code::KeyU,
        'v' => Code::KeyV,
        'w' => Code::KeyW,
        'x' => Code::KeyX,
        'y' => Code::KeyY,
        'z' => Code::KeyZ,
        '1' => Code::Digit1,
        '2' => Code::Digit2,
        '3' => Code::Digit3,
        '4' => Code::Digit4,
        '5' => Code::Digit5,
        '6' => Code::Digit6,
        '7' => Code::Digit7,
        '8' => Code::Digit8,
        '9' => Code::Digit9,
        '0' => Code::Digit0,
        '[' => Code::BracketLeft,
        ']' => Code::BracketRight,
        _ => return None,
    })
}

/// Parse "ctrl+shift+e" / "ctrl+grave" / "ctrl+shift+alt+d".
pub fn parse_chord(s: &str) -> Result<Chord, String> {
    let mut chord = Chord {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        key: String::new(),
    };
    let tokens: Vec<&str> = s.split('+').map(str::trim).collect();
    let (mods, key) = tokens.split_at(tokens.len().saturating_sub(1));
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => chord.ctrl = true,
            "alt" => chord.alt = true,
            "shift" => chord.shift = true,
            "win" | "super" | "meta" | "cmd" => chord.win = true,
            other => return Err(format!("unknown modifier '{other}' in '{s}'")),
        }
    }
    let key = key.first().copied().unwrap_or("").to_ascii_lowercase();
    if key.is_empty() {
        return Err(format!("empty key in chord '{s}'"));
    }
    if vk_name_valid(&key) {
        chord.key = key;
        Ok(chord)
    } else {
        Err(format!("unknown key '{key}' in chord '{s}'"))
    }
}

/// Names accepted as a chord's final key (must stay in sync with the
/// virtual-key table in actions.rs).
fn vk_name_valid(key: &str) -> bool {
    if trigger_code(key).is_some() {
        return true;
    }
    matches!(
        key,
        "grave"
            | "`"
            | "tab"
            | "enter"
            | "escape"
            | "esc"
            | "space"
            | "backspace"
            | "delete"
            | "home"
            | "end"
            | "pageup"
            | "pagedown"
            | "left"
            | "right"
            | "up"
            | "down"
            | "comma"
            | "period"
            | "minus"
            | "equals"
            | "slash"
            | "backslash"
            | "semicolon"
            | "quote"
            | "lbracket"
            | "rbracket"
            | "caps"
            | "capslock"
            // macOS pseudo-key: fires the system dictation shortcut
            // (see actions::mac_tap_dictation_shortcut).
            | "dictation"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_codes_cover_letters_digits_fkeys() {
        assert_eq!(trigger_code("x"), Some(Code::KeyX));
        assert_eq!(trigger_code("1"), Some(Code::Digit1));
        assert_eq!(trigger_code("f9"), Some(Code::F9));
        assert_eq!(trigger_code("f24"), Some(Code::F24));
        assert_eq!(trigger_code("f25"), None);
        assert_eq!(trigger_code("ab"), None);
        assert_eq!(trigger_code(""), None);
    }

    #[test]
    fn parses_chords() {
        let c = parse_chord("ctrl+shift+e").unwrap();
        assert!(c.ctrl && c.shift && !c.alt && !c.win);
        assert_eq!(c.key, "e");
        let c = parse_chord("ctrl+grave").unwrap();
        assert_eq!(c.key, "grave");
        let c = parse_chord("ctrl+shift+alt+d").unwrap();
        assert!(c.ctrl && c.shift && c.alt);
        assert_eq!(parse_chord("ctrl+f4").unwrap().key, "f4");
        assert_eq!(parse_chord("capslock").unwrap().key, "capslock");
        assert_eq!(parse_chord("dictation").unwrap().key, "dictation");
    }

    #[test]
    fn rejects_bad_chords() {
        assert!(parse_chord("hyper+x").is_err());
        assert!(parse_chord("ctrl+").is_err());
        assert!(parse_chord("ctrl+nosuchkey").is_err());
    }
}
