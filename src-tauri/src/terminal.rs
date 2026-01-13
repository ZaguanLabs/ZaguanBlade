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

    // Spawn the shell (bash for now, or use user's SHELL env)
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    let mut cmd = CommandBuilder::new(shell);

    // Set working directory if provided
    if let Some(path) = cwd {
        cmd.cwd(path);
    }

    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

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
    let _ = app_handle.emit("blade-event", crate::blade_protocol::BladeEventEnvelope {
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
            }
        ),
    });

    // Spawn a thread to read output and emit to frontend
    let id_clone = id.clone();
    thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    let output = String::from_utf8_lossy(&buffer[..n]).to_string();
                    
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
                    // Emit 'terminal-output' event (legacy format)
                    // TODO: Migrate to BladeEvent::Terminal(TerminalEvent::Output { id, seq, data })
                    let _ = app_handle.emit("terminal-output", payload);
                }
                Ok(_) => break,  // EOF
                Err(_) => break, // Error
            }
        }
        // Emit exit event?
    });

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

        // Remove from executing commands
        let mut executing = executing_commands.lock().unwrap();
        executing.remove(&id_clone);
    });

    Ok(())
}
