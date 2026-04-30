use tauri::State;
use crate::AppState;
use crate::hotkey::mode::RecordingCommand;

#[tauri::command]
pub async fn hud_cancel_recording(state: State<'_, AppState>) -> Result<(), String> {
    state.recording_cmd_tx
        .send(RecordingCommand::Cancel)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn hud_stop_recording(state: State<'_, AppState>) -> Result<(), String> {
    state.recording_cmd_tx
        .send(RecordingCommand::Stop)
        .await
        .map_err(|e| e.to_string())
}
