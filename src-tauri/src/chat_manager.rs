// use eframe::egui; // Removed
use std::path::PathBuf;
use std::sync::mpsc;

use crate::agentic_loop::AgenticLoop;
use crate::ai_workflow::{AiWorkflow, PendingToolBatch};
use crate::config::ApiConfig;
use crate::conversation::ConversationHistory;
use crate::models::registry::ModelInfo;
use crate::protocol::ToolFunction;
use crate::protocol::{ChatEvent, ChatMessage, ChatRole, ToolCall};
// use crate::protocol::{ChatEvent, ChatMessage, ChatRole, ToolCall}; // Duplicates removed
// use crate::xml_parser; // Duplicate removed
use crate::xml_parser;

pub enum DrainResult {
    None,
    Update(ChatMessage, String), // Immediate update for streaming chunks with the delta
    Reasoning(ChatMessage, String), // Reasoning chunk delta
    Research {
        content: String,
        suggested_name: String,
    },
    ToolCalls(Vec<ToolCall>, Option<String>),
    ToolCreated(ChatMessage, Vec<ToolCall>),
    ToolStatusUpdate(ChatMessage),
    Error(String),
}

pub struct ChatManager {
    pub streaming: bool,
    pub rx: Option<mpsc::Receiver<ChatEvent>>,
    pub xml_buffer: String,
    pub in_think_block: bool,
    pub agentic_loop: AgenticLoop,
    pub session_id: Option<String>,
    abort_handle: Option<tokio::task::AbortHandle>,
    pub accumulated_tool_calls: Vec<ToolCall>,
    pub updated_assistant_message: Option<ChatMessage>,
    pub message_seq: u64, // v1.1: sequence number for MessageDelta events
    pub pending_results: std::collections::VecDeque<DrainResult>,
}

impl ChatManager {
    pub fn new(max_turns: usize) -> Self {
        Self {
            streaming: false,
            rx: None,
            xml_buffer: String::new(),
            in_think_block: false,
            agentic_loop: AgenticLoop::new(max_turns),
            session_id: None,
            abort_handle: None,
            accumulated_tool_calls: Vec::new(),
            updated_assistant_message: None,
            message_seq: 0,
            pending_results: std::collections::VecDeque::new(),
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
        _http: reqwest::Client,
    ) -> Result<(), String> {
        self.in_think_block = false;
        self.xml_buffer.clear();
        self.accumulated_tool_calls.clear();
        self.updated_assistant_message = None;
        self.message_seq = 0; // v1.1: reset sequence counter for new message

        // Get model ID
        let selected_info = models.get(selected_model);
        let model_id = selected_info
            .map(|m| {
                let id = m.api_id.as_ref().unwrap_or(&m.id).clone();
                eprintln!("[CHAT MGR] Model selection: display_id={}, api_id={:?}, sending={}", 
                    m.id, m.api_id, id);
                id
            })
            .unwrap_or_else(|| "anthropic/claude-sonnet-4-5-20250929".to_string());

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

        // Create WebSocket Blade client
        let blade_url = api_config.blade_url.clone();
        let api_key = api_config.api_key.clone();
        eprintln!("[BLADE WS] Connecting to: {}", blade_url);
        eprintln!("[BLADE WS] Sending message: {}", user_message);
        eprintln!("[BLADE WS] API key present: {}", !api_key.is_empty());

        let ws_client = crate::blade_ws_client::BladeWsClient::new(blade_url, api_key);
        let session_id = self.session_id.clone();

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
                        match event {
                            crate::blade_ws_client::BladeWsEvent::Connected { .. } => {
                                eprintln!("[CHAT MGR] Authenticated, sending chat message");
                                authenticated = true;

                                // Send chat message
                                if let Err(e) = ws_client
                                    .send_message(
                                        session_id.clone(),
                                        model_id.clone(),
                                        user_message.clone(),
                                        Some(workspace_info.clone()),
                                    )
                                    .await
                                {
                                    eprintln!("[CHAT MGR] Failed to send message: {}", e);
                                    let _ = tx.send(ChatEvent::Error(e));
                                    break;
                                }
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
                            crate::blade_ws_client::BladeWsEvent::ChatDone { finish_reason } => {
                                eprintln!("[CHAT MGR] Chat done: {}", finish_reason);
                                saw_chat_done = true;
                                let _ = tx.send(ChatEvent::Done);
                                // Don't break - keep connection alive for tool results
                                // The connection will close when the user sends a new message
                            }
                            crate::blade_ws_client::BladeWsEvent::Error { code, message } => {
                                eprintln!("[CHAT MGR] Error: {} - {}", code, message);
                                if authenticated && (saw_chat_done || saw_content) {
                                    // Treat read/disconnect-like errors after content as end of stream
                                    let _ = tx.send(ChatEvent::Done);
                                } else {
                                    let _ = tx.send(ChatEvent::Error(message));
                                }
                                break;
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

    pub fn continue_tool_batch(
        &mut self,
        batch: PendingToolBatch,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        _models: &[ModelInfo],
        _selected_model: usize,
        _workspace: Option<&PathBuf>,
        _http: reqwest::Client,
    ) -> Result<(), String> {
        // Agentic Loop Check
        if self.agentic_loop.is_active() {
            self.agentic_loop.increment_turn();
            if !self.agentic_loop.is_active() {
                return Err("Agentic loop stopped: max turns reached".to_string());
            }
        }

        // Store tool results in conversation history
        for (_call, result) in batch.file_results.iter() {
            let mut tool_msg = ChatMessage::new(ChatRole::Tool, result.to_tool_content());
            tool_msg.tool_call_id = Some(_call.id.clone());
            conversation.push(tool_msg);
        }

        // Update tool call status in the assistant message and store for emission
        let updated_assistant = conversation.update_tool_call_status(&batch.file_results);
        self.updated_assistant_message = updated_assistant;

        // Send tool results to Blade Protocol via WebSocket
        // We need to send ALL tool results, not just the first one
        let blade_url = api_config.blade_url.clone();
        let api_key = api_config.api_key.clone();
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| "No session ID available".to_string())?;

        eprintln!(
            "[CHAT MGR] Sending {} tool results via WebSocket",
            batch.file_results.len()
        );

        // Send all tool results except the last one without waiting for response
        for (call, result) in batch
            .file_results
            .iter()
            .take(batch.file_results.len().saturating_sub(1))
        {
            let ws_client = crate::blade_ws_client::BladeWsClient::new(blade_url.clone(), api_key.clone());
            let tool_content = result.to_tool_content();
            eprintln!("[TOOL RESULT SEND] call_id={}, success={}, content_len={}, content_preview={}", 
                call.id, result.success, tool_content.len(), 
                &tool_content.chars().take(200).collect::<String>());
            let tool_result = crate::blade_ws_client::ToolResult {
                success: result.success,
                content: tool_content,
                error: if result.success {
                    None
                } else {
                    Some("Tool execution failed".to_string())
                },
            };
            let call_id = call.id.clone();
            let session_id_clone = session_id.clone();

            eprintln!(
                "[CHAT MGR] Sending intermediate tool result via WebSocket: call_id={}, success={}",
                call_id, result.success
            );

            // Send this tool result without waiting for streaming response
            tokio::spawn(async move {
                match ws_client.connect().await {
                    Ok(mut ws_rx) => {
                        while let Some(event) = ws_rx.recv().await {
                            if let crate::blade_ws_client::BladeWsEvent::Connected { .. } = event {
                                if let Err(e) = ws_client
                                    .send_tool_result(
                                        session_id_clone.clone(),
                                        call_id.clone(),
                                        tool_result.clone(),
                                    )
                                    .await
                                {
                                    eprintln!(
                                        "[CHAT MGR] Failed to send intermediate tool result: {}",
                                        e
                                    );
                                }
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[CHAT MGR] Failed to connect for intermediate tool result: {}",
                            e
                        );
                    }
                }
            });
        }

        // Send the last tool result and wait for the streaming response
        if let Some((call, result)) = batch.file_results.last() {
            let ws_client = crate::blade_ws_client::BladeWsClient::new(blade_url, api_key);
            let tool_content = result.to_tool_content();
            eprintln!("[TOOL RESULT SEND FINAL] call_id={}, success={}, content_len={}, content_preview={}", 
                call.id, result.success, tool_content.len(), 
                &tool_content.chars().take(200).collect::<String>());
            let tool_result = crate::blade_ws_client::ToolResult {
                success: result.success,
                content: tool_content,
                error: if result.success {
                    None
                } else {
                    Some("Tool execution failed".to_string())
                },
            };
            let call_id = call.id.clone();

            eprintln!(
                "[CHAT MGR] Sending final tool result via WebSocket: call_id={}, success={}",
                call_id, result.success
            );

            // Convert WebSocket events to ChatEvent channel
            let (tx, rx) = mpsc::channel();

            // Spawn async task to send tool result and handle events
            tokio::spawn(async move {
                eprintln!("[CHAT MGR] Connecting to WebSocket for tool result");
                match ws_client.connect().await {
                    Ok(mut ws_rx) => {
                        // Wait for authentication
                        let mut authenticated = false;
                        let mut saw_chat_done = false;
                        let mut saw_content = false;
                        while let Some(event) = ws_rx.recv().await {
                            match event {
                                crate::blade_ws_client::BladeWsEvent::Connected { .. } => {
                                    eprintln!("[CHAT MGR] Authenticated, sending tool result");
                                    authenticated = true;

                                    // Send tool result
                                    if let Err(e) = ws_client
                                        .send_tool_result(
                                            session_id.clone(),
                                            call_id.clone(),
                                            tool_result.clone(),
                                        )
                                        .await
                                    {
                                        eprintln!("[CHAT MGR] Failed to send tool result: {}", e);
                                        let _ = tx.send(ChatEvent::Error(e));
                                        break;
                                    }
                                }
                                crate::blade_ws_client::BladeWsEvent::TextChunk(text) => {
                                    eprintln!("[CHAT MGR] Text chunk after tool: {}", text);
                                    saw_content = true;
                                    let _ = tx.send(ChatEvent::Chunk(text));
                                }
                                crate::blade_ws_client::BladeWsEvent::ReasoningChunk(text) => {
                                    eprintln!("[CHAT MGR] Reasoning chunk after tool: {}", text);
                                    saw_content = true;
                                    let _ = tx.send(ChatEvent::ReasoningChunk(text));
                                }
                                crate::blade_ws_client::BladeWsEvent::ToolCall {
                                    id,
                                    name,
                                    arguments,
                                } => {
                                    eprintln!("[CHAT MGR] Tool call after tool result: {}", name);
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
                                crate::blade_ws_client::BladeWsEvent::ChatDone {
                                    finish_reason,
                                } => {
                                    eprintln!("[CHAT MGR] Chat done after tool: {}", finish_reason);
                                    saw_chat_done = true;
                                    let _ = tx.send(ChatEvent::Done);
                                    // Don't break - keep listening for more events
                                    // Connection will close naturally or on next request
                                }
                                crate::blade_ws_client::BladeWsEvent::Error { code, message } => {
                                    eprintln!("[CHAT MGR] Error: {} - {}", code, message);
                                    if authenticated && (saw_chat_done || saw_content) {
                                        let _ = tx.send(ChatEvent::Done);
                                    } else {
                                        let _ = tx.send(ChatEvent::Error(message));
                                    }
                                    break;
                                }
                                crate::blade_ws_client::BladeWsEvent::Disconnected => {
                                    eprintln!("[CHAT MGR] Disconnected");
                                    if authenticated && (saw_chat_done || saw_content) {
                                        let _ = tx.send(ChatEvent::Done);
                                    }
                                    break;
                                }
                                _ => {}
                            }

                            if !authenticated {
                                continue;
                            }
                        }
                        eprintln!("[CHAT MGR] Event loop ended after tool result");
                    }
                    Err(e) => {
                        eprintln!("[CHAT MGR] WebSocket connection failed: {}", e);
                        let _ = tx.send(ChatEvent::Error(e));
                    }
                }
            });

            // Add assistant placeholder for next response
            conversation.push(ChatMessage::new(ChatRole::Assistant, String::new()));

            self.rx = Some(rx);
            self.streaming = true;
        }

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
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }

        if events.is_empty() {
            return DrainResult::None;
        }

        let model_id = models
            .get(selected_model)
            .map(|m| m.id.to_lowercase())
            .unwrap_or_default();
        let is_openai_text = model_id.contains("openai") || model_id.contains("gpt-5.2") || model_id.contains("codex");

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
                            if assistant_msg.tool_calls.is_some() && assistant_msg.content_before_tools.is_some() {
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
                        self.pending_results.push_back(DrainResult::Update(assistant_msg.clone(), s));
                    } else {
                        conversation.push(ChatMessage::new(ChatRole::Assistant, String::new()));
                        if let Some(new_last) = conversation.last_mut() {
                            if is_openai_text {
                                new_last.content.push_str(&s);
                            } else {
                                self.process_incoming_chunk(&s, new_last);
                            }
                            self.pending_results.push_back(DrainResult::Update(new_last.clone(), s));
                        }
                    }
                    batched_chunk.clear();
                }
            };
        }

        for ev in events {
            match ev {
                ChatEvent::Chunk(s) => {
                    batched_chunk.push_str(&s);
                }
                ChatEvent::ReasoningChunk(s) => {
                    flush_batch!();
                    if let Some(assistant_msg) = conversation.last_assistant_mut() {
                        let r = assistant_msg.reasoning.get_or_insert_with(String::new);
                        r.push_str(&s);
                        self.pending_results.push_back(DrainResult::Reasoning(assistant_msg.clone(), s));
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
                        ChatEvent::Research { content, suggested_name } => {
                            if let Some(last) = conversation.last_mut() {
                                if last.role == ChatRole::Assistant {
                                    last.content = "✅ **Research complete!**\n\n📄 Results opened in new editor tab above.".to_string();
                                    last.progress = None;
                                }
                            }
                            self.pending_results.push_back(DrainResult::Research { content, suggested_name });
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
                                
                                self.pending_results.push_back(DrainResult::ToolCreated(last.clone(), calls_for_emit));
                            }
                        }
                        ChatEvent::Progress { message, stage, percent } => {
                            if let Some(last) = conversation.last_mut() {
                                if last.role == ChatRole::Assistant {
                                    last.progress = Some(crate::protocol::ProgressInfo {
                                        message, stage, percent
                                    });
                                }
                            }
                        }
                        ChatEvent::Done => {
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
                            let current_model = models.get(selected_model).map(|m| &m.id[..]).unwrap_or("");
                            if current_model.to_lowercase().contains("qwen") 
                                && conversation.last().map(|m| m.role == ChatRole::Assistant).unwrap_or(false)
                                && self.accumulated_tool_calls.is_empty() 
                            {
                                if let Some(last) = conversation.last() {
                                    if let Some(xml_calls) = xml_parser::detect_xml_tool_calls(&last.content) {
                                        eprintln!("[QWEN] Detected {} XML tool calls", xml_calls.len());
                                        self.accumulated_tool_calls.extend(self.convert_xml_calls(xml_calls));
                                    }
                                }
                            }

                            eprintln!("[DRAIN] chat_done received");
                            done = true;
                        }
                        ChatEvent::Error(e) => {
                             error_msg = Some(e);
                             done = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        
        flush_batch!();

        if done {
            let tool_calls = if !self.accumulated_tool_calls.is_empty() {
                eprintln!("[DRAIN] Found {} accumulated tool calls", self.accumulated_tool_calls.len());
                Some(self.accumulated_tool_calls.clone())
            } else {
                eprintln!("[DRAIN] No accumulated tool calls");
                None
            };
            self.accumulated_tool_calls.clear();

            eprintln!("[DRAIN] Calling finalize_turn with tool_calls: {:?}", tool_calls.as_ref().map(|t| t.len()));
            self.finalize_turn(conversation, tool_calls.clone(), &error_msg, models, selected_model);

            self.streaming = false;
            self.rx = None;
            self.in_think_block = false;

            if let Some(msg) = error_msg {
                self.pending_results.push_back(DrainResult::Error(msg));
            } else if let Some(calls) = tool_calls {
                let content = conversation.last().map(|m| m.content.clone());
                self.pending_results.push_back(DrainResult::ToolCalls(calls, content));
            }
        }

        if let Some(msg) = self.updated_assistant_message.take() {
            self.pending_results.push_back(DrainResult::ToolStatusUpdate(msg));
        }

        self.pending_results.pop_front().unwrap_or(DrainResult::None)
    }

    fn process_incoming_chunk(&mut self, chunk: &str, last_msg: &mut ChatMessage) {
        let mut part = chunk;

        loop {
            if part.is_empty() {
                break;
            }

            if !self.in_think_block {
                if let Some(idx) = part.find("<think>") {
                    let before = &part[..idx];
                    self.append_content(before, last_msg);

                    self.in_think_block = true;
                    if last_msg.reasoning.is_none() {
                        last_msg.reasoning = Some(String::new());
                    }
                    part = &part[idx + 7..];
                } else {
                    self.append_content(part, last_msg);
                    break;
                }
            } else {
                if let Some(idx) = part.find("</think>") {
                    let reasoning = &part[..idx];
                    if let Some(r) = last_msg.reasoning.as_mut() {
                        r.push_str(reasoning);
                    }
                    self.in_think_block = false;
                    part = &part[idx + 8..];
                } else {
                    if let Some(r) = last_msg.reasoning.as_mut() {
                        r.push_str(part);
                    }
                    break;
                }
            }
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
            self.in_think_block = false;
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
        conversation.push(ChatMessage {
            role: ChatRole::User,
            content: "Test".to_string(),
            tool_calls: None,
            reasoning: None,
            tool_call_id: None,
            progress: None,
            content_before_tools: None,
            content_after_tools: None,
        });

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
                None,
                None,
                None,
                None,
                None,
                12, // cursor_line added in later steps? No, in my read it was there?
                // Wait. Lines 1000 in read showed `12`? No.
                // Let's check `start_stream` signature again.
                // Line 58-71. 
                // args: prompt, conversation, api_config, models, selected_model, workspace, active_file, open_files, cursor_line, cursor_column, http.
                // Total 11 args.
                // Test calls it with:
                // "prompt", &mut conv, &api, &models, 0, None, None, None, None, None, http.
                // That's 11 args.
                // Wait, in my written `start_stream` (Lines 58-71):
                /*
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
        _http: reqwest::Client,
                */
                // Yes.
                // In test:
                /*
                "prompt".to_string(),
                &mut conversation,
                &api_config,
                &models,
                0,
                None,
                None,
                None,
                None,
                None,
                http,
                */
                // That's 11.
                // But my test in `write_to_file` has comments?
                // I will just use `None` for cursor line/col.
                 None, http
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
