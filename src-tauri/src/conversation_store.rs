use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

use crate::protocol::{ChatMessage, ChatRole};

/// Metadata about a conversation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationMetadata {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub model_id: String,
    pub message_count: usize,
}

/// A complete conversation with metadata and messages
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredConversation {
    pub metadata: ConversationMetadata,
    pub messages: Vec<SerializableChatMessage>,
}

/// Serializable version of ChatMessage
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerializableChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl From<&ChatMessage> for SerializableChatMessage {
    fn from(msg: &ChatMessage) -> Self {
        Self {
            role: match msg.role {
                ChatRole::User => "user".to_string(),
                ChatRole::Assistant => "assistant".to_string(),
                ChatRole::System => "system".to_string(),
                ChatRole::Tool => "tool".to_string(),
            },
            content: msg.content.clone(),
            reasoning: msg.reasoning.clone(),
            tool_call_id: msg.tool_call_id.clone(),
        }
    }
}

impl From<SerializableChatMessage> for ChatMessage {
    fn from(msg: SerializableChatMessage) -> Self {
        let mut chat_msg = ChatMessage::new(
            match msg.role.as_str() {
                "user" => ChatRole::User,
                "assistant" => ChatRole::Assistant,
                "system" => ChatRole::System,
                "tool" => ChatRole::Tool,
                _ => ChatRole::User,
            },
            msg.content,
        );
        chat_msg.reasoning = msg.reasoning;
        chat_msg.tool_call_id = msg.tool_call_id;
        chat_msg
    }
}

/// Index of all conversations
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ConversationIndex {
    conversations: Vec<ConversationMetadata>,
    active_id: Option<String>,
}

/// Manages conversation storage and retrieval
pub struct ConversationStore {
    storage_path: PathBuf,
    index: ConversationIndex,
}

impl ConversationStore {
    /// Create a new conversation store
    pub fn new(storage_path: PathBuf) -> Result<Self, String> {
        // Ensure storage directory exists
        if !storage_path.exists() {
            fs::create_dir_all(&storage_path)
                .map_err(|e| format!("Failed to create storage directory: {}", e))?;
        }

        // Load or create index
        let index_path = storage_path.join("index.json");
        let index = if index_path.exists() {
            let content = fs::read_to_string(&index_path)
                .map_err(|e| format!("Failed to read index: {}", e))?;
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse index: {}", e))?
        } else {
            ConversationIndex {
                conversations: Vec::new(),
                active_id: None,
            }
        };

        Ok(Self {
            storage_path,
            index,
        })
    }

    /// List all conversations, sorted by most recent first
    pub fn list_conversations(&self) -> Vec<ConversationMetadata> {
        let mut conversations = self.index.conversations.clone();
        conversations.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        conversations
    }

    /// Load a conversation by ID
    pub fn load_conversation(&self, id: &str) -> Result<StoredConversation, String> {
        let path = self.storage_path.join(format!("{}.json", id));
        if !path.exists() {
            return Err(format!("Conversation {} not found", id));
        }

        let content =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read conversation: {}", e))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse conversation: {}", e))
    }

    /// Save a conversation
    pub fn save_conversation(&mut self, conv: &StoredConversation) -> Result<(), String> {
        // Save conversation file
        let path = self.storage_path.join(format!("{}.json", conv.metadata.id));
        let content = serde_json::to_string_pretty(conv)
            .map_err(|e| format!("Failed to serialize conversation: {}", e))?;
        fs::write(&path, content).map_err(|e| format!("Failed to write conversation: {}", e))?;

        // Update index
        if let Some(existing) = self
            .index
            .conversations
            .iter_mut()
            .find(|m| m.id == conv.metadata.id)
        {
            *existing = conv.metadata.clone();
        } else {
            self.index.conversations.push(conv.metadata.clone());
        }

        self.save_index()?;
        Ok(())
    }

    /// Create a new conversation
    pub fn create_new_conversation(&mut self, model_id: String) -> ConversationMetadata {
        let now = Utc::now();
        let metadata = ConversationMetadata {
            id: Uuid::new_v4().to_string(),
            title: "New Conversation".to_string(),
            created_at: now,
            updated_at: now,
            model_id,
            message_count: 0,
        };

        self.index.conversations.push(metadata.clone());
        self.index.active_id = Some(metadata.id.clone());

        // Save index immediately
        let _ = self.save_index();

        metadata
    }

    /// Delete a conversation
    pub fn delete_conversation(&mut self, id: &str) -> Result<(), String> {
        // Delete file
        let path = self.storage_path.join(format!("{}.json", id));
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to delete conversation file: {}", e))?;
        }

        // Remove from index
        self.index.conversations.retain(|m| m.id != id);

        // Clear active if it was the deleted one
        if self.index.active_id.as_deref() == Some(id) {
            self.index.active_id = None;
        }

        self.save_index()?;
        Ok(())
    }

    /// Set the active conversation
    pub fn set_active(&mut self, id: &str) {
        self.index.active_id = Some(id.to_string());
        let _ = self.save_index();
    }

    /// Get the active conversation ID
    pub fn get_active(&self) -> Option<String> {
        self.index.active_id.clone()
    }

    /// Save the index file
    fn save_index(&self) -> Result<(), String> {
        let path = self.storage_path.join("index.json");
        let content = serde_json::to_string_pretty(&self.index)
            .map_err(|e| format!("Failed to serialize index: {}", e))?;
        fs::write(&path, content).map_err(|e| format!("Failed to write index: {}", e))?;
        Ok(())
    }
}

/// Generate a title from the first user message
pub fn generate_title(first_message: &str) -> String {
    let trimmed = first_message.trim();

    // Handle slash commands
    // Handle slash commands
    if trimmed.starts_with('/') {
        let without_slash = &trimmed[1..];
        if let Some(first_char) = without_slash.chars().next() {
            return format!("{}{}", first_char.to_uppercase(), &without_slash[1..]);
        }
        return String::new();
    }

    // Take first 50 characters, truncate at word boundary
    if trimmed.len() <= 50 {
        return trimmed.to_string();
    }

    let truncated = &trimmed[..50];
    if let Some(last_space) = truncated.rfind(' ') {
        format!("{}...", &truncated[..last_space])
    } else {
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_title_short() {
        assert_eq!(generate_title("Hello world"), "Hello world");
    }

    #[test]
    fn test_generate_title_long() {
        let long =
            "This is a very long message that exceeds fifty characters and should be truncated";
        let title = generate_title(long);
        assert!(title.len() <= 53); // 50 + "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn test_generate_title_slash_command() {
        assert_eq!(generate_title("/fix the bug"), "Fix the bug");
        assert_eq!(generate_title("/help"), "Help");
    }
}
