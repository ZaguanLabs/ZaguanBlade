/// Internal Tauri Events for zblade
///
/// This module defines the event contract between zblade's Rust backend and React frontend.
/// Events flow in one direction: Backend → Frontend (via Tauri's emit)
/// Frontend → Backend communication uses invoke() commands instead.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Event names as constants to prevent typos
pub mod event_names {
    // === Chat & AI Workflow ===

    /// AI chat response completed
    pub const CHAT_DONE: &str = "chat-done";

    /// Hook-driven tool approval request from server
    pub const APPROVAL_REQUEST: &str = "approval-request";

    /// Error occurred during chat
    pub const CHAT_ERROR: &str = "chat-error";

    /// Tool execution requires user confirmation
    pub const REQUEST_CONFIRMATION: &str = "request-confirmation";

    /// Tool execution completed successfully
    pub const TOOL_EXECUTION_COMPLETED: &str = "tool-execution-completed";

    /// Command execution completed
    pub const COMMAND_EXECUTED: &str = "command-executed";

    /// Command execution started (with terminal)
    pub const COMMAND_EXECUTION_STARTED: &str = "command-execution-started";

    // === File Edit Workflow ===

    /// File edit proposed by AI, needs user review
    pub const PROPOSE_EDIT: &str = "propose-edit";

    /// Change successfully applied to disk
    pub const CHANGE_APPLIED: &str = "change-applied";

    // === Workspace ===

    /// Request explorer refresh
    pub const REFRESH_EXPLORER: &str = "refresh-explorer";

    // === Chat Session ===

    /// The chat session id was minted or restored (new/loaded/reset
    /// conversation). Warmup context prefetch re-warms under the new key.
    pub const SESSION_ID_CHANGED: &str = "session-id-changed";

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

/// Payload for chat-done event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatDonePayload {
    pub finish_reason: String,
}

/// Payload for chat-error event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatErrorPayload {
    pub code: String,
    pub error: Option<String>,
    pub message: Option<String>,
    pub detail: Option<String>,
    #[serde(rename = "i18nKey")]
    pub i18n_key: Option<String>,
    #[serde(rename = "i18nParams")]
    pub i18n_params: Option<BTreeMap<String, String>>,
}

/// Payload for context-length-exceeded event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextLengthExceededPayload {
    pub message: String,
    pub token_count: Option<u64>,
    pub max_tokens: Option<u64>,
    pub excess: Option<u64>,
    pub recoverable: bool,
    pub recovery_hint: Option<String>,
    #[serde(rename = "titleKey")]
    pub title_key: Option<String>,
    #[serde(rename = "recoverableHintKey")]
    pub recoverable_hint_key: Option<String>,
    #[serde(rename = "nonRecoverableHintKey")]
    pub non_recoverable_hint_key: Option<String>,
}

/// Payload for message-too-large event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTooLargePayload {
    pub message: String,
    pub recovery_hint: String,
    #[serde(rename = "titleKey")]
    pub title_key: Option<String>,
    #[serde(rename = "recoveryHintLabelKey")]
    pub recovery_hint_label_key: Option<String>,
}

/// Payload for request-confirmation event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestConfirmationPayload {
    pub actions: Vec<StructuredAction>,
}

/// Payload for approval-request event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestPayload {
    pub session_id: String,
    pub approval_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub source: Option<String>,
    pub rule_name: Option<String>,
    pub message: Option<String>,
    pub decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredActionKind {
    Command,
    GenericTool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredAction {
    pub id: String,
    pub command: String,
    pub description: String,
    #[serde(rename = "actionKind")]
    pub action_kind: StructuredActionKind,
    #[serde(rename = "descriptionKey")]
    pub description_key: Option<String>,
    #[serde(rename = "descriptionParams")]
    pub description_params: Option<BTreeMap<String, String>>,
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

/// Payload for tool-execution-completed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionCompletedPayload {
    pub tool_name: String,
    pub tool_call_id: String,
    pub success: bool,
    #[serde(default)]
    pub skipped: bool,
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
    #[serde(default)]
    pub file_paths: Vec<String>,
}

/// Payload for command-execution-started event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandExecutionStartedPayload {
    pub command_id: String,
    pub call_id: String,
    pub command: String,
    pub program: Option<String>,
    pub args: Option<Vec<String>>,
    pub shell: Option<bool>,
    pub cwd: Option<String>,
    pub blocking: bool,
    pub wait_ms_before_async: Option<u64>,
}
