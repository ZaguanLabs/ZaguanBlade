mod change_parser;
mod tool_defs;

use change_parser::parse_change_args;

use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;
use std::time::Instant;

// use eframe::egui; // Removed for Tauri migration

use crate::protocol::ToolCall;
use crate::tool_execution::ToolExecutionContext;
use crate::tools;
use tauri::Emitter;

pub use tool_defs::{get_tool_definitions, get_tool_definitions_for_model};

fn is_parallel_read_only_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "get_editor_state"
            | "symbol_search"
            | "symbol_resolve"
            | "symbol_related"
            | "symbol_outline"
            | "read_file"
            | "read_file_range"
            | "read_many_files"
            | "grep_search"
            | "rg"
            | "codebase_search"
            | "list_dir"
            | "list_directory"
            | "get_workspace_structure"
            | "find_files"
            | "find_files_glob"
            | "glob"
            | "get_file_info"
            | "get_project_index_overview"
            | "get_project_index_chunk"
            | "codebase_investigator"
    )
}

#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub command_line: Option<String>,
    pub program: Option<String>,
    pub args: Vec<String>,
    pub shell: bool,
}

impl CommandSpec {
    fn display_command(&self) -> String {
        if let Some(command_line) = &self.command_line {
            return command_line.clone();
        }

        if let Some(program) = &self.program {
            if self.args.is_empty() {
                return program.clone();
            }
            return format!("{} {}", program, self.args.join(" "));
        }

        String::new()
    }
}

#[derive(Clone)]
pub struct PendingCommand {
    pub call: ToolCall,
    pub command: String,
    pub command_spec: CommandSpec,
    pub cwd: Option<String>,
    pub blocking: bool,
    pub wait_ms_before_async: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::{parse_run_command_args, run_command_spec_in_workspace, CommandSpec};
    use crate::protocol::{ToolCall, ToolFunction};
    use crate::tool_execution::ToolExecutionContext;
    use tempfile::tempdir;

    fn tool_call(id: &str, name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            typ: "function".to_string(),
            function: ToolFunction {
                name: name.to_string(),
                arguments: arguments.to_string(),
            },
            status: None,
            result: None,
        }
    }

    #[test]
    fn parse_run_command_defaults_to_blocking() {
        let parsed = parse_run_command_args(r#"{"command":"echo test","cwd":"/tmp"}"#)
            .expect("should parse run_command args");

        assert_eq!(parsed.command, "echo test");
        assert_eq!(parsed.spec.command_line.as_deref(), Some("echo test"));
        assert!(parsed.spec.shell);
        assert_eq!(parsed.cwd.as_deref(), Some("/tmp"));
        assert!(parsed.blocking);
        assert!(parsed.wait_ms_before_async.is_none());
    }

    #[test]
    fn parse_run_command_respects_explicit_non_blocking() {
        let parsed = parse_run_command_args(
            r#"{"command":"bun run dev","cwd":"/repo","Blocking":false,"WaitMsBeforeAsync":2500}"#,
        )
        .expect("should parse run_command args");

        assert!(!parsed.blocking);
        assert_eq!(parsed.wait_ms_before_async, Some(2500));
    }

    #[test]
    fn parse_run_command_program_args_defaults_non_shell() {
        let parsed =
            parse_run_command_args(r#"{"program":"echo","args":["hello","world"],"cwd":"/tmp"}"#)
                .expect("should parse program/args command");

        assert_eq!(parsed.command, "echo hello world");
        assert_eq!(parsed.spec.program.as_deref(), Some("echo"));
        assert_eq!(
            parsed.spec.args,
            vec!["hello".to_string(), "world".to_string()]
        );
        assert!(!parsed.spec.shell);
    }

    #[test]
    fn run_command_preserves_large_output() {
        let workspace = tempdir().expect("tempdir");
        let command_spec = CommandSpec {
            command_line: None,
            program: Some("python3".to_string()),
            args: vec![
                "-c".to_string(),
                "print('a' * 60000 + 'TAIL_MARKER')".to_string(),
            ],
            shell: false,
        };

        let result = run_command_spec_in_workspace(workspace.path(), &command_spec, None);

        assert!(result.success, "large output command should succeed");
        assert!(result.content.contains("TAIL_MARKER"));
        assert!(!result.content.contains("...truncated..."));
        assert!(result.content.len() > 60_000);
    }

    #[test]
    fn apply_patch_invalidates_cached_read_file_results() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("example.txt");
        std::fs::write(&file_path, "before\n").expect("seed file");

        let context = ToolExecutionContext::<tauri::Wry>::new(
            Some(workspace.path().to_string_lossy().to_string()),
            None,
            Vec::new(),
            0,
            None,
            None,
            None,
            None,
            None,
        );

        let mut workflow = crate::ai_workflow::AiWorkflow::new();

        let read_before = workflow
            .handle_tool_calls(
                workspace.path(),
                vec![tool_call(
                    "read-1",
                    "read_file",
                    r#"{"path":"example.txt"}"#,
                )],
                Some("initial read".to_string()),
                &context,
            )
            .expect("batch for initial read");
        assert_eq!(read_before.file_results.len(), 1);
        assert!(read_before.file_results[0].1.content.contains("before"));

        let apply_batch = workflow
            .handle_tool_calls(
                workspace.path(),
                vec![tool_call(
                    "patch-1",
                    "apply_patch",
                    r#"{"path":"example.txt","old_text":"before","new_text":"after"}"#,
                )],
                Some("apply patch".to_string()),
                &context,
            )
            .expect("batch for patch");
        assert_eq!(apply_batch.file_results.len(), 1);
        assert!(apply_batch.file_results[0].1.success);

        let read_after = workflow
            .handle_tool_calls(
                workspace.path(),
                vec![tool_call(
                    "read-2",
                    "read_file",
                    r#"{"path":"example.txt"}"#,
                )],
                Some("read after patch".to_string()),
                &context,
            )
            .expect("batch for second read");
        assert_eq!(read_after.file_results.len(), 1);
        assert!(read_after.file_results[0].1.content.contains("after"));
        assert!(!read_after.file_results[0].1.content.contains("before\n"));
    }
}

#[derive(Clone, Debug)]
struct ParsedRunCommandArgs {
    command: String,
    spec: CommandSpec,
    cwd: Option<String>,
    blocking: bool,
    wait_ms_before_async: Option<u64>,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        start_line: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        end_line: Option<usize>,
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
    pub recent_history: VecDeque<(String, String)>, // (name, args)
    recent_file_tool_cache: VecDeque<((String, String), tools::ToolResult)>,
    last_assistant_content_fingerprint: Option<String>,
    stagnant_tool_turns: usize,
}

impl AiWorkflow {
    pub fn new() -> Self {
        Self {
            pending: None,
            recent_history: VecDeque::new(),
            recent_file_tool_cache: VecDeque::new(),
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
                        // Check if any of the calls are run_command - these must go through approval
                        let has_run_command =
                            calls.iter().any(|c| c.function.name == "run_command");

                        if has_run_command {
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
                                        skipped: false,
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

        struct PendingParallelTool<R: tauri::Runtime> {
            call: ToolCall,
            context: crate::tool_execution::ToolExecutionContext<R>,
        }
        let mut pending_parallel_tasks: Vec<PendingParallelTool<R>> = Vec::new();

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
                    self.recent_history.push_back(call_sig);
                    if self.recent_history.len() > 10 {
                        self.recent_history.pop_front();
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
                    loop_detected = true;
                    file_results.push((
                        call.clone(),
                        tools::ToolResult {
                            success: false,
                            content: String::new(),
                            error: Some("SYSTEM WARNING: LOOP DETECTED - You called this tool with identical arguments before. DO NOT call any more tools. Use the information from your previous tool calls to answer the user's question NOW.".to_string()),
                            skipped: false,
                        },
                    ));
                    continue;
                }
            }

            *seen_in_batch.entry(call_sig.clone()).or_insert(0) += 1;
            self.recent_history.push_back(call_sig.clone());
            if self.recent_history.len() > 10 {
                self.recent_history.pop_front();
            }

            // INTERCEPTION LOGIC
            if call.function.name == "run_command" {
                match parse_run_command_args(&call.function.arguments) {
                    Ok(parsed) => {
                        if let Some(err) = should_block_irrelevant_language_scan(
                            &parsed.command,
                            workspace_root,
                            parsed.cwd.as_deref(),
                        ) {
                            file_results.push((call.clone(), tools::ToolResult::err(err)));
                            continue;
                        }
                        commands.push(PendingCommand {
                            call: call.clone(),
                            command: parsed.command,
                            command_spec: parsed.spec,
                            cwd: parsed.cwd,
                            blocking: parsed.blocking,
                            wait_ms_before_async: parsed.wait_ms_before_async,
                        })
                    }
                    Err(e) => file_results.push((call.clone(), tools::ToolResult::err(e))),
                }
            } else if matches!(
                call.function.name.as_str(),
                "edit_file"
                    | "apply_edit"
                    | "apply_patch"
                    | "apply_patch_validated"
                    | "replace_file_content"
                    | "multi_replace_file_content"
                    | "write_file_validated"
                    | "write_file"
                    | "create_file"
                    | "write_to_file"
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

                        // Read original content before any changes (for diff generation)
                        let original_content = fs::read_to_string(&full_path).unwrap_or_default();

                        // History Snapshot - capture the snapshot ID for uncommitted tracking
                        let mut snapshot_id: Option<String> = None;
                        if let Some(app) = &context.app_handle {
                            use tauri::Manager;
                            let state = app.state::<crate::app_state::AppState>();
                            let history_service = state.history_service().ok();
                            if let Some(history_service) = history_service.as_ref() {
                                let snapshot = if full_path.exists() {
                                    history_service
                                        .create_snapshot(&full_path, Some(call.id.clone()))
                                } else {
                                    history_service.create_missing_file_snapshot(
                                        &full_path,
                                        Some(call.id.clone()),
                                    )
                                };
                                match snapshot {
                                    Ok(entry) => {
                                        println!("[HISTORY] Snapshot created for {}", change.path);
                                        snapshot_id = Some(entry.id.clone());
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
                            } else {
                                eprintln!(
                                    "[HISTORY] Failed to initialize history service for {}",
                                    change.path
                                );
                            }
                        }

                        let apply_result = (|| -> Result<(), String> {
                            match &change.change_type {
                                ChangeType::Patch {
                                    old_content,
                                    new_content,
                                    start_line,
                                    end_line,
                                } => {
                                    let current_content = fs::read_to_string(&full_path)
                                        .map_err(|e| format!("Failed to read file: {}", e))?;
                                    let new_file_content =
                                        tools::apply_patch_to_string_with_line_hint(
                                            &current_content,
                                            old_content,
                                            new_content,
                                            *start_line,
                                            *end_line,
                                        )?;
                                    fs::write(&full_path, new_file_content)
                                        .map_err(|e| format!("Failed to write file: {}", e))?;
                                    Ok(())
                                }
                                ChangeType::MultiPatch { patches } => {
                                    let mut content = fs::read_to_string(&full_path)
                                        .map_err(|e| format!("Failed to read file: {}", e))?;
                                    for (idx, patch) in patches.iter().enumerate() {
                                        content = tools::apply_patch_to_string_with_line_hint(
                                            &content,
                                            &patch.old_text,
                                            &patch.new_text,
                                            patch.start_line,
                                            patch.end_line,
                                        )
                                        .map_err(|e| {
                                            format!(
                                                "Patch {} failed (no changes made): {}",
                                                idx + 1,
                                                e
                                            )
                                        })?;
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
                                self.clear_recent_file_tool_cache();
                                if let Some(app) = &context.app_handle {
                                    use tauri::Manager;
                                    let state = app.state::<crate::app_state::AppState>();

                                    crate::file_state_sync::sync_from_disk_after_write(
                                        app, &full_path,
                                    );

                                    // Track as uncommitted change if we have a snapshot
                                    if let Some(snap_id) = &snapshot_id {
                                        let existing =
                                            state.uncommitted_changes.get_by_path(&full_path);

                                        // Preserve the earliest snapshot baseline for a file so
                                        // repeated edits to the same file are represented as one
                                        // cumulative diff (instead of only the most recent edit).
                                        let (change_id, base_snapshot_id, base_content) =
                                            if let Some(existing_change) = existing {
                                                let baseline = state
                                                    .history_service()
                                                    .ok()
                                                    .and_then(|history_service| {
                                                        history_service
                                                            .get_snapshot_content(
                                                                &existing_change.snapshot_id,
                                                            )
                                                            .ok()
                                                    })
                                                    .unwrap_or_else(|| original_content.clone());
                                                (
                                                    existing_change.id,
                                                    existing_change.snapshot_id,
                                                    baseline,
                                                )
                                            } else {
                                                (
                                                    call.id.clone(),
                                                    snap_id.clone(),
                                                    original_content.clone(),
                                                )
                                            };

                                        // Read new content for diff
                                        let new_content =
                                            fs::read_to_string(&full_path).unwrap_or_default();
                                        let diff =
                                            crate::uncommitted_changes::generate_unified_diff(
                                                &base_content,
                                                &new_content,
                                            );
                                        let (added, removed) =
                                            crate::uncommitted_changes::count_diff_stats(&diff);

                                        let uncommitted =
                                            crate::uncommitted_changes::UncommittedChange {
                                                id: change_id,
                                                file_path: full_path.clone(),
                                                snapshot_id: base_snapshot_id,
                                                unified_diff: diff,
                                                added_lines: added,
                                                removed_lines: removed,
                                                timestamp: std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_millis()
                                                    as u64,
                                                file_modified_ms:
                                                    crate::uncommitted_changes::file_modified_ms(
                                                        &full_path,
                                                    ),
                                            };
                                        state.uncommitted_changes.track(uncommitted);
                                        println!(
                                            "[UNCOMMITTED] Tracking cumulative change for {}",
                                            change.path
                                        );
                                    }

                                    let abs_path_str = full_path.to_string_lossy().to_string();
                                    crate::blade_event_scheduler::queue_refresh_explorer(app);
                                    // Emit open-file to open the file in editor
                                    let _ = app.emit("open-file", &abs_path_str);
                                    let _ = app.emit(
                                        crate::events::event_names::CHANGE_APPLIED,
                                        crate::events::ChangeAppliedPayload {
                                            change_id: call.id.clone(),
                                            file_path: abs_path_str.clone(),
                                            file_paths: vec![abs_path_str.clone()],
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
                            let history_service = state.history_service().ok();
                            if full_path.exists() {
                                match history_service.as_ref().map(|service| {
                                    service.create_snapshot(&full_path, Some(call.id.clone()))
                                }) {
                                    Some(Ok(entry)) => {
                                        println!("[HISTORY] Snapshot created for {}", change.path);
                                        let _ = app.emit(
                                            crate::events::event_names::HISTORY_ENTRY_ADDED,
                                            crate::events::HistoryEntryAddedPayload { entry },
                                        );
                                    }
                                    Some(Err(e)) => {
                                        eprintln!(
                                            "[HISTORY] Failed to create snapshot for {}: {}",
                                            change.path, e
                                        );
                                    }
                                    None => {
                                        eprintln!(
                                            "[HISTORY] Failed to initialize history service for {}",
                                            change.path
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
                                self.clear_recent_file_tool_cache();
                                if let Some(app) = &context.app_handle {
                                    crate::file_state_sync::sync_after_delete(app, &full_path);
                                    crate::blade_event_scheduler::queue_refresh_explorer(app);
                                    let _ = app.emit(
                                        crate::events::event_names::CHANGE_APPLIED,
                                        crate::events::ChangeAppliedPayload {
                                            change_id: call.id.clone(),
                                            file_path: change.path.clone(),
                                            file_paths: vec![change.path.clone()],
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
            } else if is_parallel_read_only_tool(&call.function.name) {
                // Defer read-only tools to run in parallel outside the main loop.
                // zcoderd waits for the full client-side tool batch before continuing,
                // so serializing these calls creates large dead gaps between model turns.
                let ctx = crate::tool_execution::ToolExecutionContext {
                    workspace_root: context.workspace_root.clone(),
                    active_file: context.active_file.clone(),
                    open_files: context.open_files.clone(),
                    active_tab_index: context.active_tab_index,
                    cursor_line: context.cursor_line,
                    cursor_column: context.cursor_column,
                    selection_start_line: context.selection_start_line,
                    selection_end_line: context.selection_end_line,
                    app_handle: context.app_handle.clone(),
                };
                pending_parallel_tasks.push(PendingParallelTool {
                    call: call.clone(),
                    context: ctx,
                });
            } else {
                let started_at = Instant::now();
                eprintln!(
                    "[AI TOOL LATENCY] start serial tool={} call_id={}",
                    call.function.name, call.id
                );
                let res = crate::tool_execution::execute_tool_with_default_timeout(
                    context,
                    &call.function.name,
                    &call.function.arguments,
                );
                eprintln!(
                    "[AI TOOL LATENCY] finish serial tool={} call_id={} duration_ms={} success={}",
                    call.function.name,
                    call.id,
                    started_at.elapsed().as_millis(),
                    res.success
                );

                if res.success
                    && matches!(call.function.name.as_str(), "read_file" | "read_file_range")
                {
                    self.recent_file_tool_cache
                        .push_back((call_sig.clone(), res.clone()));
                    if self.recent_file_tool_cache.len() > 10 {
                        self.recent_file_tool_cache.pop_front();
                    }
                }
                file_results.push((call.clone(), res));
            }
        }

        // Execute read-only tasks in parallel threads.
        if !pending_parallel_tasks.is_empty() {
            let mut handles = Vec::new();
            for task in pending_parallel_tasks {
                handles.push(std::thread::spawn(move || {
                    let started_at = Instant::now();
                    let call = task.call;
                    eprintln!(
                        "[AI TOOL LATENCY] start parallel tool={} call_id={}",
                        call.function.name, call.id
                    );
                    let res = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::tool_execution::execute_tool_with_default_timeout(
                            &task.context,
                            &call.function.name,
                            &call.function.arguments,
                        )
                    })) {
                        Ok(res) => res,
                        Err(_) => tools::ToolResult::err(format!(
                            "tool '{}' execution panicked before returning a result",
                            call.function.name
                        )),
                    };
                    eprintln!(
                        "[AI TOOL LATENCY] finish parallel tool={} call_id={} duration_ms={} success={}",
                        call.function.name,
                        call.id,
                        started_at.elapsed().as_millis(),
                        res.success
                    );
                    (call, res)
                }));
            }
            for handle in handles {
                match handle.join() {
                    Ok((call, res)) => {
                        file_results.push((call.clone(), res.clone()));
                        if res.success
                            && matches!(
                                call.function.name.as_str(),
                                "read_file" | "read_file_range"
                            )
                        {
                            self.recent_file_tool_cache.push_back((
                                (
                                    call.function.name.clone(),
                                    normalize_json_string(&call.function.arguments),
                                ),
                                res.clone(),
                            ));
                            if self.recent_file_tool_cache.len() > 10 {
                                self.recent_file_tool_cache.pop_front();
                            }
                        }
                    }
                    Err(e) => {
                        let _ = e;
                        // Thread panicked - this shouldn't happen but handle it gracefully
                        // The tool call will be missing from results, which will cause an error downstream
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

    pub fn take_pending(&mut self) -> Option<PendingToolBatch> {
        self.pending.take()
    }

    pub fn clear_history(&mut self) {
        self.recent_history.clear();
        self.recent_file_tool_cache.clear();
        self.last_assistant_content_fingerprint = None;
        self.stagnant_tool_turns = 0;
    }

    pub fn clear_recent_file_tool_cache(&mut self) {
        self.recent_file_tool_cache.clear();
    }
}

fn parse_run_command_args(raw_args: &str) -> Result<ParsedRunCommandArgs, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw_args).map_err(|e| format!("invalid tool args json: {e}"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| "invalid args: expected object".to_string())?;

    let command_line = obj
        .get("command")
        .or_else(|| obj.get("Command"))
        .or_else(|| obj.get("command_line"))
        .or_else(|| obj.get("CommandLine"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    let program = obj
        .get("program")
        .or_else(|| obj.get("Program"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    let args = obj
        .get("args")
        .or_else(|| obj.get("Args"))
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToString::to_string))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let explicit_shell = obj
        .get("shell")
        .or_else(|| obj.get("Shell"))
        .and_then(|v| match v {
            Value::Bool(b) => Some(*b),
            Value::String(s) => s.parse::<bool>().ok(),
            _ => None,
        });

    let spec = if program.is_some() {
        CommandSpec {
            command_line: None,
            program,
            args,
            shell: explicit_shell.unwrap_or(false),
        }
    } else if let Some(command_line) = command_line {
        CommandSpec {
            command_line: Some(command_line),
            program: None,
            args: Vec::new(),
            shell: true,
        }
    } else {
        return Err("missing required arg: command/CommandLine or program".to_string());
    };

    let command = spec.display_command();
    if command.trim().is_empty() {
        return Err("run_command resolved to an empty command".to_string());
    }

    let cwd = obj
        .get("cwd")
        .or_else(|| obj.get("Cwd"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let blocking = obj
        .get("blocking")
        .or_else(|| obj.get("Blocking"))
        .and_then(|v| match v {
            Value::Bool(b) => Some(*b),
            Value::String(s) => s.parse::<bool>().ok(),
            _ => None,
        })
        .unwrap_or(true);

    let wait_ms_before_async = obj
        .get("wait_ms_before_async")
        .or_else(|| obj.get("WaitMsBeforeAsync"))
        .and_then(|v| match v {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.parse::<u64>().ok(),
            _ => None,
        });

    Ok(ParsedRunCommandArgs {
        command,
        spec,
        cwd,
        blocking,
        wait_ms_before_async,
    })
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

#[allow(clippy::disallowed_methods, clippy::disallowed_types)]
pub fn run_command_spec_in_workspace(
    workspace_root: &Path,
    command_spec: &CommandSpec,
    cwd: Option<&str>,
) -> tools::ToolResult {
    let ws = match fs::canonicalize(workspace_root) {
        Ok(p) => p,
        Err(e) => {
            return tools::ToolResult {
                success: false,
                content: String::new(),
                error: Some(e.to_string()),
                skipped: false,
            };
        }
    };

    let dir = if let Some(cwd) = cwd {
        let p = Path::new(cwd);
        // Handle relative paths by joining with workspace root
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
                    error: Some(format!(
                        "cwd does not exist or is inaccessible: {} ({})",
                        candidate.display(),
                        e
                    )),
                    skipped: false,
                };
            }
        };
        if !candidate.starts_with(&ws) {
            return tools::ToolResult {
                success: false,
                content: String::new(),
                error: Some(format!(
                    "cwd is outside workspace (workspace: {}, cwd: {})",
                    ws.display(),
                    candidate.display()
                )),
                skipped: false,
            };
        }
        candidate
    } else {
        ws.clone()
    };

    let output = if !command_spec.shell {
        if let Some(program) = &command_spec.program {
            let mut cmd = std::process::Command::new(program);
            cmd.args(&command_spec.args)
                .current_dir(&dir)
                .env_remove("ARGV0")
                .env_remove("APPIMAGE")
                .env_remove("APPDIR")
                .env_remove("OWD")
                .output()
        } else {
            return tools::ToolResult {
                success: false,
                content: String::new(),
                error: Some("non-shell command requires 'program'".to_string()),
                skipped: false,
            };
        }
    } else if let Some(command_line) = &command_spec.command_line {
        std::process::Command::new("sh")
            .arg("-lc")
            .arg(command_line)
            .current_dir(&dir)
            .env_remove("ARGV0")
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .env_remove("OWD")
            .output()
    } else if let Some(program) = &command_spec.program {
        let mut cmd = std::process::Command::new(program);
        cmd.args(&command_spec.args)
            .current_dir(&dir)
            .env_remove("ARGV0")
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .env_remove("OWD")
            .output()
    } else {
        return tools::ToolResult {
            success: false,
            content: String::new(),
            error: Some("missing command payload".to_string()),
            skipped: false,
        };
    };

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
            tools::ToolResult {
                success: out.status.success(),
                content: s,
                error: None,
                skipped: false,
            }
        }
        Err(e) => tools::ToolResult {
            success: false,
            content: String::new(),
            error: Some(e.to_string()),
            skipped: false,
        },
    }
}
