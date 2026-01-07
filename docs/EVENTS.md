# zblade Internal Event System

This document defines the internal event contract between zblade's Rust backend and React frontend.

## Architecture

**Event Flow Direction:**
- **Backend → Frontend**: Events emitted via Tauri's `emit()`
- **Frontend → Backend**: Commands invoked via Tauri's `invoke()`

Events are one-way notifications. For request/response patterns, use `invoke()` commands instead.

## Event Definitions

---

## Chat & AI Workflow Events

### `chat-update`
**Direction:** Backend → Frontend  
**Payload:** `{ content: string }`  
**Purpose:** Stream AI response text chunks as they arrive from zcoderd

**Example:**
```typescript
listen<ChatUpdatePayload>('chat-update', (event) => {
  console.log('Received chunk:', event.payload.content);
});
```

---

### `chat-done`
**Direction:** Backend → Frontend  
**Payload:** None  
**Purpose:** Signal that AI response streaming is complete

**Example:**
```typescript
listen('chat-done', () => {
  console.log('Chat response complete');
});
```

---

### `chat-error`
**Direction:** Backend → Frontend  
**Payload:** `{ error: string }`  
**Purpose:** Notify frontend of chat/streaming errors

**Example:**
```typescript
listen<ChatErrorPayload>('chat-error', (event) => {
  console.error('Chat error:', event.payload.error);
});
```

---

### `request-confirmation`
**Direction:** Backend → Frontend  
**Payload:** `{ tool_name: string, description: string }`  
**Purpose:** Request user approval before executing a tool

**Example:**
```typescript
listen<RequestConfirmationPayload>('request-confirmation', (event) => {
  const { tool_name, description } = event.payload;
  // Show confirmation modal
});
```

---

### `propose-edit`
**Direction:** Backend → Frontend  
**Payload:** `{ id: string, path: string, old_content: string, new_content: string }`  
**Purpose:** Propose a file edit that requires user review (Accept/Reject)

**Example:**
```typescript
listen<ProposeEditPayload>('propose-edit', (event) => {
  const { id, path, old_content, new_content } = event.payload;
  // Add to pending edits list
});
```

---

### `tool-execution-started`
**Direction:** Backend → Frontend  
**Payload:** `{ tool_name: string, tool_call_id: string }`  
**Purpose:** Notify when a tool begins executing (for loading indicators)

**Example:**
```typescript
listen<ToolExecutionStartedPayload>('tool-execution-started', (event) => {
  console.log(`Tool ${event.payload.tool_name} started`);
  // Show loading indicator
});
```

---

### `tool-execution-completed`
**Direction:** Backend → Frontend  
**Payload:** `{ tool_name: string, tool_call_id: string, success: boolean }`  
**Purpose:** Notify when a tool finishes executing

**Example:**
```typescript
listen<ToolExecutionCompletedPayload>('tool-execution-completed', (event) => {
  const { tool_name, success } = event.payload;
  // Hide loading indicator, show success/error
});
```

---

### `model-changed`
**Direction:** Backend → Frontend  
**Payload:** `{ model_id: string, model_name: string }`  
**Purpose:** Notify when user switches AI model

**Example:**
```typescript
listen<ModelChangedPayload>('model-changed', (event) => {
  console.log(`Switched to model: ${event.payload.model_name}`);
});
```

---

## File Edit Workflow Events

### `edit-applied`
**Direction:** Backend → Frontend  
**Payload:** `{ edit_id: string, file_path: string }`  
**Purpose:** Notify when an edit is successfully applied to disk

**Example:**
```typescript
listen<EditAppliedPayload>('edit-applied', (event) => {
  const { edit_id, file_path } = event.payload;
  // Remove from pending list, refresh file view
  console.log(`Edit ${edit_id} applied to ${file_path}`);
});
```

---

### `edit-rejected`
**Direction:** Backend → Frontend  
**Payload:** `{ edit_id: string, file_path: string }`  
**Purpose:** Notify when user rejects an edit

**Example:**
```typescript
listen<EditRejectedPayload>('edit-rejected', (event) => {
  // Remove from pending list
});
```

---

### `edit-failed`
**Direction:** Backend → Frontend  
**Payload:** `{ edit_id: string, file_path: string, error: string }`  
**Purpose:** Notify when edit application fails

**Example:**
```typescript
listen<EditFailedPayload>('edit-failed', (event) => {
  const { file_path, error } = event.payload;
  // Show error notification
  console.error(`Failed to apply edit to ${file_path}: ${error}`);
});
```

---

### `all-edits-applied`
**Direction:** Backend → Frontend  
**Payload:** `{ count: number, file_paths: string[] }`  
**Purpose:** Notify when Accept All completes successfully

**Example:**
```typescript
listen<AllEditsAppliedPayload>('all-edits-applied', (event) => {
  const { count, file_paths } = event.payload;
  console.log(`Applied ${count} edits to ${file_paths.length} files`);
  // Show success notification, refresh views
});
```

---

## File Operations Events

### `file-opened`
**Direction:** Backend → Frontend  
**Payload:** `{ file_path: string }`  
**Purpose:** Notify when a file is opened in the editor

**Example:**
```typescript
listen<FileOpenedPayload>('file-opened', (event) => {
  console.log(`Opened: ${event.payload.file_path}`);
});
```

---

### `file-closed`
**Direction:** Backend → Frontend  
**Payload:** `{ file_path: string }`  
**Purpose:** Notify when a file tab is closed

**Example:**
```typescript
listen<FileClosedPayload>('file-closed', (event) => {
  // Update tab list
});
```

---

### `file-saved`
**Direction:** Backend → Frontend  
**Payload:** `{ file_path: string }`  
**Purpose:** Notify when a file is saved to disk

**Example:**
```typescript
listen<FileSavedPayload>('file-saved', (event) => {
  // Clear unsaved indicator
  console.log(`Saved: ${event.payload.file_path}`);
});
```

---

### `file-modified`
**Direction:** Backend → Frontend  
**Payload:** `{ file_path: string, has_unsaved_changes: boolean }`  
**Purpose:** Notify when file content changes (for unsaved indicator)

**Example:**
```typescript
listen<FileModifiedPayload>('file-modified', (event) => {
  const { file_path, has_unsaved_changes } = event.payload;
  // Show/hide unsaved indicator dot
});
```

---

### `active-file-changed`
**Direction:** Backend → Frontend  
**Payload:** `{ file_path: string | null, previous_file_path: string | null }`  
**Purpose:** Notify when user switches between tabs

**Example:**
```typescript
listen<ActiveFileChangedPayload>('active-file-changed', (event) => {
  const { file_path, previous_file_path } = event.payload;
  // Update active tab highlight
});
```

---

## Workspace Events

### `workspace-changed`
**Direction:** Backend → Frontend  
**Payload:** `{ workspace_path: string }`  
**Purpose:** Notify when workspace folder changes

**Example:**
```typescript
listen<WorkspaceChangedPayload>('workspace-changed', (event) => {
  console.log(`Workspace changed to: ${event.payload.workspace_path}`);
  // Refresh file explorer, clear state
});
```

---

### `project-files-changed`
**Direction:** Backend → Frontend  
**Payload:** `{ added: string[], removed: string[], modified: string[] }`  
**Purpose:** Notify when files are added/deleted/modified in workspace

**Example:**
```typescript
listen<ProjectFilesChangedPayload>('project-files-changed', (event) => {
  const { added, removed, modified } = event.payload;
  // Refresh file explorer tree
});
```

---

## Connection & Status Events

### `connection-status`
**Direction:** Backend → Frontend  
**Payload:** `{ status: 'connected' | 'disconnected' | 'reconnecting' | 'error', message?: string }`  
**Purpose:** Notify of zcoderd connection state changes

**Example:**
```typescript
listen<ConnectionStatusPayload>('connection-status', (event) => {
  const { status, message } = event.payload;
  if (status === 'disconnected') {
    // Show reconnecting indicator
  } else if (status === 'connected') {
    // Hide indicator, resume operations
  }
});
```

---

### `backend-error`
**Direction:** Backend → Frontend  
**Payload:** `{ error: string, context?: string }`  
**Purpose:** General backend error notification

**Example:**
```typescript
listen<BackendErrorPayload>('backend-error', (event) => {
  const { error, context } = event.payload;
  // Show error toast/notification
});
```

---

## Document Events

### `open-ephemeral-document`
**Direction:** Backend → Frontend  
**Payload:** `{ id: string, title: string, content: string }`  
**Purpose:** Open a temporary document tab (e.g., research results)

**Example:**
```typescript
listen<OpenEphemeralDocumentPayload>('open-ephemeral-document', (event) => {
  const { id, title, content } = event.payload;
  // Open new tab with content
});
```

---

## Type Safety

### Rust (Backend)
```rust
use crate::events::{event_names, ChatUpdatePayload};

// Emit event
app.emit(event_names::CHAT_UPDATE, ChatUpdatePayload {
    content: "Hello".to_string(),
})?;
```

### TypeScript (Frontend)
```typescript
import { EventNames, ChatUpdatePayload } from '@/types/events';
import { listen } from '@tauri-apps/api/event';

// Listen to event
const unlisten = await listen<ChatUpdatePayload>(
  EventNames.CHAT_UPDATE,
  (event) => {
    console.log(event.payload.content);
  }
);
```

## Adding New Events

1. **Define in Rust** (`src-tauri/src/events.rs`):
   - Add constant to `event_names` module
   - Create payload struct with `#[derive(Serialize, Deserialize)]`

2. **Define in TypeScript** (`src/types/events.ts`):
   - Add to `EventNames` constant
   - Create matching interface
   - Add to `EventMap` type

3. **Document** (this file):
   - Add event description with examples

4. **Emit from Backend**:
   ```rust
   app.emit(event_names::YOUR_EVENT, YourPayload { ... })?;
   ```

5. **Listen in Frontend**:
   ```typescript
   listen<YourPayload>(EventNames.YOUR_EVENT, handler);
   ```

## Best Practices

- ✅ Use typed event names from constants (prevents typos)
- ✅ Use typed payloads (compile-time safety)
- ✅ Document new events in this file
- ✅ Keep payloads simple and serializable
- ❌ Don't use events for request/response (use `invoke()` instead)
- ❌ Don't emit events from frontend to backend
- ❌ Don't forget to unlisten when component unmounts
