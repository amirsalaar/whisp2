//! Floating dock pill — bottom-center of screen, above the Dock.
//!
//! States:
//!   Hidden      — panel not visible
//!   Recording   — "● Recording" pill (180×44, red accent label)
//!   Processing  — "Transcribing…" pill (220×44, neutral)
//!
//! All public functions MUST be called from the main thread
//! (via tauri::AppHandle::run_on_main_thread).

use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSPanel, NSScreen, NSTextField,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSString};
use std::cell::RefCell;

const PILL_W_RECORDING: f64 = 180.0;
const PILL_W_PROCESSING: f64 = 220.0;
const PILL_H: f64 = 44.0;
const BOTTOM_OFFSET: f64 = 16.0;

#[derive(Clone, Debug)]
pub enum HudState {
    Hidden,
    Recording,
    Processing,
}

struct Hud {
    panel: Retained<NSPanel>,
    label: Retained<NSTextField>,
}

thread_local! {
    static HUD: RefCell<Option<Hud>> = const { RefCell::new(None) };
}

/// Creates the floating dock pill panel. Call once on main thread during app setup.
pub fn create() {
    let mtm = unsafe { MainThreadMarker::new_unchecked() };

    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(PILL_W_RECORDING, PILL_H));
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;

    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        mtm.alloc::<NSPanel>(),
        rect,
        style,
        NSBackingStoreType::Buffered,
        false,
    );

    panel.setFloatingPanel(true);

    unsafe {
        use objc2_app_kit::NSStatusWindowLevel;
        panel.setLevel(NSStatusWindowLevel);
    }

    let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::Stationary
        | NSWindowCollectionBehavior::FullScreenAuxiliary;
    panel.setCollectionBehavior(behavior);
    panel.setIgnoresMouseEvents(true);
    panel.setOpaque(false);
    panel.setHasShadow(true);

    unsafe {
        use objc2_app_kit::NSColor;
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
    }

    // Blurred dark pill background
    let blur_frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(PILL_W_RECORDING, PILL_H));
    let blur = NSVisualEffectView::initWithFrame(mtm.alloc::<NSVisualEffectView>(), blur_frame);
    blur.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    blur.setState(NSVisualEffectState::Active);
    unsafe {
        blur.setMaterial(NSVisualEffectMaterial::HUDWindow);
        blur.setWantsLayer(true);
        // Set corner radius via CALayer msg_send
        if let Some(layer) = blur.layer() {
            use objc2::msg_send;
            let _: () = msg_send![&*layer, setCornerRadius: 22.0_f64];
        }
    }

    // Status label
    let label_frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(PILL_W_RECORDING, PILL_H));
    let label = NSTextField::initWithFrame(mtm.alloc::<NSTextField>(), label_frame);
    label.setEditable(false);
    label.setSelectable(false);
    label.setBordered(false);
    label.setDrawsBackground(false);
    label.setAlignment(objc2_app_kit::NSTextAlignment::Center);
    unsafe {
        use objc2_app_kit::{NSColor, NSFont};
        label.setTextColor(Some(&NSColor::whiteColor()));
        label.setFont(Some(&NSFont::systemFontOfSize_weight(
            14.0,
            objc2_app_kit::NSFontWeightMedium,
        )));
    }
    let title = NSString::from_str("Recording...");
    label.setStringValue(&title);

    if let Some(content) = panel.contentView() {
        unsafe {
            content.addSubview(&blur);
            content.addSubview(&label);
        }
    }

    HUD.with(|h| {
        *h.borrow_mut() = Some(Hud { panel, label });
    });
}

/// Updates the pill state. Call from main thread only.
pub fn update(state: HudState) {
    let mtm = unsafe { MainThreadMarker::new_unchecked() };

    HUD.with(|h| {
        if let Some(hud) = h.borrow().as_ref() {
            match &state {
                HudState::Hidden => {
                    hud.panel.orderOut(None);
                }
                HudState::Recording | HudState::Processing => {
                    let (text, pill_w) = match &state {
                        HudState::Recording => ("● Recording", PILL_W_RECORDING),
                        HudState::Processing => ("Transcribing…", PILL_W_PROCESSING),
                        _ => unreachable!(),
                    };

                    let new_size = CGSize::new(pill_w, PILL_H);
                    let origin = screen_bottom_center(mtm, pill_w, PILL_H);
                    let panel_rect = CGRect::new(origin, new_size);

                    unsafe {
                        hud.panel.setFrame_display(panel_rect, false);
                        let view_rect = CGRect::new(CGPoint::new(0.0, 0.0), new_size);
                        hud.label.setFrame(view_rect);
                    }

                    let ns_text = NSString::from_str(text);
                    hud.label.setStringValue(&ns_text);
                    hud.panel.orderFront(None);
                }
            }
        }
    });
}

/// Legacy shim — maps label text to HudState.
pub fn show(label: &str) {
    let state = if label.is_empty() {
        HudState::Hidden
    } else if label.contains("Processing") || label.contains("Transcrib") {
        HudState::Processing
    } else if label.contains("Recording") {
        HudState::Recording
    } else {
        HudState::Hidden
    };
    update(state);
}

/// Legacy shim.
pub fn hide() {
    update(HudState::Hidden);
}

fn screen_bottom_center(mtm: MainThreadMarker, w: f64, _h: f64) -> CGPoint {
    let screen_rect = NSScreen::mainScreen(mtm)
        .map(|s| unsafe { s.visibleFrame() })
        .unwrap_or(CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(1440.0, 900.0)));
    let x = screen_rect.origin.x + (screen_rect.size.width - w) / 2.0;
    let y = screen_rect.origin.y + BOTTOM_OFFSET;
    CGPoint::new(x, y)
}
