# BLADE Protocol v1.1 Migration Guide

This document guides frontend developers through migrating from v1.0 to v1.1 of the Blade Change Protocol.

## Overview of v1.1 Changes

### 1. Semantic Versioning
- **Old**: `version: u16` (e.g., `1`)
- **New**: `version: Version { major: 1, minor: 1, patch: 0 }`

### 2. Idempotency Support
- **New Field**: `idempotency_key: Option<String>` in `BladeIntentEnvelope`
- **Purpose**: Prevents double-execution on retry for critical operations

### 3. Event Ordering
- **MessageDelta**: Now includes `seq: u64` and `is_final: bool`
- **TerminalOutput**: Now includes `seq: u64`
- **Purpose**: Enables frontend buffering and reordering of out-of-sequence events

### 4. New Events
- **MessageCompleted**: Explicit end-of-stream signal for chat
- **ProcessProgress**: Progress updates for long-running tasks
- **ProtocolVersion**: Version negotiation on connect
- **TerminalSpawned**: Terminal creation with owner tracking
- **BatchCompleted**: Batch operation results with success/failure counts

### 5. Enhanced Types
- **TerminalOwner**: Enum (`User` | `Agent { task_id }`) instead of string
- **VersionMismatch**: Now includes `expected` and `received` versions

---

## Backend Changes (Already Implemented)

### Protocol Types (`blade_protocol.rs`)
```rust
// Version struct with semantic versioning
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

// Idempotency key in intent envelope
pub struct BladeIntentEnvelope {
    pub id: Uuid,
    pub timestamp: u64,
    pub idempotency_key: Option<String>, // NEW
    pub intent: BladeIntent,
}

// Sequence numbers in streaming events
pub enum ChatEvent {
    MessageDelta {
        id: String,
        seq: u64,        // NEW
        chunk: String,
        is_final: bool,  // NEW
    },
    MessageCompleted { id: String }, // NEW
    // ...
}

pub enum TerminalEvent {
    Spawned { id: String, owner: TerminalOwner }, // NEW
    Output {
        id: String,
        seq: u64,    // NEW
        data: String,
    },
    // ...
}
```

### Idempotency Cache (`idempotency.rs`)
- 24-hour TTL by default
- Stores both success and failure results
- Automatically checked in `dispatch()` before execution

### Sequence Number Tracking
- **ChatManager**: `message_seq` field, resets per message
- **Terminal**: `seq` counter per terminal instance

---

## Frontend Migration Tasks

### Task 1: Update Version Handling

**Current (v1.0)**:
```typescript
const envelope = {
  protocol: "BCP",
  version: 1,
  domain: "Chat",
  message: intentEnvelope
};
```

**New (v1.1)**:
```typescript
const envelope = {
  protocol: "BCP",
  version: { major: 1, minor: 1, patch: 0 },
  domain: "Chat",
  message: intentEnvelope
};
```

**Action**: Update `BladeEnvelope` type in frontend and all dispatch calls.

---

### Task 2: Add Idempotency Support

**Use Case**: Critical operations like `ApproveAction`, `SaveFile`

```typescript
// Generate idempotency key for retry-safe operations
const idempotencyKey = `approve-${changeId}-${Date.now()}`;

const intentEnvelope = {
  id: uuidv4(),
  timestamp: Date.now(),
  idempotency_key: idempotencyKey, // NEW
  intent: {
    type: "Workflow",
    payload: {
      type: "ApproveAction",
      payload: { action_id: changeId }
    }
  }
};
```

**Action**: Add `idempotency_key` field to `BladeIntentEnvelope` type and generate keys for critical operations.

---

### Task 3: Implement Event Ordering Buffer

**Problem**: Streaming events may arrive out of order due to async processing.

**Solution**: Buffer events by `seq` and apply in order.

```typescript
interface MessageBuffer {
  id: string;
  chunks: Map<number, { chunk: string; is_final: boolean }>;
  nextSeq: number;
}

const messageBuffers = new Map<string, MessageBuffer>();

function handleMessageDelta(event: MessageDelta) {
  const { id, seq, chunk, is_final } = event;
  
  if (!messageBuffers.has(id)) {
    messageBuffers.set(id, {
      id,
      chunks: new Map(),
      nextSeq: 0
    });
  }
  
  const buffer = messageBuffers.get(id)!;
  buffer.chunks.set(seq, { chunk, is_final });
  
  // Apply chunks in order
  while (buffer.chunks.has(buffer.nextSeq)) {
    const { chunk, is_final } = buffer.chunks.get(buffer.nextSeq)!;
    appendToMessage(id, chunk);
    buffer.chunks.delete(buffer.nextSeq);
    buffer.nextSeq++;
    
    if (is_final) {
      messageBuffers.delete(id);
      break;
    }
  }
}
```

**Action**: Implement buffering for `MessageDelta` and `TerminalOutput` events.

---

### Task 4: Handle New Events

#### A. ProtocolVersion Event
```typescript
useEffect(() => {
  const unlisten = listen<BladeEventEnvelope>("blade-event", (event) => {
    if (event.payload.event.type === "System") {
      const systemEvent = event.payload.event.payload;
      if (systemEvent.type === "ProtocolVersion") {
        const { current, supported } = systemEvent.payload;
        console.log(`Server protocol: ${current.major}.${current.minor}.${current.patch}`);
        
        // Check compatibility
        if (current.major !== 1) {
          showVersionMismatchWarning(current);
        }
      }
    }
  });
  
  return () => { unlisten(); };
}, []);
```

#### B. MessageCompleted Event
```typescript
if (chatEvent.type === "MessageCompleted") {
  const { id } = chatEvent.payload;
  markMessageComplete(id);
  stopLoadingSpinner();
}
```

#### C. ProcessProgress Event
```typescript
if (systemEvent.type === "ProcessProgress") {
  const { intent_id, progress, message } = systemEvent.payload;
  updateProgressBar(intent_id, progress, message);
}
```

#### D. BatchCompleted Event
```typescript
if (workflowEvent.type === "BatchCompleted") {
  const { batch_id, succeeded, failed } = workflowEvent.payload;
  showNotification(`Batch complete: ${succeeded} succeeded, ${failed} failed`);
}
```

**Action**: Add event listeners for new v1.1 events.

---

### Task 5: Update Terminal Handling

**TerminalSpawned Event**:
```typescript
if (terminalEvent.type === "Spawned") {
  const { id, owner } = terminalEvent.payload;
  
  // Track owner for UI display
  if (owner.type === "Agent") {
    markTerminalAsAgent(id, owner.data.task_id);
  } else {
    markTerminalAsUser(id);
  }
}
```

**TerminalOutput with Sequence**:
```typescript
// Similar buffering as MessageDelta
const terminalBuffers = new Map<string, TerminalBuffer>();

function handleTerminalOutput(event: TerminalOutput) {
  const { id, seq, data } = event;
  // Buffer and apply in order...
}
```

**Action**: Update terminal event handlers to use `TerminalOwner` enum and sequence numbers.

---

### Task 6: Migrate from Legacy Events

**Current State**: Backend emits both legacy events (`chat-update`) and new v1.1 events (`blade-event`).

**Migration Path**:
1. **Phase 1**: Add v1.1 event listeners alongside legacy listeners
2. **Phase 2**: Test v1.1 event handling thoroughly
3. **Phase 3**: Remove legacy event listeners
4. **Phase 4**: Backend removes legacy event emissions

**Example**:
```typescript
// Phase 1: Dual listeners
listen("chat-update", handleLegacyChatUpdate);
listen("blade-event", (event) => {
  if (event.payload.event.type === "Chat") {
    handleV11ChatEvent(event.payload.event.payload);
  }
});

// Phase 3: Remove legacy
// listen("chat-update", handleLegacyChatUpdate); // REMOVED
```

---

## Testing Checklist

- [ ] Version negotiation works (ProtocolVersion event received)
- [ ] Idempotency prevents double-execution on retry
- [ ] MessageDelta events arrive in order (or are buffered correctly)
- [ ] TerminalOutput events arrive in order (or are buffered correctly)
- [ ] MessageCompleted event fires at end of stream
- [ ] ProcessProgress events update UI during long operations
- [ ] BatchCompleted event shows correct success/failure counts
- [ ] TerminalSpawned event tracks owner correctly
- [ ] VersionMismatch error shows expected vs. received versions

---

## Rollback Plan

If issues arise, the backend maintains backward compatibility:
- Legacy event emissions are still active (TODO comments mark them)
- Frontend can continue using legacy events until v1.1 migration is complete
- No breaking changes to existing v1.0 functionality

---

## Next Steps

1. Update frontend types to match v1.1 protocol
2. Implement event ordering buffers
3. Add v1.1 event listeners
4. Test with backend
5. Remove legacy event listeners once stable
6. Update backend to remove legacy emissions

---

*Last Updated: 2026-01-08*
*Protocol Version: 1.1.0*
