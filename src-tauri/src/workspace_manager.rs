use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct WorkspaceState {
    pub last_workspace: Option<String>,
    pub recent_workspaces: Vec<String>,
}

pub struct WorkspaceManager {
    pub workspace: Option<PathBuf>,
    state_path: PathBuf,
}

impl WorkspaceManager {
    pub fn new() -> Self {
        let state_path = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("zaguan")
            .join("workspace_state.json");

        // Load last workspace if available
        let workspace = if state_path.exists() {
            if let Ok(content) = fs::read_to_string(&state_path) {
                if let Ok(state) = serde_json::from_str::<WorkspaceState>(&content) {
                    state.last_workspace.and_then(|p| {
                        let path = PathBuf::from(p);
                        if path.exists() && path.is_dir() {
                            Some(path)
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        Self {
            workspace,
            state_path,
        }
    }

    pub fn set_workspace(&mut self, path: PathBuf) {
        let path = match fs::canonicalize(&path) {
            Ok(p) => p,
            Err(_) => path,
        };

        if path.is_dir() {
            self.workspace = Some(path.clone());
            self.save_state();
        } else if let Some(parent) = path.parent() {
            self.workspace = Some(parent.to_path_buf());
            self.save_state();
        }
    }

    pub fn get_workspace_root(&self) -> Option<String> {
        self.workspace
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    fn save_state(&self) {
        if let Some(workspace) = &self.workspace {
            let workspace_str = workspace.to_string_lossy().to_string();

            // Load existing state or create new
            let mut state = if self.state_path.exists() {
                if let Ok(content) = fs::read_to_string(&self.state_path) {
                    serde_json::from_str::<WorkspaceState>(&content).unwrap_or_else(|_| {
                        WorkspaceState {
                            last_workspace: None,
                            recent_workspaces: Vec::new(),
                        }
                    })
                } else {
                    WorkspaceState {
                        last_workspace: None,
                        recent_workspaces: Vec::new(),
                    }
                }
            } else {
                WorkspaceState {
                    last_workspace: None,
                    recent_workspaces: Vec::new(),
                }
            };

            // Update state
            state.last_workspace = Some(workspace_str.clone());

            // Add to recent workspaces (keep last 10)
            state.recent_workspaces.retain(|p| p != &workspace_str);
            state.recent_workspaces.insert(0, workspace_str);
            state.recent_workspaces.truncate(10);

            // Save state
            if let Some(parent) = self.state_path.parent() {
                let _ = fs::create_dir_all(parent);
            }

            if let Ok(json) = serde_json::to_string_pretty(&state) {
                let _ = fs::write(&self.state_path, json);
            }
        }
    }

    pub fn get_recent_workspaces(&self) -> Vec<String> {
        if self.state_path.exists() {
            if let Ok(content) = fs::read_to_string(&self.state_path) {
                if let Ok(state) = serde_json::from_str::<WorkspaceState>(&content) {
                    return state.recent_workspaces;
                }
            }
        }
        Vec::new()
    }
}
