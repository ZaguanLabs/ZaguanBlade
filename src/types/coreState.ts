/**
 * Core State Types
 *
 * TypeScript types matching the Rust CoreStateSnapshot for headless core support.
 * Used for UI initialization, reload recovery, and debugging.
 */

export interface CoreStateSnapshot {
    protocol: ProtocolInfo;
    workspace: WorkspaceStateSnapshot;
    editor: EditorStateSnapshot;
    chat: ChatStateSnapshot;
    terminals: TerminalStateSnapshot[];
    pending_approvals: PendingApproval[];
}

export interface ProtocolInfo {
    version: {
        major: number;
        minor: number;
        patch: number;
    };
    capabilities: string[];
}

export interface WorkspaceStateSnapshot {
    path: string | null;
    project_id: string | null;
}

export interface EditorStateSnapshot {
    active_file: string | null;
    open_files: string[];
    cursor_line: number | null;
    cursor_column: number | null;
    selection_start: number | null;
    selection_end: number | null;
}

export interface ChatStateSnapshot {
    session_id: string | null;
    message_count: number;
    is_generating: boolean;
    selected_model: string | null;
}

export interface TerminalStateSnapshot {
    id: string;
    is_active: boolean;
    has_running_process: boolean;
}

export interface PendingApproval {
    id: string;
    action_type: string;
    description: string;
}

export interface FeatureFlagsSnapshot {
    editor_backend_authority: boolean;
    tabs_backend_authority: boolean;
}
