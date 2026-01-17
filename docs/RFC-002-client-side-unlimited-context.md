# RFC-002: Hybrid Unlimited Context Architecture

## Status

**Draft**

## Authors

Stig-Ørjan Smelror

## Target Audience

System architects, frontend engineers, AI engineers

## Motivation

The unlimited context feature provides significant value but creates a trust barrier when code is stored server-side. Users are reluctant to send proprietary code to third-party servers.

However, different users have different priorities:
- **Privacy-focused users**: Want code on their machine only
- **Performance-focused users**: Want fast context retrieval, willing to trust server
- **Enterprise users**: Need compliance, prefer local storage
- **Individual developers**: Want convenience, may prefer server storage

This RFC proposes a **hybrid storage architecture** that:
1. Lets users choose: local storage OR server storage
2. Uses reference-based storage for local mode (no code duplication)
3. Uses full storage for server mode (faster context retrieval)
4. Allows switching between modes
5. Uses compressor AI to intelligently assemble context

## Goals

* Provide user choice between local and server storage
* Local mode: Store references only, maximum privacy
* Server mode: Store full context, maximum performance
* Enable seamless switching between modes
* Use compressor AI for context assembly in both modes
* Maintain unlimited context capability in both modes

## Non-Goals

* Client-side encryption (unnecessary overhead for local storage)
* Forcing users into one storage mode
* Complex multi-tier storage options (keep it simple: local OR server)

---

## Storage Modes Comparison

### Local Storage Mode

**How it works:**
- Conversations stored in `.zblade/` folder
- Full messages stored locally (not just references)
- Server requests context on-demand via WebSocket
- Artifacts still stored server-side (MMU unchanged)
- Metadata only stored on server

**Pros:**
- ✅ Maximum privacy (code never leaves machine)
- ✅ User controls all data
- ✅ Works offline
- ✅ No trust required
- ✅ Efficient storage (references only)

**Cons:**
- ❌ Extra network round-trip (+1 per request for context fetch)
- ❌ Slightly higher latency (~100-200ms)
- ❌ No cross-device sync (unless user manages it)
- ❌ Increased client complexity (storage, indexing, migration)

**Best for:**
- Enterprise developers
- Privacy-conscious users
- Proprietary code
- Compliance requirements

### Server Storage Mode

**How it works:**
- Full conversations stored on server (encrypted)
- Code included in storage
- Server-side context assembly
- Faster retrieval (no compression needed)

**Pros:**
- ✅ One less network round-trip per request
- ✅ Faster context retrieval (~100ms faster)
- ✅ Cross-device sync
- ✅ Server-side search and indexing
- ✅ Simpler client (no local storage management)

**Cons:**
- ❌ Code stored on third-party server
- ❌ Requires trust in service provider
- ❌ Potential compliance issues
- ❌ Data breach risk

**Best for:**
- Individual developers
- Non-proprietary code
- Users who trust the service
- Performance-focused users

### Comparison Table

| Feature | Local Storage | Server Storage |
|---------|--------------|----------------|
| **Privacy** | Maximum | Moderate |
| **Performance** | ~17% slower | Faster |
| **Network Traffic** | +2 KB/request | Baseline |
| **Trust Required** | None | Yes |
| **Offline Support** | Yes | No |
| **Cross-Device Sync** | Manual | Automatic |
| **Storage Location** | User's machine | zcoderd servers |
| **Compliance** | Easy | Complex |

---

## High-Level Architecture

### Local Storage Mode

```
┌─────────────────────────────────────────────────────────┐
│  Project Directory                                      │
│                                                         │
│  src/                                                   │
│    auth.ts          ← Actual code files                 │
│    user.ts                                              │
│                                                         │
│  .zblade/                    ← Always present           │
│    instructions.md           ← Project instructions     │
│    artifacts/                ← Local mode only          │
│      conversations/                                     │
│        conv_abc.json    ← References to code            │
│      moments/                                           │
│        moment_xyz.json  ← Extracted decisions           │
│    index/                    ← Local mode only          │
│      conversations.db   ← SQLite for search             │
│    cache/                    ← Both modes               │
│      context.json       ← Hot cache                     │
│    config/                   ← Both modes               │
│      settings.json      ← Storage mode: "local"         │
└─────────────────────────────────────────────────────────┘

                    ▼
            zblade sends message + storage_mode: "local"
                    ▼
            zcoderd requests context from zblade
                    ▼
            zblade loads from .zblade/ and sends to zcoderd
                    ▼
            zcoderd routes to AI model
                    ▼
            AI requests artifacts (from server DB, not client)
                    ▼
            zcoderd retrieves artifacts and sends to AI
```

```
┌─────────────────────────────────────────────────────────┐
│  Project Directory                                      │
│                                                         │
│  src/                                                   │
│    auth.ts          ← Actual code files                 │
│    user.ts                                              │
│                                                         │
│  .zblade/                    ← Always present           │
│    instructions.md           ← Project instructions     │
│    config/                   ← Both modes               │
│      settings.json      ← Storage mode: "server"        │
│    cache/                    ← Both modes               │
│      recent.json        ← Local cache only              │
│                                                         │
│  (No artifacts/ or index/ in server mode)              │
└─────────────────────────────────────────────────────────┘

                    ▼
            zblade sends message + storage_mode: "server"
                    ▼
            zcoderd stores full message in DB
                    ▼
            zcoderd loads context from DB
                    ▼
            zcoderd routes to AI model
                    ▼
            AI requests artifacts (from server DB)
                    ▼
            zcoderd retrieves artifacts and sends to AI
```

---

## Data Model

### Conversation Artifact (`.zblade/artifacts/conversations/conv_abc.json`)

```json
{
  "version": "1.0",
  "conversation_id": "conv_abc123",
  "project_id": "proj_123",
  "created_at": "2026-01-17T14:00:00Z",
  "updated_at": "2026-01-17T15:30:00Z",
  "title": "Implement JWT authentication",
  "messages": [
    {
      "id": "msg_001",
      "role": "user",
      "content": "How do I implement JWT auth?",
      "timestamp": "2026-01-17T14:00:00Z",
      "code_references": [
        {
          "file": "src/auth.ts",
          "lines": [1, 50],
          "context": "User asked about this file"
        }
      ]
    },
    {
      "id": "msg_002",
      "role": "assistant",
      "content": "Here's how to implement JWT authentication...",
      "timestamp": "2026-01-17T14:01:00Z",
      "code_references": [
        {
          "file": "src/auth.ts",
          "lines": [10, 25],
          "context": "Generated this code",
          "diff": {
            "type": "addition",
            "content": "export function generateToken(user: User) { ... }"
          }
        }
      ]
    }
  ],
  "moments": [
    {
      "id": "moment_001",
      "type": "decision",
      "content": "Decided to use JWT with httpOnly cookies for security",
      "message_id": "msg_002",
      "timestamp": "2026-01-17T14:01:00Z",
      "tags": ["auth", "security", "jwt"]
    }
  ],
  "metadata": {
    "total_messages": 15,
    "total_tokens": 12500,
    "models_used": ["anthropic/claude-sonnet-4"],
    "tags": ["authentication", "security"]
  }
}
```

### Code Reference Format

**Instead of storing code:**
```json
{
  "code": "function authenticate(token: string) { ... }"
}
```

**Store reference:**
```json
{
  "file": "src/auth.ts",
  "lines": [10, 25],
  "git_hash": "abc123def",  // Optional: for version tracking
  "context": "Authentication function discussed in conversation"
}
```

**At context assembly time, GUI reads the actual file:**
```typescript
const codeContent = fs.readFileSync('src/auth.ts', 'utf8')
  .split('\n')
  .slice(9, 25)  // Lines 10-25 (0-indexed)
  .join('\n');
```

### Moment Extraction (`.zblade/artifacts/moments/moment_xyz.json`)

```json
{
  "id": "moment_xyz789",
  "conversation_id": "conv_abc123",
  "type": "decision",
  "content": "Use JWT with httpOnly cookies. Refresh tokens stored in Redis with 7-day expiry.",
  "context": "Discussed authentication strategy. Considered OAuth, sessions, and JWT. JWT chosen for stateless API.",
  "code_references": [
    {
      "file": "src/auth.ts",
      "lines": [10, 25],
      "purpose": "Token generation logic"
    },
    {
      "file": "src/middleware/auth.ts",
      "lines": [5, 30],
      "purpose": "Auth middleware implementation"
    }
  ],
  "tags": ["auth", "security", "jwt", "cookies"],
  "created_at": "2026-01-17T14:01:00Z",
  "relevance_score": 0.95
}
```

---

## Local Index (SQLite)

### Schema (`.zblade/index/conversations.db`)

```sql
-- Conversations table
CREATE TABLE conversations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    title TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    message_count INTEGER DEFAULT 0,
    tags TEXT,  -- JSON array
    artifact_path TEXT NOT NULL  -- Path to JSON file
);

CREATE INDEX idx_conv_project ON conversations(project_id);
CREATE INDEX idx_conv_created ON conversations(created_at DESC);

-- Moments table
CREATE TABLE moments (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    type TEXT NOT NULL,  -- decision, pattern, solution, context
    content TEXT NOT NULL,
    context TEXT,
    tags TEXT,  -- JSON array
    created_at TIMESTAMP NOT NULL,
    relevance_score REAL DEFAULT 0.5,
    artifact_path TEXT NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE INDEX idx_moment_conv ON moments(conversation_id);
CREATE INDEX idx_moment_type ON moments(type);
CREATE INDEX idx_moment_score ON moments(relevance_score DESC);

-- Full-text search on moments
CREATE VIRTUAL TABLE moments_fts USING fts5(
    content,
    context,
    tags,
    content=moments,
    content_rowid=rowid
);

-- Code references table
CREATE TABLE code_references (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    context TEXT,
    created_at TIMESTAMP NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE INDEX idx_code_ref_file ON code_references(file_path);
CREATE INDEX idx_code_ref_conv ON code_references(conversation_id);

-- File index (track which files are referenced)
CREATE TABLE file_references (
    file_path TEXT PRIMARY KEY,
    reference_count INTEGER DEFAULT 0,
    first_referenced TIMESTAMP,
    last_referenced TIMESTAMP
);
```

---

## Context Retrieval Flow (Local Mode)

### 1. User Sends Message

```typescript
// In zblade
const userMessage = "How did we implement authentication?";

// Send to zcoderd with storage mode flag
await websocket.send({
  type: "chat_request",
  payload: {
    session_id: "session-123",
    model_id: "anthropic/claude-sonnet-4-5",
    message: userMessage,
    storage_mode: "local"  // Key difference
  }
});
```

### 2. zcoderd Requests Context from Client

```go
// In zcoderd
if payload.StorageMode == "local" {
    // Request conversation history from client
    contextRequest := WebSocketMessage{
        Type: "get_conversation_context",
        ID: generateID(),
        Payload: map[string]interface{}{
            "session_id": payload.SessionID,
        },
    }
    
    // Send and wait for response
    c.sendAndWait(contextRequest)
}
```

### 3. zblade Loads from Local Storage

```typescript
// In zblade - handle context request
async function handleGetConversationContext(msg: WebSocketMessage) {
  const { session_id } = msg.payload;
  
  // Load from .zblade/artifacts/conversations/
  const conversationPath = `.zblade/artifacts/conversations/${session_id}.json`;
  const conversation = JSON.parse(fs.readFileSync(conversationPath, 'utf8'));
  
  // Send back to server
  await websocket.send({
    type: "conversation_context",
    id: msg.id,
    payload: {
      session_id: session_id,
      messages: conversation.messages  // Full message history
    }
  });
}
```

### 4. zcoderd Sends to AI

```go
// In zcoderd - received context from client
contextResponse := c.receiveContextResponse()
messages := contextResponse.Messages

// Send to AI (same as server mode from here)
c.streamAIResponseWS(ctx, requestID, sessionID, modelID, messages, apiKey)
```

### 5. AI Requests Artifact (If Needed)

```
AI: "Let me check @artifact:main_rs_v3"
  ↓
zcoderd: Checks storage_mode = "local"
zcoderd: Retrieves from SERVER DB (artifacts always server-side)
  ↓
AI: Gets artifact content
```

**Key Points:**
- Conversation history: Retrieved from client on-demand
- Artifacts: Always retrieved from server DB (MMU unchanged)
- No upfront compression needed
- Works like tool execution pattern

---

## Server-Side Changes

### Session Storage Changes

```go
// Session struct with storage mode
type Session struct {
    ID           string
    UserID       string
    ModelID      string
    StorageMode  string    // "local" or "server"
    MetadataOnly bool      // true for local mode
    Messages     []Message // Empty for local mode
    Metadata     []MessageMetadata // For local mode
    CreatedAt    time.Time
    UpdatedAt    time.Time
}

// Metadata-only storage for local mode
type MessageMetadata struct {
    Role       string
    Timestamp  time.Time
    TokenCount int
    // NO Content field
}
```

### WebSocket Message Handlers

```go
// Handle chat request with storage mode
func (c *WebSocketConnection) handleChatRequest(msg WebSocketMessage) error {
    payload := parseChatRequest(msg)
    
    // Branch based on storage mode
    if payload.StorageMode == "local" {
        // Request context from client
        context := c.requestContextFromClient(payload.SessionID)
        messages := context.Messages
        
        // Store metadata only
        c.handler.sessions.AddMessageMetadata(payload.SessionID, MessageMetadata{
            Role: "user",
            Timestamp: time.Now(),
            TokenCount: estimateTokens(payload.Message),
        })
    } else {
        // Server mode: load from DB
        session := c.handler.sessions.Get(payload.SessionID)
        messages := session.Messages
        
        // Store full message
        c.handler.sessions.AddMessage(payload.SessionID, Message{
            Role: "user",
            Content: payload.Message,
        })
    }
    
    // Send to AI (same for both modes)
    c.streamAIResponseWS(ctx, requestID, sessionID, modelID, messages, apiKey)
}
```

### New WebSocket Message Types

```go
// Request conversation context from client
type GetConversationContextMessage struct {
    Type    string `json:"type"` // "get_conversation_context"
    ID      string `json:"id"`
    Payload struct {
        SessionID string `json:"session_id"`
    } `json:"payload"`
}

// Response from client with context
type ConversationContextMessage struct {
    Type    string `json:"type"` // "conversation_context"
    ID      string `json:"id"`
    Payload struct {
        SessionID string    `json:"session_id"`
        Messages  []Message `json:"messages"`
    } `json:"payload"`
}
```

---

## Storage Mode Configuration

### User Settings (`.zblade/config/settings.json`)

```json
{
  "storage": {
    "mode": "local",  // "local" or "server"
    "sync_metadata": true,  // Sync conversation titles/tags to server
    "cache": {
      "enabled": true,
      "max_size_mb": 100
    }
  },
  "context": {
    "max_tokens": 8000,
    "compression": {
      "enabled": true,
      "model": "local"  // "local" or "remote"
    }
  },
  "privacy": {
    "telemetry": false
  }
}
```

### Switching Between Modes

**Local → Server:**
```typescript
async function migrateToServer() {
  // 1. Read all local conversations
  const conversations = await loadLocalConversations('.zblade/artifacts/conversations/');
  
  // 2. Upload to server via WebSocket
  for (const conv of conversations) {
    await websocket.send({
      type: 'upload_conversation',
      payload: {
        session_id: conv.id,
        messages: conv.messages,
        metadata: conv.metadata
      }
    });
    
    // Wait for confirmation
    await waitForUploadComplete(conv.id);
  }
  
  // 3. Update settings
  await updateSettings({ storage: { mode: 'server' } });
  
  // 4. Clean up local artifacts (optional)
  if (confirm('Delete local conversations after upload?')) {
    await cleanupLocalArtifacts('.zblade/artifacts/conversations/');
    await cleanupLocalIndex('.zblade/index/');
  }
  // Note: .zblade/ directory remains for project instructions
}
```

**Server → Local:**
```typescript
async function migrateToLocal() {
  // 1. Download all conversations from server
  const conversations = await downloadFromServer();
  
  // 2. Convert to reference-based format
  const localArtifacts = await convertToReferences(conversations);
  
  // 3. Save to .zblade/
  await saveLocalArtifacts(localArtifacts);
  
  // 4. Update settings
  await updateSettings({ storage: { mode: 'local' } });
  
  // 5. Optionally delete from server
  if (confirm('Delete server-side data?')) {
    await deleteFromServer();
  }
}
```

### Default Mode Selection

**First-time setup wizard:**
```
┌─────────────────────────────────────────────────────────┐
│  Welcome to Zaguán Blade!                               │
│                                                         │
│  How would you like to store your conversations?       │
│                                                         │
│  ○ Local Storage (Recommended for privacy)             │
│    • Code stays on your machine                        │
│    • Maximum privacy and control                       │
│    • Works offline                                     │
│    • Slightly slower context retrieval                 │
│                                                         │
│  ○ Server Storage (Recommended for performance)        │
│    • Faster context retrieval                          │
│    • Cross-device sync                                 │
│    • Lower network usage                               │
│    • Requires trust in service provider                │
│                                                         │
│  You can change this anytime in settings.              │
│                                                         │
│  [Continue with Local]  [Continue with Server]         │
└─────────────────────────────────────────────────────────┘
```

### Compressor AI Role

### Purpose

The compressor AI serves as an intelligent filter that:
1. Extracts relevant information from historical context
2. Summarizes decisions and patterns
3. Removes redundant information
4. Fits context into token budget

### Implementation Options

**Option A: Local Compressor (Privacy-First)**
```typescript
// Run small model locally (e.g., Llama 3.2 1B)
const compressor = new LocalCompressor({
  model: 'llama-3.2-1b',
  maxTokens: 4000
});
```

**Option B: Server-Side Compressor (Performance)**
```typescript
// Use fast model via zcoderd
const compressor = new RemoteCompressor({
  endpoint: 'https://api.zaguanai.com/v1/compress',
  model: 'groq/llama-3.3-70b-versatile'  // Fast, cheap
});
```

**Option C: Hybrid**
```typescript
// Compress locally first, refine server-side
const localSummary = await localCompressor.compress(context);
const finalContext = await remoteCompressor.refine(localSummary, query);
```

### Compression Strategy

```typescript
interface CompressionStrategy {
  // Extract key decisions
  extractDecisions(context: Context): Decision[];
  
  // Extract code patterns
  extractPatterns(context: Context): Pattern[];
  
  // Extract solutions to problems
  extractSolutions(context: Context): Solution[];
  
  // Rank by relevance to current query
  rankByRelevance(items: any[], query: string): RankedItem[];
  
  // Fit into token budget
  fitToBudget(items: RankedItem[], maxTokens: number): CompressedContext;
}
```

---

## Benefits

### 1. **Trust: Maximum**
- Code never leaves user's machine ✅
- No server-side code storage ✅
- User controls all data ✅
- Can delete `.zblade/` anytime ✅

### 2. **Unlimited Context: Preserved**
- All conversations stored locally ✅
- Cross-conversation search ✅
- Grows indefinitely ✅
- Offline-capable ✅

### 3. **Performance: Excellent**
- Local SQLite search (fast) ✅
- No network latency for context retrieval ✅
- Compressor AI reduces token usage ✅

### 4. **Storage: Efficient**
- No code duplication ✅
- References instead of copies ✅
- Compressed JSON artifacts ✅

### 5. **Privacy: Complete**
- No encryption overhead needed ✅
- Local storage only ✅
- User controls sharing ✅

---

## Network Traffic Analysis

### Local Storage Mode

**Per Request:**
```
User Query: ~100 bytes
Compressed Context: ~2-4 KB (compressed by AI)
Total Upload: ~4 KB

AI Response: ~5-10 KB
Total Download: ~10 KB

Total per request: ~14 KB
```

**Example:**
```json
// What gets sent to server (local mode)
{
  "message": "How did we implement auth?",
  "context": "Authentication uses JWT with httpOnly cookies. Token generation in src/auth.ts lines 10-25. Refresh logic uses Redis with 7-day expiry. Fixed race condition with mutex lock.",
  "metadata": {
    "conversation_id": "conv_abc123",
    "project_id": "proj_123"
  }
}
```

### Server Storage Mode

**Per Request:**
```
User Query: ~100 bytes
Conversation ID: ~50 bytes
Total Upload: ~150 bytes

AI Response: ~5-10 KB
Total Download: ~10 KB

Total per request: ~10 KB
```

**Example:**
```json
// What gets sent to server (server mode)
{
  "message": "How did we implement auth?",
  "conversation_id": "conv_abc123",
  "project_id": "proj_123"
}

// Server already has full context, no need to send
```

### Traffic Comparison

| Scenario | Local Mode | Server Mode | Difference |
|----------|-----------|-------------|------------|
| **Initial message** | 4 KB | 150 bytes | +3.85 KB |
| **With context (10 past convs)** | 15 KB | 150 bytes | +14.85 KB |
| **With context (50 past convs)** | 30 KB | 150 bytes | +29.85 KB |
| **100 requests/day** | 1.5 MB | 15 KB | +1.485 MB |
| **1000 requests/day** | 15 MB | 150 KB | +14.85 MB |

**Analysis:**
- Local mode: ~30 KB extra per request (with rich context)
- Server mode: Minimal upload, context already on server
- For heavy users (100+ requests/day): Server mode saves ~15 MB/day
- For light users (<10 requests/day): Difference negligible

**Recommendation:**
- Privacy-focused: Choose local (worth the extra bandwidth)
- Performance-focused: Choose server (faster, less traffic)
- Mobile/limited bandwidth: Choose server

---

## Migration Path

### Phase 1: Implement Local Storage (2-3 weeks)
- [ ] Create `.zblade/` directory structure
- [ ] Implement SQLite index
- [ ] Build artifact storage (JSON)
- [ ] Implement code reference system
- [ ] GUI: Context search and assembly

### Phase 2: Implement Compressor AI (1-2 weeks)
- [ ] Design compression prompts
- [ ] Implement local compressor (optional)
- [ ] Implement remote compressor
- [ ] Test compression quality

### Phase 3: Update zcoderd (1 week)
- [ ] Add metadata-only API endpoints
- [ ] Remove full conversation storage
- [ ] Update chat API to accept compressed context
- [ ] Deploy changes

### Phase 4: Migrate Existing Users (1 week)
- [ ] Export existing conversations to `.zblade/`
- [ ] Build migration tool
- [ ] User notification and migration guide

---

## Edge Cases

### File Moved or Deleted

```json
{
  "file": "src/auth.ts",
  "status": "not_found",
  "last_known_location": "src/auth.ts",
  "last_seen": "2026-01-17T14:00:00Z",
  "fallback": "Code no longer available at this location"
}
```

### Git Branch Changes

```json
{
  "file": "src/auth.ts",
  "git_hash": "abc123def",
  "current_hash": "xyz789ghi",
  "status": "modified",
  "note": "File has changed since conversation"
}
```

### Large Context (>100k tokens)

```typescript
// Compressor AI automatically prioritizes
const compressed = await compressor.compress(context, {
  maxTokens: 4000,
  strategy: 'prioritize_recent_and_relevant'
});
```

---

## Security Considerations

### Local Storage Security

**No encryption needed:**
- Files are already on user's disk
- Same security as rest of project
- User controls access permissions

**Optional encryption:**
- User can encrypt `.zblade/` folder if desired
- OS-level encryption (FileVault, BitLocker)
- Not our responsibility

### Data Leakage Prevention

**What gets sent to server:**
- Compressed context (summaries only)
- No file paths
- No actual code
- No project structure

**What stays local:**
- All code
- All file references
- Full conversation history
- Project structure

---

## Future Enhancements

### Short-term (3-6 months)
- Semantic search with local embeddings
- Better compression algorithms
- Multi-project context
- Team sharing (opt-in)

### Medium-term (6-12 months)
- Optional encrypted cloud backup
- Cross-device sync
- Collaborative contexts
- Advanced compressor models

### Long-term (12+ months)
- AI-powered moment detection
- Automatic code reference extraction
- Knowledge graph visualization
- Context quality metrics

---

## Success Metrics

### User Trust
- Adoption rate of local storage mode
- User feedback on privacy
- Enterprise customer acquisition

### Performance
- Context assembly time (<100ms)
- Compression quality (relevance score)
- Token usage reduction (vs. full context)

### Storage Efficiency
- Average `.zblade/` folder size
- Reference vs. duplication ratio
- SQLite query performance

---

## Conclusion

The hybrid storage architecture solves the trust vs. performance dilemma by giving users choice. By offering both local and server storage modes, we achieve:

**For Privacy-Focused Users (Local Mode):**
- ✅ Maximum privacy (code never leaves machine)
- ✅ User controls all data
- ✅ Reference-based storage (efficient)
- ✅ Compressor AI handles context assembly
- ⚠️ Slightly higher network traffic

**For Performance-Focused Users (Server Mode):**
- ✅ Faster context retrieval
- ✅ Lower network traffic
- ✅ Cross-device sync
- ✅ Server-side search and indexing
- ⚠️ Requires trust in service provider

**For Everyone:**
- ✅ Unlimited context in both modes
- ✅ Can switch between modes anytime
- ✅ Transparent about trade-offs
- ✅ User makes informed choice

This architecture positions zcoderd as the **flexible unlimited context AI coding assistant** that respects user choice and priorities.

**Marketing message:**
> "Unlimited context, your way. Choose privacy or performance—or switch anytime. Your code, your rules."

---

## References

- [RFC-001: Hybrid Metadata + Encrypted Blob Storage](./RFC-001-zcoderd-hybrid-metadata.md)
- [Cross-Conversation Memory System](./Cross-Conversation-Memory-System.md)
- [SQLite FTS5 Documentation](https://www.sqlite.org/fts5.html)
