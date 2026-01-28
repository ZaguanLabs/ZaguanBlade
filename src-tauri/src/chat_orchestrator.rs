use crate::app_state::AppState;
use crate::chat_manager::DrainResult;
use crate::models::registry::get_models;
use crate::project_settings;
use crate::utils::{extract_root_command, is_cwd_outside_workspace, parse_command};
use crate::{blade_protocol, local_artifacts};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub async fn handle_send_message<R: Runtime>(
    message: String,
    model_id: Option<String>,
    active_file: Option<String>,
    open_files: Option<Vec<String>>,
    cursor_line: Option<usize>,
    cursor_column: Option<usize>,
    selection_start_line: Option<usize>,
    selection_end_line: Option<usize>,
    window: tauri::Window<R>,
    state: State<'_, AppState>,
    app: AppHandle<R>,
) -> Result<(), String> {
    println!("Received message from frontend: {}", message);
    eprintln!(
        "[SEND MSG] active_file={:?}, cursor_line={:?}, cursor_column={:?}",
        active_file, cursor_line, cursor_column
    );

    // Store editor state in AppState for tool execution
    {
        *state.active_file.lock().unwrap() = active_file.clone();
        *state.open_files.lock().unwrap() = open_files.clone().unwrap_or_default();
        *state.cursor_line.lock().unwrap() = cursor_line;
        *state.cursor_column.lock().unwrap() = cursor_column;
        *state.selection_start_line.lock().unwrap() = selection_start_line;
        *state.selection_end_line.lock().unwrap() = selection_end_line;
    }

    // Parse @commands and convert to tool calls
    let (actual_message, forced_tool) = parse_command(&message);

    // 1. Add User Message
    {
        let mut conversation = state.conversation.lock().unwrap();
        conversation.push(crate::protocol::ChatMessage::new(
            crate::protocol::ChatRole::User,
            actual_message.clone(),
        ));
    }

    // Commands like @research, @search, @web are now handled directly by zcoderd
    // No need to modify the message - just send it as-is
    if let Some((tool_name, query)) = forced_tool {
        eprintln!(
            "[COMMAND] Detected command: {} with query: {}",
            tool_name, query
        );
        eprintln!("[COMMAND] zcoderd will handle this directly");
    }

    // 2. Start Stream
    let (blade_url, api_key) = {
        let config = state.config.lock().unwrap();
        (config.blade_url.clone(), config.api_key.clone())
    };
    let models = get_models(&blade_url, &api_key).await;
    {
        let mut mgr = state.chat_manager.lock().unwrap();
        let mut conversation = state.conversation.lock().unwrap();
        let config = state.config.lock().unwrap();
        let workspace = state.workspace.lock().unwrap();

        // Default to the currently selected model index from state, rather than 0
        let mut selected_model = *state.selected_model_index.lock().unwrap();

        if let Some(ref id) = model_id {
            // Smart matching logic:
            // 1. Try exact match on unique ID (composite or raw)
            // 2. Try exact match on API ID (raw)
            // 3. Try case-insensitive matches
            let matched_idx = models
                .iter()
                .position(|m| m.id == *id)
                .or_else(|| models.iter().position(|m| m.api_id.as_deref() == Some(id)))
                .or_else(|| {
                    let id_lower = id.to_lowercase();
                    models
                        .iter()
                        .position(|m| m.id.to_lowercase() == id_lower)
                        .or_else(|| {
                            models.iter().position(|m| {
                                m.api_id.as_ref().map(|s| s.to_lowercase()).as_deref()
                                    == Some(&id_lower)
                            })
                        })
                });

            if let Some(idx) = matched_idx {
                eprintln!(
                    "[MODEL DEBUG] Resolved '{}' to index {} ({})",
                    id, idx, models[idx].id
                );
                selected_model = idx;
            } else {
                eprintln!(
                    "[MODEL WARNING] Requested model '{}' not found in registry ({} available). Fallback to state index {}.",
                    id, models.len(), selected_model
                );
            }
        }

        // Ensure index is valid (models list might have changed)
        if !models.is_empty() && selected_model >= models.len() {
            eprintln!(
                "[MODEL WARNING] Selected index {} out of bounds, resetting to 0",
                selected_model
            );
            selected_model = 0;
        }

        // Store active model index for use in continue_tool_batch
        *state.selected_model_index.lock().unwrap() = selected_model;

        // We use reqwest Client
        let http = reqwest::Client::new();

        // Ensure workspace root is valid
        let ws = workspace.workspace.as_ref();

        // RFC-002: Get storage mode from project settings, default to "local"
        let storage_mode = Some(
            ws.map(|p| {
                let settings = project_settings::load_project_settings_or_default(p);
                match settings.storage.mode {
                    project_settings::StorageMode::Local => "local".to_string(),
                    project_settings::StorageMode::Server => "server".to_string(),
                }
            })
            .unwrap_or_else(|| "local".to_string()),
        );

        mgr.start_stream(
            message,
            &mut conversation,
            &config,
            &models,
            selected_model,
            ws,
            active_file.clone(),
            open_files.clone(),
            cursor_line,
            cursor_column,
            http,
            storage_mode,
        )
        .map_err(|e| e.to_string())?;
    }

    // 3. Event-Driven Processing (Background Task)
    // Only processes events when there's actual streaming activity
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let mut last_session_id: Option<String> = None;
        
        // Fetch models once at the start instead of every iteration
        let state = app_handle.state::<AppState>();
        let (blade_url, api_key) = {
            let config = state.config.lock().unwrap();
            (config.blade_url.clone(), config.api_key.clone())
        };
        let models = get_models(&blade_url, &api_key).await;

        loop {
            // Check if we're actually streaming before processing
            let (is_streaming, has_rx, has_pending) = {
                let state = app_handle.state::<AppState>();
                let mgr = state.chat_manager.lock().unwrap();
                (mgr.streaming, mgr.rx.is_some(), !mgr.pending_results.is_empty())
            };

            // If not streaming and no receiver AND no pending results, sleep longer to reduce CPU usage
            // IMPORTANT: We must check pending_results because drain_events may have queued results
            // (e.g., ToolCalls) that need to be processed even after rx is cleared
            if !is_streaming && !has_rx && !has_pending {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }

            // If we have a receiver but not actively streaming (e.g., waiting for tool results),
            // check less frequently to avoid CPU spike
            if !is_streaming && has_rx && !has_pending {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await; // 20 FPS when waiting
            } else if is_streaming && !has_pending {
                tokio::time::sleep(std::time::Duration::from_millis(16)).await; // ~60 FPS when active
            }
            // If has_pending, process immediately without sleeping

            let state = app_handle.state::<AppState>();

            let (result, is_streaming, session_id) = {
                let mut mgr = state.chat_manager.lock().unwrap();
                let mut conversation = state.conversation.lock().unwrap();
                let selected_model_idx = *state.selected_model_index.lock().unwrap();

                let res = mgr.drain_events(&mut conversation, &models, selected_model_idx);
                (res, mgr.streaming, mgr.session_id.clone())
            };

            // If the backend session_id changes, reset loop detection history.
            // Otherwise, old tool-call history can cause false-positive loop detection
            // (e.g., read_file blocked immediately in a fresh session).
            if session_id != last_session_id {
                if let Some(ref sid) = session_id {
                    eprintln!(
                        "[AI WORKFLOW] Session changed: clearing tool history (session_id={})",
                        sid
                    );
                }
                {
                    let mut workflow = state.workflow.lock().unwrap();
                    workflow.clear_history();
                }
                {
                    let mut cache = state.approved_command_roots.lock().unwrap();
                    cache.clear();
                }
                last_session_id = session_id.clone();
            }

            if let DrainResult::None = result {
                // Only consider conversation done if:
                // 1. Not streaming (no active content being received)
                // 2. No active receiver channel (no WebSocket connection waiting for events)
                // This fixes the bug where the loop would break after continue_tool_batch
                // because streaming=false but rx=Some(channel) - we're still waiting for events!
                let has_rx = {
                    let mgr = state.chat_manager.lock().unwrap();
                    mgr.rx.is_some()
                };
                
                if !is_streaming && !has_rx {
                    // Auto-save conversation before emitting done
                    {
                        let conversation = state.conversation.lock().unwrap();
                        let mut store = state.conversation_store.lock().unwrap();
                        let mut stored = conversation.to_stored();
                        // Persist the current session ID to the stored metadata
                        stored.metadata.session_id = session_id.clone();
                        if let Err(e) = store.save_conversation(&stored) {
                            eprintln!("Failed to auto-save conversation: {}", e);
                        } else {
                            println!("Auto-saved conversation: {}", stored.metadata.id);
                        }

                        // RFC-002: Also save to local artifacts if in local storage mode
                        let workspace = state.workspace.lock().unwrap();
                        if let Some(ref ws_path) = workspace.workspace {
                            let settings = project_settings::load_project_settings_or_default(ws_path);
                            if settings.storage.mode == project_settings::StorageMode::Local {
                                // Convert to local artifact format
                                let project_id = crate::project::get_or_create_project_id(ws_path)
                                    .unwrap_or_else(|_| "unknown".to_string());

                                let title = if stored.metadata.title.is_empty() {
                                    "Untitled".to_string()
                                } else {
                                    stored.metadata.title.clone()
                                };
                                let mut artifact = local_artifacts::ConversationArtifact::new(
                                    stored.metadata.id.clone(),
                                    project_id,
                                    title,
                                );

                                // Convert messages
                                for (idx, msg) in stored.messages.iter().enumerate() {
                                    let local_msg = local_artifacts::Message {
                                        id: format!("msg_{}", idx),
                                        role: msg.role.clone(),
                                        content: msg.content.clone(),
                                        timestamp: chrono::Utc::now().to_rfc3339(),
                                        code_references: vec![], // TODO: Extract from content
                                    };
                                    artifact.messages.push(local_msg);
                                }
                                artifact.metadata.total_messages = artifact.messages.len() as i32;

                                let artifact_store =
                                    local_artifacts::LocalArtifactStore::new(ws_path);
                                if let Err(e) = artifact_store.save_conversation(&artifact) {
                                    eprintln!("[LOCAL] Failed to save local artifact: {}", e);
                                } else {
                                    eprintln!("[LOCAL] Saved conversation to .zblade/artifacts/conversations/{}.json", stored.metadata.id);
                                }
                            }
                        }
                    }

                    // v1.1: Emit MessageCompleted event for explicit end-of-stream
                    let msg_id = {
                        let conversation = state.conversation.lock().unwrap();
                        conversation.last().and_then(|msg| {
                            if msg.role == crate::protocol::ChatRole::Assistant {
                                // Use existing ID if available (v1.1 compliant), else fallback to new
                                msg.id
                                    .clone()
                                    .or_else(|| Some(uuid::Uuid::new_v4().to_string()))
                            } else {
                                None
                            }
                        })
                    };

                    if let Some(id) = msg_id {
                        let _ = window.emit(
                            "blade-event",
                            blade_protocol::BladeEventEnvelope {
                                id: uuid::Uuid::new_v4(),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64,
                                causality_id: None,
                                event: blade_protocol::BladeEvent::Chat(
                                    blade_protocol::ChatEvent::MessageCompleted { id },
                                ),
                            },
                        );
                    }

                    window.emit("chat-done", ()).unwrap_or_default();
                    break;
                }
            } else if let DrainResult::Research {
                content,
                suggested_name,
            } = result
            {
                println!("[RESEARCH] Creating ephemeral document: {}", suggested_name);
                // Create ephemeral document
                let state = app_handle.state::<AppState>();
                let document_id = state
                    .ephemeral_docs
                    .create(content.clone(), suggested_name.clone());

                // Emit event to open the document tab
                #[derive(Clone, serde::Serialize)]
                struct EphemeralDocPayload {
                    id: String,
                    title: String,
                    content: String,
                    suggested_name: String,
                }

                window
                    .emit(
                        "open-ephemeral-document",
                        EphemeralDocPayload {
                            id: document_id,
                            title: "Research Results".to_string(),
                            content,
                            suggested_name,
                        },
                    )
                    .unwrap_or_default();

                // Continue polling for done event
            } else if let DrainResult::Update(msg, chunk) = result {
                // Emit streaming chunk immediately for real-time updates
                // v1.1: Use MessageDelta with sequence number
                let mut mgr = state.chat_manager.lock().unwrap();
                let seq = mgr.message_seq;
                mgr.message_seq += 1;
                // Get the message ID if possible, otherwise use a temporary one (chat manager should track this properly in future)
                // For now, we assume the frontend can correlate by expecting a stream
                let msg_id = msg
                    .id
                    .clone()
                    .unwrap_or_else(|| "streaming-msg".to_string());
                drop(mgr);

                // 2. Emit Blade v1.1 MessageDelta
                let _ = window.emit(
                    "blade-event",
                    blade_protocol::BladeEventEnvelope {
                        id: uuid::Uuid::new_v4(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                        causality_id: Some(msg_id.clone()),
                        event: blade_protocol::BladeEvent::Chat(
                            blade_protocol::ChatEvent::MessageDelta {
                                id: msg_id,
                                seq,
                                chunk,
                                is_final: false, // Will be set in MessageCompleted
                            },
                        ),
                    },
                );
            } else if let DrainResult::Reasoning(msg, chunk) = result {
                let mut mgr = state.chat_manager.lock().unwrap();
                let seq = mgr.message_seq;
                mgr.message_seq += 1;
                let msg_id = msg
                    .id
                    .clone()
                    .unwrap_or_else(|| "streaming-msg".to_string());
                drop(mgr);

                let _ = window.emit(
                    "blade-event",
                    blade_protocol::BladeEventEnvelope {
                        id: uuid::Uuid::new_v4(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                        causality_id: Some(msg_id.clone()),
                        event: blade_protocol::BladeEvent::Chat(
                            blade_protocol::ChatEvent::ReasoningDelta {
                                id: msg_id,
                                seq,
                                chunk,
                                is_final: false,
                            },
                        ),
                    },
                );
            } else if let DrainResult::Error(e) = result {
                window.emit("chat-error", e).unwrap_or_default();
                break;
            } else if let DrainResult::ToolCreated(msg, new_calls) = result {
                let msg_id = msg.id.clone().unwrap_or_else(|| "unknown".to_string());
                for tc in new_calls {
                    let _ = window.emit(
                        "blade-event",
                        blade_protocol::BladeEventEnvelope {
                            id: uuid::Uuid::new_v4(),
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64,
                            causality_id: Some(msg_id.clone()),
                            event: blade_protocol::BladeEvent::Chat(
                                blade_protocol::ChatEvent::ToolUpdate {
                                    message_id: msg_id.clone(),
                                    tool_call_id: tc.id.clone(),
                                    status: "executing".to_string(),
                                    result: None,
                                    tool_call: Some(tc.clone()),
                                },
                            ),
                        },
                    );
                }
            } else if let DrainResult::ToolActivity {
                tool_name,
                file_path,
                action,
            } = result
            {
                let _ = window.emit(
                    "blade-event",
                    blade_protocol::BladeEventEnvelope {
                        id: uuid::Uuid::new_v4(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                        causality_id: None,
                        event: blade_protocol::BladeEvent::Chat(
                            blade_protocol::ChatEvent::ToolActivity {
                                tool_name,
                                file_path,
                                action,
                            },
                        ),
                    },
                );
            } else if let DrainResult::ToolStatusUpdate(msg) = result {
                // v1.1: Emit ToolUpdate events via blade-event
                if let Some(tool_calls) = &msg.tool_calls {
                    let msg_id = msg.id.clone().unwrap_or_else(|| "unknown".to_string());
                    for tc in tool_calls {
                        // Emit update for each tool call
                        // We emit indiscriminately here because the frontend will merge/update state based on ID
                        let status = tc.status.clone().unwrap_or_else(|| "unknown".to_string());

                        let _ = window.emit(
                            "blade-event",
                            blade_protocol::BladeEventEnvelope {
                                id: uuid::Uuid::new_v4(),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64,
                                causality_id: Some(msg_id.clone()),
                                event: blade_protocol::BladeEvent::Chat(
                                    blade_protocol::ChatEvent::ToolUpdate {
                                        message_id: msg_id.clone(),
                                        tool_call_id: tc.id.clone(),
                                        status,
                                        result: tc.result.clone(),
                                        tool_call: Some(tc.clone()),
                                    },
                                ),
                            },
                        );
                    }
                }
            } else if let DrainResult::Progress {
                message,
                stage,
                percent,
            } = result
            {
                // Emit progress event to frontend for @research command
                #[derive(Clone, serde::Serialize)]
                struct ProgressPayload {
                    message: String,
                    stage: String,
                    percent: i32,
                }
                window
                    .emit(
                        "research-progress",
                        ProgressPayload {
                            message,
                            stage,
                            percent,
                        },
                    )
                    .unwrap_or_default();
            } else if let DrainResult::TodoUpdated(todos) = result {
                // Emit todo_updated event to frontend
                let event_todos: Vec<crate::events::TodoItem> = todos
                    .into_iter()
                    .map(|t| crate::events::TodoItem {
                        content: t.content.clone(),
                        active_form: t.active_form.unwrap_or_else(|| t.content.clone()),
                        status: t.status,
                    })
                    .collect();
                eprintln!(
                    "[LIB] Emitting TODO_UPDATED event with {} items",
                    event_todos.len()
                );
                match window.emit(
                    crate::events::event_names::TODO_UPDATED,
                    crate::events::TodoUpdatedPayload { todos: event_todos },
                ) {
                    Ok(_) => eprintln!("[LIB] TODO_UPDATED event emitted successfully"),
                    Err(e) => eprintln!("[LIB] Failed to emit TODO_UPDATED: {}", e),
                }
            } else if let DrainResult::MessageCompleted(id) = result {
                // Emit MessageCompleted immediately to reset loading state
                eprintln!("[ORCHESTRATOR] Emitting MessageCompleted: {}", id);
                let _ = window.emit(
                    "blade-event",
                    blade_protocol::BladeEventEnvelope {
                        id: uuid::Uuid::new_v4(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                        causality_id: None,
                        event: blade_protocol::BladeEvent::Chat(
                            blade_protocol::ChatEvent::MessageCompleted { id },
                        ),
                    },
                );
            } else if let DrainResult::ContextLengthExceeded { message, token_count, max_tokens, excess, recoverable, recovery_hint } = result {
                // RFC: Context Length Recovery - emit context-length-exceeded event to frontend
                eprintln!("[LIB] Context length exceeded: {} (tokens: {:?}/{:?})", message, token_count, max_tokens);
                let _ = window.emit(
                    "context-length-exceeded",
                    serde_json::json!({
                        "message": message,
                        "token_count": token_count,
                        "max_tokens": max_tokens,
                        "excess": excess,
                        "recoverable": recoverable,
                        "recovery_hint": recovery_hint,
                    }),
                );
            } else if let DrainResult::ToolCalls(calls, content) = result {
                println!("Tools requested: {:?}. Executing...", calls.len());
                let state = app_handle.state::<AppState>();
                let ws_root = {
                    let ws = state.workspace.lock().unwrap();
                    ws.workspace
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                };

                // Get editor state from AppState
                let active_file = state.active_file.lock().unwrap().clone();
                let open_files = state.open_files.lock().unwrap().clone();
                let cursor_line = *state.cursor_line.lock().unwrap();
                let cursor_column = *state.cursor_column.lock().unwrap();
                let selection_start_line = *state.selection_start_line.lock().unwrap();
                let selection_end_line = *state.selection_end_line.lock().unwrap();

                let context = crate::tool_execution::ToolExecutionContext::new(
                    ws_root.clone(),
                    active_file,
                    open_files,
                    0,
                    cursor_line,
                    cursor_column,
                    selection_start_line,
                    selection_end_line,
                    Some(app_handle.clone()),
                );

                let batch_opt = {
                    let mut workflow = state.workflow.lock().unwrap();
                    workflow.handle_tool_calls(
                        ws_root
                            .as_ref()
                            .map(|s| std::path::Path::new(s))
                            .unwrap_or_else(|| std::path::Path::new(".")),
                        calls,
                        content,
                        &context,
                    )
                };

                // Auto-execute commands (CHECK PENDING)
                let pending_opt = {
                    let mut workflow = state.workflow.lock().unwrap();
                    let has_cmds = workflow.has_pending_commands();
                    let has_changes = workflow.has_pending_changes();
                    let has_confirms = workflow.has_pending_confirms();

                    if has_cmds || has_changes || has_confirms {
                        workflow.take_pending()
                    } else {
                        None
                    }
                };

                let mut batch_to_run = None;
                let pending = pending_opt.or(batch_opt);

                if let Some(batch) = pending {
                    // Check if there are actions requiring approval (commands, confirms)
                    // Note: File edits (changes) are now applied immediately and not buffered here.
                    let has_pending_actions =
                        !batch.commands.is_empty() || !batch.confirms.is_empty();

                    if !has_pending_actions {
                        // No approval needed - set batch to run and let it fall through
                        batch_to_run = Some(batch);
                    } else {
                        // If we reach here, there ARE pending items that need approval
                        // MUST go through the approval flow

                        // UNIFIED BLOCKING SYSTEM
                        // 1. Store the full batch in AppState so approval commands can update it
                        {
                            let mut batch_guard = state.pending_batch.lock().unwrap();
                            *batch_guard = Some(batch.clone());
                        }

                        // 2. Emit UI events

                        // Handle Changes (file edits, new files, deletions)
                        if !batch.changes.is_empty() {
                            #[derive(serde::Serialize, Clone)]
                            #[serde(tag = "change_type")]
                            enum ChangeProposal {
                                #[serde(rename = "patch")]
                                Patch {
                                    id: String,
                                    path: String,
                                    old_content: String,
                                    new_content: String,
                                },
                                #[serde(rename = "multi_patch")]
                                MultiPatch {
                                    id: String,
                                    path: String,
                                    patches: Vec<crate::ai_workflow::PatchHunk>,
                                },
                                #[serde(rename = "new_file")]
                                NewFile {
                                    id: String,
                                    path: String,
                                    content: String,
                                },
                                #[serde(rename = "delete_file")]
                                DeleteFile { id: String, path: String },
                            }

                            let proposals: Vec<ChangeProposal> = batch
                                .changes
                                .iter()
                                .map(|change| match &change.change_type {
                                    crate::ai_workflow::ChangeType::Patch {
                                        old_content,
                                        new_content,
                                    } => ChangeProposal::Patch {
                                        id: change.call.id.clone(),
                                        path: change.path.clone(),
                                        old_content: old_content.clone(),
                                        new_content: new_content.clone(),
                                    },
                                    crate::ai_workflow::ChangeType::MultiPatch { patches } => {
                                        ChangeProposal::MultiPatch {
                                            id: change.call.id.clone(),
                                            path: change.path.clone(),
                                            patches: patches.clone(),
                                        }
                                    }
                                    crate::ai_workflow::ChangeType::NewFile { content } => {
                                        ChangeProposal::NewFile {
                                            id: change.call.id.clone(),
                                            path: change.path.clone(),
                                            content: content.clone(),
                                        }
                                    }
                                    crate::ai_workflow::ChangeType::DeleteFile { .. } => {
                                        ChangeProposal::DeleteFile {
                                            id: change.call.id.clone(),
                                            path: change.path.clone(),
                                        }
                                    }
                                })
                                .collect();

                            window
                                .emit("propose-changes", proposals)
                                .unwrap_or_default();
                        }

                        // Handle Commands and Confirms
                        if !batch.commands.is_empty() || !batch.confirms.is_empty() {
                            let mut actions = Vec::new();
                            for cmd in &batch.commands {
                                if batch.file_results.iter().any(|(c, _)| c.id == cmd.call.id) {
                                    continue;
                                }
                                let root_command = extract_root_command(&cmd.command);
                                let cwd_outside_workspace = is_cwd_outside_workspace(
                                    ws_root.as_deref(),
                                    cmd.cwd.as_deref(),
                                );
                                actions.push(crate::events::StructuredAction {
                                    id: cmd.call.id.clone(),
                                    command: cmd.command.clone(),
                                    description: format!("Run command: {}", cmd.command),
                                    cwd: cmd.cwd.clone(),
                                    root_command,
                                    cwd_outside_workspace,
                                    is_generic_tool: false,
                                });
                            }
                            for conf in &batch.confirms {
                                if batch.file_results.iter().any(|(c, _)| c.id == conf.call.id) {
                                    continue;
                                }
                                actions.push(crate::events::StructuredAction {
                                    id: conf.call.id.clone(),
                                    command: conf.tool_name.clone(),
                                    description: conf.description.clone(),
                                    cwd: None,
                                    root_command: None,
                                    cwd_outside_workspace: None,
                                    is_generic_tool: true,
                                });
                            }
                            if !actions.is_empty() {
                                eprintln!("[ORCHESTRATOR] Emitting request-confirmation with {} actions", actions.len());
                                for action in &actions {
                                    eprintln!("[ORCHESTRATOR]   - action: {} (id={})", action.command, action.id);
                                }
                                window
                                    .emit(
                                        crate::events::event_names::REQUEST_CONFIRMATION,
                                        crate::events::RequestConfirmationPayload { actions },
                                    )
                                    .unwrap_or_default();
                            }
                        }

                        // 3. Block until user addresses ALL items in this batch
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        {
                            let mut guard = state.pending_approval.lock().unwrap();
                            *guard = Some(tx);
                        }

                        // Wait for the signal (sent by approve_change, approve_tool, or approve_all_changes)
                        let _ = rx.await.unwrap_or(false);

                        // 4. Retrieve the updated batch with all results
                        let updated_batch = {
                            let mut batch_guard = state.pending_batch.lock().unwrap();
                            batch_guard.take()
                        };

                        if let Some(final_batch) = updated_batch {
                            batch_to_run = Some(final_batch);
                        } else {
                            // This shouldn't happen unless something went wrong with the state
                            batch_to_run = None;
                        }
                    }
                }

                if let Some(batch) = batch_to_run {
                    // Auto-open files in editor when file-modifying tools are called
                    // (read_file operations don't auto-open to avoid disruptive UX)
                    for (call, result) in &batch.file_results {
                        // Only auto-open files when the operation was successful
                        if !result.success {
                            continue;
                        }

                        let is_file_modifying_tool = matches!(
                            call.function.name.as_str(),
                            "write_file"
                                | "create_file"
                                | "replace_file_content"
                                | "apply_edit"
                                | "apply_patch"
                                | "edit_file"
                                | "multi_replace_file_content"
                        );

                        if is_file_modifying_tool {
                            // Extract path from tool arguments
                            if let Ok(args) = serde_json::from_str::<
                                std::collections::HashMap<String, serde_json::Value>,
                            >(&call.function.arguments)
                            {
                                if let Some(path_value) = args
                                    .get("path")
                                    .or_else(|| args.get("file_path"))
                                    .or_else(|| args.get("filepath"))
                                    .or_else(|| args.get("filename"))
                                    .or_else(|| args.get("TargetFile")) // For replace_file_content
                                    .or_else(|| args.get("target_file"))
                                {
                                    if let Some(path) = path_value.as_str() {
                                        // Convert to absolute path if relative
                                        let abs_path = if std::path::Path::new(path).is_absolute() {
                                            path.to_string()
                                        } else {
                                            let ws = state.workspace.lock().unwrap();
                                            if let Some(workspace) = &ws.workspace {
                                                workspace.join(path).to_string_lossy().to_string()
                                            } else {
                                                path.to_string()
                                            }
                                        };
                                        eprintln!("[AUTO OPEN] Opening file in editor: {}", abs_path);
                                        window.emit("open-file", &abs_path).unwrap_or_default();
                                    }
                                }
                            }
                        }
                    }

                    // Check if loop was detected - if so, stop the agentic loop
                    if batch.loop_detected {
                        eprintln!("[AGENTIC LOOP] Stopping due to loop detection");

                        // Fetch models before acquiring locks
                        let (blade_url, api_key) = {
                            let config = state.config.lock().unwrap();
                            (config.blade_url.clone(), config.api_key.clone())
                        };
                        let models = get_models(&blade_url, &api_key).await;

                        {
                            let mut mgr = state.chat_manager.lock().unwrap();
                            mgr.agentic_loop.stop("loop detected");
                            // Still send the tool results back to the model so it can respond
                            let mut conversation = state.conversation.lock().unwrap();
                            let config = state.config.lock().unwrap();
                            let selected_model_idx = *state.selected_model_index.lock().unwrap();
                            let ws = state.workspace.lock().unwrap();
                            let http = reqwest::Client::new();

                            mgr.continue_tool_batch(
                                batch,
                                &mut conversation,
                                &config,
                                &models,
                                selected_model_idx,
                                ws.workspace.as_ref(),
                                http,
                            )
                            .unwrap_or_else(|e| eprintln!("Continue batch failed: {}", e));
                        }

                        // Clear approved command roots after loop detection
                        {
                            let mut cache = state.approved_command_roots.lock().unwrap();
                            cache.clear();
                        }

                        // Don't continue the loop - let it finish naturally
                    } else {
                        // Fetch models before acquiring locks
                        let (blade_url, api_key) = {
                            let config = state.config.lock().unwrap();
                            (config.blade_url.clone(), config.api_key.clone())
                        };
                        let models = get_models(&blade_url, &api_key).await;

                        {
                            let mut mgr = state.chat_manager.lock().unwrap();
                            let mut conversation = state.conversation.lock().unwrap();
                            let config = state.config.lock().unwrap();
                            let selected_model_idx = *state.selected_model_index.lock().unwrap();
                            let ws = state.workspace.lock().unwrap();
                            let http = reqwest::Client::new();

                            mgr.continue_tool_batch(
                                batch,
                                &mut conversation,
                                &config,
                                &models,
                                selected_model_idx,
                                ws.workspace.as_ref(), // Ensure this matches Option<&PathBuf>
                                http,
                            )
                            .unwrap_or_else(|e| eprintln!("Continue batch failed: {}", e));
                        }

                        // Clear approved command roots after each AI response completes
                        // This ensures each command batch requires fresh approval
                        {
                            let mut cache = state.approved_command_roots.lock().unwrap();
                            cache.clear();
                        }

                        // Continue polling for the new response
                        continue;
                    }
                }
            }
        }
    });

    Ok(())
}
