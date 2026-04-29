use std::sync::mpsc::SyncSender;

use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};

use crate::config::models::HotkeyTrigger;

use super::mode::HotkeyEvent;

// Raw device-specific modifier flag bitmasks (from IOKit NXEventPrivate.h)
// These distinguish Left vs Right modifier keys within CGEventFlags.
const NX_DEVICELCMDKEYMASK: u64 = 0x0000_0008;
const NX_DEVICERCMDKEYMASK: u64 = 0x0000_0010;
const NX_DEVICELALTKEYMASK: u64 = 0x0000_0020;
const NX_DEVICERALTKEYMASK: u64 = 0x0000_0040;
const NX_DEVICERCTLKEYMASK: u64 = 0x0000_2000;

/// Returns the raw device-specific bitmask for the configured hotkey trigger.
fn device_mask_for_trigger(trigger: &HotkeyTrigger) -> u64 {
    match trigger {
        HotkeyTrigger::LeftOption => NX_DEVICELALTKEYMASK,
        HotkeyTrigger::RightOption => NX_DEVICERALTKEYMASK,
        HotkeyTrigger::LeftCommand => NX_DEVICELCMDKEYMASK,
        HotkeyTrigger::RightCommand => NX_DEVICERCMDKEYMASK,
        HotkeyTrigger::RightControl => NX_DEVICERCTLKEYMASK,
    }
}

/// Installs a CGEventTap on the main thread's run loop that monitors modifier key
/// changes. When the configured hotkey is pressed/released, sends HotkeyEvent
/// over the provided SyncSender.
///
/// Must be called from the main thread.
/// Requires Accessibility permission (AXIsProcessTrusted).
pub fn install(trigger: HotkeyTrigger, sender: SyncSender<HotkeyEvent>) -> anyhow::Result<()> {
    let device_mask = device_mask_for_trigger(&trigger);

    // CGEventTap::new takes a callback. We use a move closure that captures
    // device_mask and sender.
    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::FlagsChanged],
        move |_proxy, _event_type, event| {
            let flags = event.get_flags();
            let raw: u64 = flags.bits();
            let active = (raw & device_mask) != 0;

            let hotkey_event = if active {
                HotkeyEvent::KeyDown
            } else {
                HotkeyEvent::KeyUp
            };

            // Non-blocking send — drop events if the receiver isn't keeping up.
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

    // Keep tap alive for the process lifetime (leak is intentional here).
    std::mem::forget(tap);
    std::mem::forget(loop_source);

    tracing::info!("CGEventTap installed for {:?}", trigger);
    Ok(())
}
