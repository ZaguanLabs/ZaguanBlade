use crate::app_state::AppState;
use crate::blade_protocol::{self, BladeError, BladeIntent, SystemEvent, Version};
use crate::chat_orchestrator::handle_send_message;
use crate::commands::{chat, files, tools};
use tauri::{Emitter, State};

#[tauri::command]
pub async fn dispatch(
    envelope: blade_protocol::BladeEnvelope<blade_protocol::BladeIntentEnvelope>,
    window: tauri::Window,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    terminal_manager: State<'_, crate::terminal::TerminalManager>,
) -> Result<(), blade_protocol::BladeError> {
    // 1. Version Check (v1.1: semantic versioning)
    if !Version::CURRENT.is_compatible(&envelope.version) {
        let error = BladeError::VersionMismatch {
            expected: Version::CURRENT,
            received: envelope.version,
        };
        use blade_protocol::BladeEventEnvelope;

        let system_event = SystemEvent::IntentFailed {
            intent_id: envelope.message.id,
            error: error.clone(),
        };

        let event_envelope = BladeEventEnvelope {
            id: uuid::Uuid::new_v4(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            causality_id: Some(envelope.message.id.to_string()),
            event: blade_protocol::BladeEvent::System(system_event),
        };

        let _ = window.emit("blade-event", event_envelope);
        return Err(error);
    }

    let intent_id = envelope.message.id;
    let idempotency_key = envelope.message.idempotency_key.clone();
    let intent = envelope.message.intent;

    // 2. Idempotency Check (v1.1)
    if let Some(ref key) = idempotency_key {
        if let Some((cached_intent_id, cached_result)) = state.idempotency_cache.check(key) {
            println!(
                "[BladeProtocol] Idempotency hit for key '{}' (original intent_id: {})",
                key, cached_intent_id
            );

            // Return cached result
            match cached_result {
                crate::idempotency::IdempotencyResult::Success => {
                    let _ = window.emit("sys-event", SystemEvent::ProcessCompleted { intent_id });
                    return Ok(());
                }
                crate::idempotency::IdempotencyResult::Failed { error } => {
                    let blade_error = BladeError::Internal {
                        trace_id: cached_intent_id.to_string(),
                        message: error,
                    };
                    let _ = window.emit(
                        "sys-event",
                        SystemEvent::IntentFailed {
                            intent_id,
                            error: blade_error.clone(),
                        },
                    );
                    return Err(blade_error);
                }
            }
        }
    }

    // 3. Emit ProtocolVersion on first dispatch
    let protocol_version_event = SystemEvent::ProtocolVersion {
        supported: vec![Version::CURRENT],
        current: Version::CURRENT,
    };
    let _ = window.emit(
        "blade-event",
        blade_protocol::BladeEventEnvelope {
            id: uuid::Uuid::new_v4(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            causality_id: None,
            event: blade_protocol::BladeEvent::System(protocol_version_event),
        },
    );

    // 4. Ack (Process Started)
    let _ = window.emit("sys-event", SystemEvent::ProcessStarted { intent_id });

    // 3. Route Intent
    match intent {
        BladeIntent::Chat(chat_intent) => {
            match chat_intent {
                blade_protocol::ChatIntent::SendMessage {
                    content,
                    model,
                    context,
                } => {
                    // Extract context if available
                    let (
                        active_file,
                        open_files,
                        cursor_line,
                        cursor_column,
                        selection_start,
                        selection_end,
                    ) = if let Some(ctx) = context {
                        (
                            ctx.active_file,
                            Some(ctx.open_files),
                            ctx.cursor_line.map(|l| l as usize),
                            ctx.cursor_column.map(|c| c as usize),
                            ctx.selection_start.map(|l| l as usize),
                            ctx.selection_end.map(|l| l as usize),
                        )
                    } else {
                        let state_af = state.active_file.lock().unwrap().clone();
                        (state_af, None, None, None, None, None)
                    };

                    handle_send_message(
                        content,
                        Some(model),
                        active_file,
                        open_files,
                        cursor_line,
                        cursor_column,
                        selection_start,
                        selection_end,
                        window.clone(),
                        state.clone(),
                        app_handle.clone(),
                    )
                    .await
                    .map_err(|e| blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    })
                }
                blade_protocol::ChatIntent::StopGeneration => {
                    chat::stop_generation(state.clone(), app_handle.clone());
                    Ok(())
                }
                blade_protocol::ChatIntent::ClearHistory => {
                    let mut conversation = state.conversation.lock().unwrap();
                    conversation.clear();
                    let _ = window.emit(
                        "chat-update",
                        blade_protocol::BladeEvent::Chat(blade_protocol::ChatEvent::ChatState {
                            messages: Vec::new(),
                        }),
                    );
                    Ok(())
                }
            }
        }
        BladeIntent::File(file_intent) => match file_intent {
            blade_protocol::FileIntent::Read { path } => {
                match files::read_file_content_logic(path.clone(), &*state) {
                    Ok(content) => {
                        let _ = window.emit(
                            "sys-event",
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Content {
                                path: path,
                                data: content,
                            }),
                        );
                        Ok(())
                    }
                    Err(e) => Err(blade_protocol::BladeError::ResourceNotFound {
                        id: path + " (" + &e + ")",
                    }),
                }
            }
            blade_protocol::FileIntent::Write { path, content } => {
                match files::write_file_content_logic(path.clone(), content, &*state) {
                    Ok(_) => {
                        let _ = window.emit(
                            "sys-event",
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Written {
                                path: path,
                            }),
                        );
                        Ok(())
                    }
                    Err(e) => Err(blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    }),
                }
            }
            blade_protocol::FileIntent::List { path } => {
                match files::list_files_logic(path.clone(), &*state) {
                    Ok(entries) => {
                        let protocol_entries = entries
                            .into_iter()
                            .map(|e| blade_protocol::FileEntry {
                                name: e.name,
                                path: e.path,
                                is_dir: e.is_dir,
                                children: None,
                            })
                            .collect();

                        let _ = window.emit(
                            "sys-event",
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Listing {
                                path: path,
                                entries: protocol_entries,
                            }),
                        );
                        Ok(())
                    }
                    Err(e) => Err(blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    }),
                }
            }
            blade_protocol::FileIntent::Create { path, is_dir } => {
                let resolved_path = {
                    let p = std::path::PathBuf::from(&path);
                    if p.is_absolute() {
                        p
                    } else {
                        let ws = state.workspace.lock().unwrap();
                        if let Some(root) = ws.workspace.as_ref() {
                            root.join(&path)
                        } else {
                            p
                        }
                    }
                };

                let result = if is_dir {
                    std::fs::create_dir_all(&resolved_path)
                } else {
                    if let Some(parent) = resolved_path.parent() {
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            return Err(blade_protocol::BladeError::Internal {
                                trace_id: intent_id.to_string(),
                                message: format!("Failed to create parent directories: {}", e),
                            });
                        }
                    }
                    std::fs::File::create(&resolved_path).map(|_| ())
                };

                match result {
                    Ok(_) => {
                        let _ = window.emit(
                            "sys-event",
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Created {
                                path: path.clone(),
                                is_dir,
                            }),
                        );
                        let _ = window.emit("refresh-explorer", ());
                        Ok(())
                    }
                    Err(e) => Err(blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: format!("{:?}", e),
                    }),
                }
            }
            blade_protocol::FileIntent::Delete { path } => {
                let resolved_path = {
                    let p = std::path::PathBuf::from(&path);
                    if p.is_absolute() {
                        p
                    } else {
                        let ws = state.workspace.lock().unwrap();
                        if let Some(root) = ws.workspace.as_ref() {
                            root.join(&path)
                        } else {
                            p
                        }
                    }
                };

                let result = if resolved_path.is_dir() {
                    std::fs::remove_dir_all(&resolved_path)
                } else {
                    std::fs::remove_file(&resolved_path)
                };

                match result {
                    Ok(_) => {
                        let _ = window.emit(
                            "sys-event",
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Deleted {
                                path: path.clone(),
                            }),
                        );
                        let _ = window.emit("refresh-explorer", ());
                        Ok(())
                    }
                    Err(e) => Err(blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: format!("{:?}", e),
                    }),
                }
            }
            blade_protocol::FileIntent::Rename { old_path, new_path } => {
                let (resolved_old, resolved_new) = {
                    let ws = state.workspace.lock().unwrap();
                    let root = ws.workspace.clone();

                    let resolve = |p: &str| {
                        let path = std::path::PathBuf::from(p);
                        if path.is_absolute() {
                            path
                        } else if let Some(ref r) = root {
                            r.join(p)
                        } else {
                            path
                        }
                    };

                    (resolve(&old_path), resolve(&new_path))
                };

                match std::fs::rename(&resolved_old, &resolved_new) {
                    Ok(_) => {
                        let _ = window.emit(
                            "sys-event",
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Renamed {
                                old_path: old_path.clone(),
                                new_path: new_path.clone(),
                            }),
                        );
                        let _ = window.emit("refresh-explorer", ());
                        Ok(())
                    }
                    Err(e) => Err(blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: format!("{:?}", e),
                    }),
                }
            }
        },
        BladeIntent::Editor(editor_intent) => {
            println!("Editor Intent: {:?}", editor_intent);
            Ok(())
        }
        BladeIntent::Workflow(workflow_intent) => match workflow_intent {
            blade_protocol::WorkflowIntent::ApproveAction { action_id } => {
                println!(
                    "[BladeProtocol] Deprecated intent: ApproveAction({})",
                    action_id
                );
                Ok(())
            }
            blade_protocol::WorkflowIntent::ApproveAll { batch_id } => {
                println!(
                    "[BladeProtocol] Deprecated intent: ApproveAll({})",
                    batch_id
                );
                Ok(())
            }
            blade_protocol::WorkflowIntent::RejectAction { action_id } => {
                println!(
                    "[BladeProtocol] Deprecated intent: RejectAction({})",
                    action_id
                );
                Ok(())
            }
            blade_protocol::WorkflowIntent::RejectAll { batch_id: _ } => Ok(()),
            blade_protocol::WorkflowIntent::ApproveChange { change_id } => {
                println!(
                    "[BladeProtocol] Deprecated intent: ApproveChange({})",
                    change_id
                );
                Ok(())
            }
            blade_protocol::WorkflowIntent::RejectChange { change_id } => {
                println!(
                    "[BladeProtocol] Deprecated intent: RejectChange({})",
                    change_id
                );
                Ok(())
            }
            blade_protocol::WorkflowIntent::ApproveAllChanges => {
                println!("[BladeProtocol] Deprecated intent: ApproveAllChanges");
                Ok(())
            }
            blade_protocol::WorkflowIntent::ApproveTool { approved } => {
                tools::approve_tool(approved, window.clone(), state.clone());
                Ok(())
            }
            blade_protocol::WorkflowIntent::ApproveToolDecision { decision } => {
                tools::approve_tool_decision(decision, window.clone(), state.clone());
                Ok(())
            }
        },
        BladeIntent::Terminal(terminal_intent) => match terminal_intent {
            blade_protocol::TerminalIntent::Spawn {
                id,
                command,
                cwd,
                owner: _,
                interactive,
            } => {
                if interactive {
                    crate::terminal::create_terminal(
                        id,
                        cwd,
                        command,
                        app_handle.clone(),
                        terminal_manager.clone(),
                    )
                    .map_err(|e| blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    })
                } else {
                    match command {
                        Some(cmd) => crate::terminal::execute_command_in_terminal(
                            id,
                            cmd,
                            cwd,
                            app_handle.clone(),
                            state.clone(),
                        )
                        .map_err(|e| {
                            blade_protocol::BladeError::Internal {
                                trace_id: intent_id.to_string(),
                                message: e,
                            }
                        }),
                        None => Err(blade_protocol::BladeError::ValidationError {
                            field: "command".into(),
                            message: "Command required for non-interactive spawn".into(),
                        }),
                    }
                }
            }
            blade_protocol::TerminalIntent::Input { id, data } => {
                crate::terminal::write_to_terminal(id, data, terminal_manager.clone()).map_err(
                    |e| blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    },
                )
            }
            blade_protocol::TerminalIntent::Resize { id, rows, cols } => {
                crate::terminal::resize_terminal(id, rows, cols, terminal_manager.clone()).map_err(
                    |e| blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    },
                )
            }
            blade_protocol::TerminalIntent::Kill { id: _ } => Ok(()),
        },
        BladeIntent::History(history_intent) => {
            match history_intent {
                blade_protocol::HistoryIntent::ListConversations { project_id } => {
                    println!("[History] ListConversations: project={}", project_id);

                    // Get config to create BladeClient
                    let (blade_url, api_key) = {
                        let config = state.config.lock().unwrap();
                        (config.blade_url.clone(), config.api_key.clone())
                    };

                    // Create HTTP client and BladeClient
                    let http_client = reqwest::Client::new();
                    let blade_client =
                        crate::blade_client::BladeClient::new(blade_url, http_client, api_key);

                    // Call API
                    match blade_client.get_conversation_history(&project_id).await {
                        Ok(response) => {
                            // Parse response into ConversationSummary vec
                            let conversations: Vec<blade_protocol::ConversationSummary> =
                                if let Some(convs) = response.get("conversations") {
                                    serde_json::from_value(convs.clone()).unwrap_or_else(|e| {
                                        eprintln!("[History] Failed to parse conversations: {}", e);
                                        Vec::new()
                                    })
                                } else {
                                    Vec::new()
                                };

                            let _ = window.emit(
                                "blade-event",
                                blade_protocol::BladeEventEnvelope {
                                    id: uuid::Uuid::new_v4(),
                                    timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis()
                                        as u64,
                                    causality_id: Some(intent_id.to_string()),
                                    event: blade_protocol::BladeEvent::History(
                                        blade_protocol::HistoryEvent::ConversationList {
                                            conversations,
                                        },
                                    ),
                                },
                            );

                            Ok(())
                        }
                        Err(e) => {
                            let error = blade_protocol::BladeError::Internal {
                                trace_id: intent_id.to_string(),
                                message: format!("{:?}", e),
                            };
                            let _ = window.emit(
                                "blade-event",
                                blade_protocol::BladeEventEnvelope {
                                    id: uuid::Uuid::new_v4(),
                                    timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis()
                                        as u64,
                                    causality_id: Some(intent_id.to_string()),
                                    event: blade_protocol::BladeEvent::System(
                                        blade_protocol::SystemEvent::IntentFailed {
                                            intent_id,
                                            error: error.clone(),
                                        },
                                    ),
                                },
                            );
                            Err(error)
                        }
                    }
                }
                blade_protocol::HistoryIntent::LoadConversation { session_id } => {
                    println!("[History] LoadConversation: session={}", session_id);

                    // Get config to create BladeClient
                    let (blade_url, api_key) = {
                        let config = state.config.lock().unwrap();
                        (config.blade_url.clone(), config.api_key.clone())
                    };

                    // Create HTTP client and BladeClient
                    let http_client = reqwest::Client::new();
                    let blade_client =
                        crate::blade_client::BladeClient::new(blade_url, http_client, api_key);

                    // Call API
                    match blade_client.get_conversation(&session_id).await {
                        Ok(response) => {
                            // Parse response as FullConversation
                            match serde_json::from_value::<blade_protocol::FullConversation>(
                                response,
                            ) {
                                Ok(full_conversation) => {
                                    // Verify message structure for debugging
                                    eprintln!(
                                        "[History] Loaded {} messages for session {}",
                                        full_conversation.messages.len(),
                                        full_conversation.session_id
                                    );

                                    // CRITICAL FIX: Update backend state to match the loaded conversation
                                    // This ensures that subsequent "SendMessage" intents use the correct context and session ID
                                    {
                                        let mut conversation = state.conversation.lock().unwrap();

                                        // Clear current conversation
                                        conversation.clear();

                                        // Update metadata
                                        conversation.metadata.id = uuid::Uuid::new_v4().to_string(); // Temporary local ID
                                        conversation.metadata.session_id =
                                            Some(full_conversation.session_id.clone());
                                        conversation.metadata.title =
                                            full_conversation.title.clone();
                                        // We don't have model_id in FullConversation, keep default or guess?
                                        // Ideally we should get it. For now, keep existing or default.

                                        // Convert messages
                                        for msg in &full_conversation.messages {
                                            let role = match msg.role.as_str() {
                                                "user" => crate::protocol::ChatRole::User,
                                                "assistant" => crate::protocol::ChatRole::Assistant,
                                                "system" => crate::protocol::ChatRole::System,
                                                "tool" => crate::protocol::ChatRole::Tool,
                                                _ => crate::protocol::ChatRole::User,
                                            };

                                            let mut chat_msg = crate::protocol::ChatMessage::new(
                                                role,
                                                msg.content.clone(),
                                            );

                                            // Handle tool calls if present
                                            if let Some(ref tc_val) = msg.tool_calls {
                                                if let Ok(tool_calls) = serde_json::from_value::<
                                                    Vec<crate::protocol::ToolCall>,
                                                >(
                                                    tc_val.clone()
                                                ) {
                                                    chat_msg.tool_calls = Some(tool_calls);
                                                }
                                            }

                                            chat_msg.tool_call_id = msg.tool_call_id.clone();
                                            // created_at is strictly for display in history, ChatMessage doesn't store it per message usually (or defaults to now)

                                            conversation.push(chat_msg);
                                        }

                                        eprintln!("[History] Updated backend conversation state");
                                    }

                                    // Update ChatManager session_id
                                    {
                                        let mut mgr = state.chat_manager.lock().unwrap();
                                        mgr.session_id = Some(full_conversation.session_id.clone());
                                        eprintln!(
                                            "[History] Updated ChatManager session_id to {}",
                                            full_conversation.session_id
                                        );
                                    }

                                    let _ = window.emit(
                                        "blade-event",
                                        blade_protocol::BladeEventEnvelope {
                                            id: uuid::Uuid::new_v4(),
                                            timestamp: std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_millis()
                                                as u64,
                                            causality_id: Some(intent_id.to_string()),
                                            event: blade_protocol::BladeEvent::History(
                                                blade_protocol::HistoryEvent::ConversationLoaded(
                                                    full_conversation,
                                                ),
                                            ),
                                        },
                                    );
                                    Ok(())
                                }
                                Err(e) => {
                                    eprintln!("[History] Failed to parse conversation data: {}", e);
                                    Err(blade_protocol::BladeError::Internal {
                                        trace_id: intent_id.to_string(),
                                        message: format!(
                                            "Failed to parse conversation data: {}",
                                            e
                                        ),
                                    })
                                }
                            }
                        }
                        Err(e) => Err(blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: format!("{:?}", e),
                        }),
                    }
                }
            }
        }
        BladeIntent::System(system_intent) => {
            println!("System Intent: {:?}", system_intent);
            Ok(())
        }
        BladeIntent::Language(language_intent) => {
            match language_intent {
                blade_protocol::LanguageIntent::ZlpMessage { payload } => {
                    println!("[Language] Dispatching ZLP Message: {:?}", payload);

                    // 1. Get config
                    let (blade_url, api_key) = {
                        let config = state.config.lock().unwrap();
                        (config.blade_url.clone(), config.api_key.clone())
                    };

                    // 2. Create client
                    let http_client = reqwest::Client::new();
                    let blade_client =
                        crate::blade_client::BladeClient::new(blade_url, http_client, api_key);

                    // 3. Send request
                    let mut rx = blade_client.send_zlp_request(payload).await.map_err(|e| {
                        blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: format!("ZLP Request Failed: {}", e),
                        }
                    })?;

                    let window_clone = window.clone();
                    let intent_id_clone = intent_id;

                    // 4. Process response stream
                    tauri::async_runtime::spawn(async move {
                        while let Some(event) = rx.recv().await {
                            match event {
                                crate::blade_client::BladeEvent::ZlpResponse(val) => {
                                    let _ = window_clone.emit(
                                        "blade-event",
                                        blade_protocol::BladeEventEnvelope {
                                            id: uuid::Uuid::new_v4(),
                                            timestamp: std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_millis()
                                                as u64,
                                            causality_id: Some(intent_id_clone.to_string()),
                                            event: blade_protocol::BladeEvent::Language(
                                                blade_protocol::LanguageEvent::ZlpResponse {
                                                    original_request_id: intent_id_clone
                                                        .to_string(),
                                                    payload: val,
                                                },
                                            ),
                                        },
                                    );
                                }
                                crate::blade_client::BladeEvent::Error {
                                    code,
                                    message,
                                    details,
                                } => {
                                    eprintln!("[ZLP] Error: {} - {} ({})", code, message, details);
                                    let _ = window_clone.emit(
                                        "sys-event",
                                        blade_protocol::SystemEvent::IntentFailed {
                                            intent_id: intent_id_clone,
                                            error: blade_protocol::BladeError::Internal {
                                                trace_id: intent_id_clone.to_string(),
                                                message: format!("{}: {}", code, message),
                                            },
                                        },
                                    );
                                }
                                _ => {
                                    // Ignore other events for now or map them?
                                    // ZLP might use progress/status events later
                                }
                            }
                        }
                        // Emit completion
                        let _ = window_clone.emit(
                            "sys-event",
                            SystemEvent::ProcessCompleted {
                                intent_id: intent_id_clone,
                            },
                        );
                    });

                    Ok(())
                }
                other => {
                    eprintln!("[Language] Intent received: {:?}", other);
                    let maybe_event = state
                        .language_handler
                        .handle(other, intent_id)
                        .await
                        .map_err(|e| blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: format!("{:?}", e),
                        })?;

                    if let Some(event) = maybe_event {
                        let _ = window.emit("blade-event", event);
                    }
                    Ok(())
                }
            }
        }
    }
}
