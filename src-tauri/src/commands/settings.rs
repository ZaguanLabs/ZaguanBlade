use crate::app_state::AppState;
use crate::config::{self, ApiConfig};
use tauri::State;

#[tauri::command]
pub fn get_global_settings(state: State<'_, AppState>) -> ApiConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_global_settings(settings: ApiConfig, state: State<'_, AppState>) -> Result<(), String> {
    let mut config = state.config.lock().unwrap();

    // Enforce hardcoded Blade URL
    let mut safe_settings = settings.clone();
    safe_settings.blade_url = "https://coder.zaguanai.com".to_string();

    *config = safe_settings.clone();

    // Persist to disk
    let path = config::default_api_config_path();
    config::save_api_config(&path, &config)?;

    Ok(())
}
