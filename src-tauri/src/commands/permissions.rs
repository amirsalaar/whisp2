use crate::permissions;

#[tauri::command]
pub fn check_accessibility() -> bool {
    permissions::has_accessibility()
}

#[tauri::command]
pub fn open_accessibility_settings() {
    permissions::open_accessibility_settings();
}
