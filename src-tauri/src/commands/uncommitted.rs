use crate::app_state::AppState;
use crate::uncommitted_changes::UncommittedChange;
use std::fs;
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub fn get_uncommitted_changes(state: State<'_, AppState>) -> Result<Vec<UncommittedChange>, String> {
    let history_service = state.history_service()?;
    let existing = state.uncommitted_changes.get_all();
    if existing.is_empty() {
        return Ok(existing);
    }

    let mut refreshed = Vec::with_capacity(existing.len());
    let mut changed_any = false;

    for mut change in existing {
        let current_modified_ms = crate::uncommitted_changes::file_modified_ms(&change.file_path);
        if current_modified_ms == change.file_modified_ms {
            refreshed.push(change);
            continue;
        }

        let snapshot = history_service.get_snapshot_content(&change.snapshot_id);
        let current = fs::read_to_string(&change.file_path);

        if let (Ok(base_content), Ok(new_content)) = (snapshot, current) {
            let unified_diff =
                crate::uncommitted_changes::generate_unified_diff(&base_content, &new_content);
            let (added, removed) = crate::uncommitted_changes::count_diff_stats(&unified_diff);

            if change.unified_diff != unified_diff
                || change.added_lines != added
                || change.removed_lines != removed
            {
                change.unified_diff = unified_diff;
                change.added_lines = added;
                change.removed_lines = removed;
                changed_any = true;
            }

            if change.file_modified_ms != current_modified_ms {
                change.file_modified_ms = current_modified_ms;
                changed_any = true;
            }
        }

        refreshed.push(change);
    }

    if changed_any {
        state.uncommitted_changes.replace_all(refreshed.clone());
    }

    Ok(refreshed)
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
    let history_service = state.history_service()?;
    state
        .uncommitted_changes
        .reject(&id, history_service.as_ref())
}

#[tauri::command]
pub fn reject_file_changes(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<UncommittedChange, String> {
    let history_service = state.history_service()?;
    state
        .uncommitted_changes
        .reject_by_path(&PathBuf::from(file_path), history_service.as_ref())
}

#[tauri::command]
pub fn reject_all_changes(state: State<'_, AppState>) -> Result<Vec<UncommittedChange>, String> {
    let history_service = state.history_service()?;
    state
        .uncommitted_changes
        .reject_all(history_service.as_ref())
}

#[tauri::command]
pub fn get_uncommitted_changes_count(state: State<'_, AppState>) -> usize {
    state.uncommitted_changes.count()
}
