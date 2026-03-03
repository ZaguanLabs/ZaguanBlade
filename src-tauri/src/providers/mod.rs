use std::path::PathBuf;

use crate::ai_workflow::PendingToolBatch;
use crate::chat_manager::ChatManager;
use crate::config::ApiConfig;
use crate::conversation::ConversationHistory;
use crate::models::registry::ModelInfo;
use crate::protocol::{ChatEvent, TodoItem, ToolActivityPayload, ToolCall};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderId {
    Zaguan,
    Ollama,
    OpenAiCompat,
}

#[derive(Debug, Clone)]
pub struct ModelSelection {
    pub provider: ProviderId,
    pub model_id_for_request: String,
}

pub struct ProviderSessionHandle {
    pub started: bool,
}

pub enum ProviderEvent {
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
    ContextLengthExceeded {
        message: String,
        token_count: Option<u64>,
        max_tokens: Option<u64>,
        excess: Option<u64>,
        recoverable: bool,
        recovery_hint: Option<String>,
    },
    MessageTooLarge {
        message: String,
        recovery_hint: String,
    },
}

impl From<ChatEvent> for ProviderEvent {
    fn from(value: ChatEvent) -> Self {
        match value {
            ChatEvent::Session { session_id, model } => Self::Session { session_id, model },
            ChatEvent::Chunk(text) => Self::Chunk(text),
            ChatEvent::ReasoningChunk(text) => Self::ReasoningChunk(text),
            ChatEvent::Research {
                content,
                suggested_name,
            } => Self::Research {
                content,
                suggested_name,
            },
            ChatEvent::ToolCalls(calls) => Self::ToolCalls(calls),
            ChatEvent::TodoUpdated(todos) => Self::TodoUpdated(todos),
            ChatEvent::Progress {
                message,
                stage,
                percent,
            } => Self::Progress {
                message,
                stage,
                percent,
            },
            ChatEvent::ToolActivity(payload) => Self::ToolActivity(payload),
            ChatEvent::Done => Self::Done,
            ChatEvent::Error(message) => Self::Error(message),
            ChatEvent::ContextLengthExceeded {
                message,
                token_count,
                max_tokens,
                excess,
                recoverable,
                recovery_hint,
            } => Self::ContextLengthExceeded {
                message,
                token_count,
                max_tokens,
                excess,
                recoverable,
                recovery_hint,
            },
            ChatEvent::MessageTooLarge {
                message,
                recovery_hint,
            } => Self::MessageTooLarge {
                message,
                recovery_hint,
            },
        }
    }
}

impl From<ProviderEvent> for ChatEvent {
    fn from(value: ProviderEvent) -> Self {
        match value {
            ProviderEvent::Session { session_id, model } => ChatEvent::Session { session_id, model },
            ProviderEvent::Chunk(text) => ChatEvent::Chunk(text),
            ProviderEvent::ReasoningChunk(text) => ChatEvent::ReasoningChunk(text),
            ProviderEvent::Research {
                content,
                suggested_name,
            } => ChatEvent::Research {
                content,
                suggested_name,
            },
            ProviderEvent::ToolCalls(calls) => ChatEvent::ToolCalls(calls),
            ProviderEvent::TodoUpdated(todos) => ChatEvent::TodoUpdated(todos),
            ProviderEvent::Progress {
                message,
                stage,
                percent,
            } => ChatEvent::Progress {
                message,
                stage,
                percent,
            },
            ProviderEvent::ToolActivity(payload) => ChatEvent::ToolActivity(payload),
            ProviderEvent::Done => ChatEvent::Done,
            ProviderEvent::Error(message) => ChatEvent::Error(message),
            ProviderEvent::ContextLengthExceeded {
                message,
                token_count,
                max_tokens,
                excess,
                recoverable,
                recovery_hint,
            } => ChatEvent::ContextLengthExceeded {
                message,
                token_count,
                max_tokens,
                excess,
                recoverable,
                recovery_hint,
            },
            ProviderEvent::MessageTooLarge {
                message,
                recovery_hint,
            } => ChatEvent::MessageTooLarge {
                message,
                recovery_hint,
            },
        }
    }
}

pub trait AiProviderRuntime {
    fn provider_id(&self) -> ProviderId;

    fn start_turn(
        &self,
        manager: &mut ChatManager,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        http: reqwest::Client,
        workspace: Option<&PathBuf>,
        active_file: Option<String>,
        open_files: Option<Vec<String>>,
        cursor_line: Option<usize>,
        cursor_column: Option<usize>,
        storage_mode: Option<String>,
        composite_tools_enabled: bool,
    ) -> Result<ProviderSessionHandle, String>;

    fn continue_after_tools(
        &self,
        manager: &mut ChatManager,
        batch: &PendingToolBatch,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        http: reqwest::Client,
        workspace: Option<&PathBuf>,
        is_local_mode: bool,
    ) -> Result<ProviderSessionHandle, String>;
}

pub struct OllamaRuntime;
pub struct OpenAiCompatRuntime;
pub struct ZaguanRuntime;

impl AiProviderRuntime for OllamaRuntime {
    fn provider_id(&self) -> ProviderId {
        ProviderId::Ollama
    }

    fn start_turn(
        &self,
        manager: &mut ChatManager,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        http: reqwest::Client,
        workspace: Option<&PathBuf>,
        active_file: Option<String>,
        _open_files: Option<Vec<String>>,
        _cursor_line: Option<usize>,
        _cursor_column: Option<usize>,
        _storage_mode: Option<String>,
        composite_tools_enabled: bool,
    ) -> Result<ProviderSessionHandle, String> {
        manager.start_ollama_stream(
            conversation,
            api_config,
            model_id,
            http,
            workspace,
            active_file,
            composite_tools_enabled,
        )?;

        Ok(ProviderSessionHandle { started: true })
    }

    fn continue_after_tools(
        &self,
        manager: &mut ChatManager,
        _batch: &PendingToolBatch,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        http: reqwest::Client,
        workspace: Option<&PathBuf>,
        _is_local_mode: bool,
    ) -> Result<ProviderSessionHandle, String> {
        manager.start_ollama_stream(
            conversation,
            api_config,
            model_id,
            http,
            workspace,
            None,
            manager.composite_tools_enabled(),
        )?;
        Ok(ProviderSessionHandle { started: true })
    }
}

impl AiProviderRuntime for OpenAiCompatRuntime {
    fn provider_id(&self) -> ProviderId {
        ProviderId::OpenAiCompat
    }

    fn start_turn(
        &self,
        manager: &mut ChatManager,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        http: reqwest::Client,
        workspace: Option<&PathBuf>,
        active_file: Option<String>,
        _open_files: Option<Vec<String>>,
        _cursor_line: Option<usize>,
        _cursor_column: Option<usize>,
        _storage_mode: Option<String>,
        composite_tools_enabled: bool,
    ) -> Result<ProviderSessionHandle, String> {
        manager.start_openai_compat_stream(
            conversation,
            api_config,
            model_id,
            http,
            workspace,
            active_file,
            composite_tools_enabled,
        )?;

        Ok(ProviderSessionHandle { started: true })
    }

    fn continue_after_tools(
        &self,
        manager: &mut ChatManager,
        _batch: &PendingToolBatch,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        http: reqwest::Client,
        workspace: Option<&PathBuf>,
        _is_local_mode: bool,
    ) -> Result<ProviderSessionHandle, String> {
        manager.start_openai_compat_stream(
            conversation,
            api_config,
            model_id,
            http,
            workspace,
            None,
            manager.composite_tools_enabled(),
        )?;
        Ok(ProviderSessionHandle { started: true })
    }
}

impl AiProviderRuntime for ZaguanRuntime {
    fn provider_id(&self) -> ProviderId {
        ProviderId::Zaguan
    }

    fn start_turn(
        &self,
        manager: &mut ChatManager,
        conversation: &mut ConversationHistory,
        api_config: &ApiConfig,
        model_id: &str,
        _http: reqwest::Client,
        workspace: Option<&PathBuf>,
        active_file: Option<String>,
        open_files: Option<Vec<String>>,
        cursor_line: Option<usize>,
        cursor_column: Option<usize>,
        storage_mode: Option<String>,
        _composite_tools_enabled: bool,
    ) -> Result<ProviderSessionHandle, String> {
        manager.start_zaguan_stream(
            conversation,
            api_config,
            model_id,
            workspace,
            active_file,
            open_files,
            cursor_line,
            cursor_column,
            storage_mode,
        )?;

        Ok(ProviderSessionHandle { started: true })
    }

    fn continue_after_tools(
        &self,
        manager: &mut ChatManager,
        batch: &PendingToolBatch,
        conversation: &mut ConversationHistory,
        _api_config: &ApiConfig,
        _model_id: &str,
        _http: reqwest::Client,
        workspace: Option<&PathBuf>,
        is_local_mode: bool,
    ) -> Result<ProviderSessionHandle, String> {
        manager.continue_zaguan_after_tools(batch, conversation, workspace, is_local_mode)?;
        Ok(ProviderSessionHandle { started: true })
    }
}

pub fn resolve_model_selection(models: &[ModelInfo], selected_model: usize) -> ModelSelection {
    let selected = models.get(selected_model);

    let provider = selected
        .and_then(|m| m.provider.as_deref())
        .map(ProviderId::from_provider_str)
        .unwrap_or(ProviderId::Zaguan);

    let model_id_for_request = selected
        .map(|m| match provider {
            ProviderId::Ollama | ProviderId::OpenAiCompat => m.id.clone(),
            ProviderId::Zaguan => m.api_id.as_ref().unwrap_or(&m.id).clone(),
        })
        .unwrap_or_else(|| "anthropic/claude-sonnet-4-5-20250929".to_string());

    ModelSelection {
        provider,
        model_id_for_request,
    }
}

impl ProviderId {
    pub fn from_provider_str(provider: &str) -> Self {
        match provider {
            "ollama" => Self::Ollama,
            "openai-compat" => Self::OpenAiCompat,
            _ => Self::Zaguan,
        }
    }
}
