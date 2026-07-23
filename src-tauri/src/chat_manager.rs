// use eframe::egui; // Removed
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use crate::agentic_loop::AgenticLoop;
use crate::ai_workflow::get_tool_definitions_for_model;
use crate::ai_workflow::{AiWorkflow, PendingToolBatch};
use crate::blade_ws_client::BladeWsClient;
use crate::config::{normalize_openai_compat_url, ApiConfig};
use crate::conversation::ConversationHistory;
use crate::models::registry::ModelInfo;
use crate::protocol::ToolFunction;
use crate::protocol::{
    ChatEvent, ChatMessage, ChatRole, ToolActivityPayload, ToolCall, ToolCallDelta,
};
use crate::providers::{
    resolve_model_selection, AiProviderRuntime, OllamaRuntime, OpenAiCompatRuntime, ProviderEvent,
    ProviderId, ZaguanRuntime,
};
use crate::reasoning_parser::ReasoningParser;
use crate::xml_parser;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

const DISCONNECT_FLUSH_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

fn merge_tool_call_deltas(buf: &mut Vec<ToolCall>, deltas: &[ToolCallDelta]) {
    for d in deltas {
        // DEBUG: Log what we're receiving
        eprintln!(
            "[DELTA] index={}, id={:?}, type={:?}, function.name={:?}, function.args={:?}",
            d.index,
            d.id,
            d.typ,
            d.function.as_ref().and_then(|f| f.name.as_ref()),
            d.function.as_ref().and_then(|f| f.arguments.as_ref())
        );

        while buf.len() <= d.index {
            // Generate a fallback ID for models that don't provide one
            let fallback_id = format!("tool_call_{}", d.index);
            buf.push(ToolCall {
                id: fallback_id,
                typ: "function".to_string(),
                function: ToolFunction {
                    name: String::new(),
                    arguments: String::new(),
                },
                status: Some("executing".to_string()),
                result: None,
            });
        }

        let entry = &mut buf[d.index];
        if let Some(id) = &d.id {
            if !id.is_empty() {
                entry.id = id.clone();
            }
        }
        if let Some(t) = &d.typ {
            if !t.is_empty() {
                entry.typ = t.clone();
            }
        }
        if let Some(f) = &d.function {
            // CRITICAL: For Qwen models, name and arguments may arrive separately
            // We need to accumulate both, not just append arguments
            if let Some(name) = &f.name {
                if !name.is_empty() {
                    // If we have a name, set it (Qwen sends this in first chunk)
                    entry.function.name = name.clone();
                    eprintln!("[DELTA MERGE] Set function name: {}", name);
                }
            }
            if let Some(args) = &f.arguments {
                // Accumulate arguments (may come in multiple chunks)
                entry.function.arguments.push_str(args);
                eprintln!(
                    "[DELTA MERGE] Accumulated args, total length: {}",
                    entry.function.arguments.len()
                );
            }
        }
    }
}

fn is_semantically_empty_message(msg: &ChatMessage) -> bool {
    let effective_content = msg.backend_content.as_deref().unwrap_or(&msg.content);
    let has_text = !effective_content.trim().is_empty();
    let has_reasoning = msg
        .reasoning
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_tool_calls = msg
        .tool_calls
        .as_ref()
        .map(|calls| !calls.is_empty())
        .unwrap_or(false);
    let has_images = msg
        .images
        .as_ref()
        .map(|images| !images.is_empty())
        .unwrap_or(false);

    match msg.role {
        ChatRole::Assistant => !(has_text || has_reasoning || has_tool_calls),
        ChatRole::User => !(has_text || has_images),
        ChatRole::System => !has_text,
        ChatRole::Tool => !has_text,
    }
}

fn local_tool_support_cache() -> &'static Mutex<HashSet<String>> {
    static CACHE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn local_tool_support_cache_key(provider: &str, model_id: &str) -> String {
    format!("{}:{}", provider, model_id.trim().to_ascii_lowercase())
}

fn model_disables_tools(cache_key: &str) -> bool {
    local_tool_support_cache()
        .lock()
        .map(|cache| cache.contains(cache_key))
        .unwrap_or(false)
}

fn remember_model_disables_tools(cache_key: &str) {
    if let Ok(mut cache) = local_tool_support_cache().lock() {
        cache.insert(cache_key.to_string());
    }
}

const GEMMA4_THINK_TOKEN: &str = "<|think|>";
const GEMMA4_TEMPERATURE: f32 = 1.0;
const GEMMA4_TOP_P: f32 = 0.95;
const GEMMA4_TOP_K: u32 = 64;

fn is_gemma4_model(model_id: &str) -> bool {
    let model_lower = model_id.trim().to_ascii_lowercase();
    model_lower.contains("gemma4") || model_lower.contains("gemma-4")
}

fn contains_reasoning_marker(content: &str) -> bool {
    content.contains("<think>")
        || content.to_ascii_lowercase().contains("<thinking>")
        || content.contains("<|channel|>thought")
        || content.contains("<|channel>thought")
}

fn maybe_prefix_gemma4_think_token(model_id: &str, prompt: String) -> String {
    if !is_gemma4_model(model_id) {
        return prompt;
    }

    if prompt.trim_start().starts_with(GEMMA4_THINK_TOKEN) {
        prompt
    } else if prompt.trim().is_empty() {
        GEMMA4_THINK_TOKEN.to_string()
    } else {
        format!("{}\n{}", GEMMA4_THINK_TOKEN, prompt)
    }
}

fn zblade_workflow_guidance() -> &'static str {
    r#"ZBlade workflow guidance:
- For structural code navigation, use the Symbols tools before grep, broad text search, or whole-file reads. Unknown location: `symbol_search`. Known file but unknown range: `symbol_outline`. Callers of a function: `symbol_references` incoming `call` edges. Interface/trait implementers: `symbol_references` incoming `implements` edges.
- Copy exact symbol IDs from returned results — never construct one from a path and name — and read the exact returned ranges.
- Use `rg` or other text search for literal strings, regexes, unsupported file types, or after a focused Symbols miss. Do NOT use Symbols tools for builds, tests, Git, or runtime diagnosis.
- For user requests that ask you to inspect, update, edit, fix, create, rename, delete, or run project files/commands, use the provided tools immediately. Do not answer only that you are ready or ask what to do next.
- For file edits, first inspect the relevant file/context with tools, then call `apply_patch` or `write_file` with absolute paths.
- For broad, ambiguous, multi-file, or unfamiliar tasks, call `fast_context` before reading many files or editing. Use its `confidence`, `index_health`, `project_context`, `graph_context`, `suggested_ranges`, `enriched_files`, `related_files`, `related_docs`, and optional `impact` hints to plan the next reads.
- If `fast_context` returns low confidence or stale/degraded index health, do a second targeted `symbol_search`, `symbol_related`, `symbol_query`, `semantic_anchor_search`, or read the suggested ranges before editing.
- Use `symbol_path` when a task depends on explaining the strongest indexed route between two known symbols; treat its confidence, provenance, and truncation metadata as part of the result.
- Use `symbol_architecture` for subsystem boundaries, central modules, and cross-community bridge questions; treat inferred communities as graph-analysis hints rather than declared package ownership.
- Treat rationale and design-document entries in `symbol_query.semantic_context` or `semantic_anchor_search` as located evidence; follow their owner/target symbol IDs and paths before relying on them.
- Treat project-index tools as legacy fallback only; prefer `fast_context` for first-pass orientation.
- Before larger edits, refactors, public API changes, or changes to files with likely callers, inspect `fast_context.impact`; call `edit_impact` on the target file or symbol when deeper blast-radius analysis is needed. Inspect impacted files and likely tests before applying patches.
- Prefer reading only the suggested ranges first. Expand to full files only when the ranges are insufficient.
- After editing, use the likely tests and impacted files from `edit_impact` to choose focused validation."#
}

fn apply_zblade_workflow_guidance(prompt: Option<String>) -> String {
    let guidance = zblade_workflow_guidance();
    match prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        Some(prompt)
            if prompt.contains("You are an AI coding assistant in Zaguán Blade")
                || prompt.contains("You are Zaguán Blade, a senior pair programmer") =>
        {
            prompt
        }
        Some(prompt) if prompt.contains("ZBlade workflow guidance:") => prompt,
        Some(prompt) => format!("{}\n\n{}", prompt, guidance),
        None => guidance.to_string(),
    }
}

fn load_local_system_prompt(
    model_id: &str,
    workspace_root: &str,
    active_file_value: &str,
    os_value: &str,
    shell_value: &str,
    date_value: &str,
    time_value: &str,
    agent_instructions: Option<&str>,
) -> Option<String> {
    let rendered = crate::config::read_prompt_for_model(model_id)
        .ok()
        .flatten()
        .map(|prompt| {
            prompt
                .replace("{{WORKSPACE_ROOT}}", workspace_root)
                .replace("{{ACTIVE_FILE}}", active_file_value)
                .replace("{{SELECTION_OR_CURSOR}}", "Unavailable")
                .replace("{{OS}}", os_value)
                .replace("{{SHELL}}", shell_value)
                .replace("{{CURRENT_DATE}}", date_value)
                .replace("{{DATE}}", date_value)
                .replace("{{TIME}}", time_value)
                .replace("{{AVAILABLE_TOOLS}}", "Provided by Zaguán Blade at runtime")
                .replace("{{USER_PREFERENCES}}", "Unavailable")
        })
        .filter(|prompt| !prompt.trim().is_empty());

    let mut prompt =
        maybe_prefix_gemma4_think_token(model_id, apply_zblade_workflow_guidance(rendered));
    if let Some(instructions) = agent_instructions
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str("\n\n# Repository Instructions\n\n");
        prompt.push_str(instructions);
    }
    if !workspace_root.trim().is_empty() {
        let workspace_path = Path::new(workspace_root);
        if let Some(skills) =
            crate::agent_skills::render_available_skills_for_prompt(workspace_path)
        {
            prompt.push_str("\n\n");
            prompt.push_str(&skills);
        }
    }
    Some(prompt)
}

fn resolve_local_agent_instructions(
    workspace: Option<&PathBuf>,
    active_file: Option<&str>,
    open_files: Option<&[String]>,
) -> Option<String> {
    let workspace_root = workspace?;
    let mut candidate_paths = Vec::new();
    if let Some(active_file) = active_file.filter(|path| !path.trim().is_empty()) {
        candidate_paths.push(active_file.to_string());
    }
    if let Some(open_files) = open_files {
        candidate_paths.extend(open_files.iter().cloned());
    }

    crate::agent_instructions::load_agent_instructions(workspace_root, &candidate_paths).content
}

fn gemma4_temperature(model_id: &str) -> Option<f32> {
    is_gemma4_model(model_id).then_some(GEMMA4_TEMPERATURE)
}

fn gemma4_top_p(model_id: &str) -> Option<f32> {
    is_gemma4_model(model_id).then_some(GEMMA4_TOP_P)
}

fn gemma4_top_k(model_id: &str) -> Option<u32> {
    is_gemma4_model(model_id).then_some(GEMMA4_TOP_K)
}

fn is_tools_unsupported_error(status: reqwest::StatusCode, body: &str) -> bool {
    if !(status.as_u16() == 400 || status.as_u16() == 404 || status.as_u16() == 422) {
        return false;
    }

    let body_lower = body.to_ascii_lowercase();
    body_lower.contains("does not support tools")
        || body_lower.contains("doesn't support tools")
        || body_lower.contains("tools are not supported")
        || body_lower.contains("tool calling is not supported")
        || body_lower.contains("function calling is not supported")
        || body_lower.contains("does not support function calling")
        || body_lower.contains("doesn't support function calling")
}

pub enum DrainResult {
    None,
    Update(String, String),    // (message_id, delta) - streaming text chunk
    Reasoning(String, String), // (message_id, delta) - reasoning chunk
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
        tool_call_id: Option<String>,
    },
    CognitiveInterrupt(crate::protocol::CognitiveInterruptPayload),
    ApprovalRequest(crate::protocol::ApprovalRequest),
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
    /// Message too large - WebSocket message size limit exceeded
    MessageTooLarge {
        message: String,
        recovery_hint: String,
    },
}

#[derive(Clone)]
struct ProviderEventSender {
    tx: mpsc::Sender<ProviderEvent>,
    notify: Arc<tokio::sync::Notify>,
}

impl ProviderEventSender {
    fn new(tx: mpsc::Sender<ProviderEvent>, notify: Arc<tokio::sync::Notify>) -> Self {
        Self { tx, notify }
    }

    fn send_provider_event(
        &self,
        event: ProviderEvent,
    ) -> Result<(), mpsc::SendError<ProviderEvent>> {
        let res = self.tx.send(event);
        if res.is_ok() {
            self.notify.notify_one();
        }
        res
    }

    fn send(&self, event: ChatEvent) -> Result<(), mpsc::SendError<ProviderEvent>> {
        self.send_provider_event(event.into())
    }

    fn send_chunk(&self, chunk: String) -> Result<(), mpsc::SendError<ProviderEvent>> {
        self.send_provider_event(ProviderEvent::Chunk(chunk))
    }

    fn send_reasoning_chunk(&self, chunk: String) -> Result<(), mpsc::SendError<ProviderEvent>> {
        self.send_provider_event(ProviderEvent::ReasoningChunk(chunk))
    }

    fn send_tool_activity(
        &self,
        payload: ToolActivityPayload,
    ) -> Result<(), mpsc::SendError<ProviderEvent>> {
        self.send_provider_event(ProviderEvent::ToolActivity(payload))
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct OllamaMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
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

#[derive(Serialize, Clone)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
}

#[derive(Serialize, Clone)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
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
    pub rx: Option<mpsc::Receiver<ProviderEvent>>,
    pub xml_buffer: String,
    pub reasoning_parser: ReasoningParser, // v1.2: Multi-format reasoning extraction
    pub agentic_loop: AgenticLoop,
    pub session_id: Option<String>,
    pub planning_mode: Option<bool>,
    pub runtime_mode: Option<String>,
    pub mode_source: Option<String>,
    abort_handle: Option<tokio::task::AbortHandle>,
    pub accumulated_tool_calls: Vec<ToolCall>,
    pub updated_assistant_message: Option<ChatMessage>,
    pub message_seq: u64, // v1.1: sequence number for MessageDelta events
    pub pending_results: std::collections::VecDeque<DrainResult>,
    ws_client: Option<Arc<BladeWsClient>>, // Persistent connection for the conversation
    provider_event_tx: Option<ProviderEventSender>,
    pending_tool_progress: HashMap<String, String>, // tool_call_id -> tool_name from tool_progress (cleared when tool_call arrives)
    event_notify: Arc<tokio::sync::Notify>,
    active_provider: ProviderId,
    active_model_id: String,
    stream_plain_text: bool,
    stream_parse_reasoning: bool,
    stream_xml_tool_fallback: bool,
    stream_auto_start_loop_on_tools: bool,
    pending_done_without_tools: bool,
    stop_requested: bool,
    pub last_finish_reason: Option<String>,
    composite_tools_enabled: bool,
    active_request_id: Option<String>,
    ws_conversation_messages: Arc<Mutex<Vec<serde_json::Value>>>,
    /// Active-file identity (hash + dirty flag) staged by the orchestrator for
    /// the next fresh turn, so `start_zaguan_stream` can stamp the active
    /// open-file entry with a hash that agrees with the warmup snapshot. Taken
    /// (cleared) when consumed so it never leaks into a later turn.
    staged_active_file_identity: Option<crate::warmup_bundle::ActiveFileIdentity>,
}

fn supports_reasoning_tags(model_id: &str) -> bool {
    let model_lower = model_id.to_lowercase();
    model_lower.contains("deepseek")
        || model_lower.contains("qwq")
        || model_lower.contains("minimax")
        || model_lower.contains("kimi")
        || model_lower.contains("r1")
        || is_gemma4_model(&model_lower)
}

/// Blade mints conversation session ids itself (zcoderd accepts any non-empty
/// client-supplied id and only generates one as an empty-string fallback).
/// Pre-minting lets warmup context prefetch and the chat share one session key,
/// so zcoderd can attach the prewarmed bundle to the first real message.
pub(crate) fn fresh_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl ChatManager {
    pub fn new(max_turns: usize) -> Self {
        Self {
            streaming: false,
            rx: None,
            xml_buffer: String::new(),
            reasoning_parser: ReasoningParser::new(),
            agentic_loop: AgenticLoop::new(max_turns),
            session_id: Some(fresh_session_id()),
            planning_mode: None,
            runtime_mode: None,
            mode_source: None,
            abort_handle: None,
            accumulated_tool_calls: Vec::new(),
            updated_assistant_message: None,
            message_seq: 0,
            pending_results: std::collections::VecDeque::new(),
            ws_client: None,
            provider_event_tx: None,
            pending_tool_progress: HashMap::new(),
            event_notify: Arc::new(tokio::sync::Notify::new()),
            active_provider: ProviderId::Zaguan,
            active_model_id: String::new(),
            stream_plain_text: false,
            stream_parse_reasoning: false,
            stream_xml_tool_fallback: false,
            stream_auto_start_loop_on_tools: false,
            pending_done_without_tools: false,
            stop_requested: false,
            last_finish_reason: None,
            composite_tools_enabled: true,
            active_request_id: None,
            ws_conversation_messages: Arc::new(Mutex::new(Vec::new())),
            staged_active_file_identity: None,
        }
    }

    /// Stage the active-file identity computed off-lock by the orchestrator for
    /// the next `start_zaguan_stream`. Overwrites any previously staged value.
    pub(crate) fn stage_active_file_identity(
        &mut self,
        identity: Option<crate::warmup_bundle::ActiveFileIdentity>,
    ) {
        self.staged_active_file_identity = identity;
    }

    pub(crate) fn composite_tools_enabled(&self) -> bool {
        self.composite_tools_enabled
    }

    pub fn event_notifier(&self) -> Arc<tokio::sync::Notify> {
        self.event_notify.clone()
    }

    pub fn has_pending_done_without_tools(&self) -> bool {
        self.pending_done_without_tools
    }

    fn to_blade_conversation_messages(
        conversation: &ConversationHistory,
        exclude_latest_user: bool,
    ) -> Vec<serde_json::Value> {
        let messages = conversation.get_messages();
        let latest_user_idx = if exclude_latest_user {
            messages
                .iter()
                .enumerate()
                .rev()
                .find(|(_, msg)| msg.role == ChatRole::User)
                .map(|(idx, _)| idx)
        } else {
            None
        };
        let latest_user_with_images_idx = messages
            .iter()
            .enumerate()
            .rev()
            .find(|(_, msg)| {
                msg.role == ChatRole::User
                    && msg
                        .images
                        .as_ref()
                        .map(|images| !images.is_empty())
                        .unwrap_or(false)
            })
            .map(|(idx, _)| idx);

        messages
            .iter()
            .enumerate()
            .filter_map(|(idx, msg)| {
                if latest_user_idx == Some(idx) {
                    return None;
                }

                let effective_content = msg.backend_content.as_deref().unwrap_or(&msg.content);
                let has_content = !effective_content.trim().is_empty();
                let has_reasoning = msg
                    .reasoning
                    .as_ref()
                    .map(|r| !r.trim().is_empty())
                    .unwrap_or(false);
                let has_tool_calls = msg
                    .tool_calls
                    .as_ref()
                    .map(|calls| !calls.is_empty())
                    .unwrap_or(false);
                let has_images = msg
                    .images
                    .as_ref()
                    .map(|images| !images.is_empty())
                    .unwrap_or(false);

                // Local mode context must not include empty assistant placeholders.
                // They can be left behind on error turns and poison subsequent requests.
                let should_keep = match msg.role {
                    ChatRole::User => has_content || has_images,
                    ChatRole::Assistant => has_content || has_reasoning || has_tool_calls,
                    ChatRole::System => has_content,
                    ChatRole::Tool => true,
                };

                if !should_keep {
                    return None;
                }

                let role = match msg.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::System => "system",
                    ChatRole::Tool => "tool",
                };
                let mut blade_msg = serde_json::json!({
                    "role": role,
                    "content": msg.backend_content.as_deref().unwrap_or(&msg.content),
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
                if let Some(ref images) = msg.images {
                    // Keep full image payload only for the latest user image turn.
                    // Re-sending older base64 images on every context request can bloat
                    // payloads and trigger provider-side empty/error responses.
                    if msg.role != ChatRole::User || latest_user_with_images_idx != Some(idx) {
                        return Some(blade_msg);
                    }
                    blade_msg["images"] = serde_json::json!(images);
                }
                Some(blade_msg)
            })
            .collect()
    }

    fn sync_ws_conversation_messages(&self, conversation: &ConversationHistory) {
        if let Ok(mut guard) = self.ws_conversation_messages.lock() {
            *guard = Self::to_blade_conversation_messages(conversation, false);
        }
    }

    fn sync_ws_conversation_messages_without_latest_user(
        &self,
        conversation: &ConversationHistory,
    ) {
        if let Ok(mut guard) = self.ws_conversation_messages.lock() {
            *guard = Self::to_blade_conversation_messages(conversation, true);
        }
    }

    fn local_conversation_state_from_messages(
        messages: &[serde_json::Value],
    ) -> crate::blade_ws_client::LocalConversationState {
        let mut hash: u64 = 0xcbf29ce484222325;
        fn write_part(hash: &mut u64, value: &str) {
            for byte in value.as_bytes() {
                *hash ^= u64::from(*byte);
                *hash = hash.wrapping_mul(0x100000001b3);
            }
            *hash ^= 0xff;
            *hash = hash.wrapping_mul(0x100000001b3);
        }
        for msg in messages {
            write_part(
                &mut hash,
                msg.get("role")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
            );
            write_part(
                &mut hash,
                msg.get("content")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
            );
            write_part(
                &mut hash,
                msg.get("reasoning")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
            );
            write_part(
                &mut hash,
                msg.get("tool_call_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
            );
            if let Some(tool_calls) = msg.get("tool_calls").and_then(|value| value.as_array()) {
                for tool_call in tool_calls {
                    write_part(
                        &mut hash,
                        tool_call
                            .get("id")
                            .and_then(|value| value.as_str())
                            .unwrap_or(""),
                    );
                    write_part(
                        &mut hash,
                        tool_call
                            .get("type")
                            .and_then(|value| value.as_str())
                            .unwrap_or(""),
                    );
                    let function = tool_call.get("function");
                    write_part(
                        &mut hash,
                        function
                            .and_then(|value| value.get("name"))
                            .and_then(|value| value.as_str())
                            .unwrap_or(""),
                    );
                    write_part(
                        &mut hash,
                        function
                            .and_then(|value| value.get("arguments"))
                            .and_then(|value| value.as_str())
                            .unwrap_or(""),
                    );
                }
            }
        }
        crate::blade_ws_client::LocalConversationState {
            message_count: messages.len(),
            fingerprint: format!("{:016x}", hash),
        }
    }

    fn ensure_assistant_placeholder(conversation: &mut ConversationHistory) {
        let has_empty_assistant_tail = conversation
            .last()
            .map(|msg| msg.role == ChatRole::Assistant && is_semantically_empty_message(msg))
            .unwrap_or(false);

        if !has_empty_assistant_tail {
            conversation.push(ChatMessage::new(ChatRole::Assistant, String::new()));
        }
    }

    fn update_stream_profile(&mut self, provider: ProviderId, model_id_for_request: &str) {
        let model_id = model_id_for_request.to_lowercase();
        self.active_provider = provider;
        self.active_model_id = model_id.clone();

        // Local runtimes already emit clean Chunk / ReasoningChunk segments.
        // Zaguan and known OpenAI text models should bypass XML tool fallback parsing.
        self.stream_plain_text = matches!(provider, ProviderId::Zaguan)
            || (matches!(provider, ProviderId::OpenAiCompat) && !is_gemma4_model(&model_id))
            || model_id.contains("openai")
            || model_id.contains("gpt-5.2")
            || model_id.contains("codex");
        // Enable reasoning tag parsing for local models that use <think>/<thinking> tags.
        // Zaguan/OpenAI providers emit separate ReasoningChunk events, so no tag parsing needed.
        self.stream_parse_reasoning = supports_reasoning_tags(&model_id) && !self.stream_plain_text;

        let qwen_like = model_id.contains("qwen") || model_id.contains("mercury");
        self.stream_xml_tool_fallback = qwen_like;
        self.stream_auto_start_loop_on_tools = qwen_like;
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
        mode: Option<String>,
        composite_tools_enabled: bool,
    ) -> Result<(), String> {
        self.reasoning_parser.reset();
        self.xml_buffer.clear();
        self.accumulated_tool_calls.clear();
        self.updated_assistant_message = None;
        self.message_seq = 0; // v1.1: reset sequence counter for new message
        self.pending_done_without_tools = false;
        self.stop_requested = false;
        self.last_finish_reason = None;
        self.composite_tools_enabled = composite_tools_enabled;
        self.active_request_id = None;

        let selected = resolve_model_selection(models, selected_model);
        self.update_stream_profile(selected.provider, &selected.model_id_for_request);

        match selected.provider {
            ProviderId::Ollama => OllamaRuntime
                .start_turn(
                    self,
                    conversation,
                    api_config,
                    &selected.model_id_for_request,
                    http,
                    workspace,
                    active_file,
                    open_files.clone(),
                    None,
                    None,
                    None,
                    None,
                    composite_tools_enabled,
                )
                .map(|_| ()),
            ProviderId::OpenAiCompat => OpenAiCompatRuntime
                .start_turn(
                    self,
                    conversation,
                    api_config,
                    &selected.model_id_for_request,
                    http,
                    workspace,
                    active_file,
                    open_files.clone(),
                    None,
                    None,
                    None,
                    None,
                    composite_tools_enabled,
                )
                .map(|_| ()),
            ProviderId::Zaguan => ZaguanRuntime
                .start_turn(
                    self,
                    conversation,
                    api_config,
                    &selected.model_id_for_request,
                    http,
                    workspace,
                    active_file,
                    open_files,
                    cursor_line,
                    cursor_column,
                    storage_mode,
                    mode,
                    composite_tools_enabled,
                )
                .map(|_| ()),
        }
    }

    pub(crate) fn start_zaguan_stream(
        &mut self,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        workspace: Option<&PathBuf>,
        active_file: Option<String>,
        open_files: Option<Vec<String>>,
        cursor_line: Option<usize>,
        cursor_column: Option<usize>,
        storage_mode: Option<String>,
        mode: Option<String>,
    ) -> Result<(), String> {
        // Build workspace info for Blade Protocol. The active file's entry
        // carries a hash matching the warmup snapshot (buffer-over-disk when
        // dirty) so zcoderd can validate the warmed context against this turn
        // without forcing a re-read. Identity is staged off-lock by the
        // orchestrator; absent (None) means legacy hashless behavior.
        let active_identity = self.staged_active_file_identity.take();
        let (active_hash, active_modified) = match &active_identity {
            Some(identity) => (
                identity.hash.clone().unwrap_or_default(),
                identity.is_modified,
            ),
            None => (String::new(), false),
        };
        let open_file_infos = open_files
            .unwrap_or_default()
            .into_iter()
            .map(|path| {
                let is_active = active_file.as_ref() == Some(&path);
                crate::blade_ws_client::OpenFileInfo {
                    hash: if is_active {
                        active_hash.clone()
                    } else {
                        String::new()
                    },
                    is_modified: is_active && active_modified,
                    is_active,
                    path,
                }
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
            host_skills: crate::blade_ws_client::HostSkillsSnapshot::empty(),
        };

        // Get last user message
        let user_message = conversation
            .get_messages()
            .iter()
            .rev()
            .find(|m| m.role == ChatRole::User)
            .map(|m| {
                m.backend_content
                    .clone()
                    .unwrap_or_else(|| m.content.clone())
            })
            .unwrap_or_default();

        // Close any existing WebSocket connection before starting a new one
        if let Some(old_client) = self.ws_client.take() {
            // eprintln!("[CHAT MGR] Closing previous WebSocket connection");
            let old_client_clone = old_client.clone();
            tokio::spawn(async move {
                if let Err(error) = old_client_clone
                    .close_with_session_disconnect(None, DISCONNECT_FLUSH_WINDOW)
                    .await
                {
                    eprintln!(
                        "[CHAT MGR] Failed to send disconnect before replacing WebSocket: {}",
                        error
                    );
                }
            });
        }

        // Create new WebSocket client for this conversation
        let blade_url = crate::config::default_blade_url();
        let api_key = api_config.api_key.clone();
        // eprintln!("[BLADE WS] Connecting to: {}", blade_url);
        // eprintln!("[BLADE WS] Sending message: {}", user_message);
        // eprintln!("[BLADE WS] API key present: {}", !api_key.is_empty());

        let ws_client = Arc::new(BladeWsClient::new(blade_url.clone(), api_key.clone()));
        self.ws_client = Some(ws_client.clone());
        let session_id = self.session_id.clone();
        let model_id = model_id.to_string();
        let chat_request_id = BladeWsClient::new_chat_request_id();
        self.active_request_id = Some(chat_request_id.clone());

        // eprintln!("[CHAT MGR] Starting stream with session_id: {:?}", session_id);

        self.sync_ws_conversation_messages_without_latest_user(conversation);
        let ws_conversation_messages = self.ws_conversation_messages.clone();
        let local_conversation_state = ws_conversation_messages
            .lock()
            .map(|messages| Self::local_conversation_state_from_messages(&messages))
            .ok();

        // Convert WebSocket events to ChatEvent channel
        let (tx, rx) = mpsc::channel();
        let tx = ProviderEventSender::new(tx, self.event_notify.clone());
        self.provider_event_tx = Some(tx.clone());

        let user_images = conversation
            .get_messages()
            .iter()
            .rev()
            .find(|m| m.role == ChatRole::User)
            .and_then(|m| m.images.clone());

        let ws_workspace_path = workspace.map(|p| p.to_string_lossy().to_string());

        // Spawn async task to connect and handle events
        let task = tokio::spawn(async move {
            // eprintln!("[CHAT MGR] Connecting to WebSocket");
            match ws_client.connect(ws_workspace_path).await {
                Ok(mut ws_rx) => {
                    // eprintln!("[CHAT MGR] WebSocket connected, waiting for authenticated");

                    // Wait for authentication
                    let mut authenticated = false;
                    let mut saw_chat_done = false;
                    let mut saw_content = false;
                    // Some providers may emit the same content as both reasoning and text chunks.
                    // Keep the latest reasoning chunk so we can skip an immediate duplicate text chunk.
                    let mut last_reasoning_chunk: Option<String> = None;
                    while let Some(event) = ws_rx.recv().await {
                        match event {
                            crate::blade_ws_client::BladeWsEvent::Connected { .. } => {
                                // eprintln!("[CHAT MGR] Authenticated, sending chat message with session_id: {:?}", session_id);
                                authenticated = true;

                                // Send chat message with storage mode (RFC-002)
                                if let Err(e) = ws_client
                                    .send_message_with_storage_mode_and_id(
                                        chat_request_id.clone(),
                                        session_id.clone(),
                                        model_id.clone(),
                                        user_message.clone(),
                                        user_images.clone(),
                                        Some(workspace_info.clone()),
                                        storage_mode.clone(),
                                        mode.clone(),
                                        local_conversation_state.clone(),
                                        None,
                                        None,
                                        None,
                                        None,
                                        None,
                                    )
                                    .await
                                {
                                    // eprintln!("[CHAT MGR] Failed to send message: {}", e);
                                    let _ = tx.send(ChatEvent::Error(e));
                                    break;
                                }
                                // eprintln!("[CHAT MGR] Message sent successfully");
                            }
                            crate::blade_ws_client::BladeWsEvent::Session {
                                session_id,
                                model_id,
                                planning_mode,
                                runtime_mode,
                                mode_source,
                            } => {
                                // eprintln!(
                                //     "[CHAT MGR] Session event: session_id={}, model={}",
                                //     session_id, model_id
                                // );
                                ws_client.set_session_id(session_id.clone()).await;
                                let _ = tx.send(ChatEvent::Session {
                                    session_id: session_id.clone(),
                                    model: model_id,
                                    planning_mode,
                                    runtime_mode,
                                    mode_source,
                                });
                            }
                            crate::blade_ws_client::BladeWsEvent::TextChunk {
                                request_id: _,
                                content: text,
                                output_index: _,
                                phase,
                            } => {
                                if phase.as_deref() == Some("commentary") {
                                    last_reasoning_chunk = Some(text.clone());
                                    saw_content = true;
                                    let _ = tx.send_reasoning_chunk(text);
                                    continue;
                                }
                                if last_reasoning_chunk.as_deref() == Some(text.as_str()) {
                                    last_reasoning_chunk = None;
                                    continue;
                                }
                                last_reasoning_chunk = None;
                                saw_content = true;
                                let _ = tx.send_chunk(text);
                            }
                            crate::blade_ws_client::BladeWsEvent::ReasoningChunk {
                                request_id: _,
                                content: text,
                                output_index: _,
                                phase: _,
                            } => {
                                last_reasoning_chunk = Some(text.clone());
                                saw_content = true;
                                let _ = tx.send_reasoning_chunk(text);
                            }
                            crate::blade_ws_client::BladeWsEvent::ToolCall {
                                request_id: _,
                                id,
                                name,
                                arguments,
                            } => {
                                last_reasoning_chunk = None;
                                // eprintln!("[CHAT MGR] Tool call: {}", name);
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
                                pending_count: _pending_count,
                            } => {
                                last_reasoning_chunk = None;
                                // zcoderd acknowledged our tool result but is waiting for more
                                // This is informational - keep connection alive and wait for real response
                                // eprintln!(
                                //     "[CHAT MGR] Tool result acknowledged, {} more pending",
                                //     pending_count
                                // );
                                // Continue listening - don't close connection or emit Done
                            }
                            crate::blade_ws_client::BladeWsEvent::TodoUpdated { todos } => {
                                // eprintln!("[CHAT MGR] Todo updated: {} items", todos.len());
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
                            crate::blade_ws_client::BladeWsEvent::ApprovalRequest {
                                request_id: _,
                                session_id,
                                approval_id,
                                tool_call_id,
                                tool_name,
                                arguments,
                                source,
                                rule_name,
                                message,
                                decision,
                            } => {
                                last_reasoning_chunk = None;
                                let _ = tx.send(ChatEvent::ApprovalRequest(
                                    crate::protocol::ApprovalRequest {
                                        session_id,
                                        approval_id,
                                        tool_call_id,
                                        tool_name,
                                        arguments,
                                        source,
                                        rule_name,
                                        message,
                                        decision,
                                    },
                                ));
                            }
                            crate::blade_ws_client::BladeWsEvent::ChatDone {
                                request_id: _,
                                finish_reason,
                                recoverable,
                            } => {
                                last_reasoning_chunk = None;
                                // eprintln!("[CHAT MGR] Chat done: {} (recoverable: {:?})", finish_reason, recoverable);
                                saw_chat_done = true;

                                // RFC: Context Length Recovery - check for context_length_exceeded finish reason
                                if finish_reason == "context_length_exceeded" {
                                    let _ = tx.send(ChatEvent::ContextLengthExceeded {
                                        message: "Context limit reached during generation"
                                            .to_string(),
                                        token_count: None,
                                        max_tokens: None,
                                        excess: None,
                                        recoverable: recoverable.unwrap_or(true),
                                        recovery_hint: None,
                                    });
                                }

                                let _ = tx.send(ChatEvent::Done { finish_reason });
                                // Don't break - keep connection alive for tool results
                                // The connection will close when the user sends a new message
                            }
                            crate::blade_ws_client::BladeWsEvent::RequestCancelled {
                                request_id: _,
                                session_id: _,
                                reason: _,
                            } => {
                                let _ = tx.send(ChatEvent::Done {
                                    finish_reason: "stop".to_string(),
                                });
                                break;
                            }
                            crate::blade_ws_client::BladeWsEvent::Error {
                                error_type,
                                code: _code,
                                message,
                                token_count,
                                max_tokens,
                                excess,
                                recoverable,
                                recovery_hint,
                                ..
                            } => {
                                // eprintln!("[CHAT MGR] Error: {} ({}) - {} (tokens: {:?}/{:?})", error_type, code, message, token_count, max_tokens);

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
                                        let hint = recovery_hint.unwrap_or_else(|| {
                                            "Please wait a moment and try again.".to_string()
                                        });
                                        let _ = tx.send(ChatEvent::Error(format!(
                                            "{} - {}",
                                            message, hint
                                        )));
                                        // Don't break - these are transient
                                    }
                                    "message_too_large" => {
                                        // Message size limit exceeded - send error with recovery hint
                                        // The recovery hint will be shown to the user and included in the error
                                        let hint = recovery_hint.unwrap_or_else(||
                                            "Please use smaller responses and break large changes into multiple tool calls.".to_string()
                                        );
                                        let _ = tx.send(ChatEvent::MessageTooLarge {
                                            message: message.clone(),
                                            recovery_hint: hint,
                                        });
                                        // Don't break - this is recoverable, model can retry with smaller output
                                    }
                                    "authentication_error" => {
                                        // Fatal error - break the connection
                                        let _ = tx.send(ChatEvent::Error(message));
                                        break;
                                    }
                                    _ => {
                                        // Unknown error type - use recoverable flag
                                        if authenticated && (saw_chat_done || saw_content) {
                                            let _ = tx.send(ChatEvent::Done {
                                                finish_reason: "stop".to_string(),
                                            });
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
                                eprintln!(
                                    "[CHAT MGR] Server disconnected during chat stream; session will be restored from database on reconnect"
                                );
                                if saw_chat_done {
                                    break;
                                }

                                let message = if authenticated || saw_content {
                                    "Server disconnected while generating. Reconnect to zcoderd and retry when it is available."
                                } else {
                                    "Server disconnected before a response was received. Reconnect to zcoderd and retry when it is available."
                                };
                                let _ = tx.send_provider_event(ProviderEvent::Disconnected {
                                    message: message.to_string(),
                                });
                                break;
                            }
                            crate::blade_ws_client::BladeWsEvent::Progress {
                                message,
                                stage,
                                percent,
                            } => {
                                // eprintln!("[CHAT MGR] Progress: {} ({}%)", message, percent);
                                let _ = tx.send(ChatEvent::Progress {
                                    message,
                                    stage,
                                    percent: percent as i32,
                                });
                            }
                            crate::blade_ws_client::BladeWsEvent::Research { content } => {
                                // eprintln!(
                                //     "[CHAT MGR] Research result received ({} chars)",
                                //     content.len()
                                // );
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
                                let _ = tx.send_tool_activity(ToolActivityPayload {
                                    tool_name,
                                    file_path,
                                    action,
                                    tool_call_id: None,
                                });
                            }
                            crate::blade_ws_client::BladeWsEvent::CognitiveInterrupt(payload) => {
                                let _ = tx.send(ChatEvent::CognitiveInterrupt(payload));
                            }
                            crate::blade_ws_client::BladeWsEvent::ToolProgress {
                                tool_call_id,
                                tool_name,
                                file_path,
                            } => {
                                // Emit as ToolActivity with "streaming" action for UI display
                                if let Some(path) = file_path {
                                    let _ = tx.send_tool_activity(ToolActivityPayload {
                                        tool_name,
                                        file_path: path,
                                        action: "streaming".to_string(),
                                        tool_call_id: Some(tool_call_id),
                                    });
                                }
                            }
                            crate::blade_ws_client::BladeWsEvent::ContextPackRequest {
                                id,
                                query,
                                queries,
                                intent,
                                max_results,
                                include_tests,
                                include_docs,
                                include_memory,
                                include_project_index_min,
                                response_type,
                            } => {
                                let workspace_root = PathBuf::from(workspace_info.root.clone());
                                let active_file = workspace_info.active_file.clone();
                                let open_files = workspace_info
                                    .open_files
                                    .iter()
                                    .map(|file| file.path.clone())
                                    .collect::<Vec<_>>();
                                let request = crate::context_pack::ContextPackRequest {
                                    id: id.clone(),
                                    query,
                                    queries,
                                    intent,
                                    max_results,
                                    include_tests,
                                    include_docs,
                                    include_memory,
                                    include_project_index_min,
                                };

                                let payload = match tokio::time::timeout(
                                    std::time::Duration::from_secs(5),
                                    tokio::task::spawn_blocking(move || {
                                        crate::context_pack::build_context_pack(
                                            &workspace_root,
                                            active_file.as_deref(),
                                            &open_files,
                                            &request,
                                        )
                                    }),
                                )
                                .await
                                {
                                    Ok(Ok(payload)) => payload,
                                    Ok(Err(error)) => crate::context_pack::error_payload(
                                        "internal_error",
                                        &format!("Context pack worker failed: {}", error),
                                    ),
                                    Err(_) => crate::context_pack::error_payload(
                                        "timeout",
                                        "Context pack generation timed out",
                                    ),
                                };

                                if let Err(error) = ws_client
                                    .send_context_pack_response(id, response_type, payload)
                                    .await
                                {
                                    let _ = tx.send(ChatEvent::Error(format!(
                                        "Failed to send context pack response: {}",
                                        error
                                    )));
                                }
                            }
                            crate::blade_ws_client::BladeWsEvent::GetConversationContext {
                                request_id,
                                session_id: req_session_id,
                            } => {
                                let t0 = std::time::Instant::now();

                                // RFC-002: Send latest synchronized conversation context back to server.
                                let context_messages = ws_conversation_messages
                                    .lock()
                                    .map(|msgs| msgs.clone())
                                    .unwrap_or_default();

                                if let Err(e) = ws_client
                                    .send_conversation_context(
                                        request_id,
                                        req_session_id,
                                        context_messages,
                                    )
                                    .await
                                {
                                    let _ = tx.send(ChatEvent::Error(format!(
                                        "Failed to send conversation context: {}",
                                        e
                                    )));
                                    break;
                                }
                                let _ = t0;
                            }
                            crate::blade_ws_client::BladeWsEvent::HistoryListResponse {
                                ..
                            }
                            | crate::blade_ws_client::BladeWsEvent::HistoryDetailResponse {
                                ..
                            }
                            | crate::blade_ws_client::BladeWsEvent::ZlpResponse { .. }
                            | crate::blade_ws_client::BladeWsEvent::ContextPackResponse {
                                ..
                            }
                            | crate::blade_ws_client::BladeWsEvent::PairingTokenResponse {
                                ..
                            }
                            | crate::blade_ws_client::BladeWsEvent::RemoteControlConnected {
                                ..
                            }
                            | crate::blade_ws_client::BladeWsEvent::RemoteControlDisconnected => {}
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(ChatEvent::Error(e));
                }
            }

            // eprintln!("[CHAT MGR] Async task completed, keeping WebSocket open for tool results");
            // Don't close the connection here - it will be reused for tool results
            // Connection will be closed when a new message starts or conversation ends
        });

        // Push placeholder for the next streamed assistant item.
        Self::ensure_assistant_placeholder(conversation);

        self.rx = Some(rx);
        self.streaming = true;
        self.abort_handle = Some(task.abort_handle());
        Ok(())
    }

    pub(crate) fn start_ollama_stream(
        &mut self,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        http: reqwest::Client,
        workspace: Option<&PathBuf>,
        active_file: Option<String>,
        open_files: Option<Vec<String>>,
        composite_tools_enabled: bool,
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
        let date_value = chrono::Local::now().format("%Y-%m-%d").to_string();
        let time_value = chrono::Local::now().format("%H:%M:%S %z").to_string();
        let agent_instructions = resolve_local_agent_instructions(
            workspace,
            (!active_file_value.is_empty()).then_some(active_file_value.as_str()),
            open_files.as_deref(),
        );

        let mut messages: Vec<OllamaMessage> = Vec::new();
        if let Some(rendered_prompt) = load_local_system_prompt(
            &model_name,
            &workspace_root,
            &active_file_value,
            &os_value,
            &shell_value,
            &date_value,
            &time_value,
            agent_instructions.as_deref(),
        ) {
            messages.push(OllamaMessage {
                role: "system".to_string(),
                content: Some(rendered_prompt),
                images: None,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
            });
        }

        let mut tool_name_by_id: HashMap<String, String> = HashMap::new();
        for msg in conversation.get_messages() {
            if is_semantically_empty_message(&msg) {
                continue;
            }
            if let Some(tool_calls) = msg.tool_calls.as_ref() {
                for call in tool_calls {
                    tool_name_by_id.insert(call.id.clone(), call.function.name.clone());
                }
            }
        }

        for msg in conversation.get_messages() {
            if is_semantically_empty_message(&msg) {
                continue;
            }
            let (role, content, tool_call_id) = match msg.role {
                ChatRole::User => (
                    "user",
                    Some(
                        msg.backend_content
                            .clone()
                            .unwrap_or_else(|| msg.content.clone()),
                    ),
                    None,
                ),
                ChatRole::Assistant => {
                    let content = if msg.content.trim().is_empty() {
                        None
                    } else {
                        Some(msg.content.clone())
                    };
                    ("assistant", content, None)
                }
                ChatRole::System => ("system", Some(msg.content.clone()), None),
                ChatRole::Tool => (
                    "tool",
                    Some(
                        msg.backend_content
                            .clone()
                            .unwrap_or_else(|| msg.content.clone()),
                    ),
                    msg.tool_call_id.clone(),
                ),
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

            let images = if msg.role == ChatRole::User {
                msg.images
                    .as_ref()
                    .map(|imgs| imgs.iter().map(|img| img.data.clone()).collect())
            } else {
                None
            };

            messages.push(OllamaMessage {
                role: role.to_string(),
                content,
                images,
                tool_calls,
                tool_call_id,
                tool_name,
            });
        }

        let tool_cache_key = local_tool_support_cache_key("ollama", &model_name);
        let include_tools = !model_disables_tools(&tool_cache_key);
        let request = OllamaChatRequest {
            model: model_name.clone(),
            messages,
            stream: true,
            tools: include_tools
                .then(|| get_tool_definitions_for_model(&model_name, composite_tools_enabled)),
            options: Some(OllamaOptions {
                temperature: gemma4_temperature(&model_name),
                top_p: gemma4_top_p(&model_name),
                top_k: gemma4_top_k(&model_name),
            })
            .filter(|options| {
                options.temperature.is_some() || options.top_p.is_some() || options.top_k.is_some()
            }),
        };

        let (tx, rx) = mpsc::channel();
        let tx = ProviderEventSender::new(tx, self.event_notify.clone());
        let url = format!("{}/api/chat", api_config.ollama_url.trim_end_matches('/'));

        // Send Authorization header whenever cloud is enabled and we have an API key.
        // The Ollama server may require auth for ALL requests, not just :cloud models.
        let cloud_api_key =
            if api_config.ollama_cloud_enabled && !api_config.ollama_cloud_api_key.is_empty() {
                api_config.ollama_cloud_api_key.clone()
            } else {
                String::new()
            };

        let task = tokio::spawn(async move {
            // CRITICAL FIX: Only use reasoning parser for models that actually support reasoning tags.
            // The reasoning parser looks for <think> and <thinking> tags in the response.
            // If we run ALL text through it, regular content with angle brackets (HTML, XML, code)
            // gets misinterpreted as reasoning tags, causing garbled output.
            // Only models like DeepSeek R1, Qwen QwQ, MiniMax, and Kimi use these tags.
            let supports_reasoning = supports_reasoning_tags(&model_name);

            let mut reasoning_parser = if supports_reasoning {
                Some(ReasoningParser::new())
            } else {
                None
            };

            // Build the request with optional Authorization header for cloud models
            let mut request_builder = http.post(&url).json(&request);
            if !cloud_api_key.is_empty() {
                request_builder =
                    request_builder.header("Authorization", format!("Bearer {}", cloud_api_key));
            }

            let mut response = match request_builder.send().await {
                Ok(res) => res,
                Err(e) => {
                    let _ = tx.send(ChatEvent::Error(format!("Ollama request failed: {}", e)));
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();

                if include_tools && is_tools_unsupported_error(status, &body) {
                    remember_model_disables_tools(&tool_cache_key);

                    let retry_request = OllamaChatRequest {
                        tools: None,
                        ..request.clone()
                    };

                    let mut retry_builder = http.post(&url).json(&retry_request);
                    if !cloud_api_key.is_empty() {
                        retry_builder = retry_builder
                            .header("Authorization", format!("Bearer {}", cloud_api_key));
                    }

                    response = match retry_builder.send().await {
                        Ok(res) => res,
                        Err(e) => {
                            let _ = tx.send(ChatEvent::Error(format!(
                                "Ollama retry without tools failed: {}",
                                e
                            )));
                            return;
                        }
                    };
                } else {
                    let err_msg = if status.as_u16() == 401 {
                        format!("Ollama authentication failed (401). Check your cloud API key in Settings.")
                    } else {
                        format!(
                            "Ollama returned HTTP {}: {}",
                            status,
                            body.chars().take(500).collect::<String>()
                        )
                    };
                    eprintln!("[OLLAMA CHAT] HTTP error: {}", err_msg);
                    let _ = tx.send(ChatEvent::Error(err_msg));
                    return;
                }
            }

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                let err_msg = if status.as_u16() == 401 {
                    format!(
                        "Ollama authentication failed (401). Check your cloud API key in Settings."
                    )
                } else {
                    format!(
                        "Ollama returned HTTP {}: {}",
                        status,
                        body.chars().take(500).collect::<String>()
                    )
                };
                eprintln!("[OLLAMA CHAT] HTTP error: {}", err_msg);
                let _ = tx.send(ChatEvent::Error(err_msg));
                return;
            }

            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let saw_done = false;

            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(data) => data,
                    Err(e) => {
                        let _ = tx.send(ChatEvent::Error(format!("Ollama stream error: {}", e)));
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
                        let _ = tx.send(ChatEvent::Error(format!("Ollama error: {}", err)));
                        return;
                    }

                    if let Some(msg) = parsed.message {
                        if let Some(content) = msg.content {
                            if !content.is_empty() {
                                if reasoning_parser.is_none() && contains_reasoning_marker(&content)
                                {
                                    reasoning_parser = Some(ReasoningParser::new());
                                }

                                if let Some(ref mut parser) = reasoning_parser {
                                    for segment in parser.process_segments(&content) {
                                        match segment {
                                            crate::reasoning_parser::ReasoningSegment::Text(text) => {
                                                if !text.is_empty() {
                                                    let _ = tx.send_chunk(text);
                                                }
                                            }
                                            crate::reasoning_parser::ReasoningSegment::Reasoning(reasoning) => {
                                                if !reasoning.is_empty() {
                                                    let _ = tx.send_reasoning_chunk(reasoning);
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    // No reasoning parser - send content directly
                                    let _ = tx.send_chunk(content);
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
                        // Flush any buffered content from reasoning parser before Done
                        if let Some(ref mut parser) = reasoning_parser {
                            for segment in parser.flush() {
                                match segment {
                                    crate::reasoning_parser::ReasoningSegment::Text(text) => {
                                        if !text.is_empty() {
                                            let _ = tx.send_chunk(text);
                                        }
                                    }
                                    crate::reasoning_parser::ReasoningSegment::Reasoning(
                                        reasoning,
                                    ) => {
                                        if !reasoning.is_empty() {
                                            let _ = tx.send_reasoning_chunk(reasoning);
                                        }
                                    }
                                }
                            }
                        }
                        let _ = tx.send(ChatEvent::Done {
                            finish_reason: "stop".to_string(),
                        });
                        return;
                    }
                }
            }

            // Flush any buffered content before sending Done
            if let Some(ref mut parser) = reasoning_parser {
                for segment in parser.flush() {
                    match segment {
                        crate::reasoning_parser::ReasoningSegment::Text(text) => {
                            if !text.is_empty() {
                                let _ = tx.send_chunk(text);
                            }
                        }
                        crate::reasoning_parser::ReasoningSegment::Reasoning(reasoning) => {
                            if !reasoning.is_empty() {
                                let _ = tx.send_reasoning_chunk(reasoning);
                            }
                        }
                    }
                }
            }

            if !saw_done {
                let _ = tx.send(ChatEvent::Done {
                    finish_reason: "stop".to_string(),
                });
            }
        });

        Self::ensure_assistant_placeholder(conversation);
        self.rx = Some(rx);
        self.streaming = true;
        self.abort_handle = Some(task.abort_handle());
        Ok(())
    }

    pub(crate) fn start_openai_compat_stream(
        &mut self,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        http: reqwest::Client,
        workspace: Option<&PathBuf>,
        active_file: Option<String>,
        open_files: Option<Vec<String>>,
        composite_tools_enabled: bool,
    ) -> Result<(), String> {
        let model_name = model_id
            .strip_prefix("openai-compat/")
            .unwrap_or(model_id)
            .to_string();

        let workspace_root = workspace
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let active_file_value = active_file.unwrap_or_default();
        let os_value = std::env::consts::OS.to_string();
        let shell_value = std::env::var("SHELL").unwrap_or_default();
        let date_value = chrono::Local::now().format("%Y-%m-%d").to_string();
        let time_value = chrono::Local::now().format("%H:%M:%S %z").to_string();
        let agent_instructions = resolve_local_agent_instructions(
            workspace,
            (!active_file_value.is_empty()).then_some(active_file_value.as_str()),
            open_files.as_deref(),
        );

        #[derive(Serialize, Clone)]
        #[serde(untagged)]
        enum OpenAIContent {
            Text(String),
            Parts(Vec<OpenAIContentPart>),
        }

        #[derive(Serialize, Clone)]
        #[serde(tag = "type")]
        enum OpenAIContentPart {
            #[serde(rename = "text")]
            Text { text: String },
            #[serde(rename = "image_url")]
            ImageUrl { image_url: OpenAIImageUrl },
        }

        #[derive(Serialize, Clone)]
        struct OpenAIImageUrl {
            url: String,
        }

        #[derive(Serialize, Clone)]
        struct OpenAIMessage {
            role: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            content: Option<OpenAIContent>,
            #[serde(skip_serializing_if = "Option::is_none")]
            tool_calls: Option<Vec<crate::protocol::ToolCall>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            tool_call_id: Option<String>,
        }

        #[derive(Serialize, Clone)]
        struct OpenAIRequest {
            model: String,
            messages: Vec<OpenAIMessage>,
            stream: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            tools: Option<Vec<serde_json::Value>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            temperature: Option<f32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            top_p: Option<f32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            top_k: Option<u32>,
        }

        let mut messages: Vec<OpenAIMessage> = Vec::new();
        let gemma4_model = is_gemma4_model(&model_name);

        // Load and apply per-model system prompt
        if let Some(rendered_prompt) = load_local_system_prompt(
            &model_name,
            &workspace_root,
            &active_file_value,
            &os_value,
            &shell_value,
            &date_value,
            &time_value,
            agent_instructions.as_deref(),
        ) {
            messages.push(OpenAIMessage {
                role: "system".to_string(),
                content: Some(OpenAIContent::Text(rendered_prompt)),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Convert conversation history to OpenAI format
        for msg in conversation.get_messages() {
            if is_semantically_empty_message(&msg) {
                continue;
            }
            match msg.role {
                ChatRole::User => {
                    let effective_content = msg.backend_content.as_deref().unwrap_or(&msg.content);
                    let mut parts: Vec<OpenAIContentPart> = Vec::new();
                    if gemma4_model {
                        if let Some(images) = &msg.images {
                            for image in images {
                                parts.push(OpenAIContentPart::ImageUrl {
                                    image_url: OpenAIImageUrl {
                                        url: format!(
                                            "data:{};base64,{}",
                                            image.mime_type, image.data
                                        ),
                                    },
                                });
                            }
                        }
                        if !effective_content.trim().is_empty() {
                            parts.push(OpenAIContentPart::Text {
                                text: effective_content.to_string(),
                            });
                        }
                    } else {
                        if !effective_content.trim().is_empty() {
                            parts.push(OpenAIContentPart::Text {
                                text: effective_content.to_string(),
                            });
                        }
                        if let Some(images) = &msg.images {
                            for image in images {
                                parts.push(OpenAIContentPart::ImageUrl {
                                    image_url: OpenAIImageUrl {
                                        url: format!(
                                            "data:{};base64,{}",
                                            image.mime_type, image.data
                                        ),
                                    },
                                });
                            }
                        }
                    }

                    let content = if parts.is_empty() {
                        None
                    } else if parts.len() == 1 {
                        match &parts[0] {
                            OpenAIContentPart::Text { text } => {
                                Some(OpenAIContent::Text(text.clone()))
                            }
                            OpenAIContentPart::ImageUrl { .. } => Some(OpenAIContent::Parts(parts)),
                        }
                    } else {
                        Some(OpenAIContent::Parts(parts))
                    };

                    messages.push(OpenAIMessage {
                        role: "user".to_string(),
                        content,
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }
                ChatRole::Assistant => {
                    let content = if msg.content.trim().is_empty() {
                        None
                    } else {
                        Some(OpenAIContent::Text(msg.content.clone()))
                    };
                    messages.push(OpenAIMessage {
                        role: "assistant".to_string(),
                        content,
                        tool_calls: msg.tool_calls.clone(),
                        tool_call_id: None,
                    });
                }
                ChatRole::System => messages.push(OpenAIMessage {
                    role: "system".to_string(),
                    content: Some(OpenAIContent::Text(msg.content.clone())),
                    tool_calls: None,
                    tool_call_id: None,
                }),
                ChatRole::Tool => messages.push(OpenAIMessage {
                    role: "tool".to_string(),
                    content: Some(OpenAIContent::Text(
                        msg.backend_content
                            .clone()
                            .unwrap_or_else(|| msg.content.clone()),
                    )),
                    tool_calls: None,
                    tool_call_id: msg.tool_call_id.clone(),
                }),
            }
        }

        let tool_cache_key = local_tool_support_cache_key("openai-compat", &model_name);
        let include_tools = !model_disables_tools(&tool_cache_key);
        let request_body = OpenAIRequest {
            model: model_name.clone(),
            messages,
            stream: true,
            tools: include_tools
                .then(|| get_tool_definitions_for_model(&model_name, composite_tools_enabled)),
            temperature: gemma4_temperature(&model_name),
            top_p: gemma4_top_p(&model_name),
            top_k: gemma4_top_k(&model_name),
        };

        let openai_compat_base_url = normalize_openai_compat_url(&api_config.openai_compat_url);
        // OpenAI-compatible servers follow the /v1/chat/completions path; base URL should be versionless
        let url = format!("{}/v1/chat/completions", openai_compat_base_url);
        let (tx, rx) = mpsc::channel();
        let tx = ProviderEventSender::new(tx, self.event_notify.clone());

        let task = tokio::spawn(async move {
            #[derive(Deserialize)]
            struct StreamChunk {
                choices: Vec<StreamChoice>,
            }

            #[derive(Deserialize)]
            struct StreamMessage {
                #[serde(default)]
                content: Option<String>,
                #[serde(default)]
                tool_calls: Option<Vec<crate::protocol::ToolCall>>,
            }

            #[derive(Deserialize)]
            struct StreamChoice {
                #[serde(default)]
                delta: StreamDelta,
                #[serde(default)]
                message: Option<StreamMessage>,
                #[serde(default)]
                finish_reason: Option<String>,
            }

            #[derive(Default, Deserialize)]
            struct StreamDelta {
                #[serde(default)]
                content: Option<String>,
                #[serde(default)]
                tool_calls: Vec<crate::protocol::ToolCallDelta>,
            }

            // Reasoning parser is optional and enabled only for models that support it or emit <think>/<thinking> tags.
            let supports_reasoning = supports_reasoning_tags(&model_name);
            let mut reasoning_parser: Option<ReasoningParser> = if supports_reasoning {
                Some(ReasoningParser::new())
            } else {
                None
            };

            let mut response = match http.post(&url).json(&request_body).send().await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(ChatEvent::Error(format!(
                        "Failed to connect to OpenAI-compatible server: {}",
                        e
                    )));
                    let _ = tx.send(ChatEvent::Done {
                        finish_reason: "stop".to_string(),
                    });
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();

                if include_tools && is_tools_unsupported_error(status, &text) {
                    remember_model_disables_tools(&tool_cache_key);
                    let retry_request = OpenAIRequest {
                        tools: None,
                        ..request_body.clone()
                    };

                    response = match http.post(&url).json(&retry_request).send().await {
                        Ok(r) => r,
                        Err(e) => {
                            let _ = tx.send(ChatEvent::Error(format!(
                                "Retry without tools failed for OpenAI-compatible server: {}",
                                e
                            )));
                            let _ = tx.send(ChatEvent::Done {
                                finish_reason: "stop".to_string(),
                            });
                            return;
                        }
                    };
                } else {
                    let _ = tx.send(ChatEvent::Error(format!(
                        "OpenAI-compatible server returned {}: {}",
                        status, text
                    )));
                    let _ = tx.send(ChatEvent::Done {
                        finish_reason: "stop".to_string(),
                    });
                    return;
                }
            }

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                let _ = tx.send(ChatEvent::Error(format!(
                    "OpenAI-compatible server returned {}: {}",
                    status, text
                )));
                let _ = tx.send(ChatEvent::Done {
                    finish_reason: "stop".to_string(),
                });
                return;
            }

            let mut stream = response.bytes_stream();
            // Keep raw bytes until newline boundaries so multi-byte UTF-8 chars are
            // decoded only after the full SSE line is assembled.
            let mut buffer: Vec<u8> = Vec::new();
            let mut tool_call_deltas_buf: Vec<ToolCall> = Vec::new();
            let mut emitted_final_tool_calls = false;
            let mut emitted_done = false;

            'stream_loop: while let Some(chunk_result) = stream.next().await {
                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(ChatEvent::Error(format!("Stream error: {}", e)));
                        break;
                    }
                };

                buffer.extend_from_slice(&chunk);

                while let Some(line_end) = buffer.iter().position(|&b| b == b'\n') {
                    let mut line_bytes: Vec<u8> = buffer.drain(..=line_end).collect();
                    while matches!(line_bytes.last(), Some(b'\n' | b'\r')) {
                        line_bytes.pop();
                    }

                    if line_bytes.is_empty() {
                        continue;
                    }

                    let line = match std::str::from_utf8(&line_bytes) {
                        Ok(s) => s.trim(),
                        Err(e) => {
                            eprintln!("[OPENAI COMPAT] Skipping non-UTF8 SSE line: {}", e);
                            continue;
                        }
                    };

                    if line.is_empty() || line == "data: [DONE]" {
                        continue;
                    }

                    if let Some(json_str) = line.strip_prefix("data: ") {
                        if let Ok(parsed) = serde_json::from_str::<StreamChunk>(json_str) {
                            if let Some(choice) = parsed.choices.first() {
                                let content = choice.delta.content.as_ref().or_else(|| {
                                    choice
                                        .message
                                        .as_ref()
                                        .and_then(|message| message.content.as_ref())
                                });

                                // Handle text / reasoning deltas
                                if let Some(content) = content {
                                    if !content.is_empty() {
                                        if reasoning_parser.is_none()
                                            && contains_reasoning_marker(content)
                                        {
                                            reasoning_parser = Some(ReasoningParser::new());
                                        }

                                        if let Some(ref mut parser) = reasoning_parser {
                                            for segment in parser.process_segments(content) {
                                                match segment {
                                                    crate::reasoning_parser::ReasoningSegment::Text(text) => {
                                                        if !text.is_empty() {
                                                            let _ = tx.send_chunk(text);
                                                        }
                                                    }
                                                    crate::reasoning_parser::ReasoningSegment::Reasoning(reasoning) => {
                                                        if !reasoning.is_empty() {
                                                            let _ = tx.send_reasoning_chunk(reasoning);
                                                        }
                                                    }
                                                }
                                            }
                                        } else {
                                            let _ = tx.send_chunk(content.clone());
                                        }
                                    }
                                }

                                // Handle tool call deltas (OpenAI-compatible)
                                if !choice.delta.tool_calls.is_empty() {
                                    merge_tool_call_deltas(
                                        &mut tool_call_deltas_buf,
                                        &choice.delta.tool_calls,
                                    );
                                } else if let Some(message_tool_calls) = choice
                                    .message
                                    .as_ref()
                                    .and_then(|message| message.tool_calls.as_ref())
                                {
                                    tool_call_deltas_buf = message_tool_calls
                                        .iter()
                                        .map(|call| {
                                            let mut merged = call.clone();
                                            if merged.id.is_empty() {
                                                merged.id = uuid::Uuid::new_v4().to_string();
                                            }
                                            if merged.typ.is_empty() {
                                                merged.typ = "function".to_string();
                                            }
                                            merged.status = Some("executing".to_string());
                                            merged
                                        })
                                        .collect();
                                }

                                if choice.finish_reason.is_some() {
                                    // Flush any buffered content from reasoning parser before Done
                                    if let Some(ref mut parser) = reasoning_parser {
                                        for segment in parser.flush() {
                                            match segment {
                                                crate::reasoning_parser::ReasoningSegment::Text(text) => {
                                                    if !text.is_empty() {
                                                        let _ = tx.send_chunk(text);
                                                    }
                                                }
                                                crate::reasoning_parser::ReasoningSegment::Reasoning(reasoning) => {
                                                    if !reasoning.is_empty() {
                                                        let _ = tx.send_reasoning_chunk(reasoning);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    let calls: Vec<ToolCall> = tool_call_deltas_buf
                                        .iter()
                                        .filter(|call| !call.function.name.trim().is_empty())
                                        .map(|call| {
                                            let mut merged = call.clone();
                                            if merged.typ.is_empty() {
                                                merged.typ = "function".to_string();
                                            }
                                            merged.status = Some("executing".to_string());
                                            merged
                                        })
                                        .collect();

                                    if !calls.is_empty() {
                                        let _ = tx.send(ChatEvent::ToolCalls(calls));
                                        emitted_final_tool_calls = true;
                                    }

                                    let _ = tx.send(ChatEvent::Done {
                                        finish_reason: "stop".to_string(),
                                    });
                                    emitted_done = true;
                                    break 'stream_loop;
                                }
                            }
                        }
                    }
                }
            }

            // Flush any buffered content before sending Done
            if let Some(ref mut parser) = reasoning_parser {
                for segment in parser.flush() {
                    match segment {
                        crate::reasoning_parser::ReasoningSegment::Text(text) => {
                            if !text.is_empty() {
                                let _ = tx.send_chunk(text);
                            }
                        }
                        crate::reasoning_parser::ReasoningSegment::Reasoning(reasoning) => {
                            if !reasoning.is_empty() {
                                let _ = tx.send_reasoning_chunk(reasoning);
                            }
                        }
                    }
                }
            }

            if !emitted_final_tool_calls {
                let calls: Vec<ToolCall> = tool_call_deltas_buf
                    .iter()
                    .filter(|call| !call.function.name.trim().is_empty())
                    .map(|call| {
                        let mut merged = call.clone();
                        if merged.typ.is_empty() {
                            merged.typ = "function".to_string();
                        }
                        merged.status = Some("executing".to_string());
                        merged
                    })
                    .collect();

                if !calls.is_empty() {
                    let _ = tx.send(ChatEvent::ToolCalls(calls));
                }
            }

            if !emitted_done {
                let _ = tx.send(ChatEvent::Done {
                    finish_reason: "stop".to_string(),
                });
            }
        });

        Self::ensure_assistant_placeholder(conversation);
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
        is_local_mode: bool,
        http: reqwest::Client,
    ) -> Result<(), String> {
        self.pending_done_without_tools = false;

        // Agentic Loop Check
        if self.agentic_loop.is_active() {
            self.agentic_loop.increment_turn();
            if !self.agentic_loop.is_active() {
                return Err("Agentic loop stopped: max turns reached".to_string());
            }
        }

        // If every tool in this round failed, still continue by sending failures
        // back to the model. This lets the model recover (e.g. re-read file and
        // retry with adjusted edits) instead of dead-ending the turn.
        if !batch.file_results.is_empty()
            && batch.file_results.iter().all(|(_, result)| !result.success)
        {
            let mut details = Vec::new();
            for (call, result) in batch.file_results.iter().take(3) {
                let reason = result
                    .error
                    .clone()
                    .unwrap_or_else(|| "unknown tool error".to_string());
                details.push(format!("{}: {}", call.function.name, reason));
            }
            let suffix = if batch.file_results.len() > 3 {
                "; ..."
            } else {
                ""
            };
            eprintln!(
                "[CONTINUE TOOLS] All tool calls in batch failed, forwarding failures for model recovery. {}{}",
                details.join("; "),
                suffix
            );
        }

        // Store tool results in conversation history
        // RFC: Large Tool Result Handling - truncate in local mode
        for (_call, result) in batch.file_results.iter() {
            let full_content = result.to_tool_content_for_tool(&_call.function.name);
            let content = if is_local_mode {
                result.to_tool_content_truncated_for_tool(&_call.function.name)
            } else {
                full_content.clone()
            };
            let mut tool_msg = ChatMessage::new(ChatRole::Tool, content);
            if is_local_mode {
                tool_msg.backend_content = Some(full_content);
            }
            tool_msg.tool_call_id = Some(_call.id.clone());
            conversation.push(tool_msg);
        }

        // Update tool call status in the assistant message and store for emission
        // RFC: Large Tool Result Handling - truncate in local mode
        if let Some(updated_assistant) =
            conversation.update_tool_call_status_with_truncation(&batch.file_results, is_local_mode)
        {
            self.pending_results
                .push_back(DrainResult::ToolStatusUpdate(updated_assistant));
            self.updated_assistant_message = None;
        } else {
            self.updated_assistant_message = None;
        }
        self.sync_ws_conversation_messages(conversation);

        let selected = resolve_model_selection(models, selected_model);
        self.update_stream_profile(selected.provider, &selected.model_id_for_request);

        match selected.provider {
            ProviderId::Ollama => {
                return OllamaRuntime
                    .continue_after_tools(
                        self,
                        &batch,
                        conversation,
                        api_config,
                        &selected.model_id_for_request,
                        http,
                        workspace,
                        is_local_mode,
                    )
                    .map(|_| ());
            }
            ProviderId::OpenAiCompat => {
                // Keep rx open; start a fresh openai-compat stream to continue after tools
                return OpenAiCompatRuntime
                    .continue_after_tools(
                        self,
                        &batch,
                        conversation,
                        api_config,
                        &selected.model_id_for_request,
                        http,
                        workspace,
                        is_local_mode,
                    )
                    .map(|_| ());
            }
            ProviderId::Zaguan => {
                return ZaguanRuntime
                    .continue_after_tools(
                        self,
                        &batch,
                        conversation,
                        api_config,
                        &selected.model_id_for_request,
                        http,
                        workspace,
                        is_local_mode,
                    )
                    .map(|_| ());
            }
        }
    }

    pub(crate) fn continue_zaguan_after_tools(
        &mut self,
        batch: &PendingToolBatch,
        conversation: &mut ConversationHistory,
        workspace: Option<&PathBuf>,
        _is_local_mode: bool,
        model_id: &str,
    ) -> Result<(), String> {
        // Send tool results to Blade Protocol via WebSocket
        // We need to send ALL tool results, not just the first one
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| "No session ID available".to_string())?;

        // Reuse existing WebSocket client from start_stream
        let ws_client = self
            .ws_client
            .as_ref()
            .ok_or_else(|| "No WebSocket client available".to_string())?
            .clone();
        let results = batch.file_results.clone(); // Clone for the task
        let provider_event_tx = self.provider_event_tx.clone();
        let model_id = model_id.to_string();
        let _ = workspace;

        // Zaguan reuses the same websocket after tool results, so we need a fresh
        // assistant placeholder here. Without it, the next streamed chunks reuse
        // the pre-tool assistant message id and the final response renders inline
        // with the earlier text instead of as the trailing assistant turn.
        Self::ensure_assistant_placeholder(conversation);
        self.sync_ws_conversation_messages(conversation);

        // Send tool results through the existing WebSocket connection
        // No need to create a new connection - reuse the one from start_stream
        // eprintln!("[CHAT MGR] Sending {} tool results through existing connection", results.len());

        tokio::spawn(async move {
            // Send ALL results sequentially
            // RFC: Large Tool Result Handling - truncate in local mode
            for (call, result) in &results {
                let tool_content = result.to_tool_content_for_tool(&call.function.name);

                let tool_result = crate::blade_ws_client::ToolResult {
                    success: result.success,
                    content: tool_content,
                    error: if result.success {
                        None
                    } else {
                        Some(
                            result
                                .error
                                .clone()
                                .unwrap_or_else(|| "Tool execution failed".to_string()),
                        )
                    },
                };

                if let Err(e) = ws_client
                    .send_tool_result(
                        session_id.clone(),
                        call.id.clone(),
                        tool_result,
                        Some(model_id.clone()),
                    )
                    .await
                {
                    if let Some(tx) = &provider_event_tx {
                        let _ = tx.send(ChatEvent::Error(format!(
                            "Failed to send tool result for {}: {}",
                            call.function.name, e
                        )));
                    }
                    break;
                }
            }
            // eprintln!("[CHAT MGR] All {} tool results sent", results.len());
        });

        // The existing rx from start_stream will continue to receive events
        // No need to create a new rx or set streaming again
        Ok(())
    }

    pub fn drain_events(&mut self, conversation: &mut ConversationHistory) -> DrainResult {
        // v1.1 BATCHING FIX: Process pending results first
        if let Some(res) = self.pending_results.pop_front() {
            return res;
        }

        let Some(rx) = self.rx.as_ref() else {
            return DrainResult::None;
        };

        let mut events: Vec<ProviderEvent> = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(ev) => events.push(ev),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Terminal state: producer side is gone, so this turn cannot emit more events.
                    // Clear receiver + streaming so orchestrator can finalize instead of waiting forever.
                    self.rx = None;
                    self.streaming = false;
                    break;
                }
            }
        }

        if events.is_empty() {
            if self.pending_done_without_tools {
                // Confirmed no trailing events after Done on previous drain cycle.
                self.pending_done_without_tools = false;
                self.rx = None;
                self.streaming = false;

                let no_error: Option<String> = None;
                self.finalize_turn(conversation, None, &no_error);
                self.reasoning_parser.reset();

                let msg_id = conversation
                    .last_assistant()
                    .and_then(|msg| msg.id.clone())
                    .or_else(|| Some(uuid::Uuid::new_v4().to_string()));
                if let Some(id) = msg_id {
                    self.pending_results
                        .push_back(DrainResult::MessageCompleted(id));
                }

                if let Some(msg) = self.updated_assistant_message.take() {
                    self.pending_results
                        .push_back(DrainResult::ToolStatusUpdate(msg));
                }

                return self
                    .pending_results
                    .pop_front()
                    .unwrap_or(DrainResult::None);
            }
            return DrainResult::None;
        }

        let is_openai_text = self.stream_plain_text;
        let use_reasoning_parser = self.stream_parse_reasoning;

        let mut batched_chunk = String::new();
        let mut done = false;
        let mut error_msg: Option<String> = None;
        let mut server_disconnected = false;

        // Helper macro to flush current batch
        macro_rules! flush_batch {
            () => {
                if !batched_chunk.is_empty() {
                    let s = batched_chunk.clone();

                    if let Some(assistant_msg) = conversation.last_assistant_mut() {
                        if is_openai_text {
                            assistant_msg.content.push_str(&s);
                            let mid = assistant_msg.id.clone().unwrap_or_default();
                            self.pending_results.push_back(DrainResult::Update(mid, s));
                        } else {
                            let segments = self.process_incoming_chunk(
                                &s,
                                assistant_msg,
                                use_reasoning_parser,
                            );
                            for segment in segments {
                                match segment {
                                    crate::reasoning_parser::ReasoningSegment::Text(text) => {
                                        if text.is_empty() {
                                            continue;
                                        }
                                        if assistant_msg.tool_calls.is_some()
                                            && assistant_msg.content_before_tools.is_some()
                                            && !xml_parser::is_xml_tool_output(&text)
                                        {
                                            let after = assistant_msg
                                                .content_after_tools
                                                .get_or_insert_with(String::new);
                                            after.push_str(&text);
                                        }
                                        let mid = assistant_msg.id.clone().unwrap_or_default();
                                        self.pending_results
                                            .push_back(DrainResult::Update(mid, text));
                                    }
                                    crate::reasoning_parser::ReasoningSegment::Reasoning(
                                        reasoning,
                                    ) => {
                                        if reasoning.is_empty() {
                                            continue;
                                        }
                                        let r =
                                            assistant_msg.reasoning.get_or_insert_with(String::new);
                                        r.push_str(&reasoning);
                                        let mid = assistant_msg.id.clone().unwrap_or_default();
                                        self.pending_results
                                            .push_back(DrainResult::Reasoning(mid, reasoning));
                                    }
                                }
                            }
                        }
                    } else {
                        conversation.push(ChatMessage::new(ChatRole::Assistant, String::new()));
                        if let Some(new_last) = conversation.last_mut() {
                            if is_openai_text {
                                new_last.content.push_str(&s);
                                let mid = new_last.id.clone().unwrap_or_default();
                                self.pending_results.push_back(DrainResult::Update(mid, s));
                            } else {
                                let segments =
                                    self.process_incoming_chunk(&s, new_last, use_reasoning_parser);
                                for segment in segments {
                                    match segment {
                                        crate::reasoning_parser::ReasoningSegment::Text(text) => {
                                            if text.is_empty() {
                                                continue;
                                            }
                                            let mid = new_last.id.clone().unwrap_or_default();
                                            self.pending_results
                                                .push_back(DrainResult::Update(mid, text));
                                        }
                                        crate::reasoning_parser::ReasoningSegment::Reasoning(
                                            reasoning,
                                        ) => {
                                            if reasoning.is_empty() {
                                                continue;
                                            }
                                            let r =
                                                new_last.reasoning.get_or_insert_with(String::new);
                                            r.push_str(&reasoning);
                                            let mid = new_last.id.clone().unwrap_or_default();
                                            self.pending_results
                                                .push_back(DrainResult::Reasoning(mid, reasoning));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    batched_chunk.clear();
                }
            };
        }

        for provider_event in events {
            let is_done_event = matches!(&provider_event, ProviderEvent::Done { .. });
            if !is_done_event {
                self.pending_done_without_tools = false;
            }

            match provider_event {
                ProviderEvent::Chunk(s) => {
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
                ProviderEvent::ToolActivity(payload) => {
                    flush_batch!();
                    // Track tool_call_ids from tool_progress so we can detect dropped tool calls
                    if payload.action == "streaming" {
                        if let Some(ref id) = payload.tool_call_id {
                            self.pending_tool_progress
                                .insert(id.clone(), payload.tool_name.clone());
                        }
                    }
                    self.pending_results.push_back(DrainResult::ToolActivity {
                        tool_name: payload.tool_name,
                        file_path: payload.file_path,
                        action: payload.action,
                        tool_call_id: payload.tool_call_id,
                    });
                }
                ProviderEvent::CognitiveInterrupt(payload) => {
                    flush_batch!();
                    self.pending_results
                        .push_back(DrainResult::CognitiveInterrupt(payload));
                }
                ProviderEvent::ApprovalRequest(payload) => {
                    flush_batch!();
                    self.pending_results
                        .push_back(DrainResult::ApprovalRequest(payload));
                }
                ProviderEvent::ReasoningChunk(s) => {
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
                    // Flush any pending text batch before emitting reasoning
                    flush_batch!();
                    // Emit reasoning immediately for incremental display and proper separation from text
                    if let Some(assistant_msg) = conversation.last_assistant_mut() {
                        let r = assistant_msg.reasoning.get_or_insert_with(String::new);
                        r.push_str(&s);
                        let mid = assistant_msg.id.clone().unwrap_or_default();
                        self.pending_results
                            .push_back(DrainResult::Reasoning(mid, s));
                    }
                }

                ProviderEvent::Session {
                    session_id,
                    model,
                    planning_mode,
                    runtime_mode,
                    mode_source,
                } => {
                    flush_batch!();
                    // eprintln!("[CHAT MGR] Storing session_id: {}", session_id);
                    self.session_id = Some(session_id.clone());
                    self.planning_mode = planning_mode;
                    self.runtime_mode = runtime_mode.clone();
                    self.mode_source = mode_source.clone();
                    conversation.metadata.session_id = Some(session_id);
                    conversation.metadata.model_id = model;
                    conversation.metadata.planning_mode = planning_mode;
                    conversation.metadata.runtime_mode = runtime_mode;
                    conversation.metadata.mode_source = mode_source;
                }
                ProviderEvent::Research {
                    content,
                    suggested_name,
                } => {
                    flush_batch!();
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
                ProviderEvent::ToolCalls(calls) => {
                    flush_batch!();
                    // Clear matching tool_progress tracking - these tool calls arrived successfully
                    for call in &calls {
                        self.pending_tool_progress.remove(&call.id);
                    }
                    for call in &calls {
                        if let Some(idx) = self
                            .accumulated_tool_calls
                            .iter()
                            .position(|existing| existing.id == call.id)
                        {
                            self.accumulated_tool_calls[idx] = call.clone();
                        } else {
                            self.accumulated_tool_calls.push(call.clone());
                        }
                    }
                    let calls_for_emit = calls.clone();

                    if let Some(last) = conversation.last_assistant_mut() {
                        if last.content_before_tools.is_none() {
                            last.content_before_tools = Some(last.content.clone());
                        }
                        let existing = last.tool_calls.get_or_insert_with(Vec::new);
                        for call in calls {
                            if let Some(idx) = existing
                                .iter()
                                .position(|existing_call| existing_call.id == call.id)
                            {
                                existing[idx] = call;
                            } else {
                                existing.push(call);
                            }
                        }

                        self.pending_results
                            .push_back(DrainResult::ToolCreated(last.clone(), calls_for_emit));
                    }
                }
                ProviderEvent::Progress {
                    message,
                    stage,
                    percent,
                } => {
                    flush_batch!();
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
                ProviderEvent::TodoUpdated(todos) => {
                    flush_batch!();
                    // eprintln!("[DRAIN] Todo updated: {} items", todos.len());
                    self.pending_results
                        .push_back(DrainResult::TodoUpdated(todos));
                }
                ProviderEvent::Done { finish_reason } => {
                    flush_batch!();
                    // NOTE: Do NOT clear rx here.
                    // Done can mean either:
                    // - true end-of-turn (final assistant response)
                    // - boundary where the model is yielding to tools and will send more ToolCall events
                    // We decide whether to clear rx / emit MessageCompleted later, once we know if
                    // there are accumulated tool calls.
                    // eprintln!("[DRAIN] ProviderEvent::Done received");

                    // NOTE: Do not infer MessageTooLarge from pending tool-progress alone.
                    // Some providers emit optimistic ToolProgress streaming updates without a
                    // corresponding ToolCalls payload in edge cases (or empty follow-up turns),
                    // which caused false "Tool call(s) dropped" errors in the UI.
                    // We only surface size-limit failures when the provider emits
                    // ProviderEvent::MessageTooLarge explicitly.
                    self.pending_tool_progress.clear();

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
                    if self.stream_xml_tool_fallback
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
                                eprintln!("[QWEN] Detected {} XML tool calls", xml_calls.len());
                                self.accumulated_tool_calls
                                    .extend(self.convert_xml_calls(xml_calls));
                            }
                        }
                    }

                    // eprintln!("[DRAIN] chat_done received");
                    self.last_finish_reason = Some(finish_reason);
                    done = true;
                }
                ProviderEvent::Disconnected { message } => {
                    flush_batch!();
                    server_disconnected = true;
                    self.last_finish_reason = Some("server_disconnected".to_string());

                    if let Some(last) = conversation.last_assistant_mut() {
                        if last.progress.is_some() {
                            last.progress = None;
                            self.pending_results.push_back(DrainResult::Progress {
                                message: "Disconnected".to_string(),
                                stage: "disconnected".to_string(),
                                percent: 100,
                            });
                        }

                        let has_visible_content = !last.content.trim().is_empty()
                            || last
                                .reasoning
                                .as_ref()
                                .map(|reasoning| !reasoning.trim().is_empty())
                                .unwrap_or(false)
                            || last
                                .tool_calls
                                .as_ref()
                                .map(|calls| !calls.is_empty())
                                .unwrap_or(false);
                        if !has_visible_content {
                            last.content.push_str(&message);
                            let mid = last.id.clone().unwrap_or_default();
                            self.pending_results
                                .push_back(DrainResult::Update(mid, message));
                        }

                        let mut updated_tools = false;
                        if let Some(calls) = last.tool_calls.as_mut() {
                            for call in calls {
                                if call.status.as_deref() == Some("executing") {
                                    call.status = Some("failed".to_string());
                                    call.result = Some(
                                        "Server disconnected before this tool could run."
                                            .to_string(),
                                    );
                                    updated_tools = true;
                                }
                            }
                        }
                        if updated_tools {
                            self.pending_results
                                .push_back(DrainResult::ToolStatusUpdate(last.clone()));
                        }
                    }

                    done = true;
                }
                ProviderEvent::Error(e) => {
                    flush_batch!();
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
                ProviderEvent::ContextLengthExceeded {
                    message,
                    token_count,
                    max_tokens,
                    excess,
                    recoverable,
                    recovery_hint,
                } => {
                    flush_batch!();
                    // RFC: Context Length Recovery - emit the event to frontend
                    // eprintln!("[DRAIN] Context length exceeded: {} (tokens: {:?}/{:?}, recoverable: {})",
                    //     message, token_count, max_tokens, recoverable);
                    self.pending_results
                        .push_back(DrainResult::ContextLengthExceeded {
                            message,
                            token_count,
                            max_tokens,
                            excess,
                            recoverable,
                            recovery_hint,
                        });
                    // Don't set done=true - session is still valid, user can continue
                }
                ProviderEvent::MessageTooLarge {
                    message,
                    recovery_hint,
                } => {
                    flush_batch!();
                    // Message size limit exceeded - emit the event to frontend
                    // eprintln!("[DRAIN] Message too large: {} (hint: {})", message, recovery_hint);
                    self.pending_results
                        .push_back(DrainResult::MessageTooLarge {
                            message,
                            recovery_hint,
                        });
                    // Don't set done=true - this is recoverable, model can retry
                }
            }
        }

        flush_batch!();

        if done {
            let tool_calls = if !self.accumulated_tool_calls.is_empty() {
                // eprintln!(
                //     "[DRAIN] Found {} accumulated tool calls",
                //     self.accumulated_tool_calls.len()
                // );
                Some(self.accumulated_tool_calls.clone())
            } else {
                // eprintln!("[DRAIN] No accumulated tool calls");
                None
            };
            self.accumulated_tool_calls.clear();
            let tool_calls = if server_disconnected {
                None
            } else {
                tool_calls
            };

            // If there are no tool calls, this is a true end-of-turn.
            // Only then should we clear rx and emit MessageCompleted (frontend uses this to reset loading).
            let should_complete_turn = tool_calls.is_none() && error_msg.is_none();
            let should_defer_completion = should_complete_turn
                && !server_disconnected
                && matches!(self.active_provider, ProviderId::Zaguan)
                && !self.pending_done_without_tools;

            if should_defer_completion {
                // Some Zaguan streams can emit Done slightly before trailing events.
                // Wait one additional drain cycle before finalizing no-tool completion.
                self.pending_done_without_tools = true;
                return self
                    .pending_results
                    .pop_front()
                    .unwrap_or(DrainResult::None);
            }

            self.pending_done_without_tools = false;

            // If channel was closed but tool calls are pending, we keep rx until follow-up streams/tools resolve.
            if should_complete_turn {
                // eprintln!("[DRAIN] Turn complete: clearing rx + emitting MessageCompleted");
                self.rx = None;
                self.streaming = false; // Disable streaming to deactivate Stop button

                let msg_id = conversation
                    .last_assistant()
                    .and_then(|msg| msg.id.clone())
                    .or_else(|| Some(uuid::Uuid::new_v4().to_string()));
                if let Some(id) = msg_id {
                    self.pending_results
                        .push_back(DrainResult::MessageCompleted(id));
                }
            } else {
                // eprintln!("[DRAIN] Done received but tool calls pending: keeping rx open");
            }

            // eprintln!(
            //     "[DRAIN] Calling finalize_turn with tool_calls: {:?}",
            //     tool_calls.as_ref().map(|c| c.len())
            // );
            self.finalize_turn(conversation, tool_calls.clone(), &error_msg);

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
        result
    }

    fn process_incoming_chunk(
        &mut self,
        chunk: &str,
        last_msg: &mut ChatMessage,
        parse_reasoning: bool,
    ) -> Vec<crate::reasoning_parser::ReasoningSegment> {
        if !parse_reasoning {
            self.append_content(chunk, last_msg);
            return vec![crate::reasoning_parser::ReasoningSegment::Text(
                chunk.to_string(),
            )];
        }

        // v1.2: Use ReasoningParser for multi-format reasoning extraction
        let segments = self.reasoning_parser.process_segments(chunk);

        for segment in &segments {
            match segment {
                crate::reasoning_parser::ReasoningSegment::Text(text) => {
                    if !text.is_empty() {
                        self.append_content(text, last_msg);
                    }
                }
                crate::reasoning_parser::ReasoningSegment::Reasoning(reasoning) => {
                    if !reasoning.is_empty() {
                        let r = last_msg.reasoning.get_or_insert_with(String::new);
                        r.push_str(reasoning);
                    }
                }
            }
        }

        segments
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
    ) {
        let auto_start_loop_on_tools = self.stream_auto_start_loop_on_tools;

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
        } else if auto_start_loop_on_tools && has_tool_calls {
            // Auto-start loop for Qwen if tools are used
            eprintln!("[AGENTIC LOOP] Auto-starting for tool execution");
            self.agentic_loop.start();
        }

        // 2. Add tool calls to history
        if let Some(last) = conversation.last_assistant_mut() {
            if let Some(calls) = tool_calls.clone() {
                last.tool_calls = Some(calls);
            }

            if last.content.trim().is_empty() && last.tool_calls.is_none() && error_msg.is_none() {
                // If barely anything happened, mark it
                last.content = "[no content]".to_string();
            }
        }

        self.sync_ws_conversation_messages(conversation);
    }

    /// Request to stop the current streaming response
    pub fn begin_stop(&mut self) -> bool {
        let was_active = self.abort_handle.is_some()
            || self.streaming
            || self.rx.is_some()
            || self.agentic_loop.is_active();

        self.streaming = false;
        self.rx = None;
        self.stop_requested = true;
        self.reasoning_parser.reset();
        self.agentic_loop.stop("User requested stop");
        was_active
    }

    pub fn abort_stream_task(&mut self) {
        if let Some(handle) = self.abort_handle.take() {
            handle.abort();
        }
    }

    pub fn request_stop(&mut self) -> bool {
        let was_active = self.begin_stop();
        self.abort_stream_task();
        was_active
    }

    pub fn stop_requested(&self) -> bool {
        self.stop_requested
    }

    pub fn stop_signal_target(
        &self,
    ) -> Option<(Arc<BladeWsClient>, Option<String>, Option<String>)> {
        Some((
            self.ws_client.as_ref()?.clone(),
            self.active_request_id.clone(),
            self.session_id.clone(),
        ))
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
    use crate::protocol::{ChatImage, ChatMessage, ChatRole};

    #[test]
    fn blade_conversation_context_uses_backend_content_for_user_image_turns() {
        let mut conversation = ConversationHistory::new();
        let mut user = ChatMessage::new(ChatRole::User, "Fix this".to_string());
        user.backend_content =
            Some("[SYSTEM NOTE: Use the active workspace context.]\n\nFix this".to_string());
        user.images = Some(vec![ChatImage {
            data: "abc123".to_string(),
            mime_type: "image/png".to_string(),
            name: Some("capture.png".to_string()),
            size: Some(6),
        }]);
        conversation.push(user);

        let messages = ChatManager::to_blade_conversation_messages(&conversation, false);

        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].get("content").and_then(|value| value.as_str()),
            Some("[SYSTEM NOTE: Use the active workspace context.]\n\nFix this")
        );
        let images = messages[0]
            .get("images")
            .and_then(|value| value.as_array())
            .expect("expected images on latest user turn");
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn blade_conversation_context_uses_backend_content_for_tool_results() {
        let mut conversation = ConversationHistory::new();
        let mut tool = ChatMessage::new(ChatRole::Tool, "[TRUNCATED]".to_string());
        tool.backend_content = Some("full workspace analysis result".to_string());
        tool.tool_call_id = Some("call-1".to_string());
        conversation.push(tool);

        let messages = ChatManager::to_blade_conversation_messages(&conversation, false);

        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].get("role").and_then(|value| value.as_str()),
            Some("tool")
        );
        assert_eq!(
            messages[0].get("content").and_then(|value| value.as_str()),
            Some("full workspace analysis result")
        );
        assert_eq!(
            messages[0]
                .get("tool_call_id")
                .and_then(|value| value.as_str()),
            Some("call-1")
        );
    }

    #[test]
    fn blade_conversation_context_can_exclude_latest_user_turn() {
        let mut conversation = ConversationHistory::new();
        conversation.push(ChatMessage::new(
            ChatRole::User,
            "Earlier request".to_string(),
        ));
        conversation.push(ChatMessage::new(
            ChatRole::Assistant,
            "Earlier answer".to_string(),
        ));

        let mut latest = ChatMessage::new(
            ChatRole::User,
            "Long latest request about Nagomi".to_string(),
        );
        latest.backend_content =
            Some("Long latest request about Nagomi with backend notes".to_string());
        conversation.push(latest);

        let messages = ChatManager::to_blade_conversation_messages(&conversation, true);

        assert_eq!(messages.len(), 2);
        assert_eq!(
            messages[0].get("content").and_then(|value| value.as_str()),
            Some("Earlier request")
        );
        assert_eq!(
            messages[1].get("content").and_then(|value| value.as_str()),
            Some("Earlier answer")
        );
        assert!(!messages.iter().any(|message| message
            .get("content")
            .and_then(|value| value.as_str())
            .is_some_and(|content| content.contains("Nagomi"))));
    }

    #[test]
    fn zblade_workflow_guidance_is_added_to_empty_prompt() {
        let prompt = apply_zblade_workflow_guidance(None);

        assert!(prompt.contains("fast_context"));
        assert!(prompt.contains("edit_impact"));
        assert!(prompt.contains("suggested_ranges"));
        assert!(prompt.contains("Do not answer only that you are ready"));
        assert!(prompt.contains("use the provided tools immediately"));
    }

    #[test]
    fn zblade_workflow_guidance_leads_with_symbols_first_navigation_rule() {
        let guidance = zblade_workflow_guidance();
        let first_bullet = guidance
            .lines()
            .find(|line| line.starts_with("- "))
            .expect("guidance has bullets");

        assert!(first_bullet.contains("structural code navigation"));
        assert!(first_bullet.contains("symbol_search"));
        assert!(first_bullet.contains("symbol_outline"));
        assert!(first_bullet.contains("incoming `call`"));
        assert!(first_bullet.contains("incoming `implements`"));
        assert!(guidance.contains("never construct one from a path and name"));
        assert!(guidance.contains("after a focused Symbols miss"));
        assert!(guidance.contains("builds, tests, Git, or runtime diagnosis"));
    }

    #[test]
    fn zblade_workflow_guidance_preserves_existing_prompt_once() {
        let prompt = apply_zblade_workflow_guidance(Some("Base prompt".to_string()));
        let second = apply_zblade_workflow_guidance(Some(prompt.clone()));

        assert!(prompt.starts_with("Base prompt"));
        assert_eq!(second.matches("ZBlade workflow guidance:").count(), 1);
    }

    #[test]
    fn local_system_prompt_falls_back_to_bundled_default() {
        let prompt = load_local_system_prompt(
            "test-model-without-local-prompt",
            "/workspace/project",
            "src/main.rs",
            "linux",
            "zsh",
            "2026-06-03",
            "10:00:00 +0200",
            None,
        )
        .expect("fallback prompt");

        assert!(prompt.contains("You are an AI coding assistant in Zaguán Blade"));
        assert!(prompt.contains("glm-4.7-flash"));
        assert!(prompt.contains("symbol_search"));
        assert!(!prompt.contains("ZBlade workflow guidance:"));
        assert!(!prompt.contains("{{WORKSPACE_ROOT}}"));
        assert!(!prompt.trim_start().starts_with(GEMMA4_THINK_TOKEN));
    }

    #[test]
    fn local_system_prompt_uses_cloud_default_for_cloud_models() {
        let prompt = load_local_system_prompt(
            "nemotron-3-ultra:cloud",
            "/workspace/project",
            "src/main.rs",
            "linux",
            "zsh",
            "2026-06-13",
            "10:00:00 +0200",
            None,
        )
        .expect("fallback prompt");

        assert!(prompt.contains("You are Zaguán Blade, a senior pair programmer"));
        assert!(prompt.contains("Workspace root: /workspace/project"));
        assert!(prompt.contains("Active file: src/main.rs"));
        assert!(prompt.contains("Current date: 2026-06-13"));
        assert!(!prompt.contains("{{CURRENT_DATE}}"));
        assert!(!prompt.contains("ZBlade workflow guidance:"));
    }

    #[test]
    fn local_system_prompt_appends_agent_instructions() {
        let prompt = load_local_system_prompt(
            "test-model-without-local-prompt",
            "/workspace/project",
            "src/main.rs",
            "linux",
            "zsh",
            "2026-06-03",
            "10:00:00 +0200",
            Some("## AGENTS.md\n\nUse the user's workflow."),
        )
        .expect("fallback prompt");

        assert!(prompt.contains("You are an AI coding assistant in Zaguán Blade"));
        assert!(prompt.contains("# Repository Instructions"));
        assert!(prompt.contains("Use the user's workflow."));
    }

    #[test]
    fn request_stop_sets_explicit_stop_flag() {
        let mut chat_manager = ChatManager::new(50);

        assert!(!chat_manager.stop_requested());
        chat_manager.request_stop();
        assert!(chat_manager.stop_requested());
    }

    #[test]
    fn test_start_stream_adds_assistant_placeholder() {
        let mut chat_manager = ChatManager::new(50);
        chat_manager.request_stop();
        assert!(chat_manager.stop_requested());

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
                None, // mode
                true,
            );

            assert!(
                !chat_manager.stop_requested(),
                "Starting a new turn should clear any stale stop flag"
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

    #[test]
    fn ensure_assistant_placeholder_starts_new_tail_after_tool_messages() {
        let mut conversation = ConversationHistory::new();
        conversation.push(ChatMessage::new(
            ChatRole::User,
            "Run the build".to_string(),
        ));

        let mut assistant =
            ChatMessage::new(ChatRole::Assistant, "Checking the project".to_string());
        assistant.tool_calls = Some(vec![ToolCall {
            id: "call-1".to_string(),
            typ: "function".to_string(),
            function: ToolFunction {
                name: "run_command".to_string(),
                arguments: "{\"command\":\"npm run build\"}".to_string(),
            },
            status: Some("complete".to_string()),
            result: Some("ok".to_string()),
        }]);
        conversation.push(assistant);

        let mut tool = ChatMessage::new(ChatRole::Tool, "Build passed".to_string());
        tool.tool_call_id = Some("call-1".to_string());
        conversation.push(tool);

        ChatManager::ensure_assistant_placeholder(&mut conversation);
        let len_after_first_insert = conversation.len();
        ChatManager::ensure_assistant_placeholder(&mut conversation);

        assert_eq!(len_after_first_insert, 4);
        assert_eq!(
            conversation.len(),
            4,
            "placeholder insertion should be idempotent"
        );
        assert_eq!(conversation.last().unwrap().role, ChatRole::Assistant);
        assert!(
            conversation.last().unwrap().content.is_empty(),
            "new tail assistant should be a fresh placeholder"
        );
    }
}
