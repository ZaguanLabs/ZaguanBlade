//! Core State Snapshot
//!
//! Provides a unified snapshot of all backend-authoritative state.
//! This enables UI reload recovery, multi-window scenarios, and deterministic debugging.

use serde::{Deserialize, Serialize};

use crate::blade_protocol::Version;

/// Complete snapshot of core application state.
/// Used for UI initialization, reload recovery, and debugging.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CoreStateSnapshot {
    /// Protocol version and capabilities
    pub protocol: ProtocolInfo,

    /// Workspace information
    pub workspace: WorkspaceStateSnapshot,

    /// Editor state (active file, cursor, selection)
    pub editor: EditorStateSnapshot,

    /// Chat/conversation state
    pub chat: ChatStateSnapshot,

    /// Terminal sessions
    pub terminals: Vec<TerminalStateSnapshot>,

    /// Pending workflow approvals
    pub pending_approvals: Vec<PendingApproval>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProtocolInfo {
    pub version: Version,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceStateSnapshot {
    pub path: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EditorStateSnapshot {
    pub active_file: Option<String>,
    pub open_files: Vec<String>,
    pub cursor_line: Option<u32>,
    pub cursor_column: Option<u32>,
    pub selection_start: Option<u32>,
    pub selection_end: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatStateSnapshot {
    pub session_id: Option<String>,
    pub message_count: usize,
    pub is_generating: bool,
    pub selected_model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TerminalStateSnapshot {
    pub id: String,
    pub is_active: bool,
    pub has_running_process: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PendingApproval {
    pub id: String,
    pub action_type: String,
    pub description: String,
}

/// Tab information for backend-authoritative tab management
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TabInfo {
    pub id: String,
    pub title: String,
    pub tab_type: TabType,
    pub path: Option<String>,
    pub is_dirty: bool,
}

/// Type of tab content
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "data")]
pub enum TabType {
    /// Regular file tab
    File,
    /// Ephemeral document (unsaved, AI-generated content)
    Ephemeral {
        content: String,
        suggested_name: String,
    },
}

/// Tab state snapshot for UI sync
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TabStateSnapshot {
    pub tabs: Vec<TabInfo>,
    pub active_tab_id: Option<String>,
}

impl CoreStateSnapshot {
    /// Returns the list of capabilities this core supports
    pub fn default_capabilities() -> Vec<String> {
        vec![
            "headless-v1".to_string(),
            "editor-sync".to_string(),
            "state-recovery".to_string(),
        ]
    }
}
