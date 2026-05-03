use crate::app_state::AppState;
use crate::blade_event_scheduler;
use crate::blade_protocol::{ChatMention, WorkspaceDiagnostic};
use crate::chat_manager::DrainResult;
use crate::post_edit_validation;
use crate::project_settings;
use crate::utils::{extract_root_command, is_cwd_outside_workspace, parse_command};
use crate::{blade_protocol, local_artifacts};
use std::collections::HashSet;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

fn extract_explicit_output_path(message: &str) -> Option<String> {
    message
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|c: char| {
                matches!(
                    c,
                    '`' | '"' | '\'' | ',' | '.' | ':' | ';' | ')' | '(' | '[' | ']'
                )
            })
        })
        .find(|token| token.contains('/') && token.contains('.'))
        .map(|token| token.to_string())
}

fn is_planning_artifact_path(path: &str) -> bool {
    let normalized = path.trim().trim_matches('/').to_ascii_lowercase();
    (normalized.starts_with("docs/")
        || normalized.ends_with(".md")
        || normalized.ends_with(".txt")
        || normalized.ends_with(".rst"))
        && !normalized.ends_with(".rs")
        && !normalized.ends_with(".ts")
        && !normalized.ends_with(".tsx")
        && !normalized.ends_with(".js")
        && !normalized.ends_with(".jsx")
        && !normalized.ends_with(".py")
        && !normalized.ends_with(".go")
        && !normalized.ends_with(".java")
        && !normalized.ends_with(".c")
        && !normalized.ends_with(".cpp")
        && !normalized.ends_with(".h")
        && !normalized.ends_with(".hpp")
}

fn should_allow_planning_artifact_write(mode: Option<&str>, message: &str) -> Option<String> {
    let is_planning = mode
        .map(|value| value.eq_ignore_ascii_case("planning"))
        .unwrap_or(false);
    if !is_planning {
        return None;
    }

    let lowercase = message.to_ascii_lowercase();
    let requests_write = ["write", "create", "save", "put"]
        .iter()
        .any(|verb| lowercase.contains(verb));
    let requests_plan_artifact = [
        "plan",
        "implementation plan",
        "roadmap",
        "spec",
        "specification",
        "design doc",
    ]
    .iter()
    .any(|keyword| lowercase.contains(keyword));
    if !requests_write || !requests_plan_artifact {
        return None;
    }

    let output_path = extract_explicit_output_path(message)?;
    if !is_planning_artifact_path(&output_path) {
        return None;
    }

    Some(output_path)
}

fn build_image_only_fallback_prompt(image_count: usize) -> String {
    let image_label = if image_count == 1 { "image" } else { "images" };
    format!(
        "[SYSTEM NOTE: The user attached {} {} without typing additional instructions. Analyze the attached {} in the context of the current workspace and help directly based on what you see. Do not treat this as an empty input and do not ask a generic \"How can I help?\" question. If intent is ambiguous, briefly describe the most relevant issue visible and propose the next concrete debugging step.]",
        image_count,
        image_label,
        image_label,
    )
}

fn ensure_non_empty_image_prompt(
    visible_user_message: &str,
    actual_message: String,
    image_count: usize,
) -> String {
    if image_count == 0 || !visible_user_message.trim().is_empty() {
        return actual_message;
    }

    let fallback = build_image_only_fallback_prompt(image_count);
    if actual_message.trim().is_empty() {
        return fallback;
    }

    format!("{}\n\n{}", actual_message, fallback)
}

fn format_workspace_diagnostics(diagnostics: Option<Vec<WorkspaceDiagnostic>>) -> Option<String> {
    let mut entries = diagnostics
        .unwrap_or_default()
        .into_iter()
        .filter(|diagnostic| {
            diagnostic.severity.eq_ignore_ascii_case("error")
                || diagnostic.severity.eq_ignore_ascii_case("warning")
        })
        .take(12)
        .collect::<Vec<_>>();

    if entries.is_empty() {
        return None;
    }

    entries.sort_by_key(|diagnostic| {
        if diagnostic.severity.eq_ignore_ascii_case("error") {
            0
        } else {
            1
        }
    });

    let mut output = String::from(
        "Active workspace diagnostics are present. Treat these as immediate feedback and fix relevant errors before continuing:\n",
    );
    for diagnostic in entries {
        let severity = diagnostic.severity.to_ascii_uppercase();
        let location = match (&diagnostic.file, diagnostic.line, diagnostic.column) {
            (Some(file), Some(line), Some(column)) => format!("{}:{}:{}", file, line, column),
            (Some(file), Some(line), None) => format!("{}:{}", file, line),
            (Some(file), None, _) => file.clone(),
            _ => "workspace".to_string(),
        };
        output.push_str(&format!(
            "- {} [{}] {}: {}\n",
            severity, diagnostic.source, location, diagnostic.message
        ));
        if let Some(raw) = diagnostic.raw {
            let trimmed = raw.trim();
            if !trimmed.is_empty() && trimmed != diagnostic.message {
                let preview: String = trimmed.chars().take(1200).collect();
                output.push_str("  Raw diagnostic:\n");
                for line in preview.lines().take(12) {
                    output.push_str("    ");
                    output.push_str(line);
                    output.push('\n');
                }
            }
        }
    }

    Some(output)
}

fn infer_context_files_from_query(
    state: &State<'_, AppState>,
    query: &str,
    limit: usize,
) -> Vec<String> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();

    let service = match state.language_service() {
        Ok(service) => service,
        Err(error) => {
            eprintln!(
                "[LANGUAGE] Failed to initialize language service for query context: {}",
                error
            );
            return files;
        }
    };

    let active_file = state.active_file.lock().unwrap().clone();
    let open_files = state.open_files.lock().unwrap().clone();

    if let Ok(results) = service.search_symbols_contextual(
        query,
        limit.saturating_mul(4),
        active_file.as_deref(),
        &open_files,
    ) {
        for result in results {
            let path = result.symbol.file_path;
            if seen.insert(path.clone()) {
                files.push(path);
                if files.len() >= limit {
                    return files;
                }
            }
        }
    }
    let tokens: Vec<&str> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|token| token.len() >= 4)
        .take(8)
        .collect();

    for token in tokens {
        if let Ok(results) =
            service.search_symbols_contextual(token, 4, active_file.as_deref(), &open_files)
        {
            for result in results {
                let path = result.symbol.file_path;
                if seen.insert(path.clone()) {
                    files.push(path);
                    if files.len() >= limit {
                        return files;
                    }
                }
            }
        }
    }

    files
}

async fn load_available_models(
    state: &State<'_, AppState>,
) -> Result<Vec<crate::models::registry::ModelInfo>, String> {
    let config = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?
        .clone();
    Ok(crate::models::catalog::list_all_models(&config).await)
}

pub async fn handle_send_message<R: Runtime>(
    message: String,
    images: Option<Vec<crate::protocol::ChatImage>>,
    model_id: Option<String>,
    active_file: Option<String>,
    open_files: Option<Vec<String>>,
    cursor_line: Option<usize>,
    cursor_column: Option<usize>,
    selection_start_line: Option<usize>,
    selection_end_line: Option<usize>,
    diagnostics: Option<Vec<WorkspaceDiagnostic>>,
    mentions: Option<Vec<ChatMention>>,
    mode: Option<String>,
    window: tauri::Window<R>,
    state: State<'_, AppState>,
    app: AppHandle<R>,
) -> Result<(), String> {
    println!("Received message from frontend: {}", message);

    let workspace_root = {
        let workspace = state
            .workspace
            .lock()
            .map_err(|e| format!("Failed to lock workspace: {}", e))?;
        workspace.workspace.clone()
    };

    let normalize_to_workspace = |path: String| -> String {
        if std::path::Path::new(&path).is_absolute() {
            path
        } else if let Some(root) = workspace_root.as_ref() {
            root.join(path).to_string_lossy().to_string()
        } else {
            path
        }
    };
    let normalized_mentions = mentions
        .unwrap_or_default()
        .into_iter()
        .filter(|mention| mention.kind == "path")
        .map(|mention| ChatMention {
            kind: mention.kind,
            path: normalize_to_workspace(mention.path),
            is_dir: mention.is_dir,
        })
        .collect::<Vec<_>>();

    // Parse @commands and convert to tool calls
    let (parsed_message, forced_tool) = parse_command(&message);
    let visible_user_message = parsed_message.clone();

    // Check for pending error feedback from previous turn (e.g. message too large)
    // Prepend it as a system note so the model knows what happened
    let mut actual_message: String = {
        let mut feedback = state
            .pending_error_feedback
            .lock()
            .map_err(|e| format!("Failed to lock pending_error_feedback: {}", e))?;
        if let Some(hint) = feedback.take() {
            eprintln!("[SEND MSG] Prepending error feedback to message: {}", hint);
            format!("[SYSTEM NOTE: {}]\n\n{}", hint, parsed_message)
        } else {
            parsed_message
        }
    };
    if !normalized_mentions.is_empty() {
        let mention_preview = normalized_mentions
            .iter()
            .map(|mention| {
                if mention.is_dir {
                    format!("{} (dir)", mention.path)
                } else {
                    mention.path.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        actual_message = format!(
            "[SYSTEM NOTE: The user explicitly referenced these workspace paths: {}]\n\n{}",
            mention_preview, actual_message
        );
    }
    if let Some(diagnostic_feedback) = format_workspace_diagnostics(diagnostics) {
        actual_message = format!(
            "[SYSTEM NOTE: {}]\n\n{}",
            diagnostic_feedback, actual_message
        );
    }

    let effective_mode = if let Some(output_path) =
        should_allow_planning_artifact_write(mode.as_deref(), actual_message.as_str())
    {
        actual_message = format!(
            "[SYSTEM NOTE: The user explicitly requested a planning artifact to be written to {}. You may create or update that planning/documentation file while staying focused on planning. Avoid unrelated source-code edits unless the user also asked for implementation.]\n\n{}",
            output_path,
            actual_message
        );
        None
    } else {
        mode.clone()
    };

    // If there is no editor context, infer likely relevant files from indexed symbols.
    // This prevents the model from defaulting to generic project summaries.
    let mut effective_open_files = open_files
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|path| normalize_to_workspace(path))
        .collect::<Vec<_>>();
    for mention in normalized_mentions.iter().filter(|mention| !mention.is_dir) {
        if !effective_open_files
            .iter()
            .any(|path| path == &mention.path)
        {
            effective_open_files.insert(0, mention.path.clone());
        }
    }
    let mut effective_active_file = active_file.clone().map(|path| normalize_to_workspace(path));
    if effective_active_file.is_none() {
        effective_active_file = normalized_mentions
            .iter()
            .find(|mention| !mention.is_dir)
            .map(|mention| mention.path.clone());
    }

    if active_file.is_none() && effective_open_files.is_empty() {
        let blocking_app = app.clone();
        let blocking_query = actual_message.clone();
        effective_open_files = tokio::task::spawn_blocking(move || {
            let state = blocking_app.state::<AppState>();
            infer_context_files_from_query(&state, blocking_query.as_str(), 6)
        })
        .await
        .unwrap_or_else(|error| {
            eprintln!(
                "[LANGUAGE] Context inference task failed for chat query fallback: {}",
                error
            );
            Vec::new()
        })
        .into_iter()
        .map(|path| normalize_to_workspace(path))
        .collect();
    }

    if effective_active_file.is_none() {
        effective_active_file = effective_open_files.first().cloned();
    }

    if let Some(active) = effective_active_file.clone() {
        if !effective_open_files.iter().any(|path| path == &active) {
            effective_open_files.insert(0, active);
        }
    }

    let mut deduped_open_files = Vec::new();
    let mut seen_files = HashSet::new();
    for path in effective_open_files {
        if seen_files.insert(path.clone()) {
            deduped_open_files.push(path);
        }
    }
    effective_open_files = deduped_open_files;

    let active_in_open = effective_active_file
        .as_ref()
        .map(|active| effective_open_files.iter().any(|path| path == active))
        .unwrap_or(false);
    let open_preview: Vec<String> = effective_open_files
        .iter()
        .take(5)
        .map(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(path)
                .to_string()
        })
        .collect();
    let open_preview_suffix = if effective_open_files.len() > open_preview.len() {
        " ..."
    } else {
        ""
    };

    eprintln!(
        "[SEND MSG] active_file={:?}, open_files_count={}, active_in_open={}, open_files_preview=[{}{}], cursor_line={:?}, cursor_column={:?}",
        effective_active_file,
        effective_open_files.len(),
        active_in_open,
        open_preview.join(", "),
        open_preview_suffix,
        cursor_line,
        cursor_column
    );

    // Store editor state in AppState for tool execution
    {
        *state
            .active_file
            .lock()
            .map_err(|e| format!("Failed to lock active_file: {}", e))? =
            effective_active_file.clone();
        *state
            .open_files
            .lock()
            .map_err(|e| format!("Failed to lock open_files: {}", e))? =
            effective_open_files.clone();
        *state
            .cursor_line
            .lock()
            .map_err(|e| format!("Failed to lock cursor_line: {}", e))? = cursor_line;
        *state
            .cursor_column
            .lock()
            .map_err(|e| format!("Failed to lock cursor_column: {}", e))? = cursor_column;
        *state
            .selection_start_line
            .lock()
            .map_err(|e| format!("Failed to lock selection_start_line: {}", e))? =
            selection_start_line;
        *state
            .selection_end_line
            .lock()
            .map_err(|e| format!("Failed to lock selection_end_line: {}", e))? = selection_end_line;
    }

    // 1. Add User Message
    {
        let mut conversation = state
            .conversation
            .lock()
            .map_err(|e| format!("Failed to lock conversation: {}", e))?;
        let mut chat_msg = crate::protocol::ChatMessage::new(
            crate::protocol::ChatRole::User,
            visible_user_message.clone(),
        );
        chat_msg.backend_content = Some(actual_message.clone());
        chat_msg.images = images.clone();
        if !normalized_mentions.is_empty() {
            chat_msg.mentions = Some(
                normalized_mentions
                    .iter()
                    .map(|mention| crate::protocol::ChatMention {
                        kind: mention.kind.clone(),
                        path: mention.path.clone(),
                        is_dir: mention.is_dir,
                    })
                    .collect(),
            );
        }
        conversation.push(chat_msg);
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

    let image_count = images.as_ref().map(|imgs| imgs.len()).unwrap_or(0);
    actual_message =
        ensure_non_empty_image_prompt(visible_user_message.as_str(), actual_message, image_count);
    if image_count > 0 {
        let preview: String = actual_message.chars().take(160).collect();
        eprintln!(
            "[IMAGE SEND] visible_len={}, actual_len={}, image_count={}, actual_blank={}, preview={:?}",
            visible_user_message.len(),
            actual_message.len(),
            image_count,
            actual_message.trim().is_empty(),
            preview,
        );
    }

    // 2. Start Stream
    let models = load_available_models(&state).await?;
    let workspace_path = {
        let workspace = state
            .workspace
            .lock()
            .map_err(|e| format!("Failed to lock workspace: {}", e))?;
        workspace.workspace.clone()
    };
    let storage_mode = Some(
        tokio::task::spawn_blocking({
            let workspace_path = workspace_path.clone();
            move || {
                workspace_path
                    .as_ref()
                    .map(|p| {
                        let settings = project_settings::load_project_settings_or_default(p);
                        match settings.storage.mode {
                            project_settings::StorageMode::Local => "local".to_string(),
                            project_settings::StorageMode::Server => "server".to_string(),
                        }
                    })
                    .unwrap_or_else(|| "local".to_string())
            }
        })
        .await
        .unwrap_or_else(|error| {
            eprintln!("[CHAT] storage mode task failed: {}", error);
            "local".to_string()
        }),
    );
    {
        let mut mgr = state
            .chat_manager
            .lock()
            .map_err(|e| format!("Failed to lock chat_manager: {}", e))?;
        let mut conversation = state
            .conversation
            .lock()
            .map_err(|e| format!("Failed to lock conversation: {}", e))?;
        let config = state
            .config
            .lock()
            .map_err(|e| format!("Failed to lock config: {}", e))?;

        // Default to the currently selected model index from state, rather than 0
        let mut selected_model = *state
            .selected_model_index
            .lock()
            .map_err(|e| format!("Failed to lock selected_model_index: {}", e))?;

        if let Some(ref id) = model_id {
            let matched_idx = crate::models::catalog::resolve_model_index(&models, id);

            if let Some(idx) = matched_idx {
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
        *state
            .selected_model_index
            .lock()
            .map_err(|e| format!("Failed to lock selected_model_index: {}", e))? = selected_model;

        // We use reqwest Client
        let http = reqwest::Client::new();

        // Ensure workspace root is valid
        let ws = workspace_path.as_ref();

        let composite_tools_enabled = state.feature_flags.composite_tools_enabled();

        mgr.start_stream(
            actual_message,
            &mut conversation,
            &config,
            &models,
            selected_model,
            ws,
            effective_active_file.clone(),
            Some(effective_open_files.clone()),
            cursor_line,
            cursor_column,
            http,
            storage_mode,
            effective_mode,
            composite_tools_enabled,
        )
        .map_err(|e| e.to_string())?;

        blade_event_scheduler::reset_chat_stream();
    }

    // 3. Event-Driven Processing (Background Task)
    // Only processes events when there's actual streaming activity
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let mut last_session_id: Option<String> = None;
        let event_notifier = {
            let state = app_handle.state::<AppState>();
            let mgr = state.chat_manager.lock().unwrap();
            mgr.event_notifier()
        };

        loop {
            // Register for the *next* provider event before draining so we don't miss
            // notifications that arrive between processing and waiting.
            let next_event = event_notifier.notified();
            let state = app_handle.state::<AppState>();

            let (
                result,
                is_streaming,
                session_id,
                has_rx,
                has_pending,
                has_pending_done_without_tools,
                finish_reason,
            ) = {
                let mut mgr = state.chat_manager.lock().unwrap();
                let mut conversation = state.conversation.lock().unwrap();
                let res = mgr.drain_events(&mut conversation);
                (
                    res,
                    mgr.streaming,
                    mgr.session_id.clone(),
                    mgr.rx.is_some(),
                    !mgr.pending_results.is_empty(),
                    mgr.has_pending_done_without_tools(),
                    mgr.last_finish_reason.clone(),
                )
            };

            // If the backend session_id changes, reset loop detection history.
            // Otherwise, old tool-call history can cause false-positive loop detection
            // (e.g., read_file blocked immediately in a fresh session).
            if session_id != last_session_id {
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
                if !is_streaming && !has_rx {
                    // Auto-save conversation before emitting done
                    {
                        let mut stored = {
                            let conversation = state.conversation.lock().unwrap();
                            conversation.to_stored()
                        };
                        // Persist the current session ID to the stored metadata
                        stored.metadata.session_id = session_id.clone();
                        let workspace_path = {
                            let workspace = state.workspace.lock().unwrap();
                            workspace.workspace.clone()
                        };
                        let blocking_app_handle = app_handle.clone();
                        let autosave_result = tokio::task::spawn_blocking(move || {
                            let state = blocking_app_handle.state::<AppState>();

                            if let Err(e) =
                                state.with_conversation_store(|store| store.save_conversation(&stored))
                            {
                                eprintln!("Failed to auto-save conversation: {}", e);
                            } else {
                                println!("Auto-saved conversation: {}", stored.metadata.id);
                            }

                            // RFC-002: Also save to local artifacts if in local storage mode
                            if let Some(ws_path) = workspace_path {
                                let settings =
                                    project_settings::load_project_settings_or_default(&ws_path);
                                if settings.storage.mode == project_settings::StorageMode::Local {
                                    // Convert to local artifact format
                                    let project_id = crate::project::get_or_create_project_id(&ws_path)
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
                                        local_artifacts::LocalArtifactStore::new(&ws_path);
                                    if let Err(e) = artifact_store.save_conversation(&artifact) {
                                        eprintln!("[LOCAL] Failed to save local artifact: {}", e);
                                    } else {
                                        eprintln!("[LOCAL] Saved conversation to .zblade/artifacts/conversations/{}.json", stored.metadata.id);
                                    }
                                }
                            }
                        })
                        .await;

                        if let Err(e) = autosave_result {
                            eprintln!("Failed to run auto-save conversation task: {}", e);
                        }
                    }
                    window
                        .emit(
                            crate::events::event_names::CHAT_DONE,
                            crate::events::ChatDonePayload {
                                finish_reason: finish_reason.unwrap_or_else(|| "stop".to_string()),
                            },
                        )
                        .unwrap_or_default();
                    break;
                }

                if !has_pending && !has_pending_done_without_tools {
                    next_event.await;
                }
                continue;
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
            } else if let DrainResult::Update(msg_id, chunk) = result {
                let msg_id = if msg_id.is_empty() {
                    "streaming-msg".to_string()
                } else {
                    msg_id
                };

                blade_event_scheduler::queue_chat_message_delta(
                    &app_handle,
                    msg_id.clone(),
                    Some(msg_id),
                    chunk,
                );
            } else if let DrainResult::Reasoning(msg_id, chunk) = result {
                let msg_id = if msg_id.is_empty() {
                    "streaming-msg".to_string()
                } else {
                    msg_id
                };

                blade_event_scheduler::queue_chat_reasoning_delta(
                    &app_handle,
                    msg_id.clone(),
                    Some(msg_id),
                    chunk,
                );
            } else if let DrainResult::Error(e) = result {
                window
                    .emit(
                        "chat-error",
                        crate::events::ChatErrorPayload {
                            code: "request_failed".to_string(),
                            error: Some("Request failed".to_string()),
                            message: Some(e.clone()),
                            detail: Some(e),
                            i18n_key: Some("chat.system.requestFailed".to_string()),
                            i18n_params: None,
                        },
                    )
                    .unwrap_or_default();
                break;
            } else if let DrainResult::ToolCreated(msg, new_calls) = result {
                let msg_id = msg.id.clone().unwrap_or_else(|| "unknown".to_string());
                for tc in new_calls {
                    blade_event_scheduler::queue_tool_update(
                        &app_handle,
                        Some(msg_id.clone()),
                        msg_id.clone(),
                        tc.id.clone(),
                        "executing".to_string(),
                        None,
                        Some(tc.clone()),
                    );
                }
            } else if let DrainResult::ToolActivity {
                tool_name,
                file_path,
                action,
                tool_call_id,
            } = result
            {
                blade_event_scheduler::queue_tool_activity(
                    &app_handle,
                    tool_name,
                    file_path,
                    action,
                    tool_call_id,
                );
            } else if let DrainResult::ApprovalRequest(payload) = result {
                window
                    .emit(
                        crate::events::event_names::APPROVAL_REQUEST,
                        crate::events::ApprovalRequestPayload {
                            session_id: payload.session_id,
                            approval_id: payload.approval_id,
                            tool_call_id: payload.tool_call_id,
                            tool_name: payload.tool_name,
                            arguments: payload.arguments,
                            source: payload.source,
                            rule_name: payload.rule_name,
                            message: payload.message,
                            decision: payload.decision,
                        },
                    )
                    .unwrap_or_default();
            } else if let DrainResult::ToolStatusUpdate(msg) = result {
                // v1.1: Emit ToolUpdate events via blade-event
                if let Some(tool_calls) = &msg.tool_calls {
                    let msg_id = msg.id.clone().unwrap_or_else(|| "unknown".to_string());
                    for tc in tool_calls {
                        // Emit update for each tool call
                        // We emit indiscriminately here because the frontend will merge/update state based on ID
                        let status = tc.status.clone().unwrap_or_else(|| "unknown".to_string());

                        blade_event_scheduler::queue_tool_update(
                            &app_handle,
                            Some(msg_id.clone()),
                            msg_id.clone(),
                            tc.id.clone(),
                            status,
                            tc.result.clone(),
                            Some(tc.clone()),
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
                blade_event_scheduler::emit_blade_event(
                    &app_handle,
                    None,
                    blade_protocol::BladeEvent::Chat(blade_protocol::ChatEvent::MessageCompleted {
                        id,
                    }),
                );
            } else if let DrainResult::ContextLengthExceeded {
                message,
                token_count,
                max_tokens,
                excess,
                recoverable,
                recovery_hint,
            } = result
            {
                // RFC: Context Length Recovery - emit context-length-exceeded event to frontend
                eprintln!(
                    "[LIB] Context length exceeded: {} (tokens: {:?}/{:?})",
                    message, token_count, max_tokens
                );
                let _ = window.emit(
                    "context-length-exceeded",
                    crate::events::ContextLengthExceededPayload {
                        message,
                        token_count,
                        max_tokens,
                        excess,
                        recoverable,
                        recovery_hint,
                        title_key: Some("chat.system.contextLimitReachedTitle".to_string()),
                        recoverable_hint_key: Some(
                            "chat.system.contextLimitRecoverable".to_string(),
                        ),
                        non_recoverable_hint_key: Some(
                            "chat.system.contextLimitRestart".to_string(),
                        ),
                    },
                );
            } else if let DrainResult::MessageTooLarge {
                message,
                recovery_hint,
            } = result
            {
                // Message size limit exceeded - emit as chat-error (reliable) + store recovery hint
                eprintln!(
                    "[LIB] Message too large: {} (hint: {})",
                    message, recovery_hint
                );
                window
                    .emit(
                        "chat-error",
                        crate::events::ChatErrorPayload {
                            code: "response_too_large".to_string(),
                            error: Some("Response too large".to_string()),
                            message: Some(message.clone()),
                            detail: Some(recovery_hint.clone()),
                            i18n_key: Some("chat.system.responseTooLargeTitle".to_string()),
                            i18n_params: None,
                        },
                    )
                    .unwrap_or_default();
                window
                    .emit(
                        "message-too-large",
                        crate::events::MessageTooLargePayload {
                            message,
                            recovery_hint: recovery_hint.clone(),
                            title_key: Some("chat.system.responseTooLargeTitle".to_string()),
                            recovery_hint_label_key: Some(
                                "chat.system.recoveryHintLabel".to_string(),
                            ),
                        },
                    )
                    .unwrap_or_default();
                // Store recovery hint so it gets prepended to the next user message
                let state = app_handle.state::<AppState>();
                *state.pending_error_feedback.lock().unwrap() = Some(recovery_hint);
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

                let blocking_app_handle = app_handle.clone();
                let blocking_ws_root = ws_root.clone();
                let batch_opt = tokio::task::spawn_blocking(move || {
                    let state = blocking_app_handle.state::<AppState>();
                    let mut workflow = state.workflow.lock().unwrap();
                    workflow.handle_tool_calls(
                        blocking_ws_root
                            .as_ref()
                            .map(|s| std::path::Path::new(s))
                            .unwrap_or_else(|| std::path::Path::new(".")),
                        calls,
                        content,
                        &context,
                    )
                })
                .await
                .unwrap_or_else(|e| {
                    eprintln!("[ORCHESTRATOR] handle_tool_calls task failed: {}", e);
                    None
                });

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
                                    action_kind: crate::events::StructuredActionKind::Command,
                                    description_key: Some(
                                        "approval.runCommandDescription".to_string(),
                                    ),
                                    description_params: Some(std::collections::BTreeMap::from([(
                                        "command".to_string(),
                                        cmd.command.clone(),
                                    )])),
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
                                    action_kind: crate::events::StructuredActionKind::GenericTool,
                                    description_key: None,
                                    description_params: None,
                                    cwd: None,
                                    root_command: None,
                                    cwd_outside_workspace: None,
                                    is_generic_tool: true,
                                });
                            }
                            if !actions.is_empty() {
                                eprintln!(
                                    "[ORCHESTRATOR] Emitting request-confirmation with {} actions",
                                    actions.len()
                                );
                                for action in &actions {
                                    eprintln!(
                                        "[ORCHESTRATOR]   - action: {} (id={})",
                                        action.command, action.id
                                    );
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

                if let Some(mut batch) = batch_to_run {
                    post_edit_validation::augment_batch_with_validation_feedback(
                        &app_handle,
                        &mut batch,
                    )
                    .await;

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
                                | "write_file_validated"
                                | "create_file"
                                | "replace_file_content"
                                | "apply_edit"
                                | "apply_patch"
                                | "apply_patch_validated"
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
                                        eprintln!(
                                            "[AUTO OPEN] Opening file in editor: {}",
                                            abs_path
                                        );
                                        window.emit("open-file", &abs_path).unwrap_or_default();
                                    }
                                }
                            }
                        }
                    }

                    // Check if user requested stop — if so, don't start a new stream.
                    // After request_stop(): streaming=false AND rx=None.
                    // During normal tool execution: streaming=false but rx is still Some.
                    let user_stopped = {
                        let mgr = state.chat_manager.lock().unwrap();
                        !mgr.streaming && mgr.rx.is_none()
                    };
                    if user_stopped {
                        eprintln!(
                            "[ORCHESTRATOR] User requested stop — skipping continue_tool_batch"
                        );
                        // Fall through to let the loop break naturally on next iteration
                    } else if batch.loop_detected {
                        // Check if loop was detected - if so, stop the agentic loop
                        eprintln!("[AGENTIC LOOP] Stopping due to loop detection");

                        let models = match load_available_models(&state).await {
                            Ok(models) => models,
                            Err(e) => {
                                eprintln!("[ORCHESTRATOR] Failed to load models: {}", e);
                                Vec::new()
                            }
                        };

                        {
                            let workspace_path = {
                                let ws = state.workspace.lock().unwrap();
                                ws.workspace.clone()
                            };
                            let is_local_mode = tokio::task::spawn_blocking({
                                let workspace_path = workspace_path.clone();
                                move || {
                                    workspace_path
                                        .as_ref()
                                        .map(|ws| {
                                            let settings =
                                                project_settings::load_project_settings_or_default(
                                                    ws,
                                                );
                                            matches!(
                                                settings.storage.mode,
                                                project_settings::StorageMode::Local
                                            )
                                        })
                                        .unwrap_or(true)
                                }
                            })
                            .await
                            .unwrap_or_else(|error| {
                                eprintln!("[CHAT] continue batch mode task failed: {}", error);
                                true
                            });

                            let mut mgr = state.chat_manager.lock().unwrap();
                            mgr.agentic_loop.stop("loop detected");
                            // Still send the tool results back to the model so it can respond
                            let mut conversation = state.conversation.lock().unwrap();
                            let config = state.config.lock().unwrap();
                            let selected_model_idx = *state.selected_model_index.lock().unwrap();
                            let http = reqwest::Client::new();

                            mgr.continue_tool_batch(
                                batch,
                                &mut conversation,
                                &config,
                                &models,
                                selected_model_idx,
                                workspace_path.as_ref(),
                                is_local_mode,
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
                        let models = match load_available_models(&state).await {
                            Ok(models) => models,
                            Err(e) => {
                                eprintln!("[ORCHESTRATOR] Failed to load models: {}", e);
                                Vec::new()
                            }
                        };

                        {
                            let workspace_path = {
                                let ws = state.workspace.lock().unwrap();
                                ws.workspace.clone()
                            };
                            let is_local_mode = tokio::task::spawn_blocking({
                                let workspace_path = workspace_path.clone();
                                move || {
                                    workspace_path
                                        .as_ref()
                                        .map(|ws| {
                                            let settings =
                                                project_settings::load_project_settings_or_default(
                                                    ws,
                                                );
                                            matches!(
                                                settings.storage.mode,
                                                project_settings::StorageMode::Local
                                            )
                                        })
                                        .unwrap_or(true)
                                }
                            })
                            .await
                            .unwrap_or_else(|error| {
                                eprintln!("[CHAT] continue batch mode task failed: {}", error);
                                true
                            });

                            let mut mgr = state.chat_manager.lock().unwrap();
                            let mut conversation = state.conversation.lock().unwrap();
                            let config = state.config.lock().unwrap();
                            let selected_model_idx = *state.selected_model_index.lock().unwrap();
                            let http = reqwest::Client::new();

                            mgr.continue_tool_batch(
                                batch,
                                &mut conversation,
                                &config,
                                &models,
                                selected_model_idx,
                                workspace_path.as_ref(),
                                is_local_mode,
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

#[cfg(test)]
mod tests {
    use super::ensure_non_empty_image_prompt;

    #[test]
    fn image_only_turns_get_a_fallback_prompt() {
        let result = ensure_non_empty_image_prompt("", String::new(), 1);
        assert!(result.contains("attached 1 image"));
        assert!(result.contains("Do not treat this as an empty input"));
    }

    #[test]
    fn text_turns_do_not_get_image_only_fallback() {
        let result = ensure_non_empty_image_prompt(
            "Please inspect this",
            "Please inspect this".to_string(),
            1,
        );
        assert_eq!(result, "Please inspect this");
    }
}
