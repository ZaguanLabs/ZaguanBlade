use crate::app_state::AppState;
use crate::config::{self, LocalAiConfig};
use tauri::State;

#[tauri::command]
pub fn get_local_ai_settings(state: State<'_, AppState>) -> LocalAiConfig {
    state.config.lock().unwrap().local_ai_config()
}

#[tauri::command]
pub async fn save_local_ai_settings(
    settings: LocalAiConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.apply_local_ai_config(&settings);
    }

    let path = config::default_api_config_path();
    tokio::task::spawn_blocking(move || config::save_local_ai_config(&path, &settings))
        .await
        .map_err(|e| format!("save local ai settings task failed: {}", e))?
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

#[derive(serde::Serialize)]
pub struct LocalModelPromptStatus {
    pub id: String,
    pub name: String,
    pub provider: Option<String>,
    /// True when a user-saved prompt file resolves for this model.
    pub has_local_prompt: bool,
    /// The matched prompt file name (e.g. "llama3:8b.md"), when `has_local_prompt`.
    pub prompt_file: Option<String>,
    /// Which bundled default applies when there is no local prompt: "local" | "cloud".
    pub builtin_kind: String,
}

/// Lists the currently-available LOCAL models (Ollama + OpenAI-compatible) together
/// with their user-prompt status, for the Settings -> Local AI models list.
#[tauri::command]
pub async fn list_local_models_with_prompt_status(
    state: State<'_, AppState>,
) -> Result<Vec<LocalModelPromptStatus>, String> {
    let config = { state.config.lock().unwrap().clone() };
    let models = crate::models::catalog::list_all_models(&config).await;
    let result = models
        .into_iter()
        .filter(|m| {
            matches!(
                m.provider.as_deref(),
                Some("ollama") | Some("openai-compat")
            )
        })
        .map(|m| {
            let (has_local_prompt, prompt_file) = config::local_prompt_status_for_model(&m.id);
            let builtin_kind = config::builtin_prompt_kind(&m.id).to_string();
            LocalModelPromptStatus {
                id: m.id,
                name: m.name,
                provider: m.provider,
                has_local_prompt,
                prompt_file,
                builtin_kind,
            }
        })
        .collect();
    Ok(result)
}
