use crate::events::{event_names, TerminalCwdChangedPayload};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{Arc, Mutex},
    thread,
};
use tauri::{Emitter, Runtime};

// Helper struct to hold the PTY state
pub struct PtyState {
    pub writer: Box<dyn Write + Send>,
    pub master: Box<dyn portable_pty::MasterPty + Send>,
    // We keep the child around to check exit status if needed,
    // though portable-pty child doesn't always strictly need to be held if we just kill it.
    // However, for proper cleanup it's good.
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    pub seq: Arc<Mutex<u64>>, // v1.1: sequence number for TerminalOutput events
    pub owner: crate::blade_protocol::TerminalOwner, // v1.1: ownership tracking
}

pub struct TerminalManager {
    // Map of Terminal ID -> PtyState
    pub ptys: Arc<Mutex<HashMap<String, PtyState>>>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            ptys: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// Commands to be exposed to Tauri

// #[tauri::command]
pub fn create_terminal<R: Runtime>(
    id: String,
    cwd: Option<String>,
    command: Option<String>,
    app_handle: tauri::AppHandle<R>,
    state: tauri::State<'_, TerminalManager>,
) -> Result<(), String> {
    let pty_system = NativePtySystem::default();

    // Configure the PTY
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    // Determine shell and command mode
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    let shell_name = std::path::Path::new(&shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("sh");

    let (mut cmd, is_interactive) = if let Some(cmd_str) = command {
        let mut builder = CommandBuilder::new(shell.clone());
        builder.arg("-c");
        builder.arg(cmd_str);
        (builder, false)
    } else {
        (CommandBuilder::new(shell.clone()), true)
    };

    // Set working directory if provided
    if let Some(path) = cwd {
        cmd.cwd(path);
    }

    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // Explicitly set LANG to ensure UTF-8 support in the PTY
    let lang = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string());
    cmd.env("LANG", &lang);
    // Also set LC_ALL to be safe
    if let Ok(lc_all) = std::env::var("LC_ALL") {
        cmd.env("LC_ALL", lc_all);
    } else {
        cmd.env("LC_ALL", &lang);
    }

    // Ensure shells emit OSC 7 working-directory updates so the UI can track cwd changes.
    if is_interactive {
        if shell_name == "bash" {
            let osc7_cmd = "printf '\\e]7;file://localhost%s\\e\\\\' \"$PWD\"";
            let prompt_command = std::env::var("PROMPT_COMMAND").ok();
            let combined = if let Some(existing) = prompt_command {
                if existing.trim().is_empty() {
                    osc7_cmd.to_string()
                } else {
                    format!("{existing};{osc7_cmd}")
                }
            } else {
                osc7_cmd.to_string()
            };
            cmd.env("PROMPT_COMMAND", combined);
        } else if shell_name == "zsh" {
            if let Some(zdotdir) = ensure_zsh_zdotdir() {
                cmd.env("ZDOTDIR", zdotdir);
            }
        }
    }

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    // Store state
    let seq_counter = Arc::new(Mutex::new(0u64));
    let owner = crate::blade_protocol::TerminalOwner::User; // Default to User for interactive terminals
    {
        let mut ptys = state.ptys.lock().unwrap();
        ptys.insert(
            id.clone(),
            PtyState {
                writer,
                master: pair.master,
                child,
                seq: seq_counter.clone(),
                owner: owner.clone(),
            },
        );
    }

    // v1.1: Emit TerminalSpawned event
    let _ = app_handle.emit(
        "blade-event",
        crate::blade_protocol::BladeEventEnvelope {
            id: uuid::Uuid::new_v4(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            causality_id: None,
            event: crate::blade_protocol::BladeEvent::Terminal(
                crate::blade_protocol::TerminalEvent::Spawned {
                    id: id.clone(),
                    owner,
                },
            ),
        },
    );

    // Spawn a thread to read output and emit to frontend
    let id_clone = id.clone();
    let app_handle_clone = app_handle.clone();
    let ptys_arc = state.ptys.clone();

    thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let mut pending_osc = String::new();
        let mut line_buffer = String::new();
        // Track active command: accumulate output between start/exit sentinels
        let mut active_cmd: Option<String> = None; // call_id of active command
        let mut cmd_output_buffer = String::new();

        let emit_output = |app: &tauri::AppHandle<R>, id: &str, data: String, seq: &Arc<Mutex<u64>>| {
            if data.is_empty() { return; }
            let _seq = {
                let mut seq_guard = seq.lock().unwrap();
                let current = *seq_guard;
                *seq_guard += 1;
                current
            };
            let payload = TerminalOutput {
                id: id.to_string(),
                data,
            };
            let _ = app.emit("terminal-output", payload);
        };

        let process_chunk = |processable: &str,
                             app: &tauri::AppHandle<R>,
                             id: &str,
                             seq: &Arc<Mutex<u64>>,
                             active_cmd: &mut Option<String>,
                             cmd_output_buffer: &mut String| {
            let sentinel_result = strip_blade_sentinels(processable);

            // Order matters! Within a single chunk the start sentinel appears
            // before the command output which appears before the exit sentinel.
            //
            // 1. Process STARTED sentinels first → sets active_cmd so output
            //    accumulation works for output in this same chunk.
            for call_id in &sentinel_result.started {
                *active_cmd = Some(call_id.clone());
                cmd_output_buffer.clear();
                let _ = app.emit(
                    "blade-cmd-started",
                    BladeCmdStarted {
                        terminal_id: id.to_string(),
                        call_id: call_id.clone(),
                    },
                );
            }

            // 2. Emit cleaned output for terminal display AND accumulate for
            //    the active command (active_cmd is now set if start was in this chunk).
            if !sentinel_result.cleaned.is_empty() {
                if active_cmd.is_some() {
                    cmd_output_buffer.push_str(&sentinel_result.cleaned);
                }
                emit_output(app, id, sentinel_result.cleaned, seq);
            }

            // 3. Process EXITED sentinels last → takes the accumulated output.
            for (call_id, exit_code) in &sentinel_result.exited {
                let output = std::mem::take(cmd_output_buffer);
                if active_cmd.as_deref() == Some(call_id.as_str()) {
                    *active_cmd = None;
                }
                let _ = app.emit(
                    "blade-cmd-exited",
                    BladeCmdExited {
                        terminal_id: id.to_string(),
                        call_id: call_id.clone(),
                        exit_code: *exit_code,
                        output,
                    },
                );
            }
        };

        loop {
            match reader.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    let raw_output = String::from_utf8_lossy(&buffer[..n]).to_string();

                    // Extract OSC 7 cwd updates
                    let combined = if pending_osc.is_empty() {
                        raw_output.clone()
                    } else {
                        let mut merged = std::mem::take(&mut pending_osc);
                        merged.push_str(&raw_output);
                        merged
                    };
                    let (cwd_updates, new_pending) = extract_osc7_paths(&combined);
                    pending_osc = new_pending;
                    for cwd in cwd_updates {
                        let _ = app_handle_clone.emit(
                            event_names::TERMINAL_CWD_CHANGED,
                            TerminalCwdChangedPayload {
                                id: id_clone.clone(),
                                cwd,
                            },
                        );
                    }

                    // Buffer lines for sentinel detection
                    line_buffer.push_str(&raw_output);

                    if let Some(last_newline) = line_buffer.rfind('\n') {
                        let processable = line_buffer[..=last_newline].to_string();
                        line_buffer = line_buffer[last_newline + 1..].to_string();
                        process_chunk(&processable, &app_handle_clone, &id_clone, &seq_counter,
                                      &mut active_cmd, &mut cmd_output_buffer);
                    } else if line_buffer.len() > 8192 {
                        // Large buffer without newlines — flush
                        let flushed = std::mem::take(&mut line_buffer);
                        process_chunk(&flushed, &app_handle_clone, &id_clone, &seq_counter,
                                      &mut active_cmd, &mut cmd_output_buffer);
                    }
                }
                Ok(_) => {
                    // EOF — flush remaining
                    if !line_buffer.is_empty() {
                        let flushed = std::mem::take(&mut line_buffer);
                        process_chunk(&flushed, &app_handle_clone, &id_clone, &seq_counter,
                                      &mut active_cmd, &mut cmd_output_buffer);
                    }
                    break;
                }
                Err(_) => {
                    // Error — flush remaining
                    if !line_buffer.is_empty() {
                        let flushed = std::mem::take(&mut line_buffer);
                        process_chunk(&flushed, &app_handle_clone, &id_clone, &seq_counter,
                                      &mut active_cmd, &mut cmd_output_buffer);
                    }
                    break;
                }
            }
        }

        // Emit exit event and cleanup PTY
        let exit_code = {
            let mut ptys = ptys_arc.lock().unwrap();
            if let Some(mut pty) = ptys.remove(&id_clone) {
                match pty.child.wait() {
                    Ok(status) => status.exit_code() as i32,
                    Err(_) => 1,
                }
            } else {
                0
            }
        };

        let _ = app_handle_clone.emit(
            "terminal-exit",
            TerminalExit {
                id: id_clone,
                exit_code,
            },
        );

        // Refresh explorer to show changes from command
        let _ = app_handle_clone.emit("refresh-explorer", ());
        let _ = app_handle_clone.emit(event_names::REFRESH_EXPLORER, ());
    });

    Ok(())
}

pub fn kill_terminal(
    id: String,
    state: tauri::State<'_, TerminalManager>,
) -> Result<(), String> {
    let mut ptys = state.ptys.lock().unwrap();
    if let Some(mut pty) = ptys.remove(&id) {
        let _ = pty.child.kill();
    }
    Ok(())
}

// #[tauri::command]
pub fn write_to_terminal(
    id: String,
    data: String,
    state: tauri::State<'_, TerminalManager>,
) -> Result<(), String> {
    let mut ptys = state.ptys.lock().unwrap();
    if let Some(pty) = ptys.get_mut(&id) {
        write!(pty.writer, "{}", data).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// #[tauri::command]
pub fn resize_terminal(
    id: String,
    rows: u16,
    cols: u16,
    state: tauri::State<'_, TerminalManager>,
) -> Result<(), String> {
    let mut ptys = state.ptys.lock().unwrap();
    if let Some(pty) = ptys.get_mut(&id) {
        println!("Resizing PTY {} to {}x{}", id, rows, cols);
        pty.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
    } else {
        println!("Resize failed: PTY {} not found", id);
    }
    Ok(())
}

// Event payload structs
#[derive(Clone, serde::Serialize)]
struct TerminalOutput {
    id: String,
    data: String,
}

#[derive(Clone, serde::Serialize)]
struct TerminalExit {
    id: String,
    exit_code: i32,
}

#[derive(Clone, serde::Serialize)]
struct BladeCmdStarted {
    terminal_id: String,
    call_id: String,
}

#[derive(Clone, serde::Serialize)]
struct BladeCmdExited {
    terminal_id: String,
    call_id: String,
    exit_code: i32,
    output: String,
}

// Sentinel markers used by the command execution system.
// These are plain-text strings (no escape characters) that are echoed to stdout
// by the shell command wrapper. The terminal reader thread detects and strips them.
const SENTINEL_START: &str = "##BLADE_CMD_START:";
const SENTINEL_EXIT: &str = "##BLADE_CMD_EXIT:";
const SENTINEL_END: &str = "##";

/// Strip BLADE sentinel markers from terminal output.
/// Returns the cleaned output and any detected sentinel events.
/// Also strips the shell echo of the command line containing sentinels.
struct SentinelResult {
    cleaned: String,
    started: Vec<String>,       // call_ids
    exited: Vec<(String, i32)>, // (call_id, exit_code)
}

fn strip_blade_sentinels(input: &str) -> SentinelResult {
    let mut cleaned = String::with_capacity(input.len());
    let mut started = Vec::new();
    let mut exited = Vec::new();

    for line in input.split_inclusive('\n') {
        // Robust sentinel detection even if the sentinel appears mid-line
        // (e.g. prompt/ANSI prefixes or no newline before exit sentinel).
        if let Some(start_idx) = line.find(SENTINEL_START) {
            let rest = &line[start_idx + SENTINEL_START.len()..];
            if let Some(end_rel) = rest.find(SENTINEL_END) {
                let call_id = &rest[..end_rel];
                started.push(call_id.to_string());
                // Drop the entire line to avoid showing sentinel artifacts
                continue;
            }
        }

        if let Some(exit_idx) = line.find(SENTINEL_EXIT) {
            let rest = &line[exit_idx + SENTINEL_EXIT.len()..];
            if let Some(end_rel) = rest.find(SENTINEL_END) {
                let payload = &rest[..end_rel];
                if let Some((call_id, exit_str)) = payload.rsplit_once(':') {
                    let exit_code = exit_str.parse::<i32>().unwrap_or(1);
                    exited.push((call_id.to_string(), exit_code));

                    // Preserve any output before/after the sentinel on this line.
                    // This handles commands that don't end with a newline.
                    let prefix = &line[..exit_idx];
                    let suffix_start = exit_idx + SENTINEL_EXIT.len() + end_rel + SENTINEL_END.len();
                    let suffix = line.get(suffix_start..).unwrap_or("");
                    if !prefix.is_empty() {
                        cleaned.push_str(prefix);
                    }
                    if !suffix.is_empty() {
                        cleaned.push_str(suffix);
                    }
                    continue;
                }
            }
        }

        // Strip echoed command lines that contain sentinel text but didn't parse
        if line.contains(SENTINEL_START) || line.contains(SENTINEL_EXIT) {
            continue;
        }

        cleaned.push_str(line);
    }

    SentinelResult { cleaned, started, exited }
}

fn ensure_zsh_zdotdir() -> Option<String> {
    let base_dir = std::env::temp_dir().join("zblade-zsh");
    if std::fs::create_dir_all(&base_dir).is_err() {
        return None;
    }

    let existing_zdotdir = std::env::var("ZDOTDIR").ok();
    let source_line = if let Some(dir) = existing_zdotdir {
        format!(
            "if [ -f \"{}/.zshrc\" ]; then source \"{}/.zshrc\"; fi",
            dir, dir
        )
    } else {
        "if [ -f \"$HOME/.zshrc\" ]; then source \"$HOME/.zshrc\"; fi".to_string()
    };

    let zshrc = format!(
        "{source_line}\n\
function __zblade_osc7() {{ printf '\\e]7;file://localhost%s\\e\\\\' \"$PWD\"; }}\n\
autoload -U add-zsh-hook\n\
add-zsh-hook precmd __zblade_osc7\n"
    );

    let zshrc_path = base_dir.join(".zshrc");
    if std::fs::write(&zshrc_path, zshrc).is_err() {
        return None;
    }

    Some(base_dir.to_string_lossy().to_string())
}

fn extract_osc7_paths(input: &str) -> (Vec<String>, String) {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut paths = Vec::new();
    let mut pending = String::new();

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if i + 1 >= bytes.len() {
                pending = input[i..].to_string();
                break;
            }
            if bytes[i + 1] == b']' {
                if i + 3 >= bytes.len() {
                    pending = input[i..].to_string();
                    break;
                }
                if bytes[i + 2] == b'7' && bytes[i + 3] == b';' {
                    let start = i + 4;
                    let mut j = start;
                    let mut terminator_len = None;
                    while j < bytes.len() {
                        if bytes[j] == 0x07 {
                            terminator_len = Some((j, 1));
                            break;
                        }
                        if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                            terminator_len = Some((j, 2));
                            break;
                        }
                        j += 1;
                    }

                    if let Some((end, term_len)) = terminator_len {
                        let raw = &input[start..end];
                        if let Some(path) = parse_osc7_path(raw) {
                            paths.push(path);
                        }
                        i = end + term_len;
                        continue;
                    } else {
                        pending = input[i..].to_string();
                        break;
                    }
                }
            }
        }
        i += 1;
    }

    (paths, pending)
}

fn parse_osc7_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let rest = trimmed.strip_prefix("file://")?;
    let path_start = rest.find('/').unwrap_or(0);
    let path = &rest[path_start..];
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

// Execute a command in a terminal (non-interactive, for AI command execution)
// #[tauri::command]
pub fn execute_command_in_terminal<R: Runtime>(
    id: String,
    command: String,
    cwd: Option<String>,
    app_handle: tauri::AppHandle<R>,
    state: tauri::State<'_, crate::AppState>,
) -> Result<(), String> {
    let pty_system = NativePtySystem::default();

    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    // Execute the command directly (not a shell)
    let mut cmd = CommandBuilder::new("sh");
    cmd.arg("-c");
    cmd.arg(&command);

    // Use provided cwd, or fall back to workspace path
    let working_dir = cwd.or_else(|| {
        let ws = state.workspace.lock().unwrap();
        ws.workspace
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    });

    if let Some(path) = working_dir {
        cmd.cwd(path);
    }

    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // Explicitly set LANG to ensure UTF-8 support in the PTY
    let lang = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string());
    cmd.env("LANG", &lang);
    // Also set LC_ALL to be safe
    if let Ok(lc_all) = std::env::var("LC_ALL") {
        cmd.env("LC_ALL", lc_all);
    } else {
        cmd.env("LC_ALL", &lang);
    }

    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;

    // Create cancel flag for this command
    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_flag_clone = cancel_flag.clone();

    // Store the cancel flag so stop_generation can cancel this command
    {
        let mut executing = state.executing_commands.lock().unwrap();
        executing.insert(id.clone(), cancel_flag);
    }

    // Spawn thread to read output and wait for exit
    let id_clone = id.clone();
    // Clone the Arc to the Mutex so we can access it from the thread
    let executing_commands = state.executing_commands.clone();
    let seq_counter = Arc::new(Mutex::new(0u64)); // v1.1: sequence counter
    thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        let mut accumulated_output = String::new();

        loop {
            // Check if cancelled
            if cancel_flag_clone.load(std::sync::atomic::Ordering::Relaxed) {
                eprintln!("[EXEC] Command {} cancelled, killing process", id_clone);
                let _ = child.kill();

                // Emit exit with special code for cancelled
                let exit_payload = TerminalExit {
                    id: id_clone.clone(),
                    exit_code: 130, // Standard SIGINT exit code
                };
                let _ = app_handle.emit("terminal-exit", exit_payload);

                // Remove from executing commands
                let mut executing = executing_commands.lock().unwrap();
                executing.remove(&id_clone);
                return;
            }

            match reader.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    let output = String::from_utf8_lossy(&buffer[..n]).to_string();
                    accumulated_output.push_str(&output);

                    // v1.1: Increment sequence number
                    let _seq = {
                        let mut seq_guard = seq_counter.lock().unwrap();
                        let current = *seq_guard;
                        *seq_guard += 1;
                        current
                    };

                    let payload = TerminalOutput {
                        id: id_clone.clone(),
                        data: output,
                    };
                    // Legacy format for compatibility
                    let _ = app_handle.emit("terminal-output", payload);
                }
                Ok(_) => break,
                Err(_) => break,
            }
        }

        // Wait for child to exit and get exit code
        let exit_code = match child.wait() {
            Ok(status) => status.exit_code() as i32,
            Err(_) => 1,
        };

        // Emit exit event
        let exit_payload = TerminalExit {
            id: id_clone.clone(),
            exit_code,
        };
        let _ = app_handle.emit("terminal-exit", exit_payload);

        // Refresh explorer
        let _ = app_handle.emit("refresh-explorer", ());

        // Remove from executing commands
        let mut executing = executing_commands.lock().unwrap();
        executing.remove(&id_clone);
    });

    Ok(())
}
