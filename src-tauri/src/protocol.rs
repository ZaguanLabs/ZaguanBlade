use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatImage {
    pub data: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Option<String>,
    pub role: ChatRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ChatImage>>,
    pub reasoning: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<ProgressInfo>,
    // Separate content before and after tool execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_before_tools: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_after_tools: Option<String>,
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: String) -> Self {
        Self {
            id: Some(Uuid::new_v4().to_string()),
            role,
            content,
            images: None,
            reasoning: None,
            tool_call_id: None,
            tool_calls: None,
            progress: None,
            content_before_tools: None,
            content_after_tools: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressInfo {
    pub message: String,
    pub stage: String,
    pub percent: i32,
}

/// Todo item for task progress (from zcoderd)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub active_form: Option<String>,
    pub status: String,
}

pub enum ChatEvent {
    Session {
        session_id: String,
        model: String,
    },
    Chunk(String),
    ReasoningChunk(String),
    Research {
        content: String,
        suggested_name: String,
    },
    ToolCalls(Vec<ToolCall>),
    TodoUpdated(Vec<TodoItem>),
    Progress {
        message: String,
        stage: String,
        percent: i32,
    },
    ToolActivity(ToolActivityPayload),
    Done,
    Error(String),
    /// Context length exceeded error with recovery information (RFC: Context Length Recovery)
    ContextLengthExceeded {
        message: String,
        token_count: Option<u64>,
        max_tokens: Option<u64>,
        excess: Option<u64>,
        recoverable: bool,
        recovery_hint: Option<String>,
    },
    /// Message too large error - response exceeded WebSocket message size limit
    MessageTooLarge {
        message: String,
        recovery_hint: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolActivityPayload {
    pub tool_name: String,
    pub file_path: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub typ: String,
    pub function: ToolFunction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ToolCallDelta {
    #[serde(default)]
    pub index: usize,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub typ: Option<String>,
    pub function: Option<ToolFunctionDelta>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ToolFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct OpenAiMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChoice {
    pub delta: StreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamDelta {
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallDelta>,
}
