use crate::protocol::{ChatMessage, ChatRole, OpenAiMessage, ToolCall};

use crate::conversation_store::{generate_title, ConversationMetadata, StoredConversation};
use chrono::Utc;
use uuid::Uuid;

#[derive(Clone)]
pub struct ConversationHistory {
    messages: Vec<ChatMessage>,
    /// Absolute sequence of `messages[0]` in the persisted conversation.
    ///
    /// Loaded conversations retain only a bounded tail in memory. New
    /// conversations use zero.
    storage_offset: usize,
    pub metadata: ConversationMetadata,
}

impl ConversationHistory {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            storage_offset: 0,
            metadata: ConversationMetadata {
                id: Uuid::new_v4().to_string(),
                title: "New Conversation".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                model_id: "claude-sonnet".to_string(), // Default
                message_count: 0,
                session_id: None,
                planning_mode: None,
                runtime_mode: None,
                mode_source: None,
                format_version: None,
            },
        }
    }

    pub fn get_messages(&self) -> Vec<ChatMessage> {
        self.messages.clone()
    }

    /// Return a slice of messages without cloning the entire vector.
    /// The caller receives references; use this for read-only access.
    pub fn messages_ref(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Return a page of messages (zero-indexed `offset`, up to `limit` items).
    /// Clones only the returned page, not the full history.
    pub fn get_messages_page(&self, offset: usize, limit: usize) -> Vec<ChatMessage> {
        let start = offset.min(self.messages.len());
        let end = (start + limit).min(self.messages.len());
        self.messages[start..end].to_vec()
    }

    /// Return metadata without cloning the full conversation.
    pub fn metadata(&self) -> &ConversationMetadata {
        &self.metadata
    }

    pub fn storage_offset(&self) -> usize {
        self.storage_offset
    }

    pub fn absolute_len(&self) -> usize {
        self.storage_offset.saturating_add(self.messages.len())
    }

    pub fn push(&mut self, message: ChatMessage) {
        // Update title from first user message
        if self.messages.is_empty() && message.role == ChatRole::User {
            self.metadata.title = generate_title(&message.content);
        }

        self.messages.push(message);
        self.metadata.message_count = self.absolute_len();
        self.metadata.updated_at = Utc::now();
    }

    pub fn last(&self) -> Option<&ChatMessage> {
        self.messages.last()
    }

    pub fn last_mut(&mut self) -> Option<&mut ChatMessage> {
        self.messages.last_mut()
    }

    pub fn last_assistant(&self) -> Option<&ChatMessage> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == ChatRole::Assistant)
    }

    pub fn last_assistant_mut(&mut self) -> Option<&mut ChatMessage> {
        self.messages
            .iter_mut()
            .rev()
            .find(|m| m.role == ChatRole::Assistant)
    }

    pub fn update_tool_call_status(
        &mut self,
        results: &[(ToolCall, crate::tools::ToolResult)],
    ) -> Option<ChatMessage> {
        self.update_tool_call_status_with_truncation(results, false)
    }

    /// RFC: Large Tool Result Handling - Update tool call status with optional truncation
    pub fn update_tool_call_status_with_truncation(
        &mut self,
        results: &[(ToolCall, crate::tools::ToolResult)],
        truncate: bool,
    ) -> Option<ChatMessage> {
        // Update tool call status in assistant messages when results arrive
        let mut updated_assistant: Option<ChatMessage> = None;
        for (call, result) in results {
            for msg in self.messages.iter_mut().rev() {
                if msg.role == ChatRole::Assistant {
                    if let Some(ref mut tool_calls) = msg.tool_calls {
                        for tc in tool_calls.iter_mut() {
                            if tc.id == call.id {
                                // Determine status based on result
                                tc.status = Some(if result.success {
                                    "complete".to_string()
                                } else if result.skipped {
                                    "skipped".to_string()
                                } else {
                                    "error".to_string()
                                });
                                // RFC: Large Tool Result Handling - truncate in local mode
                                tc.result = Some(if truncate {
                                    result.to_tool_content_truncated_for_tool(&call.function.name)
                                } else {
                                    result.to_tool_content_for_tool(&call.function.name)
                                });
                                break;
                            }
                        }
                    }
                    updated_assistant = Some(msg.clone());
                    break;
                }
            }
        }
        updated_assistant
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.storage_offset = 0;
        self.metadata.message_count = 0;
        self.metadata.updated_at = Utc::now();
    }

    pub fn truncate(&mut self, len: usize) {
        self.messages.truncate(len);
        self.metadata.message_count = self.absolute_len();
        self.metadata.updated_at = Utc::now();
    }

    pub fn get(&self, index: usize) -> Option<&ChatMessage> {
        self.messages.get(index)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ChatMessage> {
        self.messages.iter()
    }

    /// Build API messages from history with model-specific filtering
    pub fn build_api_messages(
        &self,
        system_prompt: String,
        model: &str,
        exclude_last_assistant: bool,
    ) -> Vec<OpenAiMessage> {
        let mut messages = vec![OpenAiMessage {
            role: "system".to_string(),
            content: Some(system_prompt),
            tool_calls: None,
            tool_call_id: None,
        }];

        let is_qwen = model.to_lowercase().contains("qwen");

        let take_len = if exclude_last_assistant && self.messages.len() > 0 {
            if self.messages[self.messages.len() - 1].role == ChatRole::Assistant {
                self.messages.len() - 1
            } else {
                self.messages.len()
            }
        } else {
            self.messages.len()
        };

        messages.extend(
            self.messages
                .iter()
                .take(take_len)
                .filter(|m| m.role != ChatRole::System)
                .filter(|m| {
                    match m.role {
                        ChatRole::User | ChatRole::Tool => true,
                        ChatRole::Assistant => {
                            // For Qwen: filter out empty assistant messages
                            // For others: keep all assistant messages with content or reasoning
                            if is_qwen {
                                let has_content = !m.content.trim().is_empty();
                                let has_reasoning = m
                                    .reasoning
                                    .as_ref()
                                    .map(|r| !r.trim().is_empty())
                                    .unwrap_or(false);
                                has_content || has_reasoning
                            } else {
                                !m.content.trim().is_empty()
                                    || m.reasoning
                                        .as_ref()
                                        .map(|r| !r.trim().is_empty())
                                        .unwrap_or(false)
                            }
                        }
                        ChatRole::System => false,
                    }
                })
                .map(|m| OpenAiMessage {
                    role: match m.role {
                        ChatRole::User => "user".to_string(),
                        ChatRole::Assistant => "assistant".to_string(),
                        ChatRole::System => "system".to_string(),
                        ChatRole::Tool => "tool".to_string(),
                    },
                    content: {
                        let mut c = String::new();
                        if let Some(r) = &m.reasoning {
                            if !r.is_empty() {
                                c.push_str("<think>");
                                c.push_str(r);
                                c.push_str("</think>\n");
                            }
                        }
                        let effective_content = if m.role == ChatRole::User {
                            m.backend_content.as_deref().unwrap_or(&m.content)
                        } else {
                            &m.content
                        };
                        c.push_str(effective_content);
                        Some(c)
                    },
                    tool_calls: None,
                    tool_call_id: m.tool_call_id.clone(),
                }),
        );

        messages
    }

    /// Create from StoredConversation
    pub fn from_stored(stored: StoredConversation) -> Self {
        Self {
            metadata: stored.metadata,
            messages: stored.messages.into_iter().map(|m| m.into()).collect(),
            storage_offset: 0,
        }
    }

    /// Create a history backed by a bounded persisted tail.
    pub fn from_persisted_page(
        metadata: ConversationMetadata,
        messages: Vec<ChatMessage>,
        storage_offset: usize,
    ) -> Self {
        Self {
            metadata,
            messages,
            storage_offset,
        }
    }
}
