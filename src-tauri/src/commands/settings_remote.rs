use crate::app_state::AppState;
use crate::config::{self, RemoteAiConfig};
use tauri::State;

#[tauri::command]
pub fn get_remote_ai_settings(state: State<'_, AppState>) -> RemoteAiConfig {
    state.config.lock().unwrap().remote_config()
}

#[tauri::command]
pub async fn save_remote_ai_settings(
    settings: RemoteAiConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.apply_remote_config(&settings);
    }

    let path = config::default_api_config_path();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = config::ensure_global_prompts_dir() {
            eprintln!("[CONFIG] Failed to ensure global prompts directory: {}", e);
        }
        config::save_remote_ai_config(&path, &settings)
    })
    .await
    .map_err(|e| format!("save remote ai settings task failed: {}", e))??;

    let next_settings = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock remote settings: {}", e))?
        .remote_config();
    let next_user_id = if next_settings.user_id.trim().is_empty() {
        None
    } else {
        Some(next_settings.user_id.clone())
    };
    state.warmup_client.update_credentials(
        config::default_blade_url(),
        next_settings.api_key.clone(),
        next_settings.user_id.clone(),
    );
    state
        .ws_connection
        .update_credentials(config::default_blade_url(), next_settings.api_key.clone())
        .await;
    let mut user_id = state
        .user_id
        .lock()
        .map_err(|e| format!("Failed to lock user ID: {}", e))?;
    *user_id = next_user_id;

    Ok(())
}
