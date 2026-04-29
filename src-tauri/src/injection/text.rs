//! Smart Paste: types text into the frontmost app by posting CGEvent Unicode
//! keyboard events. Does NOT use the clipboard.
//!
//! Must be called from the main thread (CGEventPost to kCGHIDEventTap requires it).
//! Call via `tauri::AppHandle::run_on_main_thread`.
//!
//! Implementation mirrors whisp's Swift `PasteManager.typeTextViaCGEvent`:
//! - Encode text as UTF-16 code units
//! - Post in 20-unit chunks (CGEvent API limit for reliable delivery)
//! - Small inter-chunk delay for target app event loop

use core_graphics::event::{CGEvent, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

// 20 UTF-16 units per chunk — matches whisp's Swift implementation.
const CHUNK_SIZE: usize = 20;

/// Injects `text` into the currently focused app via CGEvent Unicode posting.
/// Blocking; intended to be run on the main thread via `run_on_main_thread`.
pub fn type_text(text: &str) -> anyhow::Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("CGEventSource creation failed"))?;

    let utf16: Vec<u16> = text.encode_utf16().collect();

    for chunk in utf16.chunks(CHUNK_SIZE) {
        // Create a keydown event with virtual key 0 (doesn't matter — Unicode string overrides it)
        let key_down = CGEvent::new_keyboard_event(source.clone(), 0u16, true)
            .map_err(|_| anyhow::anyhow!("CGEvent keydown creation failed"))?;

        key_down.set_string_from_utf16_unchecked(chunk);
        key_down.post(CGEventTapLocation::HID);

        let key_up = CGEvent::new_keyboard_event(source.clone(), 0u16, false)
            .map_err(|_| anyhow::anyhow!("CGEvent keyup creation failed"))?;
        key_up.post(CGEventTapLocation::HID);

        // Yield between chunks so the target app processes each batch.
        // 2ms default; terminal emulators may need 5ms (tunable later).
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    Ok(())
}
