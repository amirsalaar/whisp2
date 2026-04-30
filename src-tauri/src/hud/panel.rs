//! Floating dock pill — bottom-center of screen, above the Dock.
//!
//! States drive the WebviewWindow content via Tauri events (`hud_state`).
//! AppKit panel flags are applied post-creation via raw msg_send! calls.

use objc2::runtime::AnyObject;
use objc2::msg_send;
use objc2_app_kit::NSScreen;
use objc2_foundation::{MainThreadMarker, NSRect, NSPoint, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, Clone, PartialEq)]
pub enum HudState {
    CollapsedIdle,
    ExpandedIdle,
    ShortcutListening,
    RecordingControls,
    Processing,
    Hidden,
}

impl HudState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::CollapsedIdle => "collapsed-idle",
            Self::ExpandedIdle => "expanded-idle",
            Self::ShortcutListening => "shortcut-listening",
            Self::RecordingControls => "recording-controls",
            Self::Processing => "processing",
            Self::Hidden => "hidden",
        }
    }

    fn needs_mouse_events(&self) -> bool {
        matches!(self, Self::RecordingControls)
    }
}

pub fn create(app: &AppHandle) {
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let (x, y) = bottom_center_position(mtm);

    let window = WebviewWindowBuilder::new(app, "hud", WebviewUrl::App("hud.html".into()))
        .title("Whisp HUD")
        .inner_size(340.0, 88.0)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .always_on_top(true)
        .shadow(false)
        .skip_taskbar(true)
        .focused(false)
        .build();

    let window = match window {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("HUD window creation failed (app will run without HUD): {}", e);
            return;
        }
    };

    apply_panel_flags(&window);
}

fn apply_panel_flags(window: &tauri::WebviewWindow) {
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() else { return };

    unsafe {
        let ns_view = appkit_handle.ns_view.as_ptr() as *mut AnyObject;
        let ns_window: *mut AnyObject = msg_send![ns_view, window];
        if ns_window.is_null() { return; }

        // NSStatusWindowLevel = 25
        let _: () = msg_send![ns_window, setLevel: 25i64];

        // collectionBehavior: canJoinAllSpaces(1) | fullScreenAuxiliary(128) | stationary(16) | ignoresCycle(64)
        let _: () = msg_send![ns_window, setCollectionBehavior: 1u64 | 128u64 | 16u64 | 64u64];

        let _: () = msg_send![ns_window, setHidesOnDeactivate: false];

        let _: () = msg_send![ns_window, setOpaque: false];
        let _: () = msg_send![ns_window, setHasShadow: false];
        // Note: setFloatingPanel: removed — only valid on NSPanel subclass, not NSWindow
    }
}

fn bottom_center_position(mtm: MainThreadMarker) -> (f64, f64) {
    let screen = NSScreen::mainScreen(mtm);
    let visible_frame = screen
        .as_ref()
        .map(|s| s.visibleFrame())
        .unwrap_or_else(|| NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)));
    // NSScreen uses AppKit coordinates: origin at bottom-left, y increases upward.
    // Tauri's position() uses top-left origin: y increases downward.
    //
    // full_height = visibleFrame.minY (dock height) + visibleFrame.height + menu bar height
    // We use NSScreen.frame for the true physical height of the screen.
    let screen_height = screen
        .as_ref()
        .map(|s| s.frame().size.height)
        .unwrap_or(visible_frame.origin.y + visible_frame.size.height);

    // Horizontal: centered over visible width
    let x = visible_frame.origin.x + (visible_frame.size.width - 340.0) / 2.0;
    // Vertical: bottom of our 88px window sits 12px above the visible area bottom edge (top of Dock)
    // appkit_bottom = visibleFrame.minY + 12   (AppKit y of our window's bottom edge)
    // tauri_y = screen_height - appkit_bottom - window_height
    let appkit_bottom = visible_frame.origin.y + 12.0;
    let y = screen_height - appkit_bottom - 88.0;
    (x, y)
}

pub fn update(app: &AppHandle, state: HudState) {
    update_with_label(app, state, None);
}

/// Like `update`, but attaches an optional label to `expanded-idle` states.
/// The JS receives `"expanded-idle:Click to allow microphone"` etc.
pub fn update_with_label(app: &AppHandle, state: HudState, label: Option<&str>) {
    let payload = match (&state, label) {
        (HudState::ExpandedIdle, Some(lbl)) => format!("expanded-idle:{}", lbl),
        _ => state.as_str().to_string(),
    };
    let _ = app.emit_to(
        tauri::EventTarget::webview_window("hud"),
        "hud_state",
        payload,
    );
    if let Some(window) = app.get_webview_window("hud") {
        let _ = window.set_ignore_cursor_events(!state.needs_mouse_events());
    }
}

// Legacy shims for any remaining callers
pub fn show(_label: &str) {}
pub fn hide() {}
