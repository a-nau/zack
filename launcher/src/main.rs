//! zack — cross-platform Caps-driven launcher daemon.
//!
//! kanata turns held CapsLock into Hyper (Ctrl+Alt+Shift) on Windows/Linux.
//! On macOS, the installer maps Caps to an inert sentinel with native hidutil,
//! and Zack's event tap adds Hyper while it is held. This daemon registers
//! Hyper hotkeys and renders the chooser window. All user data lives in
//! mappings.json.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod actions;
mod chords;
mod logic;
mod mappings;
#[cfg(any(not(target_os = "macos"), test))]
mod placement;

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use eframe::egui;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use actions::AppSignal;
use logic::{Action, Screen};
#[cfg(any(windows, target_os = "macos"))]
use mappings::ContextAction;
use mappings::Mappings;

/// Hyper deliberately excludes Super/Win: Ctrl+Alt+Shift+Win combos are
/// reserved on Windows (the Office key family).
const HYPER: Modifiers = Modifiers::CONTROL
    .union(Modifiers::ALT)
    .union(Modifiers::SHIFT);

const WINDOW_SIZE: [f32; 2] = [640.0, 420.0];

/// Center of the monitor containing the mouse pointer (falling back to the
/// primary monitor) as an egui OuterPosition. display-info reports native
/// platform units (physical pixels
/// on X11/Windows, points on macOS); egui-winit multiplies OuterPosition by
/// pixels_per_point into physical pixels, so we divide it back out — except
/// on macOS, where that multiply/convert round trip lands back on points.
/// Returns None if monitors can't be enumerated (window then just stays where
/// it was created: also centered on the primary).
#[cfg(not(target_os = "macos"))]
fn chooser_position(pixels_per_point: f32) -> Option<egui::Pos2> {
    let displays: Vec<placement::Display> = display_info::DisplayInfo::all()
        .ok()?
        .into_iter()
        .map(|d| placement::Display {
            x: d.x as f32,
            y: d.y as f32,
            w: d.width as f32,
            h: d.height as f32,
            primary: d.is_primary,
        })
        .collect();
    let scale = pixels_per_point;
    let display = placement::pick_display(&displays, mouse_position())?;
    let (x, y) =
        placement::centered_position(display, WINDOW_SIZE[0] * scale, WINDOW_SIZE[1] * scale);
    Some(egui::pos2(x / scale, y / scale))
}

#[cfg(not(target_os = "macos"))]
fn mouse_position() -> Option<(f32, f32)> {
    None
}

#[cfg(target_os = "macos")]
fn configure_macos_overlay(window: &objc2_app_kit::NSWindow) {
    use objc2_app_kit::{NSStatusWindowLevel, NSWindowCollectionBehavior};

    // This is a floating system-style overlay, not an auxiliary window owned
    // by whichever app is full-screen. CanJoinAllApplications is the AppKit
    // behavior intended for that case (including Stage Manager/full-screen),
    // while CanJoinAllSpaces keeps ordinary Desktop Spaces covered.
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::CanJoinAllApplications
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    window.setHidesOnDeactivate(false);
    window.setLevel(NSStatusWindowLevel);
}

/// Center of the globally focused window in AppKit (bottom-left origin)
/// coordinates, via the Accessibility API — the same grant chord injection
/// needs. None when untrusted or nothing has keyboard focus; the caller
/// falls back to the mouse position.
#[cfg(target_os = "macos")]
fn focused_window_center(primary_height: f64) -> Option<(f64, f64)> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::geometry::{CGPoint, CGSize};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXValueGetValue(value: CFTypeRef, value_type: u32, out: *mut std::ffi::c_void) -> bool;
    }
    const AX_VALUE_CGPOINT: u32 = 1;
    const AX_VALUE_CGSIZE: u32 = 2;

    fn copy_attr(element: CFTypeRef, name: &str) -> Option<CFTypeRef> {
        let attribute = CFString::new(name);
        let mut value: CFTypeRef = std::ptr::null();
        let error = unsafe {
            AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value)
        };
        (error == 0 && !value.is_null()).then_some(value)
    }

    unsafe {
        let system = AXUIElementCreateSystemWide();
        let app = copy_attr(system, "AXFocusedApplication");
        CFRelease(system);
        let app = app?;
        let window = copy_attr(app, "AXFocusedWindow");
        CFRelease(app);
        let window = window?;
        let position = copy_attr(window, "AXPosition");
        let size = copy_attr(window, "AXSize");
        CFRelease(window);
        let mut origin = CGPoint::new(0.0, 0.0);
        let mut extent = CGSize::new(0.0, 0.0);
        let mut ok = false;
        if let (Some(position), Some(size)) = (position, size) {
            ok = AXValueGetValue(position, AX_VALUE_CGPOINT, &mut origin as *mut _ as *mut _)
                && AXValueGetValue(size, AX_VALUE_CGSIZE, &mut extent as *mut _ as *mut _);
        }
        if let Some(position) = position {
            CFRelease(position);
        }
        if let Some(size) = size {
            CFRelease(size);
        }
        if !ok {
            return None;
        }
        // AX reports Quartz coordinates (top-left origin, y down); AppKit
        // uses bottom-left origin, y up. Flip around the primary's height.
        Some((
            origin.x + extent.width / 2.0,
            primary_height - (origin.y + extent.height / 2.0),
        ))
    }
}

/// Position in AppKit's own coordinate system. This avoids translating between
/// Quartz/display-info coordinates and Cocoa coordinates on mixed displays.
/// zack is keyboard-driven, so the chooser follows the screen you are TYPING
/// on (the focused window); the mouse position is only the fallback anchor.
#[cfg(target_os = "macos")]
fn position_macos_window(window: &objc2_app_kit::NSWindow) -> Result<(), String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen};

    let mtm = MainThreadMarker::new().ok_or("launcher UI is not on the main thread")?;
    let screens = NSScreen::screens(mtm);
    let primary_height = screens
        .iter()
        .next()
        .map(|screen| screen.frame().size.height)
        .ok_or("no AppKit screens")?;
    let mouse = NSEvent::mouseLocation();
    let (anchor_x, anchor_y, anchor) = match focused_window_center(primary_height) {
        Some((x, y)) => (x, y, "focused-window"),
        None => (mouse.x, mouse.y, "mouse"),
    };
    let containing = |x: f64, y: f64| {
        screens.iter().find(|screen| {
            let frame = screen.frame();
            x >= frame.origin.x
                && x < frame.origin.x + frame.size.width
                && y >= frame.origin.y
                && y < frame.origin.y + frame.size.height
        })
    };
    let screen = containing(anchor_x, anchor_y)
        .or_else(|| containing(mouse.x, mouse.y))
        .ok_or("neither the focused window nor the mouse is on any AppKit screen")?;
    let screen_frame = screen.frame();
    let window_frame = window.frame();
    let mut origin = window_frame.origin;
    origin.x = screen_frame.origin.x + (screen_frame.size.width - window_frame.size.width) / 2.0;
    origin.y = screen_frame.origin.y + (screen_frame.size.height - window_frame.size.height) / 2.0;
    window.setFrameOrigin(origin);
    let front = actions::foreground_identity().unwrap_or_else(|| "<unknown>".into());
    let layout: Vec<String> = screens
        .iter()
        .map(|screen| {
            let frame = screen.frame();
            format!(
                "({:.0},{:.0} {:.0}x{:.0})",
                frame.origin.x, frame.origin.y, frame.size.width, frame.size.height
            )
        })
        .collect();
    actions::log_line(&format!(
        "chooser screen {anchor}=({anchor_x:.0},{anchor_y:.0}) frame=({:.0},{:.0} {:.0}x{:.0}) origin=({:.0},{:.0}) front={front} screens=[{}]",
        screen_frame.origin.x,
        screen_frame.origin.y,
        screen_frame.size.width,
        screen_frame.size.height,
        origin.x,
        origin.y,
        layout.join(" "),
    ));
    Ok(())
}

/// Keep the native AppKit window so each invocation can explicitly move it to
/// the active Space before bringing it forward. Merely joining every Space is
/// insufficient for a window that spends most of its lifetime hidden.
#[cfg(target_os = "macos")]
fn macos_window(
    cc: &eframe::CreationContext<'_>,
) -> Result<objc2::rc::Retained<objc2_app_kit::NSWindow>, String> {
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = cc
        .window_handle()
        .map_err(|e| format!("could not access launcher window: {e}"))?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err("launcher does not have an AppKit window handle".into());
    };
    let view = unsafe { appkit.ns_view.as_ptr().cast::<NSView>().as_ref() }
        .ok_or("launcher AppKit view is null")?;
    let window = view
        .window()
        .ok_or("launcher AppKit window is unavailable")?;
    // Accessory app (like Spotlight/Alfred): no Dock icon, and — crucially —
    // activating it does NOT make macOS switch to the Space its hidden window
    // lives on. Without this every open slides back to the "main" desktop.
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        NSApplication::sharedApplication(mtm)
            .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
    #[cfg(target_os = "macos")]
    actions::start_switcher_event_tap();
    configure_macos_overlay(&window);
    Ok(window)
}

fn main() -> eframe::Result {
    // Accessory policy from the very start: flipping Regular -> Accessory
    // after the app has launched leaves activation flaky on macOS 14+.
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        NSApplication::sharedApplication(mtm)
            .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(WINDOW_SIZE)
            .with_decorations(false)
            .with_always_on_top()
            .with_visible(false)
            .with_resizable(false),
        centered: true,
        ..Default::default()
    };
    eframe::run_native("zack", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

enum HotkeyTarget {
    ToggleChooser,
    /// A Hyper+key that is an app/URL hotkey, an app-context chord, or both.
    Key(String),
    #[cfg(any(windows, target_os = "macos"))]
    Window(actions::WindowCommand),
}

struct App {
    mappings: Mappings,
    manager: GlobalHotKeyManager,
    registered: Vec<HotKey>,
    targets: HashMap<u32, HotkeyTarget>,
    pressed_hotkeys: HashSet<u32>,
    /// Trigger id -> injected chord currently held down. Keeping this separate
    /// from repeat suppression lets the physical key release stop dictation.
    #[cfg(any(windows, target_os = "macos"))]
    held_context_chords: HashMap<u32, String>,
    matcher: nucleo_matcher::Matcher,
    toggle_id: u32,
    visible: bool,
    /// eframe force-shows the window after the first paint; counter to
    /// re-hide during the first frames (review finding A4).
    startup_hide_frames: u8,
    screen: Screen,
    /// Foreground app captured before the chooser takes focus. The keyboard
    /// view uses it to show the same context overrides dispatch would select.
    keyboard_context_identity: Option<String>,
    query: String,
    last_query: String,
    selected: usize,
    focus_input: bool,
    /// Transient message shown under the input (last action result).
    status: Option<String>,
    /// Persistent problem (config/hotkey failures); re-shown on every open
    /// until a successful reload clears it.
    sticky_status: Option<String>,
    /// Native handle used to move the hidden launcher into the active macOS
    /// Space when Caps+Space is pressed.
    #[cfg(target_os = "macos")]
    macos_window: Option<objc2::rc::Retained<objc2_app_kit::NSWindow>>,
    /// App that was frontmost when the chooser opened; focus is handed back
    /// to it on hide. Waiting for AppKit to pick a successor is slow enough
    /// that follow-up Caps chords would misroute against a frontmost zack.
    #[cfg(target_os = "macos")]
    previous_app: Option<objc2::rc::Retained<objc2_app_kit::NSRunningApplication>>,
    /// Remaining activation retries after a show: macOS 14 cooperative
    /// activation routinely declines the first request (the hotkey arrives
    /// via kanata's virtual keyboard, so it is not attributed to us), leaving
    /// the chooser visible but not key — keystrokes then go to the old app.
    #[cfg(target_os = "macos")]
    focus_retries: u8,
}

/// Minimal visual polish: dark panel, calm accent, roomier rows. No theming
/// framework — tweak the constants here.
fn apply_style(ctx: &egui::Context) {
    // Zack is a fixed-size launcher, not a document viewer. egui enables
    // browser-style Cmd/Ctrl +/- zoom by default, which can make the content
    // outgrow this window with no visible way to reset it.
    ctx.set_zoom_factor(1.0);
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
    ctx.set_theme(egui::Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(0x16, 0x18, 0x1d);
    visuals.selection.bg_fill = egui::Color32::from_rgb(0x2f, 0x5f, 0x8f);
    visuals.selection.stroke = egui::Stroke::NONE;
    // Rounded rows, no widget borders (the input frame is disabled entirely).
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        widget.corner_radius = egui::CornerRadius::same(6);
        widget.bg_stroke = egui::Stroke::NONE;
    }
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 3.0);
        style.spacing.button_padding = egui::vec2(10.0, 7.0);
    });
}

/// Row = normal-weight title + dimmed smaller subtext on one line.
/// Chip colors (text, background) per kind tag. Unknown tags get a neutral
/// gray chip; "" means no chip at all.
fn tag_colors(tag: &str) -> (egui::Color32, egui::Color32) {
    let white = egui::Color32::from_gray(240);
    match tag {
        "dir" => (white, egui::Color32::from_rgb(54, 92, 148)), // blue
        "web" => (white, egui::Color32::from_rgb(46, 118, 88)), // green
        "ide" => (white, egui::Color32::from_rgb(120, 86, 160)), // purple
        "app" => (white, egui::Color32::from_rgb(170, 110, 40)), // amber
        _ => (egui::Color32::from_gray(230), egui::Color32::from_gray(90)),
    }
}

fn row_text(tag: &str, title: &str, subtext: &str, max_width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = max_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    if !tag.is_empty() {
        let (fg, bg) = tag_colors(tag);
        // A padded, colored block reads as a chip even without rounded corners.
        job.append(
            &format!(" {} ", tag.to_uppercase()),
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(11.0),
                color: fg,
                background: bg,
                valign: egui::Align::Center,
                ..Default::default()
            },
        );
        job.append("  ", 0.0, egui::TextFormat::default());
    }
    job.append(
        title,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(16.0),
            color: egui::Color32::from_gray(235),
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    if !subtext.is_empty() {
        job.append(
            &format!("    {subtext}"),
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(13.0),
                color: egui::Color32::from_gray(135),
                valign: egui::Align::Center,
                ..Default::default()
            },
        );
    }
    job
}

const KEYBOARD_ROWS: &[&[(&str, f32)]] = &[
    &[
        ("esc", 1.25),
        ("f1", 1.0),
        ("f2", 1.0),
        ("f3", 1.0),
        ("f4", 1.0),
        ("f5", 1.0),
        ("f6", 1.0),
        ("f7", 1.0),
        ("f8", 1.0),
        ("f9", 1.0),
        ("f10", 1.0),
        ("f11", 1.0),
        ("f12", 1.0),
    ],
    &[
        ("grave", 1.0),
        ("1", 1.0),
        ("2", 1.0),
        ("3", 1.0),
        ("4", 1.0),
        ("5", 1.0),
        ("6", 1.0),
        ("7", 1.0),
        ("8", 1.0),
        ("9", 1.0),
        ("0", 1.0),
        ("minus", 1.0),
        ("equals", 1.0),
        ("backspace", 2.0),
    ],
    &[
        ("tab", 1.5),
        ("q", 1.0),
        ("w", 1.0),
        ("e", 1.0),
        ("r", 1.0),
        ("t", 1.0),
        ("y", 1.0),
        ("u", 1.0),
        ("i", 1.0),
        ("o", 1.0),
        ("p", 1.0),
        ("[", 1.0),
        ("]", 1.0),
        ("backslash", 1.5),
    ],
    &[
        ("caps", 1.75),
        ("a", 1.0),
        ("s", 1.0),
        ("d", 1.0),
        ("f", 1.0),
        ("g", 1.0),
        ("h", 1.0),
        ("j", 1.0),
        ("k", 1.0),
        ("l", 1.0),
        ("semicolon", 1.0),
        ("quote", 1.0),
        ("enter", 2.25),
    ],
    &[
        ("shift", 2.25),
        ("z", 1.0),
        ("x", 1.0),
        ("c", 1.0),
        ("v", 1.0),
        ("b", 1.0),
        ("n", 1.0),
        ("m", 1.0),
        ("comma", 1.0),
        ("period", 1.0),
        ("slash", 1.0),
        ("shift", 2.75),
    ],
    &[
        ("ctrl", 1.5),
        ("alt", 1.5),
        ("cmd", 1.5),
        ("space", 5.0),
        ("cmd", 1.5),
        ("left", 1.0),
        ("down", 1.0),
        ("up", 1.0),
        ("right", 1.0),
    ],
];

fn keyboard_key_name(key: &str) -> String {
    match key {
        "esc" => "Esc".into(),
        "grave" => "`".into(),
        "minus" => "-".into(),
        "equals" => "=".into(),
        "backspace" => "Back".into(),
        "tab" => "Tab".into(),
        "backslash" => "\\".into(),
        "caps" => "Caps".into(),
        "semicolon" => ";".into(),
        "quote" => "'".into(),
        "enter" => "Return".into(),
        "shift" => "Shift".into(),
        "comma" => ",".into(),
        "period" => ".".into(),
        "slash" => "/".into(),
        "ctrl" => "Ctrl".into(),
        "alt" => "Alt".into(),
        "cmd" => "Cmd".into(),
        "space" => "Space".into(),
        "left" => "Left".into(),
        "right" => "Right".into(),
        "up" => "Up".into(),
        "down" => "Down".into(),
        _ => key.to_uppercase(),
    }
}

fn compact_binding_label(label: &str, max_chars: usize) -> String {
    let (prefix, body) = label
        .strip_prefix("hold ")
        .map_or(("", label), |body| ("Hold ", body));
    let chord = body
        .split('+')
        .map(|part| match part {
            "ctrl" => "Ctl",
            "shift" => "Sh",
            "cmd" => "Cmd",
            "alt" => "Alt",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("+");
    let compact = format!("{prefix}{chord}");
    if compact.chars().count() <= max_chars {
        return compact;
    }

    let words: Vec<&str> = label.split_whitespace().collect();
    for word in words.iter().rev().chain(words.iter()) {
        if word.chars().count() <= max_chars {
            return (*word).to_string();
        }
    }
    format!(
        "{}…",
        compact
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>()
    )
}

fn keyboard_app_name(identity: Option<&str>) -> String {
    let raw = identity
        .and_then(|value| value.split('|').next())
        .unwrap_or("global");
    if raw == "chatgpt" {
        return "ChatGPT".into();
    }
    raw.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn show_keyboard_key(
    ui: &mut egui::Ui,
    key: &str,
    width: f32,
    binding: Option<&logic::KeyboardBinding>,
) {
    let fill = match binding {
        Some(binding) if binding.contextual => egui::Color32::from_rgb(39, 103, 154),
        Some(_) => egui::Color32::from_rgb(125, 82, 35),
        None => egui::Color32::from_rgb(35, 39, 47),
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 42.0), egui::Sense::hover());
    let fill = if response.hovered() {
        fill.gamma_multiply(1.15)
    } else {
        fill
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(5),
        fill,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(76, 83, 96)),
        egui::StrokeKind::Inside,
    );

    let key_name = keyboard_key_name(key);
    if let Some(binding) = binding {
        ui.painter().text(
            rect.left_top() + egui::vec2(5.0, 4.0),
            egui::Align2::LEFT_TOP,
            &key_name,
            egui::FontId::proportional(8.0),
            egui::Color32::from_gray(205),
        );
        let max_chars = ((width - 8.0) / 5.0).floor().max(3.0) as usize;
        ui.painter().text(
            rect.center_bottom() - egui::vec2(0.0, 7.0),
            egui::Align2::CENTER_BOTTOM,
            compact_binding_label(&binding.label, max_chars),
            egui::FontId::proportional(8.5),
            egui::Color32::WHITE,
        );
        response.on_hover_text(format!(
            "Caps+{}\n{}\n{}",
            key_name, binding.label, binding.detail
        ));
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            key_name,
            egui::FontId::proportional(9.0),
            egui::Color32::from_gray(175),
        );
    }
}

fn show_keyboard(ui: &mut egui::Ui, mappings: &Mappings, identity: Option<&str>) {
    let bindings = logic::keyboard_bindings(mappings, identity);
    let app = keyboard_app_name(identity);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Keyboard").strong().size(18.0));
            ui.label(
                egui::RichText::new(format!("{app} · effective Caps bindings · Esc to go back"))
                    .color(egui::Color32::from_gray(145))
                    .size(11.0),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(" APP OVERRIDE ")
                    .background_color(egui::Color32::from_rgb(39, 103, 154))
                    .color(egui::Color32::WHITE)
                    .size(9.0),
            );
            ui.label(
                egui::RichText::new(" GLOBAL ")
                    .background_color(egui::Color32::from_rgb(125, 82, 35))
                    .color(egui::Color32::WHITE)
                    .size(9.0),
            );
        });
    });
    ui.add_space(8.0);

    let gap = 3.0;
    for row in KEYBOARD_ROWS {
        let total_units: f32 = row.iter().map(|(_, units)| units).sum();
        let unit =
            (ui.available_width() - (row.len().saturating_sub(1) as f32 * gap)) / total_units;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (key, units) in *row {
                let binding = bindings.get(*key);
                show_keyboard_key(ui, key, unit * units, binding);
            }
        });
        ui.add_space(3.0);
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_style(&cc.egui_ctx);
        #[cfg(target_os = "macos")]
        let macos_window = macos_window(cc)
            .map_err(|e| {
                actions::log_line(&format!("could not enable current-Space launcher: {e}"));
            })
            .ok();
        let config_path = mappings::config_file();
        let (mappings, sticky_status) = match Mappings::load(&config_path) {
            Ok(m) => {
                // Startup inventory: makes a stale config obvious in the log
                // (e.g. an old mappings.json without the contexts section).
                actions::log_line(&format!(
                    "loaded {}: {} modes, {} folders, {} commands, {} hotkeys, {} contexts",
                    config_path.display(),
                    m.modes.len(),
                    m.folders.len(),
                    m.commands.len(),
                    m.hotkeys.len(),
                    m.contexts.len(),
                ));
                (m, None)
            }
            Err(e) => {
                actions::log_line(&format!("config error: {e}"));
                (Mappings::default(), Some(format!("config error: {e}")))
            }
        };
        let manager = match GlobalHotKeyManager::new() {
            Ok(m) => m,
            Err(e) => {
                actions::log_line(&format!("global hotkey manager init failed: {e}"));
                std::process::exit(1);
            }
        };
        // The toggle is registered exactly once and NEVER unregistered (not
        // even on reload) so Hyper+Space has no dead window.
        let toggle = HotKey::new(Some(HYPER), Code::Space);
        if let Err(e) = manager.register(toggle) {
            // Almost certainly a second instance. A hidden daemon without its
            // toggle key is a zombie — fail loudly instead (review finding B6).
            actions::log_line(&format!(
                "could not register Hyper+Space (already running?): {e} — exiting"
            ));
            std::process::exit(1);
        }
        let mut app = Self {
            mappings,
            manager,
            toggle_id: toggle.id(),
            registered: Vec::new(),
            targets: HashMap::new(),
            pressed_hotkeys: HashSet::new(),
            #[cfg(any(windows, target_os = "macos"))]
            held_context_chords: HashMap::new(),
            matcher: logic::new_matcher(),
            visible: false,
            startup_hide_frames: 3,
            screen: Screen::Root,
            keyboard_context_identity: None,
            query: String::new(),
            last_query: String::new(),
            selected: 0,
            focus_input: false,
            status: sticky_status.clone(),
            sticky_status,
            #[cfg(target_os = "macos")]
            macos_window,
            #[cfg(target_os = "macos")]
            previous_app: None,
            #[cfg(target_os = "macos")]
            focus_retries: 0,
        };
        let failures = app.register_letter_hotkeys();
        if !failures.is_empty() {
            app.sticky_status = Some(format!("hotkeys not registered: {}", failures.join(", ")));
            app.status = app.sticky_status.clone();
        }
        // Untrusted = every injected chord is silently dropped; make that loud.
        // The prompt also (re-)adds the binary to the Accessibility list —
        // needed after every rebuild because the ad-hoc signature changes.
        #[cfg(target_os = "macos")]
        if !actions::accessibility_trusted(true) {
            let message = "Accessibility NOT granted: context chords / focus chords / window keys \
                 will do nothing. System Settings > Privacy & Security > Accessibility: \
                 enable (or remove + re-add) ~/.local/bin/zack, then run `rel`."
                .to_string();
            actions::log_line(&message);
            app.sticky_status = Some(match app.sticky_status.take() {
                Some(existing) => format!("{existing} | {message}"),
                None => message,
            });
            app.status = app.sticky_status.clone();
        }
        app
    }

    /// (Re-)register the letter hotkeys (the toggle stays registered for the
    /// process lifetime); returns the keys that failed.
    fn register_letter_hotkeys(&mut self) -> Vec<String> {
        #[cfg(any(windows, target_os = "macos"))]
        self.release_held_context_chords();
        for hotkey in self.registered.drain(..) {
            if let Err(e) = self.manager.unregister(hotkey) {
                actions::log_line(&format!("unregister {hotkey:?}: {e}"));
            }
        }
        self.targets.clear();
        self.targets
            .insert(self.toggle_id, HotkeyTarget::ToggleChooser);
        let mut failures = Vec::new();

        // Hyper+arrows and O/P: native window/app/tab actions.
        #[cfg(any(windows, target_os = "macos"))]
        for (code, command) in [
            (Code::ArrowUp, actions::WindowCommand::MaximizeToggle),
            (Code::ArrowDown, actions::WindowCommand::Minimize),
            (Code::ArrowLeft, actions::WindowCommand::MonitorPrev),
            (Code::ArrowRight, actions::WindowCommand::MonitorNext),
            // o/p are reserved builtins (defining them in the
            // hotkeys map will fail registration and show in the status line).
            (Code::KeyO, actions::WindowCommand::SwitchWindow),
            (Code::KeyP, actions::WindowCommand::SwitchTab),
        ] {
            let hotkey = HotKey::new(Some(HYPER), code);
            match self.manager.register(hotkey) {
                Ok(()) => {
                    self.targets
                        .insert(hotkey.id(), HotkeyTarget::Window(command));
                    self.registered.push(hotkey);
                }
                Err(e) => {
                    actions::log_line(&format!("register Hyper+{code:?}: {e}"));
                    failures.push(format!("{code:?}"));
                }
            }
        }

        // One registration per key, shared by app hotkeys and app-context
        // chords; dispatch decides at press time (context wins).
        #[cfg_attr(all(not(windows), not(target_os = "macos")), allow(unused_mut))]
        let mut keys: std::collections::BTreeSet<String> = self
            .mappings
            .hotkeys
            .iter()
            .filter(|(_, hotkey)| hotkey.available_on_current_os())
            .map(|(letter, _)| letter.clone())
            .collect();
        #[cfg(any(windows, target_os = "macos"))]
        for context in self
            .mappings
            .contexts
            .iter()
            .filter(|context| context.available_on_current_os())
        {
            keys.extend(context.keys.keys().cloned());
        }
        for key in keys {
            #[cfg(any(windows, target_os = "macos"))]
            if key == "o" || key == "p" {
                actions::log_line(&format!(
                    "key '{key}' is reserved for the app/tab-switching builtins — ignoring the config entry"
                ));
                failures.push(format!("{key} (reserved)"));
                continue;
            }
            let Some(code) = chords::trigger_code(&key) else {
                continue;
            };
            let hotkey = HotKey::new(Some(HYPER), code);
            match self.manager.register(hotkey) {
                Ok(()) => {
                    self.targets
                        .insert(hotkey.id(), HotkeyTarget::Key(key.clone()));
                    self.registered.push(hotkey);
                }
                Err(e) => {
                    actions::log_line(&format!("register Hyper+{key}: {e}"));
                    failures.push(key);
                }
            }
        }
        failures
    }

    #[cfg(any(windows, target_os = "macos"))]
    fn release_held_context_chords(&mut self) {
        for (_, chord_str) in self.held_context_chords.drain() {
            match chords::parse_chord(&chord_str) {
                Ok(chord) => actions::release_chord(&chord),
                Err(e) => {
                    actions::log_line(&format!("could not release held chord '{chord_str}': {e}"))
                }
            }
        }
    }

    fn reset_input(&mut self) {
        self.query.clear();
        self.last_query.clear();
        self.selected = 0;
        self.focus_input = true;
    }

    fn show(&mut self, ctx: &egui::Context) {
        #[cfg(any(windows, target_os = "macos"))]
        {
            self.keyboard_context_identity = actions::foreground_identity();
        }
        #[cfg(all(not(windows), not(target_os = "macos")))]
        {
            self.keyboard_context_identity = None;
        }
        self.visible = true;
        self.screen = Screen::Root;
        // Startup/reload problems stay visible; only transient status resets.
        self.status = self.sticky_status.clone();
        self.reset_input();
        #[cfg(not(target_os = "macos"))]
        if let Some(pos) = chooser_position(ctx.pixels_per_point()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
        #[cfg(target_os = "macos")]
        if let Some(window) = &self.macos_window {
            use objc2_app_kit::NSWorkspace;

            self.previous_app = NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .filter(|app| app.processIdentifier() != std::process::id() as i32);
            configure_macos_overlay(window);
            if let Err(e) = position_macos_window(window) {
                actions::log_line(&format!(
                    "could not position launcher on current screen: {e}"
                ));
            }
            window.orderFrontRegardless();
            // Forceful self-activation via the Accessibility API: ordinary
            // activation requests fall under macOS 14 cooperative activation
            // and are silently declined (AppKit still reports isActive=true),
            // leaving the chooser visible but typing into the old app.
            actions::force_self_frontmost();
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                #[allow(deprecated)]
                objc2_app_kit::NSApplication::sharedApplication(mtm)
                    .activateIgnoringOtherApps(true);
            }
            window.makeKeyAndOrderFront(None);
            // Space diagnostics: if on_active_space is false here, the window
            // failed to join the current Space and macOS will have switched
            // Spaces instead of overlaying.
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
                let policy = NSApplication::sharedApplication(mtm).activationPolicy();
                actions::log_line(&format!(
                    "chooser shown: on_active_space={} accessory={} behavior={:?}",
                    window.isOnActiveSpace(),
                    policy == NSApplicationActivationPolicy::Accessory,
                    window.collectionBehavior(),
                ));
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        #[cfg(target_os = "macos")]
        {
            self.focus_retries = 10; // ~500ms of activation retries
        }
        #[cfg(not(target_os = "macos"))]
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn hide(&mut self, ctx: &egui::Context) {
        self.visible = false;
        #[cfg(target_os = "macos")]
        if let Some(window) = &self.macos_window {
            // Hide synchronously. This makes command-driven closes such as
            // `rel` immediate instead of waiting for the viewport command to
            // be processed at the end of the frame.
            window.orderOut(None);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        ctx.request_repaint();
        // Yield focus back to the app the user was in. zack activated itself
        // to type into the chooser; staying active-but-hidden strands the
        // keyboard AND makes the next open's focused-window anchor resolve to
        // zack (no visible window -> mouse fallback, wrong screen). Activate
        // the remembered app explicitly — AppKit's own successor pick after a
        // bare deactivate is slow enough to misroute follow-up Caps chords.
        #[cfg(target_os = "macos")]
        {
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                objc2_app_kit::NSApplication::sharedApplication(mtm).deactivate();
            }
            if let Some(previous) = self.previous_app.take() {
                if !previous.isTerminated() {
                    previous.activateWithOptions(
                        objc2_app_kit::NSApplicationActivationOptions::empty(),
                    );
                }
            }
        }
    }

    fn back_to_root(&mut self) {
        self.screen = Screen::Root;
        self.reset_input();
    }

    fn execute(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::EnterMode(key) => {
                self.screen = Screen::Search(key);
                self.reset_input();
            }
            Action::EnterFind => {
                self.screen = Screen::Find;
                self.reset_input();
            }
            Action::ShowHelp => {
                if self.screen != Screen::Help {
                    self.screen = Screen::Help;
                    self.reset_input();
                }
            }
            Action::ShowKeyboard => {
                if self.screen != Screen::Keyboard {
                    self.screen = Screen::Keyboard;
                    self.reset_input();
                }
            }
            other => match actions::run(&other, &self.mappings) {
                Ok(Some(AppSignal::CopyDate(date))) => {
                    ctx.copy_text(date.clone());
                    self.status = Some(format!("copied {date}"));
                }
                Ok(Some(AppSignal::Reload)) => match Mappings::load(&mappings::config_file()) {
                    Ok(m) => {
                        self.mappings = m;
                        let mut problems: Vec<String> = Vec::new();
                        let failures = self.register_letter_hotkeys();
                        if !failures.is_empty() {
                            problems.push(format!("failed hotkeys: {}", failures.join(", ")));
                        }
                        #[cfg(target_os = "macos")]
                        if !actions::accessibility_trusted(false) {
                            problems.push(
                                "Accessibility NOT granted — injected chords are dropped".into(),
                            );
                        }
                        if problems.is_empty() {
                            self.sticky_status = None;
                            self.status = Some("reloaded".into());
                        } else {
                            self.sticky_status =
                                Some(format!("reloaded; {}", problems.join(" | ")));
                            self.status = self.sticky_status.clone();
                        }
                        actions::log_line("reload succeeded; closing chooser");
                        self.hide(ctx);
                    }
                    Err(e) => {
                        actions::log_line(&format!("reload failed: {e}"));
                        self.sticky_status = Some(format!("reload failed: {e}"));
                        self.status = self.sticky_status.clone();
                    }
                },
                Ok(Some(AppSignal::Status(text))) => self.status = Some(text),
                Ok(None) => self.hide(ctx),
                Err(e) => {
                    // Keep the chooser open so the failure is visible (B13).
                    actions::log_line(&e);
                    self.status = Some(e);
                }
            },
        }
    }

    fn poll_hotkeys(&mut self, ctx: &egui::Context) {
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state() == HotKeyState::Released {
                self.pressed_hotkeys.remove(&event.id());
                #[cfg(any(windows, target_os = "macos"))]
                if let Some(chord_str) = self.held_context_chords.remove(&event.id()) {
                    actions::log_line(&format!("releasing held chord '{chord_str}'"));
                    match chords::parse_chord(&chord_str) {
                        Ok(chord) => actions::release_chord(&chord),
                        Err(e) => actions::log_line(&format!(
                            "could not release held chord '{chord_str}': {e}"
                        )),
                    }
                }
                continue;
            }
            if event.state() != HotKeyState::Pressed || !self.pressed_hotkeys.insert(event.id()) {
                continue;
            }
            match self.targets.get(&event.id()) {
                Some(HotkeyTarget::ToggleChooser) => {
                    if self.visible {
                        self.hide(ctx);
                    } else {
                        self.show(ctx);
                    }
                }
                Some(HotkeyTarget::Key(key)) => {
                    // App-context chord takes precedence (AHK #If semantics).
                    // Every dispatch is logged: zack.log is the first stop
                    // when a binding doesn't fire.
                    // Context chords must never target zack itself (it can be
                    // frontmost right after the chooser closes or at startup)
                    // — but app/URL hotkeys below must still run then.
                    #[cfg(target_os = "macos")]
                    let context_allowed = !actions::foreground_is_self();
                    #[cfg(windows)]
                    let context_allowed = true;
                    #[cfg(any(windows, target_os = "macos"))]
                    if !context_allowed {
                        actions::log_line(&format!(
                            "key '{key}': zack is frontmost; skipping context chord"
                        ));
                    } else {
                        let identity = actions::foreground_identity();
                        let action = identity
                            .as_deref()
                            .and_then(|app| self.mappings.context_action_for(app, key))
                            .cloned();
                        match (&identity, &action) {
                            (Some(app), Some(ContextAction::Chord(chord_str))) => {
                                actions::log_line(&format!(
                                    "key '{key}' in {app}: injecting '{chord_str}'"
                                ));
                                if let Ok(parsed) = chords::parse_chord(chord_str) {
                                    actions::send_chord(&parsed);
                                }
                                continue;
                            }
                            (Some(app), Some(ContextAction::Hold { hold })) => {
                                actions::log_line(&format!(
                                    "key '{key}' in {app}: holding '{hold}'"
                                ));
                                match chords::parse_chord(hold) {
                                    Ok(chord) => {
                                        actions::press_chord(&chord);
                                        self.held_context_chords.insert(event.id(), hold.clone());
                                    }
                                    Err(e) => actions::log_line(&format!(
                                        "could not hold chord '{hold}': {e}"
                                    )),
                                }
                                continue;
                            }
                            (Some(app), Some(ContextAction::Url { url })) => {
                                actions::log_line(&format!(
                                    "key '{key}' in {app}: opening '{url}'"
                                ));
                                if let Err(e) = actions::open_url(url) {
                                    actions::log_line(&e);
                                }
                                continue;
                            }
                            (app, _) => actions::log_line(&format!(
                                "key '{key}': no context match (foreground={}), {} contexts loaded",
                                app.as_deref().unwrap_or("<unknown>"),
                                self.mappings.contexts.len(),
                            )),
                        }
                    }
                    if let Some(hotkey) = self.mappings.hotkeys.get(key) {
                        if let Err(e) = actions::run_hotkey(hotkey, &self.mappings) {
                            actions::log_line(&e);
                        }
                    }
                }
                #[cfg(any(windows, target_os = "macos"))]
                Some(HotkeyTarget::Window(command)) => actions::manage_window(*command),
                None => {}
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        #[cfg(any(windows, target_os = "macos"))]
        self.release_held_context_chords();
        #[cfg(target_os = "macos")]
        actions::release_switcher_if_held();
    }
}

impl eframe::App for App {
    // logic() runs before ui() and — crucially — also while the window is
    // hidden, as long as a repaint is scheduled. Hotkey polling lives here.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_hotkeys(ctx);
        // Commits a pending Cmd+Tab switcher once Caps/Hyper is released.
        #[cfg(target_os = "macos")]
        actions::switcher_tick();
        // Fires a queued dictation toggle once Caps/Hyper is released.
        #[cfg(target_os = "macos")]
        actions::dictation_tick();
        // Re-request activation until the chooser really is the key window;
        // a single request after a virtual-keyboard hotkey is often declined.
        #[cfg(target_os = "macos")]
        if self.visible && self.focus_retries > 0 {
            if let (Some(window), Some(mtm)) = (&self.macos_window, objc2::MainThreadMarker::new())
            {
                let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
                // Judge by the WINDOW SERVER (frontmost app), not AppKit's
                // self-reported isActive — the latter says true even when
                // cooperative activation silently declined the request. The
                // window must ALSO be on the user's current Space; a
                // frontmost-but-elsewhere chooser swallows keystrokes
                // invisibly.
                if actions::foreground_is_self() && window.isKeyWindow() && window.isOnActiveSpace()
                {
                    actions::log_line(&format!(
                        "chooser frontmost+key+on-space (retries used: {})",
                        10 - self.focus_retries
                    ));
                    self.focus_retries = 0;
                } else {
                    if !window.isOnActiveSpace() {
                        // Re-ordering out and in gives the window server a
                        // fresh chance to apply the all-Spaces overlay flags.
                        configure_macos_overlay(window);
                        window.orderOut(None);
                        window.orderFrontRegardless();
                    }
                    actions::force_self_frontmost();
                    #[allow(deprecated)]
                    app.activateIgnoringOtherApps(true);
                    window.makeKeyAndOrderFront(None);
                    self.focus_retries -= 1;
                    if self.focus_retries == 0 {
                        actions::log_line(&format!(
                            "chooser NOT focused after retries: frontmost_self={} active={} key={} on_space={}",
                            actions::foreground_is_self(),
                            app.isActive(),
                            window.isKeyWindow(),
                            window.isOnActiveSpace(),
                        ));
                    }
                }
            }
        }
        if let Some(error) = actions::take_async_error() {
            self.status = Some(error);
        }
        // Keep the loop ticking so hotkeys are polled while hidden.
        ctx.request_repaint_after(Duration::from_millis(50));

        if !self.visible && self.startup_hide_frames > 0 {
            // eframe force-shows the window after the first paint; counter it.
            self.startup_hide_frames -= 1;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            // winit also activates the app at launch; a frontmost-but-hidden
            // zack blocks context chords until the user focuses another app.
            #[cfg(target_os = "macos")]
            if self.startup_hide_frames == 0 {
                if let Some(mtm) = objc2::MainThreadMarker::new() {
                    objc2_app_kit::NSApplication::sharedApplication(mtm).deactivate();
                }
            }
        }
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.visible {
            return;
        }
        let ctx = root.ctx().clone();

        // Consume launcher keys BEFORE TextEdit sees them; read Backspace and
        // the pre-edit emptiness for the "backspace on empty goes back" rule.
        let was_empty = self.query.is_empty();
        let (enter, escape, down, up) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            )
        });
        let backspace = ctx.input(|i| i.key_pressed(egui::Key::Backspace));

        let panel_frame =
            egui::Frame::central_panel(root.style()).inner_margin(egui::Margin::same(14));
        egui::CentralPanel::default()
            .frame(panel_frame)
            .show(root, |ui| {
                if self.screen == Screen::Keyboard {
                    show_keyboard(
                        ui,
                        &self.mappings,
                        self.keyboard_context_identity.as_deref(),
                    );
                    return;
                }

                let hint = match &self.screen {
                    // No fancy glyphs: the default egui fonts render ␠ as tofu.
                    Screen::Root => {
                        "Search or command  (g+space = Google, f+space = find)".to_string()
                    }
                    Screen::Search(key) => self
                        .mappings
                        .modes
                        .get(key)
                        .map(|m| format!("{} search…", m.label))
                        .unwrap_or_default(),
                    Screen::Find => "find anything… (folders, apps, links)".to_string(),
                    Screen::Help => "all Caps bindings — Esc to go back".to_string(),
                    Screen::Keyboard => String::new(),
                };
                // Borderless, large input — the panel itself is the frame.
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY)
                        // The margin must live on the frame; a separate .margin()
                        // call would configure the frame we just replaced.
                        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(2, 8)))
                        .font(egui::FontId::proportional(20.0)),
                );
                if self.focus_input {
                    response.request_focus();
                    self.focus_input = false;
                }

                // React to this frame's edits: mode switch on "<prefix> ".
                if self.query != self.last_query {
                    self.last_query = self.query.clone();
                    self.selected = 0;
                    if self.screen == Screen::Root {
                        if self.query.trim() == "?" {
                            self.execute(Action::ShowHelp, &ctx);
                        } else if self.query.trim() == "kbd" {
                            self.execute(Action::ShowKeyboard, &ctx);
                        } else if let Some(prefix) = logic::mode_prefix(&self.query) {
                            if prefix == "f" {
                                self.execute(Action::EnterFind, &ctx);
                            } else if self.mappings.modes.contains_key(prefix) {
                                self.execute(Action::EnterMode(prefix.to_string()), &ctx);
                            }
                        }
                    }
                }

                // Rows reflect the query as edited THIS frame (review finding A6).
                let rows =
                    logic::rows_for(&self.screen, &self.mappings, &self.query, &mut self.matcher);
                if !rows.is_empty() {
                    self.selected = self.selected.min(rows.len() - 1);
                    if down {
                        self.selected = (self.selected + 1) % rows.len();
                    }
                    if up {
                        self.selected = (self.selected + rows.len() - 1) % rows.len();
                    }
                }

                // Status ABOVE the scroll area: a full list would otherwise
                // consume all remaining height and clip it (caveman finding).
                if let Some(status) = &self.status {
                    ui.weak(status);
                }
                ui.separator();
                ui.add_space(4.0);
                let mut clicked: Option<Action> = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Justified layout: selection highlight spans the full row
                    // width instead of hugging the text.
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                        for (index, row) in rows.iter().enumerate() {
                            let text_width = (ui.available_width()
                                - 2.0 * ui.spacing().button_padding.x)
                                .max(1.0);
                            let text = row_text(&row.tag, &row.title, &row.subtext, text_width);
                            let label = ui.selectable_label(index == self.selected, text);
                            if index == self.selected && (down || up) {
                                label.scroll_to_me(None);
                            }
                            if label.clicked() || (enter && index == self.selected) {
                                clicked = Some(row.action.clone());
                            }
                        }
                    });
                });
                if let Some(action) = clicked {
                    self.execute(action, &ctx);
                }
            });

        if backspace && was_empty && self.screen != Screen::Root {
            self.back_to_root();
        }
        if escape {
            if self.screen == Screen::Root {
                self.hide(&ctx);
            } else {
                self.back_to_root();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooser_rows_stay_single_line_and_within_the_window() {
        let job = row_text("web", "A long result title", "A long result detail", 320.0);
        assert_eq!(job.wrap.max_width, 320.0);
        assert_eq!(job.wrap.max_rows, 1);
        assert!(job.wrap.break_anywhere);
    }
}
