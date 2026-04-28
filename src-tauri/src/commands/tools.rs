use crate::app_state::AppState;
use crate::events;
use crate::utils::extract_root_command;
use crate::workflow_controller::check_batch_completion;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{Emitter, Manager, Runtime, State};

fn ansi_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?x)
        \x1b\[[0-9;?]*[A-Za-z]|           # CSI sequences (colors, cursor, etc.)
        \x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|# OSC sequences
        \x1b[PX^_][^\x1b]*\x1b\\|         # DCS, SOS, PM, APC sequences
        \x1b[\x20-\x2f]*[\x30-\x7e]       # Other escape sequences
        ",
        )
        .expect("valid ANSI regex")
    })
}

fn blade_marker_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?x)
        printf\s+%s\s+\$'[^']*BLADE_CMD[^']*'[;\s]*|                           # Start marker printf
        __e=\$\?;\s*printf\s+'%s%s'\s+\$'[^']*BLADE_CMD[^']*'\s+\x22\$__e\x22;\s*printf\s+\$'[^']*'  # Exit marker printf
        "
        )
        .expect("valid BLADE marker regex")
    })
}

fn control_char_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]").expect("valid control-char regex")
    })
}

fn orphan_csi_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\[([0-9;?]*)([A-Za-z])").expect("valid orphan CSI regex"))
}

fn cleanup_newlines_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\n{3,}").expect("valid cleanup regex"))
}

/// Strip ANSI escape codes and BLADE command scaffolding from terminal output
/// for clean display in chat and AI context.
fn strip_ansi_codes(input: &str) -> String {
    // 1. Strip ANSI escape sequences
    let result = ansi_regex().replace_all(input, "").to_string();

    // 2. Strip BLADE command marker scaffolding (from shell echo)
    let result = blade_marker_regex().replace_all(&result, "").to_string();

    // 3. Strip stray control characters (keep \n, \r, \t)
    let result = control_char_regex().replace_all(&result, "").to_string();

    // 4. Strip any remaining bare ESC bytes (all known sequences already stripped above)
    let result = result.replace('\x1b', "");

    // 5. Strip orphaned CSI bracket sequences where ESC byte is already gone
    //    These look like: [0m, [1m, [38;5;4m, [0;1m, [38;5;2m, [39;49m etc.
    let result = orphan_csi_regex()
        .replace_all(&result, |caps: &regex::Captures| {
            let params = caps.get(1).map_or("", |m| m.as_str());
            // Only strip if params are purely digits/semicolons/question marks (ANSI CSI params)
            if params
                .chars()
                .all(|c| c.is_ascii_digit() || c == ';' || c == '?')
            {
                String::new()
            } else {
                caps[0].to_string()
            }
        })
        .to_string();

    // 6. Strip BEL characters
    let result = result.replace('\x07', "");

    // 7. Clean up excessive blank lines
    cleanup_newlines_regex()
        .replace_all(&result, "\n\n")
        .trim()
        .to_string()
}

fn resolve_command_dir(
    workspace_root: &Path,
    cwd: Option<&str>,
) -> Result<(PathBuf, PathBuf), String> {
    let ws = fs::canonicalize(workspace_root).map_err(|e| e.to_string())?;

    let dir = if let Some(cwd) = cwd {
        let p = Path::new(cwd);
        let candidate = if p.is_absolute() {
            p.to_path_buf()
        } else {
            ws.join(p)
        };
        let candidate = fs::canonicalize(&candidate).map_err(|e| {
            format!(
                "cwd does not exist or is inaccessible: {} ({})",
                candidate.display(),
                e
            )
        })?;
        if !candidate.starts_with(&ws) {
            return Err(format!(
                "cwd is outside workspace (workspace: {}, cwd: {})",
                ws.display(),
                candidate.display()
            ));
        }
        candidate
    } else {
        ws.clone()
    };

    Ok((ws, dir))
}

fn resolve_existing_workspace_path(
    workspace_root: &Path,
    base_dir: &Path,
    raw_path: &str,
) -> Result<PathBuf, String> {
    let raw = Path::new(raw_path);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        base_dir.join(raw)
    };
    let resolved = fs::canonicalize(&candidate).map_err(|e| {
        format!(
            "path does not exist or is inaccessible: {} ({})",
            candidate.display(),
            e
        )
    })?;
    if !resolved.starts_with(workspace_root) {
        return Err(format!(
            "path is outside workspace (workspace: {}, path: {})",
            workspace_root.display(),
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn resolve_workspace_path_allow_missing(
    workspace_root: &Path,
    base_dir: &Path,
    raw_path: &str,
) -> Result<PathBuf, String> {
    if raw_path.is_empty() {
        return Err("path cannot be empty".to_string());
    }

    let raw = Path::new(raw_path);
    let candidate = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        base_dir.join(raw)
    };

    if candidate.exists() {
        return resolve_existing_workspace_path(workspace_root, base_dir, raw_path);
    }

    let mut existing_ancestor = candidate.as_path();
    while !existing_ancestor.exists() {
        existing_ancestor = existing_ancestor.parent().ok_or_else(|| {
            format!(
                "path is outside workspace or has no existing parent: {}",
                candidate.display()
            )
        })?;
    }

    let resolved_ancestor = fs::canonicalize(existing_ancestor).map_err(|e| {
        format!(
            "path does not exist or is inaccessible: {} ({})",
            existing_ancestor.display(),
            e
        )
    })?;
    if !resolved_ancestor.starts_with(workspace_root) {
        return Err(format!(
            "path is outside workspace (workspace: {}, path: {})",
            workspace_root.display(),
            candidate.display()
        ));
    }

    Ok(candidate)
}

fn build_builtin_result(
    output: String,
    exit_code: i32,
    refresh_explorer: bool,
) -> (crate::tools::ToolResult, String, i32, bool) {
    let content = output.trim_end_matches('\n').to_string();
    let result = if exit_code == 0 {
        crate::tools::ToolResult::ok(content)
    } else {
        crate::tools::ToolResult::err(if content.is_empty() {
            format!("command failed with exit code {}", exit_code)
        } else {
            content
        })
    };
    (
        result,
        output,
        exit_code,
        refresh_explorer && exit_code == 0,
    )
}

fn copy_directory_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest)
        .map_err(|e| format!("failed to create directory {} ({})", dest.display(), e))?;

    let read_dir = fs::read_dir(src)
        .map_err(|e| format!("failed to read directory {} ({})", src.display(), e))?;
    for entry in read_dir {
        let entry = entry.map_err(|e| {
            format!(
                "failed to read directory entry in {} ({})",
                src.display(),
                e
            )
        })?;
        let source_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry.file_type().map_err(|e| {
            format!(
                "failed to read file type for {} ({})",
                source_path.display(),
                e
            )
        })?;

        if file_type.is_dir() {
            copy_directory_recursive(&source_path, &dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!("failed to create directory {} ({})", parent.display(), e)
                })?;
            }
            fs::copy(&source_path, &dest_path).map_err(|e| {
                format!(
                    "failed to copy {} to {} ({})",
                    source_path.display(),
                    dest_path.display(),
                    e
                )
            })?;
        }
    }

    Ok(())
}

fn path_is_same_or_descendant(path: &Path, candidate: &Path) -> bool {
    candidate == path || candidate.starts_with(path)
}

fn parse_rm_flags(args: &[String]) -> Option<(bool, bool, Vec<String>)> {
    let mut recursive = false;
    let mut force = false;
    let mut paths = Vec::new();

    for arg in args {
        if arg.starts_with('-') {
            if arg == "-" {
                return None;
            }
            for flag in arg.chars().skip(1) {
                match flag {
                    'r' | 'R' => recursive = true,
                    'f' => force = true,
                    _ => return None,
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    if paths.is_empty() {
        return None;
    }

    Some((recursive, force, paths))
}

fn parse_simple_builtin_command(command: &str) -> Option<(String, Vec<String>)> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().any(|c| {
        matches!(
            c,
            '|' | '&'
                | ';'
                | '>'
                | '<'
                | '$'
                | '('
                | ')'
                | '{'
                | '}'
                | '`'
                | '\''
                | '"'
                | '\n'
                | '\r'
        )
    }) {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let program = parts.next()?.to_string();
    let args = parts.map(ToString::to_string).collect::<Vec<_>>();
    Some((program, args))
}

fn try_execute_builtin_command(
    cmd: &crate::ai_workflow::PendingCommand,
    workspace_root: &Path,
) -> Option<(crate::tools::ToolResult, String, i32, bool)> {
    let (program, args) = if !cmd.command_spec.shell {
        (
            cmd.command_spec.program.clone()?,
            cmd.command_spec.args.clone(),
        )
    } else {
        parse_simple_builtin_command(&cmd.command)?
    };

    let (workspace_root, dir) = match resolve_command_dir(workspace_root, cmd.cwd.as_deref()) {
        Ok(paths) => paths,
        Err(err) => return Some(build_builtin_result(err, 1, false)),
    };

    match program.as_str() {
        "pwd" => {
            if !args.is_empty() {
                return Some(build_builtin_result(
                    "pwd built-in does not support arguments".to_string(),
                    2,
                    false,
                ));
            }
            let mut output = dir.display().to_string();
            output.push('\n');
            Some(build_builtin_result(output, 0, false))
        }
        "ls" => {
            if args.iter().any(|arg| arg.starts_with('-')) || args.len() > 1 {
                return None;
            }
            let target = match args.first() {
                Some(path) => match resolve_existing_workspace_path(&workspace_root, &dir, path) {
                    Ok(path) => path,
                    Err(err) => return Some(build_builtin_result(err, 1, false)),
                },
                _ => dir.clone(),
            };

            let mut lines = if target.is_dir() {
                let mut entries = Vec::new();
                let read_dir = match fs::read_dir(&target) {
                    Ok(read_dir) => read_dir,
                    Err(err) => {
                        let message =
                            format!("failed to list directory {} ({})", target.display(), err);
                        return Some(build_builtin_result(message, 1, false));
                    }
                };
                for entry in read_dir.filter_map(Result::ok) {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    if file_name.starts_with('.') {
                        continue;
                    }
                    let suffix = match entry.file_type() {
                        Ok(file_type) if file_type.is_dir() => "/",
                        _ => "",
                    };
                    entries.push(format!("{}{}", file_name, suffix));
                }
                entries.sort();
                entries
            } else {
                vec![target
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| target.display().to_string())]
            };

            let output = if lines.is_empty() {
                String::new()
            } else {
                lines.push(String::new());
                lines.join("\n")
            };
            Some(build_builtin_result(output, 0, false))
        }
        "cat" => {
            if args.is_empty() || args.iter().any(|arg| arg.starts_with('-')) {
                return None;
            }

            let mut output = String::new();
            for path in &args {
                let resolved = match resolve_existing_workspace_path(&workspace_root, &dir, path) {
                    Ok(path) => path,
                    Err(err) => return Some(build_builtin_result(err, 1, false)),
                };
                if resolved.is_dir() {
                    let message = format!("{} is a directory", resolved.display());
                    return Some(build_builtin_result(message, 1, false));
                }
                let bytes = match fs::read(&resolved) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        let message =
                            format!("failed to read file {} ({})", resolved.display(), err);
                        return Some(build_builtin_result(message, 1, false));
                    }
                };
                output.push_str(&String::from_utf8_lossy(&bytes));
                if output.len() > 50_000 {
                    break;
                }
            }

            Some(build_builtin_result(output, 0, false))
        }
        "mkdir" => {
            let mut create_parents = false;
            let mut paths = Vec::new();
            for arg in &args {
                if arg == "-p" {
                    create_parents = true;
                } else if arg.starts_with('-') {
                    return None;
                } else {
                    paths.push(arg.clone());
                }
            }
            if paths.is_empty() {
                return None;
            }

            let mut output = String::new();
            for path in paths {
                let target =
                    match resolve_workspace_path_allow_missing(&workspace_root, &dir, &path) {
                        Ok(path) => path,
                        Err(err) => return Some(build_builtin_result(err, 1, false)),
                    };
                let result = if create_parents {
                    fs::create_dir_all(&target)
                } else {
                    fs::create_dir(&target)
                };
                if let Err(err) = result {
                    let message =
                        format!("failed to create directory {} ({})", target.display(), err);
                    return Some(build_builtin_result(message, 1, false));
                }
                output.push_str(&format!("created directory {}\n", target.display()));
            }

            Some(build_builtin_result(output, 0, true))
        }
        "cp" => {
            let mut recursive = false;
            let mut paths = Vec::new();
            for arg in &args {
                if arg == "-r" || arg == "-R" {
                    recursive = true;
                } else if arg.starts_with('-') {
                    return None;
                } else {
                    paths.push(arg.clone());
                }
            }
            if paths.len() != 2 {
                return None;
            }

            let source = match resolve_existing_workspace_path(&workspace_root, &dir, &paths[0]) {
                Ok(path) => path,
                Err(err) => return Some(build_builtin_result(err, 1, false)),
            };
            let dest = match resolve_workspace_path_allow_missing(&workspace_root, &dir, &paths[1])
            {
                Ok(path) => path,
                Err(err) => return Some(build_builtin_result(err, 1, false)),
            };

            let final_dest = if dest.exists() && dest.is_dir() {
                let Some(file_name) = source.file_name() else {
                    return Some(build_builtin_result(
                        "cp source has no file name".to_string(),
                        1,
                        false,
                    ));
                };
                dest.join(file_name)
            } else {
                dest.clone()
            };

            if final_dest == source {
                let message = format!("source and destination are the same: {}", source.display());
                return Some(build_builtin_result(message, 1, false));
            }

            if source.is_dir() {
                if !recursive {
                    return None;
                }
                if path_is_same_or_descendant(&source, &final_dest) {
                    let message = format!(
                        "cannot copy directory {} into itself ({})",
                        source.display(),
                        final_dest.display()
                    );
                    return Some(build_builtin_result(message, 1, false));
                }
                if final_dest.exists() && final_dest.is_file() {
                    let message = format!(
                        "cannot overwrite file {} with directory {}",
                        final_dest.display(),
                        source.display()
                    );
                    return Some(build_builtin_result(message, 1, false));
                }
                if let Err(err) = copy_directory_recursive(&source, &final_dest) {
                    return Some(build_builtin_result(err, 1, false));
                }
            } else {
                if let Some(parent) = final_dest.parent() {
                    if let Err(err) = fs::create_dir_all(parent) {
                        let message =
                            format!("failed to create directory {} ({})", parent.display(), err);
                        return Some(build_builtin_result(message, 1, false));
                    }
                }
                if let Err(err) = fs::copy(&source, &final_dest) {
                    let message = format!(
                        "failed to copy {} to {} ({})",
                        source.display(),
                        final_dest.display(),
                        err
                    );
                    return Some(build_builtin_result(message, 1, false));
                }
            }

            Some(build_builtin_result(
                format!("copied {} -> {}\n", source.display(), final_dest.display()),
                0,
                true,
            ))
        }
        "mv" => {
            if args.len() != 2 || args.iter().any(|arg| arg.starts_with('-')) {
                return None;
            }

            let source = match resolve_existing_workspace_path(&workspace_root, &dir, &args[0]) {
                Ok(path) => path,
                Err(err) => return Some(build_builtin_result(err, 1, false)),
            };
            let dest = match resolve_workspace_path_allow_missing(&workspace_root, &dir, &args[1]) {
                Ok(path) => path,
                Err(err) => return Some(build_builtin_result(err, 1, false)),
            };
            let final_dest = if dest.exists() && dest.is_dir() {
                let Some(file_name) = source.file_name() else {
                    return Some(build_builtin_result(
                        "mv source has no file name".to_string(),
                        1,
                        false,
                    ));
                };
                dest.join(file_name)
            } else {
                dest.clone()
            };

            if final_dest == source {
                let message = format!("source and destination are the same: {}", source.display());
                return Some(build_builtin_result(message, 1, false));
            }
            if source.is_dir() && path_is_same_or_descendant(&source, &final_dest) {
                let message = format!(
                    "cannot move directory {} into itself ({})",
                    source.display(),
                    final_dest.display()
                );
                return Some(build_builtin_result(message, 1, false));
            }

            if let Some(parent) = final_dest.parent() {
                if let Err(err) = fs::create_dir_all(parent) {
                    let message =
                        format!("failed to create directory {} ({})", parent.display(), err);
                    return Some(build_builtin_result(message, 1, false));
                }
            }
            if let Err(err) = fs::rename(&source, &final_dest) {
                let message = format!(
                    "failed to move {} to {} ({})",
                    source.display(),
                    final_dest.display(),
                    err
                );
                return Some(build_builtin_result(message, 1, false));
            }

            Some(build_builtin_result(
                format!("moved {} -> {}\n", source.display(), final_dest.display()),
                0,
                true,
            ))
        }
        "rm" => {
            let Some((recursive, force, paths)) = parse_rm_flags(&args) else {
                return None;
            };

            let mut output = String::new();
            for path in paths {
                let target =
                    match resolve_workspace_path_allow_missing(&workspace_root, &dir, &path) {
                        Ok(path) => path,
                        Err(err) => return Some(build_builtin_result(err, 1, false)),
                    };

                if !target.exists() {
                    if force {
                        continue;
                    }
                    let message = format!("path does not exist: {}", target.display());
                    return Some(build_builtin_result(message, 1, false));
                }

                let target = match fs::canonicalize(&target) {
                    Ok(path) => path,
                    Err(err) => {
                        let message = format!(
                            "path does not exist or is inaccessible: {} ({})",
                            target.display(),
                            err
                        );
                        return Some(build_builtin_result(message, 1, false));
                    }
                };

                if target == workspace_root {
                    let message =
                        format!("refusing to remove workspace root: {}", target.display());
                    return Some(build_builtin_result(message, 1, false));
                }

                let result = if target.is_dir() {
                    if !recursive {
                        let message = format!(
                            "{} is a directory (use -r to remove directories)",
                            target.display()
                        );
                        return Some(build_builtin_result(message, 1, false));
                    }
                    fs::remove_dir_all(&target)
                } else {
                    fs::remove_file(&target)
                };

                if let Err(err) = result {
                    let message = format!("failed to remove {} ({})", target.display(), err);
                    return Some(build_builtin_result(message, 1, false));
                }
                output.push_str(&format!("removed {}\n", target.display()));
            }

            Some(build_builtin_result(output, 0, true))
        }
        _ => None,
    }
}

fn complete_command_immediately<R: Runtime>(
    app_handle: &tauri::AppHandle<R>,
    batch: &mut crate::ai_workflow::PendingToolBatch,
    cmd: &crate::ai_workflow::PendingCommand,
    result: crate::tools::ToolResult,
    output: String,
    exit_code: i32,
    refresh_explorer: bool,
) {
    let success = result.success;
    let skipped = result.skipped;
    batch.file_results.push((cmd.call.clone(), result));
    let _ = app_handle.emit(
        events::event_names::COMMAND_EXECUTED,
        events::CommandExecutedPayload {
            command: cmd.command.clone(),
            cwd: cmd.cwd.clone(),
            output,
            exit_code,
            duration: None,
            call_id: cmd.call.id.clone(),
        },
    );
    let _ = app_handle.emit(
        events::event_names::TOOL_EXECUTION_COMPLETED,
        events::ToolExecutionCompletedPayload {
            tool_name: "run_command".to_string(),
            tool_call_id: cmd.call.id.clone(),
            success,
            skipped,
        },
    );
    if refresh_explorer {
        crate::blade_event_scheduler::queue_refresh_explorer(app_handle);
    }
}

// #[tauri::command]
fn approve_tool<R: Runtime>(approved: bool, app_handle: tauri::AppHandle<R>) {
    let state = app_handle.state::<AppState>();
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
                        if let Some(workspace_root) = ws_root.as_deref() {
                            if let Some((result, output, exit_code, refresh_explorer)) =
                                try_execute_builtin_command(&cmd, Path::new(workspace_root))
                            {
                                complete_command_immediately(
                                    &app_handle,
                                    batch,
                                    &cmd,
                                    result,
                                    output,
                                    exit_code,
                                    refresh_explorer,
                                );
                                continue;
                            }
                        }

                        let command_id = format!("cmd-{}", cmd.call.id);
                        eprintln!(
                            "[COMMAND EXEC] Emitting command-execution-started for: {}",
                            cmd.command
                        );
                        let _ = app_handle.emit(
                            crate::events::event_names::COMMAND_EXECUTION_STARTED,
                            crate::events::CommandExecutionStartedPayload {
                                command_id,
                                call_id: cmd.call.id.clone(),
                                command: cmd.command.clone(),
                                program: cmd.command_spec.program.clone(),
                                args: if cmd.command_spec.args.is_empty() {
                                    None
                                } else {
                                    Some(cmd.command_spec.args.clone())
                                },
                                shell: Some(cmd.command_spec.shell),
                                cwd: cmd.cwd.clone(),
                                blocking: cmd.blocking,
                                wait_ms_before_async: cmd.wait_ms_before_async,
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
                            crate::blade_event_scheduler::queue_refresh_explorer(&app_handle);
                        }
                        batch.file_results.push((conf.call.clone(), res));
                    }
                }
            } else {
                eprintln!("[APPROVAL] User SKIPPED - NOT executing commands");
                // Skipped - add explicit error results with clear instruction not to retry
                for cmd in &batch.commands {
                    if !batch.file_results.iter().any(|(c, _)| c.id == cmd.call.id) {
                        eprintln!("[SKIP] Adding skipped result for command: {}", cmd.command);
                        let skip_msg = format!(
                            "User skipped this command: '{}'. Do NOT retry this command or similar commands. Ask the user how they would like to proceed instead.",
                            cmd.command
                        );
                        batch.file_results.push((
                            cmd.call.clone(),
                            crate::tools::ToolResult::skipped(&skip_msg),
                        ));
                    }
                }
                for conf in &batch.confirms {
                    if !batch.file_results.iter().any(|(c, _)| c.id == conf.call.id) {
                        eprintln!(
                            "[SKIP] Adding skipped result for action: {}",
                            conf.description
                        );
                        let skip_msg = format!(
                            "User skipped this action: '{}'. Do NOT retry this action. Ask the user how they would like to proceed instead.",
                            conf.description
                        );
                        batch.file_results.push((
                            conf.call.clone(),
                            crate::tools::ToolResult::skipped(&skip_msg),
                        ));
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
pub async fn approve_tool_decision<R: Runtime>(decision: String, app_handle: tauri::AppHandle<R>) {
    if let Err(error) = tokio::task::spawn_blocking(move || {
        let approved = decision == "approve_once" || decision == "approve_always";
        let state = app_handle.state::<AppState>();

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

        approve_tool(approved, app_handle);
    })
    .await
    {
        eprintln!("[APPROVAL] approve_tool_decision task failed: {}", error);
    }
}

#[tauri::command]
pub async fn send_approval_response(
    session_id: String,
    approval_id: String,
    tool_call_id: String,
    approved: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .ws_connection
        .send_approval_response(session_id, approval_id, tool_call_id, approved)
        .await
}

/// Approve or skip a single command by its call_id
/// This allows individual command approval instead of batch-only
#[tauri::command]
pub async fn approve_single_command<R: Runtime>(
    call_id: String,
    approved: bool,
    app_handle: tauri::AppHandle<R>,
) {
    if let Err(error) = tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let ws_root = {
            let ws = state.workspace.lock().unwrap();
            ws.workspace
                .clone()
                .map(|p| p.to_string_lossy().to_string())
        };

        {
            let mut batch_guard = state.pending_batch.lock().unwrap();
            if let Some(batch) = batch_guard.as_mut() {
                // Find the command by call_id
                if let Some(cmd) = batch
                    .commands
                    .iter()
                    .find(|c| c.call.id == call_id)
                    .cloned()
                {
                    // Check if result already exists
                    if !batch.file_results.iter().any(|(c, _)| c.id == call_id) {
                        if approved {
                            if let Some(workspace_root) = ws_root.as_deref() {
                                if let Some((result, output, exit_code, refresh_explorer)) =
                                    try_execute_builtin_command(&cmd, Path::new(workspace_root))
                                {
                                    complete_command_immediately(
                                        &app_handle,
                                        batch,
                                        &cmd,
                                        result,
                                        output,
                                        exit_code,
                                        refresh_explorer,
                                    );
                                    return;
                                }
                            }

                            eprintln!("[SINGLE APPROVAL] User APPROVED command: {}", cmd.command);
                            // Emit event for this specific command to be executed
                            let command_id = format!("cmd-{}", cmd.call.id);
                            let _ = app_handle.emit(
                                crate::events::event_names::COMMAND_EXECUTION_STARTED,
                                crate::events::CommandExecutionStartedPayload {
                                    command_id,
                                    call_id: cmd.call.id.clone(),
                                    command: cmd.command.clone(),
                                    program: cmd.command_spec.program.clone(),
                                    args: if cmd.command_spec.args.is_empty() {
                                        None
                                    } else {
                                        Some(cmd.command_spec.args.clone())
                                    },
                                    shell: Some(cmd.command_spec.shell),
                                    cwd: cmd.cwd.clone(),
                                    blocking: cmd.blocking,
                                    wait_ms_before_async: cmd.wait_ms_before_async,
                                },
                            );
                        } else {
                            eprintln!("[SINGLE APPROVAL] User SKIPPED command: {}", cmd.command);
                            // Add skip result immediately
                            let error_msg = format!(
                                "User explicitly skipped this command: '{}'. Do NOT retry this command. Ask the user how they would like to proceed instead.",
                                cmd.command
                            );
                            batch.file_results.push((
                                cmd.call.clone(),
                                crate::tools::ToolResult::skipped(&error_msg),
                            ));

                            // Emit tool-execution-completed for UI update
                            let _ = app_handle.emit(
                                "tool-execution-completed",
                                events::ToolExecutionCompletedPayload {
                                    tool_name: "run_command".to_string(),
                                    tool_call_id: call_id.clone(),
                                    success: false,
                                    skipped: true,
                                },
                            );
                        }
                    }
                }
            }
        }

        // Check if all commands have been processed
        check_batch_completion(&*state);
    })
    .await
    {
        eprintln!("[SINGLE APPROVAL] approve_single_command task failed: {}", error);
    }
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
                    crate::tools::ToolResult::skipped(format!(
                        "User cancelled: '{}'. This command was not executed.",
                        cmd.command
                    ))
                } else {
                    // Include the actual output in the error so the AI can see what failed
                    let error_msg = if clean_output.trim().is_empty() {
                        format!("Command failed with exit code {} (no output)", exit_code)
                    } else {
                        format!(
                            "Command failed with exit code {}:\n{}",
                            exit_code, &clean_output
                        )
                    };
                    crate::tools::ToolResult::err(error_msg)
                };
                let is_skipped = result.skipped;
                batch.file_results.push((cmd.call.clone(), result));

                // Emit command-executed event with RAW output for UI display
                // (ansi-to-react will render ANSI color codes)
                let _ = app_handle.emit(
                    events::event_names::COMMAND_EXECUTED,
                    events::CommandExecutedPayload {
                        command: cmd.command.clone(),
                        cwd: cmd.cwd.clone(),
                        output: output.clone(), // Use raw output with ANSI codes for UI
                        exit_code,
                        duration: None,
                        call_id: call_id.clone(),
                    },
                );

                // Emit tool-execution-completed after the command payload so the
                // frontend can associate completion with the rendered execution block.
                let _ = app_handle.emit(
                    "tool-execution-completed",
                    events::ToolExecutionCompletedPayload {
                        tool_name: "run_command".to_string(),
                        tool_call_id: call_id.clone(),
                        success: exit_code == 0,
                        skipped: is_skipped,
                    },
                );
            }
        }
    }
    drop(batch_guard);

    check_batch_completion(&*state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_simple_builtin_command, try_execute_builtin_command};
    use crate::ai_workflow::{CommandSpec, PendingCommand};
    use crate::protocol::{ToolCall, ToolFunction};
    use std::fs;
    use tempfile::tempdir;

    fn make_pending_shell_command(command: &str) -> PendingCommand {
        PendingCommand {
            call: ToolCall {
                id: "call-1".to_string(),
                typ: "function".to_string(),
                function: ToolFunction {
                    name: "run_command".to_string(),
                    arguments: "{}".to_string(),
                },
                status: None,
                result: None,
            },
            command: command.to_string(),
            command_spec: CommandSpec {
                command_line: Some(command.to_string()),
                program: None,
                args: Vec::new(),
                shell: true,
            },
            cwd: None,
            blocking: true,
            wait_ms_before_async: None,
        }
    }

    #[test]
    fn parse_simple_builtin_command_rejects_shell_metacharacters() {
        assert_eq!(
            parse_simple_builtin_command("cat src/main.rs"),
            Some(("cat".to_string(), vec!["src/main.rs".to_string()]))
        );
        assert_eq!(parse_simple_builtin_command("cat foo | grep bar"), None);
        assert_eq!(parse_simple_builtin_command("cat 'foo bar'"), None);
    }

    #[test]
    fn pwd_builtin_returns_workspace_or_cwd() {
        let dir = tempdir().expect("tempdir");
        let nested = dir.path().join("nested");
        fs::create_dir_all(&nested).expect("create nested dir");

        let mut cmd = make_pending_shell_command("pwd");
        cmd.cwd = Some(nested.to_string_lossy().to_string());

        let (result, output, exit_code, refresh_explorer) =
            try_execute_builtin_command(&cmd, dir.path()).expect("builtin result");

        assert!(result.success);
        assert_eq!(exit_code, 0);
        assert!(!refresh_explorer);
        assert_eq!(result.content, nested.to_string_lossy());
        assert_eq!(output, format!("{}\n", nested.to_string_lossy()));
    }

    #[test]
    fn ls_builtin_lists_visible_entries_only() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("alpha")).expect("create alpha dir");
        fs::write(dir.path().join("beta.txt"), "beta").expect("write beta");
        fs::write(dir.path().join(".hidden"), "hidden").expect("write hidden");

        let cmd = make_pending_shell_command("ls");
        let (result, output, exit_code, refresh_explorer) =
            try_execute_builtin_command(&cmd, dir.path()).expect("builtin result");

        assert!(result.success);
        assert_eq!(exit_code, 0);
        assert!(!refresh_explorer);
        assert_eq!(result.content, "alpha/\nbeta.txt");
        assert_eq!(output, "alpha/\nbeta.txt\n");
    }

    #[test]
    fn cat_builtin_reads_file_contents() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("note.txt"), "hello\nworld\n").expect("write note");

        let cmd = make_pending_shell_command("cat note.txt");
        let (result, output, exit_code, refresh_explorer) =
            try_execute_builtin_command(&cmd, dir.path()).expect("builtin result");

        assert!(result.success);
        assert_eq!(exit_code, 0);
        assert!(!refresh_explorer);
        assert_eq!(result.content, "hello\nworld");
        assert_eq!(output, "hello\nworld\n");
    }

    #[test]
    fn mkdir_builtin_creates_nested_directories_with_p() {
        let dir = tempdir().expect("tempdir");

        let cmd = make_pending_shell_command("mkdir -p one/two");
        let (result, _output, exit_code, refresh_explorer) =
            try_execute_builtin_command(&cmd, dir.path()).expect("builtin result");

        assert!(result.success);
        assert_eq!(exit_code, 0);
        assert!(refresh_explorer);
        assert!(dir.path().join("one/two").is_dir());
    }

    #[test]
    fn cp_builtin_copies_files() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("source.txt"), "copy me").expect("write source");

        let cmd = make_pending_shell_command("cp source.txt dest.txt");
        let (result, _output, exit_code, refresh_explorer) =
            try_execute_builtin_command(&cmd, dir.path()).expect("builtin result");

        assert!(result.success);
        assert_eq!(exit_code, 0);
        assert!(refresh_explorer);
        assert_eq!(
            fs::read_to_string(dir.path().join("dest.txt")).expect("read dest"),
            "copy me"
        );
    }

    #[test]
    fn cp_builtin_rejects_copying_directory_into_its_descendant() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("tree/branch")).expect("create tree");
        fs::write(dir.path().join("tree/file.txt"), "payload").expect("write file");

        let cmd = make_pending_shell_command("cp -r tree tree/branch/subcopy");
        let (result, _output, exit_code, refresh_explorer) =
            try_execute_builtin_command(&cmd, dir.path()).expect("builtin result");

        assert!(!result.success);
        assert_eq!(exit_code, 1);
        assert!(!refresh_explorer);
        assert!(!dir.path().join("tree/branch/subcopy").exists());
    }

    #[test]
    fn mv_builtin_moves_files() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("before.txt"), "move me").expect("write source");

        let cmd = make_pending_shell_command("mv before.txt after.txt");
        let (result, _output, exit_code, refresh_explorer) =
            try_execute_builtin_command(&cmd, dir.path()).expect("builtin result");

        assert!(result.success);
        assert_eq!(exit_code, 0);
        assert!(refresh_explorer);
        assert!(!dir.path().join("before.txt").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("after.txt")).expect("read dest"),
            "move me"
        );
    }

    #[test]
    fn mv_builtin_rejects_moving_directory_into_its_descendant() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("root/child")).expect("create root child");
        fs::write(dir.path().join("root/file.txt"), "payload").expect("write file");

        let cmd = make_pending_shell_command("mv root root/child/moved-root");
        let (result, _output, exit_code, refresh_explorer) =
            try_execute_builtin_command(&cmd, dir.path()).expect("builtin result");

        assert!(!result.success);
        assert_eq!(exit_code, 1);
        assert!(!refresh_explorer);
        assert!(dir.path().join("root").exists());
        assert!(!dir.path().join("root/child/moved-root").exists());
    }

    #[test]
    fn rm_builtin_removes_files_and_directories() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("trash.txt"), "delete me").expect("write file");
        fs::create_dir_all(dir.path().join("folder/nested")).expect("create nested dir");
        fs::write(dir.path().join("folder/nested/file.txt"), "nested").expect("write nested file");

        let rm_file = make_pending_shell_command("rm trash.txt");
        let (result, _output, exit_code, refresh_explorer) =
            try_execute_builtin_command(&rm_file, dir.path()).expect("builtin result");
        assert!(result.success);
        assert_eq!(exit_code, 0);
        assert!(refresh_explorer);
        assert!(!dir.path().join("trash.txt").exists());

        let rm_dir = make_pending_shell_command("rm -r folder");
        let (result, _output, exit_code, refresh_explorer) =
            try_execute_builtin_command(&rm_dir, dir.path()).expect("builtin result");
        assert!(result.success);
        assert_eq!(exit_code, 0);
        assert!(refresh_explorer);
        assert!(!dir.path().join("folder").exists());
    }
}
