use std::sync::atomic::Ordering;

use tauri::{Manager, Runtime};

use crate::AppState;

pub fn ensure_post_ui_startup<R: Runtime>(app_handle: &tauri::AppHandle<R>) {
    let state = app_handle.state::<AppState>();

    if state.startup_services_started.swap(true, Ordering::AcqRel) {
        return;
    }

    let workspace = state.workspace.lock().unwrap().workspace.clone();
    if workspace.is_none() {
        state
            .startup_services_started
            .store(false, Ordering::Release);
        return;
    }

    crate::fs_watcher::restart_fs_watcher(app_handle);

    let app_handle = app_handle.clone();
    std::thread::spawn(move || {
        let state = app_handle.state::<AppState>();
        let workspace = state.workspace.lock().unwrap().workspace.clone();

        if let Some(path) = workspace {
            let path_str = path.to_string_lossy().to_string();
            eprintln!(
                "[LanguageService] Triggering post-UI workspace indexing for: {}",
                path_str
            );
            match state.language_service() {
                Ok(service) => {
                    if let Ok(stats) = service.index_directory(".") {
                        eprintln!(
                            "[LanguageService] Post-UI indexing complete: {} files in {}ms",
                            stats.files_indexed, stats.duration_ms
                        );
                    }
                }
                Err(error) => {
                    eprintln!("[LanguageService] Failed to initialize post-UI indexing: {}", error);
                }
            }
        }
    });
}
