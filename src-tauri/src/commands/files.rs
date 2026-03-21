use crate::app_state::AppState;
use crate::gitignore_filter::GitignoreFilter;
use serde::Serialize;
use tauri::{Emitter, Manager};
use walkdir::WalkDir;

fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;

    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => normalized.push(p.as_os_str()),
            Component::RootDir => normalized.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(c) => normalized.push(c),
        }
    }
    normalized
}

pub(crate) fn resolve_path_under_workspace_root(
    workspace_root: &std::path::Path,
    path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let ws = std::fs::canonicalize(workspace_root).map_err(|e| {
        format!(
            "Failed to canonicalize workspace root '{}': {}",
            workspace_root.display(),
            e
        )
    })?;

    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        ws.join(path)
    };
    let normalized = normalize_path(&candidate);

    if !normalized.starts_with(&ws) {
        return Err(format!(
            "Path is outside workspace (workspace: {}, path: {})",
            ws.display(),
            normalized.display()
        ));
    }

    Ok(normalized)
}

pub(crate) fn resolve_path_under_workspace(
    state: &AppState,
    path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let workspace_root = {
        let ws = state.workspace.lock().unwrap();
        ws.workspace
            .clone()
            .ok_or_else(|| "No workspace open".to_string())?
    };

    resolve_path_under_workspace_root(&workspace_root, path)
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspacePathMatch {
    pub path: String,
    pub is_dir: bool,
}

fn create_gitignore_filter(workspace_root: &std::path::Path) -> Option<GitignoreFilter> {
    let settings = crate::project_settings::load_project_settings_or_default(workspace_root);
    if settings.allow_gitignored_files {
        return None;
    }
    Some(GitignoreFilter::new(workspace_root))
}

fn normalize_rel_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn path_match_rank(query: &str, rel_path: &str) -> Option<(u8, usize, usize)> {
    if query.is_empty() {
        return Some((3, rel_path.len(), rel_path.matches('/').count()));
    }

    let query_lower = query.to_lowercase();
    let path_lower = rel_path.to_lowercase();
    let file_name = rel_path
        .rsplit('/')
        .next()
        .unwrap_or(rel_path)
        .to_lowercase();

    if file_name.starts_with(&query_lower) {
        return Some((0, rel_path.len(), rel_path.matches('/').count()));
    }
    if path_lower.starts_with(&query_lower) {
        return Some((1, rel_path.len(), rel_path.matches('/').count()));
    }
    if path_lower.contains(&query_lower) {
        return Some((2, rel_path.len(), rel_path.matches('/').count()));
    }

    None
}

pub fn search_workspace_paths_logic(
    query: String,
    limit: Option<usize>,
    state: &AppState,
) -> Result<Vec<WorkspacePathMatch>, String> {
    let workspace_root = {
        let ws = state.workspace.lock().unwrap();
        ws.workspace
            .clone()
            .ok_or_else(|| "No workspace open".to_string())?
    };
    let workspace_root = std::fs::canonicalize(&workspace_root).map_err(|e| e.to_string())?;
    let gitignore_filter = create_gitignore_filter(&workspace_root);
    let query = query.trim().replace('\\', "/");
    let max_results = limit.unwrap_or(12).min(50);

    let mut matches = Vec::new();
    for entry in WalkDir::new(&workspace_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                ".git" | "node_modules" | "target" | "dist" | "build" | ".zblade"
            )
        })
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path == workspace_root {
            continue;
        }
        if let Some(ref filter) = gitignore_filter {
            if filter.should_ignore(path) {
                continue;
            }
        }

        let Ok(rel_path) = path.strip_prefix(&workspace_root) else {
            continue;
        };
        let rel = normalize_rel_path(rel_path);
        let Some(rank) = path_match_rank(&query, &rel) else {
            continue;
        };

        matches.push((
            rank,
            WorkspacePathMatch {
                path: rel,
                is_dir: entry.file_type().is_dir(),
            },
        ));
    }

    matches.sort_by(|(left_rank, left_match), (right_rank, right_match)| {
        left_rank
            .cmp(right_rank)
            .then_with(|| left_match.path.cmp(&right_match.path))
    });
    matches.truncate(max_results);

    Ok(matches
        .into_iter()
        .map(|(_, path_match)| path_match)
        .collect())
}

pub async fn open_workspace_logic(
    path: String,
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let mut ws = state.workspace.lock().unwrap();
    ws.set_workspace(std::path::PathBuf::from(&path));
    drop(ws);

    // Re-discover git repository for new workspace
    let new_git_dir = match gix::discover(&path) {
        Ok(repo) => {
            let git_path = repo.path().to_path_buf();
            eprintln!("[GIT] Discovered repository at: {:?}", git_path);
            Some(git_path)
        }
        Err(e) => {
            eprintln!("[GIT] No repository found: {}", e);
            None
        }
    };
    *state.git_dir.write().unwrap() = new_git_dir;

    // Ensure project-local .zblade exists for workspace-scoped persistence
    let workspace_root = std::path::PathBuf::from(&path);
    crate::project_settings::init_zblade_dir(&workspace_root)?;
    state.reset_project_services()?;

    crate::fs_watcher::restart_fs_watcher(app_handle);
    let _ = app_handle.emit(crate::events::event_names::REFRESH_EXPLORER, ());

    if state
        .startup_services_started
        .load(std::sync::atomic::Ordering::Acquire)
    {
        let language_service = state.language_service()?;
        tokio::task::spawn_blocking(move || {
            let _ = language_service.index_directory(".");
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn open_workspace(
    path: String,
    state: tauri::State<'_, AppState>,
    window: tauri::Window,
) -> Result<(), String> {
    open_workspace_logic(path, &*state, &window.app_handle()).await
}

pub fn list_files_logic(
    path: Option<String>,
    state: &AppState,
) -> Result<Vec<crate::explorer::FileEntry>, String> {
    let ws = state.workspace.lock().unwrap();
    let root = if let Some(p) = path {
        std::path::PathBuf::from(p)
    } else if let Some(w) = &ws.workspace {
        w.clone()
    } else {
        return Err("No workspace open".to_string());
    };

    Ok(crate::explorer::list_directory(&root))
}

#[tauri::command]
pub async fn list_files(
    path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::explorer::FileEntry>, String> {
    list_files_logic(path, &*state)
}

#[tauri::command]
pub async fn search_workspace_paths(
    query: String,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WorkspacePathMatch>, String> {
    search_workspace_paths_logic(query, limit, &*state)
}

pub fn read_file_content_logic(path: String, state: &AppState) -> Result<String, String> {
    // Virtual buffers removal - surgically removed.

    let requested_path = std::path::PathBuf::from(&path);
    let resolved_path = resolve_path_under_workspace(state, &requested_path)?;

    // No virtual content, read from disk
    match std::fs::read_to_string(&resolved_path) {
        Ok(content) => {
            if content.is_empty() {
                println!(
                    "[READ FILE CONTENT] Read empty content from: {} (requested: {})",
                    resolved_path.display(),
                    path
                );
            }
            Ok(content)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "[READ FILE CONTENT] Not found: {} (requested: {})",
                resolved_path.display(),
                path
            );
            Ok(String::new())
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn read_file_content(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    read_file_content_logic(path, &*state)
}

pub fn write_file_content_logic(
    path: String,
    content: String,
    state: &AppState,
) -> Result<(), String> {
    let requested_path = std::path::PathBuf::from(&path);
    let resolved_path = resolve_path_under_workspace(state, &requested_path)?;

    std::fs::write(&resolved_path, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn write_file_content(
    path: String,
    content: String,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    write_file_content_logic(path, content, &*state)?;
    let _ = app_handle.emit(crate::events::event_names::REFRESH_EXPLORER, ());
    Ok(())
}

#[tauri::command]
pub async fn open_file_in_editor(path: String, window: tauri::Window) -> Result<(), String> {
    // Emit the open-file event to trigger the frontend to open the file
    window.emit("open-file", &path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_path_blocks_parent_traversal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let res =
            resolve_path_under_workspace_root(dir.path(), std::path::Path::new("../../etc/passwd"));
        assert!(res.is_err());
    }

    #[test]
    fn resolve_path_blocks_absolute_outside_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = std::env::temp_dir().join("outside-zblade-test.txt");
        let res = resolve_path_under_workspace_root(dir.path(), &outside);
        assert!(res.is_err());
    }

    #[test]
    fn resolve_path_allows_in_workspace_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved =
            resolve_path_under_workspace_root(dir.path(), std::path::Path::new("src/main.rs"))
                .expect("path should resolve");
        assert!(resolved.starts_with(dir.path()));
        assert!(resolved.ends_with("src/main.rs"));
    }
}
