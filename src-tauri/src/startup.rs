use std::sync::atomic::Ordering;

use tauri::{Manager, Runtime};
use uuid::Uuid;

use crate::blade_protocol::{BladeEvent, BladeEventEnvelope, LanguageEvent};
use crate::language_service::IndexHealthSnapshot;
use crate::AppState;

fn timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn emit_index_status<R: Runtime>(app_handle: &tauri::AppHandle<R>, health: IndexHealthSnapshot) {
    crate::blade_event_scheduler::emit_envelope(
        app_handle,
        BladeEventEnvelope {
            id: Uuid::new_v4(),
            timestamp: timestamp_ms(),
            causality_id: None,
            event: BladeEvent::Language(LanguageEvent::IndexStatus { health }),
        },
    );
}

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
                    let status_app_handle = app_handle.clone();
                    let result = service.reconcile_index_with_progress(|health| {
                        emit_index_status(&status_app_handle, health.clone());
                    });
                    match result {
                        Ok(report) => {
                            eprintln!(
                                "[LanguageService] Post-UI index reconciliation complete: {} indexed, {} removed in {}ms",
                                report.files_indexed, report.files_removed, report.duration_ms
                            );
                        }
                        Err(error) => {
                            let mut health = service.index_health_snapshot();
                            health.status = crate::language_service::IndexHealthStatus::Error;
                            health.active_workers = 0;
                            health.message = format!("Code intelligence refresh failed: {}", error);
                            service.set_index_health(health.clone());
                            emit_index_status(&app_handle, health);
                            eprintln!(
                                "[LanguageService] Post-UI index reconciliation failed: {}",
                                error
                            );
                        }
                    }
                }
                Err(error) => {
                    eprintln!(
                        "[LanguageService] Failed to initialize post-UI indexing: {}",
                        error
                    );
                }
            }
        }
    });
}
