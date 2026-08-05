//! Floating dock pill — bottom-center of screen, above the Dock.
//!
//! States drive the WebviewWindow content via Tauri events (`hud_state`).
//! AppKit panel flags are applied post-creation via raw msg_send! calls.

use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSScreen};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::mpsc;

use crate::hotkey::mode::RecordingState;

/// What the island is showing. A projection of `RecordingState` — the FSM in
/// `hotkey_task` owns the truth, this is only how it looks.
///
/// The string forms are the wire format for the `hud_state` event and must stay in
/// lockstep with the `HudState` union in `src-ui/src/hud.ts`; `as_str` is tested so
/// a renamed variant can't silently stop rendering.
#[derive(Debug, Clone, PartialEq)]
pub enum HudState {
    CollapsedIdle,
    ExpandedIdle,
    RecordingControls,
    Processing,
    /// A failure the user needs to read, e.g. a dead or missing mic. The message
    /// rides along in the event payload; the FSM clears it back to idle after a few
    /// seconds. Without this the message only ever appears in a tray tooltip.
    Error,
    Hidden,
}

impl HudState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::CollapsedIdle => "collapsed-idle",
            Self::ExpandedIdle => "expanded-idle",
            Self::RecordingControls => "recording-controls",
            Self::Processing => "processing",
            Self::Error => "error",
            Self::Hidden => "hidden",
        }
    }

    /// Whether the island should accept clicks in this state.
    ///
    /// The window is a fixed [`HUD_SIZE`] rect while the pill inside it is much
    /// smaller, and macOS hit-tests the whole window regardless of CSS
    /// transparency. So anything that accepts clicks is a click *blocker* the size
    /// of the whole window. `CollapsedIdle` — by far the most common state —
    /// therefore stays pass-through, and hovering expands it via the global mouse
    /// monitor instead of a click. `ExpandedIdle` and `RecordingControls` opt in
    /// because they have something to drag or press.
    fn needs_mouse_events(&self) -> bool {
        matches!(self, Self::ExpandedIdle | Self::RecordingControls)
    }
}

/// Logical size of the island window. The pill inside it resizes per state via CSS;
/// the window stays this size so the expanded pill and its drop shadow always fit.
pub const HUD_SIZE: (f64, f64) = (340.0, 88.0);

/// Creates the island window at `saved_position` (or bottom-center when `None`),
/// clamped onto the current screen. Failure is logged and swallowed: the island is
/// an enhancement, and the app must still dictate via the tray without it.
///
/// Must be called on the main thread — window creation and `NSScreen` both require
/// it. Callers off the main thread should use [`create_async`].
pub fn create(app: &AppHandle, saved_position: Option<(f64, f64)>) {
    if app.get_webview_window("hud").is_some() {
        return;
    }
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let (x, y) = resolve_screen_position(saved_position, mtm);

    let window = WebviewWindowBuilder::new(app, "hud", WebviewUrl::App("hud.html".into()))
        .title("Whisp HUD")
        .inner_size(HUD_SIZE.0, HUD_SIZE.1)
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
    // Start pass-through: the island opens collapsed, and a full-window click
    // blocker over the user's app is worse than a slow expand. See
    // `HudState::needs_mouse_events`.
    if let Err(e) = window.set_ignore_cursor_events(true) {
        tracing::warn!(
            "HUD could not be made click-through ({}); it may block clicks to the app behind it",
            e
        );
    }
}

/// [`create`], hopped onto the main thread. For callers on a Tauri command or task
/// thread (where window creation would otherwise be unsound).
pub fn create_async(app: &AppHandle, saved_position: Option<(f64, f64)>) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || create(&handle, saved_position)) {
        // Otherwise turning the island on in Settings silently does nothing.
        tracing::error!("could not create the HUD window: {}", e);
    }
}

/// Tears the island down. Used when the user turns it off in Settings, so the
/// toggle takes effect without a restart.
pub fn destroy(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("hud") {
        if let Err(e) = window.close() {
            tracing::warn!("closing the HUD window failed: {}", e);
        }
    }
}

/// The height Tauri/tao flips window coordinates against, in points.
///
/// Tauri's `position`/`outer_position` are top-left-origin, AppKit's are
/// bottom-left, and every conversion here has to use *the same* reference height
/// tao does or the two disagree. tao flips against `CGDisplay::main().pixels_high()`
/// — the **primary** display (`bottom_left_to_top_left` in
/// tao/src/platform_impl/macos/util/mod.rs) — which is a fixed property of the
/// display arrangement.
///
/// `NSScreen::mainScreen` is emphatically *not* the same thing: it is the screen
/// holding the key window, so it changes as the user focuses windows on different
/// displays. Using it here put the island's hover hot zone off by the difference
/// between the two displays' heights (~360pt for a 1080p primary with a 1440p
/// secondary), so the island either never expanded on hover or expanded with the
/// cursor nowhere near it, depending on which app had focus.
fn tauri_flip_height() -> f64 {
    core_graphics::display::CGDisplay::main().pixels_high() as f64
}

/// Reads the screen metrics and hands them to the pure placement logic in
/// [`crate::hud::position`].
fn resolve_screen_position(saved: Option<(f64, f64)>, mtm: MainThreadMarker) -> (f64, f64) {
    use crate::hud::position::{resolve_position, VisibleFrame};

    let screen = NSScreen::mainScreen(mtm);
    let visible = screen
        .as_ref()
        .map(|s| s.visibleFrame())
        .unwrap_or_else(|| NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)));
    // Must match what tao flips against, not this screen's own height — see
    // `tauri_flip_height`.
    let screen_height = tauri_flip_height();

    let frame = VisibleFrame {
        x: visible.origin.x,
        y: visible.origin.y,
        width: visible.size.width,
        height: visible.size.height,
    };
    resolve_position(saved, frame, screen_height, HUD_SIZE)
}

/// How often the cursor is sampled to decide whether the island should expand.
/// Sets the worst-case delay before a hover is noticed; the pill's own CSS
/// transition is longer than this, so the expansion still reads as immediate.
const PROXIMITY_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// Polls the cursor and reports *changes* in island proximity over
/// `proximity_tx`, for the lifetime of the process.
///
/// This deliberately polls instead of installing an `NSEvent`
/// `addGlobalMonitorForEventsMatchingMask` mouse-moved monitor. That monitor was
/// tried first and proved unusable: it reported a successful install (valid
/// token, correct `NSEventMaskMouseMoved` bits) and then delivered zero events
/// on most process launches, while a standalone monitor in another process
/// received them at the same instant. Install ordering relative to `NSApp.run`,
/// foreground vs. background launch, and token/block retention were each ruled
/// out without finding the trigger, so the island's core affordance was resting
/// on a callback that silently died. `NSEvent::mouseLocation` is a plain class
/// method with no block, no autoreleased token, and no callback registration —
/// there is nothing left to fail intermittently. Neither approach needs
/// Accessibility or Input Monitoring permission.
///
/// Sampling hops to the main thread: `NSEvent::mouseLocation` and the window
/// geometry reads in [`cursor_is_near_hud`] are AppKit calls, and reading window
/// geometry from the main thread also avoids a round-trip through tao's event-loop
/// message channel. `run_on_main_thread` dispatches without blocking this task.
/// While the island doesn't exist (turned off in
/// Settings) the hop is skipped and proximity is simply reported as `false`, so
/// the poll costs one map lookup per tick when the feature is off.
///
/// Only transitions are sent. That keeps the FSM from waking on every sample, but
/// it also means this task's idea of "near" must never diverge from the FSM's: the
/// FSM only ever learns of a change from a message here, so a transition dropped
/// on the assumption that it doesn't matter would leave the island stuck on the
/// stale value. Hence `false` is *sent* when the island disappears rather than
/// quietly assumed — the FSM's `cursor_near` outlives the window.
pub fn spawn_proximity_poll(app: AppHandle, proximity_tx: mpsc::Sender<bool>) {
    tokio::spawn(async move {
        let mut last = false;
        let mut ticker = tokio::time::interval(PROXIMITY_POLL);
        // Skip missed ticks rather than replaying them: proximity is a snapshot of
        // where the cursor is now, so a backlog has no value.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;

            let near = if app.get_webview_window("hud").is_none() {
                false
            } else {
                let (near_tx, near_rx) = tokio::sync::oneshot::channel();
                let app_for_hop = app.clone();
                if app
                    .run_on_main_thread(move || {
                        let loc = NSEvent::mouseLocation();
                        let _ = near_tx.send(cursor_is_near_hud(&app_for_hop, loc));
                    })
                    .is_err()
                {
                    // The event loop is gone, i.e. the app is shutting down.
                    break;
                }
                // The closure was dropped without running — nothing to report, and
                // the next tick will ask again.
                let Ok(near) = near_rx.await else { continue };
                near
            };

            if near != last {
                last = near;
                if proximity_tx.send(near).await.is_err() {
                    break;
                }
            }
        }
    });
}

/// Whether the cursor is close enough to the island to expand it. Reads the
/// island's live position each call so dragging it moves the hot zone with it.
///
/// `cursor` is in AppKit screen coordinates (bottom-left origin), as reported by
/// `NSEvent::mouseLocation`. Returns `false` if the window is gone or its geometry
/// can't be read — a missing island simply never claims proximity.
fn cursor_is_near_hud(app: &AppHandle, cursor: NSPoint) -> bool {
    let Some(window) = app.get_webview_window("hud") else { return false };
    let Ok(pos) = window.outer_position() else { return false };
    let Ok(size) = window.outer_size() else { return false };
    let Ok(scale) = window.scale_factor() else { return false };

    // outer_position/outer_size are physical pixels with a top-left origin; convert
    // to the logical, bottom-left space the cursor is reported in. `NSEvent::
    // mouseLocation` is relative to the primary display's bottom-left, which is the
    // same reference tao flips window positions against — see `tauri_flip_height`.
    let screen_height = tauri_flip_height();

    let left = pos.x as f64 / scale;
    let top = pos.y as f64 / scale;
    let width = size.width as f64 / scale;
    let height = size.height as f64 / scale;

    let bottom_appkit =
        crate::hud::position::appkit_bottom_from_tauri_top(top, height, screen_height);
    crate::hud::position::is_within_padding(
        (cursor.x, cursor.y),
        (left, bottom_appkit, width, height),
        PROXIMITY_PADDING,
    )
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

/// How close the cursor must come to the island before it expands, in points
/// beyond the window's own bounds.
const PROXIMITY_PADDING: f64 = 48.0;

/// The most recent `hud_state` payload, for replay when the island's webview
/// finishes booting.
///
/// The island is created and painted from Rust while its webview is still loading,
/// so the first `hud_state` event is routinely emitted before `hud.ts` has
/// registered its listener — and is silently dropped, since Tauri events aren't
/// buffered. The FSM then paints only on *change*, so the missed state was never
/// re-sent: hovering the island during the first fraction of a second left Rust
/// believing it was expanded while it still looked collapsed, with no click
/// target. The webview asks for this once it's listening (see `hud_current_state`).
static LAST_PAYLOAD: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The current island state as a `hud_state` payload, or `None` before the first
/// paint. Read by the island's webview at boot to close the race described on
/// [`LAST_PAYLOAD`].
pub fn current_payload() -> Option<String> {
    LAST_PAYLOAD.lock().ok()?.clone()
}

pub fn update(app: &AppHandle, state: HudState) {
    update_with_label(app, state, None);
}

/// Like `update`, but attaches a label the island renders as its caption — the
/// hotkey hint on `expanded-idle`, the failure text on `error`.
///
/// The wire format is `"<state>:<label>"`; `hud_state_payload` is the tested,
/// AppKit-free half so the encoding can't drift from the parser in `hud.ts`.
pub fn update_with_label(app: &AppHandle, state: HudState, label: Option<&str>) {
    let payload = hud_state_payload(&state, label);
    if let Ok(mut last) = LAST_PAYLOAD.lock() {
        *last = Some(payload.clone());
    }
    // A dropped emit desyncs the island from the FSM until the next state change,
    // since this paints only on change. `LAST_PAYLOAD` above is already updated, so
    // it's still what a fresh webview would adopt.
    if let Err(e) = app.emit_to(
        tauri::EventTarget::webview_window("hud"),
        "hud_state",
        payload,
    ) {
        tracing::warn!("HUD state emit failed ({}); the island may be showing a stale state", e);
    }
    if let Some(window) = app.get_webview_window("hud") {
        // Getting this wrong is user-visible either way: left click-through when it
        // should accept clicks, the Stop/Discard buttons are dead; left clickable
        // when it should pass through, an invisible HUD_SIZE rect swallows clicks
        // meant for the app behind it.
        if let Err(e) = window.set_ignore_cursor_events(!state.needs_mouse_events()) {
            tracing::warn!("HUD click-through toggle failed for {:?}: {}", state, e);
        }
    }
}

/// Encodes a state (+ optional label) for the `hud_state` event. Labels are
/// dropped for states that render no caption so the payload stays comparable.
fn hud_state_payload(state: &HudState, label: Option<&str>) -> String {
    match label {
        Some(lbl) if matches!(state, HudState::ExpandedIdle | HudState::Error) => {
            // Newlines would confuse nothing but would wrap badly in a one-line pill.
            format!("{}:{}", state.as_str(), lbl.replace('\n', " "))
        }
        _ => state.as_str().to_string(),
    }
}

/// Projects the recording FSM (plus cursor proximity) onto what the island shows.
/// Proximity only matters while idle — during recording or an error the island
/// stays expanded whether or not the cursor is nearby.
pub fn hud_state_for(state: &RecordingState, cursor_near: bool) -> HudState {
    match state {
        RecordingState::Idle if cursor_near => HudState::ExpandedIdle,
        RecordingState::Idle => HudState::CollapsedIdle,
        RecordingState::Recording => HudState::RecordingControls,
        RecordingState::Processing => HudState::Processing,
        RecordingState::Error(_) => HudState::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_a_distinct_wire_name() {
        let states = [
            HudState::CollapsedIdle,
            HudState::ExpandedIdle,
            HudState::RecordingControls,
            HudState::Processing,
            HudState::Error,
            HudState::Hidden,
        ];
        let mut names: Vec<&str> = states.iter().map(|s| s.as_str()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "hud_state wire names must be unique");
    }

    #[test]
    fn only_interactive_states_capture_the_cursor() {
        // These have something to drag or press.
        assert!(HudState::ExpandedIdle.needs_mouse_events());
        assert!(HudState::RecordingControls.needs_mouse_events());

        // The rest must fall through: the window is a full HUD_SIZE rect that
        // macOS hit-tests regardless of CSS transparency, so capturing the cursor
        // here would put an invisible click blocker over the user's app.
        // `CollapsedIdle` especially — it's the resting state.
        assert!(!HudState::CollapsedIdle.needs_mouse_events());
        assert!(!HudState::Processing.needs_mouse_events());
        assert!(!HudState::Error.needs_mouse_events());
        assert!(!HudState::Hidden.needs_mouse_events());
    }

    #[test]
    fn proximity_expands_the_island_only_while_idle() {
        assert_eq!(hud_state_for(&RecordingState::Idle, false), HudState::CollapsedIdle);
        assert_eq!(hud_state_for(&RecordingState::Idle, true), HudState::ExpandedIdle);
        for near in [true, false] {
            assert_eq!(
                hud_state_for(&RecordingState::Recording, near),
                HudState::RecordingControls
            );
            assert_eq!(
                hud_state_for(&RecordingState::Processing, near),
                HudState::Processing
            );
            assert_eq!(
                hud_state_for(&RecordingState::Error("dead mic".into()), near),
                HudState::Error
            );
        }
    }

    #[test]
    fn labels_ride_along_on_captioned_states() {
        assert_eq!(
            hud_state_payload(&HudState::Error, Some("No audio from 'MacBook Mic'")),
            "error:No audio from 'MacBook Mic'"
        );
        assert_eq!(
            hud_state_payload(&HudState::ExpandedIdle, Some("Hold fn to dictate")),
            "expanded-idle:Hold fn to dictate"
        );
    }

    #[test]
    fn labels_are_dropped_for_states_that_render_no_caption() {
        assert_eq!(
            hud_state_payload(&HudState::CollapsedIdle, Some("ignored")),
            "collapsed-idle"
        );
        assert_eq!(hud_state_payload(&HudState::Processing, Some("ignored")), "processing");
    }

    #[test]
    fn a_payload_without_a_label_is_the_bare_state_name() {
        assert_eq!(hud_state_payload(&HudState::Error, None), "error");
    }

    #[test]
    fn a_multiline_label_is_flattened_to_one_line() {
        // Transcription errors can arrive with embedded newlines from an HTTP body.
        assert_eq!(
            hud_state_payload(&HudState::Error, Some("request failed\nbad key")),
            "error:request failed bad key"
        );
    }
}
