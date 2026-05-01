use tauri::State;
use crate::AppState;

#[tauri::command]
pub async fn hud_cancel_recording(_state: State<'_, AppState>) -> Result<(), String> {
    tracing::info!("HUD cancel recording requested");
    Ok(())
}

#[tauri::command]
pub async fn hud_stop_recording(_state: State<'_, AppState>) -> Result<(), String> {
    tracing::info!("HUD stop recording requested");
    Ok(())
}
