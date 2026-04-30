use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc::SyncSender};

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
// CGEventFlags::maskSecondaryFn — set when the Fn/Globe key is held
const NX_SECONDARYFNKEYMASK: u64 = 0x0080_0000;

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
        HotkeyTrigger::Fn => NX_SECONDARYFNKEYMASK,
    }
}

/// Installs a CGEventTap on a **dedicated background thread** with its own CFRunLoop.
///
/// Why a dedicated thread instead of the Tauri main thread:
///   - Tauri's setup hook runs before the main CFRunLoop starts spinning.
///     Attaching a run-loop source during setup and then returning means the source
///     gets pumped only when Tauri happens to iterate the run loop, which is unreliable.
///   - A dedicated thread that calls CFRunLoop::run() guarantees the tap receives
///     ALL events continuously, even when the app is backgrounded.
///
/// Uses CGEventTapLocation::Session which requires only Accessibility permission.
/// Session-level taps see FlagsChanged events for all apps in the user's login session.
///
/// `mask_atom` is shared with AppState — write a new value to change the hotkey
/// at runtime without reinstalling the tap.
pub fn install(
    trigger: HotkeyTrigger,
    sender: SyncSender<HotkeyEvent>,
    mask_atom: Arc<AtomicU64>,
) -> anyhow::Result<()> {
    let initial_mask = device_mask_for_trigger(&trigger);
    mask_atom.store(initial_mask, Ordering::Relaxed);

    // Use a one-shot channel to propagate tap creation errors back to the caller.
    let (err_tx, err_rx) = std::sync::mpsc::sync_channel::<Option<String>>(1);

    std::thread::Builder::new()
        .name("cgeventtap-runloop".into())
        .spawn(move || {
            let mask_atom_cb = Arc::clone(&mask_atom);

            let tap = match CGEventTap::new(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                vec![CGEventType::FlagsChanged],
                move |_proxy, _event_type, event| {
                    let device_mask = mask_atom_cb.load(Ordering::Relaxed);
                    let raw: u64 = event.get_flags().bits();
                    let active = (raw & device_mask) != 0;

                    let hotkey_event = if active {
                        let bundle_id = app_context::frontmost_bundle_id();
                        HotkeyEvent::KeyDown(bundle_id)
                    } else {
                        HotkeyEvent::KeyUp
                    };

                    let _ = sender.try_send(hotkey_event);
                    None
                },
            ) {
                Ok(t) => {
                    let _ = err_tx.send(None); // success
                    t
                }
                Err(_) => {
                    let _ = err_tx.send(Some(
                        "CGEventTap creation failed — make sure Accessibility permission \
                         is granted in System Settings → Privacy & Security → Accessibility."
                            .into(),
                    ));
                    return;
                }
            };

            let loop_source = match tap.mach_port.create_runloop_source(0) {
                Ok(s) => s,
                Err(_) => {
                    tracing::error!("failed to create CFRunLoopSource from event tap");
                    return;
                }
            };

            let runloop = CFRunLoop::get_current();
            unsafe {
                runloop.add_source(&loop_source, kCFRunLoopCommonModes);
            }
            tap.enable();

            // Extract raw port ref for the health-check thread.
            use core_foundation::base::TCFType;
            let port_ref = TCFType::as_concrete_TypeRef(&tap.mach_port) as usize;

            // Keep tap and source alive for the process lifetime.
            std::mem::forget(tap);
            std::mem::forget(loop_source);

            // Health-check: macOS silently disables taps whose callbacks are slow.
            // Re-enable every 5 s to keep it alive.
            std::thread::Builder::new()
                .name("cgeventtap-health".into())
                .spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    unsafe { CGEventTapEnable(port_ref as CFMachPortRef, true); }
                })
                .expect("failed to spawn cgeventtap-health thread");

            tracing::info!("CGEventTap run loop spinning on dedicated thread");
            CFRunLoop::run_current(); // blocks this thread forever — pumps the tap
        })
        .expect("failed to spawn cgeventtap-runloop thread");

    // Wait for the tap to be created (or fail) before returning.
    match err_rx.recv() {
        Ok(None) => {
            tracing::info!("CGEventTap installed for {:?}", trigger);
            Ok(())
        }
        Ok(Some(msg)) => Err(anyhow::anyhow!("{}", msg)),
        Err(_) => Err(anyhow::anyhow!("CGEventTap thread exited unexpectedly")),
    }
}
