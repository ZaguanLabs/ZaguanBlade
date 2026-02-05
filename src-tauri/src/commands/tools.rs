use crate::app_state::AppState;
use crate::events;
use crate::utils::extract_root_command;
use crate::workflow_controller::check_batch_completion;
use regex::Regex;
use tauri::{Emitter, Manager, Runtime, State, Window};

/// Strip ANSI escape codes from terminal output for clean display in chat
fn strip_ansi_codes(input: &str) -> String {
    // Match ANSI escape sequences:
    // - CSI sequences: \x1b[ followed by parameters and a letter
    // - OSC sequences: \x1b] followed by content and terminated by \x07 or \x1b\\
    // - Other escape sequences: \x1b followed by various characters
    let ansi_regex = Regex::new(
        r"(?x)
        \x1b\[[0-9;?]*[A-Za-z]|     # CSI sequences (colors, cursor, etc.)
        \x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|  # OSC sequences
        \x1b[PX^_][^\x1b]*\x1b\\|   # DCS, SOS, PM, APC sequences
        \x1b[\x20-\x2f]*[\x30-\x7e] # Other escape sequences
        "
    ).unwrap();
    ansi_regex.replace_all(input, "").to_string()
}

// #[tauri::command]
pub fn approve_tool<R: Runtime>(approved: bool, window: Window<R>, state: State<'_, AppState>) {
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
                // Skipped - add explicit error results with clear instruction not to retry
                for cmd in &batch.commands {
                    if !batch.file_results.iter().any(|(c, _)| c.id == cmd.call.id) {
                        eprintln!("[SKIP] Adding error result for command: {}", cmd.command);
                        let error_msg = format!(
                            "User explicitly rejected this command: '{}'. Do NOT retry this command or similar commands. Ask the user how they would like to proceed instead.",
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
                            "User explicitly rejected this action: '{}'. Do NOT retry this action. Ask the user how they would like to proceed instead.",
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
        check_batch_completion(&*state);
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

#[tauri::command]
pub fn approve_tool_decision<R: Runtime>(
    decision: String,
    window: Window<R>,
    state: State<'_, AppState>,
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

/// Approve or skip a single command by its call_id
/// This allows individual command approval instead of batch-only
#[tauri::command]
pub fn approve_single_command<R: Runtime>(
    call_id: String,
    approved: bool,
    window: Window<R>,
    state: State<'_, AppState>,
) {
    let app_handle = window.app_handle();
    
    {
        let mut batch_guard = state.pending_batch.lock().unwrap();
        if let Some(batch) = batch_guard.as_mut() {
            // Find the command by call_id
            if let Some(cmd) = batch.commands.iter().find(|c| c.call.id == call_id) {
                // Check if result already exists
                if !batch.file_results.iter().any(|(c, _)| c.id == call_id) {
                    if approved {
                        eprintln!("[SINGLE APPROVAL] User APPROVED command: {}", cmd.command);
                        // Emit event for this specific command to be executed
                        let command_id = format!("cmd-{}", cmd.call.id);
                        let _ = window.emit(
                            crate::events::event_names::COMMAND_EXECUTION_STARTED,
                            crate::events::CommandExecutionStartedPayload {
                                command_id,
                                call_id: cmd.call.id.clone(),
                                command: cmd.command.clone(),
                                cwd: cmd.cwd.clone(),
                            },
                        );
                    } else {
                        eprintln!("[SINGLE APPROVAL] User SKIPPED command: {}", cmd.command);
                        // Add skip result immediately
                        let error_msg = format!(
                            "User explicitly skipped this command: '{}'. Do NOT retry this command. Ask the user how they would like to proceed instead.",
                            cmd.command
                        );
                        batch.file_results.push((cmd.call.clone(), crate::tools::ToolResult::err(&error_msg)));
                        
                        // Emit tool-execution-completed for UI update
                        let _ = app_handle.emit(
                            "tool-execution-completed",
                            events::ToolExecutionCompletedPayload {
                                tool_name: "run_command".to_string(),
                                tool_call_id: call_id.clone(),
                                success: false,
                            },
                        );
                    }
                }
            }
        }
    }
    
    // Check if all commands have been processed
    check_batch_completion(&*state);
}

#[tauri::command]
pub fn submit_command_result(
    call_id: String,
    output: String,
    exit_code: i32,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Strip ANSI codes from output for clean display in chat and AI context
    let clean_output = strip_ansi_codes(&output);
    
    let mut batch_guard = state.pending_batch.lock().unwrap();
    if let Some(batch) = batch_guard.as_mut() {
        // Find the command by call_id
        if let Some(cmd) = batch.commands.iter().find(|c| c.call.id == call_id) {
            // Check if result already exists
            if !batch.file_results.iter().any(|(c, _)| c.id == call_id) {
                let result = if exit_code == 0 {
                    crate::tools::ToolResult::ok(clean_output.clone())
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
                    let error_msg = if clean_output.trim().is_empty() {
                        format!("Command failed with exit code {} (no output)", exit_code)
                    } else {
                        format!("Command failed with exit code {}:\n{}", exit_code, &clean_output)
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

                // Emit command-executed event with clean output for message blocks
                let _ = app_handle.emit(
                    events::event_names::COMMAND_EXECUTED,
                    events::CommandExecutedPayload {
                        command: cmd.command.clone(),
                        cwd: cmd.cwd.clone(),
                        output: clean_output.clone(),
                        exit_code,
                        duration: None,
                        call_id: call_id.clone(),
                    },
                );
            }
        }
    }
    drop(batch_guard);

    check_batch_completion(&*state);
    Ok(())
}
