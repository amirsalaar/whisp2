//! Floating non-activating HUD panel shown during recording/processing.
//! Uses objc2-app-kit directly — Tauri windows always activate.
//!
//! All public functions must be called from the main thread
//! (via tauri::AppHandle::run_on_main_thread).

use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSFloatingWindowLevel, NSPanel, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSString};
use std::cell::RefCell;

thread_local! {
    static HUD: RefCell<Option<Retained<NSPanel>>> = const { RefCell::new(None) };
}

/// Creates the floating HUD panel. Must be called once from the main thread.
pub fn create() {
    let mtm = unsafe { MainThreadMarker::new_unchecked() };

    let rect = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(200.0, 80.0),
    );

    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;

    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        mtm.alloc::<NSPanel>(),
        rect,
        style,
        NSBackingStoreType::Buffered,
        false,
    );

    panel.setFloatingPanel(true);
    panel.setLevel(NSFloatingWindowLevel);

    let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::Stationary
        | NSWindowCollectionBehavior::FullScreenAuxiliary;
    panel.setCollectionBehavior(behavior);

    // Ignore mouse events so the HUD is purely informational.
    panel.setIgnoresMouseEvents(true);

    // Center on screen initially.
    panel.center();

    HUD.with(|h| {
        *h.borrow_mut() = Some(panel);
    });
}

/// Shows the HUD with a given title label. Main thread only.
pub fn show(label: &str) {
    HUD.with(|h| {
        if let Some(panel) = h.borrow().as_ref() {
            let title = NSString::from_str(label);
            panel.setTitle(&title);
            panel.orderFront(None);
        }
    });
}

/// Hides the HUD. Main thread only.
pub fn hide() {
    HUD.with(|h| {
        if let Some(panel) = h.borrow().as_ref() {
            panel.orderOut(None);
        }
    });
}
