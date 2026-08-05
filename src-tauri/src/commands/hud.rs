//! Commands the floating island invokes on itself: the stop/cancel buttons, and
//! remembering where the user dragged it.

use tauri::State;

use crate::hotkey::mode::RecordingCommand;
use crate::AppState;

/// The island's current state, for the webview to adopt at boot.
///
/// Tauri events are not buffered, and the island is painted from Rust while its
/// webview is still loading — so the first `hud_state` event is usually emitted
/// before `hud.ts` is listening and is simply lost. Because the FSM emits only on
/// *change*, nothing re-sent it. Returns `None` before the first paint, in which
/// case the webview keeps the collapsed state it renders by default.
#[tauri::command]
pub fn hud_current_state() -> Option<String> {
    crate::hud::panel::current_payload()
}

/// Discard the in-flight recording without transcribing it. Drives the same FSM
/// path as a hotkey-driven cancel so the tray and the island stay in agreement.
#[tauri::command]
pub async fn hud_cancel_recording(state: State<'_, AppState>) -> Result<(), String> {
    tracing::info!("HUD cancel recording requested");
    state
        .recording_cmd_tx
        .send(RecordingCommand::Cancel)
        .await
        .map_err(|e| format!("recording task is gone: {e}"))
}

/// Stop recording and transcribe, as if the hotkey were released.
#[tauri::command]
pub async fn hud_stop_recording(state: State<'_, AppState>) -> Result<(), String> {
    tracing::info!("HUD stop recording requested");
    state
        .recording_cmd_tx
        .send(RecordingCommand::Stop)
        .await
        .map_err(|e| format!("recording task is gone: {e}"))
}

/// Persist the island's position after the user drags it.
///
/// Called from the drag-end handler in `hud.ts` rather than from a Tauri `Moved`
/// event, because `Moved` fires on every frame of the drag — that would rewrite
/// config.json hundreds of times per gesture.
#[tauri::command]
pub fn hud_save_position(state: State<'_, AppState>, x: f64, y: f64) -> Result<(), String> {
    // Reject non-finite values outright: NaN would serialize as `null` and come
    // back as "no saved position", silently losing the placement.
    if !x.is_finite() || !y.is_finite() {
        return Err(format!("refusing to save a non-finite island position ({x}, {y})"));
    }

    let config = {
        let mut lock = state.config.write().unwrap();
        if lock.hud_position == Some((x, y)) {
            return Ok(());
        }
        lock.hud_position = Some((x, y));
        lock.clone()
    };
    crate::config::persistence::save(&config).map_err(|e| e.to_string())
}
