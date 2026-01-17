# Conversation History - Frontend Implementation (BCP Architecture)

**Status**: ✅ Complete  
**Date**: January 16, 2026  
**Architecture**: Blade Change Protocol (BCP) v1.1

## Overview

The frontend implementation for conversation history follows the **Blade Change Protocol (BCP)** architecture. All communication happens through **Intents** (frontend → backend) and **Events** (backend → frontend). The frontend never makes direct HTTP calls or Tauri invokes - everything goes through BCP.

## Frontend Components Implemented

### 1. Type Definitions (`src/types/history.ts`)

```typescript
export interface ConversationSummary {
    id: string;
    project_id: string;
    title: string;
    created_at: string;
    last_active_at: string;
    message_count: number;
    preview: string;
}

export interface FullConversation {
    session_id: string;
    project_id: string;
    title: string;
    created_at: string;
    last_active_at: string;
    message_count: number;
    messages: HistoryMessage[];
}
```

### 2. History Hook (`src/hooks/useHistory.ts`)

Provides:
- `fetchConversations(userId, projectId)` - Fetches conversation list
- `loadConversation(sessionId, userId)` - Loads full conversation
- Loading and error states

### 3. History Tab Component (`src/components/HistoryTab.tsx`)

Features:
- Displays conversation list with titles, previews, and timestamps
- Relative time formatting (e.g., "2h ago", "Yesterday")
- Loading and error states
- Empty state when no conversations exist
- Click to load conversation

### 4. Integration in ChatPanel

- Added `userId`, `projectId`, and `onLoadConversation` props
- Conversation selection switches to Chat tab and loads messages
- Tab switching between Chat and History

## BCP Protocol Flow

### Intent 1: `ListConversations`

**Domain**: `History`  
**Type**: `ListConversations`

**Frontend Dispatches**:
```typescript
await BladeDispatcher.history({
    type: 'ListConversations',
    payload: { user_id: userId, project_id: projectId }
});
```

**Backend Responds With Event**:
```typescript
{
  type: "History",
  payload: {
    type: "ConversationList",
    payload: {
      conversations: [
        {
          id: "session-uuid-1",
          project_id: "proj-456",
          title: "Fix authentication bug",
          created_at: "2024-01-15T10:30:00Z",
          last_active_at: "2024-01-15T12:45:00Z",
          message_count: 42,
          preview: "I need help fixing the authentication flow..."
        }
      ]
    }
  }
}
```

**Backend Implementation**:
1. Receive `ListConversations` Intent via BCP dispatcher
2. Call `GET /v1/blade/history?user_id={userId}&project_id={projectId}`
3. Emit `ConversationList` Event with results via `blade-event`

---

### Intent 2: `LoadConversation`

**Domain**: `History`  
**Type**: `LoadConversation`

**Frontend Dispatches**:
```typescript
await BladeDispatcher.history({
    type: 'LoadConversation',
    payload: { session_id: sessionId, user_id: userId }
});
```

**Backend Responds With Event**:
```typescript
{
  type: "History",
  payload: {
    type: "ConversationLoaded",
    payload: {
      session_id: "session-uuid-1",
      project_id: "proj-456",
      title: "Fix authentication bug",
      created_at: "2024-01-15T10:30:00Z",
      last_active_at: "2024-01-15T12:45:00Z",
      message_count: 42,
      messages: [
        {
          role: "user",
          content: "I need help fixing...",
          created_at: "2024-01-15T10:30:00Z"
        }
      ]
    }
  }
}
```

**Backend Implementation**:
1. Receive `LoadConversation` Intent via BCP dispatcher
2. Call `GET /v1/blade/history/{session_id}?user_id={userId}`
3. Emit `ConversationLoaded` Event with full conversation via `blade-event`

## Backend Implementation Checklist

### Rust/Tauri Side (BCP Handler)

- [ ] Add `History` domain to BCP dispatcher in `src-tauri/src/`
- [ ] Implement `ListConversations` Intent handler
  - Receives Intent with `user_id` and `project_id`
  - Calls `GET /v1/blade/history` endpoint
  - Emits `ConversationList` Event via `blade-event`
- [ ] Implement `LoadConversation` Intent handler
  - Receives Intent with `session_id` and `user_id`
  - Calls `GET /v1/blade/history/{session_id}` endpoint
  - Emits `ConversationLoaded` Event via `blade-event`
- [ ] Add proper error handling (emit System.IntentFailed events)
- [ ] Test BCP flow end-to-end

### Blade Protocol Server Side

According to `docs/blade-protocol-conversation-history-addendum-2026-01-16.md`, the server needs:

- [ ] `GET /v1/blade/history` endpoint
- [ ] `GET /v1/blade/history/{session_id}` endpoint
- [ ] Title generation from first user message
- [ ] Proper project and user scoping
- [ ] Database queries for sessions and messages tables

## BCP Architecture Principles

**Critical**: The frontend **NEVER** makes direct HTTP calls or Tauri invokes. All communication flows through BCP:

1. **Frontend → Backend**: Dispatch Intent via `BladeDispatcher`
2. **Backend Processing**: Handle Intent, perform work (HTTP calls, DB queries, etc.)
3. **Backend → Frontend**: Emit Event via `blade-event` listener
4. **Frontend Reaction**: Update UI state based on Event

This ensures:
- ✅ Single source of truth (backend controls all logic)
- ✅ Consistent error handling
- ✅ Proper separation of concerns
- ✅ Event-driven reactive UI
- ✅ No business logic in frontend

## Current State

**Frontend BCP Integration**: ✅ Complete
- History Intents defined in `src/types/blade.ts`
- History Events defined in `src/types/blade.ts`
- `BladeDispatcher.history()` method implemented
- `useHistory` hook dispatches Intents and listens for Events
- UI components ready and functional

**Backend BCP Handlers**: ⏳ Pending implementation
- Need to add History domain to Rust BCP dispatcher
- Need to implement Intent handlers
- Need to emit Events via `blade-event`

**Backend API Endpoints**: ⏳ Pending implementation (see protocol spec)

## Testing

Once backend commands are implemented, test:

1. **Empty State**: No conversations exist
2. **List Display**: Multiple conversations with different timestamps
3. **Conversation Loading**: Click conversation and verify messages load
4. **Error Handling**: Backend unavailable or invalid session ID
5. **Tab Switching**: Switch between Chat and History tabs
6. **New Conversation**: Click + button (currently placeholder)

## User ID and Project ID

Currently using placeholder values in `Layout.tsx`:
- `userId`: `"user-1"` (hardcoded)
- `projectId`: Uses `workspacePath` or `"default-project"`

These should be replaced with proper values from:
- User authentication system
- Project/workspace configuration
- Backend session management

## Future Enhancements

As noted in the protocol specification:
- Search across conversation content
- Filter by date range or message count
- Archive old conversations
- Share conversations with team members
- Export to markdown/JSON
- Favorite/pin important conversations
