// v1.1: Semantic versioning
export type Version = {
    major: number;
    minor: number;
    patch: number;
};

export type BladeEnvelope<T> = {
    protocol: "BCP";
    version: Version;
    domain: string;
    message: T;
};

export type BladeIntentEnvelope = {
    id: string; // UUID
    timestamp: number;
    idempotency_key?: string; // v1.1: Optional idempotency key
    intent: BladeIntent;
};

export type BladeEventEnvelope = {
    id: string; // UUID
    timestamp: number;
    causality_id: string | null;
    event: BladeEvent;
};

// ===================================
// Intents (Writes)
// ===================================

export type BladeIntent =
    | { type: "Chat"; payload: ChatIntent }
    | { type: "Editor"; payload: EditorIntent }
    | { type: "File"; payload: FileIntent }
    | { type: "Workflow"; payload: WorkflowIntent }
    | { type: "Terminal"; payload: TerminalIntent }
    | { type: "History"; payload: HistoryIntent }
    | { type: "System"; payload: SystemIntent }
    | { type: "Language"; payload: LanguageIntent };

export type ChatIntent =
    | { type: "SendMessage"; payload: { content: string; model: string; context?: EditorContext } }
    | { type: "StopGeneration"; payload?: Record<string, never> }
    | { type: "ClearHistory"; payload?: Record<string, never> };

export type EditorContext = {
    active_file: string | null;
    open_files: string[];
    cursor_line: number | null;
    cursor_column: number | null;
    selection_start: number | null;
    selection_end: number | null;
};

export type EditorIntent =
    | { type: "OpenFile"; payload: { path: string } }
    | { type: "CloseFile"; payload: { path: string } }
    | { type: "SetActiveFile"; payload: { path: string | null } }
    | { type: "UpdateCursor"; payload: { line: number; column: number } }
    | { type: "UpdateSelection"; payload: { start: number; end: number } }
    | { type: "GetState"; payload?: Record<string, never> }
    // Legacy
    | { type: "SaveFile"; payload: { path: string } }
    | { type: "BufferUpdate"; payload: { path: string; content: string } };

export type FileIntent =
    | { type: "Read"; payload: { path: string } }
    | { type: "Write"; payload: { path: string; content: string } }
    | { type: "List"; payload: { path: string | null } }
    | { type: "Create"; payload: { path: string; is_dir: boolean } }
    | { type: "Delete"; payload: { path: string } }
    | { type: "Rename"; payload: { old_path: string; new_path: string } };

export type FileEntry = {
    name: string;
    path: string;
    is_dir: boolean;
    children: FileEntry[] | null;
};

export type WorkflowIntent =
    // v1.1 variants
    | { type: "ApproveAction"; payload: { action_id: string } }
    | { type: "ApproveAll"; payload: { batch_id: string } }
    | { type: "RejectAction"; payload: { action_id: string } }
    | { type: "RejectAll"; payload: { batch_id: string } }
    // Legacy v1.0 variants (for backward compatibility)
    | { type: "ApproveChange"; payload: { change_id: string } }
    | { type: "RejectChange"; payload: { change_id: string } }
    | { type: "ApproveAllChanges"; payload: Record<string, never> }
    | { type: "ApproveTool"; payload: { approved: boolean } }
    | { type: "ApproveToolDecision"; payload: { decision: string } };

export type TerminalOwner =
    | { type: "User" }
    | { type: "Agent"; task_id: string };

export type TerminalIntent =
    | { type: "Spawn"; payload: { id: string; command?: string; cwd?: string; owner?: TerminalOwner; interactive?: boolean } }
    | { type: "Input"; payload: { id: string; data: string } }
    | { type: "Resize"; payload: { id: string; rows: number; cols: number } }
    | { type: "Kill"; payload: { id: string } };

export type HistoryIntent =
    | { type: "ListConversations"; payload: { project_id: string } }
    | { type: "LoadConversation"; payload: { session_id: string } };

export type SystemIntent =
    | { type: "SetLogLevel"; payload: { level: string } };

// ===================================
// Events (Reads)
// ===================================

export type BladeEvent =
    | { type: "Chat"; payload: ChatEvent }
    | { type: "Editor"; payload: EditorEvent }
    | { type: "File"; payload: FileEvent }
    | { type: "Workflow"; payload: WorkflowEvent }
    | { type: "Terminal"; payload: TerminalEvent }
    | { type: "History"; payload: HistoryEvent }
    | { type: "System"; payload: SystemEvent }
    | { type: "Language"; payload: LanguageEvent };

export type ChatEvent =
    | { type: "ChatState"; payload: { messages: ChatMessage[] } }
    | { type: "MessageDelta"; payload: { id: string; seq: number; chunk: string; is_final: boolean } } // v1.1: added seq and is_final
    | { type: "ReasoningDelta"; payload: { id: string; seq: number; chunk: string; is_final: boolean } }
    | { type: "MessageCompleted"; payload: { id: string } } // v1.1: explicit end-of-stream
    | { type: "ToolUpdate"; payload: { message_id: string; tool_call_id: string; status: string; result: string | null; tool_call?: any } }
    | { type: "GenerationSignal"; payload: { is_generating: boolean } };

export type EditorEvent =
    | { type: "StateSnapshot"; payload: { active_file: string | null; open_files: string[]; cursor_line: number | null; cursor_column: number | null; selection_start: number | null; selection_end: number | null } }
    | { type: "FileOpened"; payload: { path: string } }
    | { type: "FileClosed"; payload: { path: string } }
    | { type: "ActiveFileChanged"; payload: { path: string | null } }
    | { type: "CursorMoved"; payload: { line: number; column: number } }
    | { type: "SelectionChanged"; payload: { start: number; end: number } }
    // Legacy
    | { type: "EditorState"; payload: { active_file: string | null } }
    | { type: "ContentDelta"; payload: { file: string; patch: string } };

export type FileEvent =
    | { type: "Content"; payload: { path: string; data: string } }
    | { type: "Written"; payload: { path: string } }
    | { type: "Listing"; payload: { path: string | null; entries: FileEntry[] } }
    | { type: "Created"; payload: { path: string; is_dir: boolean } }
    | { type: "Deleted"; payload: { path: string } }
    | { type: "Renamed"; payload: { old_path: string; new_path: string } };

export type WorkflowEvent =
    | { type: "ApprovalRequested"; payload: { batch_id: string; items: string[] } }
    // v1.1 variants
    | { type: "ActionCompleted"; payload: { action_id: string; success: boolean } }
    | { type: "BatchCompleted"; payload: { batch_id: string; succeeded: number; failed: number } }
    // Legacy v1.0 variant
    | { type: "TaskCompleted"; payload: { task_id: string; success: boolean } };

export type TerminalEvent =
    | { type: "Spawned"; payload: { id: string; owner: TerminalOwner } } // v1.1: terminal creation event
    | { type: "Output"; payload: { id: string; seq: number; data: string } } // v1.1: added seq
    | { type: "Exit"; payload: { id: string; code: number } };

export type ConversationSummary = {
    id: string;
    project_id: string;
    title: string;
    created_at: string;
    last_active_at: string;
    message_count: number;
    preview: string;
};

export type HistoryMessage = {
    role: "user" | "assistant" | "tool" | "system";
    content: string;
    tool_calls?: Array<{
        id: string;
        type: string;
        function: {
            name: string;
            arguments: string;
        };
    }>;
    tool_call_id?: string;
    created_at: string;
};

export type HistoryEvent =
    | { type: "ConversationList"; payload: { conversations: ConversationSummary[] } }
    | { type: "ConversationLoaded"; payload: { session_id: string; project_id: string; title: string; created_at: string; last_active_at: string; message_count: number; messages: HistoryMessage[] } };

export type SystemEvent =
    | { type: "IntentFailed"; payload: { intent_id: string; error: BladeError } }
    | { type: "ProcessStarted"; payload: { intent_id: string } }
    | { type: "ProcessCompleted"; payload: { intent_id: string } }
    | { type: "ProtocolVersion"; payload: { supported: Version[]; current: Version } } // v1.1: version negotiation
    | { type: "ProcessProgress"; payload: { intent_id: string; progress: number; message: string } }; // v1.1: progress updates

// ===================================
// Models
// ===================================

export type BladeError =
    | { code: "ValidationError"; details: { field: string; message: string } }
    | { code: "PermissionDenied"; details?: Record<string, never> }
    | { code: "ResourceNotFound"; details: { id: string } }
    | { code: "Conflict"; details: { reason: string } }
    | { code: "Internal"; details: { trace_id: string; message: string } }
    | { code: "VersionMismatch"; details: { expected: Version; received: Version } } // v1.1: detailed version info
    | { code: "Timeout"; details: { timeout_ms: number } }
    | { code: "RateLimited"; details: { retry_after_ms: number } };

export interface ChatMessage {
    role: "User" | "Assistant" | "System" | "Tool";
    content: string;
    reasoning?: string;
    tool_call_id?: string;
    // ... complete as needed based on Rust struct
}

// ===================================
// Language Domain (v1.3)
// ===================================

export type LanguageIntent =
    | { type: "IndexFile"; payload: { file_path: string } }
    | { type: "IndexWorkspace"; payload?: Record<string, never> }
    | { type: "SearchSymbols"; payload: { query: string; file_path?: string | null; symbol_types?: string[] | null } }
    | { type: "GetSymbolAt"; payload: { file_path: string; line: number; character: number } }
    | { type: "DidOpen"; payload: { file_path: string; content: string; language_id: string } }
    | { type: "DidChange"; payload: { file_path: string; content: string; version: number } }
    | { type: "DidClose"; payload: { file_path: string } }
    | { type: "ZlpMessage"; payload: any };

export type LanguageEvent =
    | { type: "FileIndexed"; payload: { file_path: string; symbol_count: number } }
    | { type: "WorkspaceIndexed"; payload: { file_count: number; symbol_count: number; duration_ms: number } }
    | { type: "SymbolsFound"; payload: { intent_id: string; symbols: LanguageSymbol[] } }
    | { type: "SymbolAt"; payload: { intent_id: string; symbol: LanguageSymbol | null } }
    | { type: "ZlpResponse"; payload: { original_request_id: string; payload: any } };

export type LanguagePosition = {
    line: number;
    character: number;
}

export type LanguageRange = {
    start: LanguagePosition;
    end: LanguagePosition;
}

export type LanguageLocation = {
    file_path: string;
    range: LanguageRange;
}

export type LanguageSymbol = {
    id: string;
    name: string;
    symbol_type: string;
    file_path: string;
    range: LanguageRange;
    parent_id: string | null;
    docstring: string | null;
    signature: string | null;
}



