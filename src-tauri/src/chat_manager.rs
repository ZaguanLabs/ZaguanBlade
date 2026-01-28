// use eframe::egui; // Removed
use std::path::PathBuf;
use std::sync::{mpsc, Arc};

use crate::agentic_loop::AgenticLoop;
use crate::ai_workflow::get_tool_definitions;
use crate::ai_workflow::{AiWorkflow, PendingToolBatch};
use crate::blade_ws_client::BladeWsClient;
use crate::config::ApiConfig;
use crate::conversation::ConversationHistory;
use crate::models::registry::ModelInfo;
use crate::protocol::ToolFunction;
use crate::protocol::{ChatEvent, ChatMessage, ChatRole, ToolCall};
use crate::reasoning_parser::ReasoningParser;
use crate::xml_parser;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub enum DrainResult {
    None,
    Update(ChatMessage, String), // Immediate update for streaming chunks with the delta
    Reasoning(ChatMessage, String), // Reasoning chunk delta
    Research {
        content: String,
        suggested_name: String,
    },
    Progress {
        message: String,
        stage: String,
        percent: i32,
    },
    ToolCalls(Vec<ToolCall>, Option<String>),
    ToolCreated(ChatMessage, Vec<ToolCall>),
    ToolStatusUpdate(ChatMessage),
    ToolActivity {
        tool_name: String,
        file_path: String,
        action: String,
    },
    TodoUpdated(Vec<crate::protocol::TodoItem>),
    MessageCompleted(String), // Message ID for completed message
    Error(String),
    /// RFC: Context Length Recovery - context limit exceeded
    ContextLengthExceeded {
        message: String,
        token_count: Option<u64>,
        max_tokens: Option<u64>,
        excess: Option<u64>,
        recoverable: bool,
        recovery_hint: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone)]
struct OllamaMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct OllamaToolCall {
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default)]
    typ: String,
    function: OllamaToolFunction,
}

#[derive(Serialize, Deserialize, Clone)]
struct OllamaToolFunction {
    name: String,
    arguments: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<usize>,
}

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
}

#[derive(Deserialize)]
struct OllamaChatChunk {
    #[serde(default)]
    message: Option<OllamaMessage>,
    #[serde(default)]
    done: Option<bool>,
    #[serde(default)]
    error: Option<String>,
}

pub struct ChatManager {
    pub streaming: bool,
    pub rx: Option<mpsc::Receiver<ChatEvent>>,
    pub xml_buffer: String,
    pub reasoning_parser: ReasoningParser, // v1.2: Multi-format reasoning extraction
    pub agentic_loop: AgenticLoop,
    pub session_id: Option<String>,
    abort_handle: Option<tokio::task::AbortHandle>,
    pub accumulated_tool_calls: Vec<ToolCall>,
    pub updated_assistant_message: Option<ChatMessage>,
    pub message_seq: u64, // v1.1: sequence number for MessageDelta events
    pub pending_results: std::collections::VecDeque<DrainResult>,
    ws_client: Option<Arc<BladeWsClient>>, // Persistent connection for the conversation
}

impl ChatManager {
    pub fn new(max_turns: usize) -> Self {
        Self {
            streaming: false,
            rx: None,
            xml_buffer: String::new(),
            reasoning_parser: ReasoningParser::new(),
            agentic_loop: AgenticLoop::new(max_turns),
            session_id: None,
            abort_handle: None,
            accumulated_tool_calls: Vec::new(),
            updated_assistant_message: None,
            message_seq: 0,
            pending_results: std::collections::VecDeque::new(),
            ws_client: None,
        }
    }
    pub fn start_stream(
        &mut self,
        _prompt: String,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        models: &[ModelInfo],
        selected_model: usize,
        workspace: Option<&PathBuf>,
        active_file: Option<String>,
        open_files: Option<Vec<String>>,
        cursor_line: Option<usize>,
        cursor_column: Option<usize>,
        http: reqwest::Client,
        storage_mode: Option<String>,
    ) -> Result<(), String> {
        self.reasoning_parser.reset();
        self.xml_buffer.clear();
        self.accumulated_tool_calls.clear();
        self.updated_assistant_message = None;
        self.message_seq = 0; // v1.1: reset sequence counter for new message

        // Get model ID
        let selected_info = models.get(selected_model);
        let model_id = selected_info
            .map(|m| {
                let id = m.api_id.as_ref().unwrap_or(&m.id).clone();
                eprintln!(
                    "[CHAT MGR] Model selection: display_id={}, api_id={:?}, sending={}",
                    m.id, m.api_id, id
                );
                id
            })
            .unwrap_or_else(|| "anthropic/claude-sonnet-4-5-20250929".to_string());

        // Short-circuit for Ollama models
        if selected_info
            .and_then(|m| m.provider.as_deref())
            .map(|provider| provider == "ollama")
            .unwrap_or(false)
        {
            return self.start_ollama_stream(
                conversation,
                api_config,
                &model_id,
                http,
                workspace,
                active_file,
            );
        }

        // Build workspace info for Blade Protocol
        let open_file_infos = open_files
            .unwrap_or_default()
            .into_iter()
            .map(|path| crate::blade_ws_client::OpenFileInfo {
                path: path.clone(),
                hash: String::new(),
                is_active: active_file.as_ref() == Some(&path),
                is_modified: false,
            })
            .collect();

        let cursor_position = if let (Some(line), Some(col)) = (cursor_line, cursor_column) {
            Some(crate::blade_ws_client::CursorPosition {
                line: line as i32,
                column: col as i32,
            })
        } else {
            None
        };

        // Get or create project ID
        let project_id = workspace.and_then(|p| crate::project::get_or_create_project_id(p).ok());

        let workspace_info = crate::blade_ws_client::WorkspaceInfo {
            root: workspace
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            project_id,
            active_file,
            cursor_position,
            open_files: open_file_infos,
        };

        // Get last user message
        let user_message = conversation
            .get_messages()
            .iter()
            .rev()
            .find(|m| m.role == ChatRole::User)
            .map(|m| m.content.clone())
            .unwrap_or_default();

        // Close any existing WebSocket connection before starting a new one
        if let Some(old_client) = self.ws_client.take() {
            eprintln!("[CHAT MGR] Closing previous WebSocket connection");
            let old_client_clone = old_client.clone();
            tokio::spawn(async move {
                old_client_clone.close().await;
            });
        }

        // Create new WebSocket client for this conversation
        let blade_url = api_config.blade_url.clone();
        let api_key = api_config.api_key.clone();
        eprintln!("[BLADE WS] Connecting to: {}", blade_url);
        eprintln!("[BLADE WS] Sending message: {}", user_message);
        eprintln!("[BLADE WS] API key present: {}", !api_key.is_empty());

        let ws_client = Arc::new(BladeWsClient::new(blade_url.clone(), api_key.clone()));
        self.ws_client = Some(ws_client.clone());
        let session_id = self.session_id.clone();
        
        eprintln!("[CHAT MGR] Starting stream with session_id: {:?}", session_id);

        // RFC-002: Clone conversation messages for local storage mode context retrieval
        // Convert to BladeMessage format that zcoderd expects
        let conversation_messages: Vec<serde_json::Value> = conversation
            .get_messages()
            .iter()
            .map(|msg| {
                let role = match msg.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::System => "system",
                    ChatRole::Tool => "tool",
                };
                let mut blade_msg = serde_json::json!({
                    "role": role,
                    "content": msg.content,
                });
                if let Some(ref reasoning) = msg.reasoning {
                    blade_msg["reasoning"] = serde_json::json!(reasoning);
                }
                if let Some(ref tool_call_id) = msg.tool_call_id {
                    blade_msg["tool_call_id"] = serde_json::json!(tool_call_id);
                }
                if let Some(ref tool_calls) = msg.tool_calls {
                    // Convert tool calls to the format zcoderd expects
                    let tc_json: Vec<serde_json::Value> = tool_calls
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": tc.typ,
                                "function": {
                                    "name": tc.function.name,
                                    "arguments": tc.function.arguments
                                }
                            })
                        })
                        .collect();
                    blade_msg["tool_calls"] = serde_json::json!(tc_json);
                }
                blade_msg
            })
            .collect();

        // Convert WebSocket events to ChatEvent channel
        let (tx, rx) = mpsc::channel();

        // Create a channel to send session_id back to main thread
        let (session_tx, session_rx) = std::sync::mpsc::channel();

        // Spawn async task to connect and handle events
        let task = tokio::spawn(async move {
            eprintln!("[CHAT MGR] Connecting to WebSocket");
            match ws_client.connect().await {
                Ok(mut ws_rx) => {
                    eprintln!("[CHAT MGR] WebSocket connected, waiting for authenticated");

                    // Wait for authentication
                    let mut authenticated = false;
                    let mut saw_chat_done = false;
                    let mut saw_content = false;
                    while let Some(event) = ws_rx.recv().await {
                        eprintln!(
                            "[CHAT MGR] Received event: {:?}",
                            std::mem::discriminant(&event)
                        );
                        match event {
                            crate::blade_ws_client::BladeWsEvent::Connected { .. } => {
                                eprintln!("[CHAT MGR] Authenticated, sending chat message with session_id: {:?}", session_id);
                                authenticated = true;

                                // Send chat message with storage mode (RFC-002)
                                if let Err(e) = ws_client
                                    .send_message_with_storage_mode(
                                        session_id.clone(),
                                        model_id.clone(),
                                        user_message.clone(),
                                        Some(workspace_info.clone()),
                                        storage_mode.clone(),
                                    )
                                    .await
                                {
                                    eprintln!("[CHAT MGR] Failed to send message: {}", e);
                                    let _ = tx.send(ChatEvent::Error(e));
                                    break;
                                }
                                eprintln!("[CHAT MGR] Message sent successfully");
                            }
                            crate::blade_ws_client::BladeWsEvent::Session {
                                session_id,
                                model_id,
                            } => {
                                eprintln!(
                                    "[CHAT MGR] Session event: session_id={}, model={}",
                                    session_id, model_id
                                );
                                ws_client.set_session_id(session_id.clone()).await;
                                let _ = tx.send(ChatEvent::Session {
                                    session_id: session_id.clone(),
                                    model: model_id,
                                });
                                let _ = session_tx.send(session_id);
                            }
                            crate::blade_ws_client::BladeWsEvent::TextChunk(text) => {
                                eprintln!("[CHAT MGR] Text chunk: {}", text);
                                saw_content = true;
                                let _ = tx.send(ChatEvent::Chunk(text));
                            }
                            crate::blade_ws_client::BladeWsEvent::ReasoningChunk(text) => {
                                eprintln!("[CHAT MGR] Reasoning chunk: {}", text);
                                saw_content = true;
                                let _ = tx.send(ChatEvent::ReasoningChunk(text));
                            }
                            crate::blade_ws_client::BladeWsEvent::ToolCall {
                                id,
                                name,
                                arguments,
                            } => {
                                eprintln!("[CHAT MGR] Tool call: {}", name);
                                saw_content = true;
                                let tool_call = ToolCall {
                                    id,
                                    typ: "function".to_string(),
                                    function: ToolFunction {
                                        name,
                                        arguments: arguments.to_string(),
                                    },
                                    status: Some("executing".to_string()),
                                    result: None,
                                };
                                let _ = tx.send(ChatEvent::ToolCalls(vec![tool_call]));
                            }
                            crate::blade_ws_client::BladeWsEvent::ToolResultAck {
                                pending_count,
                            } => {
                                // zcoderd acknowledged our tool result but is waiting for more
                                // This is informational - keep connection alive and wait for real response
                                eprintln!(
                                    "[CHAT MGR] Tool result acknowledged, {} more pending",
                                    pending_count
                                );
                                // Continue listening - don't close connection or emit Done
                            }
                            crate::blade_ws_client::BladeWsEvent::TodoUpdated { todos } => {
                                eprintln!("[CHAT MGR] Todo updated: {} items", todos.len());
                                // Convert to protocol TodoItem
                                let protocol_todos: Vec<crate::protocol::TodoItem> = todos
                                    .into_iter()
                                    .map(|t| crate::protocol::TodoItem {
                                        content: t.content.clone(),
                                        active_form: t.active_form,
                                        status: t.status,
                                    })
                                    .collect();
                                let _ = tx.send(ChatEvent::TodoUpdated(protocol_todos));
                            }
                            crate::blade_ws_client::BladeWsEvent::ChatDone { finish_reason, recoverable } => {
                                eprintln!("[CHAT MGR] Chat done: {} (recoverable: {:?})", finish_reason, recoverable);
                                saw_chat_done = true;
                                
                                // RFC: Context Length Recovery - check for context_length_exceeded finish reason
                                if finish_reason == "context_length_exceeded" {
                                    let _ = tx.send(ChatEvent::ContextLengthExceeded {
                                        message: "Context limit reached during generation".to_string(),
                                        token_count: None,
                                        max_tokens: None,
                                        excess: None,
                                        recoverable: recoverable.unwrap_or(true),
                                        recovery_hint: None,
                                    });
                                }
                                
                                let _ = tx.send(ChatEvent::Done);
                                // Don't break - keep connection alive for tool results
                                // The connection will close when the user sends a new message
                            }
                            crate::blade_ws_client::BladeWsEvent::Error { error_type, code, message, token_count, max_tokens, excess, recoverable, recovery_hint } => {
                                eprintln!("[CHAT MGR] Error: {} ({}) - {} (tokens: {:?}/{:?})", error_type, code, message, token_count, max_tokens);
                                
                                // RFC: Error Handling - use error_type for logic, message for display
                                match error_type.as_str() {
                                    "context_length_exceeded" => {
                                        let _ = tx.send(ChatEvent::ContextLengthExceeded {
                                            message: message.clone(),
                                            token_count,
                                            max_tokens,
                                            excess,
                                            recoverable: recoverable.unwrap_or(true),
                                            recovery_hint,
                                        });
                                        // Don't break - session is still valid, user can continue
                                    }
                                    "rate_limit_error" | "overloaded_error" => {
                                        // Retryable errors - don't break, let user retry
                                        let hint = recovery_hint.unwrap_or_else(|| "Please wait a moment and try again.".to_string());
                                        let _ = tx.send(ChatEvent::Error(format!("{} - {}", message, hint)));
                                        // Don't break - these are transient
                                    }
                                    "authentication_error" => {
                                        // Fatal error - break the connection
                                        let _ = tx.send(ChatEvent::Error(message));
                                        break;
                                    }
                                    _ => {
                                        // Unknown error type - use recoverable flag
                                        if authenticated && (saw_chat_done || saw_content) {
                                            let _ = tx.send(ChatEvent::Done);
                                            break;
                                        } else if recoverable.unwrap_or(false) {
                                            let _ = tx.send(ChatEvent::Error(message));
                                            // Don't break for recoverable errors
                                        } else {
                                            let _ = tx.send(ChatEvent::Error(message));
                                            break;
                                        }
                                    }
                                }
                            }
                            crate::blade_ws_client::BladeWsEvent::Disconnected => {
                                eprintln!("[CHAT MGR] Disconnected - session will be restored from database on reconnect");
                                if authenticated && (saw_chat_done || saw_content) {
                                    let _ = tx.send(ChatEvent::Done);
                                } else {
                                    let _ = tx.send(ChatEvent::Error(
                                        "Server disconnected - reconnecting will restore session"
                                            .to_string(),
                                    ));
                                }
                                break;
                            }
                            crate::blade_ws_client::BladeWsEvent::Progress {
                                message,
                                stage,
                                percent,
                            } => {
                                eprintln!("[CHAT MGR] Progress: {} ({}%)", message, percent);
                                let _ = tx.send(ChatEvent::Progress {
                                    message,
                                    stage,
                                    percent: percent as i32,
                                });
                            }
                            crate::blade_ws_client::BladeWsEvent::Research { content } => {
                                eprintln!(
                                    "[CHAT MGR] Research result received ({} chars)",
                                    content.len()
                                );
                                saw_content = true;
                                
                                // Simple base name - timestamp will be added automatically when saving
                                let suggested_name = "research.md".to_string();
                                
                                let _ = tx.send(ChatEvent::Research {
                                    content,
                                    suggested_name,
                                });
                            }
                            crate::blade_ws_client::BladeWsEvent::ToolActivity {
                                tool_name,
                                file_path,
                                action,
                            } => {
                                eprintln!(
                                    "[CHAT MGR] Tool Activity: {} on {} ({})",
                                    tool_name, file_path, action
                                );
                                let _ = tx.send(ChatEvent::ToolActivity(
                                    crate::protocol::ToolActivityPayload {
                                        tool_name,
                                        file_path,
                                        action,
                                    },
                                ));
                            }
                            crate::blade_ws_client::BladeWsEvent::GetConversationContext {
                                request_id,
                                session_id: req_session_id,
                            } => {
                                let t0 = std::time::Instant::now();
                                eprintln!(
                                    "[CHAT MGR] T+{:?} GetConversationContext event received",
                                    t0.elapsed()
                                );
                                eprintln!(
                                    "[CHAT MGR] T+{:?} session_id={}, message_count={}",
                                    t0.elapsed(),
                                    req_session_id,
                                    conversation_messages.len()
                                );

                                // RFC-002: Send conversation context back to server
                                // Use the pre-cloned conversation messages in BladeMessage format

                                eprintln!(
                                    "[CHAT MGR] T+{:?} Calling send_conversation_context with {} messages...",
                                    t0.elapsed(),
                                    conversation_messages.len()
                                );
                                let result = ws_client
                                    .send_conversation_context(
                                        request_id.clone(),
                                        req_session_id.clone(),
                                        conversation_messages.clone(),
                                    )
                                    .await;
                                eprintln!(
                                    "[CHAT MGR] T+{:?} send_conversation_context returned: {:?}",
                                    t0.elapsed(),
                                    result.is_ok()
                                );

                                if let Err(e) = result {
                                    eprintln!(
                                        "[CHAT MGR] Failed to send conversation context: {}",
                                        e
                                    );
                                }
                            }
                        }

                        if !authenticated {
                            continue;
                        }
                    }
                    eprintln!("[CHAT MGR] Event loop ended");
                }
                Err(e) => {
                    eprintln!("[CHAT MGR] WebSocket connection failed: {}", e);
                    let _ = tx.send(ChatEvent::Error(e));
                }
            }
            
            eprintln!("[CHAT MGR] Async task completed, keeping WebSocket open for tool results");
            // Don't close the connection here - it will be reused for tool results
            // Connection will be closed when a new message starts or conversation ends
        });

        // Try to receive session_id (non-blocking)
        if let Ok(new_session_id) = session_rx.try_recv() {
            eprintln!("[CHAT MGR] Captured session_id: {}", new_session_id);
            self.session_id = Some(new_session_id);
        }

        // Update session_id if we got one
        // Note: This is a limitation - we can't update it from the thread
        // Will need to handle this differently in production

        // Push placeholder for assistant response
        conversation.push(ChatMessage::new(ChatRole::Assistant, String::new()));

        self.rx = Some(rx);
        self.streaming = true;
        self.abort_handle = Some(task.abort_handle());
        Ok(())
    }

    fn start_ollama_stream(
        &mut self,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        http: reqwest::Client,
        workspace: Option<&PathBuf>,
        active_file: Option<String>,
    ) -> Result<(), String> {
        let model_name = model_id
            .strip_prefix("ollama/")
            .unwrap_or(model_id)
            .to_string();

        let workspace_root = workspace
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let active_file_value = active_file.unwrap_or_default();
        let os_value = std::env::consts::OS.to_string();
        let shell_value = std::env::var("SHELL").unwrap_or_default();

        let mut messages: Vec<OllamaMessage> = Vec::new();
        if let Ok(Some(prompt)) = crate::config::read_prompt_for_model(&model_name) {
            let rendered_prompt = prompt
                .replace("{{WORKSPACE_ROOT}}", &workspace_root)
                .replace("{{ACTIVE_FILE}}", &active_file_value)
                .replace("{{OS}}", &os_value)
                .replace("{{SHELL}}", &shell_value);
            if !rendered_prompt.trim().is_empty() {
                messages.push(OllamaMessage {
                    role: "system".to_string(),
                    content: Some(rendered_prompt),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_name: None,
                });
            }
        }

        let mut tool_name_by_id: HashMap<String, String> = HashMap::new();
        for msg in conversation.get_messages() {
            if let Some(tool_calls) = msg.tool_calls.as_ref() {
                for call in tool_calls {
                    tool_name_by_id.insert(call.id.clone(), call.function.name.clone());
                }
            }
        }

        for msg in conversation.get_messages() {
            let (role, content, tool_call_id) = match msg.role {
                ChatRole::User => ("user", Some(msg.content.clone()), None),
                ChatRole::Assistant => {
                    let content = if msg.content.trim().is_empty() {
                        None
                    } else {
                        Some(msg.content.clone())
                    };
                    ("assistant", content, None)
                }
                ChatRole::System => ("system", Some(msg.content.clone()), None),
                ChatRole::Tool => ("tool", Some(msg.content.clone()), msg.tool_call_id.clone()),
            };
            if content.as_deref().unwrap_or("").trim().is_empty()
                && msg.role != ChatRole::Assistant
                && msg.role != ChatRole::Tool
            {
                continue;
            }

            let tool_calls = if msg.role == ChatRole::Assistant {
                msg.tool_calls.as_ref().map(|calls| {
                    calls
                        .iter()
                        .enumerate()
                        .map(|(index, call)| {
                            let args = serde_json::from_str(&call.function.arguments)
                                .unwrap_or(Value::String(call.function.arguments.clone()));
                            OllamaToolCall {
                                id: if call.id.is_empty() {
                                    uuid::Uuid::new_v4().to_string()
                                } else {
                                    call.id.clone()
                                },
                                typ: if call.typ.is_empty() {
                                    "function".to_string()
                                } else {
                                    call.typ.clone()
                                },
                                function: OllamaToolFunction {
                                    name: call.function.name.clone(),
                                    arguments: args,
                                    index: Some(index),
                                },
                            }
                        })
                        .collect()
                })
            } else {
                None
            };

            let tool_name = if msg.role == ChatRole::Tool {
                tool_call_id
                    .as_ref()
                    .and_then(|id| tool_name_by_id.get(id).cloned())
            } else {
                None
            };

            messages.push(OllamaMessage {
                role: role.to_string(),
                content,
                tool_calls,
                tool_call_id,
                tool_name,
            });
        }

        let request = OllamaChatRequest {
            model: model_name.clone(),
            messages,
            stream: true,
            tools: Some(get_tool_definitions()),
        };

        let (tx, rx) = mpsc::channel();
        let url = format!(
            "{}/api/chat",
            api_config.ollama_url.trim_end_matches('/')
        );

        let task = tokio::spawn(async move {
            // CRITICAL FIX: Only use reasoning parser for models that actually support reasoning tags.
            // The reasoning parser looks for <think> and <thinking> tags in the response.
            // If we run ALL text through it, regular content with angle brackets (HTML, XML, code)
            // gets misinterpreted as reasoning tags, causing garbled output.
            // Only models like DeepSeek R1, Qwen QwQ, MiniMax, and Kimi use these tags.
            let model_lower = model_name.to_lowercase();
            let supports_reasoning = model_lower.contains("deepseek")
                || model_lower.contains("qwq")
                || model_lower.contains("minimax")
                || model_lower.contains("kimi")
                || model_lower.contains("r1");
            
            let mut reasoning_parser = if supports_reasoning {
                Some(ReasoningParser::new())
            } else {
                None
            };
            
            let response = match http.post(&url).json(&request).send().await {
                Ok(res) => res,
                Err(e) => {
                    let _ = tx.send(ChatEvent::Error(format!(
                        "Ollama request failed: {}",
                        e
                    )));
                    return;
                }
            };

            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut saw_done = false;

            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(data) => data,
                    Err(e) => {
                        let _ = tx.send(ChatEvent::Error(format!(
                            "Ollama stream error: {}",
                            e
                        )));
                        return;
                    }
                };

                let text = match std::str::from_utf8(&bytes) {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = tx.send(ChatEvent::Error(format!(
                            "Ollama response decode error: {}",
                            e
                        )));
                        return;
                    }
                };

                buffer.push_str(text);
                while let Some(idx) = buffer.find('\n') {
                    let line = buffer[..idx].trim().to_string();
                    buffer = buffer[idx + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    let parsed: OllamaChatChunk = match serde_json::from_str(&line) {
                        Ok(chunk) => chunk,
                        Err(e) => {
                            eprintln!("[OLLAMA CHAT] Failed to parse chunk: {}", e);
                            continue;
                        }
                    };

                    if let Some(err) = parsed.error {
                        let _ = tx.send(ChatEvent::Error(format!(
                            "Ollama error: {}",
                            err
                        )));
                        return;
                    }

                    if let Some(msg) = parsed.message {
                        if let Some(content) = msg.content {
                            if !content.is_empty() {
                                // Only parse reasoning tags if the model supports them
                                if let Some(ref mut parser) = reasoning_parser {
                                    for segment in parser.process_segments(&content) {
                                        match segment {
                                            crate::reasoning_parser::ReasoningSegment::Text(text) => {
                                                if !text.is_empty() {
                                                    let _ = tx.send(ChatEvent::Chunk(text));
                                                }
                                            }
                                            crate::reasoning_parser::ReasoningSegment::Reasoning(reasoning) => {
                                                if !reasoning.is_empty() {
                                                    let _ = tx.send(ChatEvent::ReasoningChunk(reasoning));
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    // No reasoning parser - send content directly
                                    let _ = tx.send(ChatEvent::Chunk(content));
                                }
                            }
                        }

                        if let Some(tool_calls) = msg.tool_calls {
                            let calls: Vec<ToolCall> = tool_calls
                                .into_iter()
                                .map(|call| ToolCall {
                                    id: if call.id.is_empty() {
                                        uuid::Uuid::new_v4().to_string()
                                    } else {
                                        call.id
                                    },
                                    typ: if call.typ.is_empty() {
                                        "function".to_string()
                                    } else {
                                        call.typ
                                    },
                                    function: ToolFunction {
                                        name: call.function.name,
                                        arguments: serde_json::to_string(&call.function.arguments)
                                            .unwrap_or_default(),
                                    },
                                    status: Some("executing".to_string()),
                                    result: None,
                                })
                                .collect();
                            if !calls.is_empty() {
                                let _ = tx.send(ChatEvent::ToolCalls(calls));
                            }
                        }
                    }

                    if parsed.done.unwrap_or(false) {
                        let _ = tx.send(ChatEvent::Done);
                        saw_done = true;
                        return;
                    }
                }
            }

            if !saw_done {
                let _ = tx.send(ChatEvent::Done);
            }
        });

        conversation.push(ChatMessage::new(ChatRole::Assistant, String::new()));
        self.rx = Some(rx);
        self.streaming = true;
        self.abort_handle = Some(task.abort_handle());
        Ok(())
    }

    pub fn continue_tool_batch(
        &mut self,
        batch: PendingToolBatch,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        models: &[ModelInfo],
        selected_model: usize,
        workspace: Option<&PathBuf>,
        http: reqwest::Client,
    ) -> Result<(), String> {
        // RFC: Large Tool Result Handling - determine if we should truncate locally
        let is_local_mode = workspace
            .map(|ws| {
                let settings = crate::project_settings::load_project_settings_or_default(ws);
                matches!(settings.storage.mode, crate::project_settings::StorageMode::Local)
            })
            .unwrap_or(true); // Default to local mode if no workspace
        // Agentic Loop Check
        if self.agentic_loop.is_active() {
            self.agentic_loop.increment_turn();
            if !self.agentic_loop.is_active() {
                return Err("Agentic loop stopped: max turns reached".to_string());
            }
        }

        // Store tool results in conversation history
        // RFC: Large Tool Result Handling - truncate in local mode
        for (_call, result) in batch.file_results.iter() {
            let content = if is_local_mode {
                result.to_tool_content_truncated()
            } else {
                result.to_tool_content()
            };
            let mut tool_msg = ChatMessage::new(ChatRole::Tool, content);
            tool_msg.tool_call_id = Some(_call.id.clone());
            conversation.push(tool_msg);
        }

        // Update tool call status in the assistant message and store for emission
        // RFC: Large Tool Result Handling - truncate in local mode
        let updated_assistant = conversation.update_tool_call_status_with_truncation(&batch.file_results, is_local_mode);
        self.updated_assistant_message = updated_assistant;

        let is_ollama = models
            .get(selected_model)
            .and_then(|m| m.provider.as_deref())
            .map(|provider| provider == "ollama")
            .unwrap_or(false);

        if is_ollama {
            let model_id = models
                .get(selected_model)
                .map(|m| m.api_id.as_ref().unwrap_or(&m.id).clone())
                .unwrap_or_else(|| "ollama/unknown".to_string());
            return self.start_ollama_stream(
                conversation,
                api_config,
                &model_id,
                http,
                workspace,
                None,
            );
        }

        // Send tool results to Blade Protocol via WebSocket
        // We need to send ALL tool results, not just the first one
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| "No session ID available".to_string())?;

        eprintln!(
            "[CHAT MGR] Sending {} tool results via WebSocket",
            batch.file_results.len()
        );

        // Reuse existing WebSocket client from start_stream
        let ws_client = self
            .ws_client
            .as_ref()
            .ok_or_else(|| "No WebSocket client available".to_string())?
            .clone();
        let results = batch.file_results.clone(); // Clone for the task
        let is_local_mode_clone = is_local_mode; // Clone for async task

        // RFC-002: Clone conversation messages for local storage mode context retrieval
        // Convert to BladeMessage format that zcoderd expects
        // Note: No longer needed since we're not creating a new connection
        let _conversation_messages: Vec<serde_json::Value> = conversation
            .get_messages()
            .iter()
            .map(|msg| {
                let role = match msg.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::System => "system",
                    ChatRole::Tool => "tool",
                };
                let mut blade_msg = serde_json::json!({
                    "role": role,
                    "content": msg.content,
                });
                if let Some(ref reasoning) = msg.reasoning {
                    blade_msg["reasoning"] = serde_json::json!(reasoning);
                }
                if let Some(ref tool_call_id) = msg.tool_call_id {
                    blade_msg["tool_call_id"] = serde_json::json!(tool_call_id);
                }
                if let Some(ref tool_calls) = msg.tool_calls {
                    let tc_json: Vec<serde_json::Value> = tool_calls
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": tc.typ,
                                "function": {
                                    "name": tc.function.name,
                                    "arguments": tc.function.arguments
                                }
                            })
                        })
                        .collect();
                    blade_msg["tool_calls"] = serde_json::json!(tc_json);
                }
                blade_msg
            })
            .collect();

        // Send tool results through the existing WebSocket connection
        // No need to create a new connection - reuse the one from start_stream
        eprintln!("[CHAT MGR] Sending {} tool results through existing connection", results.len());
        
        tokio::spawn(async move {
            // Send ALL results sequentially
            // RFC: Large Tool Result Handling - truncate in local mode
            for (call, result) in &results {
                let tool_content = if is_local_mode_clone {
                    result.to_tool_content_truncated()
                } else {
                    result.to_tool_content()
                };
                eprintln!(
                    "[TOOL RESULT SEND] call_id={}, success={}",
                    call.id, result.success
                );

                let tool_result = crate::blade_ws_client::ToolResult {
                    success: result.success,
                    content: tool_content,
                    error: if result.success {
                        None
                    } else {
                        Some("Tool execution failed".to_string())
                    },
                };

                if let Err(e) = ws_client
                    .send_tool_result(
                        session_id.clone(),
                        call.id.clone(),
                        tool_result,
                    )
                    .await
                {
                    eprintln!(
                        "[CHAT MGR] Failed to send tool result {}: {}",
                        call.id, e
                    );
                }
            }
            eprintln!("[CHAT MGR] All {} tool results sent", results.len());
        });

        // The existing rx from start_stream will continue to receive events
        // No need to create a new rx or set streaming again
        Ok(())
    }

    pub fn drain_events(
        &mut self,
        conversation: &mut ConversationHistory,
        models: &[ModelInfo],
        selected_model: usize,
    ) -> DrainResult {
        // v1.1 BATCHING FIX: Process pending results first
        if let Some(res) = self.pending_results.pop_front() {
            return res;
        }

        let Some(rx) = self.rx.as_ref() else {
            return DrainResult::None;
        };

        // Aggressively drain all available events from the channel
        let mut events = Vec::new();
        let mut channel_closed = false;
        loop {
            match rx.try_recv() {
                Ok(ev) => events.push(ev),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    channel_closed = true;
                    break;
                }
            }
        }

        // If channel is closed, clear rx so orchestrator knows we're done
        if channel_closed {
            eprintln!("[DRAIN] Channel closed, clearing rx");
            self.rx = None;
        }

        if events.is_empty() {
            return DrainResult::None;
        }

        let model_id = models
            .get(selected_model)
            .map(|m| m.id.to_lowercase())
            .unwrap_or_default();
        let is_openai_text = model_id.contains("openai")
            || model_id.contains("gpt-5.2")
            || model_id.contains("codex");

        let mut batched_chunk = String::new();
        let mut done = false;
        let mut error_msg: Option<String> = None;

        // Helper macro to flush current batch
        macro_rules! flush_batch {
            () => {
                if !batched_chunk.is_empty() {
                    let s = batched_chunk.clone();

                    if let Some(assistant_msg) = conversation.last_assistant_mut() {
                        if is_openai_text {
                            assistant_msg.content.push_str(&s);
                        } else {
                            if assistant_msg.tool_calls.is_some()
                                && assistant_msg.content_before_tools.is_some()
                            {
                                let after = assistant_msg
                                    .content_after_tools
                                    .get_or_insert_with(String::new);
                                if !xml_parser::is_xml_tool_output(&s) {
                                    after.push_str(&s);
                                }
                                self.process_incoming_chunk(&s, assistant_msg);
                            } else {
                                self.process_incoming_chunk(&s, assistant_msg);
                            }
                        }
                        self.pending_results
                            .push_back(DrainResult::Update(assistant_msg.clone(), s));
                    } else {
                        conversation.push(ChatMessage::new(ChatRole::Assistant, String::new()));
                        if let Some(new_last) = conversation.last_mut() {
                            if is_openai_text {
                                new_last.content.push_str(&s);
                            } else {
                                self.process_incoming_chunk(&s, new_last);
                            }
                            self.pending_results
                                .push_back(DrainResult::Update(new_last.clone(), s));
                        }
                    }
                    batched_chunk.clear();
                }
            };
        }

        for ev in events {
            match ev {
                ChatEvent::Chunk(s) => {
                    // Set streaming=true when we receive first content
                    if !self.streaming {
                        self.streaming = true;
                    }
                    if let Some(last) = conversation.last_assistant_mut() {
                        if last.progress.is_some() {
                            // Clear progress state as text generation has started
                            last.progress = None;
                            // Notify frontend to hide progress bar
                            self.pending_results.push_back(DrainResult::Progress {
                                message: "Generated".to_string(),
                                stage: "complete".to_string(),
                                percent: 100,
                            });
                        }
                    }
                    batched_chunk.push_str(&s);
                }
                ChatEvent::ToolActivity(payload) => {
                    flush_batch!();
                    self.pending_results.push_back(DrainResult::ToolActivity {
                        tool_name: payload.tool_name,
                        file_path: payload.file_path,
                        action: payload.action,
                    });
                }
                ChatEvent::ReasoningChunk(s) => {
                    // Set streaming=true when we receive first reasoning content
                    if !self.streaming {
                        self.streaming = true;
                    }
                    if let Some(last) = conversation.last_assistant_mut() {
                        if last.progress.is_some() {
                            last.progress = None;
                            self.pending_results.push_back(DrainResult::Progress {
                                message: "Thinking".to_string(),
                                stage: "complete".to_string(),
                                percent: 100,
                            });
                        }
                    }
                    flush_batch!();
                    if let Some(assistant_msg) = conversation.last_assistant_mut() {
                        let r = assistant_msg.reasoning.get_or_insert_with(String::new);
                        r.push_str(&s);
                        self.pending_results
                            .push_back(DrainResult::Reasoning(assistant_msg.clone(), s));
                    }
                }

                other => {
                    flush_batch!();

                    match other {
                        ChatEvent::Session { session_id, model } => {
                            eprintln!("[CHAT MGR] Storing session_id: {}", session_id);
                            self.session_id = Some(session_id);
                            let _ = model;
                        }
                        ChatEvent::Research {
                            content,
                            suggested_name,
                        } => {
                            if let Some(last) = conversation.last_mut() {
                                if last.role == ChatRole::Assistant {
                                    last.content = "✅ **Research complete!**\n\n📄 Results opened in new editor tab above.".to_string();
                                    last.progress = None;
                                }
                            }
                            self.pending_results.push_back(DrainResult::Research {
                                content,
                                suggested_name,
                            });
                        }
                        ChatEvent::ToolCalls(calls) => {
                            self.accumulated_tool_calls.extend(calls.clone());
                            let calls_for_emit = calls.clone();

                            if let Some(last) = conversation.last_assistant_mut() {
                                eprintln!("[TOOL CALLS] Adding {} tool calls", calls.len());
                                if last.content_before_tools.is_none() {
                                    last.content_before_tools = Some(last.content.clone());
                                }
                                let existing = last.tool_calls.get_or_insert_with(Vec::new);
                                existing.extend(calls);

                                self.pending_results.push_back(DrainResult::ToolCreated(
                                    last.clone(),
                                    calls_for_emit,
                                ));
                            }
                        }
                        ChatEvent::Progress {
                            message,
                            stage,
                            percent,
                        } => {
                            if let Some(last) = conversation.last_mut() {
                                if last.role == ChatRole::Assistant {
                                    last.progress = Some(crate::protocol::ProgressInfo {
                                        message: message.clone(),
                                        stage: stage.clone(),
                                        percent,
                                    });
                                }
                            }
                            self.pending_results.push_back(DrainResult::Progress {
                                message,
                                stage,
                                percent,
                            });
                        }
                        ChatEvent::TodoUpdated(todos) => {
                            eprintln!("[DRAIN] Todo updated: {} items", todos.len());
                            self.pending_results
                                .push_back(DrainResult::TodoUpdated(todos));
                        }
                        ChatEvent::Done => {
                            // NOTE: Do NOT clear rx here.
                            // ChatEvent::Done can mean either:
                            // - true end-of-turn (final assistant response)
                            // - boundary where the model is yielding to tools and will send more ToolCall events
                            // We decide whether to clear rx / emit MessageCompleted later, once we know if
                            // there are accumulated tool calls.
                            eprintln!("[DRAIN] ChatEvent::Done received");
                            
                            // Ensure progress is cleared on done
                            if let Some(last) = conversation.last_assistant_mut() {
                                if last.progress.is_some() {
                                    last.progress = None;
                                    self.pending_results.push_back(DrainResult::Progress {
                                        message: "Complete".to_string(),
                                        stage: "complete".to_string(),
                                        percent: 100,
                                    });
                                }
                            }

                            // Flush any remaining XML buffer content
                            if !self.xml_buffer.is_empty() {
                                if let Some(last) = conversation.last_mut() {
                                    if !xml_parser::is_xml_tool_output(&self.xml_buffer) {
                                        last.content.push_str(&self.xml_buffer);
                                    }
                                }
                                self.xml_buffer.clear();
                            }

                            // Handler Qwen fallback
                            let current_model =
                                models.get(selected_model).map(|m| &m.id[..]).unwrap_or("");
                            if current_model.to_lowercase().contains("qwen")
                                && conversation
                                    .last()
                                    .map(|m| m.role == ChatRole::Assistant)
                                    .unwrap_or(false)
                                && self.accumulated_tool_calls.is_empty()
                            {
                                if let Some(last) = conversation.last() {
                                    if let Some(xml_calls) =
                                        xml_parser::detect_xml_tool_calls(&last.content)
                                    {
                                        eprintln!(
                                            "[QWEN] Detected {} XML tool calls",
                                            xml_calls.len()
                                        );
                                        self.accumulated_tool_calls
                                            .extend(self.convert_xml_calls(xml_calls));
                                    }
                                }
                            }

                            eprintln!("[DRAIN] chat_done received");
                            done = true;
                        }
                        ChatEvent::Error(e) => {
                            // Ensure progress is cleared on error
                            if let Some(last) = conversation.last_assistant_mut() {
                                if last.progress.is_some() {
                                    last.progress = None;
                                    // Don't emit 'complete' here, let Layout handle 'chat-error' or we can emit error state if needed
                                    // Layout.tsx listens for 'chat-error' to clear, so we just update backend state.
                                }
                            }
                            error_msg = Some(e);
                            done = true;
                        }
                        ChatEvent::ContextLengthExceeded { message, token_count, max_tokens, excess, recoverable, recovery_hint } => {
                            // RFC: Context Length Recovery - emit the event to frontend
                            eprintln!("[DRAIN] Context length exceeded: {} (tokens: {:?}/{:?}, recoverable: {})", 
                                message, token_count, max_tokens, recoverable);
                            self.pending_results.push_back(DrainResult::ContextLengthExceeded {
                                message,
                                token_count,
                                max_tokens,
                                excess,
                                recoverable,
                                recovery_hint,
                            });
                            // Don't set done=true - session is still valid, user can continue
                        }
                        _ => {}
                    }
                }
            }
        }

        flush_batch!();

        if done {
            let tool_calls = if !self.accumulated_tool_calls.is_empty() {
                eprintln!(
                    "[DRAIN] Found {} accumulated tool calls",
                    self.accumulated_tool_calls.len()
                );
                Some(self.accumulated_tool_calls.clone())
            } else {
                eprintln!("[DRAIN] No accumulated tool calls");
                None
            };
            self.accumulated_tool_calls.clear();

            // If there are no tool calls, this is a true end-of-turn.
            // Only then should we clear rx and emit MessageCompleted (frontend uses this to reset loading).
            let should_complete_turn = tool_calls.is_none() && error_msg.is_none();
            if should_complete_turn {
                eprintln!("[DRAIN] Turn complete: clearing rx + emitting MessageCompleted");
                self.rx = None;

                let msg_id = conversation.last().and_then(|msg| {
                    if msg.role == ChatRole::Assistant {
                        msg.id
                            .clone()
                            .or_else(|| Some(uuid::Uuid::new_v4().to_string()))
                    } else {
                        None
                    }
                });
                if let Some(id) = msg_id {
                    self.pending_results
                        .push_back(DrainResult::MessageCompleted(id));
                }
            } else {
                eprintln!("[DRAIN] Done received but tool calls pending: keeping rx open");
            }

            eprintln!(
                "[DRAIN] Calling finalize_turn with tool_calls: {:?}",
                tool_calls.as_ref().map(|c| c.len())
            );
            self.finalize_turn(
                conversation,
                tool_calls.clone(),
                &error_msg,
                models,
                selected_model,
            );

            // Set streaming=false to reduce CPU usage during tool execution.
            // IMPORTANT: Do NOT clear rx if we expect more events (e.g., additional tool calls).
            self.streaming = false;
            
            // Clear reasoning parser since this turn is done
            self.reasoning_parser.reset();

            if let Some(msg) = error_msg {
                self.pending_results.push_back(DrainResult::Error(msg));
            } else if let Some(calls) = tool_calls {
                let content = conversation.last().map(|m| m.content.clone());
                self.pending_results
                    .push_back(DrainResult::ToolCalls(calls, content));
            }
        }

        if let Some(msg) = self.updated_assistant_message.take() {
            self.pending_results
                .push_back(DrainResult::ToolStatusUpdate(msg));
        }

        let result = self
            .pending_results
            .pop_front()
            .unwrap_or(DrainResult::None);
        eprintln!(
            "[DRAIN] Returning result: {:?}",
            std::mem::discriminant(&result)
        );
        result
    }

    fn process_incoming_chunk(&mut self, chunk: &str, last_msg: &mut ChatMessage) {
        // v1.2: Use ReasoningParser for multi-format reasoning extraction
        let result = self.reasoning_parser.process(chunk);

        // Append text content (non-reasoning)
        if !result.text.is_empty() {
            self.append_content(&result.text, last_msg);
        }

        // Append reasoning content
        if !result.reasoning.is_empty() {
            let r = last_msg.reasoning.get_or_insert_with(String::new);
            r.push_str(&result.reasoning);
        }
    }

    fn append_content(&mut self, text: &str, last_msg: &mut ChatMessage) {
        // XML buffering for tool call detection (Qwen/GLM models)
        // Only buffer if we're actively building an XML tag, not for stray < or > in normal text
        if !self.xml_buffer.is_empty() {
            // We're already buffering - continue until we find a closing tag or give up
            self.xml_buffer.push_str(text);

            // Check for known closing tags
            if self.xml_buffer.contains("</tool_call>") || self.xml_buffer.contains("</invoke>") {
                if let Some(status) = xml_parser::xml_to_status_message(&self.xml_buffer) {
                    last_msg.content.push_str(&status);
                    last_msg.content.push('\n');
                } else if !xml_parser::is_xml_tool_output(&self.xml_buffer) {
                    last_msg.content.push_str(&self.xml_buffer);
                }
                self.xml_buffer.clear();
            } else if self.xml_buffer.len() > 500 {
                // Buffer too large without finding closing tag - flush it as normal text
                last_msg.content.push_str(&self.xml_buffer);
                self.xml_buffer.clear();
            }
        } else if text.starts_with("<tool_call") || text.starts_with("<invoke") {
            // Start buffering only if this looks like an actual tool call tag
            self.xml_buffer.push_str(text);
        } else {
            // Normal text - append directly even if it contains < or >
            last_msg.content.push_str(text);
        }
    }

    fn convert_xml_calls(&self, xml_calls: Vec<crate::xml_parser::XmlToolCall>) -> Vec<ToolCall> {
        xml_calls
            .into_iter()
            .enumerate()
            .map(|(idx, call)| {
                let mut args = serde_json::Map::new();
                for (k, v) in call.parameters {
                    args.insert(k, serde_json::Value::String(v));
                }
                ToolCall {
                    id: format!("call_xml_{}", idx),
                    typ: "function".to_string(),
                    function: crate::protocol::ToolFunction {
                        name: call.name,
                        arguments: serde_json::to_string(&args).unwrap_or_default(),
                    },
                    status: Some("executing".to_string()),
                    result: None,
                }
            })
            .collect()
    }

    fn finalize_turn(
        &mut self,
        conversation: &mut ConversationHistory,
        tool_calls: Option<Vec<ToolCall>>,
        error_msg: &Option<String>,
        models: &[ModelInfo],
        selected_model: usize,
    ) {
        let is_qwen = models
            .get(selected_model)
            .map(|m| {
                let id = m.id.to_lowercase();
                id.contains("qwen") || id.contains("mercury")
            })
            .unwrap_or(false);

        let has_tool_calls = tool_calls.as_ref().map(|t| !t.is_empty()).unwrap_or(false);

        // 1. Agentic Loop Logic
        if self.agentic_loop.is_active() {
            if has_tool_calls {
                // Good, continuing
            } else {
                // Text response
                if let Some(last) = conversation.last() {
                    if last.role == ChatRole::Assistant && !last.content.trim().is_empty() {
                        self.agentic_loop.stop("text-only response, task complete");
                    } else {
                        // Empty response and no tool calls?
                        self.agentic_loop.stop("empty response");
                    }
                }
            }
        } else if (is_qwen) && has_tool_calls {
            // Auto-start loop for Qwen if tools are used
            eprintln!("[AGENTIC LOOP] Auto-starting for tool execution");
            self.agentic_loop.start();
        }

        // 2. Add tool calls to history
        if let Some(last) = conversation.last_assistant_mut() {
            if let Some(calls) = tool_calls.clone() {
                eprintln!(
                    "[FINALIZE] Adding {} tool calls to Assistant message",
                    calls.len()
                );
                last.tool_calls = Some(calls);
            }

            if last.content.trim().is_empty() && last.tool_calls.is_none() && error_msg.is_none() {
                // If barely anything happened, mark it
                last.content = "[no content]".to_string();
            }
        }
    }

    /// Request to stop the current streaming response
    pub fn request_stop(&mut self) -> bool {
        if let Some(handle) = self.abort_handle.take() {
            handle.abort();
            self.streaming = false;
            self.rx = None;
            self.reasoning_parser.reset();
            // Also stop agentic loop
            self.agentic_loop.stop("User requested stop");
            true
        } else {
            false
        }
    }

    /// Check if a stream can be stopped
    pub fn is_stoppable(&self) -> bool {
        self.streaming && self.abort_handle.is_some()
    }

    pub fn handle_tool_calls(
        &self,
        calls: Vec<ToolCall>,
        content: Option<String>,
        workspace: &PathBuf,
        active_file: Option<String>,
        ai: &mut AiWorkflow,
    ) -> Option<PendingToolBatch> {
        let context = crate::tool_execution::ToolExecutionContext::<tauri::Wry> {
            workspace_root: Some(workspace.to_string_lossy().to_string()),
            active_file,
            active_tab_index: 0,
            open_files: vec![],
            cursor_line: None,
            cursor_column: None,
            selection_start_line: None,
            selection_end_line: None,
            app_handle: None,
        };
        ai.handle_tool_calls(workspace, calls, content, &context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ApiConfig;
    use crate::protocol::{ChatMessage, ChatRole};

    #[test]
    fn test_start_stream_adds_assistant_placeholder() {
        let mut chat_manager = ChatManager::new(10);
        let mut conversation = ConversationHistory::new();
        conversation.push(ChatMessage::new(ChatRole::User, "Test".to_string()));

        let api_config = ApiConfig {
            api_key: "test_key".to_string(),
            ..Default::default()
        };
        let models = vec![];
        let http = reqwest::Client::new();

        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            // We expect it to launch (and maybe fail network, but that's async)
            // start_stream returns Result<(), String>
            let _ = chat_manager.start_stream(
                "prompt".to_string(),
                &mut conversation,
                &api_config,
                &models,
                0,
                None, // workspace
                None, // active_file
                None, // open_files
                None, // cursor_line
                None, // cursor_column
                http,
                None, // storage_mode
            );

            // Verify conversation has Assistant placeholder
            assert_eq!(
                conversation.len(),
                2,
                "Conversation should have 2 messages (User + Assistant placeholder)"
            );
            assert_eq!(
                conversation.last().unwrap().role,
                ChatRole::Assistant,
                "Last message should be Assistant"
            );
            assert!(
                conversation.last().unwrap().content.is_empty(),
                "Placeholder should be empty"
            );
        });
    }
}
