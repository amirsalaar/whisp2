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

/// Carries over the config fields the settings form doesn't own.
///
/// `hud_position` is written by the island's own drag handler, and the settings
/// UI has no control for it — so it isn't in the object the frontend sends, and
/// `serde` fills it with the `None` default. Taking the incoming value verbatim
/// would therefore snap the island back to bottom-center every time the user
/// changed an unrelated setting.
fn preserve_backend_owned_fields(incoming: AppConfig, current: &AppConfig) -> AppConfig {
    AppConfig {
        hud_position: current.hud_position,
        ..incoming
    }
}

#[tauri::command]
pub fn set_config(
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))] app: tauri::AppHandle,
    state: State<'_, AppState>,
    config: AppConfig,
) -> Result<(), String> {
    let config = preserve_backend_owned_fields(config, &state.config.read().unwrap());

    // Update live hotkey mask so CGEventTap picks up changes immediately (macOS only).
    #[cfg(target_os = "macos")]
    {
        use std::sync::atomic::Ordering;
        let new_mask = device_mask_for_trigger(&config.hotkey);
        state.hotkey_mask.store(new_mask, Ordering::Relaxed);
    }

    // Show/hide the floating island immediately, so the toggle doesn't silently
    // require a restart. `create_async`/`destroy` are no-ops when already in the
    // requested state.
    #[cfg(target_os = "macos")]
    {
        let was_shown = state.config.read().unwrap().show_hud;
        if config.show_hud != was_shown {
            if config.show_hud {
                crate::hud::panel::create_async(&app, config.hud_position);
            } else {
                crate::hud::panel::destroy(&app);
            }
        }
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

    // Same for the Parakeet model directory.
    #[cfg(target_os = "macos")]
    {
        let old_path = state.config.read().unwrap().parakeet_model_path.clone();
        if old_path != config.parakeet_model_path {
            if let Ok(mut ctx) = state.parakeet_ctx.try_lock() {
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

/// Reads an API key from the Keychain.
///
/// `async` on purpose. Tauri runs a **synchronous** command on the main thread,
/// and a Keychain call can block there for a long time — indefinitely, if macOS
/// decides to put up an "allow access" prompt (which it does whenever the binary's
/// signature no longer matches the one that stored the item). While the main
/// thread is parked, `NSApp`'s runloop stops turning: the floating island freezes
/// mid-state, `run_on_main_thread` closures queue up without ever running, and
/// global NSEvent monitors deliver nothing. An `async` command runs on Tauri's
/// async runtime instead, so a slow or prompting Keychain can't wedge the UI.
/// Same reasoning for [`set_api_key`] and [`delete_api_key`].
#[tauri::command]
pub async fn get_api_key(key_name: String) -> Result<Option<String>, String> {
    crate::keychain::get(&key_name).map_err(|e| e.to_string())
}

/// The bullet placeholder the settings UI shows for an already-stored key.
/// It must never be written back as the real secret.
const MASKED_KEY_SENTINEL: &str = "••••••••";

/// Reject values that would corrupt a stored API key: empty/whitespace, or the
/// UI's masked placeholder. Persisting either overwrites a valid key with junk
/// and yields a 401 "Invalid API Key" on the next request. Returns the trimmed
/// key to store on success.
///
/// This is a backend guard mirroring the frontend check — defense in depth, so
/// a UI regression can't silently poison the keychain again.
fn validate_api_key(value: &str) -> Result<&str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("API key is empty.".into());
    }
    if trimmed == MASKED_KEY_SENTINEL {
        return Err("Refusing to store the masked placeholder as an API key.".into());
    }
    Ok(trimmed)
}

/// Stores an API key. `async` to keep the Keychain off the main thread; see
/// [`get_api_key`].
#[tauri::command]
pub async fn set_api_key(key_name: String, value: String) -> Result<(), String> {
    let key = validate_api_key(&value)?;
    crate::keychain::set(&key_name, key).map_err(|e| e.to_string())
}

/// Removes a stored API key. `async` to keep the Keychain off the main thread; see
/// [`get_api_key`].
#[tauri::command]
pub async fn delete_api_key(key_name: String) -> Result<(), String> {
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

    // 2. Models directory — delete downloaded models, leave the dir.
    //    Whisper models are `*.bin` files; Parakeet models are subdirectories
    //    of ONNX weights.
    if let Ok(dir) = persistence::app_support_dir().map(|d| d.join("models")) {
        if dir.exists() {
            for entry in fs::read_dir(&dir).map_err(|e| format!("read models dir: {e}"))? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                if path.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                } else if path.extension().and_then(|s| s.to_str()) == Some("bin") {
                    let _ = fs::remove_file(&path);
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
    #[cfg(target_os = "macos")]
    if let Ok(mut ctx) = state.parakeet_ctx.try_lock() {
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

    #[test]
    fn saving_settings_keeps_the_island_where_the_user_dragged_it() {
        // The settings form has no island-position control, so the payload it
        // sends always carries the serde default. Honoring that would reset the
        // island to bottom-center on any unrelated settings change.
        let current = AppConfig { hud_position: Some((200.0, 640.0)), ..Default::default() };
        let incoming = AppConfig { show_hud: false, ..Default::default() };

        let merged = preserve_backend_owned_fields(incoming, &current);

        assert_eq!(merged.hud_position, Some((200.0, 640.0)));
        // Everything the form *does* own still comes from the payload.
        assert!(!merged.show_hud);
    }

    #[test]
    fn a_never_dragged_island_stays_unpositioned() {
        let merged = preserve_backend_owned_fields(AppConfig::default(), &AppConfig::default());
        assert_eq!(merged.hud_position, None);
    }

    #[test]
    fn validate_api_key_rejects_masked_sentinel() {
        // The exact bug: saving the UI's bullet placeholder overwrote the real
        // key and produced a 401. The guard must reject it.
        assert!(validate_api_key(MASKED_KEY_SENTINEL).is_err());
    }

    #[test]
    fn validate_api_key_rejects_empty_and_whitespace() {
        assert!(validate_api_key("").is_err());
        assert!(validate_api_key("   ").is_err());
        assert!(validate_api_key("\t\n").is_err());
    }

    #[test]
    fn validate_api_key_accepts_real_key_and_trims() {
        // Groq keys look like "gsk_...", OpenAI "sk-...". Both must pass, and
        // surrounding whitespace (stray paste newline) is trimmed, not rejected.
        assert_eq!(validate_api_key("gsk_abc123").unwrap(), "gsk_abc123");
        assert_eq!(validate_api_key("  sk-xyz789\n").unwrap(), "sk-xyz789");
    }
}
