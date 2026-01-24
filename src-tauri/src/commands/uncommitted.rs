use crate::app_state::AppState;
use crate::uncommitted_changes::UncommittedChange;
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub fn get_uncommitted_changes(state: State<'_, AppState>) -> Vec<UncommittedChange> {
    state.uncommitted_changes.get_all()
}

#[tauri::command]
pub fn get_uncommitted_change(state: State<'_, AppState>, id: String) -> Option<UncommittedChange> {
    state.uncommitted_changes.get(&id)
}

#[tauri::command]
pub fn get_uncommitted_change_for_file(
    state: State<'_, AppState>,
    file_path: String,
) -> Option<UncommittedChange> {
    state
        .uncommitted_changes
        .get_by_path(&PathBuf::from(file_path))
}

#[tauri::command]
pub fn accept_change(state: State<'_, AppState>, id: String) -> Result<UncommittedChange, String> {
    state
        .uncommitted_changes
        .accept(&id)
        .ok_or_else(|| format!("Change not found: {}", id))
}

#[tauri::command]
pub fn accept_file_changes(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<UncommittedChange, String> {
    let path = PathBuf::from(&file_path);
    state
        .uncommitted_changes
        .accept_by_path(&path)
        .ok_or_else(|| format!("No uncommitted change for file: {}", file_path))
}

#[tauri::command]
pub fn accept_all_changes(state: State<'_, AppState>) -> Vec<UncommittedChange> {
    state.uncommitted_changes.accept_all()
}

#[tauri::command]
pub fn reject_change(state: State<'_, AppState>, id: String) -> Result<UncommittedChange, String> {
    state
        .uncommitted_changes
        .reject(&id, &state.history_service)
}

#[tauri::command]
pub fn reject_file_changes(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<UncommittedChange, String> {
    state
        .uncommitted_changes
        .reject_by_path(&PathBuf::from(file_path), &state.history_service)
}

#[tauri::command]
pub fn reject_all_changes(
    state: State<'_, AppState>,
) -> Result<Vec<UncommittedChange>, String> {
    state
        .uncommitted_changes
        .reject_all(&state.history_service)
}

#[tauri::command]
pub fn get_uncommitted_changes_count(state: State<'_, AppState>) -> usize {
    state.uncommitted_changes.count()
}
