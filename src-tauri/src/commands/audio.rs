use crate::audio::capture;

#[tauri::command]
pub fn list_audio_input_devices() -> Vec<String> {
    capture::list_input_devices()
}
