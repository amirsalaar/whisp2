//! Smart Paste: types text into the frontmost app by posting CGEvent Unicode
//! keyboard events. Does NOT use the clipboard.
//!
//! Must be called from the main thread (CGEventPost to kCGHIDEventTap requires it).
//! Call via `tauri::AppHandle::run_on_main_thread`.

use core_graphics::event::{CGEvent, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

// 20 UTF-16 units per chunk — matches whisp's Swift implementation.
const CHUNK_SIZE: usize = 20;

// Terminal emulators need a longer delay between chunks to avoid dropped characters.
const TERMINAL_BUNDLE_PREFIXES: &[&str] = &[
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "net.kovidgoyal.kitty",
    "io.alacritty",
    "com.github.wez.wezterm",
    "co.zeit.hyper",
];

fn chunk_delay_ms(source_app: Option<&str>) -> u64 {
    match source_app {
        Some(bundle_id) if TERMINAL_BUNDLE_PREFIXES.iter().any(|p| bundle_id.starts_with(p)) => 5,
        _ => 2,
    }
}

/// Injects `text` into the currently focused app via CGEvent Unicode posting.
/// `source_app` is the bundle ID of the target app (used to pick injection delay).
/// Blocking; intended to be run on the main thread via `run_on_main_thread`.
pub fn type_text(text: &str, source_app: Option<&str>) -> anyhow::Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let delay_ms = chunk_delay_ms(source_app);

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("CGEventSource creation failed"))?;

    let utf16: Vec<u16> = text.encode_utf16().collect();

    for chunk in utf16.chunks(CHUNK_SIZE) {
        let key_down = CGEvent::new_keyboard_event(source.clone(), 0u16, true)
            .map_err(|_| anyhow::anyhow!("CGEvent keydown creation failed"))?;

        key_down.set_string_from_utf16_unchecked(chunk);
        key_down.post(CGEventTapLocation::HID);

        let key_up = CGEvent::new_keyboard_event(source.clone(), 0u16, false)
            .map_err(|_| anyhow::anyhow!("CGEvent keyup creation failed"))?;
        key_up.post(CGEventTapLocation::HID);

        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }

    Ok(())
}
