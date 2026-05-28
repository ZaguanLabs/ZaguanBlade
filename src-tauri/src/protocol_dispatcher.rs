use crate::app_state::AppState;
use crate::blade_event_scheduler;
use crate::blade_protocol::{self, BladeError, BladeIntent, SystemEvent, Version};
use crate::chat_orchestrator::handle_send_message;
use crate::commands::{chat, files, tools};
use std::sync::atomic::Ordering;
use tauri::{Manager, State};
use tokio::fs;

fn emit_blade_event(
    window: &tauri::Window,
    causality_id: Option<String>,
    event: blade_protocol::BladeEvent,
) {
    blade_event_scheduler::emit_blade_event(&window.app_handle(), causality_id, event);
}

fn emit_system_event(window: &tauri::Window, intent_id: uuid::Uuid, event: SystemEvent) {
    emit_blade_event(
        window,
        Some(intent_id.to_string()),
        blade_protocol::BladeEvent::System(event),
    );
}

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

        let system_event = SystemEvent::IntentFailed {
            intent_id: envelope.message.id,
            error: error.clone(),
        };

        emit_system_event(&window, envelope.message.id, system_event);
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
                    emit_system_event(
                        &window,
                        intent_id,
                        SystemEvent::ProcessCompleted { intent_id },
                    );
                    return Ok(());
                }
                crate::idempotency::IdempotencyResult::Failed { error } => {
                    let blade_error = BladeError::Internal {
                        trace_id: cached_intent_id.to_string(),
                        message: error,
                    };
                    emit_system_event(
                        &window,
                        intent_id,
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
    if !state.protocol_version_emitted.swap(true, Ordering::SeqCst) {
        let protocol_version_event = SystemEvent::ProtocolVersion {
            supported: vec![Version::CURRENT],
            current: Version::CURRENT,
        };
        blade_event_scheduler::emit_blade_event(
            &window.app_handle(),
            None,
            blade_protocol::BladeEvent::System(protocol_version_event),
        );
    }

    // 4. Ack (Process Started)
    emit_system_event(
        &window,
        intent_id,
        SystemEvent::ProcessStarted { intent_id },
    );

    // 3. Route Intent
    match intent {
        BladeIntent::Chat(chat_intent) => {
            match chat_intent {
                blade_protocol::ChatIntent::SendMessage {
                    content,
                    model,
                    images,
                    context,
                    mentions,
                    mode,
                } => {
                    // Extract context if available
                    let (
                        active_file,
                        open_files,
                        cursor_line,
                        cursor_column,
                        selection_start,
                        selection_end,
                        diagnostics,
                    ) = if let Some(ctx) = context {
                        (
                            ctx.active_file,
                            Some(ctx.open_files),
                            ctx.cursor_line.map(|l| l as usize),
                            ctx.cursor_column.map(|c| c as usize),
                            ctx.selection_start.map(|l| l as usize),
                            ctx.selection_end.map(|l| l as usize),
                            Some(ctx.diagnostics),
                        )
                    } else {
                        let state_af = state.active_file.lock().unwrap().clone();
                        (state_af, None, None, None, None, None, None)
                    };

                    handle_send_message(
                        content,
                        images,
                        Some(model),
                        active_file,
                        open_files,
                        cursor_line,
                        cursor_column,
                        selection_start,
                        selection_end,
                        diagnostics,
                        mentions,
                        mode,
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
                blade_protocol::ChatIntent::StopGeneration {} => {
                    chat::stop_generation(state.clone(), app_handle.clone());
                    Ok(())
                }
                blade_protocol::ChatIntent::ClearHistory {} => {
                    let mut conversation = state.conversation.lock().map_err(|e| {
                        blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: format!("Failed to lock conversation state: {}", e),
                        }
                    })?;
                    conversation.clear();
                    emit_blade_event(
                        &window,
                        Some(intent_id.to_string()),
                        blade_protocol::BladeEvent::Chat(blade_protocol::ChatEvent::ChatState {
                            messages: Vec::new(),
                        }),
                    );
                    Ok(())
                }
                blade_protocol::ChatIntent::NewConversation { model } => {
                    chat::new_conversation(model, state.clone(), app_handle.clone())
                        .await
                        .map(|_| ())
                        .map_err(|e| blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: e,
                        })
                }
                blade_protocol::ChatIntent::SetSelectedModel { model } => {
                    chat::set_selected_model(model, state.clone())
                        .await
                        .map_err(|e| blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: e,
                        })
                }
            }
        }
        BladeIntent::File(file_intent) => match file_intent {
            blade_protocol::FileIntent::Read { path } => {
                let requested_path = std::path::PathBuf::from(&path);
                let resolved_path =
                    files::resolve_path_under_workspace_async(&*state, requested_path)
                        .await
                        .map_err(|e| BladeError::ResourceNotFound {
                            id: path.clone() + " (" + &e + ")",
                        })?;

                match fs::read_to_string(&resolved_path).await {
                    Ok(content) => {
                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Content {
                                path: path,
                                data: content,
                            }),
                        );
                        Ok(())
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Content {
                                path: path,
                                data: String::new(),
                            }),
                        );
                        Ok(())
                    }
                    Err(e) => Err(blade_protocol::BladeError::ResourceNotFound {
                        id: path + " (" + &e.to_string() + ")",
                    }),
                }
            }
            blade_protocol::FileIntent::Write { path, content } => {
                let requested_path = std::path::PathBuf::from(&path);
                let resolved_path =
                    files::resolve_path_under_workspace_async(&*state, requested_path)
                        .await
                        .map_err(|e| blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: e,
                        })?;

                match fs::write(&resolved_path, content).await {
                    Ok(_) => {
                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Written {
                                path: path,
                            }),
                        );
                        Ok(())
                    }
                    Err(e) => Err(blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            blade_protocol::FileIntent::List { path } => {
                let blocking_app_handle = app_handle.clone();
                let blocking_path = path.clone();
                match tokio::task::spawn_blocking(move || {
                    let state = blocking_app_handle.state::<AppState>();
                    files::list_files_logic(blocking_path, &*state)
                })
                .await
                .map_err(|e| blade_protocol::BladeError::Internal {
                    trace_id: intent_id.to_string(),
                    message: format!("directory listing task failed: {}", e),
                })? {
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

                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
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
                let resolved_path = files::resolve_path_under_workspace_async(
                    &*state,
                    std::path::PathBuf::from(&path),
                )
                .await
                .map_err(|e| blade_protocol::BladeError::Internal {
                    trace_id: intent_id.to_string(),
                    message: e,
                })?;

                let result = if is_dir {
                    tokio::fs::create_dir_all(&resolved_path)
                        .await
                        .map_err(|e| e.to_string())
                } else {
                    if let Some(parent) = resolved_path.parent() {
                        if let Err(e) = tokio::fs::create_dir_all(parent).await {
                            return Err(blade_protocol::BladeError::Internal {
                                trace_id: intent_id.to_string(),
                                message: format!("Failed to create parent directories: {}", e),
                            });
                        }
                    }
                    tokio::fs::File::create(&resolved_path)
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                };

                match result {
                    Ok(_) => {
                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Created {
                                path: path.clone(),
                                is_dir,
                            }),
                        );
                        blade_event_scheduler::queue_refresh_explorer(&window.app_handle());
                        Ok(())
                    }
                    Err(e) => Err(blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: format!("{:?}", e),
                    }),
                }
            }
            blade_protocol::FileIntent::Delete { path } => {
                let resolved_path = files::resolve_path_under_workspace_async(
                    &*state,
                    std::path::PathBuf::from(&path),
                )
                .await
                .map_err(|e| blade_protocol::BladeError::Internal {
                    trace_id: intent_id.to_string(),
                    message: e,
                })?;

                let result = if resolved_path.is_dir() {
                    tokio::fs::remove_dir_all(&resolved_path)
                        .await
                        .map_err(|e| e.to_string())
                } else {
                    tokio::fs::remove_file(&resolved_path)
                        .await
                        .map_err(|e| e.to_string())
                };

                match result {
                    Ok(_) => {
                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Deleted {
                                path: path.clone(),
                            }),
                        );
                        blade_event_scheduler::queue_refresh_explorer(&window.app_handle());
                        Ok(())
                    }
                    Err(e) => Err(blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: format!("{:?}", e),
                    }),
                }
            }
            blade_protocol::FileIntent::Rename { old_path, new_path } => {
                let resolved_old = files::resolve_path_under_workspace_async(
                    &*state,
                    std::path::PathBuf::from(&old_path),
                )
                .await
                .map_err(|e| blade_protocol::BladeError::Internal {
                    trace_id: intent_id.to_string(),
                    message: e,
                })?;
                let resolved_new = files::resolve_path_under_workspace_async(
                    &*state,
                    std::path::PathBuf::from(&new_path),
                )
                .await
                .map_err(|e| blade_protocol::BladeError::Internal {
                    trace_id: intent_id.to_string(),
                    message: e,
                })?;

                match tokio::fs::rename(&resolved_old, &resolved_new).await {
                    Ok(_) => {
                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::File(blade_protocol::FileEvent::Renamed {
                                old_path: old_path.clone(),
                                new_path: new_path.clone(),
                            }),
                        );
                        blade_event_scheduler::queue_refresh_explorer(&window.app_handle());
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
            // Check if backend authority is enabled
            let backend_authority = state.feature_flags.editor_backend_authority();

            match editor_intent {
                blade_protocol::EditorIntent::OpenFile { path } => {
                    if backend_authority {
                        // Update backend state
                        {
                            let mut active = state.active_file.lock().unwrap();
                            *active = Some(path.clone());
                        }
                        {
                            let mut open = state.open_files.lock().unwrap();
                            if !open.contains(&path) {
                                open.push(path.clone());
                            }
                        }

                        // Emit FileOpened event
                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::Editor(
                                blade_protocol::EditorEvent::FileOpened { path },
                            ),
                        );
                    } else {
                        // Legacy: just log, frontend handles state
                        println!("[Editor] OpenFile (frontend authority): {}", path);
                    }
                    Ok(())
                }
                blade_protocol::EditorIntent::CloseFile { path } => {
                    if backend_authority {
                        {
                            let mut open = state.open_files.lock().unwrap();
                            open.retain(|p| p != &path);
                        }
                        // If closing the active file, clear it
                        {
                            let mut active = state.active_file.lock().unwrap();
                            if active.as_ref() == Some(&path) {
                                *active = None;
                            }
                        }

                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::Editor(
                                blade_protocol::EditorEvent::FileClosed { path },
                            ),
                        );
                    }
                    Ok(())
                }
                blade_protocol::EditorIntent::SetActiveFile { path } => {
                    if backend_authority {
                        {
                            let mut active = state.active_file.lock().unwrap();
                            *active = path.clone();
                        }

                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::Editor(
                                blade_protocol::EditorEvent::ActiveFileChanged { path },
                            ),
                        );
                    }
                    Ok(())
                }
                blade_protocol::EditorIntent::SyncDocument {
                    path,
                    content,
                    version,
                } => {
                    let blocking_app_handle = app_handle.clone();
                    let sync_path = path.clone();
                    let sync_content = content.clone();
                    match tokio::task::spawn_blocking(move || {
                        let state = blocking_app_handle.state::<AppState>();
                        let service = state.language_service()?;
                        if version <= 1 {
                            service.did_open(&sync_path, &sync_content)
                        } else {
                            service.did_change(&sync_path, version as i32, &sync_content)
                        }
                        .map_err(|error| error.to_string())
                    })
                    .await
                    {
                        Ok(Err(error)) => {
                            eprintln!(
                                "[Editor] Live document sync skipped for {}: {}",
                                path, error
                            );
                        }
                        Err(error) => {
                            eprintln!(
                                "[Editor] Live document sync task failed for {}: {}",
                                path, error
                            );
                        }
                        Ok(Ok(())) => {}
                    }
                    Ok(())
                }
                blade_protocol::EditorIntent::CloseDocument { path } => {
                    let blocking_app_handle = app_handle.clone();
                    let close_path = path.clone();
                    match tokio::task::spawn_blocking(move || {
                        let state = blocking_app_handle.state::<AppState>();
                        let service = state.language_service()?;
                        service
                            .did_close(&close_path)
                            .map_err(|error| error.to_string())
                    })
                    .await
                    {
                        Ok(Err(error)) => {
                            eprintln!("[Editor] Failed to close live document {}: {}", path, error);
                        }
                        Err(error) => {
                            eprintln!(
                                "[Editor] Live document close task failed for {}: {}",
                                path, error
                            );
                        }
                        Ok(Ok(())) => {}
                    }
                    Ok(())
                }
                blade_protocol::EditorIntent::UpdateCursor { line, column } => {
                    // Always update backend state for AI context (regardless of authority mode)
                    {
                        let mut cursor_line = state.cursor_line.lock().unwrap();
                        let mut cursor_col = state.cursor_column.lock().unwrap();
                        *cursor_line = Some(line as usize);
                        *cursor_col = Some(column as usize);
                    }
                    // No event emission for cursor - too frequent, would cause noise
                    Ok(())
                }
                blade_protocol::EditorIntent::UpdateSelection { start, end } => {
                    // Always update backend state for AI context
                    {
                        let mut sel_start = state.selection_start_line.lock().unwrap();
                        let mut sel_end = state.selection_end_line.lock().unwrap();
                        *sel_start = Some(start as usize);
                        *sel_end = Some(end as usize);
                    }
                    Ok(())
                }
                blade_protocol::EditorIntent::GetState {} => {
                    // Return current editor state snapshot
                    let snapshot = {
                        let active = state.active_file.lock().unwrap();
                        let open = state.open_files.lock().unwrap();
                        let cursor_line = state.cursor_line.lock().unwrap();
                        let cursor_col = state.cursor_column.lock().unwrap();
                        let sel_start = state.selection_start_line.lock().unwrap();
                        let sel_end = state.selection_end_line.lock().unwrap();

                        blade_protocol::EditorEvent::StateSnapshot {
                            active_file: active.clone(),
                            open_files: open.clone(),
                            cursor_line: cursor_line.map(|l| l as u32),
                            cursor_column: cursor_col.map(|c| c as u32),
                            selection_start: sel_start.map(|l| l as u32),
                            selection_end: sel_end.map(|l| l as u32),
                        }
                    };

                    emit_blade_event(
                        &window,
                        Some(intent_id.to_string()),
                        blade_protocol::BladeEvent::Editor(snapshot),
                    );
                    Ok(())
                }
                // Tab management (headless)
                blade_protocol::EditorIntent::OpenTab {
                    id,
                    title,
                    path,
                    tab_type,
                    content,
                    suggested_name,
                } => {
                    let tabs_authority = state.feature_flags.tabs_backend_authority();

                    if tabs_authority {
                        let tab_info = crate::core_state::TabInfo {
                            id: id.clone(),
                            title,
                            path: path.clone(),
                            is_dirty: false,
                            tab_type: match tab_type.as_deref() {
                                Some("ephemeral") => crate::core_state::TabType::Ephemeral {
                                    content: content.unwrap_or_default(),
                                    suggested_name: suggested_name.unwrap_or_default(),
                                },
                                _ => crate::core_state::TabType::File,
                            },
                        };

                        {
                            let mut tabs = state.tabs.lock().unwrap();
                            // Remove existing tab with same ID if present
                            tabs.retain(|t| t.id != id);
                            tabs.push(tab_info.clone());
                        }
                        {
                            let mut active = state.active_tab_id.lock().unwrap();
                            *active = Some(id.clone());
                        }
                        // Also update active_file if it's a file tab
                        if let Some(ref p) = path {
                            let mut active_file = state.active_file.lock().unwrap();
                            *active_file = Some(p.clone());
                        }

                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::Editor(
                                blade_protocol::EditorEvent::TabOpened { tab: tab_info },
                            ),
                        );
                    } else {
                        println!("[Editor] OpenTab (frontend authority): {}", id);
                    }
                    Ok(())
                }
                blade_protocol::EditorIntent::CloseTab { tab_id } => {
                    let tabs_authority = state.feature_flags.tabs_backend_authority();

                    if tabs_authority {
                        {
                            let mut tabs = state.tabs.lock().unwrap();
                            tabs.retain(|t| t.id != tab_id);
                        }
                        {
                            let mut active = state.active_tab_id.lock().unwrap();
                            if active.as_ref() == Some(&tab_id) {
                                *active = None;
                            }
                        }

                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::Editor(
                                blade_protocol::EditorEvent::TabClosed { tab_id },
                            ),
                        );
                    }
                    Ok(())
                }
                blade_protocol::EditorIntent::SetActiveTab { tab_id } => {
                    let tabs_authority = state.feature_flags.tabs_backend_authority();

                    if tabs_authority {
                        // Get the path of the tab being activated (if it's a file tab)
                        let tab_path = {
                            let tabs = state.tabs.lock().unwrap();
                            tab_id.as_ref().and_then(|id| {
                                tabs.iter()
                                    .find(|t| &t.id == id)
                                    .and_then(|t| t.path.clone())
                            })
                        };

                        {
                            let mut active = state.active_tab_id.lock().unwrap();
                            *active = tab_id.clone();
                        }
                        // Also update active_file
                        {
                            let mut active_file = state.active_file.lock().unwrap();
                            *active_file = tab_path;
                        }

                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::Editor(
                                blade_protocol::EditorEvent::ActiveTabChanged { tab_id },
                            ),
                        );
                    }
                    Ok(())
                }
                blade_protocol::EditorIntent::ReorderTabs { tab_ids } => {
                    let tabs_authority = state.feature_flags.tabs_backend_authority();

                    if tabs_authority {
                        {
                            let mut tabs = state.tabs.lock().unwrap();
                            // Reorder tabs according to the provided order
                            let mut reordered = Vec::with_capacity(tab_ids.len());
                            for id in &tab_ids {
                                if let Some(tab) = tabs.iter().find(|t| &t.id == id).cloned() {
                                    reordered.push(tab);
                                }
                            }
                            *tabs = reordered;
                        }

                        emit_blade_event(
                            &window,
                            Some(intent_id.to_string()),
                            blade_protocol::BladeEvent::Editor(
                                blade_protocol::EditorEvent::TabsReordered { tab_ids },
                            ),
                        );
                    }
                    Ok(())
                }
                blade_protocol::EditorIntent::GetTabState {} => {
                    let snapshot = {
                        let tabs = state.tabs.lock().unwrap();
                        let active = state.active_tab_id.lock().unwrap();

                        blade_protocol::EditorEvent::TabStateSnapshot {
                            tabs: tabs.clone(),
                            active_tab_id: active.clone(),
                        }
                    };

                    emit_blade_event(
                        &window,
                        Some(intent_id.to_string()),
                        blade_protocol::BladeEvent::Editor(snapshot),
                    );
                    Ok(())
                }
            }
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
            blade_protocol::WorkflowIntent::ApproveTool { approved } => {
                tools::approve_tool_decision(
                    if approved {
                        "approve_once".to_string()
                    } else {
                        "reject".to_string()
                    },
                    app_handle.clone(),
                )
                .await;
                Ok(())
            }
            blade_protocol::WorkflowIntent::ApproveToolDecision { decision } => {
                tools::approve_tool_decision(decision, app_handle.clone()).await;
                Ok(())
            }
        },
        BladeIntent::Terminal(terminal_intent) => match terminal_intent {
            blade_protocol::TerminalIntent::Spawn {
                id,
                command,
                cwd,
                owner,
                interactive,
            } => {
                let blocking_app_handle = app_handle.clone();
                let missing_command_for_non_interactive = !interactive && command.is_none();
                tokio::task::spawn_blocking(move || {
                    if interactive {
                        let terminal_app_handle = blocking_app_handle.clone();
                        let terminal_manager =
                            blocking_app_handle.state::<crate::terminal::TerminalManager>();
                        crate::terminal::create_terminal(
                            id,
                            cwd,
                            command,
                            owner,
                            terminal_app_handle,
                            terminal_manager,
                        )
                    } else {
                        match command {
                            Some(cmd) => {
                                let terminal_app_handle = blocking_app_handle.clone();
                                let state = blocking_app_handle.state::<AppState>();
                                crate::terminal::execute_command_in_terminal(
                                    id,
                                    cmd,
                                    cwd,
                                    terminal_app_handle,
                                    state,
                                )
                            }
                            None => Err("Command required for non-interactive spawn".to_string()),
                        }
                    }
                })
                .await
                .map_err(|e| blade_protocol::BladeError::Internal {
                    trace_id: intent_id.to_string(),
                    message: format!("terminal spawn task failed: {}", e),
                })?
                .map_err(|e| {
                    if missing_command_for_non_interactive {
                        blade_protocol::BladeError::ValidationError {
                            field: "command".into(),
                            message: e,
                        }
                    } else {
                        blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: e,
                        }
                    }
                })
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
            blade_protocol::TerminalIntent::Kill { id } => {
                crate::terminal::kill_terminal(id, terminal_manager.clone()).map_err(|e| {
                    blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    }
                })
            }
        },
        BladeIntent::History(history_intent) => match history_intent {
            blade_protocol::HistoryIntent::ListConversations { project_id } => {
                println!("[History] ListConversations: project={}", project_id);

                let is_local = state.is_local_model_active().await;
                if is_local {
                    match state.with_conversation_store(|store| Ok(store.list_conversations())) {
                        Ok(conversations) => {
                            let summaries = conversations
                                .into_iter()
                                .map(|c| blade_protocol::ConversationSummary {
                                    id: c.id,
                                    project_id: project_id.clone(),
                                    title: c.title,
                                    created_at: c.created_at.to_rfc3339(),
                                    last_active_at: c.updated_at.to_rfc3339(),
                                    message_count: c.message_count as u32,
                                    preview: String::new(),
                                })
                                .collect();

                            emit_blade_event(
                                &window,
                                Some(intent_id.to_string()),
                                blade_protocol::BladeEvent::History(
                                    blade_protocol::HistoryEvent::ConversationList { conversations: summaries },
                                ),
                            );

                            Ok(())
                        }
                        Err(e) => {
                            let error = blade_protocol::BladeError::Internal {
                                trace_id: intent_id.to_string(),
                                message: e,
                            };
                            emit_system_event(
                                &window,
                                intent_id,
                                blade_protocol::SystemEvent::IntentFailed {
                                    intent_id,
                                    error: error.clone(),
                                },
                            );
                            Err(error)
                        }
                    }
                } else {
                    let ws_manager = state.ws_connection.clone();

                    match ws_manager.request_history_list(project_id).await {
                        Ok(conversations) => {
                            emit_blade_event(
                                &window,
                                Some(intent_id.to_string()),
                                blade_protocol::BladeEvent::History(
                                    blade_protocol::HistoryEvent::ConversationList { conversations },
                                ),
                            );

                            Ok(())
                        }
                        Err(e) => {
                            let error = blade_protocol::BladeError::Internal {
                                trace_id: intent_id.to_string(),
                                message: e,
                            };
                            emit_system_event(
                                &window,
                                intent_id,
                                blade_protocol::SystemEvent::IntentFailed {
                                    intent_id,
                                    error: error.clone(),
                                },
                            );
                            Err(error)
                        }
                    }
                }
            }
            blade_protocol::HistoryIntent::LoadConversation { session_id } => {
                println!("[History] LoadConversation: session={}", session_id);

                let is_local = state.is_local_model_active().await;
                if is_local {
                    match state.with_conversation_store(|store| store.load_conversation(&session_id)) {
                        Ok(stored) => {
                            let messages = stored
                                .messages
                                .into_iter()
                                .map(|m| blade_protocol::HistoryMessage {
                                    role: m.role,
                                    content: m.content,
                                    tool_calls: None,
                                    tool_call_id: m.tool_call_id,
                                    created_at: chrono::Utc::now().to_rfc3339(),
                                })
                                .collect();

                            let ws_path = state.workspace_root();
                            let project_id = ws_path
                                .as_deref()
                                .and_then(|p| crate::project::get_or_create_project_id(p).ok())
                                .unwrap_or_else(|| "unknown".to_string());

                            let full_conversation = blade_protocol::FullConversation {
                                session_id: stored.metadata.session_id.unwrap_or(session_id),
                                project_id,
                                title: stored.metadata.title,
                                created_at: stored.metadata.created_at.to_rfc3339(),
                                last_active_at: stored.metadata.updated_at.to_rfc3339(),
                                message_count: stored.metadata.message_count as u32,
                                messages,
                            };

                            eprintln!(
                                "[History] Loaded {} messages locally for session {}",
                                full_conversation.messages.len(),
                                full_conversation.session_id
                            );

                            {
                                let mut conversation = state.conversation.lock().unwrap();

                                conversation.clear();

                                conversation.metadata.id = stored.metadata.id.clone();
                                conversation.metadata.session_id =
                                    Some(full_conversation.session_id.clone());
                                conversation.metadata.title = full_conversation.title.clone();

                                for msg in &full_conversation.messages {
                                    let role = match msg.role.as_str() {
                                        "user" => crate::protocol::ChatRole::User,
                                        "assistant" => crate::protocol::ChatRole::Assistant,
                                        "system" => crate::protocol::ChatRole::System,
                                        "tool" => crate::protocol::ChatRole::Tool,
                                        _ => crate::protocol::ChatRole::User,
                                    };

                                    let mut chat_msg =
                                        crate::protocol::ChatMessage::new(role, msg.content.clone());

                                    if let Some(ref tc_val) = msg.tool_calls {
                                        if let Ok(tool_calls) =
                                            serde_json::from_value::<Vec<crate::protocol::ToolCall>>(
                                                tc_val.clone(),
                                            )
                                        {
                                            chat_msg.tool_calls = Some(tool_calls);
                                        }
                                    }

                                    chat_msg.tool_call_id = msg.tool_call_id.clone();

                                    conversation.push(chat_msg);
                                }

                                eprintln!("[History] Updated backend conversation state locally");
                            }

                            {
                                let mut mgr = state.chat_manager.lock().unwrap();
                                mgr.session_id = Some(full_conversation.session_id.clone());
                                eprintln!(
                                    "[History] Updated ChatManager session_id locally to {}",
                                    full_conversation.session_id
                                );
                            }

                            emit_blade_event(
                                &window,
                                Some(intent_id.to_string()),
                                blade_protocol::BladeEvent::History(
                                    blade_protocol::HistoryEvent::ConversationLoaded(full_conversation),
                                ),
                            );
                            Ok(())
                        }
                        Err(e) => Err(blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: e,
                        }),
                    }
                } else {
                    let ws_manager = state.ws_connection.clone();

                    match ws_manager.request_history_detail(session_id).await {
                        Ok(full_conversation) => {
                            eprintln!(
                                "[History] Loaded {} messages for session {}",
                                full_conversation.messages.len(),
                                full_conversation.session_id
                            );

                            {
                                let mut conversation = state.conversation.lock().unwrap();

                                conversation.clear();

                                conversation.metadata.id = uuid::Uuid::new_v4().to_string();
                                conversation.metadata.session_id =
                                    Some(full_conversation.session_id.clone());
                                conversation.metadata.title = full_conversation.title.clone();

                                for msg in &full_conversation.messages {
                                    let role = match msg.role.as_str() {
                                        "user" => crate::protocol::ChatRole::User,
                                        "assistant" => crate::protocol::ChatRole::Assistant,
                                        "system" => crate::protocol::ChatRole::System,
                                        "tool" => crate::protocol::ChatRole::Tool,
                                        _ => crate::protocol::ChatRole::User,
                                    };

                                    let mut chat_msg =
                                        crate::protocol::ChatMessage::new(role, msg.content.clone());

                                    if let Some(ref tc_val) = msg.tool_calls {
                                        if let Ok(tool_calls) =
                                            serde_json::from_value::<Vec<crate::protocol::ToolCall>>(
                                                tc_val.clone(),
                                            )
                                        {
                                            chat_msg.tool_calls = Some(tool_calls);
                                        }
                                    }

                                    chat_msg.tool_call_id = msg.tool_call_id.clone();

                                    conversation.push(chat_msg);
                                }

                                eprintln!("[History] Updated backend conversation state");
                            }

                            {
                                let mut mgr = state.chat_manager.lock().unwrap();
                                mgr.session_id = Some(full_conversation.session_id.clone());
                                eprintln!(
                                    "[History] Updated ChatManager session_id to {}",
                                    full_conversation.session_id
                                );
                            }

                            emit_blade_event(
                                &window,
                                Some(intent_id.to_string()),
                                blade_protocol::BladeEvent::History(
                                    blade_protocol::HistoryEvent::ConversationLoaded(full_conversation),
                                ),
                            );
                            Ok(())
                        }
                        Err(e) => Err(blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: e,
                        }),
                    }
                }
            }
        },
        BladeIntent::System(system_intent) => {
            println!("System Intent: {:?}", system_intent);
            Ok(())
        }
        BladeIntent::Language(language_intent) => match language_intent {
            blade_protocol::LanguageIntent::ZlpMessage { data } => {
                let is_local = state.is_local_model_active().await;
                if is_local {
                    return Err(blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: "ZLP messages are not supported when a local model is active".to_string(),
                    });
                }
                let window_clone = window.clone();
                let intent_id_clone = intent_id;
                let request_payload = data.clone();
                let ws_manager = state.ws_connection.clone();

                tauri::async_runtime::spawn(async move {
                    match ws_manager.request_zlp(request_payload).await {
                        Ok(val) => {
                            emit_blade_event(
                                &window_clone,
                                Some(intent_id_clone.to_string()),
                                blade_protocol::BladeEvent::Language(
                                    blade_protocol::LanguageEvent::ZlpResponse {
                                        original_request_id: intent_id_clone.to_string(),
                                        result: val,
                                    },
                                ),
                            );
                        }
                        Err(message) => {
                            emit_system_event(
                                &window_clone,
                                intent_id_clone,
                                blade_protocol::SystemEvent::IntentFailed {
                                    intent_id: intent_id_clone,
                                    error: blade_protocol::BladeError::Internal {
                                        trace_id: intent_id_clone.to_string(),
                                        message,
                                    },
                                },
                            );
                        }
                    }

                    emit_system_event(
                        &window_clone,
                        intent_id_clone,
                        SystemEvent::ProcessCompleted {
                            intent_id: intent_id_clone,
                        },
                    );
                });

                Ok(())
            }
            other => {
                eprintln!("[Language] Intent received: {:?}", other);
                let blocking_app_handle = app_handle.clone();
                let handler = tokio::task::spawn_blocking(move || {
                    let state = blocking_app_handle.state::<AppState>();
                    state.language_handler()
                })
                .await
                .map_err(|e| blade_protocol::BladeError::Internal {
                    trace_id: intent_id.to_string(),
                    message: format!("language handler task failed: {}", e),
                })?
                .map_err(|e| blade_protocol::BladeError::Internal {
                    trace_id: intent_id.to_string(),
                    message: e,
                })?;
                let maybe_event = handler
                    .handle(other, intent_id, Some(&state))
                    .await
                    .map_err(|e| blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: format!("{:?}", e),
                    })?;

                if let Some(event) = maybe_event {
                    blade_event_scheduler::emit_envelope(&window.app_handle(), event);
                }
                Ok(())
            }
        },
    }
}
