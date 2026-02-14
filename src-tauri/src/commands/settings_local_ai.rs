use crate::app_state::AppState;
use crate::config::{self, LocalAiConfig};
use tauri::State;

#[tauri::command]
pub fn get_local_ai_settings(state: State<'_, AppState>) -> LocalAiConfig {
    state.config.lock().unwrap().local_ai_config()
}

#[tauri::command]
pub fn save_local_ai_settings(
    settings: LocalAiConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.apply_local_ai_config(&settings);
    }

    let path = config::default_api_config_path();
    config::save_local_ai_config(&path, &settings)
}

#[tauri::command]
pub async fn test_local_ollama_connection(
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
pub fn refresh_local_ollama_models() -> Result<(), String> {
    crate::models::ollama::clear_cache();
    Ok(())
}

#[tauri::command]
pub async fn test_local_openai_compat_connection(
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
pub fn refresh_local_openai_compat_models() -> Result<(), String> {
    crate::models::openai_compat::clear_cache();
    Ok(())
}
