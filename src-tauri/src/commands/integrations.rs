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

/// Interactive OAuth sign-in for a saved Streamable HTTP server. Opens the
/// authorization page in the user's browser and waits for its redirect to a
/// loopback port. `success` and `failure` are the translated texts the
/// browser shows afterwards.
#[tauri::command]
pub async fn sign_in_mcp_server(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    integration_id: Uuid,
    expected_revision: String,
    success: String,
    failure: String,
) -> Result<(), RuntimeError> {
    use tauri_plugin_opener::OpenerExt;
    runtime_desktop_only(&window)?;
    let bounded = |text: String| text.chars().take(512).collect::<String>();
    let texts = crate::integrations::oauth::CallbackPage {
        success: bounded(success),
        failure: bounded(failure),
    };
    let runtime = state.integrations.clone();
    runtime
        .sign_in(
            integration_id,
            expected_revision,
            |url| {
                app.opener()
                    .open_url(url, None::<&str>)
                    .map_err(|_| RuntimeError::AuthorizationFailed)
            },
            texts,
        )
        .await
}

#[tauri::command]
pub fn cancel_mcp_sign_in(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Result<(), RuntimeError> {
    runtime_desktop_only(&window)?;
    state.integrations.cancel_sign_in();
    Ok(())
}

/// Forgets this server's tokens on this computer and stops its connections.
#[tauri::command]
pub async fn sign_out_mcp_server(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    integration_id: Uuid,
) -> Result<(), RuntimeError> {
    runtime_desktop_only(&window)?;
    state.integrations.sign_out(integration_id).await
}

#[tauri::command]
pub async fn get_mcp_sign_in_status(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    integration_id: Uuid,
) -> Result<bool, RuntimeError> {
    runtime_desktop_only(&window)?;
    let runtime = state.integrations.clone();
    tokio::task::spawn_blocking(move || runtime.signed_in(integration_id))
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

#[derive(serde::Serialize)]
pub struct McpResourceServer {
    connection_id: Uuid,
    integration_id: Uuid,
    name: String,
}

/// Connected servers offering resources or prompts, for the chat composer.
/// Scoped to the workspace that is open now; the webview cannot name another.
fn feature_servers(
    window: &tauri::WebviewWindow,
    state: &AppState,
    prompts: bool,
) -> Result<Vec<McpResourceServer>, RuntimeError> {
    runtime_desktop_only(window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    Ok(state
        .integrations
        .connections
        .feature_servers(&documents, prompts)?
        .into_iter()
        .map(|status| McpResourceServer {
            connection_id: status.connection_id,
            integration_id: status.integration_id,
            name: status.name,
        })
        .collect())
}
#[tauri::command]
pub fn get_mcp_resource_servers(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<McpResourceServer>, RuntimeError> {
    feature_servers(&window, &state, false)
}
#[tauri::command]
pub fn get_mcp_prompt_servers(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<McpResourceServer>, RuntimeError> {
    feature_servers(&window, &state, true)
}

#[tauri::command]
pub async fn list_mcp_resources(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    connection_id: Uuid,
) -> Result<crate::integrations::resources::ResourceListing, RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    state
        .integrations
        .connections
        .list_resources(connection_id, &documents)
        .await
}

/// Reads one user-selected resource, by URI or by expanding a template with
/// user-entered values. Selecting it in the picker is the authorization.
#[tauri::command]
pub async fn read_mcp_resource(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    connection_id: Uuid,
    uri: Option<String>,
    template: Option<String>,
    values: Option<std::collections::HashMap<String, String>>,
) -> Result<crate::integrations::resources::ResourceRead, RuntimeError> {
    runtime_desktop_only(&window)?;
    let uri = match (uri, template) {
        (Some(uri), None) => uri,
        (None, Some(template)) => {
            crate::integrations::uri_template::expand(&template, &values.unwrap_or_default())?
        }
        _ => return Err(RuntimeError::InvalidArguments),
    };
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    state
        .integrations
        .connections
        .read_resource(connection_id, &documents, uri)
        .await
}

#[tauri::command]
pub async fn list_mcp_prompts(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    connection_id: Uuid,
) -> Result<crate::integrations::prompts::PromptListing, RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    state
        .integrations
        .connections
        .list_prompts(connection_id, &documents)
        .await
}

/// Generates one user-selected prompt for preview. Nothing reaches a
/// conversation until the user inserts it into the composer and sends it.
#[tauri::command]
pub async fn get_mcp_prompt(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    connection_id: Uuid,
    name: String,
    arguments: std::collections::HashMap<String, String>,
) -> Result<crate::integrations::prompts::PromptRender, RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    state
        .integrations
        .connections
        .get_prompt(connection_id, &documents, name, arguments)
        .await
}

// Conversation decisions are desktop-only and resolve backend-owned contexts.
// The webview cannot choose arguments, a connection, or a workspace generation.
fn current_mcp_turn(
    state: &AppState,
) -> Result<Option<std::sync::Arc<crate::integrations::native_turn::NativeMcpTurn>>, RuntimeError> {
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    let turn = state
        .chat_manager
        .lock()
        .map_err(|_| RuntimeError::Busy)?
        .native_mcp
        .clone();
    let conversation = state.conversation.lock().map_err(|_| RuntimeError::Busy)?;
    Ok(turn.filter(|turn| turn.matches(&documents, &conversation.metadata.id)))
}
#[tauri::command]
pub fn get_mcp_turn_state(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Result<Option<crate::integrations::native_turn::TurnState>, RuntimeError> {
    runtime_desktop_only(&window)?;
    current_mcp_turn(&state)?
        .map(|turn| turn.state())
        .transpose()
}
#[tauri::command]
pub async fn respond_mcp_tool_call(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    turn_id: Uuid,
    request_id: Uuid,
    allow: bool,
) -> Result<(), RuntimeError> {
    runtime_desktop_only(&window)?;
    if state.remote_control.is_configured().await {
        return Err(RuntimeError::PermissionDenied);
    }
    let turn = current_mcp_turn(&state)?
        .filter(|turn| turn.id == turn_id)
        .ok_or(RuntimeError::ApprovalExpired)?;
    turn.respond(request_id, allow)
}
#[tauri::command]
pub async fn get_mcp_result_artifact(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    reference: crate::integrations::result_artifact::ArtifactRef,
) -> Result<serde_json::Value, RuntimeError> {
    runtime_desktop_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    if state
        .conversation
        .lock()
        .map_err(|_| RuntimeError::Busy)?
        .metadata
        .id
        != reference.conversation_id.to_string()
    {
        return Err(RuntimeError::PermissionDenied);
    }
    let runtime = state.integrations.clone();
    let reading_documents = documents.clone();
    let reading_reference = reference.clone();
    let result = tokio::task::spawn_blocking(move || {
        runtime.read_result_artifact(
            &reading_documents.identity().workspace_id,
            &reading_reference,
        )
    })
    .await
    .map_err(|_| RuntimeError::ArtifactUnavailable)??;
    if documents.cancellation().is_cancelled() {
        return Err(RuntimeError::WorkspaceChanged);
    }
    if state
        .conversation
        .lock()
        .map_err(|_| RuntimeError::Busy)?
        .metadata
        .id
        != reference.conversation_id.to_string()
    {
        return Err(RuntimeError::PermissionDenied);
    }
    Ok(result)
}
