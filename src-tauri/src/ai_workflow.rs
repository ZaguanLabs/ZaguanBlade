mod change_parser;
mod tool_defs;

use change_parser::parse_change_args;

use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

// use eframe::egui; // Removed for Tauri migration

use crate::protocol::ToolCall;
use crate::tool_execution::ToolExecutionContext;
use crate::tools;
use tauri::Emitter;

pub use tool_defs::get_tool_definitions;

#[derive(Clone)]
pub struct PendingCommand {
    pub call: ToolCall,
    pub command: String,
    pub cwd: Option<String>,
}

fn normalize_json_string(input: &str) -> String {
    // Parse JSON and produce a stable canonical string for loop detection/cache keys
    serde_json::from_str::<Value>(input)
        .unwrap_or(Value::Null)
        .to_string()
}

/// A single patch hunk within a multi-patch operation
#[derive(Clone, Debug, serde::Serialize)]
pub struct PatchHunk {
    pub old_text: String,
    pub new_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
}

#[derive(Clone, serde::Serialize)]
pub enum ChangeType {
    Patch {
        old_content: String,
        new_content: String,
    },
    /// Multi-hunk atomic patch (multiple changes applied together)
    MultiPatch {
        patches: Vec<PatchHunk>,
    },
    NewFile {
        content: String,
    },
    DeleteFile {
        old_content: Option<String>,
    },
}

#[derive(Clone, serde::Serialize)]
pub struct PendingChange {
    pub call: ToolCall,
    pub path: String,
    pub change_type: ChangeType,
    pub applied: bool,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct PendingConfirm {
    pub call: ToolCall,
    pub tool_name: String,
    pub description: String,
}

#[derive(Default, Clone)]
pub struct PendingToolBatch {
    pub calls: Vec<ToolCall>,
    pub file_results: Vec<(ToolCall, tools::ToolResult)>,
    pub commands: Vec<PendingCommand>,
    pub changes: Vec<PendingChange>,
    pub confirms: Vec<PendingConfirm>,
    pub loop_detected: bool,
}

#[derive(Default)]
pub struct AiWorkflow {
    pending: Option<PendingToolBatch>,
    pub recent_history: Vec<(String, String)>, // (name, args)
    recent_file_tool_cache: Vec<((String, String), tools::ToolResult)>,
    last_assistant_content_fingerprint: Option<String>,
    stagnant_tool_turns: usize,
}

impl AiWorkflow {
    pub fn new() -> Self {
        Self {
            pending: None,
            recent_history: Vec::new(),
            recent_file_tool_cache: Vec::new(),
            last_assistant_content_fingerprint: None,
            stagnant_tool_turns: 0,
        }
    }

    pub fn has_pending_commands(&self) -> bool {
        self.pending
            .as_ref()
            .map(|b| !b.commands.is_empty())
            .unwrap_or(false)
    }

    pub fn has_pending_changes(&self) -> bool {
        self.pending
            .as_ref()
            .map(|b| !b.changes.is_empty())
            .unwrap_or(false)
    }

    pub fn has_pending_confirms(&self) -> bool {
        self.pending
            .as_ref()
            .map(|b| !b.confirms.is_empty())
            .unwrap_or(false)
    }

    pub fn handle_tool_calls<R: tauri::Runtime>(
        &mut self,
        workspace_root: &Path,
        calls: Vec<ToolCall>,
        content: Option<String>,
        context: &ToolExecutionContext<R>,
    ) -> Option<PendingToolBatch> {
        // Tool-spam / no-progress guardrail:
        // If the assistant message content doesn't materially change across tool turns,
        // the model can get stuck in tool-only loops (especially with Qwen).
        // We stop early and force a response instead of burning through AgenticLoop max_turns.
        if !calls.is_empty() {
            let fingerprint = content.as_ref().map(|s| {
                s.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string()
            });

            if let Some(fp) = fingerprint {
                // Treat empty / whitespace-only assistant content as "no content" to avoid false spam
                if fp.is_empty() {
                    self.last_assistant_content_fingerprint = None;
                    self.stagnant_tool_turns = 0;
                } else {
                    if self
                        .last_assistant_content_fingerprint
                        .as_ref()
                        .is_some_and(|prev| prev == &fp)
                    {
                        self.stagnant_tool_turns += 1;
                    } else {
                        self.last_assistant_content_fingerprint = Some(fp);
                        self.stagnant_tool_turns = 1;
                    }

                    const STAGNANT_TOOL_TURN_LIMIT: usize = 4;
                    if self.stagnant_tool_turns >= STAGNANT_TOOL_TURN_LIMIT {
                        eprintln!(
                            "[AI WORKFLOW] Tool-spam detected: assistant content unchanged for {} tool turns",
                            self.stagnant_tool_turns
                        );

                        // Check if any of the calls are run_command - these must go through approval
                        let has_run_command =
                            calls.iter().any(|c| c.function.name == "run_command");

                        if has_run_command {
                            eprintln!(
                                "[AI WORKFLOW] run_command detected in spam batch - allowing through for approval"
                            );
                            // Don't block run_command - let it go through normal interception
                            // Reset spam counter since we're allowing this through
                            self.stagnant_tool_turns = 0;
                        } else {
                            // Block other tools with error
                            let mut file_results = Vec::new();
                            for call in calls.iter().cloned() {
                                file_results.push((
                                    call,
                                    tools::ToolResult {
                                        success: false,
                                        content: String::new(),
                                        error: Some(
                                            "SYSTEM WARNING: NO PROGRESS DETECTED - You have been calling tools for multiple turns without making progress. DO NOT call any more tools. Answer the user's question NOW using the information you have gathered."
                                                .to_string(),
                                        ),
                                    },
                                ));
                            }

                            return Some(PendingToolBatch {
                                calls,
                                file_results,
                                commands: Vec::new(),
                                changes: Vec::new(),
                                confirms: Vec::new(),
                                loop_detected: true,
                            });
                        }
                    }
                }
            } else {
                // No content snapshot provided; avoid false positives.
                self.last_assistant_content_fingerprint = None;
                self.stagnant_tool_turns = 0;
            }
        }

        let mut file_results: Vec<(ToolCall, tools::ToolResult)> = Vec::new();
        let mut commands: Vec<PendingCommand> = Vec::new();
        let changes: Vec<PendingChange> = Vec::new();
        let mut confirms: Vec<PendingConfirm> = Vec::new();
        let mut loop_detected = false;
        let mut seen_in_batch: HashMap<(String, String), usize> = HashMap::new();

        struct PendingRead<R: tauri::Runtime> {
            call: ToolCall,
            context: crate::tool_execution::ToolExecutionContext<R>,
        }
        let mut pending_read_tasks: Vec<PendingRead<R>> = Vec::new();

        for call in &calls {
            // Normalize arguments for comparison
            let normalized_args = normalize_json_string(&call.function.arguments);

            // Loop Detection
            let call_sig = (call.function.name.clone(), normalized_args.clone());

            // Caching for read_file
            if matches!(call.function.name.as_str(), "read_file" | "read_file_range") {
                if let Some((_, cached)) = self
                    .recent_file_tool_cache
                    .iter()
                    .rev()
                    .find(|(sig, res)| sig == &call_sig && res.success)
                {
                    file_results.push((call.clone(), cached.clone()));
                    self.recent_history.push(call_sig);
                    if self.recent_history.len() > 10 {
                        self.recent_history.remove(0);
                    }
                    continue;
                }
            }

            // Loop Detection Exemption
            let is_exempt = matches!(
                call.function.name.as_str(),
                "get_editor_state" | "get_workspace_structure"
            );
            if !is_exempt {
                let recent_count = self
                    .recent_history
                    .iter()
                    .filter(|h| *h == &call_sig)
                    .count();
                let batch_count = *seen_in_batch.get(&call_sig).unwrap_or(&0);
                let total_seen = recent_count + batch_count;

                let limit = if matches!(
                    call.function.name.as_str(),
                    "read_file" | "read_file_range" | "grep_search"
                ) {
                    3
                } else {
                    2
                };

                if total_seen >= limit {
                    eprintln!(
                        "[AI WORKFLOW] Loop detected for tool: {}",
                        call.function.name
                    );
                    loop_detected = true;
                    file_results.push((
                        call.clone(),
                        tools::ToolResult {
                            success: false,
                            content: String::new(),
                            error: Some("SYSTEM WARNING: LOOP DETECTED - You called this tool with identical arguments before. DO NOT call any more tools. Use the information from your previous tool calls to answer the user's question NOW.".to_string()),
                        },
                    ));
                    continue;
                }
            }

            *seen_in_batch.entry(call_sig.clone()).or_insert(0) += 1;
            self.recent_history.push(call_sig.clone());
            if self.recent_history.len() > 10 {
                self.recent_history.remove(0);
            }

            // INTERCEPTION LOGIC
            if call.function.name == "run_command" {
                match parse_run_command_args(&call.function.arguments) {
                    Ok((command, cwd)) => {
                        if let Some(err) = should_block_irrelevant_language_scan(
                            &command,
                            workspace_root,
                            cwd.as_deref(),
                        ) {
                            file_results.push((call.clone(), tools::ToolResult::err(err)));
                            continue;
                        }
                        commands.push(PendingCommand {
                            call: call.clone(),
                            command,
                            cwd,
                        })
                    }
                    Err(e) => file_results.push((call.clone(), tools::ToolResult::err(e))),
                }
            } else if matches!(
                call.function.name.as_str(),
                "edit_file" | "apply_edit" | "apply_patch" | "write_file" | "create_file"
            ) {
                match parse_change_args(
                    &call.function.arguments,
                    workspace_root,
                    &call.function.name,
                ) {
                    Ok(change) => {
                        // NEW LOGIC: Apply the change IMMEDIATELY to disk
                        // This makes it act like an "Undo/Redo" buffer - change is live, can be undone.

                        let full_path = workspace_root.join(&change.path);

                        // History Snapshot
                        if let Some(app) = &context.app_handle {
                            use tauri::Manager;
                            let state = app.state::<crate::app_state::AppState>();
                            if full_path.exists() {
                                match state
                                    .history_service
                                    .create_snapshot(&full_path, Some(call.id.clone()))
                                {
                                    Ok(entry) => {
                                        println!("[HISTORY] Snapshot created for {}", change.path);
                                        let _ = app.emit(
                                            crate::events::event_names::HISTORY_ENTRY_ADDED,
                                            crate::events::HistoryEntryAddedPayload { entry },
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[HISTORY] Failed to create snapshot for {}: {}",
                                            change.path, e
                                        );
                                    }
                                }
                            }
                        }

                        let apply_result = (|| -> Result<(), String> {
                            match &change.change_type {
                                ChangeType::Patch {
                                    old_content,
                                    new_content,
                                } => {
                                    let current_content = fs::read_to_string(&full_path)
                                        .map_err(|e| format!("Failed to read file: {}", e))?;
                                    let new_file_content = tools::apply_patch_to_string(
                                        &current_content,
                                        old_content,
                                        new_content,
                                    )?;
                                    fs::write(&full_path, new_file_content)
                                        .map_err(|e| format!("Failed to write file: {}", e))?;
                                    Ok(())
                                }
                                ChangeType::MultiPatch { patches } => {
                                    let mut content = fs::read_to_string(&full_path)
                                        .map_err(|e| format!("Failed to read file: {}", e))?;
                                    for patch in patches {
                                        content = tools::apply_patch_to_string(
                                            &content,
                                            &patch.old_text,
                                            &patch.new_text,
                                        )?;
                                    }
                                    fs::write(&full_path, content)
                                        .map_err(|e| format!("Failed to write file: {}", e))?;
                                    Ok(())
                                }
                                ChangeType::NewFile { content } => {
                                    if let Some(parent) = full_path.parent() {
                                        let _ = fs::create_dir_all(parent);
                                    }
                                    fs::write(&full_path, content)
                                        .map_err(|e| format!("Failed to create file: {}", e))?;
                                    Ok(())
                                }
                                ChangeType::DeleteFile { .. } => {
                                    // Can't "apply" delete safely in a way that is easily undoable without manual backup?
                                    // Or we just do it. But logic for undo needs content.
                                    // For now, let's DELAY delete or apply it?
                                    // User said "AI-applied changes are immediately written".
                                    // So we delete it.
                                    fs::remove_file(&full_path)
                                        .map_err(|e| format!("Failed to delete file: {}", e))?;
                                    Ok(())
                                }
                            }
                        })();

                        match apply_result {
                            Ok(_) => {
                                println!("[AI WORKFLOW] Auto-applied change to {}", change.path);
                                if let Some(app) = &context.app_handle {
                                    let _ = app.emit("refresh-explorer", ());
                                    let _ = app.emit(
                                        crate::events::event_names::CHANGE_APPLIED,
                                        crate::events::ChangeAppliedPayload {
                                            change_id: call.id.clone(),
                                            file_path: change.path.clone(),
                                        },
                                    );
                                }
                                file_results.push((
                                    call.clone(),
                                    tools::ToolResult::ok(format!(
                                        "Change applied to {}",
                                        change.path
                                    )),
                                ));
                            }
                            Err(e) => {
                                eprintln!("[AI WORKFLOW] Failed to auto-apply change: {}", e);
                                file_results.push((
                                    call.clone(),
                                    tools::ToolResult::err(format!(
                                        "Failed to apply change: {}",
                                        e
                                    )),
                                ));
                            }
                        }

                        // change.call = call.clone();
                        // changes.push(change);
                    }
                    Err(e) => file_results.push((call.clone(), tools::ToolResult::err(e))),
                }
            } else if call.function.name == "delete_file" {
                match parse_change_args(
                    &call.function.arguments,
                    workspace_root,
                    &call.function.name,
                ) {
                    Ok(mut change) => {
                        // Same immediate apply logic for delete_file
                        let full_path = workspace_root.join(&change.path);

                        // History Snapshot
                        if let Some(app) = &context.app_handle {
                            use tauri::Manager;
                            let state = app.state::<crate::app_state::AppState>();
                            if full_path.exists() {
                                match state
                                    .history_service
                                    .create_snapshot(&full_path, Some(call.id.clone()))
                                {
                                    Ok(entry) => {
                                        println!("[HISTORY] Snapshot created for {}", change.path);
                                        let _ = app.emit(
                                            crate::events::event_names::HISTORY_ENTRY_ADDED,
                                            crate::events::HistoryEntryAddedPayload { entry },
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[HISTORY] Failed to create snapshot for {}: {}",
                                            change.path, e
                                        );
                                    }
                                }
                            }
                        }

                        // Capture content for undo
                        if let ChangeType::DeleteFile {
                            ref mut old_content,
                        } = change.change_type
                        {
                            if let Ok(content) = fs::read_to_string(&full_path) {
                                *old_content = Some(content);
                            }
                        }

                        match fs::remove_file(&full_path) {
                            Ok(_) => {
                                if let Some(app) = &context.app_handle {
                                    let _ = app.emit("refresh-explorer", ());
                                    let _ = app.emit(
                                        crate::events::event_names::CHANGE_APPLIED,
                                        crate::events::ChangeAppliedPayload {
                                            change_id: call.id.clone(),
                                            file_path: change.path.clone(),
                                        },
                                    );
                                }
                                file_results.push((
                                    call.clone(),
                                    tools::ToolResult::ok(format!("File deleted: {}", change.path)),
                                ));
                            }
                            Err(e) => {
                                let err_msg = e.to_string();
                                eprintln!("[AI WORKFLOW] Failed to auto-delete file: {}", err_msg);
                                file_results.push((
                                    call.clone(),
                                    tools::ToolResult::err(format!(
                                        "Failed to delete file: {}",
                                        err_msg
                                    )),
                                ));
                            }
                        }

                        // change.call = call.clone();
                        // changes.push(change);
                    }
                    Err(e) => file_results.push((call.clone(), tools::ToolResult::err(e))),
                }
            } else if matches!(
                call.function.name.as_str(),
                "create_directory" | "move_file" | "copy_file"
            ) {
                let description = match call.function.name.as_str() {
                    "create_directory" => {
                        let path =
                            serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                                .ok()
                                .and_then(|v| {
                                    v.get("path")
                                        .and_then(|p| p.as_str())
                                        .map(|s| s.to_string())
                                })
                                .unwrap_or_else(|| "unknown path".to_string());
                        format!("Create directory: {}", path)
                    }
                    "delete_file" => {
                        let path =
                            serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                                .ok()
                                .and_then(|v| {
                                    v.get("path")
                                        .and_then(|p| p.as_str())
                                        .map(|s| s.to_string())
                                })
                                .unwrap_or_else(|| "unknown path".to_string());
                        format!("Delete file: {}", path)
                    }
                    "move_file" => {
                        let args =
                            serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                                .ok();
                        let src = args
                            .as_ref()
                            .and_then(|v| {
                                v.get("src_path")
                                    .or_else(|| v.get("from"))
                                    .and_then(|p| p.as_str())
                            })
                            .unwrap_or("unknown");
                        let dst = args
                            .as_ref()
                            .and_then(|v| {
                                v.get("dest_path")
                                    .or_else(|| v.get("to"))
                                    .and_then(|p| p.as_str())
                            })
                            .unwrap_or("unknown");
                        format!("Move {} to {}", src, dst)
                    }
                    "copy_file" => {
                        let args =
                            serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                                .ok();
                        let src = args
                            .as_ref()
                            .and_then(|v| {
                                v.get("src_path")
                                    .or_else(|| v.get("from"))
                                    .and_then(|p| p.as_str())
                            })
                            .unwrap_or("unknown");
                        let dst = args
                            .as_ref()
                            .and_then(|v| {
                                v.get("dest_path")
                                    .or_else(|| v.get("to"))
                                    .and_then(|p| p.as_str())
                            })
                            .unwrap_or("unknown");
                        format!("Copy {} to {}", src, dst)
                    }
                    _ => format!("Execute tool: {}", call.function.name),
                };

                file_results.push((
                    call.clone(),
                    tools::ToolResult::ok(format!("Action proposed: {}", description)),
                ));
                confirms.push(PendingConfirm {
                    call: call.clone(),
                    tool_name: call.function.name.clone(),
                    description,
                });
            } else if matches!(call.function.name.as_str(), "read_file" | "read_file_range") {
                // Defer read_file to run in parallel outside the main loop
                let ctx = crate::tool_execution::ToolExecutionContext {
                    workspace_root: context.workspace_root.clone(),
                    active_file: context.active_file.clone(),
                    open_files: context.open_files.clone(),
                    active_tab_index: context.active_tab_index,
                    cursor_line: context.cursor_line,
                    cursor_column: context.cursor_column,
                    selection_start_line: context.selection_start_line,
                    selection_end_line: context.selection_end_line,
                    app_handle: None, // not needed for read operations
                };
                pending_read_tasks.push(PendingRead {
                    call: call.clone(),
                    context: ctx,
                });
            } else {
                let res = crate::tool_execution::execute_tool_with_context(
                    context,
                    &call.function.name,
                    &call.function.arguments,
                );
                let preview = if res.content.chars().count() > 100 {
                    res.content.chars().take(100).collect::<String>() + "..."
                } else {
                    res.content.clone()
                };
                eprintln!(
                    "[TOOL RESULT] name={} success={} content={:?}",
                    call.function.name, res.success, preview
                );

                if res.success
                    && matches!(call.function.name.as_str(), "read_file" | "read_file_range")
                {
                    self.recent_file_tool_cache
                        .push((call_sig.clone(), res.clone()));
                    if self.recent_file_tool_cache.len() > 10 {
                        self.recent_file_tool_cache.remove(0);
                    }
                }
                file_results.push((call.clone(), res));
            }
        }

        // Execute read_file/read_file_range tasks in parallel threads
        if !pending_read_tasks.is_empty() {
            let mut handles = Vec::new();
            for task in pending_read_tasks {
                handles.push(std::thread::spawn(move || {
                    let res = crate::tool_execution::execute_tool_with_context(
                        &task.context,
                        &task.call.function.name,
                        &task.call.function.arguments,
                    );
                    (task.call, res)
                }));
            }
            for handle in handles {
                if let Ok((call, res)) = handle.join() {
                    let preview = if res.content.chars().count() > 100 {
                        res.content.chars().take(100).collect::<String>() + "..."
                    } else {
                        res.content.clone()
                    };
                    eprintln!(
                        "[TOOL RESULT] name={} success={} content={:?} (parallel read)",
                        call.function.name, res.success, preview
                    );
                    file_results.push((call.clone(), res.clone()));
                    if res.success
                        && matches!(call.function.name.as_str(), "read_file" | "read_file_range")
                    {
                        self.recent_file_tool_cache.push((
                            (
                                call.function.name.clone(),
                                normalize_json_string(&call.function.arguments),
                            ),
                            res.clone(),
                        ));
                        if self.recent_file_tool_cache.len() > 10 {
                            self.recent_file_tool_cache.remove(0);
                        }
                    }
                }
            }
        }

        if !file_results.is_empty()
            || !commands.is_empty()
            || !changes.is_empty()
            || !confirms.is_empty()
        {
            return Some(PendingToolBatch {
                calls,
                file_results,
                commands,
                changes,
                confirms,
                loop_detected,
            });
        }
        self.pending = Some(PendingToolBatch {
            calls,
            file_results,
            commands,
            changes,
            confirms,
            loop_detected,
        });
        None
    }

    /*
        // UI Pending Tool Actions (Commented for Tauri Migration)
        pub fn ui_pending_tool_actions(
            &mut self,
            ui: &mut egui::Ui,
            workspace_root: Option<&Path>,
        ) -> Option<PendingToolBatch> {
            // ... implementation commented out ...
            None
        }
    */
    pub fn take_pending(&mut self) -> Option<PendingToolBatch> {
        self.pending.take()
    }

    pub fn clear_history(&mut self) {
        self.recent_history.clear();
        self.recent_file_tool_cache.clear();
        self.last_assistant_content_fingerprint = None;
        self.stagnant_tool_turns = 0;
    }
}

fn parse_run_command_args(raw_args: &str) -> Result<(String, Option<String>), String> {
    let v: serde_json::Value =
        serde_json::from_str(raw_args).map_err(|e| format!("invalid tool args json: {e}"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| "invalid args: expected object".to_string())?;
    let command = obj
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing required arg: command".to_string())?
        .to_string();
    let cwd = obj
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Ok((command, cwd))
}

fn should_block_irrelevant_language_scan(
    command: &str,
    workspace_root: &Path,
    cwd: Option<&str>,
) -> Option<String> {
    // Only gate obvious Python-file hunts in a Rust workspace (Cargo.toml present) when
    // there are no Python signals. This avoids irrelevant cross-language searches.
    let is_py_find = command.contains("find")
        && (command.contains("*.py") || command.contains(".py ") || command.contains(".py\""));
    if !is_py_find {
        return None;
    }

    let workspace = if let Some(cwd_str) = cwd {
        // If cwd is absolute, use it; otherwise resolve relative to workspace_root.
        let cwd_path = Path::new(cwd_str);
        if cwd_path.is_absolute() {
            cwd_path.to_path_buf()
        } else {
            workspace_root.join(cwd_path)
        }
    } else {
        workspace_root.to_path_buf()
    };

    let cargo_present = workspace.join("Cargo.toml").exists();
    let python_signals = [
        "pyproject.toml",
        "requirements.txt",
        "Pipfile",
        "setup.py",
        ".python-version",
    ]
    .iter()
    .any(|f| workspace.join(f).exists());

    if cargo_present && !python_signals {
        return Some(
            "Blocked irrelevant language scan: Rust workspace (Cargo.toml) with no Python signals. Do not search for Python files here."
                .to_string(),
        );
    }

    None
}

pub fn run_command_in_workspace(
    workspace_root: &Path,
    command: &str,
    cwd: Option<&str>,
) -> tools::ToolResult {
    let ws = match fs::canonicalize(workspace_root) {
        Ok(p) => p,
        Err(e) => {
            return tools::ToolResult {
                success: false,
                content: String::new(),
                error: Some(e.to_string()),
            };
        }
    };

    let dir = if let Some(cwd) = cwd {
        let p = Path::new(cwd);
        let candidate = if p.is_absolute() {
            p.to_path_buf()
        } else {
            ws.join(p)
        };
        let candidate = match fs::canonicalize(&candidate) {
            Ok(p) => p,
            Err(e) => {
                return tools::ToolResult {
                    success: false,
                    content: String::new(),
                    error: Some(e.to_string()),
                };
            }
        };
        if !candidate.starts_with(&ws) {
            return tools::ToolResult {
                success: false,
                content: String::new(),
                error: Some("cwd is outside workspace".to_string()),
            };
        }
        candidate
    } else {
        ws.clone()
    };

    let output = Command::new("sh")
        .arg("-lc")
        .arg(command)
        .current_dir(&dir)
        .output();

    match output {
        Ok(out) => {
            let mut s = String::new();
            s.push_str(&format!("exit_code: {:?}\n", out.status.code()));
            if !out.stdout.is_empty() {
                s.push_str("stdout:\n");
                s.push_str(&String::from_utf8_lossy(&out.stdout));
                if !s.ends_with('\n') {
                    s.push('\n');
                }
            }
            if !out.stderr.is_empty() {
                s.push_str("stderr:\n");
                s.push_str(&String::from_utf8_lossy(&out.stderr));
                if !s.ends_with('\n') {
                    s.push('\n');
                }
            }
            if s.len() > 50_000 {
                s.truncate(50_000);
                s.push_str("\n...truncated...\n");
            }
            tools::ToolResult {
                success: out.status.success(),
                content: s,
                error: None,
            }
        }
        Err(e) => tools::ToolResult {
            success: false,
            content: String::new(),
            error: Some(e.to_string()),
        },
    }
}
