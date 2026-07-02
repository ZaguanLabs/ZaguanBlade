use crate::app_state::AppState;
use crate::uncommitted_changes::{refresh_change_from_contents, UncommittedChange};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

fn normalize_path(value: &str) -> String {
    value
        .replace('\\', "/")
        .replace("//", "/")
        .trim_end_matches('/')
        .to_string()
}

fn is_boundary_suffix_match(full: &str, suffix: &str) -> bool {
    if !full.ends_with(suffix) {
        return false;
    }
    if full.len() == suffix.len() {
        return true;
    }
    full.as_bytes()
        .get(full.len().saturating_sub(suffix.len() + 1))
        .copied()
        == Some(b'/')
}

fn resolve_change_for_file_path(state: &AppState, file_path: &str) -> Option<UncommittedChange> {
    let requested = normalize_path(file_path);
    let all = state.uncommitted_changes.get_all();

    let mut exact_matches: Vec<UncommittedChange> = all
        .iter()
        .filter(|change| normalize_path(&change.file_path.to_string_lossy()) == requested)
        .cloned()
        .collect();
    if !exact_matches.is_empty() {
        exact_matches.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        return exact_matches
            .into_iter()
            .find(|change| !change.unified_diff.trim().is_empty())
            .or_else(|| {
                state
                    .uncommitted_changes
                    .get_all()
                    .into_iter()
                    .filter(|change| {
                        normalize_path(&change.file_path.to_string_lossy()) == requested
                    })
                    .max_by_key(|change| change.timestamp)
            });
    }

    let mut suffix_matches: Vec<UncommittedChange> = all
        .into_iter()
        .filter(|change| {
            let candidate = normalize_path(&change.file_path.to_string_lossy());
            is_boundary_suffix_match(&candidate, &requested)
                || is_boundary_suffix_match(&requested, &candidate)
        })
        .collect();
    if suffix_matches.is_empty() {
        return None;
    }

    suffix_matches.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    suffix_matches
        .into_iter()
        .find(|change| !change.unified_diff.trim().is_empty())
}

fn refresh_tracked_change(
    state: &AppState,
    change: UncommittedChange,
) -> Option<UncommittedChange> {
    let history_service = state.history_service().ok()?;
    let base_content = history_service
        .get_snapshot_content(&change.snapshot_id)
        .ok()?;
    let new_content = fs::read_to_string(&change.file_path).ok()?;
    let current_modified_ms = crate::uncommitted_changes::file_modified_ms(&change.file_path);

    refresh_change_from_contents(change, &base_content, &new_content, current_modified_ms)
}

fn workspace_relative_supported_index_path(
    workspace_root: Option<&Path>,
    path: &Path,
) -> Option<String> {
    let index_path = if path.is_absolute() {
        workspace_root
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    } else {
        path.to_string_lossy().replace('\\', "/")
    };

    crate::tree_sitter::Language::from_path(&index_path).map(|_| index_path)
}

fn schedule_symbol_index_refresh_for_paths(
    app: &AppHandle,
    paths: Vec<PathBuf>,
    reason: &'static str,
) {
    if paths.is_empty() {
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let service = match state.language_service() {
            Ok(service) => service,
            Err(error) => {
                eprintln!(
                    "[UNCOMMITTED] Failed to get language service after {}: {}",
                    reason, error
                );
                return;
            }
        };
        let workspace_root = state.workspace_root();
        let mut seen = HashSet::new();

        for path in paths {
            let Some(index_path) =
                workspace_relative_supported_index_path(workspace_root.as_deref(), &path)
            else {
                continue;
            };
            if !seen.insert(index_path.clone()) {
                continue;
            }

            if let Err(error) = service.index_file(&index_path) {
                eprintln!(
                    "[UNCOMMITTED] Failed to refresh symbol index for {} after {}: {}",
                    index_path, reason, error
                );
            }
        }
    });
}

fn schedule_symbol_index_refresh_for_changes(
    app: &AppHandle,
    changes: &[UncommittedChange],
    reason: &'static str,
) {
    schedule_symbol_index_refresh_for_paths(
        app,
        changes
            .iter()
            .map(|change| change.file_path.clone())
            .collect::<Vec<_>>(),
        reason,
    );
}

#[tauri::command]
pub async fn get_uncommitted_changes(
    _state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<UncommittedChange>, String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let history_service = state.history_service()?;
        let existing = state.uncommitted_changes.get_all();
        if existing.is_empty() {
            return Ok(existing);
        }

        let mut refreshed = Vec::with_capacity(existing.len());
        let mut changed_any = false;

        for mut change in existing {
            let current_modified_ms =
                crate::uncommitted_changes::file_modified_ms(&change.file_path);
            let snapshot = history_service.get_snapshot_content(&change.snapshot_id);
            let current = fs::read_to_string(&change.file_path);

            if let (Ok(base_content), Ok(new_content)) = (snapshot, current) {
                let refreshed_change = refresh_change_from_contents(
                    change.clone(),
                    &base_content,
                    &new_content,
                    current_modified_ms,
                );

                let Some(next_change) = refreshed_change else {
                    changed_any = true;
                    continue;
                };

                if change.unified_diff != next_change.unified_diff
                    || change.added_lines != next_change.added_lines
                    || change.removed_lines != next_change.removed_lines
                    || change.file_modified_ms != next_change.file_modified_ms
                {
                    changed_any = true;
                }

                change = next_change;
            }

            refreshed.push(change);
        }

        if changed_any {
            state.uncommitted_changes.replace_all(refreshed.clone());
        }

        Ok(refreshed)
    })
    .await
    .map_err(|e| format!("get uncommitted changes task failed: {}", e))?
}

#[tauri::command]
pub fn get_uncommitted_change_for_file(
    state: State<'_, AppState>,
    file_path: String,
) -> Option<UncommittedChange> {
    let app_state = state.inner();
    let resolved = resolve_change_for_file_path(app_state, &file_path)?;
    let refreshed = refresh_tracked_change(app_state, resolved.clone());
    match refreshed {
        Some(change) => {
            state.uncommitted_changes.track(change.clone());
            Some(change)
        }
        None => {
            state.uncommitted_changes.accept(&resolved.id);
            None
        }
    }
}

#[tauri::command]
pub fn accept_change(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> Result<UncommittedChange, String> {
    let change = state
        .uncommitted_changes
        .accept(&id)
        .ok_or_else(|| format!("Change not found: {}", id))?;
    schedule_symbol_index_refresh_for_changes(&app, std::slice::from_ref(&change), "accept");
    Ok(change)
}

#[tauri::command]
pub fn accept_file_changes(
    state: State<'_, AppState>,
    app: AppHandle,
    file_path: String,
) -> Result<UncommittedChange, String> {
    let resolved = resolve_change_for_file_path(&*state, &file_path)
        .ok_or_else(|| format!("No uncommitted change for file: {}", file_path))?;
    let change = state
        .uncommitted_changes
        .accept(&resolved.id)
        .ok_or_else(|| format!("Change not found: {}", resolved.id))?;
    schedule_symbol_index_refresh_for_changes(&app, std::slice::from_ref(&change), "accept file");
    Ok(change)
}

#[tauri::command]
pub fn accept_all_changes(state: State<'_, AppState>, app: AppHandle) -> Vec<UncommittedChange> {
    let changes = state.uncommitted_changes.accept_all();
    schedule_symbol_index_refresh_for_changes(&app, &changes, "accept all");
    changes
}

#[tauri::command]
pub async fn reject_change(
    _state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> Result<UncommittedChange, String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let history_service = state.history_service()?;
        let change = state
            .uncommitted_changes
            .reject(&id, history_service.as_ref())?;
        crate::file_state_sync::sync_from_disk_after_write(&app, &change.file_path);
        schedule_symbol_index_refresh_for_changes(&app, std::slice::from_ref(&change), "reject");
        Ok(change)
    })
    .await
    .map_err(|e| format!("reject change task failed: {}", e))?
}

#[tauri::command]
pub async fn reject_file_changes(
    _state: State<'_, AppState>,
    app: AppHandle,
    file_path: String,
) -> Result<UncommittedChange, String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let history_service = state.history_service()?;
        let resolved = resolve_change_for_file_path(&*state, &file_path)
            .ok_or_else(|| format!("No uncommitted change for file: {}", file_path))?;
        let change = state
            .uncommitted_changes
            .reject(&resolved.id, history_service.as_ref())?;
        crate::file_state_sync::sync_from_disk_after_write(&app, &change.file_path);
        schedule_symbol_index_refresh_for_changes(
            &app,
            std::slice::from_ref(&change),
            "reject file",
        );
        Ok(change)
    })
    .await
    .map_err(|e| format!("reject file changes task failed: {}", e))?
}

#[tauri::command]
pub async fn reject_all_changes(
    _state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<UncommittedChange>, String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let history_service = state.history_service()?;
        let changes = state
            .uncommitted_changes
            .reject_all(history_service.as_ref())?;
        for change in &changes {
            crate::file_state_sync::sync_from_disk_after_write(&app, &change.file_path);
        }
        schedule_symbol_index_refresh_for_changes(&app, &changes, "reject all");
        Ok(changes)
    })
    .await
    .map_err(|e| format!("reject all changes task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_refresh_path_is_workspace_relative_when_possible() {
        let root = Path::new("/workspace/project");
        let path = Path::new("/workspace/project/src/main.ts");

        assert_eq!(
            workspace_relative_supported_index_path(Some(root), path),
            Some("src/main.ts".to_string())
        );
    }

    #[test]
    fn index_refresh_path_skips_unsupported_files() {
        assert_eq!(
            workspace_relative_supported_index_path(
                Some(Path::new("/workspace/project")),
                Path::new("/workspace/project/package.json")
            ),
            None
        );
    }
}
