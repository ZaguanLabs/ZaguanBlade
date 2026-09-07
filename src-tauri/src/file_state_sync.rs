use crate::app_state::AppState;
use std::path::Path;
use tauri::{Manager, Runtime};

fn invalidate_recent_file_tool_cache<R: Runtime>(app_handle: &tauri::AppHandle<R>) {
    let state = app_handle.state::<AppState>();
    match state.workflow.try_lock() {
        Ok(mut workflow) => workflow.clear_recent_file_tool_cache(),
        Err(std::sync::TryLockError::WouldBlock) => {
            eprintln!(
                "[FILE SYNC] Skipping workflow cache invalidation because workflow lock is already held"
            );
        }
        Err(std::sync::TryLockError::Poisoned(_)) => {
            eprintln!(
                "[FILE SYNC] Failed to invalidate workflow file cache because workflow lock is poisoned"
            );
        }
    };
}

pub(crate) fn sync_after_write<R: Runtime>(
    app_handle: &tauri::AppHandle<R>,
    path: &Path,
    content: &str,
) {
    invalidate_recent_file_tool_cache(app_handle);

    let state = app_handle.state::<AppState>();
    let result = state.document_service().and_then(|documents| {
        documents
            .saved(&path.to_string_lossy(), content)
            .map_err(|error| error.to_string())?;
        state.language_service_for_documents(&documents)
    });
    match result {
        Ok(Some(service)) => {
            if let Err(error) = service.index_saved_document(&path.to_string_lossy(), content) {
                eprintln!(
                    "[FILE SYNC] Failed to refresh language snapshot for {}: {}",
                    path.display(),
                    error
                );
            }
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!(
                "[FILE SYNC] Failed to synchronize document state for {}: {}",
                path.display(),
                error
            );
        }
    }
}

pub(crate) fn sync_from_disk_after_write<R: Runtime>(
    app_handle: &tauri::AppHandle<R>,
    path: &Path,
) {
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
    let result = state.document_service().and_then(|documents| {
        documents
            .close(&path.to_string_lossy())
            .map_err(|error| error.to_string())?;
        state.language_service_for_documents(&documents)
    });
    match result {
        Ok(Some(service)) => {
            if let Err(error) = service.remove_deleted_file_index(&path.to_string_lossy()) {
                eprintln!(
                    "[FILE SYNC] Failed to remove language snapshot for {}: {}",
                    path.display(),
                    error
                );
            }
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!(
                "[FILE SYNC] Failed to synchronize document state for {}: {}",
                path.display(),
                error
            );
        }
    }
}
