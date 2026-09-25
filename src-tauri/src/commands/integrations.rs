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
use tauri::Manager;
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
    state: tauri::State<'_, AppState>,
    integration_id: Uuid,
    expected_revision: String,
    name: String,
    value: Option<String>,
) -> Result<(), RuntimeError> {
    runtime_desktop_only(&window)?;
    let runtime = state.integrations.clone();
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
        let result = match value {
            Some(value) => {
                crate::integrations::credentials::OsSecrets::set(integration_id, &name, &value)
            }
            None => crate::integrations::credentials::OsSecrets::delete(integration_id, &name),
        };
        if result.is_ok() {
            runtime.credentials_changed(integration_id);
        }
        result
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
    let runtime = window.state::<AppState>().integrations.clone();
    let saved = tokio::task::spawn_blocking(move || {
        let saved = IntegrationStore::new(crate::config::default_global_config_dir())
            .save(&expected_revision, config)?;
        runtime.reconcile_connections();
        Ok::<_, ConfigError>(saved)
    })
    .await
    .map_err(|_| ConfigError::WriteFailed)??;
    crate::index_policy::refresh();
    crate::startup::refresh_symbols_index(window.app_handle());
    Ok(saved)
}

#[derive(serde::Serialize)]
pub struct SymbolsIndexStatus {
    pub workspace: crate::integrations::identity::WorkspaceIdentity,
    pub enabled: bool,
    pub health: crate::language_service::IndexHealthSnapshot,
}

#[tauri::command]
pub async fn get_symbols_index_status(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    workspace_path: String,
) -> Result<SymbolsIndexStatus, String> {
    desktop_only(&window).map_err(|_| "desktop_only")?;
    let documents = state.document_service()?;
    let service = state
        .language_service
        .read()
        .map_err(|_| "symbols_index_policy_unavailable")?
        .clone();
    tokio::task::spawn_blocking(move || {
        let root = std::fs::canonicalize(workspace_path).map_err(|_| "workspace_changed")?;
        if root != documents.workspace_root() || documents.cancellation().is_cancelled() {
            return Err("workspace_changed".into());
        }
        crate::index_policy::refresh();
        let enabled = crate::index_policy::enabled(&root)?;
        let mut health = service
            .filter(|service| service.uses_documents(&documents))
            .map(|service| service.index_health_snapshot())
            .unwrap_or_default();
        if crate::index_policy::stopping(&root) {
            health.status = crate::language_service::IndexHealthStatus::Stopping;
        } else if !enabled {
            health.status = crate::language_service::IndexHealthStatus::Disabled;
        }
        if documents.cancellation().is_cancelled() {
            return Err("workspace_changed".into());
        }
        Ok(SymbolsIndexStatus {
            workspace: documents.identity().clone(),
            enabled,
            health,
        })
    })
    .await
    .map_err(|_| "symbols_index_policy_unavailable".to_string())?
}

#[tauri::command]
pub async fn prepare_mcp_connection(
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
        let root =
            std::fs::canonicalize(workspace_path).map_err(|_| RuntimeError::WorkspaceChanged)?;
        if root != documents.workspace_root() {
            return Err(RuntimeError::WorkspaceChanged);
        }
        runtime.prepare_connection(integration_id, expected_revision, &documents)
    });
    tokio::time::timeout(std::time::Duration::from_secs(20), preparation)
        .await
        .map_err(|_| RuntimeError::TimedOut)?
        .map_err(|_| RuntimeError::LaunchFailed)?
}

#[tauri::command]
pub async fn connect_mcp(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    ticket_id: Uuid,
) -> Result<crate::integrations::connections::ConnectionStatus, RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    state.integrations.connect(ticket_id, documents).await
}

#[tauri::command]
pub async fn get_mcp_connections(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    workspace_path: String,
) -> Result<Vec<crate::integrations::connections::ConnectionStatus>, RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    let runtime = state.integrations.clone();
    tokio::task::spawn_blocking(move || {
        let root =
            std::fs::canonicalize(workspace_path).map_err(|_| RuntimeError::WorkspaceChanged)?;
        if root != documents.workspace_root() {
            return Err(RuntimeError::WorkspaceChanged);
        }
        runtime.connections.statuses(&documents)
    })
    .await
    .map_err(|_| RuntimeError::WorkspaceChanged)?
}

#[tauri::command]
pub fn disconnect_mcp(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    connection_id: Uuid,
) -> Result<(), RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    state
        .integrations
        .connections
        .disconnect(connection_id, &documents)
}

#[tauri::command]
pub fn refresh_mcp_catalog(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    connection_id: Uuid,
) -> Result<(), RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    state
        .integrations
        .connections
        .refresh(connection_id, &documents)
}

#[tauri::command]
pub fn get_mcp_catalog(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    connection_id: Uuid,
) -> Result<crate::integrations::catalog::McpCatalog, RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    state
        .integrations
        .connections
        .catalog(connection_id, &documents)
}
