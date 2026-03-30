use crate::app_state::AppState;
use notify::{event::ModifyKind, EventKind, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, Runtime};

#[derive(Clone, serde::Serialize)]
pub struct FileChangeEvent {
    pub count: usize,
    pub paths: Vec<String>,
}

fn reindex_changed_paths(state: &AppState, workspace_root: &Path, event: &notify::Event) {
    let service = match state.language_service() {
        Ok(service) => service,
        Err(error) => {
            eprintln!(
                "[WATCHER] Failed to initialize language service for reindex: {}",
                error
            );
            return;
        }
    };

    for path in &event.paths {
        let Ok(relative_path) = path.strip_prefix(workspace_root) else {
            continue;
        };

        let Some(relative_str) = relative_path.to_str().map(|value| value.replace('\\', "/"))
        else {
            continue;
        };

        if crate::tree_sitter::Language::from_path(&relative_str).is_none() {
            continue;
        }

        let is_delete = matches!(event.kind, EventKind::Remove(_));
        if is_delete || !path.exists() {
            if let Err(error) = service.remove_file(&relative_str) {
                eprintln!(
                    "[WATCHER] Failed to remove {} from symbol index: {}",
                    relative_str, error
                );
            }
            continue;
        }

        if path.is_file() {
            if let Err(error) = service.index_file(&relative_str) {
                eprintln!("[WATCHER] Failed to reindex {}: {}", relative_str, error);
            }
        }
    }
}

pub fn restart_fs_watcher<R: Runtime>(app_handle: &tauri::AppHandle<R>) {
    let app_handle = app_handle.clone();

    std::thread::spawn(move || {
        let state = app_handle.state::<AppState>();
        let workspace_root = { state.workspace.lock().unwrap().workspace.clone() };

        {
            let mut watcher_guard = state.fs_watcher.lock().unwrap();
            *watcher_guard = None;
        }

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
            let callback_root = root.clone();
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

                            let state = app_handle_clone.state::<AppState>();
                            if let Ok(store) = state.worktree() {
                                if let Err(error) = store.refresh() {
                                    eprintln!("[WATCHER] Failed to refresh worktree snapshot: {}", error);
                                }
                            }
                            reindex_changed_paths(&state, &callback_root, &event);

                            let _ =
                                app_handle_clone.emit("file-changes-detected", file_change_event);
                            crate::blade_event_scheduler::queue_refresh_explorer(
                                &app_handle_clone,
                            );
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

            let mut watcher_guard = state.fs_watcher.lock().unwrap();
            *watcher_guard = Some(watcher);
            eprintln!("[WATCHER] Watching workspace: {}", root.display());
        }
    });
}
