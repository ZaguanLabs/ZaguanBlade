# Audit of Current IPC (pre-Blade Protocol)

This document lists the existing IPC mechanisms to serves as the checklist for migration to the new `BladeIntent` / `BladeEvent` system.

## 1. Registered Tauri Commands (Backend)

Found 36 commands registered in `lib.rs`:

| Command Name | Module/Function | Purpose |
| :--- | :--- | :--- |
| `greet` | `greet` | Simple test command |
| `log_frontend` | `log_frontend` | Forwards logs from frontend to terminal |
| `send_message` | `send_message` | Sends user message to AI |
| `get_conversation` | `get_conversation` | Returns chat history |
| `list_models` | `list_models` | Models from zcoderd |
| `approve_tool` | `approve_tool` | Approves pending tool execution batch |
| `approve_tool_decision` | `approve_tool_decision` | Sets approval policy ("always", "once") |
| `open_folder` | `open_folder` | Sets workspace root |
| `list_files` | `list_files` | FS listing |
| `read_file_content` | `read_file_content` | FS read |
| `write_file_content` | `write_file_content` | FS write |
| `set_virtual_buffer` | `set_virtual_buffer` | Optimistic update helper |
| `clear_virtual_buffer` | `clear_virtual_buffer` | Optimistic update helper |
| `has_virtual_buffer` | `has_virtual_buffer` | Optimistic update helper |
| `get_virtual_files` | `get_virtual_files` | Optimistic update helper |
| `stop_generation` | `stop_generation` | Aborts AI stream |
| `approve_change` | `approve_change` | Applies a specific file edit |
| `approve_changes_for_file` | `approve_changes_for_file` | Applies all edits for a file |
| `reject_change` | `reject_change` | Discards a pending edit |
| `list_conversations` | `list_conversations` | Conversation management |
| `load_conversation` | `load_conversation` | Conversation management |
| `new_conversation` | `new_conversation` | Conversation management |
| `delete_conversation` | `delete_conversation` | Conversation management |
| `save_conversation` | `save_conversation` | Conversation management |
| `get_recent_workspaces` | `get_recent_workspaces` | Workspace management |
| `get_current_workspace` | `get_current_workspace` | Workspace management |
| `approve_all_changes` | `approve_all_changes` | Bulk approval |
| `set_selected_model` | `set_selected_model` | Config update |
| `get_selected_model` | `get_selected_model` | Config read |
| `submit_command_result` | `submit_command_result` | Returns terminal output to AI |
| `terminal::create_terminal` | `terminal.rs` | Spawns PTY |
| `terminal::write_to_terminal` | `terminal.rs` | Writes to PTY |
| `terminal::resize_terminal` | `terminal.rs` | Resizes PTY |
| `terminal::execute_command...` | `terminal.rs` | Runs shell cmd |
| `ephemeral_commands::*` | `ephemeral_commands.rs` | (6 commands) Manage temp docs |

## 2. Emitted Events (Backend -> Frontend)

| Event Name | Source | Purpose |
| :--- | :--- | :--- |
| `chat-update` | `lib.rs` | Streaming chat chunks/messages |
| `chat-done` | `lib.rs` | Stream completion signal |
| `chat-error` | `lib.rs` | Chat failure |
| `propose-changes` | `lib.rs` | Pending edit list for UI |
| `propose-edit` | `events.rs` | (Defined but likely unused?) |
| `tool-execution-completed` | `lib.rs`, `tools.rs` | Status update for tool calls |
| `tool-execution-started` | `lib.rs` | Status update |
| `command-execution-started` | `lib.rs` | Shell command status |
| `request-confirmation` | `lib.rs` | Tool approval modal trigger |
| `change-applied` | `lib.rs` | Edit success |
| `all-edits-applied` | `lib.rs` | Bulk edit success |
| `open-file` | `lib.rs` | Auto-open file in editor |
| `open-file-with-highlight` | `lib.rs` | Auto-open with range |
| `open-ephemeral-document` | `lib.rs` | Research results display |
| `refresh-explorer` | `lib.rs`, `tools.rs` | FS change signal |
| `terminal-output` | `terminal.rs` | PTY stdout/stderr |
| `terminal-exit` | `terminal.rs` | PTY close |
| `todo-updated` | `tools.rs` | **HIDDEN:** Emitted from `todo_write` tool |

## 3. Hidden Interception & Logic (The "Junk" to Formalize)

### A. The "Interception Layer" (`ai_workflow.rs`)
The function `handle_tool_calls` contains hardcoded policy logic:
*   **Interception**: Decides which tools need user approval (`edit_file`, `run_command`, etc.).
*   **Implicit Batching**: Groups parallel tool calls into a `PendingToolBatch`.
*   **Loop Detection**: Counts tool signatures and returns "SYSTEM WARNING" strings.
*   **Spam Detection**: Monitors assistant content repetition.

**Recommendation**: This needs to move from an ad-hoc struct (`AiWorkflow`) to the `BladeIntent` handler. E.g., `BladeIntent::AssistantRequest(ToolCall)` -> `Mutation` -> `BladeEvent::RequestApproval`.

### B. Virtual Buffers (`lib.rs`)
A mechanism (`virtual_buffers` HashMap in `AppState`) allows the frontend to override disk content.
*   Used by `read_file` to serve unsaved changes.
*   Managed via 4 ad-hoc commands (`set_`, `clear_`, `has_`, `get_`).

**Recommendation**: Formalize as `BladeIntent::BufferUpdate` and `BladeState::Buffers`.

### C. Tool Side Effects (`tools.rs`)
*   `todo_write` tool manually emits `todo-updated`. This bypasses the standard event stream.
*   **Fix**: Returns a standardized result that the *caller* uses to emit a generic `StateUpdated` event.

### D. Terminal PTY
*   Heavy direct `emit` usage (`terminal-output`).
*   **Recommendation**: Keep as a specialized high-frequency stream for now, but wrap the *creation* and *resizing* in Intents (`BladeIntent::SpawnTerminal`).

## 4. Frontend Logic & Hiding (`useChat.ts` & others)

The frontend is not just a view; it holds state and logic that duplicates or hides backend intent.

### A. Complex Message Merging (`useChat.ts`)
*   **Logic**: The `chat-update` listener manually stitches streaming chunks, retroactively finds tool calls in previous messages, and updates their status.
*   **Problem**: This is fragile state management. The backend should emit a *canonical* Message list or a precise "Patch Operation" rather than forcing the client to dedup/search/stitch.

### B. Implicit Side Effects
*   **Auto-Opening Docs**: `useChat` listens to `propose-changes` and *immediately* triggers `open-ephemeral-document` if the change is a new file. This is hidden UI logic. The backend should explicitly include an "Open Tab" instruction in the event if that is the intent.

### C. Todo Injection
*   **Logic**: `todo-updated` event causes the frontend to find the *last assistant message* in its state and attach the todos to it.
*   **Problem**: This modifies history in-place on the client based on a separate event channel.

### D. Client-Side Tool Execution (`useCommandExecution.ts`)
*   The backend emits `command-execution-started`, the Frontend (Terminal UI) runs it, then calls `submit_command_result`.
*   **Recommendation**: This is a valid "Client Capability" pattern but should be formalized. `BladeIntent::RequestClientAction` -> Frontend executes -> `BladeIntent::SubmitClientActionResult`.

## 5. Migration Strategy

1.  **Define Protocol**: `BladeIntent` (Request) and `BladeEvent` (Response/Broadcast).
2.  **Centralize Dispatch**: Replace 36 commands with 1 `dispatch(intent)`.
3.  **Refactor Workflow**: Move `ai_workflow.rs` logic into the Dispatcher's robust state machine.
4.  **Standardize Events**: Ensure every state change emits a `BladeEvent`.
5.  **Simplify Frontend**: Reduce `useChat` to a dumb renderer of the State emitted by `BladeEvent`.
