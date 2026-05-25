use tauri::State;

use crate::config::models::AppConfig;
use crate::config::persistence;
use crate::AppState;

#[cfg(target_os = "macos")]
use crate::hotkey::event_tap::device_mask_for_trigger;

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state.config.read().unwrap().clone();
    Ok(config)
}

#[tauri::command]
pub fn set_config(state: State<'_, AppState>, config: AppConfig) -> Result<(), String> {
    // Update live hotkey mask so CGEventTap picks up changes immediately (macOS only).
    #[cfg(target_os = "macos")]
    {
        use std::sync::atomic::Ordering;
        let new_mask = device_mask_for_trigger(&config.hotkey);
        state.hotkey_mask.store(new_mask, Ordering::Relaxed);
    }

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

/// Wipe all user data: downloaded models, transcription history, config file,
/// and Keychain-stored API keys. Resets the in-memory config to defaults and
/// invalidates the cached WhisperContext. Hotkey/tray keep running, but the
/// next launch (or settings re-open) starts as if first install.
#[tauri::command]
pub async fn reset_app_data(state: State<'_, AppState>) -> Result<(), String> {
    use std::fs;

    // 1. History table
    crate::history::store::delete_all(&state.db)
        .await
        .map_err(|e| format!("clear history: {e}"))?;

    // 2. Models directory — delete *.bin only, leave the dir
    if let Ok(dir) = persistence::app_support_dir().map(|d| d.join("models")) {
        if dir.exists() {
            for entry in fs::read_dir(&dir).map_err(|e| format!("read models dir: {e}"))? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.path().extension().and_then(|s| s.to_str()) == Some("bin") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    // 3. Config file — delete so next load returns defaults
    if let Ok(p) = persistence::config_path() {
        let _ = fs::remove_file(&p);
    }

    // 4. In-memory config + cached Whisper model
    {
        let mut lock = state.config.write().unwrap();
        *lock = AppConfig::default();
    }
    if let Ok(mut ctx) = state.whisper_ctx.try_lock() {
        *ctx = (None, None);
    }

    // 5. Keychain — best-effort, ignore not-found
    for key in ["openai_api_key", "groq_api_key", "gemini_api_key"] {
        let _ = crate::keychain::delete(key);
    }

    Ok(())
}

#[tauri::command]
pub fn get_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    { "macos" }
    #[cfg(target_os = "ios")]
    { "ios" }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    { "other" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_platform_returns_known_value() {
        let p = get_platform();
        assert!(
            p == "macos" || p == "ios" || p == "other",
            "unexpected platform: {p}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_platform_is_macos_on_macos() {
        assert_eq!(get_platform(), "macos");
    }
}
