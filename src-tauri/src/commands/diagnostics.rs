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

/// Largest amount of log text returned to the UI for copy/paste. The daily files
/// are read newest-first up to this cap so a bug-report paste stays manageable
/// while still covering the recent failure; the full files remain on disk and
/// are reachable via `open_log_dir`.
#[cfg(target_os = "macos")]
const MAX_LOG_BYTES: u64 = 256 * 1024;

/// Reads the most recent desktop log content (newest daily files first, up to
/// `MAX_LOG_BYTES`) for the user to copy into a bug report. Returns lines in
/// chronological order. macOS only; other targets return empty.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn read_recent_logs() -> Result<String, String> {
    Ok(String::new())
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn read_recent_logs() -> Result<String, String> {
    let log_dir = crate::config::persistence::app_support_dir()
        .map_err(|e| e.to_string())?
        .join("logs");

    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&log_dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("whisp.log"))
                .unwrap_or(false)
        })
        .collect();

    // Newest filename last → reading from the end walks newest-first. The daily
    // suffix (whisp.log.YYYY-MM-DD) sorts chronologically as a plain string.
    files.sort();

    let mut chunks: Vec<String> = Vec::new();
    let mut total: u64 = 0;
    for path in files.iter().rev() {
        if total >= MAX_LOG_BYTES {
            break;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            total += content.len() as u64;
            chunks.push(content);
        }
    }

    // chunks is newest-first; reverse back to chronological order for reading.
    chunks.reverse();
    Ok(chunks.join(""))
}

/// Reveals the logs directory in Finder so the user can attach the full files
/// to a bug report. macOS only; other targets are a no-op.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn open_log_dir() -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn open_log_dir() -> Result<(), String> {
    let log_dir = crate::config::persistence::app_support_dir()
        .map_err(|e| e.to_string())?
        .join("logs");
    std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    std::process::Command::new("open")
        .arg(&log_dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
