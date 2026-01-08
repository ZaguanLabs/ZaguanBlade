# Blade Change Protocol (BCP) v1.0 [Hardened]

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
    protocol: "BCP",  // Protocol identifier
    version: u16,     // e.g., 1
    domain: String,   // e.g., "Chat", "Editor"
    message: T,
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
    pub id: Uuid,        // Unique ID for correlation/causality
    pub timestamp: u64,  // Client-side timestamp
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
    // Universal Error Event
    IntentFailed {
        intent_id: Uuid, 
        error: BladeError 
    },
    // Lifecycle Events
    ProcessStarted { intent_id: Uuid },
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

### Rule 3: Versioning
The Backend MUST check the `version` field.
*   **Minor mismatch**: Warn but proceed (if backward compatible).
*   **Major mismatch**: Reject with `BladeError::VersionMismatch`.

### Rule 4: Explicit Lifecycle
For long-running tasks (e.g., "Run Benchmark"), the backend must emit:
1.  `SystemEvent::ProcessStarted` (ack)
2.  `SystemEvent::ProcessProgress` (optional)
3.  `SystemEvent::ProcessCompleted` OR `SystemEvent::IntentFailed`

## 4. Domain Specifications

### A. Chat Domain
*   **Intents**: `SendMessage`, `StopGeneration`, `ClearHistory`.
*   **Events**:
    *   `ChatState { messages: Vec<Message> }` (Full sync)
    *   `MessageDelta { id: Uuid, chunk: String }` (Streaming)
    *   `GenerationSignal { is_generating: bool }`

### B. Editor Domain
*   **Intents**: `OpenFile`, `SaveFile`, `BufferUpdate` (Virtual Buffers).
*   **Events**:
    *   `EditorState { active_file: String, ... }`
    *   `ContentDelta { file: String, patch: String }` (Collaborative editing prep)

### C. Workflow Domain
*   **Intents**: `ApproveAction`, `RejectAction`.
*   **Events**:
    *   `ApprovalRequested { batch_id: String, items: Vec<Action> }`
    *   `TaskCompleted { task_id: String, result: ToolResult }`

### D. Terminal Domain (Hardened)
*   **Intents**:
    *   `Spawn { command: String, owner: String }` (Owner = "User" | "Agent:Check")
    *   `Input { id: String, data: String }`
    *   `Resize { id: String, rows: u16, cols: u16 }`
    *   `Kill { id: String }`
*   **Events**:
    *   `TerminalOutput { id: String, data: String }`
    *   `TerminalExit { id: String, code: i32 }`

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
*Status: Hardened v1.0*
*Last Updated: 2026-01-08*
