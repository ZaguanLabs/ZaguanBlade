use crate::app_state::AppState;
use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio::fs;

pub(crate) fn resolve_path_under_workspace_root(
    workspace_root: &std::path::Path,
    path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    crate::worktree::resolve_path_under_workspace_root(workspace_root, path)
}

fn workspace_root_path(state: &AppState) -> Result<std::path::PathBuf, String> {
    let ws = state.workspace.lock().unwrap();
    ws.workspace
        .clone()
        .ok_or_else(|| "No workspace open".to_string())
}

pub(crate) async fn resolve_path_under_workspace_async(
    state: &AppState,
    path: std::path::PathBuf,
) -> Result<std::path::PathBuf, String> {
    let workspace_root = workspace_root_path(state)?;

    tokio::task::spawn_blocking(move || resolve_path_under_workspace_root(&workspace_root, &path))
        .await
        .map_err(|e| format!("path resolution task failed: {}", e))?
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspacePathMatch {
    pub path: String,
    pub is_dir: bool,
}

pub fn search_workspace_paths_logic(
    query: String,
    limit: Option<usize>,
    state: &AppState,
) -> Result<Vec<WorkspacePathMatch>, String> {
    let store = state.worktree()?;
    Ok(store
        .search_paths(&query, limit)
        .into_iter()
        .map(|path_match| WorkspacePathMatch {
            path: path_match.path,
            is_dir: path_match.is_dir,
        })
        .collect())
}

pub async fn open_workspace_logic(
    path: String,
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let blocking_app_handle = app_handle.clone();
    let blocking_path = path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let state = blocking_app_handle.state::<AppState>();
        {
            let mut ws = state.workspace.lock().unwrap();
            ws.set_workspace(std::path::PathBuf::from(&blocking_path));
        }

        // Re-discover git repository for new workspace
        let new_git_dir = match gix::discover(&blocking_path) {
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

        // Ensure project-local .zblade exists for workspace-scoped persistence
        let workspace_root = std::path::PathBuf::from(&blocking_path);
        crate::project_settings::init_zblade_dir(&workspace_root)?;
        state.reset_project_services()?;
        let _ = state.worktree()?;
        Ok(())
    })
    .await
    .map_err(|e| format!("failed to initialize workspace: {}", e))??;

    crate::fs_watcher::restart_fs_watcher(app_handle);
    crate::blade_event_scheduler::queue_refresh_explorer(app_handle);

    if state
        .startup_services_started
        .load(std::sync::atomic::Ordering::Acquire)
    {
        let blocking_app_handle = app_handle.clone();
        tokio::task::spawn_blocking(move || {
            let state = blocking_app_handle.state::<AppState>();
            if let Ok(language_service) = state.language_service() {
                let _ = language_service.index_directory(".");
            }
        });
    }

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
    let workspace_root = workspace_root_path(state)?;
    let resolved_path = match path.as_ref() {
        Some(path) => {
            resolve_path_under_workspace_root(&workspace_root, std::path::Path::new(path))?
        }
        None => workspace_root,
    };

    Ok(crate::explorer::list_directory(&resolved_path))
}

#[tauri::command]
pub async fn list_files(
    path: Option<String>,
    _state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<Vec<crate::explorer::FileEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        list_files_logic(path, &*state)
    })
    .await
    .map_err(|e| format!("list files task failed: {}", e))?
}

#[tauri::command]
pub async fn search_workspace_paths(
    query: String,
    limit: Option<usize>,
    _state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<Vec<WorkspacePathMatch>, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        search_workspace_paths_logic(query, limit, &*state)
    })
    .await
    .map_err(|e| format!("search workspace paths task failed: {}", e))?
}

#[tauri::command]
pub async fn write_file_content(
    path: String,
    content: String,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let requested_path = std::path::PathBuf::from(&path);
    let resolved_path = resolve_path_under_workspace_async(&*state, requested_path).await?;

    fs::write(&resolved_path, content)
        .await
        .map_err(|e| e.to_string())?;
    crate::blade_event_scheduler::queue_refresh_explorer(&app_handle);
    Ok(())
}

#[tauri::command]
pub async fn open_file_in_editor(path: String, window: tauri::Window) -> Result<(), String> {
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
        let res =
            resolve_path_under_workspace_root(dir.path(), std::path::Path::new("../../etc/passwd"));
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
        let resolved =
            resolve_path_under_workspace_root(dir.path(), std::path::Path::new("src/main.rs"))
                .expect("path should resolve");
        assert!(resolved.starts_with(dir.path()));
        assert!(resolved.ends_with("src/main.rs"));
    }
}
