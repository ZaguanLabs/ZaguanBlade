use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Per-project state that persists across sessions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectState {
    /// The project directory this state belongs to
    pub project_path: String,

    /// Currently active file path (relative to project root)
    pub active_file: Option<String>,

    /// List of open file tabs
    pub open_tabs: Vec<TabState>,

    /// ID of the currently selected AI model
    pub selected_model_id: Option<String>,

    /// Terminal pane state
    pub terminals: Vec<TerminalState>,

    /// Active terminal ID
    pub active_terminal_id: Option<String>,

    /// Terminal pane height in pixels
    pub terminal_height: Option<u32>,

    /// Chat panel width in pixels
    pub chat_panel_width: Option<u32>,

    /// Explorer panel width in pixels
    pub explorer_width: Option<u32>,
}

/// State for a single editor tab
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabState {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub tab_type: String, // "file" or "ephemeral"
    pub path: Option<String>,
}

/// State for a single terminal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalState {
    pub id: String,
    pub title: String,
    pub cwd: Option<String>,
}

/// Get the config directory for ZaguanBlade state files
/// Uses OS-specific paths:
/// - Linux: ~/.config/zaguanblade/
/// - macOS: ~/Library/Application Support/com.zaguan.zblade/
/// - Windows: C:\Users\<User>\AppData\Roaming\zaguan\zblade\
fn get_state_dir() -> Option<PathBuf> {
    ProjectDirs::from("com", "zaguan", "zblade").map(|dirs| dirs.config_dir().join("projects"))
}

/// Generate a unique filename for a project based on its path
fn project_state_filename(project_path: &str) -> String {
    let mut hasher = DefaultHasher::new();
    project_path.hash(&mut hasher);
    let hash = hasher.finish();

    // Use last component of path + hash for human readability
    let name = Path::new(project_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    format!("{}-{:016x}.json", name, hash)
}

/// Get the full path to a project's state file
pub fn get_project_state_path(project_path: &str) -> Option<PathBuf> {
    get_state_dir().map(|dir| dir.join(project_state_filename(project_path)))
}

/// Load project state from disk
pub fn load_project_state(project_path: &str) -> Option<ProjectState> {
    let state_path = get_project_state_path(project_path)?;

    if !state_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&state_path).ok()?;
    let mut state: ProjectState = serde_json::from_str(&content).ok()?;

    // Validate that this state is for the correct project
    if state.project_path != project_path {
        return None;
    }

    // Filter out tabs for files that no longer exist
    let project_root = Path::new(project_path);
    state.open_tabs.retain(|tab| {
        if tab.tab_type == "ephemeral" {
            return false; // Don't restore ephemeral tabs
        }
        if let Some(ref path) = tab.path {
            let full_path = project_root.join(path);
            full_path.exists()
        } else {
            false
        }
    });

    // Update active_file if it no longer exists
    if let Some(ref active) = state.active_file {
        let full_path = project_root.join(active);
        if !full_path.exists() {
            state.active_file = state.open_tabs.first().and_then(|t| t.path.clone());
        }
    }

    Some(state)
}

/// Save project state to disk
pub fn save_project_state(state: &ProjectState) -> Result<(), String> {
    let state_dir = get_state_dir().ok_or_else(|| "Could not determine config directory".to_string())?;

    // Ensure directory exists
    fs::create_dir_all(&state_dir).map_err(|e| format!("Failed to create state directory: {}", e))?;

    let state_path = state_dir.join(project_state_filename(&state.project_path));

    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize state: {}", e))?;

    fs::write(&state_path, json).map_err(|e| format!("Failed to write state file: {}", e))?;

    Ok(())
}

/// Delete project state from disk
pub fn delete_project_state(project_path: &str) -> Result<(), String> {
    if let Some(state_path) = get_project_state_path(project_path) {
        if state_path.exists() {
            fs::remove_file(&state_path)
                .map_err(|e| format!("Failed to delete state file: {}", e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_state_filename() {
        let path1 = "/home/user/projects/myapp";
        let path2 = "/home/user/projects/myapp"; // Same path
        let path3 = "/home/user/projects/otherapp";

        let name1 = project_state_filename(path1);
        let name2 = project_state_filename(path2);
        let name3 = project_state_filename(path3);

        assert_eq!(name1, name2); // Same path = same filename
        assert_ne!(name1, name3); // Different path = different filename
        assert!(name1.starts_with("myapp-"));
        assert!(name3.starts_with("otherapp-"));
    }

    #[test]
    fn test_state_serialization() {
        let state = ProjectState {
            project_path: "/test/project".to_string(),
            active_file: Some("src/main.rs".to_string()),
            open_tabs: vec![TabState {
                id: "file-src/main.rs".to_string(),
                title: "main.rs".to_string(),
                tab_type: "file".to_string(),
                path: Some("src/main.rs".to_string()),
            }],
            selected_model_id: Some("anthropic/claude-sonnet-4-5-20250929".to_string()),
            terminals: vec![TerminalState {
                id: "term-1".to_string(),
                title: "zsh".to_string(),
                cwd: None,
            }],
            active_terminal_id: Some("term-1".to_string()),
            terminal_height: Some(300),
            chat_panel_width: Some(400),
            explorer_width: Some(256),
        };

        let json = serde_json::to_string(&state).unwrap();
        let restored: ProjectState = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.project_path, state.project_path);
        assert_eq!(restored.active_file, state.active_file);
        assert_eq!(restored.open_tabs.len(), 1);
        assert_eq!(restored.selected_model_id, state.selected_model_id);
    }
}
