use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ==============================================================================
// 0. Version (v1.3) - Added Language domain for ZLP (tree-sitter based)
// ==============================================================================

/// Semantic version for protocol compatibility checking
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl Version {
    pub const CURRENT: Version = Version {
        major: 1,
        minor: 3,
        patch: 0,
    };

    /// Check if two versions are compatible (same major version)
    pub fn is_compatible(&self, other: &Version) -> bool {
        self.major == other.major
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ==============================================================================
// 1. Envelopes
// ==============================================================================

/// The standard envelope for all Client -> Server communication.
#[derive(Debug, Serialize, Deserialize)]
pub struct BladeEnvelope<T> {
    pub protocol: String, // Must be "BCP"
    pub version: Version, // Semantic version
    pub domain: String,
    pub message: T,
}

/// The causality envelope for Intents.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BladeIntentEnvelope {
    pub id: Uuid,                        // Correlation ID
    pub timestamp: u64,                  // Client-side timestamp (ms since epoch)
    pub idempotency_key: Option<String>, // Optional: prevents double-execution on retry
    pub intent: BladeIntent,
}

/// The causality envelope for Events.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BladeEventEnvelope {
    pub id: Uuid,                     // Event ID
    pub timestamp: u64,               // Server-side timestamp
    pub causality_id: Option<String>, // ID of the Intent that caused this (String to support non-UUID legacy IDs)
    pub event: BladeEvent,
}

// ==============================================================================
// 2. Intents (Writes)
// ==============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum BladeIntent {
    Chat(ChatIntent),
    Editor(EditorIntent),
    File(FileIntent),
    Workflow(WorkflowIntent),
    Terminal(TerminalIntent),
    History(HistoryIntent),
    System(SystemIntent),
    Language(LanguageIntent), // v1.3: ZLP (tree-sitter based language protocol)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum ChatIntent {
    SendMessage {
        content: String,
        model: String,
        #[serde(default)]
        context: Option<EditorContext>,
    },
    StopGeneration,
    ClearHistory,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EditorContext {
    pub active_file: Option<String>,
    pub open_files: Vec<String>,
    pub cursor_line: Option<u32>,
    pub cursor_column: Option<u32>,
    pub selection_start: Option<u32>,
    pub selection_end: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum EditorIntent {
    /// Open a file (adds to open files, sets as active)
    OpenFile { path: String },
    /// Close a file (removes from open files)
    CloseFile { path: String },
    /// Set the active file (must already be in open files, or None to clear)
    SetActiveFile { path: Option<String> },
    /// Update cursor position (for AI context)
    UpdateCursor { line: u32, column: u32 },
    /// Update selection (for AI context)
    UpdateSelection { start: u32, end: u32 },
    /// Request current editor state snapshot
    GetState {},
    // Tab management (headless)
    /// Open a tab (file or ephemeral)
    OpenTab {
        id: String,
        title: String,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        tab_type: Option<String>, // "file" or "ephemeral"
        #[serde(default)]
        content: Option<String>, // For ephemeral tabs
        #[serde(default)]
        suggested_name: Option<String>, // For ephemeral tabs
    },
    /// Close a tab by ID
    CloseTab { tab_id: String },
    /// Set the active tab
    SetActiveTab { tab_id: Option<String> },
    /// Reorder tabs
    ReorderTabs { tab_ids: Vec<String> },
    /// Request tab state snapshot
    GetTabState {},
    /// Legacy: save file
    SaveFile { path: String },
    /// Legacy: virtual buffer update
    BufferUpdate { path: String, content: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum FileIntent {
    Read { path: String },
    Write { path: String, content: String },
    List { path: Option<String> }, // None = root workspace
    Create { path: String, is_dir: bool },
    Delete { path: String },
    Rename { old_path: String, new_path: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Option<Vec<FileEntry>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum WorkflowIntent {
    ApproveAction { action_id: String },
    ApproveAll { batch_id: String },
    RejectAction { action_id: String },
    RejectAll { batch_id: String },
    // Legacy support
    ApproveChange { change_id: String },
    RejectChange { change_id: String },
    ApproveAllChanges,
    ApproveTool { approved: bool },
    ApproveToolDecision { decision: String },
}

/// Terminal ownership for tracking who spawned a terminal
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "type", content = "data")]
pub enum TerminalOwner {
    User,
    Agent { task_id: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum TerminalIntent {
    Spawn {
        id: String,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        owner: Option<TerminalOwner>, // v1.1: typed owner
        #[serde(default)]
        interactive: bool, // true = shell (create_terminal), false = command (execute_command)
    },
    Input {
        id: String,
        data: String,
    },
    Resize {
        id: String,
        rows: u16,
        cols: u16,
    },
    Kill {
        id: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum HistoryIntent {
    ListConversations { project_id: String },
    LoadConversation { session_id: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum SystemIntent {
    // For bootstrapping or config
    SetLogLevel { level: String },
}

// v1.3: Language domain for ZLP (tree-sitter based language protocol)
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum LanguageIntent {
    // Symbol operations (tree-sitter)
    IndexFile {
        file_path: String,
    },
    IndexWorkspace,
    SearchSymbols {
        query: String,
        #[serde(default)]
        file_path: Option<String>, // Optional: limit to specific file
        #[serde(default)]
        symbol_types: Option<Vec<String>>, // Optional: filter by type
    },
    GetSymbolAt {
        file_path: String,
        line: u32,
        character: u32,
    },
    // Project context indexing
    GetFullContext {
        #[serde(default)]
        max_files: Option<usize>,
        #[serde(default)]
        preview_lines: Option<usize>,
    },
    // Raw ZLP operations (v1.4)
    ZlpMessage {
        payload: serde_json::Value,
    },
}

// ==============================================================================
// 3. Events (Reads/Updates)
// ==============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum BladeEvent {
    Chat(ChatEvent),
    Editor(EditorEvent),
    File(FileEvent),
    Workflow(WorkflowEvent),
    Terminal(TerminalEvent),
    History(HistoryEvent),
    System(SystemEvent),
    Language(LanguageEvent), // v1.3: ZLP (tree-sitter based language protocol)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum ChatEvent {
    ChatState {
        messages: Vec<crate::protocol::ChatMessage>,
    }, // Full State
    MessageDelta {
        id: String,
        seq: u64, // Monotonic sequence number
        chunk: String,
        is_final: bool, // True on last chunk
    }, // Streaming
    ReasoningDelta {
        id: String,
        seq: u64,
        chunk: String,
        is_final: bool,
    },
    MessageCompleted {
        id: String, // Explicit end-of-stream signal
    },
    ToolUpdate {
        message_id: String,
        tool_call_id: String,
        status: String,
        result: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_call: Option<crate::protocol::ToolCall>,
    },
    GenerationSignal {
        is_generating: bool,
    }, // Signal
    ToolActivity {
        tool_name: String,
        file_path: String,
        action: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolActivityPayload {
    pub tool_name: String,
    pub file_path: String,
    pub action: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum EditorEvent {
    /// Full editor state snapshot (for recovery/sync)
    StateSnapshot {
        active_file: Option<String>,
        open_files: Vec<String>,
        cursor_line: Option<u32>,
        cursor_column: Option<u32>,
        selection_start: Option<u32>,
        selection_end: Option<u32>,
    },
    /// File was opened and is now in the open files list
    FileOpened { path: String },
    /// File was closed and removed from open files list
    FileClosed { path: String },
    /// Active file changed
    ActiveFileChanged { path: Option<String> },
    /// Cursor position changed (debounced, not every keystroke)
    CursorMoved { line: u32, column: u32 },
    /// Selection changed
    SelectionChanged { start: u32, end: u32 },
    // Tab events (headless)
    /// Tab was opened
    TabOpened { tab: crate::core_state::TabInfo },
    /// Tab was closed
    TabClosed { tab_id: String },
    /// Active tab changed
    ActiveTabChanged { tab_id: Option<String> },
    /// Tabs were reordered
    TabsReordered { tab_ids: Vec<String> },
    /// Full tab state snapshot
    TabStateSnapshot {
        tabs: Vec<crate::core_state::TabInfo>,
        active_tab_id: Option<String>,
    },
    /// Legacy: minimal state
    EditorState { active_file: Option<String> },
    /// Legacy: content delta
    ContentDelta { file: String, patch: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum FileEvent {
    Content {
        path: String,
        data: String,
    },
    Written {
        path: String,
    },
    Listing {
        path: Option<String>,
        entries: Vec<FileEntry>,
    },
    Created {
        path: String,
        is_dir: bool,
    },
    Deleted {
        path: String,
    },
    Renamed {
        old_path: String,
        new_path: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum WorkflowEvent {
    ApprovalRequested {
        batch_id: Uuid,
        items: Vec<String>, // Simplified for now, will hold structured actions
    },
    ActionCompleted {
        action_id: String,
        success: bool,
        result: Option<String>,
    },
    BatchCompleted {
        batch_id: String,
        succeeded: usize,
        failed: usize,
    },
    // Legacy support
    TaskCompleted {
        task_id: Uuid,
        success: bool,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum TerminalEvent {
    Spawned {
        id: String,
        owner: TerminalOwner,
    },
    Output {
        id: String,
        seq: u64, // v1.1: sequence number for ordering
        data: String,
    },
    Exit {
        id: String,
        code: i32,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub created_at: String,
    pub last_active_at: String,
    pub message_count: u32,
    pub preview: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum HistoryEvent {
    ConversationList {
        conversations: Vec<ConversationSummary>,
    },
    ConversationLoaded(FullConversation),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FullConversation {
    pub session_id: String,
    pub project_id: String,
    pub title: String,
    pub created_at: String,
    pub last_active_at: String,
    pub message_count: u32,
    pub messages: Vec<HistoryMessage>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum SystemEvent {
    // v1.1: Protocol version negotiation
    ProtocolVersion {
        supported: Vec<Version>,
        current: Version,
    },
    IntentFailed {
        intent_id: Uuid,
        error: BladeError,
    },
    ProcessStarted {
        intent_id: Uuid,
    },
    ProcessProgress {
        intent_id: Uuid,
        progress: f32,           // 0.0 to 1.0
        message: Option<String>, // Optional status message
    },
    ProcessCompleted {
        intent_id: Uuid,
    },
}

// v1.3: Language domain events
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum LanguageEvent {
    // Symbol events
    FileIndexed {
        file_path: String,
        symbol_count: usize,
    },
    WorkspaceIndexed {
        file_count: usize,
        symbol_count: usize,
        duration_ms: u64,
    },
    SymbolsFound {
        intent_id: Uuid,
        symbols: Vec<LanguageSymbol>,
    },
    SymbolAt {
        intent_id: Uuid,
        symbol: Option<LanguageSymbol>,
    },
    // Project context indexing
    FullContextGenerated {
        intent_id: Uuid,
        file_path: String,
        file_count: usize,
    },
    // Raw ZLP response (v1.4)
    ZlpResponse {
        original_request_id: String,
        payload: serde_json::Value,
    },
}

// ... existing structs ...

// Language domain data types
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LanguageSymbol {
    pub id: String,
    pub name: String,
    pub symbol_type: String,
    pub file_path: String,
    pub range: LanguageRange,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docstring: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct LanguagePosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct LanguageRange {
    pub start: LanguagePosition,
    pub end: LanguagePosition,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LanguageLocation {
    pub file_path: String,
    pub range: LanguageRange,
}

// ==============================================================================
// 4. Error Model
// ==============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "code", content = "details")]
pub enum BladeError {
    ValidationError {
        field: String,
        message: String,
    },
    PermissionDenied,
    ResourceNotFound {
        id: String,
    },
    Conflict {
        reason: String,
    },
    Internal {
        trace_id: String,
        message: String,
    },
    VersionMismatch {
        expected: Version,
        received: Version,
    },
    Timeout {
        timeout_ms: u64,
    },
    RateLimited {
        retry_after_ms: u64,
    },
}

/// Result type for Blade Protocol operations
pub type BladeResult<T> = Result<T, BladeError>;

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_intent_envelope_serialization() {
        let id = Uuid::new_v4();
        let timestamp = 1700000000;
        let intent = BladeIntent::Chat(ChatIntent::SendMessage {
            content: "Hello World".to_string(),
            model: "gwt-5".to_string(),
            context: None,
        });

        let envelope = BladeIntentEnvelope {
            id,
            timestamp,
            idempotency_key: None,
            intent: intent.clone(),
        };

        let json = serde_json::to_string(&envelope).expect("Failed to serialize intent envelope");
        let deserialized: BladeIntentEnvelope =
            serde_json::from_str(&json).expect("Failed to deserialize intent envelope");

        assert_eq!(envelope.id, deserialized.id);
        assert_eq!(envelope.timestamp, deserialized.timestamp);

        // Match intent variants manually since PartialEq is not derived for all
        if let BladeIntent::Chat(ChatIntent::SendMessage { content, .. }) = deserialized.intent {
            assert_eq!(content, "Hello World");
        } else {
            panic!("Deserialized intent has wrong type");
        }
    }

    #[test]
    fn test_event_envelope_serialization() {
        let id = Uuid::new_v4();
        let causality_id = Uuid::new_v4();
        let timestamp = 1700000001;
        let event = BladeEvent::Chat(ChatEvent::GenerationSignal {
            is_generating: true,
        });

        let envelope = BladeEventEnvelope {
            id,
            timestamp,
            causality_id: Some(causality_id.to_string()),
            event: event.clone(),
        };

        let json = serde_json::to_string(&envelope).expect("Failed to serialize event envelope");
        let deserialized: BladeEventEnvelope =
            serde_json::from_str(&json).expect("Failed to deserialize event envelope");

        assert_eq!(envelope.id, deserialized.id);
        assert_eq!(envelope.causality_id, deserialized.causality_id);

        if let BladeEvent::Chat(ChatEvent::GenerationSignal { is_generating }) = deserialized.event
        {
            assert!(is_generating);
        } else {
            panic!("Deserialized event has wrong type");
        }
    }

    #[test]
    fn test_error_serialization() {
        let error = BladeError::Timeout { timeout_ms: 5000 };
        let json = serde_json::to_string(&error).expect("Failed to serialize error");

        // Verify structure: { "code": "Timeout", "details": { "timeout_ms": 5000 } }
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("Failed to parse error JSON");
        assert_eq!(value["code"], "Timeout");
        assert_eq!(value["details"]["timeout_ms"], 5000);
    }
}
