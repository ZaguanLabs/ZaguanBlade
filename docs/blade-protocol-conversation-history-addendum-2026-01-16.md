# Blade Protocol - Conversation History Addendum

**Version**: 1.0-alpha  
**Date**: January 16, 2026  
**Status**: Specification  
**Authors**: Zaguán AI Team

---

## Overview

This addendum extends the Blade Protocol with conversation history endpoints that enable clients to retrieve and display past conversations. The history feature is project-scoped, ensuring conversations from different projects remain separate and organized.

## Design Principles

1. **Project Isolation**: Conversations are strictly scoped to projects - `user_id` and `project_id` must always be provided together
2. **Efficient Loading**: Separate endpoints for listing conversations (lightweight) vs loading full conversation content (detailed)
3. **Consistent Titles**: Conversation titles are generated from the first user message and cached for consistency
4. **Client-Side Pagination**: Server returns all conversations for a project; client handles pagination/filtering
5. **Authorization**: All requests require `user_id` for ownership verification

---

## API Endpoints

### 1. List Conversations

**Endpoint**: `GET /v1/blade/history`

**Purpose**: Retrieve a list of all conversations for a specific user and project. Used to populate the History tab UI.

#### Request

**Query Parameters**:
- `user_id` (string, required): User identifier
- `project_id` (string, required): Project identifier

**Example**:
```http
GET /v1/blade/history?user_id=user-123&project_id=proj-456
```

#### Response

**Status Code**: `200 OK`

**Content-Type**: `application/json`

**Response Body**:
```json
{
  "conversations": [
    {
      "id": "session-uuid-1",
      "project_id": "proj-456",
      "title": "Fix authentication bug",
      "created_at": "2024-01-15T10:30:00Z",
      "last_active_at": "2024-01-15T12:45:00Z",
      "message_count": 42,
      "preview": "I need help fixing the authentication flow..."
    },
    {
      "id": "session-uuid-2",
      "project_id": "proj-456",
      "title": "Add user profile page",
      "created_at": "2024-01-14T09:15:00Z",
      "last_active_at": "2024-01-14T11:30:00Z",
      "message_count": 28,
      "preview": "Can you help me create a user profile page..."
    }
  ]
}
```

**Response Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `conversations` | array | List of conversation summaries |
| `conversations[].id` | string | Unique session identifier (UUID) |
| `conversations[].project_id` | string | Project identifier |
| `conversations[].title` | string | Conversation title (generated from first user message, max 50 chars) |
| `conversations[].created_at` | string | ISO 8601 timestamp of conversation creation |
| `conversations[].last_active_at` | string | ISO 8601 timestamp of last activity |
| `conversations[].message_count` | integer | Total number of messages in conversation |
| `conversations[].preview` | string | Preview text from first user message (max 50 chars) |

**Ordering**: Conversations are ordered by `last_active_at` in descending order (most recent first).

#### Error Responses

**400 Bad Request** - Missing required parameters:
```json
{
  "error": "user_id and project_id are required"
}
```

**500 Internal Server Error** - Database or server error:
```json
{
  "error": "failed to retrieve conversations"
}
```

---

### 2. Get Full Conversation

**Endpoint**: `GET /v1/blade/history/{session_id}`

**Purpose**: Retrieve the complete message history for a specific conversation. Used when a user selects a conversation from the history list.

#### Request

**Path Parameters**:
- `session_id` (string, required): Session identifier from the conversation list

**Query Parameters**:
- `user_id` (string, required): User identifier for authorization

**Example**:
```http
GET /v1/blade/history/session-uuid-1?user_id=user-123
```

#### Response

**Status Code**: `200 OK`

**Content-Type**: `application/json`

**Response Body**:
```json
{
  "session_id": "session-uuid-1",
  "project_id": "proj-456",
  "title": "Fix authentication bug",
  "created_at": "2024-01-15T10:30:00Z",
  "last_active_at": "2024-01-15T12:45:00Z",
  "message_count": 42,
  "messages": [
    {
      "role": "user",
      "content": "I need help fixing the authentication flow in my Express app...",
      "created_at": "2024-01-15T10:30:00Z"
    },
    {
      "role": "assistant",
      "content": "I'll help you fix the authentication flow. Let me first examine your current implementation.",
      "tool_calls": [
        {
          "id": "call_abc123",
          "type": "function",
          "function": {
            "name": "read_file",
            "arguments": "{\"path\":\"src/auth/middleware.js\"}"
          }
        }
      ],
      "created_at": "2024-01-15T10:30:15Z"
    },
    {
      "role": "tool",
      "content": "// File contents...",
      "tool_call_id": "call_abc123",
      "created_at": "2024-01-15T10:30:16Z"
    }
  ]
}
```

**Response Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | Session identifier |
| `project_id` | string | Project identifier |
| `title` | string | Conversation title |
| `created_at` | string | ISO 8601 timestamp of conversation creation |
| `last_active_at` | string | ISO 8601 timestamp of last activity |
| `message_count` | integer | Total number of messages |
| `messages` | array | Complete message history |
| `messages[].role` | string | Message role: `user`, `assistant`, `tool`, or `system` |
| `messages[].content` | string | Message content |
| `messages[].tool_calls` | array | (Optional) Tool calls made by assistant |
| `messages[].tool_call_id` | string | (Optional) ID of tool call this message responds to |
| `messages[].created_at` | string | ISO 8601 timestamp of message |

**Message Ordering**: Messages are ordered chronologically (oldest first) to maintain conversation flow.

#### Error Responses

**400 Bad Request** - Missing user_id:
```json
{
  "error": "user_id is required"
}
```

**404 Not Found** - Session doesn't exist or user doesn't have access:
```json
{
  "error": "session not found"
}
```

**500 Internal Server Error** - Database or server error:
```json
{
  "error": "failed to retrieve conversation"
}
```

---

## Title Generation

Conversation titles are automatically generated to provide meaningful context in the history list.

### Generation Strategy

1. **Source**: Extract from the first user message in the conversation
2. **Length**: Truncate to 50 characters maximum
3. **Truncation**: Use ellipsis (`...`) if truncated
4. **Storage**: Cache in session metadata (`metadata.title`) for consistency
5. **Timing**: Generate on-demand when history is requested (lazy generation)

### Example Title Generation

**First User Message**:
```
"I need help fixing the authentication flow in my Express application. The JWT tokens are expiring too quickly."
```

**Generated Title**:
```
"I need help fixing the authentication flow in..."
```

### Fallback Behavior

If the first user message cannot be retrieved or is empty:
- **Fallback Title**: `"Conversation on [date]"` (e.g., "Conversation on Jan 15, 2024")

---

## Database Schema

### Sessions Table

The existing `sessions` table supports conversation history with the following relevant fields:

```sql
CREATE TABLE sessions (
    id VARCHAR(64) PRIMARY KEY,
    user_id VARCHAR(64) NOT NULL,
    project_id VARCHAR(64),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_active_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    total_messages INT DEFAULT 0,
    metadata LONGTEXT,  -- Stores JSON including title
    INDEX idx_user_project (user_id, project_id),
    INDEX idx_project_sessions (project_id, last_active_at)
);
```

### Messages Table

The `messages` table stores the complete conversation history:

```sql
CREATE TABLE messages (
    id VARCHAR(64) PRIMARY KEY,
    user_id VARCHAR(64) NOT NULL,
    session_id VARCHAR(64) NOT NULL,
    role ENUM('system', 'user', 'assistant', 'tool') NOT NULL,
    content LONGTEXT NOT NULL,
    tool_call_id VARCHAR(64),
    tool_calls JSON,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    turn_number INT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    INDEX idx_session_turn (session_id, turn_number),
    INDEX idx_user_session (user_id, session_id)
);
```

---

## Client Implementation Guide

### Typical Workflow

1. **User Opens History Tab**
   - Client calls `GET /v1/blade/history?user_id=X&project_id=Y`
   - Display list of conversations with titles, timestamps, and previews

2. **User Selects a Conversation**
   - Client calls `GET /v1/blade/history/{session_id}?user_id=X`
   - Load and display the complete message history in the chat interface

3. **User Continues Conversation**
   - Use the existing `/v1/blade/chat` endpoint with the loaded `session_id`
   - New messages are appended to the existing conversation

### UI Recommendations

**History List Display**:
```
┌─────────────────────────────────────────────────┐
│ Fix authentication bug                          │
│ I need help fixing the authentication flow...  │
│ 42 messages • Last active 2 hours ago          │
├─────────────────────────────────────────────────┤
│ Add user profile page                           │
│ Can you help me create a user profile page...  │
│ 28 messages • Last active yesterday            │
└─────────────────────────────────────────────────┘
```

**Pagination** (Client-Side):
- Implement virtual scrolling for large conversation lists
- Show most recent 50-100 conversations initially
- Load more on scroll

**Search/Filter** (Client-Side):
- Filter by title or preview text
- Filter by date range
- Sort by last active or created date

---

## Security Considerations

### Authorization

1. **User Ownership**: All endpoints verify that `user_id` matches the session owner
2. **Project Scoping**: Conversations are isolated by project - users can only see conversations for their own projects
3. **No Cross-User Access**: Users cannot access conversations from other users, even within the same project

### Data Privacy

1. **Encryption**: Session metadata (including titles) is encrypted at rest if encryption is enabled
2. **Message Content**: Full message content is stored encrypted in the database
3. **No Sensitive Data in URLs**: Session IDs are UUIDs, not sequential integers

---

## Performance Considerations

### List Endpoint Optimization

1. **Lightweight Response**: Only returns metadata, not full message content
2. **Database Index**: Uses `idx_user_project` index for fast filtering
3. **Ordering Index**: Uses `idx_project_sessions` for efficient sorting by `last_active_at`

### Full Conversation Endpoint

1. **Lazy Loading**: Only loads when user explicitly selects a conversation
2. **Caching**: Consider client-side caching of recently viewed conversations
3. **Pagination**: For very long conversations (>1000 messages), consider implementing message pagination in future versions

---

## Future Enhancements

Potential additions for future versions:

1. **Search**: Full-text search across conversation content
2. **Filters**: Filter by date range, model used, or message count
3. **Archiving**: Archive old conversations to reduce list clutter
4. **Sharing**: Share conversation links with team members
5. **Export**: Export conversations to markdown or JSON
6. **Favorites**: Mark important conversations for quick access

---

## Implementation Checklist

Server-side implementation tasks:

- [ ] Add `SessionSummary` type to `internal/context/types.go`
- [ ] Implement `GetProjectSessions()` in `internal/context/store.go`
- [ ] Implement title generation helper in `internal/context/store.go`
- [ ] Add `HandleGetHistory()` handler in `internal/blade/handler.go`
- [ ] Add `HandleGetConversation()` handler in `internal/blade/handler.go`
- [ ] Register routes in main server
- [ ] Add integration tests for both endpoints
- [ ] Update API documentation

Client-side implementation tasks:

- [ ] Create History tab UI component
- [ ] Implement conversation list view
- [ ] Implement conversation selection and loading
- [ ] Add client-side pagination/virtual scrolling
- [ ] Add search and filter functionality
- [ ] Handle loading states and errors
- [ ] Test with various conversation sizes

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2026-01-16 | Initial specification for conversation history endpoints |

---

## References

- [Blade Protocol Main Specification](./blade-protocol.md)
- [Session Management Implementation](./SESSION_MANAGEMENT_IMPLEMENTATION.md)
- [Database Schema Migrations](../internal/context/migrations.go)
