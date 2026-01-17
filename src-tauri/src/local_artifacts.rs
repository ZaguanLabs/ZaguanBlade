use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::local_index::{ConversationIndex, LocalIndex, MomentIndex, CodeReferenceIndex};
use crate::project_settings::get_zblade_dir;

/// Code reference within a message (stores reference, not actual code)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeReference {
    pub file: String,
    pub lines: (i32, i32),
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<CodeDiff>,
}

/// Diff information for code changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeDiff {
    #[serde(rename = "type")]
    pub diff_type: String,
    pub content: String,
}

/// A message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub code_references: Vec<CodeReference>,
}

/// A moment (decision, pattern, solution) extracted from conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Moment {
    pub id: String,
    #[serde(rename = "type")]
    pub moment_type: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub code_references: Vec<CodeReference>,
    #[serde(default = "default_relevance")]
    pub relevance_score: f64,
}

fn default_relevance() -> f64 {
    0.5
}

/// Conversation metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMetadata {
    #[serde(default)]
    pub total_messages: i32,
    #[serde(default)]
    pub total_tokens: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models_used: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl Default for ConversationMetadata {
    fn default() -> Self {
        Self {
            total_messages: 0,
            total_tokens: 0,
            models_used: Vec::new(),
            tags: Vec::new(),
        }
    }
}

/// Full conversation artifact stored in .zblade/artifacts/conversations/
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationArtifact {
    pub version: String,
    pub conversation_id: String,
    pub project_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub title: String,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moments: Vec<Moment>,
    #[serde(default)]
    pub metadata: ConversationMetadata,
}

impl ConversationArtifact {
    pub fn new(conversation_id: String, project_id: String, title: String) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            version: "1.0".to_string(),
            conversation_id,
            project_id,
            created_at: now.clone(),
            updated_at: now,
            title,
            messages: Vec::new(),
            moments: Vec::new(),
            metadata: ConversationMetadata::default(),
        }
    }
    
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
        self.metadata.total_messages = self.messages.len() as i32;
        self.updated_at = chrono::Utc::now().to_rfc3339();
    }
    
    pub fn add_moment(&mut self, moment: Moment) {
        self.moments.push(moment);
        self.updated_at = chrono::Utc::now().to_rfc3339();
    }
}

/// Local artifact store for a project
pub struct LocalArtifactStore {
    project_path: PathBuf,
}

impl LocalArtifactStore {
    pub fn new(project_path: &Path) -> Self {
        Self {
            project_path: project_path.to_path_buf(),
        }
    }
    
    fn conversations_dir(&self) -> PathBuf {
        get_zblade_dir(&self.project_path)
            .join("artifacts")
            .join("conversations")
    }
    
    fn moments_dir(&self) -> PathBuf {
        get_zblade_dir(&self.project_path)
            .join("artifacts")
            .join("moments")
    }
    
    fn conversation_path(&self, id: &str) -> PathBuf {
        self.conversations_dir().join(format!("{}.json", id))
    }
    
    fn moment_path(&self, id: &str) -> PathBuf {
        self.moments_dir().join(format!("{}.json", id))
    }
    
    /// Save a conversation artifact to disk and update the index
    pub fn save_conversation(&self, artifact: &ConversationArtifact) -> Result<(), String> {
        // Ensure directory exists
        let dir = self.conversations_dir();
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create conversations directory: {}", e))?;
        
        // Write JSON file
        let path = self.conversation_path(&artifact.conversation_id);
        let json = serde_json::to_string_pretty(artifact)
            .map_err(|e| format!("Failed to serialize conversation: {}", e))?;
        fs::write(&path, json)
            .map_err(|e| format!("Failed to write conversation: {}", e))?;
        
        // Update SQLite index
        let index = LocalIndex::open(&self.project_path)
            .map_err(|e| format!("Failed to open index: {}", e))?;
        
        let conv_index = ConversationIndex {
            id: artifact.conversation_id.clone(),
            project_id: artifact.project_id.clone(),
            title: artifact.title.clone(),
            created_at: artifact.created_at.clone(),
            updated_at: artifact.updated_at.clone(),
            message_count: artifact.metadata.total_messages,
            tags: artifact.metadata.tags.clone(),
            artifact_path: path.to_string_lossy().to_string(),
        };
        
        index.upsert_conversation(&conv_index)
            .map_err(|e| format!("Failed to update index: {}", e))?;
        
        // Index code references from messages
        for msg in &artifact.messages {
            for code_ref in &msg.code_references {
                let ref_index = CodeReferenceIndex {
                    id: 0,
                    conversation_id: artifact.conversation_id.clone(),
                    message_id: msg.id.clone(),
                    file_path: code_ref.file.clone(),
                    start_line: code_ref.lines.0,
                    end_line: code_ref.lines.1,
                    context: code_ref.context.clone(),
                    created_at: msg.timestamp.clone(),
                };
                let _ = index.insert_code_reference(&ref_index);
            }
        }
        
        // Index moments
        for moment in &artifact.moments {
            let moment_index = MomentIndex {
                id: moment.id.clone(),
                conversation_id: artifact.conversation_id.clone(),
                moment_type: moment.moment_type.clone(),
                content: moment.content.clone(),
                context: moment.context.clone(),
                tags: moment.tags.clone(),
                created_at: moment.timestamp.clone(),
                relevance_score: moment.relevance_score,
                artifact_path: self.moment_path(&moment.id).to_string_lossy().to_string(),
            };
            let _ = index.upsert_moment(&moment_index);
        }
        
        Ok(())
    }
    
    /// Load a conversation artifact from disk
    pub fn load_conversation(&self, id: &str) -> Result<ConversationArtifact, String> {
        let path = self.conversation_path(id);
        
        if !path.exists() {
            return Err(format!("Conversation not found: {}", id));
        }
        
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read conversation: {}", e))?;
        
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse conversation: {}", e))
    }
    
    /// Delete a conversation artifact and remove from index
    pub fn delete_conversation(&self, id: &str) -> Result<(), String> {
        let path = self.conversation_path(id);
        
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to delete conversation file: {}", e))?;
        }
        
        // Remove from index
        let index = LocalIndex::open(&self.project_path)
            .map_err(|e| format!("Failed to open index: {}", e))?;
        
        index.delete_conversation(id)
            .map_err(|e| format!("Failed to remove from index: {}", e))?;
        
        Ok(())
    }
    
    /// List all conversations from the index
    pub fn list_conversations(&self) -> Result<Vec<ConversationIndex>, String> {
        let index = LocalIndex::open(&self.project_path)
            .map_err(|e| format!("Failed to open index: {}", e))?;
        
        index.list_conversations()
            .map_err(|e| format!("Failed to list conversations: {}", e))
    }
    
    /// Search moments using full-text search
    pub fn search_moments(&self, query: &str, limit: i32) -> Result<Vec<MomentIndex>, String> {
        let index = LocalIndex::open(&self.project_path)
            .map_err(|e| format!("Failed to open index: {}", e))?;
        
        index.search_moments(query, limit)
            .map_err(|e| format!("Failed to search moments: {}", e))
    }
    
    /// Get code references for a file
    pub fn get_file_references(&self, file_path: &str) -> Result<Vec<CodeReferenceIndex>, String> {
        let index = LocalIndex::open(&self.project_path)
            .map_err(|e| format!("Failed to open index: {}", e))?;
        
        index.get_references_for_file(file_path)
            .map_err(|e| format!("Failed to get file references: {}", e))
    }
}

/// Resolve a code reference to actual file content
pub fn resolve_code_reference(project_path: &Path, code_ref: &CodeReference) -> Result<String, String> {
    let file_path = if Path::new(&code_ref.file).is_absolute() {
        PathBuf::from(&code_ref.file)
    } else {
        project_path.join(&code_ref.file)
    };
    
    if !file_path.exists() {
        return Err(format!("File not found: {}", code_ref.file));
    }
    
    let content = fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    
    let lines: Vec<&str> = content.lines().collect();
    let start = (code_ref.lines.0 - 1).max(0) as usize;
    let end = (code_ref.lines.1).min(lines.len() as i32) as usize;
    
    if start >= lines.len() {
        return Err(format!("Start line {} out of range (file has {} lines)", code_ref.lines.0, lines.len()));
    }
    
    Ok(lines[start..end].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_conversation_artifact_creation() {
        let artifact = ConversationArtifact::new(
            "conv_123".to_string(),
            "proj_456".to_string(),
            "Test Conversation".to_string(),
        );
        
        assert_eq!(artifact.version, "1.0");
        assert_eq!(artifact.conversation_id, "conv_123");
        assert!(artifact.messages.is_empty());
    }

    #[test]
    fn test_add_message() {
        let mut artifact = ConversationArtifact::new(
            "conv_123".to_string(),
            "proj_456".to_string(),
            "Test".to_string(),
        );
        
        let msg = Message {
            id: "msg_001".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: "2026-01-17T14:00:00Z".to_string(),
            code_references: vec![],
        };
        
        artifact.add_message(msg);
        
        assert_eq!(artifact.messages.len(), 1);
        assert_eq!(artifact.metadata.total_messages, 1);
    }

    #[test]
    fn test_save_and_load_conversation() {
        let temp = tempdir().unwrap();
        let project_path = temp.path();
        
        // Initialize .zblade directory
        crate::project_settings::init_zblade_dir(project_path).unwrap();
        
        let store = LocalArtifactStore::new(project_path);
        
        let mut artifact = ConversationArtifact::new(
            "conv_test".to_string(),
            "proj_test".to_string(),
            "Test Conversation".to_string(),
        );
        
        artifact.add_message(Message {
            id: "msg_001".to_string(),
            role: "user".to_string(),
            content: "How do I implement auth?".to_string(),
            timestamp: "2026-01-17T14:00:00Z".to_string(),
            code_references: vec![
                CodeReference {
                    file: "src/auth.ts".to_string(),
                    lines: (10, 25),
                    git_hash: None,
                    context: Some("Authentication module".to_string()),
                    diff: None,
                }
            ],
        });
        
        // Save
        store.save_conversation(&artifact).unwrap();
        
        // Load
        let loaded = store.load_conversation("conv_test").unwrap();
        assert_eq!(loaded.title, "Test Conversation");
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].code_references.len(), 1);
        
        // List
        let all = store.list_conversations().unwrap();
        assert_eq!(all.len(), 1);
        
        // Delete
        store.delete_conversation("conv_test").unwrap();
        let result = store.load_conversation("conv_test");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_code_reference() {
        let temp = tempdir().unwrap();
        let project_path = temp.path();
        
        // Create a test file
        let test_file = project_path.join("test.rs");
        fs::write(&test_file, "line 1\nline 2\nline 3\nline 4\nline 5").unwrap();
        
        let code_ref = CodeReference {
            file: "test.rs".to_string(),
            lines: (2, 4),
            git_hash: None,
            context: None,
            diff: None,
        };
        
        let content = resolve_code_reference(project_path, &code_ref).unwrap();
        assert_eq!(content, "line 2\nline 3\nline 4");
    }
}
