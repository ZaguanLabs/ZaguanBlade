//! Administrative configuration is exposed only to the local main webview.
//! Do not add these operations to BladeIntent or the remote-control dispatcher.
use crate::integrations::{
    config::{ConfigError, IntegrationConfig},
    store::{ConfigSnapshot, IntegrationStore},
};

fn desktop_only(window: &tauri::WebviewWindow) -> Result<(), ConfigError> {
    if window.label() != "main" {
        return Err(ConfigError::DesktopOnly);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_integration_settings(
    window: tauri::WebviewWindow,
) -> Result<ConfigSnapshot, ConfigError> {
    desktop_only(&window)?;
    tokio::task::spawn_blocking(|| {
        IntegrationStore::new(crate::config::default_global_config_dir()).load()
    })
    .await
    .map_err(|_| ConfigError::ReadFailed)?
}

#[tauri::command]
pub async fn save_integration_settings(
    window: tauri::WebviewWindow,
    expected_revision: String,
    config: IntegrationConfig,
) -> Result<ConfigSnapshot, ConfigError> {
    desktop_only(&window)?;
    tokio::task::spawn_blocking(move || {
        IntegrationStore::new(crate::config::default_global_config_dir())
            .save(&expected_revision, config)
    })
    .await
    .map_err(|_| ConfigError::WriteFailed)?
}
