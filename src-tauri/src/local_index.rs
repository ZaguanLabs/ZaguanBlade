use rusqlite::{Connection, Result as SqliteResult, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::project_settings::get_zblade_dir;

/// Conversation metadata stored in the index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationIndex {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: i32,
    pub tags: Vec<String>,
    pub artifact_path: String,
}

/// Moment (extracted decision/pattern) stored in the index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MomentIndex {
    pub id: String,
    pub conversation_id: String,
    pub moment_type: String,
    pub content: String,
    pub context: Option<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub relevance_score: f64,
    pub artifact_path: String,
}

/// Code reference stored in the index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeReferenceIndex {
    pub id: i64,
    pub conversation_id: String,
    pub message_id: String,
    pub file_path: String,
    pub start_line: i32,
    pub end_line: i32,
    pub context: Option<String>,
    pub created_at: String,
}

/// Local index manager for a project
pub struct LocalIndex {
    conn: Connection,
}

impl LocalIndex {
    /// Open or create the local index database
    pub fn open(project_path: &Path) -> SqliteResult<Self> {
        let zblade_dir = get_zblade_dir(project_path);
        let index_dir = zblade_dir.join("index");
        
        // Ensure index directory exists
        std::fs::create_dir_all(&index_dir)
            .map_err(|e| rusqlite::Error::InvalidPath(index_dir.join(e.to_string())))?;
        
        let db_path = index_dir.join("conversations.db");
        let conn = Connection::open(&db_path)?;
        
        // Enable WAL mode for better concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        
        let index = Self { conn };
        index.init_schema()?;
        
        Ok(index)
    }
    
    /// Initialize the database schema
    fn init_schema(&self) -> SqliteResult<()> {
        self.conn.execute_batch(r#"
            -- Conversations table
            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                message_count INTEGER DEFAULT 0,
                tags TEXT,
                artifact_path TEXT NOT NULL
            );
            
            CREATE INDEX IF NOT EXISTS idx_conv_project ON conversations(project_id);
            CREATE INDEX IF NOT EXISTS idx_conv_created ON conversations(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_conv_updated ON conversations(updated_at DESC);
            
            -- Moments table
            CREATE TABLE IF NOT EXISTS moments (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                moment_type TEXT NOT NULL,
                content TEXT NOT NULL,
                context TEXT,
                tags TEXT,
                created_at TEXT NOT NULL,
                relevance_score REAL DEFAULT 0.5,
                artifact_path TEXT NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );
            
            CREATE INDEX IF NOT EXISTS idx_moment_conv ON moments(conversation_id);
            CREATE INDEX IF NOT EXISTS idx_moment_type ON moments(moment_type);
            CREATE INDEX IF NOT EXISTS idx_moment_score ON moments(relevance_score DESC);
            
            -- Code references table
            CREATE TABLE IF NOT EXISTS code_references (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                context TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );
            
            CREATE INDEX IF NOT EXISTS idx_code_ref_file ON code_references(file_path);
            CREATE INDEX IF NOT EXISTS idx_code_ref_conv ON code_references(conversation_id);
            
            -- File references table (track which files are referenced)
            CREATE TABLE IF NOT EXISTS file_references (
                file_path TEXT PRIMARY KEY,
                reference_count INTEGER DEFAULT 0,
                first_referenced TEXT,
                last_referenced TEXT
            );
        "#)?;
        
        // Create FTS5 virtual table for full-text search on moments
        // This may fail if already exists, so we ignore the error
        let _ = self.conn.execute_batch(r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS moments_fts USING fts5(
                content,
                context,
                tags,
                content=moments,
                content_rowid=rowid
            );
            
            -- Triggers to keep FTS in sync
            CREATE TRIGGER IF NOT EXISTS moments_ai AFTER INSERT ON moments BEGIN
                INSERT INTO moments_fts(rowid, content, context, tags)
                VALUES (NEW.rowid, NEW.content, NEW.context, NEW.tags);
            END;
            
            CREATE TRIGGER IF NOT EXISTS moments_ad AFTER DELETE ON moments BEGIN
                INSERT INTO moments_fts(moments_fts, rowid, content, context, tags)
                VALUES ('delete', OLD.rowid, OLD.content, OLD.context, OLD.tags);
            END;
            
            CREATE TRIGGER IF NOT EXISTS moments_au AFTER UPDATE ON moments BEGIN
                INSERT INTO moments_fts(moments_fts, rowid, content, context, tags)
                VALUES ('delete', OLD.rowid, OLD.content, OLD.context, OLD.tags);
                INSERT INTO moments_fts(rowid, content, context, tags)
                VALUES (NEW.rowid, NEW.content, NEW.context, NEW.tags);
            END;
        "#);
        
        Ok(())
    }
    
    // =========================================================================
    // Conversation Operations
    // =========================================================================
    
    /// Insert or update a conversation in the index
    pub fn upsert_conversation(&self, conv: &ConversationIndex) -> SqliteResult<()> {
        let tags_json = serde_json::to_string(&conv.tags).unwrap_or_default();
        
        self.conn.execute(
            r#"
            INSERT INTO conversations (id, project_id, title, created_at, updated_at, message_count, tags, artifact_path)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                updated_at = excluded.updated_at,
                message_count = excluded.message_count,
                tags = excluded.tags,
                artifact_path = excluded.artifact_path
            "#,
            params![
                conv.id,
                conv.project_id,
                conv.title,
                conv.created_at,
                conv.updated_at,
                conv.message_count,
                tags_json,
                conv.artifact_path,
            ],
        )?;
        
        Ok(())
    }
    
    /// Get a conversation by ID
    pub fn get_conversation(&self, id: &str) -> SqliteResult<Option<ConversationIndex>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, title, created_at, updated_at, message_count, tags, artifact_path FROM conversations WHERE id = ?1"
        )?;
        
        let mut rows = stmt.query(params![id])?;
        
        if let Some(row) = rows.next()? {
            let tags_json: String = row.get(6)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            
            Ok(Some(ConversationIndex {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                message_count: row.get(5)?,
                tags,
                artifact_path: row.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }
    
    /// List all conversations, ordered by updated_at descending
    pub fn list_conversations(&self) -> SqliteResult<Vec<ConversationIndex>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, title, created_at, updated_at, message_count, tags, artifact_path FROM conversations ORDER BY updated_at DESC"
        )?;
        
        let rows = stmt.query_map([], |row| {
            let tags_json: String = row.get(6)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            
            Ok(ConversationIndex {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                message_count: row.get(5)?,
                tags,
                artifact_path: row.get(7)?,
            })
        })?;
        
        rows.collect()
    }
    
    /// Delete a conversation and all related data
    pub fn delete_conversation(&self, id: &str) -> SqliteResult<()> {
        self.conn.execute("DELETE FROM conversations WHERE id = ?1", params![id])?;
        Ok(())
    }
    
    // =========================================================================
    // Moment Operations
    // =========================================================================
    
    /// Insert or update a moment in the index
    pub fn upsert_moment(&self, moment: &MomentIndex) -> SqliteResult<()> {
        let tags_json = serde_json::to_string(&moment.tags).unwrap_or_default();
        
        self.conn.execute(
            r#"
            INSERT INTO moments (id, conversation_id, moment_type, content, context, tags, created_at, relevance_score, artifact_path)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                content = excluded.content,
                context = excluded.context,
                tags = excluded.tags,
                relevance_score = excluded.relevance_score,
                artifact_path = excluded.artifact_path
            "#,
            params![
                moment.id,
                moment.conversation_id,
                moment.moment_type,
                moment.content,
                moment.context,
                tags_json,
                moment.created_at,
                moment.relevance_score,
                moment.artifact_path,
            ],
        )?;
        
        Ok(())
    }
    
    /// Search moments using full-text search
    pub fn search_moments(&self, query: &str, limit: i32) -> SqliteResult<Vec<MomentIndex>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT m.id, m.conversation_id, m.moment_type, m.content, m.context, m.tags, m.created_at, m.relevance_score, m.artifact_path
            FROM moments m
            JOIN moments_fts fts ON m.rowid = fts.rowid
            WHERE moments_fts MATCH ?1
            ORDER BY m.relevance_score DESC
            LIMIT ?2
            "#
        )?;
        
        let rows = stmt.query_map(params![query, limit], |row| {
            let tags_json: String = row.get(5)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            
            Ok(MomentIndex {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                moment_type: row.get(2)?,
                content: row.get(3)?,
                context: row.get(4)?,
                tags,
                created_at: row.get(6)?,
                relevance_score: row.get(7)?,
                artifact_path: row.get(8)?,
            })
        })?;
        
        rows.collect()
    }
    
    /// Get moments by conversation ID
    pub fn get_moments_for_conversation(&self, conversation_id: &str) -> SqliteResult<Vec<MomentIndex>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, moment_type, content, context, tags, created_at, relevance_score, artifact_path FROM moments WHERE conversation_id = ?1 ORDER BY created_at DESC"
        )?;
        
        let rows = stmt.query_map(params![conversation_id], |row| {
            let tags_json: String = row.get(5)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            
            Ok(MomentIndex {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                moment_type: row.get(2)?,
                content: row.get(3)?,
                context: row.get(4)?,
                tags,
                created_at: row.get(6)?,
                relevance_score: row.get(7)?,
                artifact_path: row.get(8)?,
            })
        })?;
        
        rows.collect()
    }
    
    // =========================================================================
    // Code Reference Operations
    // =========================================================================
    
    /// Insert a code reference
    pub fn insert_code_reference(&self, ref_: &CodeReferenceIndex) -> SqliteResult<i64> {
        self.conn.execute(
            r#"
            INSERT INTO code_references (conversation_id, message_id, file_path, start_line, end_line, context, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                ref_.conversation_id,
                ref_.message_id,
                ref_.file_path,
                ref_.start_line,
                ref_.end_line,
                ref_.context,
                ref_.created_at,
            ],
        )?;
        
        // Update file_references table
        self.conn.execute(
            r#"
            INSERT INTO file_references (file_path, reference_count, first_referenced, last_referenced)
            VALUES (?1, 1, ?2, ?2)
            ON CONFLICT(file_path) DO UPDATE SET
                reference_count = reference_count + 1,
                last_referenced = excluded.last_referenced
            "#,
            params![ref_.file_path, ref_.created_at],
        )?;
        
        Ok(self.conn.last_insert_rowid())
    }
    
    /// Get code references for a file
    pub fn get_references_for_file(&self, file_path: &str) -> SqliteResult<Vec<CodeReferenceIndex>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, message_id, file_path, start_line, end_line, context, created_at FROM code_references WHERE file_path = ?1 ORDER BY created_at DESC"
        )?;
        
        let rows = stmt.query_map(params![file_path], |row| {
            Ok(CodeReferenceIndex {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                message_id: row.get(2)?,
                file_path: row.get(3)?,
                start_line: row.get(4)?,
                end_line: row.get(5)?,
                context: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        
        rows.collect()
    }
    
    /// Get code references for a conversation
    pub fn get_references_for_conversation(&self, conversation_id: &str) -> SqliteResult<Vec<CodeReferenceIndex>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, message_id, file_path, start_line, end_line, context, created_at FROM code_references WHERE conversation_id = ?1 ORDER BY created_at DESC"
        )?;
        
        let rows = stmt.query_map(params![conversation_id], |row| {
            Ok(CodeReferenceIndex {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                message_id: row.get(2)?,
                file_path: row.get(3)?,
                start_line: row.get(4)?,
                end_line: row.get(5)?,
                context: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_open_and_init() {
        let temp = tempdir().unwrap();
        let project_path = temp.path();
        
        // Create .zblade directory first
        crate::project_settings::init_zblade_dir(project_path).unwrap();
        
        let index = LocalIndex::open(project_path).unwrap();
        
        // Verify tables exist
        let tables: Vec<String> = index.conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        
        assert!(tables.contains(&"conversations".to_string()));
        assert!(tables.contains(&"moments".to_string()));
        assert!(tables.contains(&"code_references".to_string()));
    }

    #[test]
    fn test_conversation_crud() {
        let temp = tempdir().unwrap();
        let project_path = temp.path();
        crate::project_settings::init_zblade_dir(project_path).unwrap();
        
        let index = LocalIndex::open(project_path).unwrap();
        
        let conv = ConversationIndex {
            id: "conv_123".to_string(),
            project_id: "proj_456".to_string(),
            title: "Test Conversation".to_string(),
            created_at: "2026-01-17T14:00:00Z".to_string(),
            updated_at: "2026-01-17T15:00:00Z".to_string(),
            message_count: 5,
            tags: vec!["test".to_string(), "rust".to_string()],
            artifact_path: ".zblade/artifacts/conversations/conv_123.json".to_string(),
        };
        
        // Insert
        index.upsert_conversation(&conv).unwrap();
        
        // Get
        let loaded = index.get_conversation("conv_123").unwrap().unwrap();
        assert_eq!(loaded.title, "Test Conversation");
        assert_eq!(loaded.message_count, 5);
        assert_eq!(loaded.tags, vec!["test", "rust"]);
        
        // List
        let all = index.list_conversations().unwrap();
        assert_eq!(all.len(), 1);
        
        // Delete
        index.delete_conversation("conv_123").unwrap();
        let deleted = index.get_conversation("conv_123").unwrap();
        assert!(deleted.is_none());
    }

    #[test]
    fn test_code_references() {
        let temp = tempdir().unwrap();
        let project_path = temp.path();
        crate::project_settings::init_zblade_dir(project_path).unwrap();
        
        let index = LocalIndex::open(project_path).unwrap();
        
        // First create a conversation
        let conv = ConversationIndex {
            id: "conv_123".to_string(),
            project_id: "proj_456".to_string(),
            title: "Test".to_string(),
            created_at: "2026-01-17T14:00:00Z".to_string(),
            updated_at: "2026-01-17T14:00:00Z".to_string(),
            message_count: 1,
            tags: vec![],
            artifact_path: "test.json".to_string(),
        };
        index.upsert_conversation(&conv).unwrap();
        
        // Insert code reference
        let ref_ = CodeReferenceIndex {
            id: 0,
            conversation_id: "conv_123".to_string(),
            message_id: "msg_001".to_string(),
            file_path: "src/auth.ts".to_string(),
            start_line: 10,
            end_line: 25,
            context: Some("Authentication function".to_string()),
            created_at: "2026-01-17T14:00:00Z".to_string(),
        };
        
        let id = index.insert_code_reference(&ref_).unwrap();
        assert!(id > 0);
        
        // Get by file
        let refs = index.get_references_for_file("src/auth.ts").unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].start_line, 10);
        assert_eq!(refs[0].end_line, 25);
        
        // Get by conversation
        let refs = index.get_references_for_conversation("conv_123").unwrap();
        assert_eq!(refs.len(), 1);
    }
}
