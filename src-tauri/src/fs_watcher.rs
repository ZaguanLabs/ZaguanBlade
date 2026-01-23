use crate::app_state::AppState;
use notify::{event::ModifyKind, EventKind, RecursiveMode, Watcher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, Runtime};

#[derive(Clone, serde::Serialize)]
pub struct FileChangeEvent {
    pub count: usize,
    pub paths: Vec<String>,
}

pub fn restart_fs_watcher<R: Runtime>(app_handle: &tauri::AppHandle<R>) {
    let app_handle = app_handle.clone();

    std::thread::spawn(move || {
        let state = app_handle.state::<AppState>();
        let workspace_root = { state.workspace.lock().unwrap().workspace.clone() };

        let mut watcher_guard = state.fs_watcher.lock().unwrap();
        *watcher_guard = None;

        if let Some(root) = workspace_root {
            // Check if root exists before trying to watch
            if !root.exists() {
                eprintln!(
                    "[WATCHER] Workspace root does not exist: {}",
                    root.display()
                );
                return;
            }

            let app_handle_clone = app_handle.clone();
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

                            let paths: Vec<String> = event
                                .paths
                                .iter()
                                .map(|p| p.display().to_string())
                                .collect();

                            let file_change_event = FileChangeEvent {
                                count: paths.len(),
                                paths: paths.clone(),
                            };

                            let _ =
                                app_handle_clone.emit("file-changes-detected", file_change_event);
                            let _ = app_handle_clone
                                .emit(crate::events::event_names::REFRESH_EXPLORER, ());
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

            // This is the blocking call (recursive watch crawl)
            if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
                eprintln!("[WATCHER] Failed to watch {}: {}", root.display(), e);
                return;
            }

            *watcher_guard = Some(watcher);
            eprintln!("[WATCHER] Watching workspace: {}", root.display());
        }
    });
}
