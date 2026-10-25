//! Side effects: opening paths/URLs, launching apps, builtin commands.
//! Everything returns Result so the UI can show failures instead of hiding.

use std::process::Command;

use crate::logic::Action;
use crate::mappings::{AppSpec, BuiltinAction, Hotkey, Mappings};

/// Outcomes the egui layer must handle itself (they need UI context).
pub enum AppSignal {
    CopyDate(String),
    Reload,
    /// Show this text in the status line (window stays open).
    Status(String),
}

/// Append to `<config_dir>/zack/zack.log` and stderr. The Windows GUI
/// subsystem has no console, so the file is the only trace there.
pub fn log_line(message: &str) {
    eprintln!("zack: {message}");
    if let Some(dir) = dirs::config_dir().map(|d| d.join("zack")) {
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("zack.log"))
        {
            use std::io::Write;
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(f, "{now} {message}");
        }
    }
}

pub fn expand_home(path: &str) -> String {
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().into_owned();
        }
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    path.to_string()
}

/// Failures from already-detached processes (opener exited nonzero, …).
/// Logged immediately; the UI drains them into the status line.
static ASYNC_ERRORS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn report_async(message: &str) {
    log_line(message);
    if let Ok(mut queue) = ASYNC_ERRORS.lock() {
        queue.push(message.to_string());
    }
}

pub fn take_async_error() -> Option<String> {
    ASYNC_ERRORS.lock().ok().and_then(|mut queue| {
        if queue.is_empty() {
            None
        } else {
            Some(queue.remove(0))
        }
    })
}

fn open_detached(target: &str) -> Result<(), String> {
    let context = target.to_string();
    let handle = open::that_in_background(target);
    // Watcher thread: reaps the opener and surfaces nonzero exits.
    std::thread::spawn(move || match handle.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => report_async(&format!("open {context}: {e}")),
        Err(_) => report_async(&format!("open {context}: opener thread panicked")),
    });
    Ok(())
}

/// Open an app or web deep link without blocking the hotkey event loop.
#[cfg(any(windows, target_os = "macos"))]
pub fn open_url(target: &str) -> Result<(), String> {
    open_detached(target)
}

fn spawn(program: &str, args: &[&str]) -> Result<(), String> {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("run {program} {args:?}: {e}"))?;
    let context = format!("{program} {args:?}");
    // Reaper thread: no zombies, and nonzero exits become visible.
    std::thread::spawn(move || match child.wait() {
        Ok(status) if !status.success() => {
            report_async(&format!("{context}: exited with {status}"))
        }
        Err(e) => report_async(&format!("{context}: wait failed: {e}")),
        Ok(_) => {}
    });
    Ok(())
}

/// Find a visible top-level window by process image name ("chrome.exe") or
/// window class ("CabinetWClass") and bring it to the foreground.
#[cfg(windows)]
mod win_activate {
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow,
        SW_MINIMIZE, SW_RESTORE,
    };

    struct Search {
        /// Lowercase matcher; ".exe" suffix => process image, else class name.
        matcher: String,
        toggle_if_focused: bool,
        activated: bool,
    }

    pub(super) fn window_process_image(hwnd: HWND) -> Option<String> {
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buffer = [0u16; 4096];
            let mut length = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buffer[..length as usize]);
            path.rsplit('\\').next().map(|base| base.to_lowercase())
        }
    }

    fn window_class(hwnd: HWND) -> String {
        unsafe {
            let mut buffer = [0u16; 256];
            let length = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
            String::from_utf16_lossy(&buffer[..length.max(0) as usize]).to_lowercase()
        }
    }

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> i32 {
        let search = unsafe { &mut *(lparam as *mut Search) };
        // Only visible, titled windows — skips the many invisible helper
        // windows processes like Chrome keep around.
        if unsafe { IsWindowVisible(hwnd) } == 0 || unsafe { GetWindowTextLengthW(hwnd) } == 0 {
            return 1;
        }
        let matched = if search.matcher.ends_with(".exe") {
            window_process_image(hwnd).as_deref() == Some(search.matcher.as_str())
        } else {
            window_class(hwnd) == search.matcher
        };
        if matched {
            // Activate inside the callback: the HWND is guaranteed live here,
            // not after enumeration returns (review finding).
            unsafe {
                if hwnd == GetForegroundWindow() {
                    if search.toggle_if_focused {
                        // Ordinary app hotkeys toggle away on a second press.
                        ShowWindow(hwnd, SW_MINIMIZE);
                    }
                    search.activated = true;
                } else {
                    if IsIconic(hwnd) != 0 {
                        ShowWindow(hwnd, SW_RESTORE);
                    }
                    search.activated = SetForegroundWindow(hwnd) != 0;
                }
            }
            return 0; // stop enumeration
        }
        1
    }

    /// Diagnostics: all visible, titled top-level windows as (image, class).
    pub fn list_windows() -> Vec<(String, String)> {
        struct Collect(Vec<(String, String)>);
        unsafe extern "system" fn collect_cb(hwnd: HWND, lparam: LPARAM) -> i32 {
            let out = unsafe { &mut *(lparam as *mut Collect) };
            if unsafe { IsWindowVisible(hwnd) } != 0 && unsafe { GetWindowTextLengthW(hwnd) } != 0 {
                let image = window_process_image(hwnd).unwrap_or_else(|| "<unknown>".into());
                out.0.push((image, window_class(hwnd)));
            }
            1
        }
        let mut collect = Collect(Vec::new());
        unsafe {
            EnumWindows(Some(collect_cb), &mut collect as *mut Collect as LPARAM);
        }
        collect.0
    }

    pub fn activate(matcher: &str, toggle_if_focused: bool) -> bool {
        let mut search = Search {
            matcher: matcher.to_lowercase(),
            toggle_if_focused,
            activated: false,
        };
        unsafe {
            EnumWindows(Some(enum_callback), &mut search as *mut Search as LPARAM);
        }
        search.activated
    }
}

/// Foreground-window management and switching builtins.
#[cfg(any(windows, target_os = "macos"))]
#[derive(Clone, Copy, Debug)]
pub enum WindowCommand {
    /// Hyper+Up: maximize, or restore if already maximized.
    MaximizeToggle,
    /// Hyper+Down: exit native full screen on macOS when active; otherwise
    /// minimize.
    Minimize,
    /// Hyper+Left / Hyper+Right: move to the previous/next monitor,
    /// keeping the window's relative position.
    MonitorPrev,
    MonitorNext,
    /// Hyper+O: native app switcher. Repeated O taps cycle; macOS commits after
    /// a short idle timeout because kanata's virtual Hyper modifier state is
    /// not reliably observable between events.
    SwitchWindow,
    /// Hyper+P: Ctrl+Tab (next tab in the focused app) — Alt/Shift lifted
    /// around an injected Tab while Ctrl stays down.
    SwitchTab,
}

/// Inject key events; `true` = key-up. Modifiers re-pressed at the end keep
/// the physically-held Hyper consistent so repeated hotkey presses match.
#[cfg(windows)]
fn send_keys(keys: &[(u16, bool)]) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|(vk, up)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: *vk,
                    wScan: 0,
                    dwFlags: if *up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
        .collect();
    unsafe {
        let inserted = SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
        if inserted != inputs.len() as u32 {
            log_line(&format!(
                "SendInput injected {inserted}/{} events (blocked by UIPI/elevated window?)",
                inputs.len()
            ));
        }
    }
}

#[cfg(windows)]
pub fn manage_window(command: WindowCommand) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, IsZoomed, SetWindowPos, ShowWindow, SWP_NOACTIVATE,
        SWP_NOSIZE, SWP_NOZORDER, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return;
        }
        // VK codes: Tab 0x09, Shift 0x10, Ctrl 0x11, Alt 0x12.
        const TAB: u16 = 0x09;
        const SHIFT: u16 = 0x10;
        const CTRL: u16 = 0x11;
        const ALT: u16 = 0x12;
        match command {
            WindowCommand::SwitchWindow => {
                send_keys(&[
                    (CTRL, true),
                    (SHIFT, true), // leave only Alt held
                    (TAB, false),
                    (TAB, true), // Alt+Tab
                    (SHIFT, false),
                    (CTRL, false), // restore Hyper
                ]);
                return;
            }
            WindowCommand::SwitchTab => {
                send_keys(&[
                    (ALT, true),
                    (SHIFT, true), // leave only Ctrl held
                    (TAB, false),
                    (TAB, true), // Ctrl+Tab
                    (SHIFT, false),
                    (ALT, false), // restore Hyper
                ]);
                return;
            }
            _ => {}
        }
        match command {
            WindowCommand::MaximizeToggle => {
                ShowWindow(
                    hwnd,
                    if IsZoomed(hwnd) != 0 {
                        SW_RESTORE
                    } else {
                        SW_MAXIMIZE
                    },
                );
            }
            WindowCommand::Minimize => {
                ShowWindow(hwnd, SW_MINIMIZE);
            }
            // Handled (with early return) above.
            WindowCommand::SwitchWindow | WindowCommand::SwitchTab => {}
            WindowCommand::MonitorPrev | WindowCommand::MonitorNext => {
                let Ok(mut displays) = display_info::DisplayInfo::all() else {
                    return;
                };
                if displays.len() < 2 {
                    return;
                }
                displays.sort_by_key(|d| (d.x, d.y));
                let was_maximized = IsZoomed(hwnd) != 0;
                if was_maximized {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                let mut rect = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                if GetWindowRect(hwnd, &mut rect) == 0 {
                    return;
                }
                let center_x = (rect.left + rect.right) / 2;
                let center_y = (rect.top + rect.bottom) / 2;
                let current = displays
                    .iter()
                    .position(|d| {
                        center_x >= d.x
                            && center_x < d.x + d.width as i32
                            && center_y >= d.y
                            && center_y < d.y + d.height as i32
                    })
                    .unwrap_or(0);
                let step: isize = if matches!(command, WindowCommand::MonitorNext) {
                    1
                } else {
                    -1
                };
                let target = &displays
                    [(current as isize + step).rem_euclid(displays.len() as isize) as usize];
                let source = &displays[current];
                // Same relative position on the target monitor, same size.
                let rel_x = (rect.left - source.x) as f64 / source.width.max(1) as f64;
                let rel_y = (rect.top - source.y) as f64 / source.height.max(1) as f64;
                let new_x = target.x + (rel_x * target.width as f64) as i32;
                let new_y = target.y + (rel_y * target.height as f64) as i32;
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    new_x,
                    new_y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                if was_maximized {
                    ShowWindow(hwnd, SW_MAXIMIZE);
                }
            }
        }
    }
}

/// Lowercase image name ("code.exe") of the foreground window's process.
#[cfg(windows)]
pub fn foreground_image() -> Option<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        win_activate::window_process_image(hwnd)
    }
}

/// Lowercase identity of the currently focused app. On Windows this is the
/// executable image; on macOS it is the localized application name, falling
/// back to its bundle identifier.
#[cfg(windows)]
pub fn foreground_identity() -> Option<String> {
    foreground_image()
}

/// True while zack itself is the frontmost app — e.g. for a short moment
/// after the chooser hides, before macOS finishes returning focus. Context
/// chords must not be dispatched against that transient state.
#[cfg(target_os = "macos")]
pub fn foreground_is_self() -> bool {
    use objc2_app_kit::NSWorkspace;

    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .is_some_and(|app| app.processIdentifier() == std::process::id() as i32)
}

/// Make zack the frontmost app via the Accessibility API. Ordinary
/// activation requests are subject to macOS 14 "cooperative activation" and
/// get silently declined (AppKit still reports isActive=true!) because our
/// hotkey arrives via kanata's virtual keyboard and is not attributed to
/// zack. AX-driven activation bypasses those rules — and zack already holds
/// the Accessibility grant for chord injection.
#[cfg(target_os = "macos")]
pub fn force_self_frontmost() {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
        fn AXUIElementSetAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: CFTypeRef,
        ) -> i32;
    }
    unsafe {
        let app = AXUIElementCreateApplication(std::process::id() as i32);
        if app.is_null() {
            return;
        }
        let attribute = CFString::from_static_string("AXFrontmost");
        let error = AXUIElementSetAttributeValue(
            app,
            attribute.as_concrete_TypeRef(),
            CFBoolean::true_value().as_CFTypeRef(),
        );
        if error != 0 {
            log_line(&format!("AXFrontmost activation failed: AXError {error}"));
        }
        CFRelease(app);
    }
}

#[cfg(target_os = "macos")]
pub fn foreground_identity() -> Option<String> {
    use objc2_app_kit::NSWorkspace;

    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let name = app
        .localizedName()
        .map(|value| value.to_string().to_lowercase());
    let bundle = app
        .bundleIdentifier()
        .map(|value| value.to_string().to_lowercase());
    match (name, bundle) {
        (Some(name), Some(bundle)) => Some(format!("{name}|{bundle}")),
        (Some(name), None) => Some(name),
        (None, Some(bundle)) => Some(bundle),
        (None, None) => None,
    }
}

/// Inject a chord into the focused app: lift the held-Hyper modifiers the
/// chord doesn't use, send the requested key edge(s), then restore. Must stay
/// in sync with the key names accepted by chords::vk_name_valid.
#[cfg(windows)]
fn modifier_is_down(vk: u16) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
}

#[cfg(windows)]
fn send_chord_key_events(chord: &crate::chords::Chord, key_events: &[bool]) {
    const SHIFT: u16 = 0x10;
    const CTRL: u16 = 0x11;
    const ALT: u16 = 0x12;
    const LWIN: u16 = 0x5B;
    let Some(vk) = vk_for(&chord.key) else {
        log_line(&format!("no virtual-key mapping for '{}'", chord.key));
        return;
    };
    // Snapshot the REAL modifier state: the hotkey event is handled up to
    // ~50 ms after the keypress, so Hyper may already be (partly) released.
    // Adjust each modifier from its actual state to the chord's needs, then
    // restore the snapshot — no bare keys, no stuck modifiers.
    let mut sequence: Vec<(u16, bool)> = Vec::new();
    let mut restore: Vec<(u16, bool)> = Vec::new();
    for (modifier, wanted) in [
        (CTRL, chord.ctrl),
        (ALT, chord.alt),
        (SHIFT, chord.shift),
        (LWIN, chord.win),
    ] {
        let down = modifier_is_down(modifier);
        if down && !wanted {
            sequence.push((modifier, true)); // lift
            restore.push((modifier, false));
        } else if !down && wanted {
            sequence.push((modifier, false)); // press
            restore.push((modifier, true));
        }
    }
    sequence.extend(key_events.iter().map(|key_up| (vk, *key_up)));
    sequence.extend(restore.into_iter().rev());
    send_keys(&sequence);
}

#[cfg(windows)]
pub fn send_chord(chord: &crate::chords::Chord) {
    send_chord_key_events(chord, &[false, true]);
}

/// Start a held chord. The matching `release_chord` call emits only the key-up
/// edge, so the focused application sees a real hold duration.
#[cfg(windows)]
pub fn press_chord(chord: &crate::chords::Chord) {
    send_chord_key_events(chord, &[false]);
}

#[cfg(windows)]
pub fn release_chord(chord: &crate::chords::Chord) {
    send_chord_key_events(chord, &[true]);
}

#[cfg(windows)]
fn vk_for(key: &str) -> Option<u16> {
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        match c {
            'a'..='z' => return Some(c as u16 - 'a' as u16 + 0x41),
            '0'..='9' => return Some(c as u16 - '0' as u16 + 0x30),
            '`' => return Some(0xC0),
            _ => {}
        }
    }
    if let Some(rest) = key.strip_prefix('f') {
        if let Ok(n @ 1..=24) = rest.parse::<u16>() {
            return Some(0x70 + n - 1);
        }
    }
    Some(match key {
        "grave" => 0xC0,
        "caps" | "capslock" => 0x14,
        "tab" => 0x09,
        "enter" => 0x0D,
        "escape" | "esc" => 0x1B,
        "space" => 0x20,
        "backspace" => 0x08,
        "delete" => 0x2E,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "comma" => 0xBC,
        "period" => 0xBE,
        "minus" => 0xBD,
        "equals" => 0xBB,
        "slash" => 0xBF,
        "backslash" => 0xDC,
        "semicolon" => 0xBA,
        "quote" => 0xDE,
        "lbracket" => 0xDB,
        "rbracket" => 0xDD,
        _ => return None,
    })
}

/// Whether macOS trusts this process for Accessibility. Untrusted processes
/// have every posted CGEvent silently dropped — hotkeys dispatch and log but
/// nothing happens on screen. `prompt` shows the system dialog once, which
/// also adds the binary to the Accessibility list. NOTE: the ad-hoc code
/// signature changes on every rebuild, which invalidates a previous grant
/// even though its checkbox still looks enabled.
#[cfg(target_os = "macos")]
pub fn accessibility_trusted(prompt: bool) -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::CFString;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    }
    let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let value = if prompt {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0 }
}

#[cfg(target_os = "macos")]
fn mac_flags(chord: &crate::chords::Chord) -> core_graphics::event::CGEventFlags {
    use core_graphics::event::CGEventFlags;

    let mut flags = CGEventFlags::CGEventFlagNull;
    if chord.shift {
        flags.insert(CGEventFlags::CGEventFlagShift);
    }
    if chord.ctrl {
        flags.insert(CGEventFlags::CGEventFlagControl);
    }
    if chord.alt {
        flags.insert(CGEventFlags::CGEventFlagAlternate);
    }
    // The portable chord parser calls this field `win`; on macOS `super`,
    // `meta`, and `cmd` all intentionally mean Command.
    if chord.win {
        flags.insert(CGEventFlags::CGEventFlagCommand);
    }
    flags
}

#[cfg(target_os = "macos")]
const MAC_SYNTHETIC_EVENT_TAG: i64 = 0x5a41_434b; // "ZACK"

#[cfg(target_os = "macos")]
fn mac_send_keycode(keycode: u16, flags: core_graphics::event::CGEventFlags, key_events: &[bool]) {
    use core_graphics::event::{CGEvent, CGEventTapLocation, EventField};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    for key_up in key_events {
        let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            log_line("could not create macOS keyboard event source");
            return;
        };
        let Ok(event) = CGEvent::new_keyboard_event(source, keycode, !*key_up) else {
            log_line("could not create macOS keyboard event");
            return;
        };
        // Explicit flags replace the physically held Hyper flags, preventing
        // Ctrl+Option+Shift from leaking into the chord we are forwarding.
        event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, MAC_SYNTHETIC_EVENT_TAG);
        event.set_flags(flags);
        event.post(CGEventTapLocation::HID);
    }
}

/// Trigger the macOS dictation shortcut by double-tapping Right Command.
/// Requires System Settings -> Keyboard -> Dictation -> Shortcut =
/// "Press Right Command Key Twice". Only called with no Hyper held (see
/// dictation_tick), so the double-press is a clean gesture.
#[cfg(target_os = "macos")]
fn mac_tap_dictation_shortcut() {
    use core_graphics::event::CGEventFlags;

    const KEY_RCMD: u16 = 54;
    // Real Right Command events carry the device-specific right-side bit on
    // top of the Command mask; the "Press Right Command Key Twice" matcher
    // may require it to tell right from left.
    const NX_DEVICE_RCMD: u64 = 0x0000_0010;
    let rcmd_down = unsafe {
        CGEventFlags::from_bits_unchecked(CGEventFlags::CGEventFlagCommand.bits() | NX_DEVICE_RCMD)
    };
    // Modifier edges carry the state AFTER the edge: down = Command held,
    // up = nothing held — an up still flagged Command reads as "never
    // released" and the double-press pattern never completes. Human-ish
    // timing so the system's double-press detector accepts it.
    let press = std::time::Duration::from_millis(25);
    let gap = std::time::Duration::from_millis(90);
    for tap in 0..2 {
        if tap > 0 {
            std::thread::sleep(gap);
        }
        mac_send_keycode(KEY_RCMD, rcmd_down, &[false]);
        std::thread::sleep(press);
        mac_send_keycode(KEY_RCMD, CGEventFlags::CGEventFlagNull, &[true]);
    }
}

/// Whether the macOS dictation UI is on screen (a window owned by the
/// dictation input-method process). Stateless: survives the user stopping
/// dictation by other means.
#[cfg(target_os = "macos")]
fn dictation_ui_visible() -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::dictionary::CFDictionaryRef;
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::window::{
        copy_window_info, kCGNullWindowID, kCGWindowListOptionOnScreenOnly,
    };

    let Some(windows) = copy_window_info(kCGWindowListOptionOnScreenOnly, kCGNullWindowID) else {
        return false;
    };
    let owner_key = CFString::from_static_string("kCGWindowOwnerName");
    for index in 0..windows.len() {
        let Some(item) = windows.get(index) else {
            continue;
        };
        let dict = *item as CFDictionaryRef;
        if dict.is_null() {
            continue;
        }
        let mut value: *const std::ffi::c_void = std::ptr::null();
        let found = unsafe {
            core_foundation::dictionary::CFDictionaryGetValueIfPresent(
                dict,
                owner_key.as_CFTypeRef(),
                &mut value,
            )
        };
        if found == 0 || value.is_null() {
            continue;
        }
        let owner = unsafe { CFString::wrap_under_get_rule(value as CFStringRef) }.to_string();
        if owner.to_lowercase().contains("dictation") {
            return true;
        }
    }
    false
}

/// Caps+D toggle, queued until Caps/Hyper is released so the injected
/// gesture is always clean (kanata holds Ctrl+Alt+Shift while Caps is down).
#[cfg(target_os = "macos")]
static DICTATION_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "macos")]
pub fn request_dictation_toggle() {
    DICTATION_PENDING.store(true, std::sync::atomic::Ordering::SeqCst);
    dictation_tick(); // fires immediately when no Hyper is held
}

/// Runs from the UI poll loop: once no Hyper modifier is held, toggle —
/// Esc stops an active dictation session (a second double-Right-Command
/// does NOT stop it, observed), the shortcut starts one otherwise.
#[cfg(target_os = "macos")]
pub fn dictation_tick() {
    use core_graphics::event::CGEventFlags;
    use objc2_app_kit::{NSEvent, NSEventModifierFlags};

    if !DICTATION_PENDING.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let held = NSEvent::modifierFlags_class();
    if held.contains(NSEventModifierFlags::Control)
        || held.contains(NSEventModifierFlags::Option)
        || held.contains(NSEventModifierFlags::Shift)
    {
        return; // wait for the Caps release
    }
    DICTATION_PENDING.store(false, std::sync::atomic::Ordering::SeqCst);
    if dictation_ui_visible() {
        log_line("dictation: active -> stopping with Esc");
        const KEY_ESC: u16 = 53;
        mac_send_keycode(KEY_ESC, CGEventFlags::CGEventFlagNull, &[false, true]);
    } else {
        log_line("dictation: starting (double Right Command)");
        mac_tap_dictation_shortcut();
    }
}

#[cfg(target_os = "macos")]
fn mac_send_chord_key_events(chord: &crate::chords::Chord, key_events: &[bool]) {
    // Pseudo-key: each tap toggles dictation (start / stop), firing once
    // Caps is released so the injected gesture is clean.
    if chord.key == "dictation" {
        request_dictation_toggle();
        return;
    }
    let Some(keycode) = mac_key_for(&chord.key) else {
        log_line(&format!("no macOS keycode mapping for '{}'", chord.key));
        return;
    };
    mac_send_keycode(keycode, mac_flags(chord), key_events);
}

#[cfg(target_os = "macos")]
pub fn send_chord(chord: &crate::chords::Chord) {
    mac_send_chord_key_events(chord, &[false, true]);
}

#[cfg(target_os = "macos")]
pub fn press_chord(chord: &crate::chords::Chord) {
    mac_send_chord_key_events(chord, &[false]);
}

#[cfg(target_os = "macos")]
pub fn release_chord(chord: &crate::chords::Chord) {
    mac_send_chord_key_events(chord, &[true]);
}

#[cfg(target_os = "macos")]
fn mac_key_for(key: &str) -> Option<u16> {
    let keycode = match key {
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "v" => 9,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "equals" => 24,
        "9" => 25,
        "7" => 26,
        "minus" => 27,
        "8" => 28,
        "0" => 29,
        "rbracket" => 30,
        "o" => 31,
        "u" => 32,
        "lbracket" => 33,
        "i" => 34,
        "p" => 35,
        "enter" => 36,
        "l" => 37,
        "j" => 38,
        "quote" => 39,
        "k" => 40,
        "semicolon" => 41,
        "backslash" => 42,
        "comma" => 43,
        "slash" => 44,
        "n" => 45,
        "m" => 46,
        "period" => 47,
        "tab" => 48,
        "space" => 49,
        "grave" | "`" => 50,
        "backspace" => 51,
        "escape" | "esc" => 53,
        "caps" | "capslock" => 57,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        "f13" => 105,
        "f14" => 107,
        "f15" => 113,
        "f16" => 106,
        "f17" => 64,
        "f18" => 79,
        "f19" => 80,
        "f20" => 90,
        "home" => 115,
        "pageup" => 116,
        "delete" => 117,
        "end" => 119,
        "pagedown" => 121,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        _ => return None,
    };
    Some(keycode)
}

/// True while a synthetic Command press keeps the Cmd+Tab switcher open.
#[cfg(target_os = "macos")]
static SWITCHER_CMD_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "macos")]
static SWITCHER_LAST_O_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "macos")]
static SWITCHER_EVENT_TAP_LAST_O_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "macos")]
static SWITCHER_PENDING_CYCLES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// macOS-native Caps transport. `hidutil` maps physical Caps Lock to F18, an
/// otherwise-unused sentinel. The event tap below consumes F18 and adds Hyper
/// flags to ordinary keys while it is held. Unlike Kanata's IOHID device grab,
/// this leaves the MacBook function/media row entirely under macOS control.
#[cfg(target_os = "macos")]
static MAC_CAPS_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "macos")]
static MAC_CAPS_USED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "macos")]
static MAC_CAPS_DOWN_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "macos")]
static MAC_CAPS_LAST_TAP_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(u64::MAX);

#[cfg(target_os = "macos")]
static MAC_CAPS_ACTIVE_KEYS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<i64>>> =
    std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
std::thread_local! {
    /// CoreGraphics resets a source's accumulated modifier state when the
    /// source is released. Keep this source alive from Cmd-down through every
    /// Tab and the final Cmd-up so macOS sees one real switcher session.
    static SWITCHER_EVENT_SOURCE: std::cell::RefCell<Option<core_graphics::event_source::CGEventSource>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "macos")]
fn switcher_clock_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

#[cfg(any(target_os = "macos", test))]
const CAPS_TAP_TIMEOUT_MS: u64 = 200;

#[cfg(any(target_os = "macos", test))]
fn caps_tap_result(
    had_key_down: bool,
    used: bool,
    down_ms: u64,
    up_ms: u64,
    previous_tap_ms: u64,
) -> (bool, u64) {
    // Ignore duplicate/orphaned releases. A physical tap is exactly one
    // matched down→up pair; otherwise one macOS Caps tap can look like two.
    if !had_key_down {
        return (false, previous_tap_ms);
    }
    if used || up_ms.saturating_sub(down_ms) > CAPS_TAP_TIMEOUT_MS {
        return (false, u64::MAX);
    }
    if previous_tap_ms != u64::MAX && down_ms.saturating_sub(previous_tap_ms) <= CAPS_TAP_TIMEOUT_MS
    {
        (true, u64::MAX)
    } else {
        (false, up_ms)
    }
}

/// Convert the native F18 Caps sentinel into Hyper flags without grabbing the
/// keyboard device. This also observes O while the native Command-Tab overlay
/// is active. `manage_window` uses the timestamp to discard any duplicate
/// registered delivery.
#[cfg(target_os = "macos")]
pub fn start_switcher_event_tap() {
    if let Err(e) = std::thread::Builder::new()
        .name("zack-switcher-events".into())
        .spawn(|| {
            use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
            use core_graphics::event::{
                CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
                CGEventTapPlacement, CGEventType, EventField,
            };

            const KEY_O: i64 = 31;
            const KEY_ESC: u16 = 53;
            const KEY_CAPS_SENTINEL: i64 = 79; // kVK_F18
            let hyper = CGEventFlags::CGEventFlagControl
                | CGEventFlags::CGEventFlagAlternate
                | CGEventFlags::CGEventFlagShift;
            let tap = match CGEventTap::new(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                vec![CGEventType::KeyDown, CGEventType::KeyUp],
                move |_proxy, event_type, event| {
                    // Never transform keystrokes Zack injects itself (window
                    // commands, context chords, Cmd+Tab, dictation, etc.).
                    // They already carry their final modifiers; adding Hyper
                    // here would turn Cmd+Tab into Cmd+Ctrl+Option+Shift+Tab.
                    if event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA)
                        == MAC_SYNTHETIC_EVENT_TAG
                        || event.get_integer_value_field(EventField::EVENT_SOURCE_UNIX_PROCESS_ID)
                            == std::process::id() as i64
                    {
                        return None;
                    }
                    let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                    if keycode == KEY_CAPS_SENTINEL {
                        if matches!(event_type, CGEventType::KeyDown) {
                            if !MAC_CAPS_HELD.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                MAC_CAPS_USED.store(false, std::sync::atomic::Ordering::SeqCst);
                                MAC_CAPS_DOWN_MS.store(
                                    switcher_clock_ms(),
                                    std::sync::atomic::Ordering::SeqCst,
                                );
                            }
                        } else if matches!(event_type, CGEventType::KeyUp) {
                            let had_key_down =
                                MAC_CAPS_HELD.swap(false, std::sync::atomic::Ordering::SeqCst);
                            // Same event-tap thread that created the persistent
                            // source releases Command, exactly at physical
                            // Caps-up. No UI polling/timing race is involved.
                            release_switcher_if_held();
                            let now = switcher_clock_ms();
                            let previous_tap =
                                MAC_CAPS_LAST_TAP_MS.load(std::sync::atomic::Ordering::SeqCst);
                            let (send_escape, next_tap) = caps_tap_result(
                                had_key_down,
                                MAC_CAPS_USED.swap(false, std::sync::atomic::Ordering::SeqCst),
                                MAC_CAPS_DOWN_MS.load(std::sync::atomic::Ordering::SeqCst),
                                now,
                                previous_tap,
                            );
                            MAC_CAPS_LAST_TAP_MS
                                .store(next_tap, std::sync::atomic::Ordering::SeqCst);
                            if send_escape {
                                mac_send_keycode(
                                    KEY_ESC,
                                    CGEventFlags::CGEventFlagNull,
                                    &[false, true],
                                );
                            }
                        }
                        // F18 has no normal user-facing behavior, but turn the
                        // event into Null as well so it cannot reach an app.
                        event.set_type(CGEventType::Null);
                        return None;
                    }

                    if matches!(event_type, CGEventType::KeyDown) {
                        // Like Kanata's tap-dance, any other key interrupts a
                        // pending first tap instead of letting a later Caps
                        // press complete the double-tap.
                        MAC_CAPS_LAST_TAP_MS.store(u64::MAX, std::sync::atomic::Ordering::SeqCst);
                    }

                    let active_keys = MAC_CAPS_ACTIVE_KEYS
                        .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
                    // Handle the macOS switcher entirely in this physical-key
                    // callback. Passing through Carbon/global-hotkey first
                    // allowed quick Caps releases to commit before Cmd+Tab
                    // was posted. Null the physical O so it dispatches once.
                    let o_belongs_to_caps = match event_type {
                        CGEventType::KeyDown
                            if keycode == KEY_O
                                && MAC_CAPS_HELD.load(std::sync::atomic::Ordering::SeqCst) =>
                        {
                            MAC_CAPS_USED.store(true, std::sync::atomic::Ordering::SeqCst);
                            active_keys.lock().unwrap().insert(keycode);
                            let now = switcher_clock_ms();
                            SWITCHER_LAST_O_MS.store(now, std::sync::atomic::Ordering::SeqCst);
                            if !SWITCHER_CMD_HELD.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                log_line("switcher: native Caps event -> Cmd down + Tab");
                                switcher_send_keycode(55, &[false]);
                            } else {
                                log_line("switcher: native Caps event -> Tab");
                            }
                            switcher_send_keycode(48, &[false, true]);
                            true
                        }
                        CGEventType::KeyUp if keycode == KEY_O => {
                            active_keys.lock().unwrap().remove(&keycode)
                        }
                        _ => false,
                    };
                    if o_belongs_to_caps {
                        event.set_type(CGEventType::Null);
                        return None;
                    }

                    let key_belongs_to_caps = match event_type {
                        CGEventType::KeyDown
                            if MAC_CAPS_HELD.load(std::sync::atomic::Ordering::SeqCst) =>
                        {
                            MAC_CAPS_USED.store(true, std::sync::atomic::Ordering::SeqCst);
                            active_keys.lock().unwrap().insert(keycode);
                            true
                        }
                        CGEventType::KeyUp => active_keys.lock().unwrap().remove(&keycode),
                        _ => false,
                    };
                    if !key_belongs_to_caps {
                        return None;
                    }

                    event.set_flags(event.get_flags() | hyper);
                    if matches!(event_type, CGEventType::KeyDown)
                        && SWITCHER_CMD_HELD.load(std::sync::atomic::Ordering::SeqCst)
                        && keycode == KEY_O
                    {
                        let now = switcher_clock_ms();
                        SWITCHER_EVENT_TAP_LAST_O_MS
                            .store(now, std::sync::atomic::Ordering::SeqCst);
                        SWITCHER_LAST_O_MS.store(now, std::sync::atomic::Ordering::SeqCst);
                        SWITCHER_PENDING_CYCLES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        log_line("switcher: event tap queued one cycle");
                    }
                    None
                },
            ) {
                Ok(tap) => tap,
                Err(()) => {
                    log_line("switcher: could not start event tap; check Accessibility permission");
                    return;
                }
            };

            let run_loop = CFRunLoop::get_current();
            let source = match tap.mach_port.create_runloop_source(0) {
                Ok(source) => source,
                Err(()) => {
                    log_line("switcher: could not create event-tap run-loop source");
                    return;
                }
            };
            run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
            tap.enable();
            log_line("macOS Caps event tap ready (native transport + switcher deduplication)");
            CFRunLoop::run_current();
        })
    {
        log_line(&format!("switcher: event-tap thread failed: {e}"));
    }
}

#[cfg(target_os = "macos")]
fn switcher_send_keycode(keycode: u16, key_events: &[bool]) -> bool {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, EventField};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    SWITCHER_EVENT_SOURCE.with(|stored| {
        let mut stored = stored.borrow_mut();
        if stored.is_none() {
            let Ok(source) = CGEventSource::new(CGEventSourceStateID::CombinedSessionState) else {
                log_line("switcher: could not create persistent macOS event source");
                return false;
            };
            *stored = Some(source);
        }
        let source = stored.as_ref().expect("switcher source was initialized");
        for key_up in key_events {
            let Ok(event) = CGEvent::new_keyboard_event(source.clone(), keycode, !*key_up) else {
                log_line("switcher: could not create macOS keyboard event");
                return false;
            };
            event.set_integer_value_field(
                EventField::EVENT_SOURCE_USER_DATA,
                MAC_SYNTHETIC_EVENT_TAG,
            );
            event.set_flags(if keycode == 55 && *key_up {
                CGEventFlags::CGEventFlagNull
            } else {
                CGEventFlags::CGEventFlagCommand
            });
            event.post(CGEventTapLocation::HID);
        }
        true
    })
}

#[cfg(target_os = "macos")]
fn switcher_release_command() {
    if switcher_send_keycode(55, &[true]) {
        SWITCHER_EVENT_SOURCE.with(|stored| {
            stored.borrow_mut().take();
        });
    }
}

/// Commit after no O press has arrived for long enough to make another cycle
/// unlikely. Carbon does not expose the release of Caps/kanata's virtual Hyper
/// reliably, so inactivity is the deterministic commit signal.
#[cfg(target_os = "macos")]
pub fn switcher_tick() {
    if !SWITCHER_CMD_HELD.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let cycles = SWITCHER_PENDING_CYCLES.swap(0, std::sync::atomic::Ordering::SeqCst);
    for _ in 0..cycles {
        log_line("switcher: cycle exactly once (persistent Cmd + Tab)");
        switcher_send_keycode(48, &[false, true]);
    }
    if !MAC_CAPS_HELD.load(std::sync::atomic::Ordering::SeqCst) {
        log_line("switcher: Caps released -> commit (Cmd up)");
        release_switcher_if_held();
        return;
    }
    const IDLE_COMMIT_MS: u64 = 1_000;
    let idle_ms = switcher_clock_ms()
        .saturating_sub(SWITCHER_LAST_O_MS.load(std::sync::atomic::Ordering::SeqCst));
    if idle_ms >= IDLE_COMMIT_MS {
        log_line("switcher: idle -> commit (Cmd up)");
        switcher_release_command();
        SWITCHER_CMD_HELD.store(false, std::sync::atomic::Ordering::SeqCst);
        SWITCHER_LAST_O_MS.store(0, std::sync::atomic::Ordering::SeqCst);
        SWITCHER_EVENT_TAP_LAST_O_MS.store(0, std::sync::atomic::Ordering::SeqCst);
        SWITCHER_PENDING_CYCLES.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Safety net for shutdown/reload: never leave the synthetic Cmd stuck down.
#[cfg(target_os = "macos")]
pub fn release_switcher_if_held() {
    if SWITCHER_CMD_HELD.swap(false, std::sync::atomic::Ordering::SeqCst) {
        switcher_release_command();
        SWITCHER_LAST_O_MS.store(0, std::sync::atomic::Ordering::SeqCst);
        SWITCHER_EVENT_TAP_LAST_O_MS.store(0, std::sync::atomic::Ordering::SeqCst);
        SWITCHER_PENDING_CYCLES.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(target_os = "macos")]
pub fn manage_window(command: WindowCommand) {
    use core_graphics::event::CGEventFlags;

    const KEY_TAB: u16 = 48;
    const KEY_CMD: u16 = 55;
    let (keycode, flags) = match command {
        // macOS 15+: Fill and Minimize use their native shortcuts.
        WindowCommand::MaximizeToggle => (
            3, // F
            CGEventFlags::CGEventFlagControl | CGEventFlags::CGEventFlagSecondaryFn,
        ),
        WindowCommand::Minimize => {
            match mac_exit_fullscreen_if_needed() {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => log_line(&format!(
                    "could not inspect focused window's full-screen state: {e}"
                )),
            }
            (46, CGEventFlags::CGEventFlagCommand) // Cmd+M
        }
        WindowCommand::MonitorPrev | WindowCommand::MonitorNext => {
            if let Err(e) = mac_move_focused_window(matches!(command, WindowCommand::MonitorNext)) {
                log_line(&format!(
                    "could not move focused window between displays: {e}"
                ));
            }
            return;
        }
        WindowCommand::SwitchWindow => {
            // A tapped Cmd+Tab only flips to the previous app. Real switcher
            // semantics need Command genuinely held (Hyper has no Cmd), so:
            // first O press = synthetic Cmd down + Tab (switcher opens),
            // each further O cycles, then an idle timeout commits.
            let now = switcher_clock_ms();
            SWITCHER_LAST_O_MS.store(now, std::sync::atomic::Ordering::SeqCst);
            if !SWITCHER_CMD_HELD.swap(true, std::sync::atomic::Ordering::SeqCst) {
                log_line("switcher: open (synthetic Cmd down + Tab)");
                switcher_send_keycode(KEY_CMD, &[false]);
            } else {
                const DUPLICATE_WINDOW_MS: u64 = 100;
                let event_tap_ms =
                    SWITCHER_EVENT_TAP_LAST_O_MS.load(std::sync::atomic::Ordering::SeqCst);
                if event_tap_ms != 0 && now.saturating_sub(event_tap_ms) <= DUPLICATE_WINDOW_MS {
                    log_line("switcher: duplicate registered delivery ignored");
                    return;
                }
                log_line("switcher: cycle from registered hotkey (persistent Cmd + Tab)");
            }
            switcher_send_keycode(KEY_TAB, &[false, true]);
            return;
        }
        WindowCommand::SwitchTab => (KEY_TAB, CGEventFlags::CGEventFlagControl),
    };
    mac_send_keycode(keycode, flags, &[false, true]);
}

/// Exit native macOS full-screen mode for the focused window. Returns `true`
/// only when the window was full-screen and the AX state change succeeded, so
/// Caps+Down can retain its ordinary minimize behavior for all other windows.
#[cfg(target_os = "macos")]
fn mac_exit_fullscreen_if_needed() -> Result<bool, String> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::boolean::{CFBoolean, CFBooleanRef};
    use core_foundation::string::{CFString, CFStringRef};
    use objc2_app_kit::NSWorkspace;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXUIElementSetAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: CFTypeRef,
        ) -> i32;
    }

    let pid = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .ok_or("no frontmost application")?
        .processIdentifier();
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return Err("could not create the frontmost AX application".into());
        }
        let focused_attribute = CFString::from_static_string("AXFocusedWindow");
        let mut window: CFTypeRef = std::ptr::null();
        let focused_error = AXUIElementCopyAttributeValue(
            app,
            focused_attribute.as_concrete_TypeRef(),
            &mut window,
        );
        CFRelease(app);
        if focused_error != 0 || window.is_null() {
            return Err(format!(
                "AXFocusedWindow unavailable (AXError {focused_error})"
            ));
        }

        let fullscreen_attribute = CFString::from_static_string("AXFullScreen");
        let mut fullscreen_value: CFTypeRef = std::ptr::null();
        let fullscreen_error = AXUIElementCopyAttributeValue(
            window,
            fullscreen_attribute.as_concrete_TypeRef(),
            &mut fullscreen_value,
        );
        if fullscreen_error != 0 || fullscreen_value.is_null() {
            CFRelease(window);
            return Err(format!(
                "AXFullScreen unavailable (AXError {fullscreen_error})"
            ));
        }
        let fullscreen = bool::from(CFBoolean::wrap_under_create_rule(
            fullscreen_value as CFBooleanRef,
        ));
        if !fullscreen {
            CFRelease(window);
            return Ok(false);
        }

        let error = AXUIElementSetAttributeValue(
            window,
            fullscreen_attribute.as_concrete_TypeRef(),
            CFBoolean::false_value().as_CFTypeRef(),
        );
        CFRelease(window);
        if error != 0 {
            return Err(format!("AXFullScreen update failed (AXError {error})"));
        }
        log_line("Caps+Down: exited native full-screen mode");
    }
    Ok(true)
}

/// Move the frontmost app's focused window to the adjacent display while
/// preserving its relative position. AX uses Quartz coordinates (top-left
/// origin); NSScreen uses AppKit coordinates (bottom-left origin), so convert
/// through the primary display height before choosing and placing the window.
#[cfg(target_os = "macos")]
fn mac_move_focused_window(next: bool) -> Result<(), String> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::geometry::{CGPoint, CGSize};
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSWorkspace};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXUIElementSetAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: CFTypeRef,
        ) -> i32;
        fn AXValueGetValue(value: CFTypeRef, value_type: u32, out: *mut std::ffi::c_void) -> bool;
        fn AXValueCreate(value_type: u32, value: *const std::ffi::c_void) -> CFTypeRef;
    }

    const AX_VALUE_CGPOINT: u32 = 1;
    const AX_VALUE_CGSIZE: u32 = 2;

    let mtm = MainThreadMarker::new().ok_or("window command did not run on the main thread")?;
    let screens = NSScreen::screens(mtm);
    if screens.len() < 2 {
        return Ok(());
    }
    let primary_height = screens
        .iter()
        .next()
        .map(|screen| screen.frame().size.height)
        .ok_or("no AppKit screens")?;
    let mut frames: Vec<(f64, f64, f64, f64)> = screens
        .iter()
        .map(|screen| {
            let frame = screen.frame();
            (
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height,
            )
        })
        .collect();
    frames.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));

    let pid = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .ok_or("no frontmost application")?
        .processIdentifier();

    unsafe {
        let copy_attr = |element: CFTypeRef, name: &str| -> Result<CFTypeRef, String> {
            let attribute = CFString::new(name);
            let mut value: CFTypeRef = std::ptr::null();
            let error =
                AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value);
            if error != 0 || value.is_null() {
                Err(format!("{name} unavailable (AXError {error})"))
            } else {
                Ok(value)
            }
        };

        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return Err("could not create the frontmost AX application".into());
        }
        let window = match copy_attr(app, "AXFocusedWindow") {
            Ok(window) => window,
            Err(e) => {
                CFRelease(app);
                return Err(e);
            }
        };
        CFRelease(app);

        let position_value = match copy_attr(window, "AXPosition") {
            Ok(value) => value,
            Err(e) => {
                CFRelease(window);
                return Err(e);
            }
        };
        let size_value = match copy_attr(window, "AXSize") {
            Ok(value) => value,
            Err(e) => {
                CFRelease(position_value);
                CFRelease(window);
                return Err(e);
            }
        };
        let mut position = CGPoint::new(0.0, 0.0);
        let mut size = CGSize::new(0.0, 0.0);
        let values_ok =
            AXValueGetValue(
                position_value,
                AX_VALUE_CGPOINT,
                &mut position as *mut _ as *mut _,
            ) && AXValueGetValue(size_value, AX_VALUE_CGSIZE, &mut size as *mut _ as *mut _);
        CFRelease(position_value);
        CFRelease(size_value);
        if !values_ok {
            CFRelease(window);
            return Err("could not read focused-window geometry".into());
        }

        let appkit_x = position.x;
        let appkit_y = primary_height - position.y - size.height;
        let center_x = appkit_x + size.width / 2.0;
        let center_y = appkit_y + size.height / 2.0;
        let current = frames
            .iter()
            .position(|&(x, y, width, height)| {
                center_x >= x && center_x < x + width && center_y >= y && center_y < y + height
            })
            .unwrap_or_else(|| {
                frames
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        let distance = |frame: &&(f64, f64, f64, f64)| {
                            let dx = center_x - (frame.0 + frame.2 / 2.0);
                            let dy = center_y - (frame.1 + frame.3 / 2.0);
                            dx * dx + dy * dy
                        };
                        distance(a).total_cmp(&distance(b))
                    })
                    .map(|(index, _)| index)
                    .unwrap_or(0)
            });
        let step: isize = if next { 1 } else { -1 };
        let target_index = (current as isize + step).rem_euclid(frames.len() as isize) as usize;
        let source = frames[current];
        let target = frames[target_index];
        let relative_x = (appkit_x - source.0) / source.2.max(1.0);
        let relative_y = (appkit_y - source.1) / source.3.max(1.0);
        let target_appkit_x = target.0 + relative_x * target.2;
        let target_appkit_y = target.1 + relative_y * target.3;
        let target_position = CGPoint::new(
            target_appkit_x,
            primary_height - target_appkit_y - size.height,
        );
        let new_position = AXValueCreate(
            AX_VALUE_CGPOINT,
            &target_position as *const _ as *const std::ffi::c_void,
        );
        if new_position.is_null() {
            CFRelease(window);
            return Err("could not create the target window position".into());
        }
        let attribute = CFString::from_static_string("AXPosition");
        let error =
            AXUIElementSetAttributeValue(window, attribute.as_concrete_TypeRef(), new_position);
        CFRelease(new_position);
        CFRelease(window);
        if error != 0 {
            return Err(format!("AXPosition update failed (AXError {error})"));
        }
    }
    Ok(())
}

/// The `check` command: log everything the window/context machinery can see,
/// return a one-line summary for the status line.
fn run_diagnostics(mappings: &Mappings) -> String {
    #[cfg(windows)]
    {
        let windows = win_activate::list_windows();
        log_line(&format!("diag: {} visible windows:", windows.len()));
        for (image, class) in &windows {
            log_line(&format!("diag:   image={image} class={class}"));
        }
        let foreground = foreground_image().unwrap_or_else(|| "<unknown>".into());
        log_line(&format!(
            "diag: foreground={foreground} (this chooser), {} contexts loaded",
            mappings.contexts.len()
        ));
        format!(
            "diag: {} windows, foreground={foreground}, {} contexts -> details in zack.log",
            windows.len(),
            mappings.contexts.len()
        )
    }
    #[cfg(target_os = "macos")]
    {
        let foreground = foreground_identity().unwrap_or_else(|| "<unknown>".into());
        let trusted = accessibility_trusted(false);
        log_line(&format!(
            "diag: foreground={foreground}, accessibility={trusted}, {} macOS contexts loaded",
            mappings.contexts.len()
        ));
        format!(
            "diag: foreground={foreground}, accessibility={trusted}, {} contexts -> details in zack.log",
            mappings.contexts.len()
        )
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        log_line(&format!(
            "diag: window enumeration is Windows-only; {} contexts loaded",
            mappings.contexts.len()
        ));
        format!(
            "diag: {} contexts loaded (window checks are Windows-only)",
            mappings.contexts.len()
        )
    }
}

/// Pull, build, and reinstall in a visible PowerShell after Zack exits.
fn self_update(_mappings: &Mappings) -> Result<Option<AppSignal>, String> {
    #[cfg(windows)]
    {
        let config = crate::mappings::config_file();
        let repo = config
            .parent()
            .and_then(|launcher_dir| launcher_dir.parent())
            .ok_or("cannot derive repo root from config path")?;
        let updater = repo.join("scripts").join("update-windows.ps1");
        if !updater.is_file() {
            return Err(
                "update requires a repo-mode installation with scripts/update-windows.ps1".into(),
            );
        }
        // Pass paths as arguments, never interpolate them into PowerShell source.
        Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&updater)
            .arg("-Repo")
            .arg(repo)
            .arg("-LauncherPid")
            .arg(std::process::id().to_string())
            .spawn()
            .map_err(|e| format!("start updater: {e}"))?;
        log_line("self-update started, exiting so the updater can replace the binary");
        std::process::exit(0);
    }
    #[cfg(not(windows))]
    {
        Err("update is Windows-only; pull the repository, rebuild, and rerun the installer for your OS (see README)".into())
    }
}

#[cfg(target_os = "macos")]
fn activate_running_macos_app(command: &str) -> bool {
    use objc2_app_kit::{NSApplicationActivationOptions, NSWorkspace};

    let requested = command.to_lowercase();
    for app in NSWorkspace::sharedWorkspace().runningApplications().iter() {
        let name_matches = app
            .localizedName()
            .is_some_and(|name| name.to_string().to_lowercase() == requested);
        let bundle_matches = app
            .bundleIdentifier()
            .is_some_and(|bundle| bundle.to_string().to_lowercase() == requested);
        if name_matches || bundle_matches {
            return app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        }
    }
    false
}

fn launch_app(spec: &AppSpec, label: &str) -> Result<(), String> {
    let Some(command) = spec.for_current_os() else {
        return Err(format!("hotkey '{label}' has no command for this OS"));
    };
    if cfg!(target_os = "macos") {
        // `open -a` focuses a running instance instead of relaunching.
        #[cfg(target_os = "macos")]
        let focus_chord = spec
            .macos_focus_chord
            .as_deref()
            .map(crate::chords::parse_chord)
            .transpose()
            .map_err(|e| format!("hotkey '{label}' focus chord: {e}"))?;
        #[cfg(target_os = "macos")]
        let activated_existing = activate_running_macos_app(command);
        let result = if activated_existing {
            log_line(&format!("activated existing macOS app '{command}'"));
            Ok(())
        } else {
            spawn("open", &["-a", command])
        };
        #[cfg(target_os = "macos")]
        if result.is_ok() {
            if let Some(chord) = focus_chord {
                let app_name = command.to_lowercase();
                std::thread::spawn(move || {
                    // `open -a` returns before a cold-started GUI necessarily
                    // owns the menu bar. Wait until the requested app really
                    // is foreground, then send the follow-up chord.
                    for _ in 0..50 {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        let focused = foreground_identity().is_some_and(|identity| {
                            identity.split('|').any(|part| part == app_name)
                        });
                        if focused {
                            log_line(&format!(
                                "sending post-focus chord to {app_name}: {chord:?}"
                            ));
                            send_chord(&chord);
                            return;
                        }
                    }
                    report_async(&format!(
                        "opened macOS app but it did not focus in time for post-focus chord: {app_name}"
                    ));
                });
            }
        }
        result
    } else if cfg!(target_os = "windows") {
        // Activate an existing window first; launch only if none matches.
        #[cfg(windows)]
        let focus_chord = spec
            .windows_focus_chord
            .as_deref()
            .map(crate::chords::parse_chord)
            .transpose()
            .map_err(|e| format!("hotkey '{label}' focus chord: {e}"))?;
        #[cfg(windows)]
        let matcher = spec.windows_matcher();
        #[cfg(windows)]
        if let Some(matcher) = &matcher {
            if win_activate::activate(matcher, focus_chord.is_none()) {
                if let Some(chord) = &focus_chord {
                    send_chord(chord);
                }
                return Ok(());
            }
        }
        let result = spawn("cmd", &["/C", "start", "", command]);
        #[cfg(windows)]
        if result.is_ok() {
            if let (Some(matcher), Some(chord)) = (matcher, focus_chord) {
                // A freshly launched GUI may take a few seconds to create its
                // first window. Poll off the hotkey/UI thread, then apply the
                // requested chord once that window can be focused.
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        if win_activate::activate(&matcher, false) {
                            send_chord(&chord);
                            return;
                        }
                    }
                    report_async(&format!(
                        "launched app but could not focus '{matcher}' to send post-focus chord"
                    ));
                });
            }
        }
        result
    } else {
        // Linux v1: plain launch; most target apps are single-instance and
        // self-focus (documented limitation).
        spawn("sh", &["-c", command])
    }
}

pub fn run_hotkey(hotkey: &Hotkey, _mappings: &Mappings) -> Result<(), String> {
    if let Some(url) = &hotkey.url {
        open_detached(url)
    } else if let Some(app) = &hotkey.app {
        launch_app(app, &hotkey.label)
    } else if let Some(path) = &hotkey.path {
        let Some(p) = path.for_current_os() else {
            return Err(format!("hotkey '{}' has no path for this OS", hotkey.label));
        };
        open_detached(&expand_home(p))
    } else {
        Err(format!("hotkey '{}' has no target", hotkey.label))
    }
}

fn open_cursor(cli: &str, uri: &str) -> Result<(), String> {
    // Args for the cursor CLI itself: remote URIs go via --folder-uri; local paths
    // are passed positionally (with ~ expanded).
    let cli_args: Vec<String> = if uri.starts_with("vscode-remote://") {
        vec!["--folder-uri".to_string(), uri.to_string()]
    } else {
        vec![expand_home(uri)]
    };
    // On Windows the `cursor` (and `code`) CLI is a .cmd wrapper, which std's
    // Command (CreateProcess) will not run - it only resolves .exe, so a bare
    // "cursor" fails with "program not found". Route through `cmd /c` so PATHEXT
    // resolves cursor.cmd. This keeps cursor_cli portable (just "cursor").
    #[cfg(windows)]
    {
        let mut args: Vec<&str> = vec!["/c", cli];
        args.extend(cli_args.iter().map(String::as_str));
        spawn("cmd", &args)
    }
    #[cfg(not(windows))]
    {
        let args: Vec<&str> = cli_args.iter().map(String::as_str).collect();
        spawn(cli, &args)
    }
}

pub fn run(action: &Action, mappings: &Mappings) -> Result<Option<AppSignal>, String> {
    match action {
        Action::OpenPath(path) => open_detached(&expand_home(path)).map(|_| None),
        Action::OpenUrl(url) => open_detached(url).map(|_| None),
        Action::OpenCursor(uri) => open_cursor(&mappings.cursor_cli, uri).map(|_| None),
        Action::Builtin(BuiltinAction::CopyDate) => Ok(Some(AppSignal::CopyDate(
            chrono::Local::now().format("%Y-%m-%d").to_string(),
        ))),
        Action::Builtin(BuiltinAction::Reload) => Ok(Some(AppSignal::Reload)),
        Action::Builtin(BuiltinAction::OpenConfig) => {
            open_detached(&crate::mappings::config_file().to_string_lossy()).map(|_| None)
        }
        Action::Builtin(BuiltinAction::OpenConfigDir) => {
            let file = crate::mappings::config_file();
            let dir = file.parent().ok_or("config file has no parent dir")?;
            open_detached(&dir.to_string_lossy()).map(|_| None)
        }
        Action::Builtin(BuiltinAction::Diag) => {
            Ok(Some(AppSignal::Status(run_diagnostics(mappings))))
        }
        Action::Builtin(BuiltinAction::Update) => self_update(mappings),
        Action::Builtin(BuiltinAction::Dictation) => {
            #[cfg(target_os = "macos")]
            {
                // Hide first (Ok(None)), then fire once the previous app has
                // its focus back — dictating into the hidden chooser is useless.
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    request_dictation_toggle();
                });
                Ok(None)
            }
            #[cfg(not(target_os = "macos"))]
            Err("dictation is macOS-only for now".into())
        }
        Action::LaunchHotkey(key) => {
            let hotkey = mappings
                .hotkeys
                .get(key)
                .ok_or_else(|| format!("no hotkey '{key}'"))?;
            run_hotkey(hotkey, mappings).map(|_| None)
        }
        // Screen transitions are handled by the UI layer, never here.
        Action::EnterMode(_) | Action::EnterFind | Action::ShowHelp | Action::ShowKeyboard => {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_requires_two_quick_unused_taps_for_escape() {
        assert_eq!(
            caps_tap_result(true, false, 100, 150, u64::MAX),
            (false, 150)
        );
        assert_eq!(
            caps_tap_result(true, false, 340, 500, 150),
            (true, u64::MAX)
        );
    }

    #[test]
    fn duplicate_caps_release_does_not_complete_double_tap() {
        assert_eq!(caps_tap_result(false, false, 100, 150, 150), (false, 150));
    }

    #[test]
    fn caps_hold_or_chord_cancels_pending_tap() {
        assert_eq!(
            caps_tap_result(true, false, 100, 301, 50),
            (false, u64::MAX)
        );
        assert_eq!(caps_tap_result(true, true, 100, 150, 50), (false, u64::MAX));
        assert_eq!(caps_tap_result(true, false, 500, 550, 150), (false, 550));
    }

    #[test]
    fn expands_leading_tilde_only() {
        let home = dirs::home_dir().unwrap().to_string_lossy().into_owned();
        assert_eq!(expand_home("~/Downloads"), format!("{home}/Downloads"));
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("/a/~/b"), "/a/~/b");
    }

    #[test]
    fn picks_app_command_for_current_os() {
        let spec: AppSpec =
            serde_json::from_str(r#"{"macos":"Finder","linux":"nautilus","windows":"explorer"}"#)
                .unwrap();
        assert!(spec.for_current_os().is_some());
    }
}
