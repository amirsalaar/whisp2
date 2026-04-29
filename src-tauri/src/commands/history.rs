use tauri::State;

use crate::history::models::HistoryEntry;
use crate::history::store;
use crate::AppState;

#[tauri::command]
pub async fn get_history(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<HistoryEntry>, String> {
    store::list(&state.db, limit.unwrap_or(100))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_history_entry(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    store::delete(&state.db, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    store::delete_all(&state.db)
        .await
        .map_err(|e| e.to_string())
}
