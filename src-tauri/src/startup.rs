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

// M5.19 — the eager project-context indexer build was removed. It walked the
// whole workspace and wrote a 130 MiB `.zblade/cache/index.json` at every launch
// to feed the legacy project-index overview — which is off by default
// (`project_index_legacy_enabled = false`) and whose only intent (`GetFullContext`)
// the frontend never sends. The GUI file tree reads the filesystem directly, so
// nothing consumed this. The symbols index supersedes it entirely.

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

    crate::startup_marks::record("post_ui_service_start");
    crate::fs_watcher::restart_fs_watcher(app_handle);

    refresh_symbols_index(app_handle);
}

/// Capture document identity before scheduling; a queued startup must never
/// index a replacement workspace. Settings saves use this to restart a retired index.
pub fn refresh_symbols_index<R: Runtime>(app_handle: &tauri::AppHandle<R>) {
    let state = app_handle.state::<AppState>();
    let Ok(documents) = state.document_service() else {
        return;
    };
    crate::index_policy::refresh();
    {
        let Ok(mut slot) = state.language_service.write() else {
            return;
        };
        if !crate::index_policy::enabled(documents.workspace_root()).unwrap_or(false) {
            if let Some(service) = slot.take() {
                service.lifetime.cancel();
            }
            return;
        }
    }
    let app_handle = app_handle.clone();
    std::thread::spawn(move || {
        while crate::index_policy::stopping(documents.workspace_root()) {
            if documents.cancellation().is_cancelled()
                || !crate::index_policy::enabled(documents.workspace_root()).unwrap_or(false)
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if documents.cancellation().is_cancelled() {
            return;
        }
        let state = app_handle.state::<AppState>();
        let Ok(service) = state.language_service() else {
            return;
        };
        if !service.uses_documents(&documents) || !service.lifetime.claim_background_refresh() {
            return;
        }
        let result = service.reconcile_index_with_progress(|health| {
            if !documents.cancellation().is_cancelled() && !service.lifetime.is_cancelled() {
                emit_index_status(&app_handle, health.clone());
            }
        });
        if documents.cancellation().is_cancelled() || service.lifetime.is_cancelled() {
            return;
        }
        if let Err(error) = result {
            let mut health = service.index_health_snapshot();
            health.status = crate::language_service::IndexHealthStatus::Error;
            health.active_workers = 0;
            health.current_file = None;
            health.message = format!("Code intelligence refresh failed: {}", error);
            service.set_index_health(health.clone());
            emit_index_status(&app_handle, health);
        }
    });
}
