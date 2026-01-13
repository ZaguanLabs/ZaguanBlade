use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Blade Protocol client for communicating with zcoderd
pub struct BladeClient {
    base_url: String,
    http_client: reqwest::Client,
    api_key: String,
}

/// Events from the Blade Protocol SSE stream
#[derive(Debug, Clone)]
pub enum BladeEvent {
    Session {
        session_id: String,
        model: String,
    },
    Text(String),
    Research(String),
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    Progress {
        message: String,
        stage: String,
        percent: i32,
    },
    Compression {
        triggered: bool,
        reason: String,
    },
    Staleness {
        stale_files: Vec<Value>,
    },
    Done {
        message_count: i32,
        tokens_used: i32,
    },
    Error {
        code: String,
        message: String,
        details: String,
    },
}

/// Workspace information sent to zcoderd
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInfo {
    pub root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_position: Option<CursorPosition>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub open_files: Vec<OpenFileInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CursorPosition {
    pub line: i32,
    pub column: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenFileInfo {
    pub path: String,
    pub hash: String,
    pub is_active: bool,
    pub is_modified: bool,
}

/// Tool execution result
#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    pub success: bool,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Blade Protocol request for user messages
#[derive(Debug, Serialize)]
struct BladeMessageRequest {
    session_id: Option<String>,
    #[serde(rename = "type")]
    request_type: String,
    model_id: String,
    content: String,
    workspace: WorkspaceInfo,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    file_hashes: HashMap<String, String>,
}

/// Blade Protocol request for tool results
#[derive(Debug, Serialize)]
struct BladeToolResultRequest {
    session_id: String,
    #[serde(rename = "type")]
    request_type: String,
    tool_call_id: String,
    result: ToolResult,
}

/// SSE event structure from zcoderd
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SSEEvent {
    #[serde(default)]
    event: String,
    data: Value,
}

impl BladeClient {
    /// Create a new Blade Protocol client
    pub fn new(base_url: String, http_client: reqwest::Client, api_key: String) -> Self {
        Self {
            base_url,
            http_client,
            api_key,
        }
    }

    /// Send a user message and start streaming response
    pub async fn send_message(
        &self,
        session_id: Option<String>,
        model_id: String,
        content: String,
        workspace: WorkspaceInfo,
        file_hashes: HashMap<String, String>,
    ) -> Result<mpsc::UnboundedReceiver<BladeEvent>, String> {
        let request = BladeMessageRequest {
            session_id,
            request_type: "message".to_string(),
            model_id,
            content,
            workspace,
            file_hashes,
        };

        self.send_blade_request(request).await
    }

    /// Send a tool execution result
    pub async fn send_tool_result(
        &self,
        session_id: String,
        tool_call_id: String,
        result: ToolResult,
    ) -> Result<mpsc::UnboundedReceiver<BladeEvent>, String> {
        let request = BladeToolResultRequest {
            session_id,
            request_type: "tool_result".to_string(),
            tool_call_id,
            result,
        };

        self.send_blade_request(request).await
    }

    /// Internal method to send any Blade Protocol request
    async fn send_blade_request<T: Serialize>(
        &self,
        request: T,
    ) -> Result<mpsc::UnboundedReceiver<BladeEvent>, String> {
        let url = format!("{}/v1/blade/chat", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("Blade Protocol error {}: {}", status, text));
        }

        // Create channel for events (unbounded for simplicity)
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn task to parse SSE stream
        tokio::spawn(async move {
            if let Err(e) = Self::parse_sse_stream(response, tx.clone()).await {
                let _ = tx.send(BladeEvent::Error {
                    code: "stream_error".to_string(),
                    message: e,
                    details: String::new(),
                });
            }
        });

        Ok(rx)
    }

    /// Parse SSE stream from zcoderd
    async fn parse_sse_stream(
        response: reqwest::Response,
        tx: mpsc::UnboundedSender<BladeEvent>,
    ) -> Result<(), String> {
        eprintln!("[BLADE CLIENT] Starting SSE stream parsing");
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut current_event_type: Option<String> = None;

        while let Some(item) = stream.next().await {
            let chunk = item.map_err(|e| format!("Stream error: {}", e))?;

            let text = std::str::from_utf8(&chunk).map_err(|e| format!("UTF-8 error: {}", e))?;

            eprintln!("[BLADE CLIENT] Received chunk: {}", text);
            buffer.push_str(text);

            // Process complete lines
            while let Some(idx) = buffer.find('\n') {
                let line = buffer[..idx].trim().to_string();
                buffer = buffer[idx + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                eprintln!("[BLADE CLIENT] Processing line: {}", line);

                // Parse SSE format: "event: type" or "data: json"
                if let Some(event_type) = line.strip_prefix("event: ") {
                    eprintln!("[BLADE CLIENT] Event type: {}", event_type);
                    current_event_type = Some(event_type.to_string());
                    continue;
                }

                if let Some(data_str) = line.strip_prefix("data: ") {
                    eprintln!(
                        "[BLADE CLIENT] Parsing data with event type: {:?}",
                        current_event_type
                    );
                    if let Err(e) =
                        Self::parse_event_data(data_str, current_event_type.as_deref(), &tx)
                    {
                        eprintln!("[BLADE] Failed to parse event: {}", e);
                    }
                    current_event_type = None; // Reset after processing
                }
            }
        }

        eprintln!("[BLADE CLIENT] SSE stream ended");

        Ok(())
    }

    /// Parse event data and send to channel
    fn parse_event_data(
        data: &str,
        event_type: Option<&str>,
        tx: &mpsc::UnboundedSender<BladeEvent>,
    ) -> Result<(), String> {
        let value: Value =
            serde_json::from_str(data).map_err(|e| format!("JSON parse error: {}", e))?;

        eprintln!("[BLADE CLIENT] Parsed JSON: {:?}", value);

        // Determine event type from the data structure
        // The event type is sent separately in SSE, but we can infer from data
        if let Some(session_id) = value.get("session_id").and_then(|v| v.as_str()) {
            if let Some(model) = value.get("model").and_then(|v| v.as_str()) {
                eprintln!("[BLADE CLIENT] Sending Session event");
                let _ = tx.send(BladeEvent::Session {
                    session_id: session_id.to_string(),
                    model: model.to_string(),
                });
                return Ok(());
            }
        }

        if let Some(content) = value.get("content").and_then(|v| v.as_str()) {
            // Check if this is a research event
            if event_type == Some("research") {
                eprintln!("[BLADE CLIENT] Sending Research event");
                let _ = tx.send(BladeEvent::Research(content.to_string()));
            } else {
                eprintln!("[BLADE CLIENT] Sending Text event: {}", content);
                let _ = tx.send(BladeEvent::Text(content.to_string()));
            }
            return Ok(());
        }

        if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
            if let Some(name) = value.get("name").and_then(|v| v.as_str()) {
                let arguments = value.get("arguments").cloned().unwrap_or(Value::Null);
                let _ = tx.send(BladeEvent::ToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments,
                });
                return Ok(());
            }
        }

        if let Some(message) = value.get("message").and_then(|v| v.as_str()) {
            if let Some(stage) = value.get("stage").and_then(|v| v.as_str()) {
                let percent = value.get("percent").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let _ = tx.send(BladeEvent::Progress {
                    message: message.to_string(),
                    stage: stage.to_string(),
                    percent,
                });
                return Ok(());
            }
        }

        if let Some(message_count) = value.get("message_count").and_then(|v| v.as_i64()) {
            let tokens_used = value
                .get("tokens_used")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let _ = tx.send(BladeEvent::Done {
                message_count: message_count as i32,
                tokens_used,
            });
            return Ok(());
        }

        if let Some(code) = value.get("code").and_then(|v| v.as_str()) {
            let message = value.get("message").and_then(|v| v.as_str()).unwrap_or("");
            let details = value.get("details").and_then(|v| v.as_str()).unwrap_or("");
            let _ = tx.send(BladeEvent::Error {
                code: code.to_string(),
                message: message.to_string(),
                details: details.to_string(),
            });
            return Ok(());
        }

        // Unknown event type - ignore
        Ok(())
    }
}
