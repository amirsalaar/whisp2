use crate::permissions;

// ── Microphone (iOS + macOS) ────────────────────────────────────────────────

#[tauri::command]
pub fn check_microphone() -> bool {
    permissions::has_microphone()
}

#[tauri::command]
pub fn request_microphone() {
    permissions::request_microphone_access();
}

#[tauri::command]
pub fn open_microphone_settings() {
    permissions::open_microphone_settings();
}

// ── macOS-only (stubs on other platforms) ──────────────────────────────────

#[tauri::command]
pub fn check_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    { permissions::has_accessibility() }
    #[cfg(not(target_os = "macos"))]
    { false }
}

#[tauri::command]
pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    permissions::open_accessibility_settings();
}

#[tauri::command]
pub fn check_input_monitoring() -> bool {
    #[cfg(target_os = "macos")]
    { permissions::has_input_monitoring() }
    #[cfg(not(target_os = "macos"))]
    { false }
}

#[tauri::command]
pub fn request_input_monitoring() {
    #[cfg(target_os = "macos")]
    permissions::request_input_monitoring();
}

#[tauri::command]
pub fn open_input_monitoring_settings() {
    #[cfg(target_os = "macos")]
    permissions::open_input_monitoring_settings();
}
