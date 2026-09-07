//! Administrative configuration is exposed only to the local main webview.
//! Do not add these operations to BladeIntent or the remote-control dispatcher.
use crate::integrations::{
    config::{ConfigError, IntegrationConfig},
    store::{ConfigSnapshot, IntegrationStore},
};
use crate::{
    app_state::AppState,
    integrations::{probe::ProbeResult, runtime::LaunchReview, RuntimeError},
};
use uuid::Uuid;

fn runtime_desktop_only(window: &tauri::WebviewWindow) -> Result<(), RuntimeError> {
    desktop_only(window).map_err(|_| RuntimeError::DesktopOnly)
}

#[tauri::command]
pub async fn prepare_integration_test(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    integration_id: Uuid,
    expected_revision: String,
    workspace_path: String,
) -> Result<LaunchReview, RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    let runtime = state.integrations.clone();
    let preparation = tokio::task::spawn_blocking(move || {
        let requested =
            std::fs::canonicalize(workspace_path).map_err(|_| RuntimeError::WorkspaceChanged)?;
        if requested != documents.workspace_root() {
            return Err(RuntimeError::WorkspaceChanged);
        }
        runtime.prepare(integration_id, expected_revision, &documents)
    });
    tokio::time::timeout(std::time::Duration::from_secs(20), preparation)
        .await
        .map_err(|_| RuntimeError::TimedOut)?
        .map_err(|_| RuntimeError::LaunchFailed)?
}

#[tauri::command]
pub async fn run_integration_test(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    ticket_id: Uuid,
) -> Result<ProbeResult, RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    state.integrations.run(ticket_id, documents).await
}

#[tauri::command]
pub fn cancel_integration_test(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    ticket_id: Uuid,
) -> Result<(), RuntimeError> {
    runtime_desktop_only(&window)?;
    state.integrations.cancel(ticket_id);
    Ok(())
}

/// Write-only administration of a named integration secret. Values never enter
/// integrations.json, remote control, tool responses or diagnostic messages.
#[tauri::command]
pub async fn set_integration_secret(
    window: tauri::WebviewWindow,
    integration_id: Uuid,
    expected_revision: String,
    name: String,
    value: Option<String>,
) -> Result<(), RuntimeError> {
    runtime_desktop_only(&window)?;
    tokio::task::spawn_blocking(move || {
        let snapshot = IntegrationStore::new(crate::config::default_global_config_dir())
            .load()
            .map_err(|_| RuntimeError::ConfigChanged)?;
        if snapshot.revision != expected_revision
            || !snapshot
                .config
                .entries
                .iter()
                .any(|entry| entry.id == integration_id)
        {
            return Err(RuntimeError::ConfigChanged);
        }
        match value {
            Some(value) => {
                crate::integrations::credentials::OsSecrets::set(integration_id, &name, &value)
            }
            None => crate::integrations::credentials::OsSecrets::delete(integration_id, &name),
        }
    })
    .await
    .map_err(|_| RuntimeError::SecretUnavailable)?
}

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
