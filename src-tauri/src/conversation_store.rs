use chrono::{DateTime, Utc};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
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
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub planning_mode: Option<bool>,
    #[serde(default)]
    pub runtime_mode: Option<String>,
    #[serde(default)]
    pub mode_source: Option<String>,
    #[serde(default)]
    pub format_version: Option<u32>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<crate::protocol::ChatImage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mentions: Option<Vec<crate::protocol::ChatMention>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<crate::protocol::ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<crate::protocol::ProgressInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_before_tools: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_after_tools: Option<String>,
}

impl From<&ChatMessage> for SerializableChatMessage {
    fn from(msg: &ChatMessage) -> Self {
        Self {
            id: msg.id.clone(),
            role: match msg.role {
                ChatRole::User => "user".to_string(),
                ChatRole::Assistant => "assistant".to_string(),
                ChatRole::System => "system".to_string(),
                ChatRole::Tool => "tool".to_string(),
            },
            content: msg.content.clone(),
            backend_content: msg.backend_content.clone(),
            images: msg.images.clone(),
            mentions: msg.mentions.clone(),
            reasoning: msg.reasoning.clone(),
            tool_call_id: msg.tool_call_id.clone(),
            tool_calls: msg.tool_calls.clone(),
            progress: msg.progress.clone(),
            content_before_tools: msg.content_before_tools.clone(),
            content_after_tools: msg.content_after_tools.clone(),
        }
    }
}

impl From<ChatMessage> for SerializableChatMessage {
    fn from(msg: ChatMessage) -> Self {
        Self {
            id: msg.id,
            role: match msg.role {
                ChatRole::User => "user".to_string(),
                ChatRole::Assistant => "assistant".to_string(),
                ChatRole::System => "system".to_string(),
                ChatRole::Tool => "tool".to_string(),
            },
            content: msg.content,
            backend_content: msg.backend_content,
            images: msg.images,
            mentions: msg.mentions,
            reasoning: msg.reasoning,
            tool_call_id: msg.tool_call_id,
            tool_calls: msg.tool_calls,
            progress: msg.progress,
            content_before_tools: msg.content_before_tools,
            content_after_tools: msg.content_after_tools,
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
        if msg.id.is_some() {
            chat_msg.id = msg.id;
        }
        chat_msg.backend_content = msg.backend_content;
        chat_msg.images = msg.images;
        chat_msg.mentions = msg.mentions;
        chat_msg.reasoning = msg.reasoning;
        chat_msg.tool_call_id = msg.tool_call_id;
        chat_msg.tool_calls = msg.tool_calls;
        chat_msg.progress = msg.progress;
        chat_msg.content_before_tools = msg.content_before_tools;
        chat_msg.content_after_tools = msg.content_after_tools;
        chat_msg
    }
}

const STORE_VERSION: u32 = 2;
const MAX_PAGE_MESSAGES: usize = 200;
const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;

/// Index of all conversations
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ConversationIndex {
    conversations: Vec<ConversationMetadata>,
    active_id: Option<String>,
}

/// A bounded page returned to the frontend.
#[derive(Clone, Debug, Serialize)]
pub struct ConversationPage {
    pub messages: Vec<ChatMessage>,
    pub offset: usize,
    pub total: usize,
}

/// Owned changed tail captured while the live history lock is held briefly.
///
/// Disk I/O happens only after this snapshot has been constructed, so chat
/// event processing is never blocked on filesystem latency.
pub struct ConversationDelta {
    metadata: ConversationMetadata,
    start_sequence: usize,
    message_count: usize,
    messages: Vec<SerializableChatMessage>,
}

impl ConversationDelta {
    pub fn from_history(
        history: &crate::conversation::ConversationHistory,
        start_sequence: usize,
    ) -> Self {
        let message_count = history.absolute_len();
        let start_sequence = start_sequence
            .max(history.storage_offset())
            .min(message_count);
        let local_start = start_sequence - history.storage_offset();
        Self {
            metadata: history.metadata().clone(),
            start_sequence,
            message_count,
            messages: history.messages_ref()[local_start..]
                .iter()
                .map(SerializableChatMessage::from)
                .collect(),
        }
    }

    pub fn conversation_id(&self) -> &str {
        &self.metadata.id
    }
}

/// Manages the versioned, incremental conversation store.
///
/// Each conversation owns a metadata file and one compact JSON file per
/// message. Appends and streaming updates therefore rewrite only the changed
/// tail, while a page read touches only the requested message files.
pub struct ConversationStore {
    storage_path: PathBuf,
    index: ConversationIndex,
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.contains('/')
        || id.contains('\\')
        || id.contains('\0')
    {
        return Err("Invalid conversation id".to_string());
    }
    Ok(())
}

fn message_file_name(sequence: usize) -> String {
    format!("{sequence:010}.json")
}

fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
    }
}

/// Replace a file without deleting the last known-good copy first.
///
/// POSIX rename replaces atomically. Windows cannot rename over an existing
/// destination, so it uses a recoverable backup swap and rolls back if the
/// second rename fails.
pub(crate) fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("Invalid destination path {}", path.display()))?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| format!("Failed to create {}: {e}", temporary.display()))?;
    file.write_all(content)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("Failed to write {}: {e}", temporary.display()))?;
    drop(file);

    #[cfg(not(windows))]
    let replace_result = fs::rename(&temporary, path);

    #[cfg(windows)]
    let replace_result = if path.exists() {
        let backup = parent.join(format!(".{file_name}.{}.bak", Uuid::new_v4()));
        fs::rename(path, &backup).and_then(|()| match fs::rename(&temporary, path) {
            Ok(()) => {
                let _ = fs::remove_file(backup);
                Ok(())
            }
            Err(error) => {
                let _ = fs::rename(&backup, path);
                Err(error)
            }
        })
    } else {
        fs::rename(&temporary, path)
    };

    if let Err(error) = replace_result {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Failed to replace {}: {error}", path.display()));
    }
    sync_parent(path);
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Failed to parse {}: {e}", path.display()))
}

struct MessageSequenceSeed<'a> {
    directory: &'a Path,
}

impl<'de> DeserializeSeed<'de> for MessageSequenceSeed<'_> {
    type Value = usize;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct SequenceVisitor<'a> {
            directory: &'a Path,
        }

        impl<'de> Visitor<'de> for SequenceVisitor<'_> {
            type Value = usize;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a sequence of conversation messages")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut count = 0usize;
                while let Some(message) = sequence.next_element::<SerializableChatMessage>()? {
                    let bytes = serde_json::to_vec(&message).map_err(serde::de::Error::custom)?;
                    let path = self.directory.join(message_file_name(count));
                    fs::write(&path, bytes).map_err(serde::de::Error::custom)?;
                    count = count.saturating_add(1);
                }
                Ok(count)
            }
        }

        deserializer.deserialize_seq(SequenceVisitor {
            directory: self.directory,
        })
    }
}

struct LegacyConversationSeed<'a> {
    messages_directory: &'a Path,
}

impl<'de> DeserializeSeed<'de> for LegacyConversationSeed<'_> {
    type Value = (ConversationMetadata, usize);

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ConversationVisitor<'a> {
            messages_directory: &'a Path,
        }

        impl<'de> Visitor<'de> for ConversationVisitor<'_> {
            type Value = (ConversationMetadata, usize);

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a legacy StoredConversation object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut metadata = None;
                let mut message_count = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "metadata" => {
                            metadata = Some(map.next_value::<ConversationMetadata>()?);
                        }
                        "messages" => {
                            message_count = Some(map.next_value_seed(MessageSequenceSeed {
                                directory: self.messages_directory,
                            })?);
                        }
                        "conversation_id" => {
                            return Err(serde::de::Error::custom(
                                "searchable conversation artifact schema",
                            ));
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                let metadata =
                    metadata.ok_or_else(|| serde::de::Error::missing_field("metadata"))?;
                let count =
                    message_count.ok_or_else(|| serde::de::Error::missing_field("messages"))?;
                Ok((metadata, count))
            }
        }

        deserializer.deserialize_map(ConversationVisitor {
            messages_directory: self.messages_directory,
        })
    }
}

impl ConversationStore {
    /// Open the canonical store and migrate legacy UI-conversation JSON files.
    ///
    /// Searchable `ConversationArtifact` files are deliberately left in the
    /// legacy artifacts directory. Successfully migrated UI files are retained
    /// there as `.migrated-v2.bak` backups, so migration is restartable and a
    /// source is never destroyed before the new copy has been opened.
    pub fn new(storage_path: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&storage_path)
            .map_err(|e| format!("Failed to create storage directory: {e}"))?;
        let index_path = storage_path.join("index.json");
        let index = if index_path.exists() {
            match read_json(&index_path) {
                Ok(index) => index,
                Err(error) => {
                    let backup =
                        storage_path.join(format!("index.corrupt.{}.json", Uuid::new_v4()));
                    fs::rename(&index_path, &backup).map_err(|rename_error| {
                        format!(
                            "{error}; failed to preserve corrupt index as {}: {rename_error}",
                            backup.display()
                        )
                    })?;
                    Self::rebuild_index(&storage_path)?
                }
            }
        } else {
            Self::rebuild_index(&storage_path)?
        };

        let mut store = Self {
            storage_path,
            index,
        };
        store.migrate_legacy_files()?;
        store.save_index()?;
        Ok(store)
    }

    fn rebuild_index(storage_path: &Path) -> Result<ConversationIndex, String> {
        let mut conversations = Vec::new();
        for entry in fs::read_dir(storage_path)
            .map_err(|e| format!("Failed to scan {}: {e}", storage_path.display()))?
        {
            let entry = entry.map_err(|e| format!("Failed to read store entry: {e}"))?;
            if !entry
                .file_type()
                .map_err(|e| format!("Failed to inspect {}: {e}", entry.path().display()))?
                .is_dir()
            {
                continue;
            }
            let metadata_path = entry.path().join("metadata.json");
            if let Ok(metadata) = read_json::<ConversationMetadata>(&metadata_path) {
                conversations.push(metadata);
            }
        }
        Ok(ConversationIndex {
            conversations,
            active_id: None,
        })
    }

    fn conversation_dir(&self, id: &str) -> PathBuf {
        self.storage_path.join(id)
    }

    fn messages_dir(&self, id: &str) -> PathBuf {
        self.conversation_dir(id).join("messages")
    }

    fn metadata_path(&self, id: &str) -> PathBuf {
        self.conversation_dir(id).join("metadata.json")
    }

    fn message_path(&self, id: &str, sequence: usize) -> PathBuf {
        self.messages_dir(id).join(message_file_name(sequence))
    }

    fn legacy_path(&self) -> Option<PathBuf> {
        self.storage_path
            .parent()
            .map(|root| root.join("artifacts").join("conversations"))
            .filter(|path| path != &self.storage_path)
    }

    fn canonical_tail_is_valid(&self, id: &str) -> bool {
        self.load_metadata(id).is_ok_and(|metadata| {
            metadata.message_count == 0
                || self
                    .read_message(id, metadata.message_count.saturating_sub(1))
                    .is_ok()
        })
    }

    fn promote_staging_directory(
        &mut self,
        id: &str,
        staging_dir: &Path,
        metadata: &ConversationMetadata,
    ) -> Result<(), String> {
        let final_dir = self.conversation_dir(id);
        if final_dir.exists() {
            let backup = self
                .storage_path
                .join(format!(".{id}.{}.incomplete.bak", Uuid::new_v4()));
            fs::rename(&final_dir, &backup)
                .map_err(|e| format!("Failed to preserve {}: {e}", final_dir.display()))?;
            if let Err(error) = fs::rename(staging_dir, &final_dir) {
                let _ = fs::rename(&backup, &final_dir);
                return Err(format!("Failed to install migrated conversation: {error}"));
            }
            let _ = fs::remove_dir_all(backup);
        } else {
            fs::rename(staging_dir, &final_dir)
                .map_err(|e| format!("Failed to install migrated conversation: {e}"))?;
        }
        sync_parent(&final_dir);
        self.upsert_metadata(metadata);
        self.save_index()
    }

    fn stream_legacy_conversation(
        &mut self,
        source: &Path,
        expected_id: &str,
    ) -> Result<(), String> {
        validate_id(expected_id)?;
        let staging_dir = self
            .storage_path
            .join(format!(".{expected_id}.{}.migrating", Uuid::new_v4()));
        let staging_messages = staging_dir.join("messages");
        fs::create_dir_all(&staging_messages)
            .map_err(|e| format!("Failed to create {}: {e}", staging_messages.display()))?;

        let result = (|| {
            let source_file = fs::File::open(source)
                .map_err(|e| format!("Failed to open {}: {e}", source.display()))?;
            let mut deserializer =
                serde_json::Deserializer::from_reader(BufReader::new(source_file));
            let (mut metadata, message_count) = LegacyConversationSeed {
                messages_directory: &staging_messages,
            }
            .deserialize(&mut deserializer)
            .map_err(|e| format!("Failed to stream {}: {e}", source.display()))?;
            deserializer
                .end()
                .map_err(|e| format!("Trailing data in {}: {e}", source.display()))?;

            if metadata.id != expected_id {
                return Err(format!(
                    "Conversation id {} does not match legacy filename {expected_id}",
                    metadata.id
                ));
            }
            metadata.message_count = message_count;
            metadata.format_version = Some(STORE_VERSION);
            let metadata_bytes = serde_json::to_vec(&metadata)
                .map_err(|e| format!("Failed to serialize metadata: {e}"))?;
            atomic_write(&staging_dir.join("metadata.json"), &metadata_bytes)?;
            self.promote_staging_directory(expected_id, &staging_dir, &metadata)
        })();

        if result.is_err() {
            let _ = fs::remove_dir_all(&staging_dir);
        }
        result
    }

    fn migrate_legacy_files(&mut self) -> Result<(), String> {
        let Some(legacy_path) = self.legacy_path() else {
            return Ok(());
        };
        if !legacy_path.is_dir() {
            return Ok(());
        }

        for entry in fs::read_dir(&legacy_path)
            .map_err(|e| format!("Failed to scan legacy conversations: {e}"))?
        {
            let entry = entry.map_err(|e| format!("Failed to read legacy entry: {e}"))?;
            let source = entry.path();
            if !entry
                .file_type()
                .map_err(|e| format!("Failed to inspect {}: {e}", source.display()))?
                .is_file()
            {
                continue;
            }
            let file_name = source
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let is_source_json = file_name.ends_with(".json");
            let is_migration_backup =
                file_name.contains(".json.migrated-v2") && file_name.ends_with(".bak");
            if !is_source_json && !is_migration_backup {
                continue;
            }

            let expected_id = file_name.split(".json").next().unwrap_or_default();
            if validate_id(expected_id).is_err() {
                continue;
            }

            if is_migration_backup && self.canonical_tail_is_valid(expected_id) {
                continue;
            }

            // Artifact files have a different metadata schema, so streaming
            // them as StoredConversation fails and leaves them untouched.
            if let Err(error) = self.stream_legacy_conversation(&source, expected_id) {
                if is_migration_backup {
                    eprintln!(
                        "[ConversationStore] Failed to recover {}: {error}",
                        source.display()
                    );
                }
                continue;
            }

            if !is_migration_backup {
                let preferred_backup = source.with_extension("json.migrated-v2.bak");
                let backup = if preferred_backup.exists() {
                    source.with_extension(format!("json.migrated-v2.{}.bak", Uuid::new_v4()))
                } else {
                    preferred_backup
                };
                fs::rename(&source, &backup).map_err(|e| {
                    format!(
                        "Migrated {}, but failed to preserve source as {}: {e}",
                        source.display(),
                        backup.display()
                    )
                })?;
            }
        }
        Ok(())
    }

    fn load_metadata(&self, id: &str) -> Result<ConversationMetadata, String> {
        validate_id(id)?;
        read_json(&self.metadata_path(id))
    }

    pub fn load_conversation_tail(
        &self,
        id: &str,
        limit: usize,
    ) -> Result<(ConversationMetadata, ConversationPage), String> {
        let metadata = self.load_metadata(id)?;
        let page = self.load_message_page_before(id, None, limit)?;
        Ok((metadata, page))
    }

    fn write_message(
        &self,
        id: &str,
        sequence: usize,
        message: &SerializableChatMessage,
    ) -> Result<(), String> {
        let bytes = serde_json::to_vec(message)
            .map_err(|e| format!("Failed to serialize message {sequence}: {e}"))?;
        atomic_write(&self.message_path(id, sequence), &bytes)
    }

    fn read_message(&self, id: &str, sequence: usize) -> Result<SerializableChatMessage, String> {
        let path = self.message_path(id, sequence);
        let mut message: SerializableChatMessage = read_json(&path)?;
        if message.id.is_none() {
            message.id = Some(format!("legacy:{id}:{sequence}"));
        }
        Ok(message)
    }

    fn upsert_metadata(&mut self, metadata: &ConversationMetadata) {
        if let Some(existing) = self
            .index
            .conversations
            .iter_mut()
            .find(|candidate| candidate.id == metadata.id)
        {
            *existing = metadata.clone();
        } else {
            self.index.conversations.push(metadata.clone());
        }
    }

    /// Return the earliest message that may need rewriting for the next save.
    ///
    /// The previously persisted tail is included because streaming mutates it
    /// in place; appended messages begin immediately after it.
    pub fn changed_tail_start(&self, id: &str, new_count: usize) -> usize {
        let old_count = self
            .load_metadata(id)
            .map(|metadata| metadata.message_count)
            .unwrap_or(0);
        if new_count == 0 {
            0
        } else if new_count > old_count {
            old_count.saturating_sub(1)
        } else {
            new_count - 1
        }
    }

    /// Commit an owned changed-tail snapshot.
    pub fn save_delta(&mut self, delta: ConversationDelta) -> Result<(), String> {
        let id = delta.metadata.id.clone();
        validate_id(&id)?;
        if delta.start_sequence.saturating_add(delta.messages.len()) != delta.message_count {
            return Err("Conversation delta is not a contiguous tail".to_string());
        }
        fs::create_dir_all(self.messages_dir(&id))
            .map_err(|e| format!("Failed to create message directory: {e}"))?;

        let old_count = self
            .load_metadata(&id)
            .map(|metadata| metadata.message_count)
            .unwrap_or(0);
        for (index, message) in delta.messages.iter().enumerate() {
            self.write_message(&id, delta.start_sequence + index, message)?;
        }

        let mut metadata = delta.metadata;
        metadata.message_count = delta.message_count;
        metadata.format_version = Some(STORE_VERSION);
        let metadata_bytes = serde_json::to_vec(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {e}"))?;
        atomic_write(&self.metadata_path(&id), &metadata_bytes)?;

        for sequence in delta.message_count..old_count {
            let path = self.message_path(&id, sequence);
            if path.exists() {
                fs::remove_file(&path)
                    .map_err(|e| format!("Failed to remove {}: {e}", path.display()))?;
            }
        }
        self.upsert_metadata(&metadata);
        self.save_index()
    }

    /// List all conversations, sorted by most recent first
    pub fn list_conversations(&self) -> Vec<ConversationMetadata> {
        let mut conversations = self.index.conversations.clone();
        conversations.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        conversations
    }

    /// Load a complete conversation for the backend model context.
    ///
    /// UI callers must use `load_message_page` so the Tauri boundary remains
    /// bounded.
    pub fn load_conversation(&self, id: &str) -> Result<StoredConversation, String> {
        let metadata = self.load_metadata(id)?;
        let mut messages = Vec::with_capacity(metadata.message_count);
        for sequence in 0..metadata.message_count {
            let path = self.message_path(id, sequence);
            if !path.is_file() {
                return Err(format!(
                    "Conversation {id} is incomplete: missing message {sequence}"
                ));
            }
            messages.push(self.read_message(id, sequence)?);
        }
        Ok(StoredConversation { metadata, messages })
    }

    /// Read only the requested message files. The message count is hard
    /// bounded; the byte budget is best-effort so one oversized historical
    /// message remains readable on its own.
    pub fn load_message_page(
        &self,
        id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ConversationPage, String> {
        let metadata = self.load_metadata(id)?;
        let total = metadata.message_count;
        let start = offset.min(total);
        let end = start
            .saturating_add(limit.min(MAX_PAGE_MESSAGES))
            .min(total);
        let mut messages = Vec::with_capacity(end - start);
        let mut page_bytes = 0usize;
        for sequence in start..end {
            let path = self.message_path(id, sequence);
            let bytes =
                fs::read(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
            if !messages.is_empty() && page_bytes.saturating_add(bytes.len()) > MAX_PAGE_BYTES {
                break;
            }
            let mut message: SerializableChatMessage = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;
            if message.id.is_none() {
                message.id = Some(format!("legacy:{id}:{sequence}"));
            }
            page_bytes = page_bytes.saturating_add(bytes.len());
            messages.push(message.into());
        }
        Ok(ConversationPage {
            messages,
            offset: start,
            total,
        })
    }

    /// Load the bounded page immediately preceding `before`.
    ///
    /// `None` means the tail of the conversation. Reading backwards ensures
    /// the initial page contains the newest messages even when the byte budget
    /// is reached before the message-count budget.
    pub fn load_message_page_before(
        &self,
        id: &str,
        before: Option<usize>,
        limit: usize,
    ) -> Result<ConversationPage, String> {
        let metadata = self.load_metadata(id)?;
        let total = metadata.message_count;
        let end = before.unwrap_or(total).min(total);
        let minimum = end.saturating_sub(limit.min(MAX_PAGE_MESSAGES));
        let mut reversed = Vec::with_capacity(end - minimum);
        let mut page_bytes = 0usize;

        for sequence in (minimum..end).rev() {
            let path = self.message_path(id, sequence);
            let bytes =
                fs::read(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
            if !reversed.is_empty() && page_bytes.saturating_add(bytes.len()) > MAX_PAGE_BYTES {
                break;
            }
            let mut message: SerializableChatMessage = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;
            if message.id.is_none() {
                message.id = Some(format!("legacy:{id}:{sequence}"));
            }
            page_bytes = page_bytes.saturating_add(bytes.len());
            reversed.push(message.into());
        }
        reversed.reverse();
        let offset = end.saturating_sub(reversed.len());
        Ok(ConversationPage {
            messages: reversed,
            offset,
            total,
        })
    }

    /// Incrementally save a full snapshot.
    ///
    /// New messages are appended, the mutable tail is replaced, and truncation
    /// removes only stale tail files. Metadata is committed after message data,
    /// so a crash cannot advertise messages that were not durably written.
    pub fn save_conversation(&mut self, conv: &StoredConversation) -> Result<(), String> {
        let id = &conv.metadata.id;
        validate_id(id)?;
        fs::create_dir_all(self.messages_dir(id))
            .map_err(|e| format!("Failed to create message directory: {e}"))?;

        let old_count = self
            .load_metadata(id)
            .map(|metadata| metadata.message_count)
            .unwrap_or(0);
        let new_count = conv.messages.len();
        let start = if new_count == 0 {
            0
        } else if new_count > old_count {
            old_count.saturating_sub(1)
        } else {
            new_count - 1
        };
        for sequence in start..new_count {
            self.write_message(id, sequence, &conv.messages[sequence])?;
        }

        let mut metadata = conv.metadata.clone();
        metadata.message_count = new_count;
        metadata.format_version = Some(STORE_VERSION);
        let metadata_bytes = serde_json::to_vec(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {e}"))?;
        atomic_write(&self.metadata_path(id), &metadata_bytes)?;

        if old_count > new_count {
            for sequence in new_count..old_count {
                let path = self.message_path(id, sequence);
                if path.exists() {
                    fs::remove_file(&path)
                        .map_err(|e| format!("Failed to remove {}: {e}", path.display()))?;
                }
            }
        }
        self.upsert_metadata(&metadata);
        self.save_index()?;
        Ok(())
    }

    /// Persist a new empty conversation and return its metadata.
    pub fn create_new_conversation(
        &mut self,
        model_id: String,
    ) -> Result<ConversationMetadata, String> {
        let now = Utc::now();
        let metadata = ConversationMetadata {
            id: Uuid::new_v4().to_string(),
            title: "New Conversation".to_string(),
            created_at: now,
            updated_at: now,
            model_id,
            message_count: 0,
            session_id: None,
            planning_mode: None,
            runtime_mode: None,
            mode_source: None,
            format_version: Some(STORE_VERSION),
        };

        fs::create_dir_all(self.messages_dir(&metadata.id))
            .map_err(|e| format!("Failed to create conversation: {e}"))?;
        let metadata_bytes = serde_json::to_vec(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {e}"))?;
        atomic_write(&self.metadata_path(&metadata.id), &metadata_bytes)?;
        self.upsert_metadata(&metadata);
        self.index.active_id = Some(metadata.id.clone());
        self.save_index()?;
        Ok(metadata)
    }

    /// Delete a conversation
    pub fn delete_conversation(&mut self, id: &str) -> Result<(), String> {
        validate_id(id)?;
        let path = self.conversation_dir(id);
        if path.exists() {
            fs::remove_dir_all(&path)
                .map_err(|e| format!("Failed to delete {}: {e}", path.display()))?;
        }
        self.index.conversations.retain(|m| m.id != id);
        if self.index.active_id.as_deref() == Some(id) {
            self.index.active_id = None;
        }
        self.save_index()?;
        Ok(())
    }

    /// Set the active conversation.
    pub fn set_active(&mut self, id: &str) -> Result<(), String> {
        validate_id(id)?;
        if !self.metadata_path(id).is_file() {
            return Err(format!("Conversation {id} not found"));
        }
        self.index.active_id = Some(id.to_string());
        self.save_index()
    }

    /// Get the active conversation ID
    pub fn get_active(&self) -> Option<String> {
        self.index.active_id.clone()
    }

    /// Save the index file atomically.
    fn save_index(&self) -> Result<(), String> {
        let path = self.storage_path.join("index.json");
        let content = serde_json::to_vec(&self.index)
            .map_err(|e| format!("Failed to serialize index: {e}"))?;
        atomic_write(&path, &content)
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
    use crate::protocol::{ProgressInfo, ToolCall, ToolFunction};
    use tempfile::TempDir;

    fn metadata(id: &str) -> ConversationMetadata {
        let now = Utc::now();
        ConversationMetadata {
            id: id.to_string(),
            title: "Test conversation".to_string(),
            created_at: now,
            updated_at: now,
            model_id: "test-model".to_string(),
            message_count: 0,
            session_id: Some("session".to_string()),
            planning_mode: Some(false),
            runtime_mode: Some("code".to_string()),
            mode_source: Some("test".to_string()),
            format_version: None,
        }
    }

    fn stored(id: &str, count: usize) -> StoredConversation {
        let messages = (0..count)
            .map(|index| {
                let mut message = ChatMessage::new(
                    if index % 2 == 0 {
                        ChatRole::User
                    } else {
                        ChatRole::Assistant
                    },
                    format!("message {index}"),
                );
                message.reasoning = Some(format!("reasoning {index}"));
                SerializableChatMessage::from(&message)
            })
            .collect::<Vec<_>>();
        let mut metadata = metadata(id);
        metadata.message_count = messages.len();
        StoredConversation { metadata, messages }
    }

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

    #[test]
    fn round_trip_preserves_every_message_field() {
        let temp = TempDir::new().unwrap();
        let mut store = ConversationStore::new(temp.path().join("conversations")).unwrap();
        let mut message = ChatMessage::new(ChatRole::Assistant, "answer".to_string());
        message.backend_content = Some("backend answer".to_string());
        message.reasoning = Some("careful reasoning".to_string());
        message.tool_call_id = Some("parent-call".to_string());
        message.tool_calls = Some(vec![ToolCall {
            id: "call-1".to_string(),
            typ: "function".to_string(),
            function: ToolFunction {
                name: "read_file".to_string(),
                arguments: "{\"path\":\"src/main.rs\"}".to_string(),
            },
            status: Some("complete".to_string()),
            result: Some("contents".to_string()),
        }]);
        message.progress = Some(ProgressInfo {
            message: "Working".to_string(),
            stage: "tools".to_string(),
            percent: 75,
        });
        message.content_before_tools = Some("before".to_string());
        message.content_after_tools = Some("after".to_string());

        let conversation = StoredConversation {
            metadata: metadata("lossless"),
            messages: vec![SerializableChatMessage::from(&message)],
        };
        store.save_conversation(&conversation).unwrap();
        let restored: ChatMessage = store
            .load_conversation("lossless")
            .unwrap()
            .messages
            .remove(0)
            .into();

        assert_eq!(restored.id, message.id);
        assert_eq!(restored.backend_content, message.backend_content);
        assert_eq!(restored.reasoning, message.reasoning);
        assert_eq!(restored.tool_call_id, message.tool_call_id);
        assert_eq!(
            restored.tool_calls.unwrap()[0].result.as_deref(),
            Some("contents")
        );
        assert_eq!(restored.progress.unwrap().percent, 75);
        assert_eq!(restored.content_before_tools.as_deref(), Some("before"));
        assert_eq!(restored.content_after_tools.as_deref(), Some("after"));
    }

    #[test]
    fn page_read_does_not_touch_earlier_messages() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("conversations");
        let mut store = ConversationStore::new(path.clone()).unwrap();
        store.save_conversation(&stored("paged", 10)).unwrap();

        fs::write(
            path.join("paged")
                .join("messages")
                .join(message_file_name(0)),
            b"not json",
        )
        .unwrap();

        let page = store.load_message_page("paged", 5, 3).unwrap();
        assert_eq!(page.offset, 5);
        assert_eq!(page.total, 10);
        assert_eq!(page.messages.len(), 3);
        assert!(store.load_conversation("paged").is_err());
    }

    #[test]
    fn append_rewrites_only_the_mutable_tail() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("conversations");
        let mut store = ConversationStore::new(path.clone()).unwrap();
        let mut conversation = stored("incremental", 4);
        store.save_conversation(&conversation).unwrap();
        let first_path = path
            .join("incremental")
            .join("messages")
            .join(message_file_name(0));
        let first_before = fs::read(&first_path).unwrap();

        conversation
            .messages
            .push(SerializableChatMessage::from(&ChatMessage::new(
                ChatRole::User,
                "appended".to_string(),
            )));
        store.save_conversation(&conversation).unwrap();

        assert_eq!(fs::read(first_path).unwrap(), first_before);
        assert_eq!(
            store
                .load_conversation("incremental")
                .unwrap()
                .messages
                .len(),
            5
        );
    }

    #[test]
    fn legacy_ui_file_migrates_without_consuming_artifact_files() {
        let temp = TempDir::new().unwrap();
        let canonical = temp.path().join("conversations");
        let legacy = temp.path().join("artifacts").join("conversations");
        fs::create_dir_all(&legacy).unwrap();
        let source = legacy.join("legacy.json");
        fs::write(&source, serde_json::to_vec(&stored("legacy", 3)).unwrap()).unwrap();
        fs::write(
            legacy.join("artifact.json"),
            br#"{"version":"1.0","conversation_id":"artifact"}"#,
        )
        .unwrap();

        let store = ConversationStore::new(canonical.clone()).unwrap();
        assert_eq!(store.load_conversation("legacy").unwrap().messages.len(), 3);
        assert!(!source.exists());
        assert!(legacy.join("legacy.json.migrated-v2.bak").exists());
        assert!(legacy.join("artifact.json").exists());

        drop(store);
        fs::write(
            canonical
                .join("legacy")
                .join("messages")
                .join(message_file_name(2)),
            b"corrupt after migration",
        )
        .unwrap();
        let recovered = ConversationStore::new(canonical).unwrap();
        assert_eq!(
            recovered
                .load_conversation("legacy")
                .unwrap()
                .messages
                .len(),
            3
        );
    }

    #[test]
    fn rejects_path_traversal_ids() {
        let temp = TempDir::new().unwrap();
        let mut store = ConversationStore::new(temp.path().join("conversations")).unwrap();
        let conversation = stored("../outside", 1);
        assert!(store.save_conversation(&conversation).is_err());
        assert!(!temp.path().join("outside").exists());
    }
}
