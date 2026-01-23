use crate::app_state::AppState;
use crate::project_settings;
use crate::project_state;
use tauri::State;

#[tauri::command]
pub fn get_recent_workspaces(state: State<'_, AppState>) -> Vec<String> {
    let workspace = state.workspace.lock().unwrap();
    workspace.get_recent_workspaces()
}

#[tauri::command]
pub fn get_current_workspace(state: State<'_, AppState>) -> Option<String> {
    let workspace = state.workspace.lock().unwrap();
    workspace.get_workspace_root()
}

#[tauri::command]
pub fn load_project_state(project_path: String) -> Option<project_state::ProjectState> {
    project_state::load_project_state(&project_path)
}

#[tauri::command]
pub fn save_project_state(state_data: project_state::ProjectState) -> Result<(), String> {
    project_state::save_project_state(&state_data)
}

#[tauri::command]
pub fn graceful_shutdown_with_state(
    app_handle: tauri::AppHandle,
    state_data: project_state::ProjectState,
) -> Result<(), String> {
    if let Err(e) = project_state::save_project_state(&state_data) {
        println!("[Backend] Failed to save state during shutdown: {}", e);
        // We continue to exit even if save fails, to prevent hanging
    } else {
        println!("[Backend] State saved successfully. Exiting.");
    }

    app_handle.exit(0);
    Ok(())
}

#[tauri::command]
pub fn get_project_state_path(project_path: String) -> Option<String> {
    project_state::get_project_state_path(&project_path).map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
pub fn read_binary_file(path: String) -> Result<Vec<u8>, String> {
    std::fs::read(&path).map_err(|e| format!("Failed to read binary file: {}", e))
}

#[tauri::command]
pub fn get_user_id(state: State<'_, AppState>) -> Option<String> {
    state.user_id.lock().unwrap().clone()
}

#[tauri::command]
pub fn get_project_id(workspace_path: String) -> Option<String> {
    let path = std::path::PathBuf::from(workspace_path);
    crate::project::get_or_create_project_id(&path).ok()
}

// Project Settings

#[tauri::command]
pub fn load_project_settings(project_path: String) -> project_settings::ProjectSettings {
    let path = std::path::PathBuf::from(project_path);
    project_settings::load_project_settings(&path)
}

#[tauri::command]
pub fn save_project_settings(
    project_path: String,
    settings: project_settings::ProjectSettings,
) -> Result<(), String> {
    let path = std::path::PathBuf::from(project_path);
    project_settings::save_project_settings(&path, &settings)
}

#[tauri::command]
pub fn init_zblade_directory(project_path: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(project_path);
    project_settings::init_zblade_dir(&path)
}

#[tauri::command]
pub fn has_zblade_directory(project_path: String) -> bool {
    let path = std::path::PathBuf::from(project_path);
    project_settings::has_zblade_dir(&path)
}
