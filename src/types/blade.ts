export type BladeEnvelope<T> = {
    protocol: "BCP";
    version: number;
    domain: string;
    message: T;
};

export type BladeIntentEnvelope = {
    id: string; // UUID
    timestamp: number;
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
    | { type: "System"; payload: SystemIntent };

export type ChatIntent =
    | { type: "SendMessage"; payload: { content: string; model: string; context?: EditorContext } }
    | { type: "StopGeneration"; payload?: {} }
    | { type: "ClearHistory"; payload?: {} };

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
    | { type: "SaveFile"; payload: { path: string } }
    | { type: "BufferUpdate"; payload: { path: string; content: string } };

export type FileIntent =
    | { type: "Read"; payload: { path: string } }
    | { type: "Write"; payload: { path: string; content: string } }
    | { type: "List"; payload: { path: string | null } };

export type FileEntry = {
    name: string;
    path: string;
    is_dir: boolean;
    children: FileEntry[] | null;
};

export type WorkflowIntent =
    | { type: "ApproveChange"; payload: { change_id: string } }
    | { type: "RejectChange"; payload: { change_id: string } }
    | { type: "ApproveAllChanges"; payload: {} }
    | { type: "ApproveTool"; payload: { approved: boolean } }
    | { type: "ApproveToolDecision"; payload: { decision: string } };

export type TerminalIntent =
    | { type: "Spawn"; payload: { id: string; command?: string; cwd?: string; interactive?: boolean } }
    | { type: "Input"; payload: { id: string; data: string } }
    | { type: "Resize"; payload: { id: string; rows: number; cols: number } }
    | { type: "Kill"; payload: { id: string } };

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
    | { type: "System"; payload: SystemEvent };

export type ChatEvent =
    | { type: "ChatState"; payload: { messages: ChatMessage[] } }
    | { type: "MessageDelta"; payload: { id: string; chunk: string } }
    | { type: "GenerationSignal"; payload: { is_generating: boolean } };

export type EditorEvent =
    | { type: "EditorState"; payload: { active_file: string | null } }
    | { type: "ContentDelta"; payload: { file: string; patch: string } };

export type FileEvent =
    | { type: "Content"; payload: { path: string; data: string } }
    | { type: "Written"; payload: { path: string } }
    | { type: "Listing"; payload: { path: string | null; entries: FileEntry[] } };

export type WorkflowEvent =
    | { type: "ApprovalRequested"; payload: { batch_id: string; items: string[] } }
    | { type: "TaskCompleted"; payload: { task_id: string; success: boolean } };

export type TerminalEvent =
    | { type: "Output"; payload: { id: string; data: string } }
    | { type: "Exit"; payload: { id: string; code: number } };

export type SystemEvent =
    | { type: "IntentFailed"; payload: { intent_id: string; error: BladeError } }
    | { type: "ProcessStarted"; payload: { intent_id: string } }
    | { type: "ProcessCompleted"; payload: { intent_id: string } };

// ===================================
// Models
// ===================================

export type BladeError =
    | { code: "ValidationError"; details: { field: string; message: string } }
    | { code: "PermissionDenied"; details?: {} }
    | { code: "ResourceNotFound"; details: { id: string } }
    | { code: "Conflict"; details: { reason: string } }
    | { code: "Internal"; details: { trace_id: string; message: string } }
    | { code: "VersionMismatch"; details: { version: number } }
    | { code: "Timeout"; details: { timeout_ms: number } }
    | { code: "RateLimited"; details: { retry_after_ms: number } };

export interface ChatMessage {
    role: "User" | "Assistant" | "System" | "Tool";
    content: string;
    reasoning?: string;
    tool_call_id?: string;
    // ... complete as needed based on Rust struct
}
