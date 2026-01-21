use crate::app_state::AppState;
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub fn get_file_history(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<crate::history::HistoryEntry>, String> {
    let path_buf = PathBuf::from(path);
    Ok(state.history_service.get_history(&path_buf))
}

#[tauri::command]
pub fn revert_file_to_snapshot(
    snapshot_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.history_service.revert_to(&snapshot_id)
}
