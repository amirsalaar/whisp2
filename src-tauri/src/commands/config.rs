use tauri::State;

use crate::config::models::AppConfig;
use crate::config::persistence;
use crate::AppState;

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state.config.read().unwrap().clone();
    Ok(config)
}

#[tauri::command]
pub fn set_config(state: State<'_, AppState>, config: AppConfig) -> Result<(), String> {
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
