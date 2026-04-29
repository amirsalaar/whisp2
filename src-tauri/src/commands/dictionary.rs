use tauri::State;

use crate::correction::dictionary::{load, save, SubEntry};
use crate::AppState;

#[tauri::command]
pub fn get_dictionary(_state: State<AppState>) -> Vec<SubEntry> {
    load().unwrap_or_default()
}

#[tauri::command]
pub fn add_dictionary_entry(_state: State<AppState>, from: String, to: String) -> Result<(), String> {
    let mut entries = load().map_err(|e| e.to_string())?;
    // Update if exists, else add
    if let Some(e) = entries.iter_mut().find(|e| e.from == from) {
        e.to = to;
    } else {
        entries.push(SubEntry { from, to });
    }
    save(&entries).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_dictionary_entry(_state: State<AppState>, from: String) -> Result<(), String> {
    let mut entries = load().map_err(|e| e.to_string())?;
    entries.retain(|e| e.from != from);
    save(&entries).map_err(|e| e.to_string())
}
