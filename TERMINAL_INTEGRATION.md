# Terminal Integration for AI Command Execution

## Overview

Commands from the AI now execute in a live xterm.js terminal widget displayed inline in the chat. The terminal output is captured and sent back to the AI, allowing it to see build failures and fix them.

## Architecture

```
User Approves Command
    ↓
Backend (lib.rs) emits command-execution-started event
    ↓
Frontend (useCommandExecution hook) receives event
    ↓
ChatPanel displays ChatTerminal component
    ↓
ChatTerminal calls execute_command_in_terminal
    ↓
Backend (terminal.rs) spawns PTY and streams output
    ↓
Terminal output appears in xterm.js widget
    ↓
Command completes, terminal-exit event emitted
    ↓
ChatTerminal onComplete callback fires
    ↓
Frontend calls submit_command_result
    ↓
Backend adds result to batch
    ↓
Result sent to AI in conversation
```

## Components

### Backend (Rust)

1. **`terminal.rs`** - Added `execute_command_in_terminal()`
   - Spawns command in PTY
   - Streams output via `terminal-output` events
   - Emits `terminal-exit` event with exit code

2. **`lib.rs`** - Modified command approval flow
   - Line 220-239: Emits `command-execution-started` instead of blocking
   - Line 317-347: Added `submit_command_result()` command
   - Commands execute asynchronously with terminal display

3. **`events.rs`** - Added event infrastructure
   - `COMMAND_EXECUTION_STARTED` event
   - `CommandExecutionStartedPayload` struct

### Frontend (TypeScript/React)

1. **`ChatTerminal.tsx`** - New component
   - Compact xterm.js widget (8 rows default, 20 expanded)
   - Read-only display
   - Captures all output for AI feedback
   - Calls `onComplete` callback with output and exit code

2. **`useCommandExecution.ts`** - New hook
   - Listens for `command-execution-started` events
   - Manages execution state (running/completed)
   - Calls `submit_command_result` on completion

3. **`ChatPanel.tsx`** - Integration point
   - Uses `useCommandExecution` hook
   - Displays `ChatTerminal` for active executions
   - Passes `handleCommandComplete` callback

## Flow Example

```typescript
// 1. User approves "cargo build"
approveToolDecision('approve_once')

// 2. Backend emits event
emit('command-execution-started', {
  command_id: 'cmd-call_123',
  call_id: 'call_123',
  command: 'cargo build',
  cwd: '/path/to/project'
})

// 3. Frontend displays terminal
<ChatTerminal
  commandId="cmd-call_123"
  command="cargo build"
  cwd="/path/to/project"
  onComplete={(output, exitCode) => {
    // 4. Submit result to backend
    invoke('submit_command_result', {
      callId: 'call_123',
      output: "Compiling...\nerror: ...",
      exitCode: 1
    })
  }}
/>

// 5. Backend adds to batch results
batch.file_results.push((call, ToolResult {
  success: false,
  content: "Compiling...\nerror: ...",
  error: Some("Command failed with exit code 1")
}))

// 6. Result sent to AI
// AI sees the error and can fix it
```

## Key Features

✅ **Live Terminal Display** - Commands execute in real xterm.js terminal
✅ **Output Capture** - All stdout/stderr captured for AI
✅ **Exit Code Tracking** - Success/failure properly reported
✅ **Expandable UI** - Terminal can expand from 8 to 20 rows
✅ **Non-Blocking** - Commands run asynchronously
✅ **AI Feedback Loop** - Output sent back to conversation

## Testing

To test the integration:

1. Start zblade
2. Ask AI to run a command: "Run `cargo build`"
3. Approve the command
4. Terminal should appear in chat showing live output
5. When complete, output is sent back to AI
6. AI can see errors and suggest fixes

## Files Modified

**Backend:**
- `src-tauri/src/terminal.rs` - Added `execute_command_in_terminal()`
- `src-tauri/src/lib.rs` - Modified approval flow, added `submit_command_result()`
- `src-tauri/src/events.rs` - Added command execution events

**Frontend:**
- `src/components/ChatTerminal.tsx` - New terminal widget component
- `src/hooks/useCommandExecution.ts` - New execution state hook
- `src/components/ChatPanel.tsx` - Integrated terminal display

## Benefits

1. **Visibility** - User sees exactly what the AI is running
2. **Debugging** - AI can see build failures and fix them
3. **Trust** - No hidden command execution
4. **Feedback** - Real-time progress indication
5. **Context** - AI maintains full awareness of command results
