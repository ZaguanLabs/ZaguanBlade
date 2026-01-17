# zblade Implementation Guide: Server Commands & Progress Updates

## Overview

This guide explains how to implement support for server-side commands (`@research`, `@web`, `@search`) and progress updates in zblade.

## Architecture

### Command Flow

```
User Input: "@research best typescript file explorer 2026"
    ↓
zblade: Sends via WebSocket to zcoderd
    ↓
zcoderd: Detects command, executes server-side
    ↓
zcoderd: Sends progress updates via WebSocket
    ↓
zblade: Displays progress in UI
    ↓
zcoderd: Sends final result
    ↓
zblade: Displays result in appropriate UI location
```

## WebSocket Message Types

### 1. Outgoing: Chat Request with Command

When user types a command, send it as a normal chat request:

```json
{
  "type": "chat_request",
  "id": "req_123",
  "payload": {
    "session_id": "session_abc",
    "model_id": "anthropic/claude-sonnet-4-20250514",
    "message": "@research best typescript file explorer 2026",
    "workspace": {
      "root": "/path/to/project",
      "project_id": "proj_8ed179df8e4947ef97a797d3e2af93a2",
      "active_file": "/path/to/file.ts"
    }
  }
}
```

**Important:** Send commands as-is. zcoderd will detect and handle them.

### 2. Incoming: Progress Updates

During `@research` execution, you'll receive progress updates:

```json
{
  "type": "progress",
  "request_id": "req_123",
  "payload": {
    "message": "📄 Extracting content (5/10 sources)...",
    "stage": "scraping",
    "percent": 40
  }
}
```

**Progress Stages:**
- `starting` (0%) - "🔍 Starting web research..."
- `generating_variations` (10%) - "📝 Generating search variations..."
- `searching` (20%) - "🌐 Searching (N variations)..."
- `scraping` (20-60%) - "📄 Extracting content (X/Y sources)..."
- `extracting_content` (40-60%) - "📄 Extracting content (X/Y sources)..."
- `grading` (60-80%) - "⚖️ Grading relevance (X/Y)..."
- `summarizing` (90%) - "✨ Synthesizing results..."
- `complete` (100%) - "✅ Research complete"

### 3. Incoming: Research Results

Research results are sent to a **dedicated research tab** (not the chat):

```json
{
  "type": "research",
  "request_id": "req_123",
  "payload": {
    "content": "# Research Results\n\n## Summary\n\n..."
  }
}
```

### 4. Incoming: Search/Web Results

Search and fetch_url results are sent as text (to the chat):

```json
{
  "type": "text",
  "request_id": "req_123",
  "payload": {
    "content": "Search results:\n1. [Title](url)\n..."
  }
}
```

### 5. Incoming: Done Event

After command completes:

```json
{
  "type": "done",
  "request_id": "req_123",
  "payload": {
    "message_count": 1,
    "tokens_used": 0
  }
}
```

## Implementation Steps

### Step 1: Command Detection (Client-Side)

Detect commands in user input **before** sending to server:

```typescript
function isServerCommand(message: string): boolean {
  const trimmed = message.trim();
  return trimmed.startsWith('@research ') || 
         trimmed.startsWith('@search ') || 
         trimmed.startsWith('@web ');
}

function getCommandType(message: string): 'research' | 'search' | 'web' | null {
  const trimmed = message.trim();
  if (trimmed.startsWith('@research ')) return 'research';
  if (trimmed.startsWith('@search ')) return 'search';
  if (trimmed.startsWith('@web ')) return 'web';
  return null;
}
```

### Step 2: Handle Progress Updates

Create a progress UI component:

```typescript
interface ProgressState {
  message: string;
  stage: string;
  percent: number;
  isActive: boolean;
}

class ProgressTracker {
  private state: ProgressState = {
    message: '',
    stage: '',
    percent: 0,
    isActive: false
  };

  start(requestId: string) {
    this.state.isActive = true;
    this.state.percent = 0;
    // Show progress UI
  }

  update(message: string, stage: string, percent: number) {
    this.state.message = message;
    this.state.stage = stage;
    this.state.percent = percent;
    // Update progress UI
  }

  complete() {
    this.state.isActive = false;
    // Hide progress UI after delay
    setTimeout(() => this.hide(), 2000);
  }
}
```

### Step 3: Message Handler

Handle incoming WebSocket messages:

```typescript
function handleWebSocketMessage(message: any) {
  switch (message.type) {
    case 'progress':
      progressTracker.update(
        message.payload.message,
        message.payload.stage,
        message.payload.percent
      );
      break;

    case 'research':
      // Open dedicated research tab/panel
      openResearchTab(message.payload.content);
      progressTracker.complete();
      break;

    case 'text':
      // Add to chat as assistant message
      addChatMessage('assistant', message.payload.content);
      progressTracker.complete();
      break;

    case 'done':
      progressTracker.complete();
      break;

    case 'error':
      progressTracker.complete();
      showError(message.payload.message);
      break;
  }
}
```

### Step 4: Research Tab UI

Create a dedicated research tab/panel:

```typescript
class ResearchTab {
  private isOpen: boolean = false;
  private content: string = '';

  open(content: string) {
    this.content = content;
    this.isOpen = true;
    // Show research panel/tab
    // Render markdown content
    this.render();
  }

  close() {
    this.isOpen = false;
    // Hide research panel/tab
  }

  render() {
    // Render markdown content in dedicated UI area
    // Should be separate from chat messages
    // Consider: side panel, modal, or dedicated tab
  }
}
```

## UI/UX Recommendations

### Progress Display

**Option 1: Inline Progress Bar (Recommended)**
```
┌─────────────────────────────────────────┐
│ 🔍 @research best typescript explorer  │
│ ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
│ 📄 Extracting content (5/10 sources)   │
│ 40%                                     │
└─────────────────────────────────────────┘
```

**Option 2: Status Bar**
```
Bottom of window:
[📄 Extracting content (5/10 sources)... 40%]
```

### Research Results Display

**Option 1: Side Panel (Recommended)**
```
┌──────────────┬─────────────────────────┐
│              │ 📊 Research Results     │
│              │ ─────────────────────── │
│              │                         │
│  Chat Area   │  # Summary              │
│              │  Found 10 sources...    │
│              │                         │
│              │  ## Sources             │
│              │  1. [Title](url)        │
│              │     Grade: 9/10         │
│              │                         │
└──────────────┴─────────────────────────┘
```

**Option 2: Modal/Overlay**
```
┌─────────────────────────────────────────┐
│  📊 Research Results           [Close]  │
│  ─────────────────────────────────────  │
│                                         │
│  # Summary                              │
│  Found 10 sources with avg relevance... │
│                                         │
│  ## Sources                             │
│  1. **Title** (Grade: 9/10)            │
│     URL: https://...                    │
│     Excerpt...                          │
│                                         │
└─────────────────────────────────────────┘
```

**Option 3: New Tab**
```
[Chat] [Research Results] [Files]
       ↑ New tab opens
```

### Command Input UX

**Visual Feedback:**
```
User types: @research best typescript explorer
            ↑ Highlight command in different color
            ↑ Show tooltip: "Server-side research command"
```

**Autocomplete:**
```
User types: @re
Suggestions:
  @research <query>  - Deep web research
  @read_file <path>  - Read file (client-side)
```

## Error Handling

### Command Execution Errors

```typescript
function handleCommandError(error: any) {
  // Show error in chat
  addChatMessage('system', `❌ Command failed: ${error.message}`);
  
  // Log for debugging
  console.error('Command execution failed:', error);
  
  // Clear progress UI
  progressTracker.complete();
}
```

### Timeout Handling

```typescript
const COMMAND_TIMEOUT = 180000; // 3 minutes

function executeCommand(command: string) {
  const timeoutId = setTimeout(() => {
    handleCommandError({ message: 'Command timed out after 3 minutes' });
  }, COMMAND_TIMEOUT);

  // Send command...
  
  // Clear timeout when done
  return () => clearTimeout(timeoutId);
}
```

## Testing Checklist

### @research Command
- [ ] Progress updates display correctly
- [ ] All stages show appropriate messages
- [ ] Percentage increases smoothly
- [ ] Research tab opens with results
- [ ] Results are formatted as markdown
- [ ] Progress UI clears after completion
- [ ] Errors are handled gracefully

### @search Command
- [ ] Results appear in chat
- [ ] JSON format is parsed correctly
- [ ] URLs are clickable
- [ ] No progress UI needed (fast operation)

### @web Command
- [ ] Content appears in chat
- [ ] Markdown is rendered
- [ ] Long content is scrollable
- [ ] No progress UI needed (fast operation)

### Edge Cases
- [ ] Multiple concurrent commands
- [ ] Command cancellation
- [ ] Network disconnection during command
- [ ] Very long research queries
- [ ] Empty/no results handling

## Example Implementation (TypeScript)

```typescript
// Command handler
class ServerCommandHandler {
  private progressTracker: ProgressTracker;
  private researchTab: ResearchTab;
  private ws: WebSocket;

  constructor(ws: WebSocket) {
    this.ws = ws;
    this.progressTracker = new ProgressTracker();
    this.researchTab = new ResearchTab();
  }

  async sendCommand(message: string, sessionId: string, modelId: string) {
    const commandType = getCommandType(message);
    
    if (!commandType) {
      throw new Error('Not a valid command');
    }

    // Start progress tracking for research
    if (commandType === 'research') {
      this.progressTracker.start(sessionId);
    }

    // Send command as normal chat request
    this.ws.send(JSON.stringify({
      type: 'chat_request',
      id: generateRequestId(),
      payload: {
        session_id: sessionId,
        model_id: modelId,
        message: message,
        workspace: this.getWorkspaceInfo()
      }
    }));
  }

  handleMessage(message: any) {
    switch (message.type) {
      case 'progress':
        this.progressTracker.update(
          message.payload.message,
          message.payload.stage,
          message.payload.percent
        );
        break;

      case 'research':
        this.researchTab.open(message.payload.content);
        this.progressTracker.complete();
        break;

      case 'text':
        this.addToChat(message.payload.content);
        this.progressTracker.complete();
        break;

      case 'done':
        this.progressTracker.complete();
        break;

      case 'error':
        this.handleError(message.payload);
        this.progressTracker.complete();
        break;
    }
  }

  private getWorkspaceInfo() {
    return {
      root: workspace.rootPath,
      project_id: workspace.projectId,
      active_file: workspace.activeFile,
      cursor_position: workspace.cursorPosition
    };
  }

  private addToChat(content: string) {
    // Add message to chat UI
  }

  private handleError(error: any) {
    // Show error in UI
  }
}
```

## Configuration

### User Settings

Consider adding user preferences:

```json
{
  "zcoderd": {
    "commands": {
      "research": {
        "showProgress": true,
        "openInNewTab": true,
        "autoClose": false
      },
      "search": {
        "maxResults": 10,
        "showInChat": true
      }
    }
  }
}
```

## Debugging

### Enable Verbose Logging

```typescript
const DEBUG = true;

function logCommand(command: string, stage: string, data?: any) {
  if (DEBUG) {
    console.log(`[Command:${command}] ${stage}`, data);
  }
}

// Usage:
logCommand('research', 'sent', { query: '...' });
logCommand('research', 'progress', { percent: 40 });
logCommand('research', 'complete', { resultLength: 5000 });
```

### Monitor WebSocket Messages

```typescript
ws.addEventListener('message', (event) => {
  const message = JSON.parse(event.data);
  console.log('[WS ←]', message.type, message);
});

ws.addEventListener('send', (event) => {
  console.log('[WS →]', event.data);
});
```

## Performance Considerations

### Throttle Progress Updates

```typescript
class ThrottledProgressTracker extends ProgressTracker {
  private lastUpdate = 0;
  private throttleMs = 100; // Update UI max every 100ms

  update(message: string, stage: string, percent: number) {
    const now = Date.now();
    if (now - this.lastUpdate < this.throttleMs) {
      return; // Skip update
    }
    this.lastUpdate = now;
    super.update(message, stage, percent);
  }
}
```

### Lazy Load Research Tab

```typescript
class LazyResearchTab {
  private tab: ResearchTab | null = null;

  open(content: string) {
    if (!this.tab) {
      this.tab = new ResearchTab(); // Load only when needed
    }
    this.tab.open(content);
  }
}
```

## Summary

**Key Points:**
1. Send commands as normal chat messages - zcoderd handles detection
2. Listen for `progress`, `research`, `text`, and `done` message types
3. Show progress UI for `@research` (takes 30-60s)
4. Open dedicated research tab for results
5. Handle errors gracefully with user feedback

**Implementation Priority:**
1. ✅ Basic command sending (already works)
2. ⚠️ Progress update handling (needed for good UX)
3. ⚠️ Research tab UI (needed for proper display)
4. ✅ Error handling (basic already works)
5. 🔄 Advanced features (cancellation, history, etc.)

Good luck with the implementation! 🚀
