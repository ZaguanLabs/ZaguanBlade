use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::language_service::IndexHealthSnapshot;

// ==============================================================================
// 0. Version (v1.5) - Added versioned event transport for batched blade-event delivery
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
        minor: 6,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "transport", content = "payload")]
pub enum BladeEventTransport {
    Single(BladeEventEnvelope),
    Batch(Vec<BladeEventEnvelope>),
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
        images: Option<Vec<crate::protocol::ChatImage>>,
        #[serde(default)]
        context: Option<EditorContext>,
        #[serde(default)]
        mentions: Option<Vec<ChatMention>>,
        #[serde(default)]
        mode: Option<String>,
    },
    StopGeneration {},
    ClearHistory {},
    NewConversation {
        model: String,
    },
    SetSelectedModel {
        model: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMention {
    pub kind: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EditorContext {
    pub active_file: Option<String>,
    pub open_files: Vec<String>,
    pub cursor_line: Option<u32>,
    pub cursor_column: Option<u32>,
    pub selection_start: Option<u32>,
    pub selection_end: Option<u32>,
    #[serde(default)]
    pub diagnostics: Vec<WorkspaceDiagnostic>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceDiagnostic {
    pub source: String,
    pub severity: String,
    pub message: String,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub column: Option<u32>,
    #[serde(default)]
    pub raw: Option<String>,
    #[serde(default, alias = "terminalId")]
    pub terminal_id: Option<String>,
    #[serde(default)]
    pub timestamp: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum EditorIntent {
    /// Open a file (adds to open files, sets as active)
    OpenFile {
        path: String,
    },
    /// Close a file (removes from open files)
    CloseFile {
        path: String,
    },
    /// Set the active file (must already be in open files, or None to clear)
    SetActiveFile {
        path: Option<String>,
    },
    SyncDocument {
        path: String,
        content: String,
        version: u32,
    },
    CloseDocument {
        path: String,
    },
    /// Update cursor position (for AI context)
    UpdateCursor {
        line: u32,
        column: u32,
    },
    /// Update selection (for AI context)
    UpdateSelection {
        start: u32,
        end: u32,
    },
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
    CloseTab {
        tab_id: String,
    },
    /// Set the active tab
    SetActiveTab {
        tab_id: Option<String>,
    },
    /// Reorder tabs
    ReorderTabs {
        tab_ids: Vec<String>,
    },
    /// Request tab state snapshot
    GetTabState {},
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
        #[serde(default)]
        hidden: bool,
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
    IndexWorkspace {},
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
    // Note: field name avoids collision with serde content="payload"
    ZlpMessage {
        data: serde_json::Value,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
    },
    CognitiveInterrupt {
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        level: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        frame: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        next_action_type: Option<String>,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cleared_by_tool: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        failure_count: Option<u64>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolActivityPayload {
    pub tool_name: String,
    pub file_path: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
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
    IndexStatus {
        health: crate::language_service::IndexHealthSnapshot,
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
    // Note: field name avoids collision with serde content="payload"
    ZlpResponse {
        original_request_id: String,
        result: serde_json::Value,
    },
}

// ... existing structs ...

// Language domain data types
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LanguageSymbol {
    pub id: String,
    pub name: String,
    pub qualified_name: String,
    pub symbol_type: String,
    pub file_path: String,
    pub range: LanguageRange,
    pub byte_offset: usize,
    pub byte_length: usize,
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
// 3.5 Context Pack Protocol (v1.6+)
// ==============================================================================

/// Response payload for a context pack request
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextPackPayload {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queries_used: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<ContextWorkspace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_context: Option<ContextProjectInfo>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default)]
    pub confidence: ContextPackConfidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub primary_files: Vec<ContextFileResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_tests: Vec<ContextFileResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_docs: Vec<ContextFileResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memories: Vec<ContextMemoryItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hypothesized_flow: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enriched_files: Vec<ContextFileEnrichment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_files: Vec<ContextRelatedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<ContextImpactSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_health: Option<IndexHealthSnapshot>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recommended_next_step: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ContextPackError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextWorkspace {
    pub root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextProjectInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_index_min: Option<String>,
    #[serde(default)]
    pub project_index_min_available: bool,
    #[serde(default)]
    pub project_index_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_index_path: Option<String>,
    #[serde(default)]
    pub project_index_min_requested: bool,
    #[serde(default)]
    pub project_index_min_included: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_index_min_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_files: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_files: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_files: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_files: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orphaned_files: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<ProjectLanguageSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_directories: Vec<ProjectDirectorySummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub likely_entry_points: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_health_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_instruction_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_instruction_includes: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub agent_instructions_truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_skills: Vec<ContextSkillSummary>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextSkillSummary {
    pub skill_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectLanguageSummary {
    pub name: String,
    pub files: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectDirectorySummary {
    pub path: String,
    pub files: usize,
}

/// Confidence level for context pack results
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackConfidence {
    High,
    Medium,
    Low,
}

impl Default for ContextPackConfidence {
    fn default() -> Self {
        Self::Low
    }
}

/// A file result in a context pack response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextFileResult {
    pub path: String,
    #[serde(default)]
    pub score: u32,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_ranges: Vec<ContextRange>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextFileEnrichment {
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbol_summaries: Vec<ContextSymbolSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_symbols: Vec<ContextSymbolSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub semantic_anchors: Vec<ContextSemanticAnchorSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_files: Vec<ContextRelatedFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_ranges: Vec<ContextRange>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub confidence: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub next_step: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextSymbolSummary {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub line: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextSemanticAnchorSummary {
    pub kind: String,
    pub value: String,
    pub line: u32,
    pub preview: String,
    pub confidence: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextRelatedFile {
    pub path: String,
    pub relationship: String,
    pub reason: String,
    #[serde(default)]
    pub score: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextImpactSummary {
    #[serde(default)]
    pub enabled: bool,
    pub reason: String,
    pub risk: String,
    pub confidence: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_symbols: Vec<ContextImpactTargetSymbol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub impacted_files: Vec<ContextImpactFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub likely_tests: Vec<ContextFileResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommended_next_steps: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextImpactTargetSymbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub path: String,
    pub line: u32,
    #[serde(default)]
    pub incoming_count: usize,
    #[serde(default)]
    pub outgoing_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextImpactFile {
    pub path: String,
    #[serde(default)]
    pub score: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_ranges: Vec<ContextRange>,
}

/// A line range suggestion for reading a file
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct ContextRange {
    pub start_line: u32,
    pub end_line: u32,
}

/// A memory/saved-knowledge item in a context pack response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextMemoryItem {
    pub summary: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub score: u32,
}

/// Error payload inside a context pack response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextPackError {
    pub code: String,
    pub message: String,
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
            images: None,
            context: None,
            mentions: None,
            mode: None,
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
