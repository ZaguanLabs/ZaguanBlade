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

    if let Err(e) = config::ensure_global_prompts_dir() {
        eprintln!("[CONFIG] Failed to ensure global prompts directory: {}", e);
    }

    // Enforce hardcoded Blade URL
    let mut safe_settings = settings.clone();
    safe_settings.blade_url = "https://coder.zaguanai.com".to_string();

    *config = safe_settings.clone();

    // Persist to disk
    let path = config::default_api_config_path();
    config::save_api_config(&path, &config)?;

    Ok(())
}

#[tauri::command]
pub async fn test_ollama_connection(
    state: State<'_, AppState>,
    ollama_url: Option<String>,
) -> Result<(), String> {
    let url = if let Some(url) = ollama_url {
        url
    } else {
        let config = state.config.lock().unwrap();
        config.ollama_url.clone()
    };
    crate::models::ollama::test_connection(&url).await
}

#[tauri::command]
pub fn refresh_ollama_models() -> Result<(), String> {
    crate::models::ollama::clear_cache();
    Ok(())
}

#[tauri::command]
pub async fn test_openai_compat_connection(
    state: State<'_, AppState>,
    server_url: Option<String>,
) -> Result<(), String> {
    let url = if let Some(url) = server_url {
        url
    } else {
        let config = state.config.lock().unwrap();
        config.openai_compat_url.clone()
    };
    crate::models::openai_compat::test_connection(&url).await
}

#[tauri::command]
pub fn refresh_openai_compat_models() -> Result<(), String> {
    crate::models::openai_compat::clear_cache();
    Ok(())
}
