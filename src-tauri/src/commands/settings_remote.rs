use crate::app_state::AppState;
use crate::config::{self, RemoteAiConfig};
use tauri::State;

#[tauri::command]
pub fn get_remote_ai_settings(state: State<'_, AppState>) -> RemoteAiConfig {
    state.config.lock().unwrap().remote_config()
}

#[tauri::command]
pub fn save_remote_ai_settings(
    settings: RemoteAiConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Err(e) = config::ensure_global_prompts_dir() {
        eprintln!("[CONFIG] Failed to ensure global prompts directory: {}", e);
    }

    // Enforce hardcoded Blade URL for remote flow.
    let mut safe_settings = settings;
    safe_settings.blade_url = "https://coder.zaguanai.com".to_string();

    {
        let mut cfg = state.config.lock().unwrap();
        cfg.apply_remote_config(&safe_settings);
    }

    let path = config::default_api_config_path();
    config::save_remote_ai_config(&path, &safe_settings)
}
