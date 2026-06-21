use crate::blade_protocol::ContextPackPayload;
use crate::environment::EnvironmentInfo;
use crate::protocol::CognitiveInterruptPayload;
use futures_util::{SinkExt, StreamExt};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        client::IntoClientRequest,
        http::{header::AUTHORIZATION, Request},
        protocol::{Message, WebSocketConfig},
    },
};

lazy_static! {
    static ref FILE_PATH_REGEXES: Vec<Regex> = vec![
        Regex::new(r#""path"\s*:\s*"([^"]*)""#).unwrap(),
        Regex::new(r#""file_path"\s*:\s*"([^"]*)""#).unwrap(),
        Regex::new(r#""target_file"\s*:\s*"([^"]*)"#).unwrap(),
        Regex::new(r#""absolute_path"\s*:\s*"([^"]*)"#).unwrap(),
        Regex::new(r#""file"\s*:\s*"([^"]*)"#).unwrap(),
    ];
}

/// WebSocket-based Blade Protocol v3 client
#[derive(Clone)]
pub struct BladeWsClient {
    base_url: String,
    api_key: String,
    connection: Arc<Mutex<Option<WsConnection>>>,
    connected_role: Arc<RwLock<Option<crate::blade_endpoint::BladeEndpointRole>>>,
}

struct WsConnection {
    tx: mpsc::UnboundedSender<WsMessage>,
    session_id: Option<String>,
    active_request_id: Option<String>,
    storage_mode: Option<String>,
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
    HistoryListResponse {
        request_id: String,
        conversations: Vec<crate::blade_protocol::ConversationSummary>,
    },
    HistoryDetailResponse {
        request_id: String,
        conversation: crate::blade_protocol::FullConversation,
    },
    ZlpResponse {
        request_id: String,
        response: Value,
    },
    Session {
        session_id: String,
        model_id: String,
        planning_mode: Option<bool>,
        runtime_mode: Option<String>,
        mode_source: Option<String>,
    },
    TextChunk {
        request_id: Option<String>,
        content: String,
        output_index: Option<i64>,
        phase: Option<String>,
    },
    ReasoningChunk {
        request_id: Option<String>,
        content: String,
        output_index: Option<i64>,
        phase: Option<String>,
    },
    ToolCall {
        request_id: Option<String>,
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
    ApprovalRequest {
        request_id: Option<String>,
        session_id: String,
        approval_id: String,
        tool_call_id: String,
        tool_name: String,
        arguments: Value,
        source: Option<String>,
        rule_name: Option<String>,
        message: Option<String>,
        decision: Option<String>,
    },
    ChatDone {
        request_id: Option<String>,
        finish_reason: String,
        recoverable: Option<bool>,
    },
    RequestCancelled {
        request_id: String,
        session_id: Option<String>,
        reason: Option<String>,
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
        request_id: Option<String>,
        /// Standardized error type (context_length_exceeded, rate_limit_error, etc.)
        error_type: String,
        /// Machine-readable error code
        code: String,
        /// Full error message (may contain provider-specific details)
        message: String,
        /// Current token count (0 if unknown)
        token_count: Option<u64>,
        /// Model's max tokens (0 if unknown)
        max_tokens: Option<u64>,
        /// Excess tokens over limit
        excess: Option<u64>,
        /// Can the client retry?
        recoverable: Option<bool>,
        /// User-friendly guidance
        recovery_hint: Option<String>,
    },
    Disconnected,
    ToolActivity {
        tool_name: String,
        file_path: String,
        action: String,
    },
    CognitiveInterrupt(CognitiveInterruptPayload),
    /// Tool progress - streaming partial arguments as tool call is being generated
    ToolProgress {
        tool_call_id: String,
        tool_name: String,
        file_path: Option<String>,
    },
    /// Context pack request from zcoderd — query local semantic/symbol/docs index
    ContextPackRequest {
        id: String,
        query: String,
        queries: Vec<String>,
        intent: Option<String>,
        max_results: Option<usize>,
        include_tests: Option<bool>,
        include_docs: Option<bool>,
        include_memory: Option<bool>,
        include_project_index_min: Option<bool>,
        response_type: String,
    },
    /// Context pack response destined for zcoderd
    ContextPackResponse {
        id: String,
        payload: ContextPackPayload,
    },
    /// Pairing token received from relay server
    PairingTokenResponse {
        token: String,
        bot_username: String,
        expires_in: u64,
    },
    /// Remote control connection status update from relay
    RemoteControlConnected {
        telegram_username: String,
        telegram_chat_id: String,
    },
    /// Remote control disconnected
    RemoteControlDisconnected,
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

#[derive(Debug, Clone, Serialize)]
pub struct LocalConversationState {
    pub message_count: usize,
    pub fingerprint: String,
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
    client: String,
    version: String,
    client_name: String,
    client_version: String,
    protocol_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    environment: Option<EnvironmentInfo>,
}

#[derive(Debug, Serialize)]
struct ChatRequestPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    model_id: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<crate::protocol::ChatImage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<WorkspaceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    storage_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    planning_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_conversation_state: Option<LocalConversationState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parallel_tool_calls: Option<bool>,
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
    model_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApprovalResponsePayload {
    session_id: String,
    approval_id: String,
    tool_call_id: String,
    approved: bool,
}

#[derive(Debug, Serialize)]
struct CancelRequestPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    reason: String,
}

#[derive(Debug, Serialize)]
struct DisconnectPayload {
    session_id: String,
}

#[derive(Debug, Serialize)]
struct ConversationContextPayload {
    session_id: String,
    messages: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct HistoryListRequestPayload {
    project_id: String,
}

#[derive(Debug, Serialize)]
struct HistoryDetailRequestPayload {
    session_id: String,
}

/// Incoming WebSocket message
#[derive(Debug, Deserialize)]
struct WsIncomingMessage {
    #[serde(default)]
    id: String,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(rename = "type")]
    msg_type: String,
    #[allow(dead_code)]
    #[serde(default)]
    timestamp: i64,
    #[serde(default)]
    payload: Value,
}

impl BladeWsClient {
    fn websocket_endpoint_url(base_url: &str) -> String {
        let ws_url = crate::blade_endpoint::to_websocket_url(base_url);
        format!("{}/v1/blade/v2", ws_url)
    }

    fn websocket_request(base_url: &str, api_key: &str) -> Result<Request<()>, String> {
        let url = Self::websocket_endpoint_url(base_url);
        let mut request = url
            .into_client_request()
            .map_err(|e| format!("failed to build WebSocket request: {}", e))?;

        let trimmed_key = api_key.trim();
        if !trimmed_key.is_empty() {
            let header_value = format!("Bearer {}", trimmed_key)
                .parse()
                .map_err(|e| format!("invalid Authorization header: {}", e))?;
            request.headers_mut().insert(AUTHORIZATION, header_value);
        }

        Ok(request)
    }

    fn correlation_id_for_message(msg: &WsIncomingMessage) -> String {
        msg.request_id
            .clone()
            .or_else(|| {
                msg.payload
                    .get("request_id")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
            })
            .or_else(|| {
                msg.payload
                    .get("original_request_id")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| msg.id.clone())
    }

    fn request_id_for_message(msg: &WsIncomingMessage) -> Option<String> {
        msg.request_id
            .clone()
            .or_else(|| {
                msg.payload
                    .get("request_id")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
            })
            .or_else(|| {
                msg.payload
                    .get("original_request_id")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
            })
    }

    /// Create a new WebSocket Blade Protocol client
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            base_url,
            api_key,
            connection: Arc::new(Mutex::new(None)),
            connected_role: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn connected_role(&self) -> Option<crate::blade_endpoint::BladeEndpointRole> {
        *self.connected_role.read().await
    }

    /// Connect to the WebSocket server and authenticate with retry logic
    /// If `workspace_path` is provided, it overrides `working_dir` in the environment info.
    pub async fn connect(
        &self,
        workspace_path: Option<String>,
    ) -> Result<mpsc::UnboundedReceiver<BladeWsEvent>, String> {
        let mut retry_count = 0;
        let max_retries = 8; // ~2 minutes total wait time with exponential backoff
        let connect_timeout = Duration::from_secs(10);

        let (ws_stream, connected_role) = 'connect_loop: loop {
            let candidates = crate::blade_endpoint::connection_candidates(&self.base_url).await;
            let mut last_error = "no Blade endpoints configured".to_string();

            for endpoint in candidates {
                // Intentionally quiet: avoid high-frequency websocket connect logs.
                let request = match Self::websocket_request(&endpoint.base_url, &self.api_key) {
                    Ok(request) => request,
                    Err(error) => {
                        last_error = format!("{}: {}", endpoint.base_url, error);
                        crate::blade_endpoint::mark_failure(&endpoint, &self.api_key).await;
                        continue;
                    }
                };

                // Configure WebSocket with larger message size limit (64MB instead of default 16MB)
                // This prevents "Space limit exceeded" errors for large tool results
                let mut ws_config = WebSocketConfig::default();
                ws_config.max_message_size = Some(64 * 1024 * 1024); // 64MB
                ws_config.max_frame_size = Some(64 * 1024 * 1024); // 64MB per frame

                match tokio::time::timeout(
                    connect_timeout,
                    connect_async_with_config(request, Some(ws_config), false),
                )
                .await
                {
                    Ok(Ok((stream, _))) => {
                        crate::blade_endpoint::mark_success(&endpoint).await;
                        if endpoint.role == crate::blade_endpoint::BladeEndpointRole::Backup {
                            crate::blade_endpoint::start_primary_monitor(self.api_key.clone());
                        }
                        break 'connect_loop (stream, endpoint.role);
                    }
                    Ok(Err(error)) => {
                        last_error = format!("{}: {}", endpoint.base_url, error);
                        crate::blade_endpoint::mark_failure(&endpoint, &self.api_key).await;
                    }
                    Err(_) => {
                        last_error = format!(
                            "{}: connection timed out after {:?}",
                            endpoint.base_url, connect_timeout
                        );
                        crate::blade_endpoint::mark_failure(&endpoint, &self.api_key).await;
                    }
                }
            }

            retry_count += 1;
            if retry_count > max_retries {
                return Err(format!(
                    "WebSocket connection failed after {} retries: {}",
                    max_retries, last_error
                ));
            }

            // Exponential backoff: 500ms, 1s, 2s, 4s, 8s, 16s, 32s, 64s
            let delay_ms = 500 * (1 << (retry_count - 1));
            let delay = Duration::from_millis(delay_ms);
            tokio::time::sleep(delay).await;
        };
        {
            let mut role = self.connected_role.write().await;
            *role = Some(connected_role);
        }

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
                active_request_id: None,
                storage_mode: None,
            });
        }

        // Spawn write task
        let _write_task = tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                match msg {
                    WsMessage::Send(text) => {
                        if let Err(_e) = write.send(Message::Text(text.into())).await {
                            break;
                        }

                        // CRITICAL: flush() is required! send() only queues the message
                        if let Err(_e) = futures_util::SinkExt::flush(&mut write).await {
                            break;
                        }
                    }
                    WsMessage::Ping => {
                        if let Err(_e) = write.send(Message::Ping(Vec::new().into())).await {
                            // eprintln!("[BLADE WS] Ping error: {}", _e);
                            break;
                        }
                        if let Err(_e) = futures_util::SinkExt::flush(&mut write).await {
                            // eprintln!("[BLADE WS] Ping flush error: {}", _e);
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
        let msg_tx_clone = msg_tx.clone();

        tokio::spawn(async move {
            // Collect environment information for the system prompt
            let mut environment = tokio::task::spawn_blocking(EnvironmentInfo::collect)
                .await
                .unwrap_or_else(|error| {
                    eprintln!("[BLADE WS] Environment collection task failed: {}", error);
                    EnvironmentInfo::default()
                });
            // Override working_dir with the actual workspace path if available.
            // env::current_dir() returns the AppImage mount point, not the user's workspace.
            if let Some(ref ws_path) = workspace_path {
                environment.working_dir = Some(ws_path.clone());
            }
            // eprintln!("[BLADE WS] Environment: os={}, arch={:?}, shell={:?}",
            //     environment.os, environment.arch, environment.shell);

            // Send authentication message
            let mut capabilities = serde_json::Map::new();
            capabilities.insert(
                "context_pack_request".to_string(),
                serde_json::Value::Bool(true),
            );

            let auth_msg = WsBaseMessage {
                id: "auth-1".to_string(),
                msg_type: "authenticate".to_string(),
                timestamp: chrono::Utc::now().timestamp_millis(),
                payload: Some(
                    serde_json::to_value(AuthenticatePayload {
                        client: "zblade".to_string(),
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        client_name: "zblade".to_string(),
                        client_version: env!("CARGO_PKG_VERSION").to_string(),
                        protocol_version: 3,
                        capabilities: Some(serde_json::Value::Object(capabilities)),
                        environment: Some(environment),
                    })
                    .unwrap(),
                ),
            };

            let auth_json = serde_json::to_string(&auth_msg).unwrap();
            // eprintln!("[BLADE WS] Sending authentication");

            if let Err(_e) = msg_tx_clone.send(WsMessage::Send(auth_json)) {
                // eprintln!("[BLADE WS] Failed to send auth: {}", _e);
                let _ = event_tx_clone.send(BladeWsEvent::Error {
                    request_id: None,
                    error_type: "authentication_error".to_string(),
                    code: "auth_failed".to_string(),
                    message: "Failed to send authentication".to_string(),
                    token_count: None,
                    max_tokens: None,
                    excess: None,
                    recoverable: Some(false),
                    recovery_hint: Some("Check your API key and try again".to_string()),
                });
                return;
            }

            let mut connected_event_sent = false;

            // Read messages
            while let Some(msg_result) = read.next().await {
                match msg_result {
                    Ok(Message::Text(text)) => {
                        if text.len() > 500 {
                            // eprintln!("[BLADE WS] Received: {}... ({} bytes)", &text[..200], text.len());
                        } else {
                            // eprintln!("[BLADE WS] Received: {}", text);
                        }
                        if let Err(_e) = Self::parse_message_with_auth_gate(
                            text.as_ref(),
                            &event_tx_clone,
                            &mut connected_event_sent,
                        ) {
                            // eprintln!("[BLADE WS] Parse error: {}", _e);
                        }
                    }
                    Ok(Message::Close(_)) => {
                        // eprintln!("[BLADE WS] Connection closed by server");
                        let _ = event_tx_clone.send(BladeWsEvent::Disconnected);
                        break;
                    }
                    Ok(Message::Ping(_)) => {
                        // Pong is handled automatically by tungstenite
                    }
                    Err(e) => {
                        // eprintln!("[BLADE WS] Read error: {}", e);
                        let msg = e.to_string();

                        // Handle specific error types with appropriate recovery hints
                        if msg.contains("Connection reset by peer") {
                            // Treat connection reset as a disconnect so upstream can finish gracefully
                            let _ = event_tx_clone.send(BladeWsEvent::Disconnected);
                        } else if msg.contains("Space limit exceeded")
                            || msg.contains("Message too long")
                        {
                            // Message size limit exceeded - tell the model to use smaller responses
                            // eprintln!("[BLADE WS] Message size limit exceeded, sending recoverable error");
                            let _ = event_tx_clone.send(BladeWsEvent::Error {
                                request_id: None,
                                error_type: "message_too_large".to_string(),
                                code: "size_limit_exceeded".to_string(),
                                message: "The response was too large to process. Please break your response into smaller parts or use more concise output.".to_string(),
                                token_count: None,
                                max_tokens: None,
                                excess: None,
                                recoverable: Some(true),
                                recovery_hint: Some("Your previous response exceeded the message size limit. Please retry with a more concise approach: use smaller code blocks, avoid outputting entire files, and break large changes into multiple smaller tool calls.".to_string()),
                            });
                        } else {
                            let _ = event_tx_clone.send(BladeWsEvent::Error {
                                request_id: None,
                                error_type: "unknown_error".to_string(),
                                code: "read_error".to_string(),
                                message: format!("Read error: {}", msg),
                                token_count: None,
                                max_tokens: None,
                                excess: None,
                                recoverable: Some(true),
                                recovery_hint: Some("Connection error. Try again.".to_string()),
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
        images: Option<Vec<crate::protocol::ChatImage>>,
        workspace: Option<WorkspaceInfo>,
    ) -> Result<String, String> {
        self.send_message_with_storage_mode(
            session_id, model_id, message, images, workspace, None, None, None, None, None, None,
            None, None,
        )
        .await
    }

    pub fn new_chat_request_id() -> String {
        format!("chat-{}", uuid::Uuid::new_v4())
    }

    /// Send a chat message with explicit storage mode (RFC-002)
    pub async fn send_message_with_storage_mode(
        &self,
        session_id: Option<String>,
        model_id: String,
        message: String,
        images: Option<Vec<crate::protocol::ChatImage>>,
        workspace: Option<WorkspaceInfo>,
        storage_mode: Option<String>,
        mode: Option<String>,
        local_conversation_state: Option<LocalConversationState>,
        tools: Option<Vec<Value>>,
        tool_choice: Option<Value>,
        parallel_tool_calls: Option<bool>,
        tag: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<String, String> {
        let request_id = Self::new_chat_request_id();
        self.send_message_with_storage_mode_and_id(
            request_id.clone(),
            session_id,
            model_id,
            message,
            images,
            workspace,
            storage_mode,
            mode,
            local_conversation_state,
            tools,
            tool_choice,
            parallel_tool_calls,
            tag,
            tags,
        )
        .await?;
        Ok(request_id)
    }

    pub async fn send_message_with_storage_mode_and_id(
        &self,
        request_id: String,
        session_id: Option<String>,
        model_id: String,
        message: String,
        images: Option<Vec<crate::protocol::ChatImage>>,
        workspace: Option<WorkspaceInfo>,
        storage_mode: Option<String>,
        mode: Option<String>,
        local_conversation_state: Option<LocalConversationState>,
        tools: Option<Vec<Value>>,
        tool_choice: Option<Value>,
        parallel_tool_calls: Option<bool>,
        tag: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<(), String> {
        let mut conn = self.connection.lock().await;
        let conn = conn.as_mut().ok_or("Not connected")?;

        let planning_mode = mode
            .as_deref()
            .map(|value| value.eq_ignore_ascii_case("planning"));

        let payload = ChatRequestPayload {
            session_id,
            model_id,
            message,
            images,
            workspace,
            tag,
            tags,
            storage_mode: storage_mode.clone(),
            mode,
            planning_mode,
            local_conversation_state,
            tools,
            tool_choice,
            parallel_tool_calls,
        };

        let msg = WsBaseMessage {
            id: request_id.clone(),
            msg_type: "chat_request".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(serde_json::to_value(&payload).unwrap()),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        eprintln!(
            "[BLADE WS] Sending chat_request: id={}, model={}, message_len={}, image_count={}, workspace_root={:?}",
            request_id,
            payload.model_id,
            payload.message.len(),
            payload.images.as_ref().map_or(0, Vec::len),
            payload.workspace.as_ref().map(|workspace| workspace.root.as_str())
        );

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send message: {}", e))?;
        conn.active_request_id = Some(request_id);
        conn.storage_mode = storage_mode;

        Ok(())
    }

    /// Send a tool execution result
    pub async fn send_tool_result(
        &self,
        session_id: String,
        tool_call_id: String,
        result: ToolResult,
        model_id: Option<String>,
    ) -> Result<(), String> {
        let conn = self.connection.lock().await;
        let conn = conn.as_ref().ok_or("Not connected")?;

        let payload = ToolResultPayload {
            session_id,
            tool_call_id,
            success: result.success,
            content: result.content,
            error: result.error,
            model_id,
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

    pub async fn send_approval_response(
        &self,
        session_id: String,
        approval_id: String,
        tool_call_id: String,
        approved: bool,
    ) -> Result<(), String> {
        let conn = self.connection.lock().await;
        let conn = conn.as_ref().ok_or("Not connected")?;

        let payload = ApprovalResponsePayload {
            session_id,
            approval_id,
            tool_call_id,
            approved,
        };

        let msg = WsBaseMessage {
            id: format!(
                "approval-response-{}",
                chrono::Utc::now().timestamp_millis()
            ),
            msg_type: "approval_response".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(serde_json::to_value(payload).unwrap()),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send approval response: {}", e))?;

        Ok(())
    }

    pub async fn send_cancel_request(
        &self,
        request_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<(), String> {
        let conn = self.connection.lock().await;
        let conn = conn.as_ref().ok_or("Not connected")?;

        let payload = CancelRequestPayload {
            request_id,
            session_id,
            reason: "user_stop".to_string(),
        };

        let msg = WsBaseMessage {
            id: format!("cancel-{}", uuid::Uuid::new_v4()),
            msg_type: "cancel_request".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(serde_json::to_value(payload).unwrap()),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send cancel request: {}", e))?;

        Ok(())
    }

    /// Send conversation context in response to get_conversation_context request (RFC-002)
    pub async fn send_conversation_context(
        &self,
        request_id: String,
        session_id: String,
        messages: Vec<serde_json::Value>,
    ) -> Result<(), String> {
        let t0 = std::time::Instant::now();
        eprintln!(
            "[WS CTX] T+{:?} send_conversation_context called",
            t0.elapsed()
        );

        eprintln!("[WS CTX] T+{:?} Acquiring connection lock...", t0.elapsed());
        let conn = self.connection.lock().await;
        eprintln!("[WS CTX] T+{:?} Lock acquired", t0.elapsed());

        let conn = conn.as_ref().ok_or("Not connected")?;

        let payload = ConversationContextPayload {
            session_id: session_id.clone(),
            messages,
        };

        let msg = WsBaseMessage {
            id: request_id.clone(),
            msg_type: "conversation_context".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(serde_json::to_value(payload).unwrap()),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        eprintln!(
            "[WS CTX] T+{:?} Sending to channel: id={}, session={}, len={}",
            t0.elapsed(),
            request_id,
            session_id,
            json.len()
        );

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send conversation context: {}", e))?;

        eprintln!("[WS CTX] T+{:?} Channel send complete!", t0.elapsed());
        Ok(())
    }

    /// Send context pack response to zcoderd (v0.8.0 capability: context_pack_request)
    pub async fn send_context_pack_response(
        &self,
        request_id: String,
        response_type: String,
        payload: crate::blade_protocol::ContextPackPayload,
    ) -> Result<(), String> {
        let conn = self.connection.lock().await;
        let conn = conn.as_ref().ok_or("Not connected")?;

        let msg = WsBaseMessage {
            id: request_id,
            msg_type: response_type,
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(serde_json::to_value(payload).unwrap()),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send context pack response: {}", e))?;

        Ok(())
    }

    pub async fn send_history_list_request(
        &self,
        request_id: String,
        project_id: String,
    ) -> Result<(), String> {
        let conn = self.connection.lock().await;
        let conn = conn.as_ref().ok_or("Not connected")?;

        let msg = WsBaseMessage {
            id: request_id,
            msg_type: "history_list_request".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(
                serde_json::to_value(HistoryListRequestPayload { project_id })
                    .map_err(|e| format!("JSON serialization error: {}", e))?,
            ),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send history list request: {}", e))?;

        Ok(())
    }

    pub async fn send_history_detail_request(
        &self,
        request_id: String,
        session_id: String,
    ) -> Result<(), String> {
        let conn = self.connection.lock().await;
        let conn = conn.as_ref().ok_or("Not connected")?;

        let msg = WsBaseMessage {
            id: request_id,
            msg_type: "history_detail_request".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(
                serde_json::to_value(HistoryDetailRequestPayload { session_id })
                    .map_err(|e| format!("JSON serialization error: {}", e))?,
            ),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send history detail request: {}", e))?;

        Ok(())
    }

    pub async fn send_zlp_request(&self, request_id: String, payload: Value) -> Result<(), String> {
        let conn = self.connection.lock().await;
        let conn = conn.as_ref().ok_or("Not connected")?;

        let msg = WsBaseMessage {
            id: request_id,
            msg_type: "zlp_request".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(payload),
        };

        let json =
            serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))?;

        conn.tx
            .send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send ZLP request: {}", e))?;

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

    pub async fn get_active_request_id(&self) -> Option<String> {
        let conn = self.connection.lock().await;
        conn.as_ref().and_then(|c| c.active_request_id.clone())
    }

    fn is_server_backed_storage_mode(storage_mode: Option<&str>) -> bool {
        !matches!(storage_mode, Some(mode) if mode.eq_ignore_ascii_case("local"))
    }

    pub fn is_already_closed_error(error: &str) -> bool {
        error.contains("channel closed") || error == "Not connected"
    }

    fn disconnect_message_json(session_id: String) -> Result<String, String> {
        let msg = WsBaseMessage {
            id: format!("disconnect-{}", uuid::Uuid::new_v4()),
            msg_type: "disconnect".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: Some(
                serde_json::to_value(DisconnectPayload { session_id })
                    .map_err(|e| format!("JSON serialization error: {}", e))?,
            ),
        };

        serde_json::to_string(&msg).map_err(|e| format!("JSON serialization error: {}", e))
    }

    pub async fn send_disconnect(&self, session_id: String) -> Result<(), String> {
        let (tx, should_send) = {
            let conn = self.connection.lock().await;
            let conn = conn.as_ref().ok_or("Not connected")?;
            (
                conn.tx.clone(),
                Self::is_server_backed_storage_mode(conn.storage_mode.as_deref()),
            )
        };

        if !should_send {
            return Ok(());
        }

        let json = Self::disconnect_message_json(session_id)?;
        tx.send(WsMessage::Send(json))
            .map_err(|e| format!("Failed to send disconnect: {}", e))
    }

    /// Send the Blade disconnect lifecycle message when possible, then close the socket.
    pub async fn close_with_session_disconnect(
        &self,
        session_id_hint: Option<String>,
        flush_window: std::time::Duration,
    ) -> Result<(), String> {
        let session_id = match session_id_hint {
            Some(session_id) => Some(session_id),
            None => self.get_session_id().await,
        };

        let mut result = Ok(());
        let mut sent_disconnect = false;
        if let Some(session_id) = session_id {
            result = self.send_disconnect(session_id).await;
            if let Err(error) = &result {
                if Self::is_already_closed_error(error) {
                    result = Ok(());
                }
            } else {
                sent_disconnect = true;
            }
            if sent_disconnect && !flush_window.is_zero() {
                tokio::time::sleep(flush_window).await;
            }
        }

        self.close().await;
        result
    }

    /// Close the WebSocket connection
    pub async fn close(&self) {
        let conn = self.connection.lock().await;
        if let Some(ref c) = *conn {
            let _ = c.tx.send(WsMessage::Close);
        }
        let mut role = self.connected_role.write().await;
        *role = None;
    }

    /// Parse incoming WebSocket message without preserving socket-level auth state.
    #[cfg(test)]
    fn parse_message(text: &str, tx: &mpsc::UnboundedSender<BladeWsEvent>) -> Result<(), String> {
        let mut connected_event_sent = false;
        Self::parse_message_with_auth_gate(text, tx, &mut connected_event_sent)
    }

    /// Parse incoming WebSocket message while preserving socket-level auth state.
    fn parse_message_with_auth_gate(
        text: &str,
        tx: &mpsc::UnboundedSender<BladeWsEvent>,
        connected_event_sent: &mut bool,
    ) -> Result<(), String> {
        let msg: WsIncomingMessage =
            serde_json::from_str(text).map_err(|e| format!("JSON parse error: {}", e))?;
        let correlation_id = Self::correlation_id_for_message(&msg);
        let request_id = Self::request_id_for_message(&msg);

        match msg.msg_type.as_str() {
            "authenticated" => {
                if *connected_event_sent {
                    return Ok(());
                }
                *connected_event_sent = true;

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

                // eprintln!("[BLADE WS] Authenticated as {}", user_id);
                let _ = tx.send(BladeWsEvent::Connected {
                    user_id,
                    server_version,
                });
            }
            "history_list_response" => {
                let conversations = msg
                    .payload
                    .get("conversations")
                    .cloned()
                    .ok_or_else(|| "Missing conversations in history_list_response".to_string())
                    .and_then(|value| {
                        serde_json::from_value::<Vec<crate::blade_protocol::ConversationSummary>>(
                            value,
                        )
                        .map_err(|e| format!("Failed to parse history list response: {}", e))
                    })?;

                let _ = tx.send(BladeWsEvent::HistoryListResponse {
                    request_id: correlation_id.clone(),
                    conversations,
                });
            }
            "history_detail_response" => {
                let conversation =
                    serde_json::from_value::<crate::blade_protocol::FullConversation>(
                        msg.payload.clone(),
                    )
                    .map_err(|e| format!("Failed to parse history detail response: {}", e))?;

                let _ = tx.send(BladeWsEvent::HistoryDetailResponse {
                    request_id: correlation_id.clone(),
                    conversation,
                });
            }
            "zlp_response" => {
                let _ = tx.send(BladeWsEvent::ZlpResponse {
                    request_id: correlation_id.clone(),
                    response: msg.payload.clone(),
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
                let planning_mode = msg.payload.get("planning_mode").and_then(|v| v.as_bool());
                let runtime_mode = msg
                    .payload
                    .get("runtime_mode")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());
                let mode_source = msg
                    .payload
                    .get("mode_source")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());

                // eprintln!("[BLADE WS] Session created: {}", session_id);
                let _ = tx.send(BladeWsEvent::Session {
                    session_id,
                    model_id,
                    planning_mode,
                    runtime_mode,
                    mode_source,
                });
            }
            "text_chunk" => {
                // Parse output_index to distinguish parallel output streams (OpenAI Responses API)
                let output_index = msg
                    .payload
                    .get("output_index")
                    .or_else(|| msg.payload.get("content_index"))
                    .or_else(|| msg.payload.get("index"))
                    .and_then(|v| v.as_i64());
                let phase = msg
                    .payload
                    .get("phase")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if let Some(content) = msg.payload.get("content").and_then(|v| v.as_str()) {
                    let _ = tx.send(BladeWsEvent::TextChunk {
                        request_id,
                        content: content.to_string(),
                        output_index,
                        phase,
                    });
                }
            }
            "reasoning_chunk" => {
                let output_index = msg
                    .payload
                    .get("output_index")
                    .or_else(|| msg.payload.get("content_index"))
                    .or_else(|| msg.payload.get("index"))
                    .and_then(|v| v.as_i64());
                let phase = msg
                    .payload
                    .get("phase")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if let Some(content) = msg.payload.get("content").and_then(|v| v.as_str()) {
                    let _ = tx.send(BladeWsEvent::ReasoningChunk {
                        request_id,
                        content: content.to_string(),
                        output_index,
                        phase,
                    });
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
                    let _preview: String = str_args.chars().take(200).collect();
                    // eprintln!("[BLADE WS] Parsing string arguments ({} bytes): {}...", str_args.len(), _preview);
                    match serde_json::from_str::<Value>(str_args) {
                        Ok(v) => v,
                        Err(_e) => {
                            // eprintln!("[BLADE WS] Failed to parse arguments as JSON: {}", _e);
                            Value::String(str_args.to_string())
                        }
                    }
                } else {
                    raw_args.cloned().unwrap_or(Value::Null)
                };

                // eprintln!("[BLADE WS] Tool call: {} ({})", name, id);
                let _ = tx.send(BladeWsEvent::ToolCall {
                    request_id,
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
                            // eprintln!("[BLADE WS] Todo updated: {} items", todos.len());
                            match tx.send(BladeWsEvent::TodoUpdated { todos }) {
                                Ok(_) => {}
                                // eprintln!("[BLADE WS] TodoUpdated event sent to channel"),
                                Err(e) => eprintln!(
                                    "[BLADE WS] Failed to send TodoUpdated to channel: {}",
                                    e
                                ),
                            }
                        }
                        Err(_e) => {
                            // eprintln!("[BLADE WS] Failed to parse todos: {}", _e);
                        }
                    }
                }
            }
            "approval_request" => {
                let session_id = msg
                    .payload
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let approval_id = msg
                    .payload
                    .get("approval_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_call_id = msg
                    .payload
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_name = msg
                    .payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let source = msg
                    .payload
                    .get("source")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());
                let rule_name = msg
                    .payload
                    .get("rule_name")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());
                let message = msg
                    .payload
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());
                let decision = msg
                    .payload
                    .get("decision")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());
                let arguments = msg.payload.get("arguments").cloned().unwrap_or(Value::Null);

                let _ = tx.send(BladeWsEvent::ApprovalRequest {
                    request_id,
                    session_id,
                    approval_id,
                    tool_call_id,
                    tool_name,
                    arguments,
                    source,
                    rule_name,
                    message,
                    decision,
                });
            }
            "chat_done" => {
                let finish_reason = msg
                    .payload
                    .get("finish_reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("stop")
                    .to_string();
                let recoverable = msg.payload.get("recoverable").and_then(|v| v.as_bool());

                // eprintln!("[BLADE WS] Chat done: {} (recoverable: {:?})", finish_reason, recoverable);
                let _ = tx.send(BladeWsEvent::ChatDone {
                    request_id,
                    finish_reason,
                    recoverable,
                });
            }
            "request_cancelled" => {
                let request_id = msg
                    .payload
                    .get("request_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&correlation_id)
                    .to_string();
                let session_id = msg
                    .payload
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());
                let reason = msg
                    .payload
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());

                let _ = tx.send(BladeWsEvent::RequestCancelled {
                    request_id,
                    session_id,
                    reason,
                });
            }
            "error" => {
                // Standardized error type (context_length_exceeded, rate_limit_error, etc.)
                let error_type = msg
                    .payload
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown_error")
                    .to_string();
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

                // Error detail fields (RFC: Error Handling)
                let token_count = msg.payload.get("token_count").and_then(|v| v.as_u64());
                let max_tokens = msg.payload.get("max_tokens").and_then(|v| v.as_u64());
                let excess = msg.payload.get("excess").and_then(|v| v.as_u64());
                let recoverable = msg.payload.get("recoverable").and_then(|v| v.as_bool());
                let recovery_hint = msg
                    .payload
                    .get("recovery_hint")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // eprintln!("[BLADE WS] Error: {} ({}) - {} (tokens: {:?}/{:?}, recoverable: {:?})",
                //     error_type, code, message, token_count, max_tokens, recoverable);
                let _ = tx.send(BladeWsEvent::Error {
                    request_id: Some(correlation_id.clone()),
                    error_type,
                    code,
                    message,
                    token_count,
                    max_tokens,
                    excess,
                    recoverable,
                    recovery_hint,
                });
            }
            "tool_result_ack" => {
                // Tool result acknowledgment - zcoderd received our result but is waiting for more
                let pending_count = msg
                    .payload
                    .get("pending_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                let _ = pending_count;
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

                // eprintln!("[BLADE WS] Progress: {} ({}%)", message, percent);
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

                let _ = tx.send(BladeWsEvent::Research { content });
            }
            "tool_activity" => {
                let tool_name = msg
                    .payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let file_path = msg
                    .payload
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let action = msg
                    .payload
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("processing")
                    .to_string();

                let _ = tx.send(BladeWsEvent::ToolActivity {
                    tool_name,
                    file_path,
                    action,
                });
            }
            "cognitive_interrupt" => {
                match serde_json::from_value::<CognitiveInterruptPayload>(msg.payload.clone()) {
                    Ok(payload) => {
                        let _ = tx.send(BladeWsEvent::CognitiveInterrupt(payload));
                    }
                    Err(error) => {
                        eprintln!(
                            "[BLADE WS] Failed to parse cognitive_interrupt payload: {}",
                            error
                        );
                    }
                }
            }
            "tool_progress" => {
                // Tool progress - streaming partial arguments as tool call is being generated
                let tool_call_id = msg
                    .payload
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_name = msg
                    .payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let partial_arguments = msg
                    .payload
                    .get("partial_arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Extract file path from partial_arguments using regex
                // partial_arguments is incomplete JSON like: '{"path": "/home/stig/dev/ai/z'
                let file_path = Self::extract_file_path_from_partial_args(partial_arguments);

                let _ = tx.send(BladeWsEvent::ToolProgress {
                    tool_call_id,
                    tool_name,
                    file_path,
                });
            }
            "get_conversation_context" => {
                // zcoderd sends payload as Base64-encoded JSON string
                // Decode: "eyJzZXNzaW9uX2lkIjoiLi4uIn0=" -> {"session_id":"..."}
                let session_id = if let Some(payload_str) = msg.payload.as_str() {
                    // Payload is a Base64-encoded string
                    use base64::Engine;
                    match base64::engine::general_purpose::STANDARD.decode(payload_str) {
                        Ok(decoded_bytes) => match String::from_utf8(decoded_bytes) {
                            Ok(json_str) => match serde_json::from_str::<Value>(&json_str) {
                                Ok(json_obj) => json_obj
                                    .get("session_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                Err(_e) => "".to_string(),
                            },
                            Err(_e) => "".to_string(),
                        },
                        Err(_e) => "".to_string(),
                    }
                } else {
                    // Fallback: payload is a JSON object directly (old format)
                    msg.payload
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                };

                let _ = tx.send(BladeWsEvent::GetConversationContext {
                    request_id: correlation_id,
                    session_id,
                });
            }
            "context_pack_request" | "fast_context" | "fast_context_request" => {
                let include = msg.payload.get("include");
                let id = msg
                    .payload
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&correlation_id)
                    .to_string();
                let query = msg
                    .payload
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let queries = msg
                    .payload
                    .get("queries")
                    .and_then(|v| v.as_array())
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str())
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let intent = msg
                    .payload
                    .get("intent")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let max_results = msg
                    .payload
                    .get("max_results")
                    .or_else(|| msg.payload.get("maxResults"))
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize);
                let include_tests = msg
                    .payload
                    .get("include_tests")
                    .or_else(|| msg.payload.get("includeTests"))
                    .or_else(|| include.and_then(|v| v.get("tests")))
                    .and_then(|v| v.as_bool());
                let include_docs = msg
                    .payload
                    .get("include_docs")
                    .or_else(|| msg.payload.get("includeDocs"))
                    .or_else(|| include.and_then(|v| v.get("docs")))
                    .and_then(|v| v.as_bool());
                let include_memory = msg
                    .payload
                    .get("include_memory")
                    .or_else(|| msg.payload.get("includeMemory"))
                    .or_else(|| include.and_then(|v| v.get("memory")))
                    .and_then(|v| v.as_bool());
                let include_project_index_min = msg
                    .payload
                    .get("include_project_index_min")
                    .or_else(|| msg.payload.get("includeProjectIndexMin"))
                    .or_else(|| include.and_then(|v| v.get("project_index_min")))
                    .or_else(|| include.and_then(|v| v.get("projectIndexMin")))
                    .and_then(|v| v.as_bool());

                let _ = tx.send(BladeWsEvent::ContextPackRequest {
                    id,
                    query,
                    queries,
                    intent,
                    max_results,
                    include_tests,
                    include_docs,
                    include_memory,
                    include_project_index_min,
                    response_type: if msg.msg_type == "context_pack_request" {
                        "context_pack_response".to_string()
                    } else {
                        "fast_context_response".to_string()
                    },
                });
            }
            "pairing_token_response" => {
                let token = msg
                    .payload
                    .get("token")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let bot_username = msg
                    .payload
                    .get("bot_username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ZaguanAIBot") // Default if not provided
                    .to_string();
                let expires_in = msg
                    .payload
                    .get("expires_in")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(300);

                let _ = tx.send(BladeWsEvent::PairingTokenResponse {
                    token,
                    bot_username,
                    expires_in,
                });
            }
            "remote_control_connected" => {
                let telegram_username = msg
                    .payload
                    .get("telegram_username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let telegram_chat_id = msg
                    .payload
                    .get("telegram_chat_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let _ = tx.send(BladeWsEvent::RemoteControlConnected {
                    telegram_username,
                    telegram_chat_id,
                });
            }
            "remote_control_disconnected" => {
                let _ = tx.send(BladeWsEvent::RemoteControlDisconnected);
            }
            _ => {
                // Intentionally ignore unknown message types.
            }
        }

        Ok(())
    }

    /// Extract file path from partial JSON arguments
    /// Handles incomplete JSON like: '{"path": "/home/stig/dev/ai/z'
    fn extract_file_path_from_partial_args(partial_args: &str) -> Option<String> {
        for re in FILE_PATH_REGEXES.iter() {
            // Use the LAST match - partial_arguments accumulates repeated prefixes,
            // so the last match has the longest/most complete path
            let mut best: Option<String> = None;
            for caps in re.captures_iter(partial_args) {
                if let Some(path) = caps.get(1) {
                    let path_str = path.as_str();
                    if !path_str.is_empty() && path_str.starts_with('/') {
                        best = Some(path_str.to_string());
                    }
                }
            }
            if best.is_some() {
                return best;
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_endpoint_url_has_no_api_key_query() {
        let url = BladeWsClient::websocket_endpoint_url("https://coder-api.zblade.dev/");

        assert_eq!(url, "wss://coder-api.zblade.dev/v1/blade/v2");
        assert!(!url.contains("api_key"));
        assert!(!url.contains('?'));
    }

    #[test]
    fn websocket_request_uses_authorization_bearer_header() {
        let request =
            BladeWsClient::websocket_request("https://coder-api.zblade.dev", "  ps_live_secret  ")
                .unwrap();

        assert_eq!(
            request.uri().to_string(),
            "wss://coder-api.zblade.dev/v1/blade/v2"
        );
        assert_eq!(
            request
                .headers()
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer ps_live_secret")
        );
        assert!(request.uri().query().is_none());
    }

    #[test]
    fn authenticate_payload_omits_api_key_for_header_auth() {
        let value = serde_json::to_value(AuthenticatePayload {
            client: "zblade".to_string(),
            version: "0.0.0-test".to_string(),
            client_name: "zblade".to_string(),
            client_version: "0.0.0-test".to_string(),
            protocol_version: 3,
            capabilities: None,
            environment: None,
        })
        .unwrap();

        assert!(value.get("api_key").is_none());
    }

    #[test]
    fn post_auth_payloads_omit_api_key() {
        let chat_payload = serde_json::to_value(ChatRequestPayload {
            session_id: Some("sess-1".to_string()),
            model_id: "model".to_string(),
            message: "hello".to_string(),
            images: None,
            workspace: None,
            tag: None,
            tags: None,
            storage_mode: None,
            mode: None,
            planning_mode: None,
            local_conversation_state: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
        })
        .unwrap();
        let tool_payload = serde_json::to_value(ToolResultPayload {
            session_id: "sess-1".to_string(),
            tool_call_id: "tool-1".to_string(),
            success: true,
            content: "ok".to_string(),
            error: None,
            model_id: Some("model".to_string()),
        })
        .unwrap();

        assert!(chat_payload.get("api_key").is_none());
        assert!(tool_payload.get("api_key").is_none());
    }

    #[test]
    fn parses_immediate_authenticated_message_after_header_auth() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let message = r#"{
            "type": "authenticated",
            "id": "",
            "payload": {
                "user_id": "user_abcdefgh",
                "server_version": "2026.06.20",
                "protocol_version": 3,
                "capabilities": ["tool_execution"]
            }
        }"#;

        BladeWsClient::parse_message(message, &tx).unwrap();

        match rx.try_recv().unwrap() {
            BladeWsEvent::Connected {
                user_id,
                server_version,
            } => {
                assert_eq!(user_id, "user_abcdefgh");
                assert_eq!(server_version, "2026.06.20");
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn suppresses_duplicate_authenticated_events_on_one_socket() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut connected_event_sent = false;
        let header_auth_message = r#"{
            "type": "authenticated",
            "id": "",
            "payload": {
                "user_id": "user_header",
                "server_version": "2026.06.20"
            }
        }"#;
        let legacy_auth_message = r#"{
            "type": "authenticated",
            "id": "auth-1",
            "payload": {
                "user_id": "user_legacy",
                "server_version": "2026.06.20"
            }
        }"#;

        BladeWsClient::parse_message_with_auth_gate(
            header_auth_message,
            &tx,
            &mut connected_event_sent,
        )
        .unwrap();
        BladeWsClient::parse_message_with_auth_gate(
            legacy_auth_message,
            &tx,
            &mut connected_event_sent,
        )
        .unwrap();

        match rx.try_recv().unwrap() {
            BladeWsEvent::Connected { user_id, .. } => {
                assert_eq!(user_id, "user_header");
            }
            other => panic!("unexpected event: {:?}", other),
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn parses_documented_context_pack_request_without_timestamp() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let message = r#"{
            "type": "context_pack_request",
            "id": "ctx-123",
            "payload": {
                "query": "Fix checkout modal",
                "intent": "bug_fix",
                "max_results": 8,
                "include_tests": true,
                "include_docs": true,
                "include_memory": true
            }
        }"#;

        BladeWsClient::parse_message(message, &tx).unwrap();

        match rx.try_recv().unwrap() {
            BladeWsEvent::ContextPackRequest {
                id,
                query,
                intent,
                max_results,
                include_tests,
                include_docs,
                include_memory,
                response_type,
                ..
            } => {
                assert_eq!(id, "ctx-123");
                assert_eq!(query, "Fix checkout modal");
                assert_eq!(intent.as_deref(), Some("bug_fix"));
                assert_eq!(max_results, Some(8));
                assert_eq!(include_tests, Some(true));
                assert_eq!(include_docs, Some(true));
                assert_eq!(include_memory, Some(true));
                assert_eq!(response_type, "context_pack_response");
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn parses_request_cancelled_acknowledgement() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let message = r#"{
            "type": "request_cancelled",
            "id": "chat-123",
            "payload": {
                "request_id": "chat-123",
                "session_id": "sess-456",
                "reason": "user_stop"
            }
        }"#;

        BladeWsClient::parse_message(message, &tx).unwrap();

        match rx.try_recv().unwrap() {
            BladeWsEvent::RequestCancelled {
                request_id,
                session_id,
                reason,
            } => {
                assert_eq!(request_id, "chat-123");
                assert_eq!(session_id.as_deref(), Some("sess-456"));
                assert_eq!(reason.as_deref(), Some("user_stop"));
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn parses_stream_event_request_ids() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let message = r#"{
            "type": "text_chunk",
            "id": "event-1",
            "request_id": "chat-123",
            "payload": {
                "content": "fix(git): correlate stream events",
                "phase": "final"
            }
        }"#;

        BladeWsClient::parse_message(message, &tx).unwrap();

        match rx.try_recv().unwrap() {
            BladeWsEvent::TextChunk {
                request_id,
                content,
                phase,
                ..
            } => {
                assert_eq!(request_id.as_deref(), Some("chat-123"));
                assert_eq!(content, "fix(git): correlate stream events");
                assert_eq!(phase.as_deref(), Some("final"));
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn parses_cognitive_interrupt_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let message = r#"{
            "type": "cognitive_interrupt",
            "id": "request-id",
            "timestamp": 1781450000000,
            "payload": {
                "state": "blocked",
                "level": "hard",
                "frame": "patch_failure_repair",
                "reason": "same_failure_after_multiple_corrections",
                "next_action_type": "diagnostic",
                "summary": "Corrective action blocked until diagnostic evidence is gathered",
                "tool_name": "apply_patch",
                "failure_count": 2,
                "future_field": "ignored"
            }
        }"#;

        BladeWsClient::parse_message(message, &tx).unwrap();

        match rx.try_recv().unwrap() {
            BladeWsEvent::CognitiveInterrupt(payload) => {
                assert_eq!(payload.state, "blocked");
                assert_eq!(payload.level.as_deref(), Some("hard"));
                assert_eq!(payload.frame.as_deref(), Some("patch_failure_repair"));
                assert_eq!(
                    payload.reason.as_deref(),
                    Some("same_failure_after_multiple_corrections")
                );
                assert_eq!(payload.next_action_type.as_deref(), Some("diagnostic"));
                assert_eq!(
                    payload.summary,
                    "Corrective action blocked until diagnostic evidence is gathered"
                );
                assert_eq!(payload.tool_name.as_deref(), Some("apply_patch"));
                assert_eq!(payload.failure_count, Some(2));
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn cancel_request_payload_includes_request_and_session_ids() {
        let payload = CancelRequestPayload {
            request_id: Some("chat-123".to_string()),
            session_id: Some("sess-456".to_string()),
            reason: "user_stop".to_string(),
        };

        let value = serde_json::to_value(payload).unwrap();

        assert_eq!(value["request_id"], "chat-123");
        assert_eq!(value["session_id"], "sess-456");
        assert_eq!(value["reason"], "user_stop");
    }

    #[test]
    fn disconnect_message_payload_includes_session_id() {
        let json = BladeWsClient::disconnect_message_json("sess-456".to_string()).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "disconnect");
        assert!(value["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("disconnect-")));
        assert_eq!(value["payload"]["session_id"], "sess-456");
    }

    #[test]
    fn already_closed_errors_are_detected() {
        assert!(BladeWsClient::is_already_closed_error(
            "Failed to send disconnect: channel closed"
        ));
        assert!(BladeWsClient::is_already_closed_error("Not connected"));
        assert!(!BladeWsClient::is_already_closed_error(
            "Failed to send disconnect: permission denied"
        ));
    }

    #[test]
    fn local_storage_mode_is_not_server_backed() {
        assert!(!BladeWsClient::is_server_backed_storage_mode(Some("local")));
        assert!(!BladeWsClient::is_server_backed_storage_mode(Some("LOCAL")));
        assert!(BladeWsClient::is_server_backed_storage_mode(Some("server")));
        assert!(BladeWsClient::is_server_backed_storage_mode(None));
    }

    #[test]
    fn parses_fast_context_request_with_nested_include_flags() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let message = r#"{
            "type": "fast_context",
            "id": "ctx-456",
            "payload": {
                "queries": ["symbols db", "fast context"],
                "maxResults": 12,
                "include": {
                    "tests": false,
                    "docs": true,
                    "memory": false,
                    "project_index_min": true
                }
            }
        }"#;

        BladeWsClient::parse_message(message, &tx).unwrap();

        match rx.try_recv().unwrap() {
            BladeWsEvent::ContextPackRequest {
                id,
                queries,
                max_results,
                include_tests,
                include_docs,
                include_memory,
                include_project_index_min,
                response_type,
                ..
            } => {
                assert_eq!(id, "ctx-456");
                assert_eq!(queries, vec!["symbols db", "fast context"]);
                assert_eq!(max_results, Some(12));
                assert_eq!(include_tests, Some(false));
                assert_eq!(include_docs, Some(true));
                assert_eq!(include_memory, Some(false));
                assert_eq!(include_project_index_min, Some(true));
                assert_eq!(response_type, "fast_context_response");
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }
}
