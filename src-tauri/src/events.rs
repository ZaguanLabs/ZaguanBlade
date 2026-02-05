/// Internal Tauri Events for zblade
///
/// This module defines the event contract between zblade's Rust backend and React frontend.
/// Events flow in one direction: Backend → Frontend (via Tauri's emit)
/// Frontend → Backend communication uses invoke() commands instead.
use serde::{Deserialize, Serialize};

/// Event names as constants to prevent typos
pub mod event_names {
    // === Chat & AI Workflow ===

    /// AI chat message chunk received from zcoderd
    pub const CHAT_UPDATE: &str = "chat-update";

    /// AI chat response completed
    pub const CHAT_DONE: &str = "chat-done";

    /// Error occurred during chat
    pub const CHAT_ERROR: &str = "chat-error";

    /// Tool execution requires user confirmation
    pub const REQUEST_CONFIRMATION: &str = "request-confirmation";

    /// Tool execution started
    pub const TOOL_EXECUTION_STARTED: &str = "tool-execution-started";

    /// Tool execution completed successfully
    pub const TOOL_EXECUTION_COMPLETED: &str = "tool-execution-completed";

    /// AI model changed
    pub const MODEL_CHANGED: &str = "model-changed";

    /// Command execution completed
    pub const COMMAND_EXECUTED: &str = "command-executed";

    /// Command execution started (with terminal)
    pub const COMMAND_EXECUTION_STARTED: &str = "command-execution-started";

    // === File Edit Workflow ===

    /// File edit proposed by AI, needs user review
    pub const PROPOSE_EDIT: &str = "propose-edit";

    /// Change successfully applied to disk
    pub const CHANGE_APPLIED: &str = "change-applied";

    /// Change rejected by user
    pub const CHANGE_REJECTED: &str = "change-rejected";

    /// Edit application failed
    pub const EDIT_FAILED: &str = "edit-failed";

    /// All edits applied successfully (Accept All completed)
    pub const ALL_EDITS_APPLIED: &str = "all-edits-applied";

    // === File Operations ===

    /// File opened in editor
    pub const FILE_OPENED: &str = "file-opened";

    /// File closed in editor
    pub const FILE_CLOSED: &str = "file-closed";

    /// File saved to disk
    pub const FILE_SAVED: &str = "file-saved";

    /// File content modified (unsaved)
    pub const FILE_MODIFIED: &str = "file-modified";

    /// Active file/tab changed
    pub const ACTIVE_FILE_CHANGED: &str = "active-file-changed";

    // === Workspace ===

    /// Workspace folder changed
    pub const WORKSPACE_CHANGED: &str = "workspace-changed";

    /// Project files changed (added/deleted)
    pub const PROJECT_FILES_CHANGED: &str = "project-files-changed";

    /// Request explorer refresh
    pub const REFRESH_EXPLORER: &str = "refresh-explorer";

    // === Connection & Status ===

    /// Connection status to zcoderd changed
    pub const CONNECTION_STATUS: &str = "connection-status";

    /// General backend error
    pub const BACKEND_ERROR: &str = "backend-error";

    // === Documents ===

    /// Open ephemeral document (research results, etc)
    pub const OPEN_EPHEMERAL_DOCUMENT: &str = "open-ephemeral-document";

    // === Task Management ===

    /// Todo list updated by AI for task progress tracking
    pub const TODO_UPDATED: &str = "todo_updated";

    // === Terminal ===

    /// Terminal reported a cwd change
    pub const TERMINAL_CWD_CHANGED: &str = "terminal-cwd-changed";

    // === History ===

    /// History entry added (snapshot created)
    pub const HISTORY_ENTRY_ADDED: &str = "history-entry-added";
}

/// Payload for history-entry-added event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntryAddedPayload {
    pub entry: crate::history::HistoryEntry,
}

/// Payload for chat-update event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatUpdatePayload {
    pub content: String,
}

/// Payload for chat-error event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatErrorPayload {
    pub error: String,
}

/// Payload for request-confirmation event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestConfirmationPayload {
    pub actions: Vec<StructuredAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredAction {
    pub id: String,
    pub command: String,
    pub description: String,
    pub cwd: Option<String>,
    pub root_command: Option<String>,
    pub cwd_outside_workspace: Option<bool>,
    pub is_generic_tool: bool,
}

/// Payload for propose-edit event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposeEditPayload {
    pub id: String,
    pub path: String,
    pub old_content: String,
    pub new_content: String,
}

/// Payload for open-ephemeral-document event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenEphemeralDocumentPayload {
    pub id: String,
    pub title: String,
    pub content: String,
}

/// Todo item for task progress tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    #[serde(rename = "activeForm")]
    pub active_form: String,
    pub status: String, // 'pending' | 'in_progress' | 'completed' | 'cancelled'
}

/// Payload for todo_updated event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoUpdatedPayload {
    pub todos: Vec<TodoItem>,
}

/// Payload for terminal-cwd-changed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalCwdChangedPayload {
    pub id: String,
    pub cwd: String,
}

/// Payload for tool-execution-started event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionStartedPayload {
    pub tool_name: String,
    pub tool_call_id: String,
}

/// Payload for tool-execution-completed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionCompletedPayload {
    pub tool_name: String,
    pub tool_call_id: String,
    pub success: bool,
    #[serde(default)]
    pub skipped: bool,
}

/// Payload for model-changed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChangedPayload {
    pub model_id: String,
    pub model_name: String,
}

/// Payload for command-executed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandExecutedPayload {
    pub command: String,
    pub cwd: Option<String>,
    pub output: String,
    #[serde(rename = "exitCode")]
    pub exit_code: i32,
    pub duration: Option<u64>,
    pub call_id: String,
}

/// Payload for change-applied event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeAppliedPayload {
    pub change_id: String,
    pub file_path: String,
}

/// Payload for edit-rejected event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditRejectedPayload {
    pub edit_id: String,
    pub file_path: String,
}

/// Payload for edit-failed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditFailedPayload {
    pub edit_id: String,
    pub file_path: String,
    pub error: String,
}

/// Payload for all-edits-applied event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllEditsAppliedPayload {
    pub count: usize,
    pub file_paths: Vec<String>,
}

/// Payload for file-opened event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOpenedPayload {
    pub file_path: String,
}

/// Payload for file-closed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileClosedPayload {
    pub file_path: String,
}

/// Payload for file-saved event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSavedPayload {
    pub file_path: String,
}

/// Payload for file-modified event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileModifiedPayload {
    pub file_path: String,
    pub has_unsaved_changes: bool,
}

/// Payload for active-file-changed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveFileChangedPayload {
    pub file_path: Option<String>,
    pub previous_file_path: Option<String>,
}

/// Payload for workspace-changed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceChangedPayload {
    pub workspace_path: String,
}

/// Payload for project-files-changed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFilesChangedPayload {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}

/// Connection status enum
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Reconnecting,
    Error,
}

/// Payload for connection-status event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatusPayload {
    pub status: ConnectionStatus,
    pub message: Option<String>,
}

/// Payload for command-execution-started event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandExecutionStartedPayload {
    pub command_id: String,
    pub call_id: String,
    pub command: String,
    pub cwd: Option<String>,
}

/// Payload for backend-error event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendErrorPayload {
    pub error: String,
    pub context: Option<String>,
}
