use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc::SyncSender};

use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};

use crate::app_context;
use crate::config::models::HotkeyTrigger;

use super::mode::HotkeyEvent;

// Raw device-specific modifier flag bitmasks (from IOKit NXEventPrivate.h)
const NX_DEVICELCMDKEYMASK: u64 = 0x0000_0008;
const NX_DEVICERCMDKEYMASK: u64 = 0x0000_0010;
const NX_DEVICELALTKEYMASK: u64 = 0x0000_0020;
const NX_DEVICERALTKEYMASK: u64 = 0x0000_0040;
const NX_DEVICERCTLKEYMASK: u64 = 0x0000_2000;

extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

pub fn device_mask_for_trigger(trigger: &HotkeyTrigger) -> u64 {
    match trigger {
        HotkeyTrigger::LeftOption => NX_DEVICELALTKEYMASK,
        HotkeyTrigger::RightOption => NX_DEVICERALTKEYMASK,
        HotkeyTrigger::LeftCommand => NX_DEVICELCMDKEYMASK,
        HotkeyTrigger::RightCommand => NX_DEVICERCMDKEYMASK,
        HotkeyTrigger::RightControl => NX_DEVICERCTLKEYMASK,
    }
}

/// Installs a CGEventTap on the main thread's run loop.
///
/// `mask_atom` is shared with `AppState` — write a new value via `device_mask_for_trigger`
/// to change the active hotkey at runtime without reinstalling the tap.
///
/// Spawns a background thread that re-enables the tap every 5 s. macOS silently
/// disables taps whose callbacks are too slow; this ensures it stays active.
///
/// Must be called from the main thread.
/// Requires Accessibility permission (AXIsProcessTrusted).
pub fn install(
    trigger: HotkeyTrigger,
    sender: SyncSender<HotkeyEvent>,
    mask_atom: Arc<AtomicU64>,
) -> anyhow::Result<()> {
    let initial_mask = device_mask_for_trigger(&trigger);
    mask_atom.store(initial_mask, Ordering::Relaxed);
    let mask_atom_cb = Arc::clone(&mask_atom);

    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::FlagsChanged],
        move |_proxy, _event_type, event| {
            let device_mask = mask_atom_cb.load(Ordering::Relaxed);
            let flags = event.get_flags();
            let raw: u64 = flags.bits();
            let active = (raw & device_mask) != 0;

            let hotkey_event = if active {
                // Capture frontmost app while on main thread, before the HUD shifts focus.
                let bundle_id = app_context::frontmost_bundle_id();
                HotkeyEvent::KeyDown(bundle_id)
            } else {
                HotkeyEvent::KeyUp
            };

            let _ = sender.try_send(hotkey_event);
            None
        },
    )
    .map_err(|_| anyhow::anyhow!(
        "CGEventTap creation failed. Make sure Accessibility permission is granted \
         in System Settings → Privacy & Security → Accessibility."
    ))?;

    let loop_source = tap
        .mach_port
        .create_runloop_source(0)
        .map_err(|_| anyhow::anyhow!("failed to create CFRunLoopSource from event tap"))?;

    let runloop = CFRunLoop::get_current();
    unsafe {
        runloop.add_source(&loop_source, kCFRunLoopCommonModes);
    }
    tap.enable();

    // Extract the raw mach port ref as a usize so we can send it to the health-check thread.
    // Safety: the tap is kept alive by std::mem::forget below; the port remains valid for
    // the process lifetime. TCFType is in scope via the use above.
    let port_ref = TCFType::as_concrete_TypeRef(&tap.mach_port) as usize;

    // Keep tap and runloop source alive for the process lifetime.
    std::mem::forget(tap);
    std::mem::forget(loop_source);

    // Health-check thread: re-enable the tap every 5 s in case macOS silently disabled it.
    std::thread::Builder::new()
        .name("cgeventtap-health".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            unsafe {
                CGEventTapEnable(port_ref as CFMachPortRef, true);
            }
        })
        .expect("failed to spawn cgeventtap-health thread");

    tracing::info!("CGEventTap installed for {:?}", trigger);
    Ok(())
}
