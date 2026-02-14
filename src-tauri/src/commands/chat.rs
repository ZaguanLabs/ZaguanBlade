use crate::app_state::AppState;
use crate::chat_orchestrator::handle_send_message;
use crate::conversation::ConversationHistory;
use crate::conversation_store;
use tauri::{AppHandle, Emitter, Runtime, State, Window};

#[tauri::command]
pub async fn send_message<R: Runtime>(
    message: String,
    images: Option<Vec<crate::protocol::ChatImage>>,
    model_id: Option<String>,
    active_file: Option<String>,
    open_files: Option<Vec<String>>,
    cursor_line: Option<usize>,
    cursor_column: Option<usize>,
    selection_start_line: Option<usize>,
    selection_end_line: Option<usize>,
    window: Window<R>,
    state: State<'_, AppState>,
    app: AppHandle<R>,
) -> Result<(), String> {
    handle_send_message(
        message,
        images,
        model_id,
        active_file,
        open_files,
        cursor_line,
        cursor_column,
        selection_start_line,
        selection_end_line,
        window,
        state,
        app,
    )
    .await
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
pub fn list_conversations(
    state: State<'_, AppState>,
) -> Result<Vec<conversation_store::ConversationMetadata>, String> {
    let store = state.conversation_store.lock().unwrap();
    Ok(store.list_conversations())
}

#[tauri::command]
pub fn load_conversation(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let store = state.conversation_store.lock().unwrap();
    let stored = store.load_conversation(&id)?;

    let mut conversation = state.conversation.lock().unwrap();
    *conversation = ConversationHistory::from_stored(stored.clone());

    // Restore session ID to ChatManager so it can resume the session
    {
        let mut mgr = state.chat_manager.lock().unwrap();
        if let Some(session_id) = &stored.metadata.session_id {
            mgr.session_id = Some(session_id.clone());
            eprintln!("[CHAT] Restored session ID: {}", session_id);
        } else {
            mgr.session_id = None;
            eprintln!("[CHAT] No session ID in loaded conversation");
        }
    }

    Ok(())
}

#[tauri::command]
pub fn new_conversation(model_id: String, state: State<'_, AppState>) -> Result<String, String> {
    // Save current conversation if it has messages
    {
        let conversation = state.conversation.lock().unwrap();
        if conversation.len() > 0 {
            let mut store = state.conversation_store.lock().unwrap();
            let stored = conversation.to_stored();
            // Note: session_id is auto-saved by background loop, but we should make sure
            // we don't lose the current session ID if we switch away.
            // However, conversation.to_stored() uses ConversationMetadata which we don't hold in ConversationHistory.
            // This logic relies on `store` having the correct metadata already or creating new.
            // The background loop in chat_orchestrator handles continuous saving with session_id.

            store.save_conversation(&stored)?;
        }
    }

    // Clear session ID in ChatManager for the new conversation
    {
        let mut mgr = state.chat_manager.lock().unwrap();
        mgr.session_id = None;
    }

    // Create new conversation
    let mut store = state.conversation_store.lock().unwrap();
    let metadata = store.create_new_conversation(model_id);
    let id = metadata.id.clone();

    let mut conversation = state.conversation.lock().unwrap();
    *conversation = ConversationHistory::from_stored(conversation_store::StoredConversation {
        metadata,
        messages: vec![],
    });

    Ok(id)
}

#[tauri::command]
pub fn delete_conversation(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut store = state.conversation_store.lock().unwrap();
    store.delete_conversation(&id)
}

#[tauri::command]
pub fn save_conversation(state: State<'_, AppState>) -> Result<(), String> {
    let conversation = state.conversation.lock().unwrap();
    let mut store = state.conversation_store.lock().unwrap();
    let stored = conversation.to_stored();
    store.save_conversation(&stored)
}

#[tauri::command]
pub fn stop_generation(state: State<'_, AppState>, app_handle: tauri::AppHandle) -> bool {
    let mut mgr = state.chat_manager.lock().unwrap();
    let stopped = mgr.request_stop();

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
    let _ = app_handle.emit("chat-done", ());

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

#[tauri::command]
pub fn get_selected_model(_state: State<'_, AppState>) -> Option<String> {
    // Model is now stored in project state only, not main config
    // Return None to let the frontend use project state or default
    None
}
