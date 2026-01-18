pub mod agentic_loop;
pub mod ai_workflow;
pub mod blade_client;
pub mod blade_protocol; // [NEW] Blade Protocol v1.0
pub mod blade_ws_client;
pub mod chat;
pub mod chat_manager;
pub mod config;
pub mod conversation;
pub mod conversation_store;
pub mod ephemeral_commands;
pub mod ephemeral_documents;
pub mod events;
pub mod explorer;
pub mod idempotency; // [NEW] v1.1: Idempotency cache
pub mod local_artifacts; // [NEW] RFC-002: Local conversation artifact storage
pub mod local_index; // [NEW] RFC-002: Local SQLite index for conversations
pub mod models;
pub mod project;
pub mod project_settings;
pub mod project_state;
pub mod protocol;
pub mod reasoning_parser; // [NEW] v1.2: Multi-format reasoning extraction
pub mod terminal;
pub mod tool_execution;
pub mod tools;
pub mod tree_sitter; // [NEW] Tree-sitter parsing for LSP integration
pub mod warmup; // [NEW] v2.1: Cache warmup
pub mod workspace_manager;
pub mod xml_parser;

use crate::chat_manager::{ChatManager, DrainResult};
use clap::Parser;
use notify::event::ModifyKind;
use notify::EventKind;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, Runtime, State};

/// ZaguanBlade - AI-Native Intelligent Code Editor
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Optional path to open as workspace root
    #[arg(value_name = "PATH")]
    pub path: Option<String>,
}

/// Parse @command syntax and extract tool name and query
/// Returns (actual_message, Option<(tool_name, query)>)
fn parse_command(message: &str) -> (String, Option<(String, String)>) {
    let trimmed = message.trim();

    // Check if message starts with @command
    if trimmed.starts_with("@research ") {
        let query = trimmed.strip_prefix("@research ").unwrap().to_string();
        return (message.to_string(), Some(("research".to_string(), query)));
    } else if trimmed.starts_with("@search ") {
        let query = trimmed.strip_prefix("@search ").unwrap().to_string();
        return (message.to_string(), Some(("search".to_string(), query)));
    } else if trimmed.starts_with("@web ") {
        let query = trimmed.strip_prefix("@web ").unwrap().to_string();
        return (message.to_string(), Some(("fetch_url".to_string(), query)));
    }

    // No command found, return original message
    (message.to_string(), None)
}

fn extract_root_command(command: &str) -> Option<String> {
    let first_segment = command
        .split(|c| c == '|' || c == ';')
        .next()
        .unwrap_or(command);
    let first_segment = first_segment.split("&&").next().unwrap_or(first_segment);
    let first_segment = first_segment.split("||").next().unwrap_or(first_segment);

    let mut it = first_segment.split_whitespace().peekable();
    while let Some(tok) = it.peek().copied() {
        if tok == "sudo" || tok == "env" || tok == "command" || tok == "time" {
            it.next();
            continue;
        }
        if tok.contains('=') && !tok.starts_with("./") && !tok.contains('/') {
            it.next();
            continue;
        }
        break;
    }
    it.next().map(|s| s.to_string())
}

fn is_cwd_outside_workspace(ws_root: Option<&str>, cwd: Option<&str>) -> Option<bool> {
    let ws_root = ws_root?;
    let cwd = cwd?;
    let ws = std::fs::canonicalize(std::path::Path::new(ws_root)).ok()?;
    let p = std::path::Path::new(cwd);
    let candidate = if p.is_absolute() {
        p.to_path_buf()
    } else {
        ws.join(p)
    };
    let candidate = std::fs::canonicalize(&candidate).ok()?;
    Some(!candidate.starts_with(&ws))
}
use crate::ai_workflow::AiWorkflow;
use crate::config::ApiConfig;
use crate::conversation::ConversationHistory;
use crate::models::registry::get_models;
use crate::workspace_manager::WorkspaceManager;
use dotenvy::dotenv;
use futures::future::join_all;

pub struct AppState {
    pub chat_manager: Mutex<ChatManager>,
    pub conversation: Mutex<ConversationHistory>,
    pub conversation_store: Mutex<conversation_store::ConversationStore>,
    pub workspace: Mutex<WorkspaceManager>,
    pub config: Mutex<ApiConfig>,
    pub workflow: Mutex<AiWorkflow>,
    pub pending_approval: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,
    pub pending_changes: Mutex<Vec<crate::ai_workflow::PendingChange>>,
    pub pending_batch: Mutex<Option<crate::ai_workflow::PendingToolBatch>>,
    pub selected_model_index: Mutex<usize>,
    pub ephemeral_docs: ephemeral_documents::EphemeralDocumentStore,
    pub active_file: Mutex<Option<String>>,
    pub open_files: Mutex<Vec<String>>,
    pub cursor_line: Mutex<Option<usize>>,
    pub cursor_column: Mutex<Option<usize>>,
    pub selection_start_line: Mutex<Option<usize>>,
    pub selection_end_line: Mutex<Option<usize>>,
    pub virtual_buffers: Mutex<std::collections::HashMap<String, String>>, // path -> virtual content
    pub approved_command_roots: Mutex<std::collections::HashSet<String>>,
    pub executing_commands: std::sync::Arc<
        Mutex<std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    >,
    pub idempotency_cache: crate::idempotency::IdempotencyCache, // v1.1: Idempotency support
    pub warmup_client: warmup::WarmupClient,                     // v2.1: Cache warmup
    pub user_id: Mutex<Option<String>>, // Authenticated user ID from WebSocket
    pub fs_watcher: Mutex<Option<RecommendedWatcher>>, // Workspace file watcher
}

impl AppState {
    pub fn new(initial_path: Option<String>) -> Self {
        // Load environment variables from .env file
        dotenv().ok();

        // Load config from disk
        let config_path = config::default_api_config_path();
        let mut config = config::load_api_config(&config_path);

        // Fallback or override logic:
        // If config.blade_url is empty, use default or check environment variable.
        if config.blade_url.trim().is_empty() {
            if let Ok(url) = std::env::var("BLADE_URL") {
                config.blade_url = url;
            } else {
                config.blade_url = "http://10.0.0.1:8880".to_string();
            }
        }

        // Load API key from environment if not in config
        if config.api_key.trim().is_empty() {
            if let Ok(key) = std::env::var("ZAGUAN_API_KEY") {
                config.api_key = key;
            }
        }

        // Initialize selected model index from config
        // We can't fetch models synchronously here, so we default to 0
        // The actual index will be corrected when models are fetched or when set_selected_model is called
        let initial_model_index = 0;

        // Initialize conversation store
        let storage_path = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("zaguan")
            .join("conversations");

        let conversation_store = conversation_store::ConversationStore::new(storage_path)
            .unwrap_or_else(|e| {
                eprintln!("Failed to initialize conversation store: {}", e);
                // Fallback to temp directory
                conversation_store::ConversationStore::new(
                    std::env::temp_dir().join("zaguan_conversations"),
                )
                .expect("Failed to create conversation store in temp directory")
            });

        let mut workspace_manager = WorkspaceManager::new();
        // Override workspace if provided via CLI
        if let Some(path_str) = initial_path {
            workspace_manager.set_workspace(std::path::PathBuf::from(path_str));
        }

        // Get or create user_id
        let user_id = config::get_or_create_user_id(&config_path);

        // Initialize warmup client with config values
        let warmup_client = warmup::WarmupClient::new(
            config.blade_url.clone(),
            config.api_key.clone(),
            user_id.clone(),
        );

        Self {
            chat_manager: Mutex::new(ChatManager::new(10)),
            conversation: Mutex::new(ConversationHistory::new()),
            conversation_store: Mutex::new(conversation_store),
            workspace: Mutex::new(workspace_manager),
            config: Mutex::new(config),
            workflow: Mutex::new(AiWorkflow::new()),
            pending_approval: Mutex::new(None),
            pending_changes: Mutex::new(Vec::new()),
            pending_batch: Mutex::new(None),
            selected_model_index: Mutex::new(initial_model_index),
            ephemeral_docs: ephemeral_documents::EphemeralDocumentStore::new(),
            active_file: Mutex::new(None),
            open_files: Mutex::new(Vec::new()),
            cursor_line: Mutex::new(None),
            cursor_column: Mutex::new(None),
            user_id: Mutex::new(Some(user_id)),
            selection_start_line: Mutex::new(None),
            selection_end_line: Mutex::new(None),
            virtual_buffers: Mutex::new(std::collections::HashMap::new()),
            approved_command_roots: Mutex::new(std::collections::HashSet::new()),
            executing_commands: std::sync::Arc::new(Mutex::new(std::collections::HashMap::new())),
            idempotency_cache: crate::idempotency::IdempotencyCache::default(), // 24h TTL
            warmup_client, // v2.1: Cache warmup
            fs_watcher: Mutex::new(None),
        }
    }
}

#[derive(Clone, serde::Serialize)]
struct FileChangeEvent {
    paths: Vec<String>,
    count: usize,
}

fn restart_fs_watcher<R: Runtime>(app_handle: &tauri::AppHandle<R>, state: &State<'_, AppState>) {
    let workspace_root = { state.workspace.lock().unwrap().workspace.clone() };

    let mut watcher_guard = state.fs_watcher.lock().unwrap();
    *watcher_guard = None;

    if let Some(root) = workspace_root {
        let app_handle = app_handle.clone();
        let last_emit = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
        let last_emit_ref = last_emit.clone();

        let mut watcher =
            match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                match res {
                    Ok(event) => {
                        let relevant = matches!(
                            event.kind,
                            EventKind::Create(_)
                                | EventKind::Remove(_)
                                | EventKind::Modify(ModifyKind::Name(_))
                                | EventKind::Modify(ModifyKind::Data(_))
                                | EventKind::Modify(ModifyKind::Metadata(_))
                                | EventKind::Modify(ModifyKind::Any)
                                | EventKind::Modify(_)
                                | EventKind::Any
                                | EventKind::Other
                        );
                        if !relevant {
                            return;
                        }

                        let now = Instant::now();
                        let mut last = last_emit_ref.lock().unwrap();
                        if now.duration_since(*last) < Duration::from_millis(250) {
                            return;
                        }
                        *last = now;

                        // Emit detailed file change event with paths
                        let paths: Vec<String> = event
                            .paths
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect();

                        let file_change_event = FileChangeEvent {
                            count: paths.len(),
                            paths: paths.clone(),
                        };

                        let _ = app_handle.emit("file-changes-detected", file_change_event);
                        let _ = app_handle.emit(crate::events::event_names::REFRESH_EXPLORER, ());
                    }
                    Err(e) => eprintln!("[WATCHER] error: {}", e),
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("[WATCHER] Failed to start: {}", e);
                    return;
                }
            };

        if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
            eprintln!("[WATCHER] Failed to watch {}: {}", root.display(), e);
            return;
        }

        *watcher_guard = Some(watcher);
        eprintln!("[WATCHER] Watching workspace: {}", root.display());
    }
}

fn check_batch_completion(state: &State<'_, AppState>) {
    let mut batch_guard = state.pending_batch.lock().unwrap();
    if let Some(batch) = batch_guard.as_mut() {
        // If nothing is pending, consider the batch complete
        let no_pending_items =
            batch.commands.is_empty() && batch.changes.is_empty() && batch.confirms.is_empty();

        // A batch is complete if all calls have a corresponding result in file_results
        let all_addressed = batch.calls.iter().all(|call| {
            batch
                .file_results
                .iter()
                .any(|(res_call, _)| res_call.id == call.id)
        });

        if no_pending_items || all_addressed {
            let mut approval_guard = state.pending_approval.lock().unwrap();
            if let Some(tx) = approval_guard.take() {
                let _ = tx.send(true);
            }
        }
    } else {
        // No batch tracked; unblock any pending approval
        let mut approval_guard = state.pending_approval.lock().unwrap();
        if let Some(tx) = approval_guard.take() {
            let _ = tx.send(true);
        }
    }
}

// #[tauri::command]
pub fn approve_tool<R: Runtime>(
    approved: bool,
    window: tauri::Window<R>,
    state: tauri::State<'_, AppState>,
) {
    let app_handle = window.app_handle();
    // Legacy support for shell commands and generic tools
    {
        let mut batch_guard = state.pending_batch.lock().unwrap();
        if let Some(batch) = batch_guard.as_mut() {
            let ws_root = {
                let ws = state.workspace.lock().unwrap();
                ws.workspace
                    .clone()
                    .map(|p| p.to_string_lossy().to_string())
            };

            if approved {
                eprintln!("[APPROVAL] User APPROVED - executing commands");
                // 1. Emit events for shell commands to be executed with terminal display
                for cmd in batch.commands.clone() {
                    // Only emit if not already result
                    if !batch.file_results.iter().any(|(c, _)| c.id == cmd.call.id) {
                        let command_id = format!("cmd-{}", cmd.call.id);
                        eprintln!(
                            "[COMMAND EXEC] Emitting command-execution-started for: {}",
                            cmd.command
                        );
                        let _ = window.emit(
                            crate::events::event_names::COMMAND_EXECUTION_STARTED,
                            crate::events::CommandExecutionStartedPayload {
                                command_id,
                                call_id: cmd.call.id.clone(),
                                command: cmd.command.clone(),
                                cwd: cmd.cwd.clone(),
                            },
                        );
                    }
                }

                // Commands will complete asynchronously via terminal
                // Results will be submitted via submit_command_result command
                // Don't add to file_results here - wait for terminal completion

                // 2. Execute confirmed generic tools
                for conf in batch.confirms.clone() {
                    if !batch.file_results.iter().any(|(c, _)| c.id == conf.call.id) {
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

                        let res = crate::tool_execution::execute_tool_with_context(
                            &context,
                            &conf.call.function.name,
                            &conf.call.function.arguments,
                        );
                        if res.success {
                            let _ = window.emit(crate::events::event_names::REFRESH_EXPLORER, ());
                        }
                        batch.file_results.push((conf.call.clone(), res));
                    }
                }
            } else {
                eprintln!("[APPROVAL] User SKIPPED - NOT executing commands");
                // Skipped - add explicit error results
                for cmd in &batch.commands {
                    if !batch.file_results.iter().any(|(c, _)| c.id == cmd.call.id) {
                        eprintln!("[SKIP] Adding error result for command: {}", cmd.command);
                        let error_msg = format!(
                            "User skipped: '{}'. This command was not executed.",
                            cmd.command
                        );
                        batch
                            .file_results
                            .push((cmd.call.clone(), crate::tools::ToolResult::err(&error_msg)));
                    }
                }
                for conf in &batch.confirms {
                    if !batch.file_results.iter().any(|(c, _)| c.id == conf.call.id) {
                        eprintln!(
                            "[SKIP] Adding error result for action: {}",
                            conf.description
                        );
                        let error_msg = format!(
                            "User skipped: '{}'. This action was not executed.",
                            conf.description
                        );
                        batch
                            .file_results
                            .push((conf.call.clone(), crate::tools::ToolResult::err(&error_msg)));
                    }
                }
            }
        }
    }

    // Only check batch completion if there are no pending command executions
    // Commands execute asynchronously via terminal and submit results via submit_command_result
    let has_pending_commands = {
        let batch_guard = state.pending_batch.lock().unwrap();
        if let Some(batch) = batch_guard.as_ref() {
            batch
                .commands
                .iter()
                .any(|cmd| !batch.file_results.iter().any(|(c, _)| c.id == cmd.call.id))
        } else {
            false
        }
    };

    if !has_pending_commands {
        eprintln!("[APPROVAL] No pending commands, checking batch completion");
        check_batch_completion(&state);
    } else {
        eprintln!(
            "[APPROVAL] Waiting for {} command(s) to complete via terminal",
            {
                let batch_guard = state.pending_batch.lock().unwrap();
                batch_guard.as_ref().map(|b| b.commands.len()).unwrap_or(0)
            }
        );
    }
}

pub fn approve_tool_decision<R: Runtime>(
    decision: String,
    window: tauri::Window<R>,
    state: tauri::State<'_, AppState>,
) {
    let approved = decision == "approve_once" || decision == "approve_always";

    if decision == "approve_always" {
        let mut cache = state.approved_command_roots.lock().unwrap();
        let batch_guard = state.pending_batch.lock().unwrap();
        if let Some(batch) = batch_guard.as_ref() {
            for cmd in &batch.commands {
                if let Some(root) = extract_root_command(&cmd.command) {
                    cache.insert(root);
                }
            }
        }
    }

    approve_tool(approved, window, state);
}

#[tauri::command]
fn submit_command_result(
    call_id: String,
    output: String,
    exit_code: i32,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut batch_guard = state.pending_batch.lock().unwrap();
    if let Some(batch) = batch_guard.as_mut() {
        // Find the command by call_id
        if let Some(cmd) = batch.commands.iter().find(|c| c.call.id == call_id) {
            // Check if result already exists
            if !batch.file_results.iter().any(|(c, _)| c.id == call_id) {
                let result = if exit_code == 0 {
                    crate::tools::ToolResult::ok(output)
                } else if exit_code == 130 {
                    // Exit code 130 means the command was cancelled (SIGINT)
                    // Treat it as a skip
                    eprintln!(
                        "[SUBMIT] Command {} was cancelled (exit 130), treating as skip",
                        call_id
                    );
                    crate::tools::ToolResult {
                        success: false,
                        content: format!(
                            "User skipped: '{}'. This command was not executed.",
                            cmd.command
                        ),
                        error: Some("Tool execution failed".to_string()),
                    }
                } else {
                    // Include the actual output in the error so the AI can see what failed
                    let error_msg = if output.trim().is_empty() {
                        format!("Command failed with exit code {} (no output)", exit_code)
                    } else {
                        format!("Command failed with exit code {}:\n{}", exit_code, output)
                    };
                    crate::tools::ToolResult::err(error_msg)
                };
                batch.file_results.push((cmd.call.clone(), result));

                // Emit tool-execution-completed event for UI to update status
                let _ = app_handle.emit(
                    "tool-execution-completed",
                    events::ToolExecutionCompletedPayload {
                        tool_name: "run_command".to_string(),
                        tool_call_id: call_id.clone(),
                        success: exit_code == 0,
                    },
                );
            }
        }
    }
    drop(batch_guard);

    check_batch_completion(&state);
    Ok(())
}

#[tauri::command]
fn log_frontend(message: String) {
    println!("[FRONTEND] {}", message);
}

// #[tauri::command]
pub async fn approve_change<R: Runtime>(
    change_id: String,
    window: tauri::Window<R>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let app_handle = window.app_handle();
    let change_opt = {
        let mut changes = state.pending_changes.lock().unwrap();
        if let Some(pos) = changes.iter().position(|c| c.call.id == change_id) {
            Some(changes.remove(pos))
        } else {
            None
        }
    };

    if let Some(change) = change_opt {
        println!("[CHANGE APPROVED] Applying change: {}", change_id);

        let workspace_root = {
            let ws = state.workspace.lock().unwrap();
            ws.workspace
                .clone()
                .ok_or_else(|| "No workspace set".to_string())?
        };

        let active_file = state.active_file.lock().unwrap().clone();
        let open_files = state.open_files.lock().unwrap().clone();
        let cursor_line = *state.cursor_line.lock().unwrap();
        let cursor_column = *state.cursor_column.lock().unwrap();
        let selection_start_line = *state.selection_start_line.lock().unwrap();
        let selection_end_line = *state.selection_end_line.lock().unwrap();

        let context = crate::tool_execution::ToolExecutionContext::new(
            Some(workspace_root.to_string_lossy().to_string()),
            active_file,
            open_files,
            0,
            cursor_line,
            cursor_column,
            selection_start_line,
            selection_end_line,
            Some(app_handle.clone()),
        );

        let result = crate::tool_execution::execute_tool_with_context(
            &context,
            &change.call.function.name,
            &change.call.function.arguments,
        );

        if result.success {
            let _ = window.emit(crate::events::event_names::REFRESH_EXPLORER, ());
            let _ = window.emit(
                crate::events::event_names::CHANGE_APPLIED,
                crate::events::ChangeAppliedPayload {
                    change_id: change.call.id.clone(),
                    file_path: change.path.clone(),
                },
            );
        }

        // Update batch results
        {
            let mut batch_guard = state.pending_batch.lock().unwrap();
            if let Some(batch) = batch_guard.as_mut() {
                batch
                    .file_results
                    .push((change.call.clone(), result.clone()));
                // Remove the processed change from the batch to avoid lingering "has_changes"
                batch.changes.retain(|c| c.call.id != change.call.id);
                // Emit tool-execution-completed so UI updates tool call status
                let _ = app_handle.emit(
                    crate::events::event_names::TOOL_EXECUTION_COMPLETED,
                    crate::events::ToolExecutionCompletedPayload {
                        tool_name: change.call.function.name.clone(),
                        tool_call_id: change.call.id.clone(),
                        success: result.success,
                    },
                );
            }
        }

        check_batch_completion(&state);

        if result.success {
            Ok(())
        } else {
            Err(result.error.unwrap_or_else(|| "Unknown error".to_string()))
        }
    } else {
        Err("Change not found".to_string())
    }
}

// #[tauri::command]
pub async fn approve_changes_for_file<R: Runtime>(
    file_path: String,
    window: tauri::Window<R>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    use std::fs;

    println!(
        "[CHANGE APPROVED] Applying all changes for file: {}",
        file_path
    );

    // Get workspace root
    let workspace_root = {
        let ws = state.workspace.lock().unwrap();
        ws.workspace
            .clone()
            .ok_or_else(|| "No workspace set".to_string())?
    };

    // Collect all changes for this file in order
    let changes_for_file = {
        let mut pending = state.pending_changes.lock().unwrap();
        let mut collected = Vec::new();
        let mut remaining = Vec::new();

        for change in pending.drain(..) {
            if change.path == file_path {
                collected.push(change);
            } else {
                remaining.push(change);
            }
        }
        *pending = remaining;
        collected
    };

    if changes_for_file.is_empty() {
        return Err("No changes found for file".to_string());
    }

    println!(
        "[CHANGE APPROVED] Found {} changes for {}",
        changes_for_file.len(),
        file_path
    );

    let full_path = workspace_root.join(&file_path);
    let mut content = match fs::read_to_string(&full_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Failed to read file: {}", e)),
    };

    let mut results = Vec::new();
    for change in &changes_for_file {
        use crate::ai_workflow::ChangeType;

        match &change.change_type {
            ChangeType::NewFile {
                content: new_content,
            } => {
                content = new_content.clone();
                results.push((
                    change.call.clone(),
                    crate::tools::ToolResult::ok(format!("File created: {}", change.path)),
                ));
            }
            ChangeType::Patch {
                old_content,
                new_content,
            } => match crate::tools::apply_patch_to_string(&content, old_content, new_content) {
                Ok(new_content) => {
                    content = new_content;
                    results.push((
                        change.call.clone(),
                        crate::tools::ToolResult::ok(format!("Patch applied to {}", change.path)),
                    ));
                }
                Err(e) => {
                    results.push((
                        change.call.clone(),
                        crate::tools::ToolResult::err(e.clone()),
                    ));
                    break;
                }
            },
            ChangeType::MultiPatch { patches } => {
                // Apply each patch sequentially
                let mut all_ok = true;
                let patch_count = patches.len();
                for (idx, patch) in patches.iter().enumerate() {
                    match crate::tools::apply_patch_to_string(
                        &content,
                        &patch.old_text,
                        &patch.new_text,
                    ) {
                        Ok(new_content) => {
                            content = new_content;
                        }
                        Err(e) => {
                            results.push((
                                change.call.clone(),
                                crate::tools::ToolResult::err(format!(
                                    "Multi-patch failed at hunk {}/{}: {}",
                                    idx + 1,
                                    patch_count,
                                    e
                                )),
                            ));
                            all_ok = false;
                            break;
                        }
                    }
                }
                if all_ok {
                    results.push((
                        change.call.clone(),
                        crate::tools::ToolResult::ok(format!(
                            "Applied {} patches atomically to {}",
                            patch_count, change.path
                        )),
                    ));
                }
            }
            ChangeType::DeleteFile => {
                // Delete file - don't update content, just mark for deletion
                results.push((
                    change.call.clone(),
                    crate::tools::ToolResult::ok(format!(
                        "File marked for deletion: {}",
                        change.path
                    )),
                ));
            }
        }
    }

    // Write back to disk
    if let Some(parent) = full_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&full_path, content.as_bytes()).map_err(|e| e.to_string())?;

    // Refresh explorer after successful write
    let _ = window.emit(crate::events::event_names::REFRESH_EXPLORER, ());

    // Notify frontend that all edits for this file are applied
    let _ = window.emit(
        crate::events::event_names::ALL_EDITS_APPLIED,
        crate::events::AllEditsAppliedPayload {
            count: results.len(),
            file_paths: vec![file_path.clone()],
        },
    );

    // Update batch results
    {
        let mut batch_guard = state.pending_batch.lock().unwrap();
        if let Some(batch) = batch_guard.as_mut() {
            for res in results {
                batch.file_results.push(res);
                // Remove processed changes for this file to clear pending state
                batch.changes.retain(|c| c.path != file_path);
                let (call, tool_res) = batch.file_results.last().unwrap();
                let _ = window.app_handle().emit(
                    crate::events::event_names::TOOL_EXECUTION_COMPLETED,
                    crate::events::ToolExecutionCompletedPayload {
                        tool_name: call.function.name.clone(),
                        tool_call_id: call.id.clone(),
                        success: tool_res.success,
                    },
                );
            }
        }
    }

    check_batch_completion(&state);
    Ok(())
}

// #[tauri::command]
pub async fn approve_all_changes(
    window: tauri::Window<tauri::Wry>, // Wait, uses Window so needs R or Wry?
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let files_to_process: Vec<String> = {
        let pending = state.pending_changes.lock().unwrap();
        let mut paths: Vec<String> = pending.iter().map(|c| c.path.clone()).collect();
        let mut seen = std::collections::HashSet::new();
        paths.retain(|p| seen.insert(p.clone()));
        paths
    };

    if files_to_process.is_empty() {
        // Check if there are commands or confirms waiting
        check_batch_completion(&state);
        return Ok(());
    }

    let mut errors = Vec::new();

    // Run per-file approvals in parallel
    let futures: Vec<_> = files_to_process
        .iter()
        .cloned()
        .map(|file_path| approve_changes_for_file(file_path.clone(), window.clone(), state.clone()))
        .collect();

    let results: Vec<Result<(), String>> = join_all(futures).await;

    let mut succeeded = 0;
    let mut failed = 0;

    for (idx, res) in results.into_iter().enumerate() {
        if let Err(e) = res {
            failed += 1;
            if let Some(path) = files_to_process.get(idx) {
                errors.push(format!("{}: {}", path, e));
            } else {
                errors.push(e);
            }
        } else {
            succeeded += 1;
        }
    }

    // v1.1: Emit BatchCompleted event
    let batch_id = uuid::Uuid::new_v4().to_string();
    let _ = window.emit(
        "blade-event",
        blade_protocol::BladeEventEnvelope {
            id: uuid::Uuid::new_v4(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            causality_id: None,
            event: blade_protocol::BladeEvent::Workflow(
                blade_protocol::WorkflowEvent::BatchCompleted {
                    batch_id,
                    succeeded,
                    failed,
                },
            ),
        },
    );

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Failed to apply some changes: {}",
            errors.join("; ")
        ))
    }
}

// #[tauri::command]
pub async fn reject_change(
    change_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let change_opt = {
        let mut changes = state.pending_changes.lock().unwrap();
        if let Some(pos) = changes.iter().position(|c| c.call.id == change_id) {
            Some(changes.remove(pos))
        } else {
            None
        }
    };

    if let Some(change) = change_opt {
        let mut batch_guard = state.pending_batch.lock().unwrap();
        if let Some(batch) = batch_guard.as_mut() {
            batch.file_results.push((
                change.call.clone(),
                crate::tools::ToolResult::err("User rejected change"),
            ));
        }
        drop(batch_guard);
        check_batch_completion(&state);
        Ok(())
    } else {
        Err("Change not found".to_string())
    }
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn toggle_devtools(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(debug_assertions)]
        {
            if window.is_devtools_open() {
                window.close_devtools();
            } else {
                window.open_devtools();
            }
        }
        #[cfg(not(debug_assertions))]
        {
            // In production builds with devtools feature enabled
            if window.is_devtools_open() {
                window.close_devtools();
            } else {
                window.open_devtools();
            }
        }
    }
}

#[tauri::command]
async fn list_models(
    state: State<'_, AppState>,
) -> Result<Vec<crate::models::registry::ModelInfo>, String> {
    let blade_url = {
        let config = state.config.lock().unwrap();
        config.blade_url.clone()
    };
    Ok(crate::models::registry::get_models(&blade_url).await)
}

// Legacy wrapper removed from handler, keeping generic function for dispatch if needed
// #[tauri::command]
pub async fn send_message<R: Runtime>(
    message: String,
    model_id: Option<String>,
    active_file: Option<String>,
    open_files: Option<Vec<String>>,
    cursor_line: Option<usize>,
    cursor_column: Option<usize>,
    selection_start_line: Option<usize>,
    selection_end_line: Option<usize>,
    window: tauri::Window<R>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    handle_send_message(
        message,
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

async fn handle_send_message<R: Runtime>(
    message: String,
    model_id: Option<String>,
    active_file: Option<String>,
    open_files: Option<Vec<String>>,
    cursor_line: Option<usize>,
    cursor_column: Option<usize>,
    selection_start_line: Option<usize>,
    selection_end_line: Option<usize>,
    window: tauri::Window<R>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle<R>,
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

        // Emit update immediately - REMOVED for v1.1 (Frontend handles optimistic updates)
        /*
        if let Some(msg) = conversation.last() {
             window.emit("chat-update", msg).unwrap_or_default();
        }
        */
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
    let blade_url = {
        let config = state.config.lock().unwrap();
        config.blade_url.clone()
    };
    let models = get_models(&blade_url).await;
    {
        let mut mgr = state.chat_manager.lock().unwrap();
        let mut conversation = state.conversation.lock().unwrap();
        let config = state.config.lock().unwrap();
        let workspace = state.workspace.lock().unwrap();

        let mut selected_model = 0; // Default

        if let Some(id) = model_id {
            if let Some(idx) = models.iter().position(|m| m.id == id) {
                selected_model = idx;
            }
        }

        // Store selected model index for use in continue_tool_batch
        *state.selected_model_index.lock().unwrap() = selected_model;

        // We use reqwest Client
        let http = reqwest::Client::new();

        // Ensure workspace root is valid
        let ws = workspace.workspace.as_ref();

        // RFC-002: Get storage mode from project settings, default to "local"
        let storage_mode = Some(
            ws.map(|p| {
                let settings = project_settings::load_project_settings(p);
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

    // 3. Poll for Events (Background Task)
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let mut last_session_id: Option<String> = None;
        let mut _last_emit_fp: Option<String> = None;
        let mut _repeat_emits: u32 = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(32)).await; // 30 FPS

            let state = app_handle.state::<AppState>();

            // Fetch models asynchronously before acquiring locks
            let blade_url = {
                let config = state.config.lock().unwrap();
                config.blade_url.clone()
            };
            let models = get_models(&blade_url).await;

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
                last_session_id = session_id;
            }

            // Emit update
            // Emit update - DISABLED in favor of explicit DrainResult events
            // This prevents double-emission and race conditions with blade-event
            /*
            {
                let conversation = state.conversation.lock().unwrap();
                // We emit the last Assistant message (skip Tool messages)
                let messages = conversation.get_messages();
                if let Some(msg) = messages
                    .iter()
                    .rev()
                    .find(|m| m.role == crate::protocol::ChatRole::Assistant)
                {
                    let content_len = msg.content.len();
                    // ... (rest of logic) ...
                        last_emit_fp = Some(fp);

                        eprintln!(
                            "[EMIT] Assistant message - content: {}, before_tools: {}, after_tools: {}, tool_calls: {}",
                            content_len,
                            before_len,
                            after_len,
                            tool_calls_len
                        );
                        eprintln!("[EMIT] Content preview: {:?}", &msg.content);
                        window.emit("chat-update", msg).unwrap_or_default();
                    }
                }
            }
            */

            if let DrainResult::None = result {
                if !is_streaming {
                    // Auto-save conversation before emitting done
                    {
                        let conversation = state.conversation.lock().unwrap();
                        let mut store = state.conversation_store.lock().unwrap();
                        let stored = conversation.to_stored();
                        if let Err(e) = store.save_conversation(&stored) {
                            eprintln!("Failed to auto-save conversation: {}", e);
                        } else {
                            println!("Auto-saved conversation: {}", stored.metadata.id);
                        }

                        // RFC-002: Also save to local artifacts if in local storage mode
                        let workspace = state.workspace.lock().unwrap();
                        if let Some(ref ws_path) = workspace.workspace {
                            let settings = project_settings::load_project_settings(ws_path);
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

                // 1. Emit legacy format for compatibility - REMOVED for v1.1
                // window.emit("chat-update", msg).unwrap_or_default();

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
                // Convert protocol::TodoItem to events::TodoItem
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

                    eprintln!(
                        "[PENDING CHECK] commands={} changes={} confirms={}",
                        has_cmds, has_changes, has_confirms
                    );

                    if has_cmds || has_changes || has_confirms {
                        let pending = workflow.take_pending();
                        if let Some(ref p) = pending {
                            eprintln!("[PENDING BATCH] commands.len={} changes.len={} confirms.len={} file_results.len={}",
                                p.commands.len(), p.changes.len(), p.confirms.len(), p.file_results.len());
                        }
                        pending
                    } else {
                        None
                    }
                };

                // CRITICAL FIX: Check if batch_opt itself contains commands/changes requiring approval
                // handle_tool_calls() returns the batch immediately if it has ANY results,
                // so we need to check batch_opt first before checking pending_opt
                let mut batch_to_run = None;
                let pending = pending_opt.or(batch_opt);

                if let Some(batch) = pending {
                    let has_pending_changes = !batch.changes.is_empty();
                    let has_pending_actions =
                        !batch.commands.is_empty() || !batch.confirms.is_empty();

                    eprintln!(
                        "[APPROVAL CHECK] has_changes={} has_actions={}",
                        has_pending_changes, has_pending_actions
                    );
                    eprintln!("[APPROVAL CHECK] batch.commands.len={} batch.confirms.len={} batch.file_results.len={}", 
                        batch.commands.len(), batch.confirms.len(), batch.file_results.len());

                    // Debug: Print command details
                    for (idx, cmd) in batch.commands.iter().enumerate() {
                        let has_result =
                            batch.file_results.iter().any(|(c, _)| c.id == cmd.call.id);
                        eprintln!(
                            "[APPROVAL CHECK] Command {}: '{}' has_result={}",
                            idx, cmd.command, has_result
                        );
                    }

                    // If there are NO pending items requiring approval, run immediately
                    if !has_pending_changes && !has_pending_actions {
                        // No approval needed - set batch to run and let it fall through
                        // to the processing code below (line 1057+)
                        batch_to_run = Some(batch);
                    } else {
                        // If we reach here, there ARE pending items that need approval
                        // MUST go through the approval flow

                        // UNIFIED BLOCKING SYSTEM
                        // 1. Store the full batch in AppState so approval commands can update it
                        {
                            let mut batch_guard = state.pending_batch.lock().unwrap();
                            *batch_guard = Some(batch.clone());
                            // Mirror pending changes for approval handlers (approve_change/approve_all_changes)
                            let mut pending_changes = state.pending_changes.lock().unwrap();
                            *pending_changes = batch.changes.clone();
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
                                    crate::ai_workflow::ChangeType::DeleteFile => {
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
                    // Auto-open files in editor only when write_file or create_file tools are called
                    // (read_file operations don't auto-open to avoid disruptive UX)
                    for (call, result) in &batch.file_results {
                        if result.success
                            && (call.function.name == "write_file"
                                || call.function.name == "create_file")
                        {
                            // Extract path from tool arguments
                            if let Ok(args) = serde_json::from_str::<
                                std::collections::HashMap<String, serde_json::Value>,
                            >(&call.function.arguments)
                            {
                                if let Some(path_value) =
                                    args.get("path").or_else(|| args.get("file_path"))
                                {
                                    if let Some(path) = path_value.as_str() {
                                        eprintln!("[AUTO OPEN] Opening file in editor: {}", path);
                                        window.emit("open-file", path).unwrap_or_default();
                                    }
                                }
                            }
                        }
                    }

                    // Check if loop was detected - if so, stop the agentic loop
                    if batch.loop_detected {
                        eprintln!("[AGENTIC LOOP] Stopping due to loop detection");

                        // Fetch models before acquiring locks
                        let blade_url = {
                            let config = state.config.lock().unwrap();
                            config.blade_url.clone()
                        };
                        let models = get_models(&blade_url).await;

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
                        let blade_url = {
                            let config = state.config.lock().unwrap();
                            config.blade_url.clone()
                        };
                        let models = get_models(&blade_url).await;

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
                        // Don't check is_streaming from before - the new stream has started
                        continue;
                    }
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
fn get_conversation(state: State<'_, AppState>) -> Vec<crate::protocol::ChatMessage> {
    let conversation = state.conversation.lock().unwrap();
    conversation.get_messages()
}

#[tauri::command]
async fn open_workspace(
    path: String,
    state: tauri::State<'_, AppState>,
    window: tauri::Window,
) -> Result<(), String> {
    let mut ws = state.workspace.lock().unwrap();
    ws.set_workspace(std::path::PathBuf::from(&path));
    drop(ws);
    restart_fs_watcher(&window.app_handle(), &state);
    let _ = window.emit(crate::events::event_names::REFRESH_EXPLORER, ());
    Ok(())
}

// #[tauri::command]
pub async fn list_files(
    path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::explorer::FileEntry>, String> {
    let ws = state.workspace.lock().unwrap();
    let root = if let Some(p) = path {
        std::path::PathBuf::from(p)
    } else if let Some(w) = &ws.workspace {
        w.clone()
    } else {
        return Err("No workspace open".to_string());
    };

    Ok(crate::explorer::list_directory(&root))
}

// #[tauri::command]
pub async fn read_file_content(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    // Check if there's virtual content for this file
    let virtual_buffers = state.virtual_buffers.lock().unwrap();
    if let Some(virtual_content) = virtual_buffers.get(&path) {
        println!("[VIRTUAL BUFFER] Returning virtual content for: {}", path);
        return Ok(virtual_content.clone());
    }
    drop(virtual_buffers);

    // Resolve path relative to workspace if needed
    let resolved_path = {
        let p = std::path::PathBuf::from(&path);
        if p.is_absolute() {
            p
        } else {
            let ws = state.workspace.lock().unwrap();
            if let Some(root) = &ws.workspace {
                root.join(&p)
            } else {
                p
            }
        }
    };

    // No virtual content, read from disk
    match std::fs::read_to_string(&resolved_path) {
        Ok(content) => {
            if content.is_empty() {
                println!(
                    "[READ FILE CONTENT] Read empty content from: {} (requested: {})",
                    resolved_path.display(),
                    path
                );
            }
            Ok(content)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "[READ FILE CONTENT] Not found: {} (requested: {})",
                resolved_path.display(),
                path
            );
            Ok(String::new())
        }
        Err(e) => Err(e.to_string()),
    }
}

// #[tauri::command]
pub async fn write_file_content(
    path: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let p = std::path::PathBuf::from(&path);
    let resolved_path = if p.is_absolute() {
        p
    } else {
        let ws = state.workspace.lock().unwrap();
        if let Some(root) = ws.workspace.as_ref() {
            root.join(&path)
        } else {
            std::path::PathBuf::from(&path)
        }
    };

    std::fs::write(&resolved_path, content).map_err(|e| e.to_string())
}

// Virtual Buffer Management Commands

#[tauri::command]
fn set_virtual_buffer(
    path: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut virtual_buffers = state.virtual_buffers.lock().unwrap();
    virtual_buffers.insert(path.clone(), content);
    println!("[VIRTUAL BUFFER] Set virtual content for: {}", path);
    Ok(())
}

#[tauri::command]
fn clear_virtual_buffer(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut virtual_buffers = state.virtual_buffers.lock().unwrap();
    virtual_buffers.remove(&path);
    println!("[VIRTUAL BUFFER] Cleared virtual content for: {}", path);
    Ok(())
}

#[tauri::command]
fn has_virtual_buffer(path: String, state: State<'_, AppState>) -> bool {
    let virtual_buffers = state.virtual_buffers.lock().unwrap();
    virtual_buffers.contains_key(&path)
}

#[tauri::command]
fn get_virtual_files(state: State<'_, AppState>) -> Vec<String> {
    let virtual_buffers = state.virtual_buffers.lock().unwrap();
    virtual_buffers.keys().cloned().collect()
}

#[tauri::command]
fn stop_generation(state: State<'_, AppState>, app_handle: tauri::AppHandle) -> bool {
    let mut mgr = state.chat_manager.lock().unwrap();
    let stopped = mgr.request_stop();

    // Clear any pending command batch when stopping
    let mut batch_guard = state.pending_batch.lock().unwrap();
    *batch_guard = None;

    // Cancel all executing commands and emit events immediately
    let mut executing = state.executing_commands.lock().unwrap();
    for (call_id, cancel_flag) in executing.drain() {
        cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        eprintln!("[STOP] Cancelled executing command: {}", call_id);

        // Emit tool-execution-completed event immediately so UI updates
        let _ = app_handle.emit(
            "tool-execution-completed",
            events::ToolExecutionCompletedPayload {
                tool_name: "run_command".to_string(),
                tool_call_id: call_id.clone(),
                success: false,
            },
        );
    }

    stopped
}

#[tauri::command]
fn list_conversations(
    state: State<'_, AppState>,
) -> Result<Vec<conversation_store::ConversationMetadata>, String> {
    let store = state.conversation_store.lock().unwrap();
    Ok(store.list_conversations())
}

#[tauri::command]
fn load_conversation(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let store = state.conversation_store.lock().unwrap();
    let stored = store.load_conversation(&id)?;

    let mut conversation = state.conversation.lock().unwrap();
    *conversation = ConversationHistory::from_stored(stored);

    Ok(())
}

#[tauri::command]
fn new_conversation(model_id: String, state: State<'_, AppState>) -> Result<String, String> {
    // Save current conversation if it has messages
    {
        let conversation = state.conversation.lock().unwrap();
        if conversation.len() > 0 {
            let mut store = state.conversation_store.lock().unwrap();
            let stored = conversation.to_stored();
            store.save_conversation(&stored)?;
        }
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
fn delete_conversation(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut store = state.conversation_store.lock().unwrap();
    store.delete_conversation(&id)
}

#[tauri::command]
fn save_conversation(state: State<'_, AppState>) -> Result<(), String> {
    let conversation = state.conversation.lock().unwrap();
    let mut store = state.conversation_store.lock().unwrap();
    let stored = conversation.to_stored();
    store.save_conversation(&stored)
}

#[tauri::command]
fn get_recent_workspaces(state: State<'_, AppState>) -> Vec<String> {
    let workspace = state.workspace.lock().unwrap();
    workspace.get_recent_workspaces()
}

#[tauri::command]
fn get_current_workspace(state: State<'_, AppState>) -> Option<String> {
    let workspace = state.workspace.lock().unwrap();
    workspace.get_workspace_root()
}

#[tauri::command]
async fn set_selected_model(model_id: String, state: State<'_, AppState>) -> Result<(), String> {
    // Update the selected model index (in-memory only)
    // Model persistence is handled by project state, not main config
    let blade_url = {
        let config = state.config.lock().unwrap();
        config.blade_url.clone()
    };
    let models = get_models(&blade_url).await;
    if let Some(idx) = models.iter().position(|m| m.id == model_id) {
        *state.selected_model_index.lock().unwrap() = idx;
        eprintln!(
            "[MODEL] Set selected model index to {} for {}",
            idx, model_id
        );
        Ok(())
    } else {
        Err(format!("Model not found: {}", model_id))
    }
}

#[tauri::command]
fn get_selected_model(_state: State<'_, AppState>) -> Option<String> {
    // Model is now stored in project state only, not main config
    // Return None to let the frontend use project state or default
    None
}

// ==============================================================================
// Project State Persistence
// ==============================================================================

#[tauri::command]
fn load_project_state(project_path: String) -> Option<project_state::ProjectState> {
    project_state::load_project_state(&project_path)
}

#[tauri::command]
fn save_project_state(state_data: project_state::ProjectState) -> Result<(), String> {
    project_state::save_project_state(&state_data)
}

#[tauri::command]
fn get_project_state_path(project_path: String) -> Option<String> {
    project_state::get_project_state_path(&project_path).map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn read_binary_file(path: String) -> Result<Vec<u8>, String> {
    std::fs::read(&path).map_err(|e| format!("Failed to read binary file: {}", e))
}

// ==============================================================================
// Cache Warmup (Blade Protocol v2.1)
// ==============================================================================

#[tauri::command]
async fn warmup_cache(
    session_id: String,
    model: String,
    trigger: String,
    state: State<'_, AppState>,
) -> Result<warmup::WarmupResponse, String> {
    let trigger = match trigger.as_str() {
        "launch" => warmup::WarmupTrigger::Launch,
        "model_change" => warmup::WarmupTrigger::ModelChange,
        "workspace_change" => warmup::WarmupTrigger::WorkspaceChange,
        "session_resume" => warmup::WarmupTrigger::SessionResume,
        _ => warmup::WarmupTrigger::Launch,
    };

    state
        .warmup_client
        .warmup(&session_id, &model, trigger)
        .await
}

#[tauri::command]
fn should_rewarm_cache(state: State<'_, AppState>) -> bool {
    state.warmup_client.should_rewarm()
}

#[tauri::command]
fn get_user_id(state: State<'_, AppState>) -> Option<String> {
    state.user_id.lock().unwrap().clone()
}

#[tauri::command]
fn get_project_id(workspace_path: String) -> Option<String> {
    let path = std::path::PathBuf::from(workspace_path);
    crate::project::get_or_create_project_id(&path).ok()
}

// ==============================================================================
// Project Settings (RFC-002: Hybrid Unlimited Context)
// ==============================================================================

#[tauri::command]
fn load_project_settings(project_path: String) -> project_settings::ProjectSettings {
    let path = std::path::PathBuf::from(project_path);
    project_settings::load_project_settings(&path)
}

#[tauri::command]
fn save_project_settings(
    project_path: String,
    settings: project_settings::ProjectSettings,
) -> Result<(), String> {
    let path = std::path::PathBuf::from(project_path);
    project_settings::save_project_settings(&path, &settings)
}

#[tauri::command]
fn init_zblade_directory(project_path: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(project_path);
    project_settings::init_zblade_dir(&path)
}

#[tauri::command]
fn has_zblade_directory(project_path: String) -> bool {
    let path = std::path::PathBuf::from(project_path);
    project_settings::has_zblade_dir(&path)
}

// ==============================================================================
// Local Context Retrieval (RFC-002: Hybrid Unlimited Context)
// ==============================================================================

#[tauri::command]
fn list_local_conversations(
    project_path: String,
) -> Result<Vec<local_index::ConversationIndex>, String> {
    let path = std::path::PathBuf::from(project_path);
    let store = local_artifacts::LocalArtifactStore::new(&path);
    store.list_conversations()
}

#[tauri::command]
fn load_local_conversation(
    project_path: String,
    conversation_id: String,
) -> Result<local_artifacts::ConversationArtifact, String> {
    let path = std::path::PathBuf::from(project_path);
    let store = local_artifacts::LocalArtifactStore::new(&path);
    store.load_conversation(&conversation_id)
}

#[tauri::command]
fn search_local_moments(
    project_path: String,
    query: String,
    limit: i32,
) -> Result<Vec<local_index::MomentIndex>, String> {
    let path = std::path::PathBuf::from(project_path);
    let store = local_artifacts::LocalArtifactStore::new(&path);
    store.search_moments(&query, limit)
}

#[tauri::command]
fn get_file_context(
    project_path: String,
    file_path: String,
) -> Result<Vec<local_index::CodeReferenceIndex>, String> {
    let path = std::path::PathBuf::from(project_path);
    let store = local_artifacts::LocalArtifactStore::new(&path);
    store.get_file_references(&file_path)
}

#[tauri::command]
fn delete_local_conversation(project_path: String, conversation_id: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(project_path);
    let store = local_artifacts::LocalArtifactStore::new(&path);
    store.delete_conversation(&conversation_id)
}

// ==============================================================================
// Blade Protocol v1.0 Dispatcher
// ==============================================================================

#[tauri::command]
async fn dispatch(
    envelope: blade_protocol::BladeEnvelope<blade_protocol::BladeIntentEnvelope>,
    window: tauri::Window,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    terminal_manager: State<'_, crate::terminal::TerminalManager>,
) -> Result<(), blade_protocol::BladeError> {
    use blade_protocol::{BladeError, BladeIntent, SystemEvent, Version};

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

    println!(
        "[BladeProtocol] Dispatching Intent: {:?} (ID: {})",
        intent, intent_id
    );

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

    // 3. Emit ProtocolVersion on first dispatch (optional: track in state)
    // For now, emit on every dispatch - frontend can dedupe
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

    // 3. Route Intent (Placeholder Implementation)
    let result: Result<(), blade_protocol::BladeError> = match intent {
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
                    // Logic for stop generation
                    // reusing stop_generation command logic
                    stop_generation(state.clone(), app_handle.clone());
                    Ok(())
                }
                blade_protocol::ChatIntent::ClearHistory => {
                    // Logic for clear history
                    let mut conversation = state.conversation.lock().unwrap();
                    conversation.clear();
                    // emit update?
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
        BladeIntent::File(file_intent) => {
            match file_intent {
                blade_protocol::FileIntent::Read { path } => {
                    // Reuse read_file_content command
                    match read_file_content(path.clone(), state.clone()).await {
                        Ok(content) => {
                            let _ = window.emit(
                                "sys-event",
                                blade_protocol::BladeEvent::File(
                                    blade_protocol::FileEvent::Content {
                                        path: path,
                                        data: content,
                                    },
                                ),
                            );
                            Ok(())
                        }
                        Err(e) => Err(blade_protocol::BladeError::ResourceNotFound {
                            id: path + " (" + &e + ")",
                        }),
                    }
                }
                blade_protocol::FileIntent::Write { path, content } => {
                    // Reuse write_file_content command
                    match write_file_content(path.clone(), content, state.clone()).await {
                        Ok(_) => {
                            let _ = window.emit(
                                "sys-event",
                                blade_protocol::BladeEvent::File(
                                    blade_protocol::FileEvent::Written { path: path },
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
                blade_protocol::FileIntent::List { path } => {
                    // Reuse list_files command
                    match list_files(path.clone(), state.clone()).await {
                        Ok(entries) => {
                            // Convert crate::explorer::FileEntry to blade_protocol::FileEntry
                            // We need a helper or map manually.
                            // Since structs are identical but distinct types, we must map.
                            let protocol_entries = entries
                                .into_iter()
                                .map(|e| blade_protocol::FileEntry {
                                    name: e.name,
                                    path: e.path,
                                    is_dir: e.is_dir,
                                    children: None, // Simplified for now (shallow)
                                })
                                .collect();

                            let _ = window.emit(
                                "sys-event",
                                blade_protocol::BladeEvent::File(
                                    blade_protocol::FileEvent::Listing {
                                        path: path,
                                        entries: protocol_entries,
                                    },
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
                blade_protocol::FileIntent::Create { path, is_dir } => {
                    // Resolve path relative to workspace
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
                        // Create parent directories if needed
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
                                blade_protocol::BladeEvent::File(
                                    blade_protocol::FileEvent::Created {
                                        path: path.clone(),
                                        is_dir,
                                    },
                                ),
                            );
                            // Trigger explorer refresh
                            let _ = window.emit("refresh-explorer", ());
                            Ok(())
                        }
                        Err(e) => Err(blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: e.to_string(),
                        }),
                    }
                }
                blade_protocol::FileIntent::Delete { path } => {
                    // Resolve path relative to workspace
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
                                blade_protocol::BladeEvent::File(
                                    blade_protocol::FileEvent::Deleted { path: path.clone() },
                                ),
                            );
                            // Trigger explorer refresh
                            let _ = window.emit("refresh-explorer", ());
                            Ok(())
                        }
                        Err(e) => Err(blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: e.to_string(),
                        }),
                    }
                }
                blade_protocol::FileIntent::Rename { old_path, new_path } => {
                    // Resolve paths relative to workspace
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
                                blade_protocol::BladeEvent::File(
                                    blade_protocol::FileEvent::Renamed {
                                        old_path: old_path.clone(),
                                        new_path: new_path.clone(),
                                    },
                                ),
                            );
                            // Trigger explorer refresh
                            let _ = window.emit("refresh-explorer", ());
                            Ok(())
                        }
                        Err(e) => Err(blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: e.to_string(),
                        }),
                    }
                }
            }
        }
        BladeIntent::Editor(editor_intent) => {
            println!("Editor Intent: {:?}", editor_intent);
            Ok(())
        }
        BladeIntent::Workflow(workflow_intent) => match workflow_intent {
            // v1.1 variants
            blade_protocol::WorkflowIntent::ApproveAction { action_id } => {
                approve_change(action_id, window.clone(), state.clone())
                    .await
                    .map_err(|e| blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    })
            }
            blade_protocol::WorkflowIntent::ApproveAll { batch_id: _ } => {
                approve_all_changes(window.clone(), state.clone())
                    .await
                    .map_err(|e| blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    })
            }
            blade_protocol::WorkflowIntent::RejectAction { action_id } => {
                reject_change(action_id, state.clone()).await.map_err(|e| {
                    blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    }
                })
            }
            blade_protocol::WorkflowIntent::RejectAll { batch_id: _ } => {
                // TODO: Implement batch rejection
                Ok(())
            }
            // Legacy v1.0 variants (for backward compatibility)
            blade_protocol::WorkflowIntent::ApproveChange { change_id } => {
                approve_change(change_id, window.clone(), state.clone())
                    .await
                    .map_err(|e| blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    })
            }
            blade_protocol::WorkflowIntent::RejectChange { change_id } => {
                reject_change(change_id, state.clone()).await.map_err(|e| {
                    blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    }
                })
            }
            blade_protocol::WorkflowIntent::ApproveAllChanges => {
                approve_all_changes(window.clone(), state.clone())
                    .await
                    .map_err(|e| blade_protocol::BladeError::Internal {
                        trace_id: intent_id.to_string(),
                        message: e,
                    })
            }
            blade_protocol::WorkflowIntent::ApproveTool { approved } => {
                approve_tool(approved, window.clone(), state.clone());
                Ok(())
            }
            blade_protocol::WorkflowIntent::ApproveToolDecision { decision } => {
                approve_tool_decision(decision, window.clone(), state.clone());
                Ok(())
            }
        },
        BladeIntent::Terminal(terminal_intent) => {
            match terminal_intent {
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
                            app_handle.clone(),
                            terminal_manager.clone(),
                        )
                        .map_err(|e| {
                            blade_protocol::BladeError::Internal {
                                trace_id: intent_id.to_string(),
                                message: e,
                            }
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
                    crate::terminal::resize_terminal(id, rows, cols, terminal_manager.clone())
                        .map_err(|e| blade_protocol::BladeError::Internal {
                            trace_id: intent_id.to_string(),
                            message: e,
                        })
                }
                blade_protocol::TerminalIntent::Kill { id: _ } => {
                    // TODO: Implement kill
                    Ok(())
                }
            }
        }
        BladeIntent::History(history_intent) => {
            match history_intent {
                blade_protocol::HistoryIntent::ListConversations {
                    user_id,
                    project_id,
                } => {
                    println!(
                        "[History] ListConversations: user={}, project={}",
                        user_id, project_id
                    );

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
                    match blade_client
                        .get_conversation_history(&user_id, &project_id)
                        .await
                    {
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

                            println!("[History] Fetched {} conversations", conversations.len());
                            if let Some(first) = conversations.first() {
                                println!("[History] Sample conversation dates - created_at: {}, last_active_at: {}", 
                                    first.created_at, first.last_active_at);
                            }

                            // Emit ConversationList event
                            let _ = window.emit(
                                "blade-event",
                                blade_protocol::BladeEventEnvelope {
                                    id: uuid::Uuid::new_v4(),
                                    timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
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
                            eprintln!("[History] API error: {}", e);
                            Err(blade_protocol::BladeError::Internal {
                                trace_id: intent_id.to_string(),
                                message: e,
                            })
                        }
                    }
                }
                blade_protocol::HistoryIntent::LoadConversation {
                    session_id,
                    user_id,
                } => {
                    println!(
                        "[History] LoadConversation: session={}, user={}",
                        session_id, user_id
                    );

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
                    match blade_client.get_conversation(&session_id, &user_id).await {
                        Ok(response) => {
                            // Parse response into FullConversation
                            let session_id = response
                                .get("session_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&session_id)
                                .to_string();
                            let project_id = response
                                .get("project_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let title = response
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Untitled")
                                .to_string();
                            let created_at = response
                                .get("created_at")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let last_active_at = response
                                .get("last_active_at")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let message_count = response
                                .get("message_count")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0)
                                as u32;

                            let messages: Vec<blade_protocol::HistoryMessage> =
                                if let Some(msgs) = response.get("messages") {
                                    serde_json::from_value(msgs.clone()).unwrap_or_else(|e| {
                                        eprintln!("[History] Failed to parse messages: {}", e);
                                        Vec::new()
                                    })
                                } else {
                                    Vec::new()
                                };

                            println!(
                                "[History] Loaded conversation with {} messages",
                                messages.len()
                            );

                            // Emit ConversationLoaded event
                            let _ = window.emit(
                                "blade-event",
                                blade_protocol::BladeEventEnvelope {
                                    id: uuid::Uuid::new_v4(),
                                    timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_millis()
                                        as u64,
                                    causality_id: Some(intent_id.to_string()),
                                    event: blade_protocol::BladeEvent::History(
                                        blade_protocol::HistoryEvent::ConversationLoaded {
                                            session_id,
                                            project_id,
                                            title,
                                            created_at,
                                            last_active_at,
                                            message_count,
                                            messages,
                                        },
                                    ),
                                },
                            );
                            Ok(())
                        }
                        Err(e) => {
                            eprintln!("[History] API error: {}", e);
                            Err(blade_protocol::BladeError::ResourceNotFound {
                                id: format!("Conversation {}: {}", session_id, e),
                            })
                        }
                    }
                }
            }
        }
        BladeIntent::System(system_intent) => {
            println!("System Intent: {:?}", system_intent);
            Ok(())
        }
    };

    // 5. Handle Result & Store Idempotency (v1.1)
    match result {
        Ok(_) => {
            // Store success in idempotency cache if key provided
            if let Some(key) = idempotency_key {
                state.idempotency_cache.store_success(key, intent_id);
            }
            let _ = window.emit("sys-event", SystemEvent::ProcessCompleted { intent_id });
            Ok(())
        }
        Err(e) => {
            // Store failure in idempotency cache if key provided
            if let Some(key) = idempotency_key {
                state
                    .idempotency_cache
                    .store_failure(key, intent_id, format!("{:?}", e));
            }
            let _ = window.emit(
                "sys-event",
                SystemEvent::IntentFailed {
                    intent_id,
                    error: e.clone(),
                },
            );
            Err(e)
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cli = Cli::parse();

    // Resolve relative paths (like "." or "..") to absolute paths
    let resolved_path = cli.path.map(|p| {
        let path = std::path::PathBuf::from(&p);
        if path.is_relative() {
            // Resolve relative to current working directory
            std::env::current_dir()
                .ok()
                .map(|cwd| cwd.join(&path))
                .and_then(|full| std::fs::canonicalize(&full).ok())
                .map(|abs| abs.to_string_lossy().to_string())
                .unwrap_or(p)
        } else {
            // Already absolute, just canonicalize if possible
            std::fs::canonicalize(&path)
                .map(|abs| abs.to_string_lossy().to_string())
                .unwrap_or(p)
        }
    });

    tauri::Builder::default()
        .setup(|app| {
            let state = app.state::<AppState>();
            restart_fs_watcher(&app.handle(), &state);
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::new(resolved_path))
        .manage(terminal::TerminalManager::new())
        .invoke_handler(tauri::generate_handler![
            greet,
            toggle_devtools,
            log_frontend,
            get_conversation,
            list_models,
            open_workspace,
            set_virtual_buffer,
            clear_virtual_buffer,
            has_virtual_buffer,
            get_virtual_files,
            list_conversations,
            load_conversation,
            new_conversation,
            delete_conversation,
            save_conversation,
            get_recent_workspaces,
            get_current_workspace,
            set_selected_model,
            get_selected_model,
            load_project_state,
            save_project_state,
            get_project_state_path,
            warmup_cache,
            should_rewarm_cache,
            get_user_id,
            get_project_id,
            load_project_settings,
            save_project_settings,
            init_zblade_directory,
            has_zblade_directory,
            list_local_conversations,
            load_local_conversation,
            search_local_moments,
            get_file_context,
            delete_local_conversation,
            submit_command_result,
            read_binary_file,
            ephemeral_commands::create_ephemeral_document,
            ephemeral_commands::get_ephemeral_document,
            ephemeral_commands::update_ephemeral_document,
            ephemeral_commands::close_ephemeral_document,
            ephemeral_commands::list_ephemeral_documents,
            ephemeral_commands::save_ephemeral_document,
            dispatch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
