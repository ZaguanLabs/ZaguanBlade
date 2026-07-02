/**
 * Internal Tauri Events for zblade
 * 
 * This module defines the event contract between zblade's Rust backend and React frontend.
 * Events flow in one direction: Backend → Frontend (via Tauri's emit)
 * Frontend → Backend communication uses invoke() commands instead.
 * 
 * These types must match the Rust definitions in src-tauri/src/events.rs
 */

/**
 * Event names as constants to prevent typos
 */
export const EventNames = {
  // === Chat & AI Workflow ===
  
  /** AI chat message chunk received from zcoderd */
  CHAT_UPDATE: 'chat-update',
  
  /** AI chat response completed */
  CHAT_DONE: 'chat-done',

  /** Hook-driven tool approval request */
  APPROVAL_REQUEST: 'approval-request',
  
  /** Error occurred during chat */
  CHAT_ERROR: 'chat-error',
  
  /** Tool execution requires user confirmation */
  REQUEST_CONFIRMATION: 'request-confirmation',
  
  /** Tool execution started */
  TOOL_EXECUTION_STARTED: 'tool-execution-started',
  
  /** Tool execution completed successfully */
  TOOL_EXECUTION_COMPLETED: 'tool-execution-completed',
  
  /** AI model changed */
  MODEL_CHANGED: 'model-changed',
  
  /** Command execution completed */
  COMMAND_EXECUTED: 'command-executed',
  
  // === File Edit Workflow ===
  
  /** File edit proposed by AI, needs user review */
  PROPOSE_EDIT: 'propose-edit',
  
  /** Change successfully applied to disk */
  CHANGE_APPLIED: 'change-applied',
  
  /** Change rejected by user */
  CHANGE_REJECTED: 'change-rejected',
  
  /** Edit application failed */
  EDIT_FAILED: 'edit-failed',
  
  /** All edits applied successfully (Accept All completed) */
  ALL_EDITS_APPLIED: 'all-edits-applied',
  
  // === File Operations ===
  
  /** File opened in editor */
  FILE_OPENED: 'file-opened',
  
  /** File closed in editor */
  FILE_CLOSED: 'file-closed',
  
  /** File saved to disk */
  FILE_SAVED: 'file-saved',
  
  /** File content modified (unsaved) */
  FILE_MODIFIED: 'file-modified',
  
  /** Active file/tab changed */
  ACTIVE_FILE_CHANGED: 'active-file-changed',
  
  // === Workspace ===
  
  /** Workspace folder changed */
  WORKSPACE_CHANGED: 'workspace-changed',
  
  /** Project files changed (added/deleted) */
  PROJECT_FILES_CHANGED: 'project-files-changed',

  /** Request explorer refresh */
  REFRESH_EXPLORER: 'refresh-explorer',

  // === Terminal ===

  /** Terminal reported a cwd change */
  TERMINAL_CWD_CHANGED: 'terminal-cwd-changed',

  // === Connection & Status ===
  
  /** Connection status to zcoderd changed */
  CONNECTION_STATUS: 'connection-status',
  
  /** General backend error */
  BACKEND_ERROR: 'backend-error',
  
  // === Documents ===
  
  /** Open ephemeral document (research results, etc) */
  OPEN_EPHEMERAL_DOCUMENT: 'open-ephemeral-document',
  
  /** Todo list updated by AI for task progress tracking */
  TODO_UPDATED: 'todo_updated',
} as const;

export interface ChatDonePayload {
  finish_reason: string;
}

/**
 * Payload for chat-error event
 */
export interface ChatErrorPayload {
  code: string;
  error?: string | null;
  message?: string | null;
  detail?: string | null;
  i18nKey?: string | null;
  i18nParams?: Record<string, string> | null;
}

export interface ContextLengthExceededPayload {
  message: string;
  token_count: number | null;
  max_tokens: number | null;
  excess: number | null;
  recoverable: boolean;
  recovery_hint: string | null;
  titleKey?: string | null;
  recoverableHintKey?: string | null;
  nonRecoverableHintKey?: string | null;
}

export interface MessageTooLargePayload {
  message: string;
  recovery_hint: string;
  titleKey?: string | null;
  recoveryHintLabelKey?: string | null;
}

/**
 * Payload for request-confirmation event
 */
export interface RequestConfirmationPayload {
  actions: StructuredAction[];
}

export interface ApprovalRequestPayload {
  session_id: string;
  approval_id: string;
  tool_call_id: string;
  tool_name: string;
  arguments: unknown;
  source?: string | null;
  rule_name?: string | null;
  message?: string | null;
  decision?: string | null;
}

export interface StructuredAction {
  id: string;
  command: string;
  description: string;
  actionKind?: 'command' | 'generic_tool';
  descriptionKey?: string | null;
  descriptionParams?: Record<string, string> | null;
  cwd?: string;
  root_command?: string;
  cwd_outside_workspace?: boolean;
  is_generic_tool: boolean;
}

/**
 * Payload for propose-edit event
 */
export interface ProposeEditPayload {
  id: string;
  path: string;
  old_content: string;
  new_content: string;
}

/**
 * Todo item for task progress tracking
 */
export interface TodoItem {
  content: string;      // Imperative form: "Fix authentication bug"
  activeForm: string;   // Present continuous: "Fixing authentication bug"
  status: 'pending' | 'in_progress' | 'completed';
}

/**
 * Payload for tool-execution-completed event
 */
export interface ToolExecutionCompletedPayload {
  tool_name: string;
  tool_call_id: string;
  success: boolean;
  skipped?: boolean;
}

