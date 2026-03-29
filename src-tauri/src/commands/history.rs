use crate::app_state::AppState;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub async fn get_file_history(
    path: String,
    _state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<crate::history::HistoryEntry>, String> {
    let path_buf = PathBuf::from(path);
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let history_service = state.history_service()?;
        Ok(history_service.get_history(&path_buf))
    })
    .await
    .map_err(|e| format!("get file history task failed: {}", e))?
}

#[tauri::command]
pub async fn revert_file_to_snapshot(
    snapshot_id: String,
    _state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let history_service = state.history_service()?;
        history_service.revert_to(&snapshot_id)
    })
    .await
    .map_err(|e| format!("revert snapshot task failed: {}", e))?
}

#[tauri::command]
pub async fn undo_batch(
    group_id: String,
    _state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let history_service = state.history_service()?;
        history_service.undo_batch(&group_id)
    })
    .await
    .map_err(|e| format!("undo batch task failed: {}", e))?
}
