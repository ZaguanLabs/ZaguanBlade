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

fn ensure_project_context_indexer(state: &AppState, workspace: &std::path::Path) {
    {
        let guard = state.indexer_manager.lock().unwrap();
        if let Some(manager) = guard.as_ref() {
            if manager.matches_workspace(workspace) {
                return;
            }
        }
    }

    match crate::indexer::IndexerManager::new(workspace) {
        Ok(manager) => {
            let mut guard = state.indexer_manager.lock().unwrap();
            if guard
                .as_ref()
                .map(|existing| existing.matches_workspace(workspace))
                .unwrap_or(false)
            {
                return;
            }
            *guard = Some(manager);
            eprintln!(
                "[Indexer] Project context files initialized for: {}",
                workspace.display()
            );
        }
        Err(error) => {
            eprintln!(
                "[Indexer] Failed to initialize project context files for {}: {}",
                workspace.display(),
                error
            );
        }
    }
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
                            let top_unresolved = report
                                .graph_quality
                                .top_unresolved_targets
                                .iter()
                                .map(|target| {
                                    format!(
                                        "{}:{}({} @ {})",
                                        target.relationship_type,
                                        target.target_name,
                                        target.count,
                                        target.example_source_file
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            eprintln!(
                                "[LanguageService] Post-UI index reconciliation complete: {} indexed, {} removed in {}ms; relationships {}/{} resolved, {} unresolved, {} suppressed external, {} missing sources, {} missing targets, {} missing roots; top unresolved [{}]",
                                report.files_indexed,
                                report.files_removed,
                                report.duration_ms,
                                report.graph_quality.resolved_relationships,
                                report.graph_quality.total_relationships,
                                report.graph_quality.unresolved_symbol_relationships,
                                report.graph_quality.suppressed_external_relationships,
                                report.graph_quality.missing_source_symbols,
                                report.graph_quality.missing_target_symbols,
                                report.graph_quality.indexed_files_missing_root_symbol,
                                top_unresolved
                            );
                        }
                        Err(error) => {
                            let mut health = service.index_health_snapshot();
                            health.status = crate::language_service::IndexHealthStatus::Error;
                            health.active_workers = 0;
                            health.current_file = None;
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

            ensure_project_context_indexer(&state, &path);
        }
    });
}
