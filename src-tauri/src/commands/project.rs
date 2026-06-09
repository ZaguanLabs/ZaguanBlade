use crate::app_state::AppState;
use crate::project_settings;
use crate::project_state;
use tauri::{Manager, State};

#[tauri::command]
pub async fn get_recent_workspaces(app_handle: tauri::AppHandle) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let workspace = state.workspace.lock().unwrap();
        workspace.get_recent_workspaces()
    })
    .await
    .map_err(|e| format!("get recent workspaces task failed: {}", e))
}

#[tauri::command]
pub fn get_current_workspace(state: State<'_, AppState>) -> Option<String> {
    let workspace = state.workspace.lock().unwrap();
    workspace.get_workspace_root()
}

#[tauri::command]
pub async fn load_project_state(
    state: State<'_, AppState>,
    project_path: String,
) -> Result<Option<project_state::ProjectState>, String> {
    let project_path_for_load = project_path.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        project_state::load_project_state(&project_path_for_load)
    })
    .await
    .map_err(|e| format!("load project state task failed: {}", e))?;

    if let Some(ref project_state) = loaded {
        state
            .uncommitted_changes
            .replace_all(project_state.uncommitted_changes.clone());
    } else {
        state.uncommitted_changes.clear();
    }

    Ok(loaded)
}

#[tauri::command]
pub async fn save_project_state(
    state: State<'_, AppState>,
    mut state_data: project_state::ProjectState,
) -> Result<(), String> {
    state_data.uncommitted_changes = state.uncommitted_changes.get_all();
    tokio::task::spawn_blocking(move || project_state::save_project_state(&state_data))
        .await
        .map_err(|e| format!("save project state task failed: {}", e))?
}

#[tauri::command]
pub async fn graceful_shutdown_with_state(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    mut state_data: project_state::ProjectState,
) -> Result<(), String> {
    state_data.uncommitted_changes = state.uncommitted_changes.get_all();

    if let Err(e) =
        tokio::task::spawn_blocking(move || project_state::save_project_state(&state_data))
            .await
            .map_err(|e| format!("graceful shutdown save task failed: {}", e))?
    {
        println!("[Backend] Failed to save state during shutdown: {}", e);
        // We continue to exit even if save fails, to prevent hanging
    } else {
        println!("[Backend] State saved successfully. Exiting.");
    }

    crate::commands::chat::graceful_close_active_chat_session(state.inner()).await;
    state.ws_connection.disconnect().await;

    app_handle.exit(0);
    Ok(())
}

#[tauri::command]
pub async fn get_project_state_path(project_path: String) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        project_state::get_project_state_path(&project_path)
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("get project state path task failed: {}", e))
}

#[tauri::command]
pub async fn read_binary_file(path: String) -> Result<Vec<u8>, String> {
    tokio::task::spawn_blocking(move || std::fs::read(&path))
        .await
        .map_err(|e| format!("read binary file task failed: {}", e))?
        .map_err(|e| format!("Failed to read binary file: {}", e))
}

#[tauri::command]
pub fn get_user_id(state: State<'_, AppState>) -> Option<String> {
    state.user_id.lock().unwrap().clone()
}

#[tauri::command]
pub async fn get_project_id(workspace_path: String) -> Result<Option<String>, String> {
    let path = std::path::PathBuf::from(workspace_path);
    tokio::task::spawn_blocking(move || crate::project::get_or_create_project_id(&path).ok())
        .await
        .map_err(|e| format!("get project id task failed: {}", e))
}

// Project Settings

#[tauri::command]
pub async fn load_project_settings(
    project_path: String,
) -> Result<project_settings::ProjectSettings, String> {
    let path = std::path::PathBuf::from(project_path);
    tokio::task::spawn_blocking(move || project_settings::load_project_settings(&path))
        .await
        .map_err(|e| format!("load project settings task failed: {}", e))?
}

#[tauri::command]
pub async fn save_project_settings(
    project_path: String,
    settings: project_settings::ProjectSettings,
) -> Result<(), String> {
    let path = std::path::PathBuf::from(project_path);
    tokio::task::spawn_blocking(move || project_settings::save_project_settings(&path, &settings))
        .await
        .map_err(|e| format!("save project settings task failed: {}", e))?
}

#[tauri::command]
pub async fn init_zblade_directory(project_path: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(project_path);
    tokio::task::spawn_blocking(move || project_settings::init_zblade_dir(&path))
        .await
        .map_err(|e| format!("init zblade directory task failed: {}", e))?
}

#[tauri::command]
pub async fn has_zblade_directory(project_path: String) -> Result<bool, String> {
    let path = std::path::PathBuf::from(project_path);
    tokio::task::spawn_blocking(move || project_settings::has_zblade_dir(&path))
        .await
        .map_err(|e| format!("has zblade directory task failed: {}", e))
}
