use crate::app_state::AppState;
use tauri::{Emitter, Manager};

pub async fn open_workspace_logic(
    path: String,
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let mut ws = state.workspace.lock().unwrap();
    ws.set_workspace(std::path::PathBuf::from(&path));
    drop(ws);
    crate::fs_watcher::restart_fs_watcher(app_handle, state);
    let _ = app_handle.emit(crate::events::event_names::REFRESH_EXPLORER, ());

    let language_service = state.language_service.clone();
    let workspace_path = path.clone();
    tokio::task::spawn_blocking(move || {
        eprintln!(
            "[LanguageService] Starting background workspace indexing: {}",
            workspace_path
        );
        match language_service.index_directory(".") {
            Ok(stats) => {
                eprintln!(
                    "[LanguageService] Workspace indexed: {} files, {} symbols in {}ms",
                    stats.files_indexed, stats.symbols_extracted, stats.duration_ms
                );
            }
            Err(e) => {
                eprintln!("[LanguageService] Workspace indexing failed: {}", e);
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn open_workspace(
    path: String,
    state: tauri::State<'_, AppState>,
    window: tauri::Window,
) -> Result<(), String> {
    open_workspace_logic(path, &*state, &window.app_handle()).await
}

pub fn list_files_logic(
    path: Option<String>,
    state: &AppState,
) -> Result<Vec<crate::explorer::FileEntry>, String> {
    let ws = state.workspace.lock().unwrap();
    let root = if let Some(p) = path {
        std::path::PathBuf::from(p)
    } else if let Some(w) = &ws.workspace {
        w.clone()
    } else {
        return Err("No workspace open".to_string());
    };

    Ok(crate::explorer::list_directory(&root))
}

#[tauri::command]
pub async fn list_files(
    path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::explorer::FileEntry>, String> {
    list_files_logic(path, &*state)
}

pub fn read_file_content_logic(path: String, state: &AppState) -> Result<String, String> {
    // Check if there's virtual content for this file
    let virtual_buffers = state.virtual_buffers.lock().unwrap();
    if let Some(virtual_content) = virtual_buffers.get(&path) {
        println!("[VIRTUAL BUFFER] Returning virtual content for: {}", path);
        return Ok(virtual_content.clone());
    }
    drop(virtual_buffers);

    // Resolve path relative to workspace if needed
    let resolved_path = {
        let p = std::path::PathBuf::from(&path);
        if p.is_absolute() {
            p
        } else {
            let ws = state.workspace.lock().unwrap();
            if let Some(root) = &ws.workspace {
                root.join(&p)
            } else {
                p
            }
        }
    };

    // No virtual content, read from disk
    match std::fs::read_to_string(&resolved_path) {
        Ok(content) => {
            if content.is_empty() {
                println!(
                    "[READ FILE CONTENT] Read empty content from: {} (requested: {})",
                    resolved_path.display(),
                    path
                );
            }
            Ok(content)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "[READ FILE CONTENT] Not found: {} (requested: {})",
                resolved_path.display(),
                path
            );
            Ok(String::new())
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn read_file_content(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    read_file_content_logic(path, &*state)
}

pub fn write_file_content_logic(
    path: String,
    content: String,
    state: &AppState,
) -> Result<(), String> {
    let p = std::path::PathBuf::from(&path);
    let resolved_path = if p.is_absolute() {
        p
    } else {
        let ws = state.workspace.lock().unwrap();
        if let Some(root) = ws.workspace.as_ref() {
            root.join(&path)
        } else {
            std::path::PathBuf::from(&path)
        }
    };

    std::fs::write(&resolved_path, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn write_file_content(
    path: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    write_file_content_logic(path, content, &*state)
}
