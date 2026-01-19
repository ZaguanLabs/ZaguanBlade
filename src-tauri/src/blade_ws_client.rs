use base64::prelude::*;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

/// WebSocket-based Blade Protocol v2 client
pub struct BladeWsClient {
    base_url: String,
    api_key: String,
    connection: Arc<Mutex<Option<WsConnection>>>,
}

struct WsConnection {
    tx: mpsc::UnboundedSender<WsMessage>,
    session_id: Option<String>,
}

enum WsMessage {
    Send(String),
    Ping,
    Close,
}

/// Todo item from zcoderd
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub content: String,
    #[serde(default)]
    pub active_form: Option<String>,
    pub status: String,
}

/// Events from the Blade Protocol WebSocket stream
#[derive(Debug, Clone)]
pub enum BladeWsEvent {
    Connected {
        user_id: String,
        server_version: String,
    },
    Session {
        session_id: String,
        model_id: String,
    },
    TextChunk(String),
    ReasoningChunk(String),
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    ToolResultAck {
        pending_count: i64,
    },
    TodoUpdated {
        todos: Vec<TodoItem>,
    },
    ChatDone {
        finish_reason: String,
    },
    Progress {
        message: String,
        stage: String,
        percent: u8,
    },
    Research {
        content: String,
    },
    GetConversationContext {
        request_id: String,
        session_id: String,
    },
    Error {
        code: String,
        message: String,
    },
    Disconnected,
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

/// WebSocket message types
#[derive(Debug, Serialize)]
struct WsBaseMessage {
    id: String,
    #[serde(rename = "type")]
    msg_type: String,
    timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<Value>,
}

#[derive(Debug, Serialize)]
struct AuthenticatePayload {
    api_key: String,
    client_name: String,
    client_version: String,
}
#[derive(Debug, Serialize)]
struct ChatRequestPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    model_id: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<WorkspaceInfo>,
    api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>, // Request ID embedded in payload for tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    storage_mode: Option<String>,
}

#[derive(Debug, Serialize)]
struct ToolResultPayload {
    session_id: String,
    tool_call_id: String,
    success: bool,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<String>, // API key for auth in multi-turn conversations
}

#[derive(Debug, Serialize)]
struct ConversationContextPayload {
    session_id: String,
    messages: Vec<serde_json::Value>,
}

/// Incoming WebSocket message
#[derive(Debug, Deserialize)]
struct WsIncomingMessage {
    #[allow(dead_code)]
    id: String,
    #[serde(rename = "type")]
    msg_type: String,
    #[allow(dead_code)]
    timestamp: i64,
    payload: Value,
}

impl BladeWsClient {
    /// Create a new WebSocket Blade Protocol client
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            base_url,
            api_key,
            connection: Arc::new(Mutex::new(None)),
        }
    }

    /// Connect to the WebSocket server and authenticate
    pub async fn connect(&self) -> Result<mpsc::UnboundedReceiver<BladeWsEvent>, String> {
        // Convert HTTP URL to WebSocket URL
        let ws_url = self
            .base_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        let url = format!("{}/v1/blade/v2", ws_url);

        eprintln!("[BLADE WS] Connecting to {}", url);

        // Connect to WebSocket
        let (ws_stream, _) = connect_async(&url)
            .await
            .map_err(|e| format!("WebSocket connection failed: {}", e))?;

        eprintln!("[BLADE WS] Connected successfully");

        let (mut write, mut read) = ws_stream.split();

        // Create channels
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

        // Store connection
        {
            let mut conn = self.connection.lock().await;
            *conn = Some(WsConnection {
                tx: msg_tx.clone(),
                session_id: None,
            });
        }

        // Spawn write task
        let _write_task = tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                match msg {
                    WsMessage::Send(text) => {
                        if let Err(e) = write.send(Message::Text(text)).await {
                            eprintln!("[BLADE WS] Write error: {}", e);
                            break;
                        }
                    }
                    WsMessage::Ping => {
                        if let Err(e) = write.send(Message::Ping(Vec::new())).await {
                            eprintln!("[BLADE WS] Ping error: {}", e);
                            break;
                        }
                    }
                    WsMessage::Close => {
                        let _ = write.close().await;
                        break;
                    }
                }
            }
        });

        // Heartbeat: send websocket ping periodically to keep connection alive
        // Use 10-second interval for aggressive keep-alive during long-running operations
        {
            let hb_tx = msg_tx.clone();
            tokio::spawn(async move {
                let interval = std::time::Duration::from_secs(10);
                loop {
                    tokio::time::sleep(interval).await;
                    if hb_tx.send(WsMessage::Ping).is_err() {
                        break;
                    }
                }
            });
        }

        // Spawn read task
        let event_tx_clone = event_tx.clone();
        let api_key = self.api_key.clone();
        let msg_tx_clone = msg_tx.clone();

        tokio::spawn(async move {
            // Send authentication message
            let auth_msg = WsBaseMessage {
                id: "auth-1".to_string(),
                msg_type: "authenticate".to_string(),
                timestamp: chrono::Utc::now().timestamp_millis(),
                payload: Some(
                    serde_json::to_value(AuthenticatePayload {
                        api_key,
                        client_name: "zblade".to_string(),
                        client_version: env!("CARGO_PKG_VERSION").to_string(),
                    })
                    .unwrap(),
                ),
            };

            let auth_json = serde_json::to_string(&auth_msg).unwrap();
            eprintln!("[BLADE WS] Sending authentication");

            if let Err(e) = msg_tx_clone.send(WsMessage::Send(auth_json)) {
                eprintln!("[BLADE WS] Failed to send auth: {}", e);
                let _ = event_tx_clone.send(BladeWsEvent::Error {
                    code: "auth_failed".to_string(),
                    message: "Failed to send authentication".to_string(),
                });
                return;
            }

            // Read messages
            while let Some(msg_result) = read.next().await {
                match msg_result {
                    Ok(Message::Text(text)) => {
                        eprintln!("[BLADE WS] Received: {}", text);
                        if let Err(e) = Self::parse_message(&text, &event_tx_clone) {
                            eprintln!("[BLADE WS] Parse error: {}", e);
                        }
                    }
                    Ok(Message::Close(_)) => {
                        eprintln!("[BLADE WS] Connection closed by server");
                        let _ = event_tx_clone.send(BladeWsEvent::Disconnected);
                        break;
                    }
                    Ok(Message::Ping(_)) => {
                        // Pong is handled automatically by tungstenite
                    }
                    Err(e) => {
                        eprintln!("[BLADE WS] Read error: {}", e);
                        // Treat connection reset as a disconnect so upstream can finish gracefully
                        let msg = e.to_string();
                        if msg.contains("Connection reset by peer") {
                            let _ = event_tx_clone.send(BladeWsEvent::Disconnected);
                        } else {
                            let _ = event_tx_clone.send(BladeWsEvent::Error {
                                code: "read_error".to_string(),
                                message: format!("Read error: {}", msg),
                            });
                        }
                        break;
                    }
                    _ => {}
                }
            }

            let _ = event_tx_clone.send(BladeWsEvent::Disconnected);
        });

        Ok(event_rx)
    }

    /// Send a chat message
    pub async fn send_message(
        &self,
        session_id: Option<String>,
        model_id: String,
        message: String,
        workspace: Option<WorkspaceInfo>,
    ) -> Result<(), String> {
        self.send_message_with_storage_mode(session_id, model_id, message, workspace, None)
            .await
    }

    /// Send a chat message with explicit storage mode (RFC-002)
    pub async fn send_message_with_storage_mode(
        &self,
        session_id: Option<String>,
        model_id: String,
        message: String,
        workspace: Option<WorkspaceInfo>,
        storage_mode: Option<String>,
    ) -> Result<(), String> {
        let conn = self.connection.lock().await;
        let conn = conn.as_ref().ok_or("Not connected")?;

        let msg_id = format!("chat-{}", chrono::Utc::now().timestamp_millis());

        let payload = ChatRequestPayload {
            session_id,
            model_id,
            message,
            workspace,
            api_key: self.api_key.clone(),
            request_id: Some(msg_id.clone()),
            storage_mode,
        };

        let msg = WsBaseMessage {
            id: msg_id.clone(),
            msg_type: "chat_request".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(serde_json::to_value(payload).unwrap()),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send message: {}", e))?;

        Ok(())
    }

    /// Send a tool execution result
    pub async fn send_tool_result(
        &self,
        session_id: String,
        tool_call_id: String,
        result: ToolResult,
    ) -> Result<(), String> {
        let conn = self.connection.lock().await;
        let conn = conn.as_ref().ok_or("Not connected")?;

        let payload = ToolResultPayload {
            session_id,
            tool_call_id,
            success: result.success,
            content: result.content,
            error: result.error,
            api_key: Some(self.api_key.clone()), // Include API key for multi-turn auth
        };

        let msg = WsBaseMessage {
            id: format!("tool-result-{}", chrono::Utc::now().timestamp_millis()),
            msg_type: "tool_result".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(serde_json::to_value(payload).unwrap()),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send tool result: {}", e))?;

        Ok(())
    }

    /// Send conversation context in response to get_conversation_context request (RFC-002)
    pub async fn send_conversation_context(
        &self,
        request_id: String,
        session_id: String,
        messages: Vec<serde_json::Value>,
    ) -> Result<(), String> {
        let conn = self.connection.lock().await;
        let conn = conn.as_ref().ok_or("Not connected")?;

        let payload = ConversationContextPayload {
            session_id: session_id.clone(),
            messages,
        };

        eprintln!(
            "[BLADE WS] Sending conversation context response: id={} session={}",
            request_id, session_id
        );

        let msg = WsBaseMessage {
            id: request_id, // Must match the request ID for server correlation
            msg_type: "conversation_context".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(serde_json::to_value(payload).unwrap()),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send conversation context: {}", e))?;

        Ok(())
    }

    /// Update stored session ID
    pub async fn set_session_id(&self, session_id: String) {
        let mut conn = self.connection.lock().await;
        if let Some(ref mut c) = *conn {
            c.session_id = Some(session_id);
        }
    }

    /// Get stored session ID
    pub async fn get_session_id(&self) -> Option<String> {
        let conn = self.connection.lock().await;
        conn.as_ref().and_then(|c| c.session_id.clone())
    }

    /// Close the WebSocket connection
    pub async fn close(&self) {
        let conn = self.connection.lock().await;
        if let Some(ref c) = *conn {
            let _ = c.tx.send(WsMessage::Close);
        }
    }

    /// Parse incoming WebSocket message
    fn parse_message(text: &str, tx: &mpsc::UnboundedSender<BladeWsEvent>) -> Result<(), String> {
        let msg: WsIncomingMessage =
            serde_json::from_str(text).map_err(|e| format!("JSON parse error: {}", e))?;

        match msg.msg_type.as_str() {
            "authenticated" => {
                let user_id = msg
                    .payload
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let server_version = msg
                    .payload
                    .get("server_version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                eprintln!("[BLADE WS] Authenticated as {}", user_id);
                let _ = tx.send(BladeWsEvent::Connected {
                    user_id,
                    server_version,
                });
            }
            "session_created" => {
                let session_id = msg
                    .payload
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let model_id = msg
                    .payload
                    .get("model_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                eprintln!("[BLADE WS] Session created: {}", session_id);
                let _ = tx.send(BladeWsEvent::Session {
                    session_id,
                    model_id,
                });
            }
            "text_chunk" => {
                if let Some(content) = msg.payload.get("content").and_then(|v| v.as_str()) {
                    let _ = tx.send(BladeWsEvent::TextChunk(content.to_string()));
                }
            }
            "reasoning_chunk" => {
                if let Some(content) = msg.payload.get("content").and_then(|v| v.as_str()) {
                    let _ = tx.send(BladeWsEvent::ReasoningChunk(content.to_string()));
                }
            }
            "tool_call" => {
                let id = msg
                    .payload
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = msg
                    .payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                // Handle arguments: if it's a string (which it is now from server), parse it to JSON Value
                // This ensures ChatManager's to_string() produces clean JSON, not an escaped string
                let raw_args = msg.payload.get("arguments");
                let arguments = if let Some(str_args) = raw_args.and_then(|v| v.as_str()) {
                    eprintln!("[BLADE WS] Parsing string arguments: {}", str_args);
                    match serde_json::from_str::<Value>(str_args) {
                        Ok(v) => {
                            eprintln!("[BLADE WS] Successfully parsed arguments to JSON object");
                            v
                        }
                        Err(e) => {
                            eprintln!("[BLADE WS] Failed to parse arguments as JSON: {}", e);
                            Value::String(str_args.to_string())
                        }
                    }
                } else {
                    eprintln!("[BLADE WS] Arguments are not a string, using raw value");
                    raw_args.cloned().unwrap_or(Value::Null)
                };

                eprintln!("[BLADE WS] Tool call: {} ({})", name, id);
                let _ = tx.send(BladeWsEvent::ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
            "todo_updated" => {
                // Parse todos array from payload
                if let Some(todos_value) = msg.payload.get("todos") {
                    match serde_json::from_value::<Vec<TodoItem>>(todos_value.clone()) {
                        Ok(todos) => {
                            eprintln!("[BLADE WS] Todo updated: {} items", todos.len());
                            match tx.send(BladeWsEvent::TodoUpdated { todos }) {
                                Ok(_) => eprintln!("[BLADE WS] TodoUpdated event sent to channel"),
                                Err(e) => eprintln!(
                                    "[BLADE WS] Failed to send TodoUpdated to channel: {}",
                                    e
                                ),
                            }
                        }
                        Err(e) => {
                            eprintln!("[BLADE WS] Failed to parse todos: {}", e);
                        }
                    }
                }
            }
            "chat_done" => {
                let finish_reason = msg
                    .payload
                    .get("finish_reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("stop")
                    .to_string();

                eprintln!("[BLADE WS] Chat done: {}", finish_reason);
                let _ = tx.send(BladeWsEvent::ChatDone { finish_reason });
            }
            "error" => {
                let code = msg
                    .payload
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let message = msg
                    .payload
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                eprintln!("[BLADE WS] Error: {} - {}", code, message);
                let _ = tx.send(BladeWsEvent::Error { code, message });
            }
            "tool_result_ack" => {
                // Tool result acknowledgment - zcoderd received our result but is waiting for more
                let pending_count = msg
                    .payload
                    .get("pending_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                eprintln!(
                    "[BLADE WS] Tool result acknowledged, waiting for {} more result(s)",
                    pending_count
                );
                let _ = tx.send(BladeWsEvent::ToolResultAck { pending_count });
            }
            "progress" => {
                let message = msg
                    .payload
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let stage = msg
                    .payload
                    .get("stage")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let percent = msg
                    .payload
                    .get("percent")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u8;

                eprintln!("[BLADE WS] Progress: {} ({}%)", message, percent);
                let _ = tx.send(BladeWsEvent::Progress {
                    message,
                    stage,
                    percent,
                });
            }
            "research" => {
                let content = msg
                    .payload
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                eprintln!(
                    "[BLADE WS] Research result received ({} chars)",
                    content.len()
                );
                let _ = tx.send(BladeWsEvent::Research { content });
            }
            "get_conversation_context" => {
                let session_id = if let Some(payload_str) = msg.payload.as_str() {
                    // It's a string, likely base64 encoded JSON
                    match base64::prelude::BASE64_STANDARD.decode(payload_str) {
                        Ok(decoded_bytes) => {
                            match String::from_utf8(decoded_bytes) {
                                Ok(json_str) => match serde_json::from_str::<Value>(&json_str) {
                                    Ok(json_obj) => json_obj
                                        .get("session_id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    Err(e) => {
                                        eprintln!("[BLADE WS] Failed to parse decoded payload as JSON: {}", e);
                                        "".to_string()
                                    }
                                },
                                Err(e) => {
                                    eprintln!("[BLADE WS] Failed to convert decoded payload to string: {}", e);
                                    "".to_string()
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[BLADE WS] Failed to decode base64 payload: {}", e);
                            "".to_string()
                        }
                    }
                } else {
                    // It's a JSON object directly
                    msg.payload
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                };

                eprintln!(
                    "[BLADE WS] Server requesting conversation context for session: {}",
                    session_id
                );
                let _ = tx.send(BladeWsEvent::GetConversationContext {
                    request_id: msg.id.clone(),
                    session_id,
                });
            }
            _ => {
                eprintln!("[BLADE WS] Unknown message type: {}", msg.msg_type);
            }
        }

        Ok(())
    }
}
