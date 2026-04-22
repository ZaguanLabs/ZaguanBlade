use crate::app_state::AppState;
use std::path::Path;
use tauri::{Manager, Runtime};

fn invalidate_recent_file_tool_cache<R: Runtime>(app_handle: &tauri::AppHandle<R>) {
    let state = app_handle.state::<AppState>();
    let workflow_lock = state.workflow.lock();
    if let Ok(mut workflow) = workflow_lock {
        workflow.clear_recent_file_tool_cache();
    }
}

pub(crate) fn sync_after_write<R: Runtime>(app_handle: &tauri::AppHandle<R>, path: &Path, content: &str) {
    invalidate_recent_file_tool_cache(app_handle);

    let state = app_handle.state::<AppState>();
    match state.language_service() {
        Ok(service) => {
            if let Err(error) = service.did_open(&path.to_string_lossy(), content) {
                eprintln!(
                    "[FILE SYNC] Failed to refresh language snapshot for {}: {}",
                    path.display(),
                    error
                );
            }
        }
        Err(error) => {
            eprintln!(
                "[FILE SYNC] Failed to get language service for {}: {}",
                path.display(),
                error
            );
        }
    }
}

pub(crate) fn sync_from_disk_after_write<R: Runtime>(app_handle: &tauri::AppHandle<R>, path: &Path) {
    invalidate_recent_file_tool_cache(app_handle);

    match std::fs::read_to_string(path) {
        Ok(content) => sync_after_write(app_handle, path, &content),
        Err(error) => {
            eprintln!(
                "[FILE SYNC] Failed to read {} after write/revert: {}",
                path.display(),
                error
            );
        }
    }
}

pub(crate) fn sync_after_delete<R: Runtime>(app_handle: &tauri::AppHandle<R>, path: &Path) {
    invalidate_recent_file_tool_cache(app_handle);

    let state = app_handle.state::<AppState>();
    match state.language_service() {
        Ok(service) => {
            if let Err(error) = service.remove_file(&path.to_string_lossy()) {
                eprintln!(
                    "[FILE SYNC] Failed to remove language snapshot for {}: {}",
                    path.display(),
                    error
                );
            }
        }
        Err(error) => {
            eprintln!(
                "[FILE SYNC] Failed to get language service for {}: {}",
                path.display(),
                error
            );
        }
    }
}
