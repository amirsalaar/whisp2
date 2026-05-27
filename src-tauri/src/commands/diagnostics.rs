//! Diagnostics surfaced in the iOS Settings → Diagnostics card.
//!
//! Reads the persistent on-device log written by `WhispLogger` (Swift). On
//! macOS these commands are stubs that return empty / succeed silently — the
//! desktop app already surfaces logs via stdout / Console.app.

#[cfg(target_os = "ios")]
use std::ffi::{c_char, CStr};

#[cfg(target_os = "ios")]
extern "C" {
    fn whisp_log_read() -> *mut c_char;
    fn whisp_log_free(ptr: *mut c_char);
    fn whisp_log_clear();
}

#[tauri::command]
pub fn read_ios_log() -> String {
    #[cfg(target_os = "ios")]
    unsafe {
        let ptr = whisp_log_read();
        if ptr.is_null() {
            return String::new();
        }
        let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        whisp_log_free(ptr);
        s
    }
    #[cfg(not(target_os = "ios"))]
    String::new()
}

#[tauri::command]
pub fn clear_ios_log() {
    #[cfg(target_os = "ios")]
    unsafe {
        whisp_log_clear();
    }
}
