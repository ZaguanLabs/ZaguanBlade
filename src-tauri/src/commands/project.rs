use crate::app_state::AppState;
use crate::project_settings;
use crate::project_state;
use tauri::State;

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

    // Kill any lingering background AI commands so they don't outlive the app.
    {
        use tauri::Manager;
        app_handle
            .state::<crate::terminal::TerminalManager>()
            .terminate_all_bg_jobs();
    }

    app_handle.exit(0);
    Ok(())
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
    tokio::task::spawn_blocking(move || {
        project_settings::save_project_settings(&path, &settings)?;
        crate::agent_skills::invalidate_skill_cache(Some(&path));
        Ok(())
    })
    .await
    .map_err(|e| format!("save project settings task failed: {}", e))?
}

#[tauri::command]
pub async fn get_skill_diagnostics(
    project_path: String,
) -> Result<crate::agent_skills::SkillDiagnosticsReport, String> {
    let path = std::path::PathBuf::from(project_path);
    tokio::task::spawn_blocking(move || {
        if !path.is_dir() {
            return Err(format!(
                "project path does not exist or is not a directory: {}",
                path.display()
            ));
        }
        Ok(crate::agent_skills::skill_diagnostics_report(&path))
    })
    .await
    .map_err(|e| format!("skill diagnostics task failed: {}", e))?
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
