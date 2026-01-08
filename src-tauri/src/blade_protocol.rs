use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ==============================================================================
// 1. Envelopes
// ==============================================================================

/// The standard envelope for all Client -> Server communication.
#[derive(Debug, Serialize, Deserialize)]
pub struct BladeEnvelope<T> {
    pub protocol: String, // Must be "BCP"
    pub version: u16,     // Must be 1
    pub domain: String,
    pub message: T,
}

/// The causality envelope for Intents.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BladeIntentEnvelope {
    pub id: Uuid,       // Correlation ID
    pub timestamp: u64, // Client-side timestamp
    pub intent: BladeIntent,
}

/// The causality envelope for Events.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BladeEventEnvelope {
    pub id: Uuid,                   // Event ID
    pub timestamp: u64,             // Server-side timestamp
    pub causality_id: Option<Uuid>, // ID of the Intent that caused this
    pub event: BladeEvent,
}

// ==============================================================================
// 2. Intents (Writes)
// ==============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum BladeIntent {
    Chat(ChatIntent),
    Editor(EditorIntent),
    File(FileIntent),
    Workflow(WorkflowIntent),
    Terminal(TerminalIntent),
    System(SystemIntent),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum ChatIntent {
    SendMessage {
        content: String,
        model: String,
        #[serde(default)]
        context: Option<EditorContext>,
    },
    StopGeneration,
    ClearHistory,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EditorContext {
    pub active_file: Option<String>,
    pub open_files: Vec<String>,
    pub cursor_line: Option<u32>,
    pub cursor_column: Option<u32>,
    pub selection_start: Option<u32>,
    pub selection_end: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum EditorIntent {
    OpenFile { path: String },
    SaveFile { path: String },
    BufferUpdate { path: String, content: String }, // Virtual Buffer
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum FileIntent {
    Read { path: String },
    Write { path: String, content: String },
    List { path: Option<String> }, // None = root workspace
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Option<Vec<FileEntry>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum WorkflowIntent {
    ApproveChange { change_id: String },
    RejectChange { change_id: String },
    ApproveAllChanges,
    ApproveTool { approved: bool },
    ApproveToolDecision { decision: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum TerminalIntent {
    Spawn {
        id: String,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        interactive: bool, // true = shell (create_terminal), false = command (execute_command)
    },
    Input {
        id: String,
        data: String,
    },
    Resize {
        id: String,
        rows: u16,
        cols: u16,
    },
    Kill {
        id: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum SystemIntent {
    // For bootstrapping or config
    SetLogLevel { level: String },
}

// ==============================================================================
// 3. Events (Reads/Updates)
// ==============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum BladeEvent {
    Chat(ChatEvent),
    Editor(EditorEvent),
    File(FileEvent),
    Workflow(WorkflowEvent),
    Terminal(TerminalEvent),
    System(SystemEvent),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum ChatEvent {
    ChatState {
        messages: Vec<crate::protocol::ChatMessage>,
    }, // Full State
    MessageDelta {
        id: String,
        chunk: String,
    }, // Streaming
    GenerationSignal {
        is_generating: bool,
    }, // Signal
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum EditorEvent {
    EditorState { active_file: Option<String> }, // Minimal state for now
    ContentDelta { file: String, patch: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum FileEvent {
    Content {
        path: String,
        data: String,
    },
    Written {
        path: String,
    },
    Listing {
        path: Option<String>,
        entries: Vec<FileEntry>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum WorkflowEvent {
    ApprovalRequested {
        batch_id: Uuid,
        items: Vec<String>, // Simplified for now, will hold structured actions
    },
    TaskCompleted {
        task_id: Uuid,
        success: bool,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum TerminalEvent {
    Output { id: String, data: String },
    Exit { id: String, code: i32 },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum SystemEvent {
    IntentFailed { intent_id: Uuid, error: BladeError },
    ProcessStarted { intent_id: Uuid },
    ProcessCompleted { intent_id: Uuid },
}

// ==============================================================================
// 4. Error Model
// ==============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "code", content = "details")]
pub enum BladeError {
    ValidationError { field: String, message: String },
    PermissionDenied,
    ResourceNotFound { id: String },
    Conflict { reason: String },
    Internal { trace_id: String, message: String },
    VersionMismatch { version: u16 },
    Timeout { timeout_ms: u64 },
    RateLimited { retry_after_ms: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_intent_envelope_serialization() {
        let id = Uuid::new_v4();
        let timestamp = 1700000000;
        let intent = BladeIntent::Chat(ChatIntent::SendMessage {
            content: "Hello World".to_string(),
            model: "gwt-5".to_string(),
            context: None,
        });

        let envelope = BladeIntentEnvelope {
            id,
            timestamp,
            intent: intent.clone(),
        };

        let json = serde_json::to_string(&envelope).expect("Failed to serialize intent envelope");
        let deserialized: BladeIntentEnvelope =
            serde_json::from_str(&json).expect("Failed to deserialize intent envelope");

        assert_eq!(envelope.id, deserialized.id);
        assert_eq!(envelope.timestamp, deserialized.timestamp);

        // Match intent variants manually since PartialEq is not derived for all
        if let BladeIntent::Chat(ChatIntent::SendMessage { content, .. }) = deserialized.intent {
            assert_eq!(content, "Hello World");
        } else {
            panic!("Deserialized intent has wrong type");
        }
    }

    #[test]
    fn test_event_envelope_serialization() {
        let id = Uuid::new_v4();
        let causality_id = Uuid::new_v4();
        let timestamp = 1700000001;
        let event = BladeEvent::Chat(ChatEvent::GenerationSignal {
            is_generating: true,
        });

        let envelope = BladeEventEnvelope {
            id,
            timestamp,
            causality_id: Some(causality_id),
            event: event.clone(),
        };

        let json = serde_json::to_string(&envelope).expect("Failed to serialize event envelope");
        let deserialized: BladeEventEnvelope =
            serde_json::from_str(&json).expect("Failed to deserialize event envelope");

        assert_eq!(envelope.id, deserialized.id);
        assert_eq!(envelope.causality_id, deserialized.causality_id);

        if let BladeEvent::Chat(ChatEvent::GenerationSignal { is_generating }) = deserialized.event
        {
            assert!(is_generating);
        } else {
            panic!("Deserialized event has wrong type");
        }
    }

    #[test]
    fn test_error_serialization() {
        let error = BladeError::Timeout { timeout_ms: 5000 };
        let json = serde_json::to_string(&error).expect("Failed to serialize error");

        // Verify structure: { "code": "Timeout", "details": { "timeout_ms": 5000 } }
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("Failed to parse error JSON");
        assert_eq!(value["code"], "Timeout");
        assert_eq!(value["details"]["timeout_ms"], 5000);
    }
}
