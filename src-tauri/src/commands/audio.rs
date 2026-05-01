use tauri::State;

use crate::audio::capture;
use crate::hotkey::mode::RecordingCommand;
use crate::AppState;

#[tauri::command]
pub fn list_audio_input_devices() -> Vec<String> {
    capture::list_input_devices()
}

#[tauri::command]
pub async fn start_recording_mobile(state: State<'_, AppState>) -> Result<(), String> {
    state
        .recording_cmd_tx
        .send(RecordingCommand::Start(None))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_recording_mobile(state: State<'_, AppState>) -> Result<(), String> {
    state
        .recording_cmd_tx
        .send(RecordingCommand::Stop)
        .await
        .map_err(|e| e.to_string())
}
