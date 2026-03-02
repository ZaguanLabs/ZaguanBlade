use crate::app_state::AppState;
use tauri::{Emitter, Manager};

fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;

    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => normalized.push(p.as_os_str()),
            Component::RootDir => normalized.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(c) => normalized.push(c),
        }
    }
    normalized
}

pub(crate) fn resolve_path_under_workspace_root(
    workspace_root: &std::path::Path,
    path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let ws = std::fs::canonicalize(workspace_root).map_err(|e| {
        format!(
            "Failed to canonicalize workspace root '{}': {}",
            workspace_root.display(),
            e
        )
    })?;

    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        ws.join(path)
    };
    let normalized = normalize_path(&candidate);

    if !normalized.starts_with(&ws) {
        return Err(format!(
            "Path is outside workspace (workspace: {}, path: {})",
            ws.display(),
            normalized.display()
        ));
    }

    Ok(normalized)
}

pub(crate) fn resolve_path_under_workspace(
    state: &AppState,
    path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let workspace_root = {
        let ws = state.workspace.lock().unwrap();
        ws.workspace
            .clone()
            .ok_or_else(|| "No workspace open".to_string())?
    };

    resolve_path_under_workspace_root(&workspace_root, path)
}

pub async fn open_workspace_logic(
    path: String,
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let mut ws = state.workspace.lock().unwrap();
    ws.set_workspace(std::path::PathBuf::from(&path));
    drop(ws);

    // Re-discover git repository for new workspace
    let new_git_dir = match gix::discover(&path) {
        Ok(repo) => {
            let git_path = repo.path().to_path_buf();
            eprintln!("[GIT] Discovered repository at: {:?}", git_path);
            Some(git_path)
        }
        Err(e) => {
            eprintln!("[GIT] No repository found: {}", e);
            None
        }
    };
    *state.git_dir.write().unwrap() = new_git_dir;

    crate::fs_watcher::restart_fs_watcher(app_handle);
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
    // Virtual buffers removal - surgically removed.

    let requested_path = std::path::PathBuf::from(&path);
    let resolved_path = resolve_path_under_workspace(state, &requested_path)?;

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
    let requested_path = std::path::PathBuf::from(&path);
    let resolved_path = resolve_path_under_workspace(state, &requested_path)?;

    std::fs::write(&resolved_path, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn write_file_content(
    path: String,
    content: String,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    write_file_content_logic(path, content, &*state)?;
    let _ = app_handle.emit(crate::events::event_names::REFRESH_EXPLORER, ());
    Ok(())
}

#[tauri::command]
pub async fn open_file_in_editor(
    path: String,
    window: tauri::Window,
) -> Result<(), String> {
    // Emit the open-file event to trigger the frontend to open the file
    window.emit("open-file", &path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_path_blocks_parent_traversal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let res = resolve_path_under_workspace_root(
            dir.path(),
            std::path::Path::new("../../etc/passwd"),
        );
        assert!(res.is_err());
    }

    #[test]
    fn resolve_path_blocks_absolute_outside_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = std::env::temp_dir().join("outside-zblade-test.txt");
        let res = resolve_path_under_workspace_root(dir.path(), &outside);
        assert!(res.is_err());
    }

    #[test]
    fn resolve_path_allows_in_workspace_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = resolve_path_under_workspace_root(
            dir.path(),
            std::path::Path::new("src/main.rs"),
        )
        .expect("path should resolve");
        assert!(resolved.starts_with(dir.path()));
        assert!(resolved.ends_with("src/main.rs"));
    }
}
