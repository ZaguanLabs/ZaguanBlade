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

/**
 * Payload for chat-update event
 */
export interface ChatUpdatePayload {
  content: string;
}

/**
 * Payload for command-executed event
 */
export interface CommandExecutedPayload {
  command: string;
  cwd?: string;
  output: string;
  exitCode: number;
  duration?: number;
}

/**
 * Payload for terminal-cwd-changed event
 */
export interface TerminalCwdChangedPayload {
  id: string;
  cwd: string;
}

/**
 * Payload for chat-error event
 */
export interface ChatErrorPayload {
  error: string;
}

/**
 * Payload for request-confirmation event
 */
export interface RequestConfirmationPayload {
  actions: StructuredAction[];
}

export interface StructuredAction {
  id: string;
  command: string;
  description: string;
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
 * Payload for open-ephemeral-document event
 */
export interface OpenEphemeralDocumentPayload {
  id: string;
  title: string;
  content: string;
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
 * Payload for todo_updated event
 */
export interface TodoUpdatedPayload {
  todos: TodoItem[];
}

/**
 * Payload for tool-execution-started event
 */
export interface ToolExecutionStartedPayload {
  tool_name: string;
  tool_call_id: string;
}

/**
 * Payload for tool-execution-completed event
 */
export interface ToolExecutionCompletedPayload {
  tool_name: string;
  tool_call_id: string;
  success: boolean;
}

/**
 * Payload for model-changed event
 */
export interface ModelChangedPayload {
  model_id: string;
  model_name: string;
}

/**
 * Payload for change-applied event
 */
export interface ChangeAppliedPayload {
  change_id: string;
  file_path: string;
}

/**
 * Payload for change-rejected event
 */
export interface ChangeRejectedPayload {
  change_id: string;
  file_path: string;
}

/**
 * Payload for edit-failed event
 */
export interface EditFailedPayload {
  edit_id: string;
  file_path: string;
  error: string;
}

/**
 * Payload for all-edits-applied event
 */
export interface AllEditsAppliedPayload {
  count: number;
  file_paths: string[];
}

/**
 * Payload for file-opened event
 */
export interface FileOpenedPayload {
  file_path: string;
}

/**
 * Payload for file-closed event
 */
export interface FileClosedPayload {
  file_path: string;
}

/**
 * Payload for file-saved event
 */
export interface FileSavedPayload {
  file_path: string;
}

/**
 * Payload for file-modified event
 */
export interface FileModifiedPayload {
  file_path: string;
  has_unsaved_changes: boolean;
}

/**
 * Payload for active-file-changed event
 */
export interface ActiveFileChangedPayload {
  file_path: string | null;
  previous_file_path: string | null;
}

/**
 * Payload for workspace-changed event
 */
export interface WorkspaceChangedPayload {
  workspace_path: string;
}

/**
 * Payload for project-files-changed event
 */
export interface ProjectFilesChangedPayload {
  added: string[];
  removed: string[];
  modified: string[];
}

/**
 * Connection status enum
 */
export type ConnectionStatus = 'connected' | 'disconnected' | 'reconnecting' | 'error';

/**
 * Payload for connection-status event
 */
export interface ConnectionStatusPayload {
  status: ConnectionStatus;
  message?: string;
}

/**
 * Payload for backend-error event
 */
export interface BackendErrorPayload {
  error: string;
  context?: string;
}

/**
 * Type-safe event name to payload mapping
 */
export interface EventMap {
  // Chat & AI Workflow
  [EventNames.CHAT_UPDATE]: ChatUpdatePayload;
  [EventNames.CHAT_DONE]: void;
  [EventNames.CHAT_ERROR]: ChatErrorPayload;
  [EventNames.REQUEST_CONFIRMATION]: RequestConfirmationPayload;
  [EventNames.TOOL_EXECUTION_STARTED]: ToolExecutionStartedPayload;
  [EventNames.TOOL_EXECUTION_COMPLETED]: ToolExecutionCompletedPayload;
  [EventNames.MODEL_CHANGED]: ModelChangedPayload;
  
  // File Edit Workflow
  [EventNames.PROPOSE_EDIT]: ProposeEditPayload;
  [EventNames.CHANGE_APPLIED]: ChangeAppliedPayload;
  [EventNames.CHANGE_REJECTED]: ChangeRejectedPayload;
  [EventNames.EDIT_FAILED]: EditFailedPayload;
  [EventNames.ALL_EDITS_APPLIED]: AllEditsAppliedPayload;
  
  // File Operations
  [EventNames.FILE_OPENED]: FileOpenedPayload;
  [EventNames.FILE_CLOSED]: FileClosedPayload;
  [EventNames.FILE_SAVED]: FileSavedPayload;
  [EventNames.FILE_MODIFIED]: FileModifiedPayload;
  [EventNames.ACTIVE_FILE_CHANGED]: ActiveFileChangedPayload;
  
  // Workspace
  [EventNames.WORKSPACE_CHANGED]: WorkspaceChangedPayload;
  [EventNames.PROJECT_FILES_CHANGED]: ProjectFilesChangedPayload;
  
  // Connection & Status
  [EventNames.CONNECTION_STATUS]: ConnectionStatusPayload;
  [EventNames.BACKEND_ERROR]: BackendErrorPayload;
  
  // Documents
  [EventNames.OPEN_EPHEMERAL_DOCUMENT]: OpenEphemeralDocumentPayload;

  /** Request explorer refresh */
  [EventNames.REFRESH_EXPLORER]: void;
  
  // Task Management
  [EventNames.TODO_UPDATED]: TodoUpdatedPayload;
}
