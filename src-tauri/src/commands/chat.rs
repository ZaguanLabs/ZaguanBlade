use crate::app_state::AppState;
use crate::conversation::ConversationHistory;
use crate::conversation_store;
use tauri::{AppHandle, Emitter, Manager, State};

const DISCONNECT_FLUSH_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

/// Tell the frontend the chat session id was minted or restored, so warmup
/// context prefetch can re-warm zcoderd under the new session key before the
/// first message of the (new/loaded/reset) conversation.
fn emit_session_id_changed(app: &AppHandle, session_id: &str) {
    let _ = app.emit(
        crate::events::event_names::SESSION_ID_CHANGED,
        session_id.to_string(),
    );
}
const SHUTDOWN_DISCONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(900);

pub(crate) async fn graceful_close_active_chat_session(state: &AppState) {
    let (close_target, was_active) = {
        let mut mgr = state.chat_manager.lock().unwrap();
        let close_target = mgr.stop_signal_target();
        let was_active = mgr.begin_stop();
        if was_active {
            mgr.abort_stream_task();
        }
        (close_target, was_active)
    };

    let Some((ws_client, request_id, session_id)) = close_target else {
        return;
    };

    if was_active && (request_id.is_some() || session_id.is_some()) {
        if let Err(error) = ws_client
            .send_cancel_request(request_id.clone(), session_id.clone())
            .await
        {
            if !crate::blade_ws_client::BladeWsClient::is_already_closed_error(&error) {
                eprintln!(
                    "[SHUTDOWN] Failed to send cancel_request before disconnect: {}",
                    error
                );
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    match tokio::time::timeout(
        SHUTDOWN_DISCONNECT_TIMEOUT,
        ws_client.close_with_session_disconnect(session_id, DISCONNECT_FLUSH_WINDOW),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("[SHUTDOWN] Failed to send disconnect: {}", error),
        Err(_) => {
            eprintln!("[SHUTDOWN] Timed out while sending disconnect; closing without waiting")
        }
    }
}

#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
) -> Result<Vec<crate::models::registry::ModelInfo>, String> {
    let config = { state.config.lock().unwrap().clone() };
    Ok(crate::models::catalog::list_all_models(&config).await)
}

#[tauri::command]
pub fn get_conversation(state: State<'_, AppState>) -> Vec<crate::protocol::ChatMessage> {
    let conversation = state.conversation.lock().unwrap();
    conversation.get_messages()
}

#[tauri::command]
pub fn truncate_conversation(
    len: usize,
    reset_session: Option<bool>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let mut conversation = state
        .conversation
        .lock()
        .map_err(|e| format!("Failed to lock conversation: {}", e))?;
    if len > conversation.len() {
        return Err(format!(
            "Cannot truncate conversation to {} messages; current length is {}",
            len,
            conversation.len()
        ));
    }
    conversation.truncate(len);
    if reset_session.unwrap_or(false) {
        // Reset means "start a fresh server session" — mint the id now so
        // warmup and the next chat message share one session key.
        let fresh = crate::chat_manager::fresh_session_id();
        conversation.metadata.session_id = Some(fresh.clone());
        drop(conversation);
        let mut mgr = state
            .chat_manager
            .lock()
            .map_err(|e| format!("Failed to lock chat manager: {}", e))?;
        mgr.session_id = Some(fresh.clone());
        mgr.planning_mode = None;
        mgr.runtime_mode = None;
        mgr.mode_source = None;
        drop(mgr);
        emit_session_id_changed(&app, &fresh);
    }
    Ok(())
}

#[tauri::command]
pub async fn list_conversations(
    _state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<conversation_store::ConversationMetadata>, String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.with_conversation_store(|store| Ok(store.list_conversations()))
    })
    .await
    .map_err(|e| format!("list conversations task failed: {}", e))?
}

#[tauri::command]
pub async fn load_conversation(
    id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    graceful_close_active_chat_session(state.inner()).await;

    let blocking_id = id.clone();
    let app_for_task = app.clone();
    let stored = tokio::task::spawn_blocking(move || {
        let state = app_for_task.state::<AppState>();
        state.with_conversation_store(|store| store.load_conversation(&blocking_id))
    })
    .await
    .map_err(|e| format!("load conversation task failed: {}", e))??;

    let mut conversation = state.conversation.lock().unwrap();
    *conversation = ConversationHistory::from_stored(stored.clone());

    // Restore session ID to ChatManager so it can resume the session.
    // Pre-Blade-minting conversations may lack one — mint on the spot so the
    // resumed conversation still shares a session key with warmup.
    let session_id = {
        let mut mgr = state.chat_manager.lock().unwrap();
        let session_id = match &stored.metadata.session_id {
            Some(session_id) => {
                eprintln!("[CHAT] Restored session ID: {}", session_id);
                session_id.clone()
            }
            None => {
                let fresh = crate::chat_manager::fresh_session_id();
                eprintln!("[CHAT] No session ID in loaded conversation; minted {}", fresh);
                fresh
            }
        };
        mgr.session_id = Some(session_id.clone());
        mgr.planning_mode = stored.metadata.planning_mode;
        mgr.runtime_mode = stored.metadata.runtime_mode.clone();
        mgr.mode_source = stored.metadata.mode_source.clone();
        session_id
    };
    emit_session_id_changed(&app, &session_id);

    Ok(())
}

#[tauri::command]
pub async fn new_conversation(
    model_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<String, String> {
    graceful_close_active_chat_session(state.inner()).await;

    // Save current conversation if it has messages
    let stored_to_save = {
        let conversation = state.conversation.lock().unwrap();
        if conversation.len() > 0 {
            Some(conversation.to_stored())
            // Note: session_id is auto-saved by background loop, but we should make sure
            // we don't lose the current session ID if we switch away.
            // However, conversation.to_stored() uses ConversationMetadata which we don't hold in ConversationHistory.
            // This logic relies on `store` having the correct metadata already or creating new.
            // The background loop in chat_orchestrator handles continuous saving with session_id.
        } else {
            None
        }
    };

    let blocking_model_id = model_id.clone();
    let app_for_task = app.clone();
    let metadata = tokio::task::spawn_blocking(move || {
        let state = app_for_task.state::<AppState>();
        if let Some(stored) = stored_to_save {
            state.with_conversation_store(|store| store.save_conversation(&stored))?;
        }
        state.with_conversation_store(|store| Ok(store.create_new_conversation(blocking_model_id)))
    })
    .await
    .map_err(|e| format!("new conversation task failed: {}", e))??;

    // Mint the session id for the new conversation up front — warmup sends it
    // to zcoderd before the first message, and the first chat message reuses
    // it, so the prewarmed context bundle lands on this conversation.
    let fresh = crate::chat_manager::fresh_session_id();
    {
        let mut mgr = state.chat_manager.lock().unwrap();
        mgr.session_id = Some(fresh.clone());
        mgr.planning_mode = None;
        mgr.runtime_mode = None;
        mgr.mode_source = None;
    }
    emit_session_id_changed(&app, &fresh);

    let id = metadata.id.clone();

    let mut conversation = state.conversation.lock().unwrap();
    *conversation = ConversationHistory::from_stored(conversation_store::StoredConversation {
        metadata,
        messages: vec![],
    });

    Ok(id)
}

pub fn stop_generation(state: State<'_, AppState>, app_handle: tauri::AppHandle) -> bool {
    let mut mgr = state.chat_manager.lock().unwrap();
    let stop_signal_target = mgr.stop_signal_target();
    let stopped = mgr.begin_stop();
    drop(mgr);

    if let Some((ws_client, request_id_hint, session_id_hint)) = stop_signal_target {
        let app_for_stop = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let cancel_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut request_id = request_id_hint;
            let mut session_id = session_id_hint;
            let mut sent_cancel = false;
            let mut last_error: Option<String> = None;

            while std::time::Instant::now() < cancel_deadline {
                if request_id.is_none() {
                    request_id = ws_client.get_active_request_id().await;
                }
                if session_id.is_none() {
                    session_id = ws_client.get_session_id().await;
                }

                if request_id.is_some() || session_id.is_some() {
                    match ws_client
                        .send_cancel_request(request_id.clone(), session_id.clone())
                        .await
                    {
                        Ok(()) => {
                            sent_cancel = true;
                            break;
                        }
                        Err(error) => {
                            last_error = Some(error);
                        }
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }

            if sent_cancel {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            } else {
                let error = last_error.unwrap_or_else(|| {
                    "no request or session ID available for remote cancel".to_string()
                });
                if !crate::blade_ws_client::BladeWsClient::is_already_closed_error(&error) {
                    eprintln!("[STOP] Failed to send cancel_request to zcoderd: {}", error);
                }
            }

            if let Err(error) = ws_client
                .close_with_session_disconnect(session_id.clone(), DISCONNECT_FLUSH_WINDOW)
                .await
            {
                eprintln!(
                    "[STOP] Failed to send disconnect after cancel_request: {}",
                    error
                );
            }

            let state = app_for_stop.state::<AppState>();
            let mut mgr = state.chat_manager.lock().unwrap();
            mgr.abort_stream_task();
        });
    } else {
        let mut mgr = state.chat_manager.lock().unwrap();
        mgr.abort_stream_task();
    }

    // Clear any pending command batch when stopping
    let mut batch_guard = state.pending_batch.lock().unwrap();
    *batch_guard = None;

    // Signal the pending_approval oneshot so the orchestrator unblocks
    // from command approval waits (e.g. waiting for user to approve a command)
    {
        let mut approval_guard = state.pending_approval.lock().unwrap();
        if let Some(tx) = approval_guard.take() {
            eprintln!("[STOP] Signalling pending_approval oneshot to unblock orchestrator");
            let _ = tx.send(false); // false = not approved, just unblocking
        }
    }

    // Cancel all executing commands and emit events immediately
    let mut executing = state.executing_commands.lock().unwrap();
    for (call_id, cancel_flag) in executing.drain() {
        cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        eprintln!("[STOP] Cancelled executing command: {}", call_id);

        // Emit tool-execution-completed event immediately so UI updates
        let _ = app_handle.emit(
            "tool-execution-completed",
            crate::events::ToolExecutionCompletedPayload {
                tool_name: "run_command".to_string(),
                tool_call_id: call_id.clone(),
                success: false,
                skipped: true, // Cancelled commands are treated as skipped
            },
        );
    }

    // Emit chat-done so the frontend resets loading state immediately
    // The orchestrator loop will also see streaming=false + rx=None and break
    let _ = app_handle.emit(
        crate::events::event_names::CHAT_DONE,
        crate::events::ChatDonePayload {
            finish_reason: "stop".to_string(),
        },
    );

    stopped
}

#[tauri::command]
pub async fn set_selected_model(
    model_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Update the selected model index (in-memory only)
    let config = { state.config.lock().unwrap().clone() };
    let models = crate::models::catalog::list_all_models(&config).await;
    let matched_idx = crate::models::catalog::resolve_model_index(&models, &model_id);

    if let Some(idx) = matched_idx {
        *state.selected_model_index.lock().unwrap() = idx;
        eprintln!(
            "[MODEL] Set selected model index to {} for {} (Registry ID: {})",
            idx, model_id, models[idx].id
        );
        Ok(())
    } else {
        Err(format!("Model not found: {}", model_id))
    }
}

/// Returns whether the backend is currently streaming a response.
/// Used by the frontend to restore `loading` state after a UI reload.
#[tauri::command]
pub fn get_chat_status(state: State<'_, AppState>) -> bool {
    let mgr = state.chat_manager.lock().unwrap();
    mgr.streaming || mgr.rx.is_some()
}

#[tauri::command]
pub async fn is_local_model_active(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.is_local_model_active().await)
}
