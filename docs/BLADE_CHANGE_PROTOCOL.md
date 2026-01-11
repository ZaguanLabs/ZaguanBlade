# Blade Change Protocol (BCP) v1.1 [Hardened]

> "The only constant is change. The only defense is structure."

This document defines the **Blade Change Protocol**, the standard governing all communication and state management within the Zaguán Blade system. It is the authoritative guide for how the "Body" (Frontend) and "Brain" (Backend) interact.

## 1. Core Philosophy

The system is not a collection of function calls; it is a **Versioned State Machine**.
*   The Backend holds the **Truth** (State).
*   The Frontend renders the **Reflection** (UI).
*   Interaction is the act of requesting a **Change** via a specialized Intent.

### The Unidirectional Flow

1.  **Intent** (Frontend -> Backend): "I want to change X (ID: `uuid`, v1.0)."
2.  **Mutation** (Backend): "I validate and apply change X."
3.  **Event** (Backend -> Frontend): "State X has changed to Y (Caused by: `uuid`)."
4.  **Render** (Frontend): "I update the UI to match Y."

## 2. Hardened Architecture

To ensure durability, we enforce explicit envelopes, versioning, and causality tracking.

### A. The Standard Envelope

All messages (Intents and Events) are wrapped in a standard envelope structure.

```rust
struct BladeEnvelope<T> {
    protocol: "BCP",     // Protocol identifier
    version: Version,    // Semantic version (major.minor.patch)
    domain: String,      // e.g., "Chat", "Editor"
    message: T,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Version {
    pub major: u16,  // Breaking changes
    pub minor: u16,  // Backward-compatible features
    pub patch: u16,  // Bug fixes
}

impl Version {
    pub const CURRENT: Version = Version { major: 1, minor: 1, patch: 0 };
    
    pub fn is_compatible(&self, other: &Version) -> bool {
        self.major == other.major
    }
}
```

### B. Intent (Client -> Server)

An **Intent** is a meaningful request to perform an action. It is *declarative* and *causal*.

*   **Structure**: `BladeIntentEnvelope` (adds Causality metadata).
*   **Transport**: Single Tauri Command `dispatch(envelope: BladeIntentEnvelope)`.

#### Schema (Rust)
```rust
#[derive(Serialize, Deserialize)]
pub struct BladeIntentEnvelope {
    pub id: Uuid,                      // Unique ID for correlation/causality
    pub timestamp: u64,                // Client-side timestamp (ms since epoch)
    pub idempotency_key: Option<String>, // Optional: prevents double-execution on retry
    pub intent: BladeIntent,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum BladeIntent {
    Chat(ChatIntent),
    Editor(EditorIntent),
    Workflow(WorkflowIntent),
    Terminal(TerminalIntent),
    System(SystemIntent),
}
```

### C. Event (Server -> Client)

An **Event** is a notification of something that happened. It is *informative* and strictly categorized.

*   **Categories**:
    *   `*State`: Full authoritative state snapshot.
    *   `*Delta`: Incremental change (patch).
    *   `*Signal`: Ephemeral notification (no state persistence).
*   **Transport**: Tauri Event Emission.

#### Schema (Rust)
```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum BladeEvent {
    Chat(ChatEvent),
    Editor(EditorEvent),
    Workflow(WorkflowEvent),
    Terminal(TerminalEvent),
    System(SystemEvent),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum SystemEvent {
    // Protocol Negotiation
    ProtocolVersion { 
        supported: Vec<Version>,
        current: Version,
    },
    // Universal Error Event
    IntentFailed {
        intent_id: Uuid, 
        error: BladeError 
    },
    // Lifecycle Events
    ProcessStarted { intent_id: Uuid },
    ProcessProgress { intent_id: Uuid, progress: f32, message: Option<String> },
    ProcessCompleted { intent_id: Uuid },
}
```

### D. The Error Model

Errors are first-class citizens, properly typed and correlated.

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "code", content = "details")]
pub enum BladeError {
    ValidationError { field: String, message: String },
    PermissionDenied,
    ResourceNotFound { id: String },
    Conflict { reason: String },
    Internal { trace_id: String, message: String },
}
```

## 3. Implementation Rules

### Rule 1: The Unified Dispatcher
There shall be only **ONE** command exposed to the frontend for business logic: `dispatch`.

### Rule 2: Causality & Correlation
Every Event triggered by a specific Intent MUST include the `intent_id` (if applicable). This allows the UI to resolve promises, stop spinners, and track long-running tasks.

### Rule 3: Versioning (Semantic)
The Backend MUST check the `version` field using semantic versioning rules:
*   **Major mismatch**: Reject with `BladeError::VersionMismatch { expected, received }`.
*   **Minor/Patch mismatch**: Warn (log) but proceed if `major` matches.
*   **On Connect**: Backend emits `SystemEvent::ProtocolVersion` so frontend can adapt or warn user.

### Rule 4: Explicit Lifecycle
For long-running tasks (e.g., "Run Benchmark"), the backend must emit:
1.  `SystemEvent::ProcessStarted { intent_id }` (ack)
2.  `SystemEvent::ProcessProgress { intent_id, progress, message }` (optional, for UI feedback)
3.  `SystemEvent::ProcessCompleted { intent_id }` OR `SystemEvent::IntentFailed { intent_id, error }`

### Rule 5: Idempotency
For critical state-mutating Intents (e.g., `ApproveAction`, `SaveFile`), the backend SHOULD:
*   Check `idempotency_key` if present.
*   If a request with the same key was already processed, return the cached result via `SystemEvent::ProcessCompleted` without re-execution.
*   Store idempotency keys with a TTL (e.g., 24 hours) to prevent memory leaks.

### Rule 6: Event Ordering
For streaming events (e.g., `MessageDelta`), the backend MUST include a monotonic sequence number:
```rust
MessageDelta { 
    id: Uuid, 
    seq: u64,      // Monotonic sequence per message
    chunk: String,
    is_final: bool // True on last chunk
}
```
The frontend SHOULD buffer out-of-order events and apply them in sequence.

## 4. Domain Specifications

### A. Chat Domain
*   **Intents**: `SendMessage`, `StopGeneration`, `ClearHistory`, `RegenerateMessage`.
*   **Events**:
    *   `ChatState { messages: Vec<Message> }` (Full sync)
    *   `MessageDelta { id: Uuid, seq: u64, chunk: String, is_final: bool }` (Streaming)
    *   `MessageCompleted { id: Uuid }` (Explicit end-of-stream)
    *   `GenerationSignal { is_generating: bool }` (UI spinner state)

### B. Editor Domain
*   **Intents**: `OpenFile`, `SaveFile`, `BufferUpdate` (Virtual Buffers).
*   **Events**:
    *   `EditorState { active_file: String, ... }`
    *   `ContentDelta { file: String, patch: String }` (Collaborative editing prep)

### C. Workflow Domain
*   **Intents**: 
    *   `ApproveAction { action_id: String }`
    *   `ApproveAll { batch_id: String }`
    *   `RejectAction { action_id: String }`
    *   `RejectAll { batch_id: String }`
*   **Events**:
    *   `ApprovalRequested { batch_id: String, items: Vec<Action> }`
    *   `ActionCompleted { action_id: String, result: ToolResult }`
    *   `BatchCompleted { batch_id: String, succeeded: usize, failed: usize }`

### D. Terminal Domain (Hardened)
*   **Intents**:
    *   `Spawn { command: String, cwd: Option<String>, owner: TerminalOwner }`
    *   `Input { id: String, data: String }`
    *   `Resize { id: String, rows: u16, cols: u16 }`
    *   `Kill { id: String }`
*   **Events**:
    *   `TerminalSpawned { id: String, owner: TerminalOwner }`
    *   `TerminalOutput { id: String, seq: u64, data: String }` (seq for ordering)
    *   `TerminalExit { id: String, code: i32 }`

```rust
#[derive(Serialize, Deserialize, Clone)]
pub enum TerminalOwner {
    User,
    Agent { task_id: String },
}
```

## 5. Future Capabilities (Reserved Namespaces)

These namespaces are reserved in the Enum to ensure future extensibility without breaking the protocol structure.

### A. Context Domain (`BladeIntent::Context`)
*   **Vision**: Deep codebase understanding (RAG, AST indexing).
*   **Future Intents**: `IndexWorkspace`, `SearchVectors`.

### B. Agent Domain (`BladeIntent::Agent`)
*   **Vision**: Proactive background agents.
*   **Future Intents**: `SpawnAgent`, `PauseAgent`.

### C. Extension Domain (`BladeIntent::Extension`)
*   **Vision**: Sandboxed user extensions.
*   **Security**: Extensions operate in a capability sandbox. The core system can revoke/mute them.

---

## 6. Changelog

### v1.1 (2026-01-08)
**Enhancements:**
*   Added semantic versioning (`Version` struct with `major.minor.patch`).
*   Added `idempotency_key` to `BladeIntentEnvelope` for critical operations.
*   Added sequence numbers (`seq`) to streaming events (`MessageDelta`, `TerminalOutput`).
*   Added `is_final` flag to `MessageDelta` for explicit stream completion.
*   Added `MessageCompleted` event for clear end-of-stream signal.
*   Added `ProcessProgress` event for long-running task feedback.
*   Added `ProtocolVersion` event for version negotiation on connect.
*   Typed `TerminalOwner` as enum (User | Agent) instead of string.
*   Added `ApproveAll` / `RejectAll` intents for batch operations.
*   Added `BatchCompleted` event with success/failure counts.
*   Added `cwd` field to `Spawn` intent for working directory control.
*   Documented idempotency and event ordering rules.

### v1.0 (2026-01-08)
*   Initial hardened protocol definition.
*   Unified dispatcher pattern.
*   Causality tracking via `intent_id`.
*   Explicit error model.
*   State/Delta/Signal taxonomy.
*   Reserved namespaces for future domains.

---
*Status: Hardened v1.1*
*Last Updated: 2026-01-08*
