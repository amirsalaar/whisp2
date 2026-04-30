use std::sync::atomic::Ordering;

use tauri::State;

use crate::config::models::AppConfig;
use crate::config::persistence;
use crate::hotkey::event_tap::device_mask_for_trigger;
use crate::AppState;

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state.config.read().unwrap().clone();
    Ok(config)
}

#[tauri::command]
pub fn set_config(state: State<'_, AppState>, config: AppConfig) -> Result<(), String> {
    // Update live hotkey mask so CGEventTap picks up changes immediately.
    let new_mask = device_mask_for_trigger(&config.hotkey);
    state.hotkey_mask.store(new_mask, Ordering::Relaxed);

    // If the model path changed, invalidate the cached WhisperContext so it reloads.
    {
        let old_path = state.config.read().unwrap().local_whisper_model_path.clone();
        if old_path != config.local_whisper_model_path {
            if let Ok(mut ctx) = state.whisper_ctx.try_lock() {
                *ctx = (None, None);
            }
        }
    }

    {
        let mut lock = state.config.write().unwrap();
        *lock = config.clone();
    }
    persistence::save(&config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_api_key(key_name: String) -> Result<Option<String>, String> {
    crate::keychain::get(&key_name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_api_key(key_name: String, value: String) -> Result<(), String> {
    crate::keychain::set(&key_name, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_api_key(key_name: String) -> Result<(), String> {
    crate::keychain::delete(&key_name).map_err(|e| e.to_string())
}

/// Opens the HuggingFace whisper.cpp model page in the default browser.
#[tauri::command]
pub fn open_model_url() {
    let _ = std::process::Command::new("open")
        .arg("https://huggingface.co/ggerganov/whisper.cpp/tree/main")
        .spawn();
}
