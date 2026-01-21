use crate::app_state::AppState;
use crate::{blade_protocol, workflow_controller::check_batch_completion};
use tauri::{Emitter, Manager, Runtime, State, Wry};

pub async fn approve_change_logic<R: Runtime>(
    change_id: String,
    window: tauri::Window<R>,
    state: &AppState,
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

        check_batch_completion(state);

        if result.success {
            Ok(())
        } else {
            Err(result.error.unwrap_or_else(|| "Unknown error".to_string()))
        }
    } else {
        Err("Change not found".to_string())
    }
}

pub async fn approve_changes_for_file_logic<R: Runtime>(
    file_path: String,
    window: tauri::Window<R>,
    state: &AppState,
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
            ChangeType::DeleteFile { .. } => {
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

    check_batch_completion(state);
    Ok(())
}

pub async fn approve_all_changes_logic<R: Runtime>(
    window: tauri::Window<R>,
    state: &AppState,
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
        check_batch_completion(state);
        return Ok(());
    }

    let mut errors = Vec::new();

    for file_path in files_to_process {
        if let Err(e) =
            approve_changes_for_file_logic(file_path.clone(), window.clone(), state).await
        {
            errors.push(format!("{}: {}", file_path, e));
        }
    }

    let _succeeded = 0; // Placeholder
    let failed = errors.len();

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
                    succeeded: 0,
                    failed: failed,
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

pub async fn reject_change_logic<R: Runtime>(
    change_id: String,
    window: tauri::Window<R>,
    state: &AppState,
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
        println!("[REJECT] Reverting change: {}", change_id);

        let workspace_root = {
            let ws = state.workspace.lock().unwrap();
            ws.workspace.clone()
        };

        if let Some(root) = workspace_root {
            let full_path = root.join(&change.path);

            if let Ok(current_content) = std::fs::read_to_string(&full_path) {
                let revert_result = match &change.change_type {
                    crate::ai_workflow::ChangeType::Patch {
                        old_content,
                        new_content,
                    } => crate::tools::apply_patch_to_string(
                        &current_content,
                        new_content,
                        old_content,
                    ),
                    crate::ai_workflow::ChangeType::MultiPatch { patches } => {
                        let mut content = current_content.clone();
                        let mut all_ok = true;
                        for patch in patches.iter().rev() {
                            match crate::tools::apply_patch_to_string(
                                &content,
                                &patch.new_text,
                                &patch.old_text,
                            ) {
                                Ok(c) => content = c,
                                Err(_) => {
                                    all_ok = false;
                                    break;
                                }
                            }
                        }
                        if all_ok {
                            Ok(content)
                        } else {
                            Err("Failed to revert multi-patch".to_string())
                        }
                    }
                    crate::ai_workflow::ChangeType::NewFile { .. } => Ok(String::new()),
                    crate::ai_workflow::ChangeType::DeleteFile { old_content } => {
                        if let Some(content) = old_content {
                            Ok(content.clone())
                        } else {
                            Err("Cannot undo file deletion (content not saved)".to_string())
                        }
                    }
                };

                match revert_result {
                    Ok(reverted_content) => {
                        if let crate::ai_workflow::ChangeType::NewFile { .. } = change.change_type {
                            let _ = std::fs::remove_file(&full_path);
                        } else {
                            let _ = std::fs::write(&full_path, reverted_content);
                        }
                        let _ = window.emit(crate::events::event_names::REFRESH_EXPLORER, ());
                        let _ = window.emit(
                            crate::events::event_names::CHANGE_REJECTED,
                            crate::events::EditRejectedPayload {
                                edit_id: change.call.id.clone(),
                                file_path: change.path.clone(),
                            },
                        );
                    }
                    Err(e) => {
                        eprintln!("[REJECT] Failed to revert change: {}", e);
                    }
                }
            }
        }

        let mut batch_guard = state.pending_batch.lock().unwrap();
        if let Some(batch) = batch_guard.as_mut() {
            batch.file_results.push((
                change.call.clone(),
                crate::tools::ToolResult::err("User rejected change"),
            ));
        }
        drop(batch_guard);
        check_batch_completion(state);
        Ok(())
    } else {
        Err("Change not found".to_string())
    }
}

// Wrapper Commands

#[tauri::command]
pub async fn approve_change<R: Runtime>(
    change_id: String,
    window: tauri::Window<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    approve_change_logic(change_id, window, &*state).await
}

#[tauri::command]
pub async fn approve_changes_for_file<R: Runtime>(
    file_path: String,
    window: tauri::Window<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    approve_changes_for_file_logic(file_path, window, &*state).await
}

#[tauri::command]
pub async fn approve_all_changes(
    window: tauri::Window<Wry>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    approve_all_changes_logic(window, &*state).await
}

#[tauri::command]
pub async fn reject_change<R: Runtime>(
    change_id: String,
    window: tauri::Window<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    reject_change_logic(change_id, window, &*state).await
}
