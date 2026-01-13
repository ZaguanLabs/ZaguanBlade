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
    | { type: "System"; payload: SystemIntent };

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
    | { type: "MessageDelta"; payload: { id: string; seq: number; chunk: string; is_final: boolean } } // v1.1: added seq and is_final
    | { type: "ReasoningDelta"; payload: { id: string; seq: number; chunk: string; is_final: boolean } }
    | { type: "MessageCompleted"; payload: { id: string } } // v1.1: explicit end-of-stream
    | { type: "ToolUpdate"; payload: { message_id: string; tool_call_id: string; status: string; result: string | null; tool_call?: any } }
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
    // v1.1 variants
    | { type: "ActionCompleted"; payload: { action_id: string; success: boolean } }
    | { type: "BatchCompleted"; payload: { batch_id: string; succeeded: number; failed: number } }
    // Legacy v1.0 variant
    | { type: "TaskCompleted"; payload: { task_id: string; success: boolean } };

export type TerminalEvent =
    | { type: "Spawned"; payload: { id: string; owner: TerminalOwner } } // v1.1: terminal creation event
    | { type: "Output"; payload: { id: string; seq: number; data: string } } // v1.1: added seq
    | { type: "Exit"; payload: { id: string; code: number } };

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
