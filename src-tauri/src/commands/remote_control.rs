use crate::app_state::AppState;
use crate::config::{default_api_config_path, load_api_config, save_api_config};
use crate::remote_control::RemoteControlStatus;
use serde::Deserialize;
use tauri::State;

#[derive(Deserialize)]
struct TelegramUser {
    username: String,
}

#[derive(Deserialize)]
struct GetMeResponse {
    ok: bool,
    result: Option<TelegramUser>,
    description: Option<String>,
}

/// Get current remote control status (pairing state, connected user, etc.)
#[tauri::command]
pub async fn get_remote_control_status(
    state: State<'_, AppState>,
) -> Result<RemoteControlStatus, String> {
    Ok(state.remote_control.get_status().await)
}

/// Set and verify a Telegram Bot Token.
#[tauri::command]
pub async fn set_telegram_bot_token(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
) -> Result<RemoteControlStatus, String> {
    let trimmed_token = token.trim().to_string();
    if trimmed_token.is_empty() {
        return Err("Bot token cannot be empty".to_string());
    }

    let client = reqwest::Client::new();
    let res = client
        .get(&format!(
            "https://api.telegram.org/bot{}/getMe",
            trimmed_token
        ))
        .send()
        .await
        .map_err(|e| format!("Failed to reach Telegram: {}", e))?;

    let get_me: GetMeResponse = res
        .json()
        .await
        .map_err(|e| format!("Failed to parse Telegram response: {}", e))?;

    if !get_me.ok {
        return Err(get_me
            .description
            .unwrap_or_else(|| "Invalid bot token".to_string()));
    }

    let bot_username = get_me.result.unwrap().username;

    // Save token in config
    let config_path = default_api_config_path();
    let mut config = load_api_config(&config_path);
    config.telegram_bot_token = trimmed_token.clone();
    if let Err(e) = save_api_config(&config_path, &config) {
        eprintln!("Failed to save bot token: {}", e);
    }

    // Set state
    state
        .remote_control
        .set_configured(bot_username.clone())
        .await;

    // Start polling in background
    crate::telegram_service::start_polling(app_handle.clone(), trimmed_token, bot_username).await;

    Ok(state.remote_control.get_status().await)
}

/// Disconnect the remote control session
#[tauri::command]
pub async fn disconnect_remote_control(
    state: State<'_, AppState>,
) -> Result<RemoteControlStatus, String> {
    state.remote_control.disconnect().await;

    // Clear config
    let config_path = default_api_config_path();
    let mut config = load_api_config(&config_path);
    config.telegram_bot_token = String::new();
    let _ = save_api_config(&config_path, &config);

    // Stop polling
    crate::telegram_service::stop_polling().await;

    Ok(state.remote_control.get_status().await)
}
