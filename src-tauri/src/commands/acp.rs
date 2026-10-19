//! Conversations run by a local ACP agent instead of Blade. Local main webview
//! only: do not add these operations to BladeIntent or the remote-control
//! dispatcher. The agent's turns never reach the native or cloud model loop.
use crate::{
    app_state::AppState,
    conversation::ConversationHistory,
    conversation_store::{ConversationDelta, ConversationExecution, StoredConversation},
    integrations::{
        acp_config::{ConfigChange, SessionConfig},
        acp_session::{
            self, AcpEvent, AcpSession, FinishedTurn, PromptPart, SessionSetup, Sink, TurnRecorder,
        },
        agent_sessions::{AgentStatus, LiveAgent},
        runtime::LaunchReview,
        RuntimeError,
    },
    protocol::{ChatImage, ChatMessage, ChatRole},
};
use serde::Serialize;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

const MAX_IMAGES: usize = 8;

fn local_only(window: &tauri::WebviewWindow) -> Result<(), RuntimeError> {
    if window.label() != "main" {
        return Err(RuntimeError::DesktopOnly);
    }
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcpEventPayload<'a> {
    conversation_id: &'a str,
    event: &'a AcpEvent,
}

/// Appends to the conversation's transcript: through the live history when it
/// is the active conversation, otherwise straight to the store.
fn record(app: &AppHandle, conversation_id: &str, message: ChatMessage) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.with_conversation_store(|store| {
        let mut message = Some(message);
        let delta = {
            let mut conversation = state
                .conversation
                .lock()
                .map_err(|_| "Conversation unavailable".to_string())?;
            if conversation.metadata.id == conversation_id {
                conversation.push(message.take().expect("message"));
                let start = store.changed_tail_start(conversation_id, conversation.absolute_len());
                Some(ConversationDelta::from_history(&conversation, start))
            } else {
                None
            }
        };
        match (delta, message) {
            (Some(delta), _) => store.save_delta(delta),
            (None, Some(message)) => store.append_message(conversation_id, &message.into()),
            (None, None) => Ok(()),
        }
    })
}

fn finished_message(finished: FinishedTurn) -> ChatMessage {
    let mut message = ChatMessage::new(ChatRole::Assistant, finished.text);
    if !finished.reasoning.is_empty() {
        message.reasoning = Some(finished.reasoning);
    }
    message.agent_activity = Some(finished.activity);
    message
}

fn record_turn(app: &AppHandle, conversation_id: String, finished: FinishedTurn) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = record(&app, &conversation_id, finished_message(finished)) {
            eprintln!("[ACP] Failed to save the agent's turn: {error}");
        }
    });
}

/// Ends the live agent session unless it belongs to `keep`. A turn still
/// running is saved as cancelled first, into its own conversation.
pub(crate) fn end_agent_session_unless(app: &AppHandle, keep: Option<&str>) {
    let state = app.state::<AppState>();
    let Some(live) = state.integrations.agents.take_unless(keep) else {
        return;
    };
    let abandoned = live
        .recorder
        .lock()
        .ok()
        .and_then(|mut recorder| recorder.abandon(&live.name));
    live.close();
    if let Some(finished) = abandoned {
        if let Err(error) = record(app, &live.conversation_id, finished_message(finished)) {
            eprintln!("[ACP] Failed to save the interrupted turn: {error}");
        }
    }
}

#[tauri::command]
pub async fn prepare_acp_conversation(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    integration_id: Uuid,
    expected_revision: String,
    workspace_path: String,
) -> Result<LaunchReview, RuntimeError> {
    local_only(&window)?;
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
        runtime.prepare_agent(integration_id, expected_revision, &documents)
    });
    tokio::time::timeout(std::time::Duration::from_secs(20), preparation)
        .await
        .map_err(|_| RuntimeError::TimedOut)?
        .map_err(|_| RuntimeError::LaunchFailed)?
}

/// Starts the approved agent and a new conversation that it owns. The
/// previous conversation is saved and any previous agent session ends.
#[tauri::command]
pub async fn start_acp_conversation(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    ticket_id: Uuid,
) -> Result<AgentStatus, RuntimeError> {
    local_only(&window)?;
    let documents = state
        .document_service()
        .map_err(|_| RuntimeError::WorkspaceChanged)?;
    let launched = state
        .integrations
        .launch_agent(ticket_id, documents.clone())
        .await?;
    let conversation: Arc<OnceLock<String>> = Arc::default();
    let recorder: Arc<Mutex<TurnRecorder>> = Arc::default();
    let sink: Sink = {
        let app = app.clone();
        let conversation = conversation.clone();
        let recorder = recorder.clone();
        let name = launched.name.clone();
        Arc::new(move |event: AcpEvent| {
            let Some(conversation_id) = conversation.get() else {
                return;
            };
            let _ = app.emit(
                crate::events::event_names::ACP_EVENT,
                AcpEventPayload {
                    conversation_id,
                    event: &event,
                },
            );
            let finished = recorder
                .lock()
                .ok()
                .and_then(|mut recorder| recorder.observe(&event, &name));
            if let Some(finished) = finished {
                record_turn(&app, conversation_id.clone(), finished);
            }
        })
    };
    let workspace_cancel = launched.workspace_cancel.clone();
    let started = tokio::select! {
        biased;
        _ = workspace_cancel.cancelled() => Err(RuntimeError::WorkspaceChanged),
        started = AcpSession::start(
            launched.read,
            launched.write,
            SessionSetup {
                cwd: documents.workspace_root().to_path_buf(),
                mcp_servers: launched.forwarding.servers.clone(),
            },
            sink,
        ) => started,
    };
    let started = match started {
        Ok(started) => started,
        Err(error) => {
            launched.process.shutdown().await;
            return Err(error);
        }
    };

    end_agent_session_unless(&app, None);
    crate::commands::chat::graceful_close_active_chat_session(state.inner()).await;
    let execution = ConversationExecution {
        kind: "acp".into(),
        agent_id: launched.integration.to_string(),
        agent_name: launched.name.clone(),
        external_session_id: started.session_id.clone(),
        workspace_id: launched.workspace.workspace_id.clone(),
        protocol_version: started.session.snapshot.protocol_version,
        capabilities: serde_json::to_value(&*started.session.snapshot).unwrap_or_default(),
    };
    let blocking_app = app.clone();
    let model_id = format!("acp:{}", launched.integration);
    let created = tokio::task::spawn_blocking(move || {
        let state = blocking_app.state::<AppState>();
        state.with_conversation_store(|store| {
            let delta = {
                let conversation = state.conversation.lock().unwrap();
                (conversation.len() > 0).then(|| {
                    let start = store
                        .changed_tail_start(&conversation.metadata.id, conversation.absolute_len());
                    ConversationDelta::from_history(&conversation, start)
                })
            };
            if let Some(delta) = delta {
                store.save_delta(delta)?;
            }
            store.create_conversation(model_id, Some(execution))
        })
    })
    .await
    .map_err(|_| RuntimeError::ProtocolFailed)
    .and_then(|created| created.map_err(|_| RuntimeError::ProtocolFailed));
    let metadata = match created {
        Ok(metadata) => metadata,
        Err(error) => {
            started.session.close();
            launched.process.shutdown().await;
            return Err(error);
        }
    };
    let conversation_id = metadata.id.clone();
    *state.conversation.lock().map_err(|_| RuntimeError::Busy)? =
        ConversationHistory::from_stored(StoredConversation {
            metadata,
            messages: Vec::new(),
        });
    let _ = conversation.set(conversation_id.clone());
    let forwarding = launched
        .forwarding
        .view_for(|server| acp_session::supports(&started.session.snapshot, server));
    let live = LiveAgent::new(
        conversation_id,
        launched.integration,
        launched.name,
        started.session,
        started.session_id,
        recorder,
        forwarding,
    );
    let status = live.status();
    state
        .integrations
        .agents
        .install(live, launched.process, workspace_cancel);
    Ok(status)
}

/// The live agent session of a conversation, if it has one.
#[tauri::command]
pub fn get_acp_conversation(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    conversation_id: String,
) -> Result<Option<AgentStatus>, RuntimeError> {
    local_only(&window)?;
    Ok(state
        .integrations
        .agents
        .bound(&conversation_id)
        .ok()
        .map(|live| live.status()))
}

fn active(state: &AppState, conversation_id: &str) -> Result<Arc<LiveAgent>, RuntimeError> {
    let is_active = state
        .conversation
        .lock()
        .map_err(|_| RuntimeError::Busy)?
        .metadata
        .id
        == conversation_id;
    if !is_active {
        return Err(RuntimeError::ConnectionNotReady);
    }
    state.integrations.agents.bound(conversation_id)
}

/// Sends the user's message to the agent and returns the turn number.
#[tauri::command]
pub async fn send_acp_prompt(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    conversation_id: String,
    text: String,
    images: Option<Vec<ChatImage>>,
) -> Result<u64, RuntimeError> {
    local_only(&window)?;
    let live = active(&state, &conversation_id)?;
    let images = images.unwrap_or_default();
    if text.trim().is_empty() || images.len() > MAX_IMAGES {
        return Err(RuntimeError::InvalidArguments);
    }
    let mut parts = vec![PromptPart::Text(text.clone())];
    parts.extend(images.iter().map(|image| PromptPart::Image {
        mime_type: image.mime_type.clone(),
        data: image.data.clone(),
    }));
    let mut message = ChatMessage::new(ChatRole::User, text);
    if !images.is_empty() {
        message.images = Some(images);
    }
    let message_id = message.id.clone();
    let record_app = app.clone();
    let record_id = conversation_id.clone();
    tokio::task::spawn_blocking(move || record(&record_app, &record_id, message))
        .await
        .map_err(|_| RuntimeError::Busy)?
        .map_err(|_| RuntimeError::Busy)?;
    match live.session.prompt(parts).await {
        Ok(turn) => Ok(turn),
        Err(error) => {
            // The agent never saw it; take it back out of the transcript.
            let mut conversation = state.conversation.lock().map_err(|_| RuntimeError::Busy)?;
            let last = conversation
                .messages_ref()
                .last()
                .map(|last| last.id.clone());
            if conversation.metadata.id == conversation_id && last == Some(message_id) {
                let len = conversation.len();
                conversation.truncate(len - 1);
                drop(conversation);
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let state = app.state::<AppState>();
                    let _ = state.with_conversation_store(|store| {
                        let delta = {
                            let conversation = state.conversation.lock().unwrap();
                            let start = store.changed_tail_start(
                                &conversation.metadata.id,
                                conversation.absolute_len(),
                            );
                            ConversationDelta::from_history(&conversation, start)
                        };
                        store.save_delta(delta)
                    });
                });
            }
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn cancel_acp_turn(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    conversation_id: String,
) -> Result<(), RuntimeError> {
    local_only(&window)?;
    let live = state.integrations.agents.bound(&conversation_id)?;
    live.session.cancel().await
}

/// The user's choice for one of the agent's permission requests: one of the
/// agent's own option IDs, or `None` to cancel the request.
#[tauri::command]
pub async fn respond_acp_permission(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    conversation_id: String,
    request_id: Uuid,
    option_id: Option<String>,
) -> Result<(), RuntimeError> {
    local_only(&window)?;
    if state.remote_control.is_configured().await && option_id.is_some() {
        return Err(RuntimeError::PermissionDenied);
    }
    let live = active(&state, &conversation_id)?;
    live.session.answer_permission(request_id, option_id).await
}

/// Changes one of the session settings the agent offered (model, effort,
/// mode…) and returns the agent's full new state.
#[tauri::command]
pub async fn set_acp_config(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    conversation_id: String,
    change: ConfigChange,
) -> Result<SessionConfig, RuntimeError> {
    local_only(&window)?;
    let live = active(&state, &conversation_id)?;
    live.session.configure(change).await
}

/// Runs one of the agent's own sign-in methods, then creates its session.
#[tauri::command]
pub async fn authenticate_acp_agent(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    conversation_id: String,
    method_id: String,
) -> Result<AgentStatus, RuntimeError> {
    local_only(&window)?;
    let live = active(&state, &conversation_id)?;
    let session_id = live.session.authenticate(method_id).await?;
    if let Ok(mut current) = live.session_id.lock() {
        *current = Some(session_id.clone());
    }
    if let Ok(mut conversation) = state.conversation.lock() {
        if conversation.metadata.id == conversation_id {
            if let Some(execution) = conversation.metadata.execution.as_mut() {
                execution.external_session_id = Some(session_id);
            }
        }
    }
    Ok(live.status())
}

#[tauri::command]
pub fn close_acp_conversation(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    conversation_id: String,
) -> Result<(), RuntimeError> {
    local_only(&window)?;
    if state.integrations.agents.bound(&conversation_id).is_ok() {
        end_agent_session_unless(&app, None);
    }
    Ok(())
}
