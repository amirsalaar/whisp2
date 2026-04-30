use crate::permissions;

#[tauri::command]
pub fn check_accessibility() -> bool {
    permissions::has_accessibility()
}

#[tauri::command]
pub fn open_accessibility_settings() {
    permissions::open_accessibility_settings();
}

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

#[tauri::command]
pub fn check_input_monitoring() -> bool {
    permissions::has_input_monitoring()
}

#[tauri::command]
pub fn request_input_monitoring() {
    permissions::request_input_monitoring();
}

#[tauri::command]
pub fn open_input_monitoring_settings() {
    permissions::open_input_monitoring_settings();
}
