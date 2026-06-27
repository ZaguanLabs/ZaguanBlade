//! Symbol storage using SQLite
//!
//! Persistent storage for extracted code symbols with efficient
//! indexing and retrieval.

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use crate::tree_sitter::{Language, Symbol, SymbolRelationship, SymbolRelationshipType, SymbolType};

const GENERATED_INDEX_CLEAR_STATEMENTS: &[&str] = &[
    "DELETE FROM symbol_relationships",
    "DELETE FROM semantic_anchors",
    "DELETE FROM symbols",
    "DELETE FROM indexed_files",
];

const GENERATED_INDEX_TABLES: &[&str] = &[
    "symbol_relationships",
    "semantic_anchors",
    "symbols",
    "indexed_files",
];

/// M2.4 set-based back-fill. One `UPDATE`: for every relationship whose
/// `target_symbol_id` is still NULL, write the id of the matching symbol ONLY
/// when `target_name` matches EXACTLY ONE candidate symbol globally. The
/// `COUNT(*) = 1` guard and the value sub-`SELECT` share an identical candidate
/// filter (`name = target_name`, excluding `import` placeholders and the
/// synthetic `__file__` root), so an ambiguous name can never be written and a
/// unique name resolves deterministically. `import` relationships are skipped.
/// `confidence = 0.5`: a name-only heuristic (no scope/type analysis) — the
/// uniqueness guard is what makes it safe, not the score.
const BACKFILL_GLOBAL_UNIQUE_SQL: &str = r#"
UPDATE symbol_relationships
SET target_symbol_id = (
        SELECT s.id
        FROM symbols s
        WHERE s.name = symbol_relationships.target_name
          AND s.name != ''
          AND s.symbol_type != 'import'
          AND s.qualified_name != '__file__'
    ),
    resolution_strategy = 'global_unique',
    confidence = 0.5
WHERE target_symbol_id IS NULL
  AND relationship_type != 'import'
  AND symbol_relationships.target_name != ''
  AND (
        SELECT COUNT(*)
        FROM symbols s2
        WHERE s2.name = symbol_relationships.target_name
          AND s2.name != ''
          AND s2.symbol_type != 'import'
          AND s2.qualified_name != '__file__'
      ) = 1
"#;

/// SQLite-backed symbol store
pub struct SymbolStore {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct SymbolReference {
    pub source_symbol: Symbol,
    pub relationship_type: SymbolRelationshipType,
    pub target_name: String,
    pub target_symbol_id: Option<String>,
    pub target_symbol: Option<Symbol>,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct IndexedFileRecord {
    pub file_path: String,
    pub file_hash: String,
    pub indexed_at: i64,
    pub symbol_count: usize,
    pub file_size: Option<u64>,
    pub line_count: Option<usize>,
    pub modified_at: Option<i64>,
    pub extractor_version: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct FileIndexRecord {
    pub file_path: String,
    pub file_hash: String,
    pub file_size: Option<u64>,
    pub line_count: Option<usize>,
    pub modified_at: Option<i64>,
    pub extractor_version: Option<u32>,
    pub symbols: Vec<Symbol>,
    pub anchors: Vec<SemanticAnchor>,
    pub relationships: Vec<SymbolRelationship>,
}

#[derive(Debug, Clone)]
pub struct FileRelationshipRecord {
    pub file_path: String,
    pub relationships: Vec<SymbolRelationship>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationshipIntegrityStats {
    pub total_relationships: usize,
    pub resolved_relationships: usize,
    pub unresolved_symbol_relationships: usize,
    pub missing_source_symbols: usize,
    pub missing_target_symbols: usize,
    pub by_type: Vec<RelationshipTypeStats>,
    pub top_unresolved_targets: Vec<UnresolvedRelationshipTarget>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationshipTypeStats {
    pub relationship_type: String,
    pub total_relationships: usize,
    pub resolved_relationships: usize,
    pub unresolved_symbol_relationships: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnresolvedRelationshipTarget {
    pub relationship_type: String,
    pub target_name: String,
    pub count: usize,
    pub example_source_file: String,
}

/// Per-language × per-kind coverage matrix with explicit `0` rows back-filled.
///
/// Read-only diagnostic answering "are we indexing the right data per file
/// type." Serialize-friendly so it can later feed the schema tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoverageHistogram {
    pub per_language: Vec<LanguageCoverage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LanguageCoverage {
    pub language: String,
    /// Count per `SymbolType` variant; absent variants are present as `0`.
    pub symbol_counts: Vec<(String, u64)>,
    /// Count per `SymbolRelationshipType` variant; absent variants are `0`.
    pub relationship_counts: Vec<(String, u64)>,
    /// Fraction (0.0..=1.0) of Function+Method symbols with a non-empty signature.
    pub function_method_signature_pct: f64,
}

/// Every `SymbolType` variant, so the histogram can back-fill an explicit `0`
/// row for each kind that never appeared in a given language.
const ALL_SYMBOL_TYPES: &[SymbolType] = &[
    SymbolType::Function,
    SymbolType::Method,
    SymbolType::Class,
    SymbolType::Struct,
    SymbolType::Interface,
    SymbolType::Type,
    SymbolType::Enum,
    SymbolType::EnumMember,
    SymbolType::Constant,
    SymbolType::Variable,
    SymbolType::Property,
    SymbolType::Module,
    SymbolType::Namespace,
    SymbolType::Import,
    SymbolType::Export,
    SymbolType::Trait,
    SymbolType::Impl,
    SymbolType::Heading,
    SymbolType::CssSelector,
    SymbolType::CssCustomProperty,
    SymbolType::CssKeyframes,
    SymbolType::CssAtRule,
    SymbolType::CssLayer,
    SymbolType::CssFontFace,
];

/// Every `SymbolRelationshipType` variant, for the same zero back-fill.
const ALL_RELATIONSHIP_TYPES: &[SymbolRelationshipType] = &[
    SymbolRelationshipType::Call,
    SymbolRelationshipType::Import,
    SymbolRelationshipType::Export,
    SymbolRelationshipType::Extends,
    SymbolRelationshipType::Implements,
    SymbolRelationshipType::Contains,
    SymbolRelationshipType::Usage,
];

// NOTE: language derived from file path until M2.3 adds a real `language` column; re-base then.
fn coverage_language_label(file_path: &str) -> String {
    Language::from_path(file_path)
        .map(|language| language.display_name().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// Drift guard: adding a variant to either enum without extending the arrays
// above is a compile error, keeping the zero back-fill exhaustive.
#[allow(dead_code)]
fn coverage_variant_drift_guard(
    symbol_type: SymbolType,
    relationship_type: SymbolRelationshipType,
) {
    match symbol_type {
        SymbolType::Function
        | SymbolType::Method
        | SymbolType::Class
        | SymbolType::Struct
        | SymbolType::Interface
        | SymbolType::Type
        | SymbolType::Enum
        | SymbolType::EnumMember
        | SymbolType::Constant
        | SymbolType::Variable
        | SymbolType::Property
        | SymbolType::Module
        | SymbolType::Namespace
        | SymbolType::Import
        | SymbolType::Export
        | SymbolType::Trait
        | SymbolType::Impl
        | SymbolType::Heading
        | SymbolType::CssSelector
        | SymbolType::CssCustomProperty
        | SymbolType::CssKeyframes
        | SymbolType::CssAtRule
        | SymbolType::CssLayer
        | SymbolType::CssFontFace => {}
    }
    match relationship_type {
        SymbolRelationshipType::Call
        | SymbolRelationshipType::Import
        | SymbolRelationshipType::Export
        | SymbolRelationshipType::Extends
        | SymbolRelationshipType::Implements
        | SymbolRelationshipType::Contains
        | SymbolRelationshipType::Usage => {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticAnchor {
    pub id: String,
    pub file_path: String,
    pub kind: String,
    pub value: String,
    pub line: u32,
    pub character: u32,
    pub preview: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticAnchorResult {
    pub anchor: SemanticAnchor,
    pub score: f32,
}

fn query_scoped_count(
    conn: &Connection,
    unscoped_sql: &str,
    scoped_sql: &str,
    scoped: Option<(&str, &str)>,
) -> Result<i64, SymbolStoreError> {
    match scoped {
        Some((scope, like)) => conn.query_row(scoped_sql, params![scope, like], |row| row.get(0)),
        None => conn.query_row(unscoped_sql, [], |row| row.get(0)),
    }
    .map_err(SymbolStoreError::from)
}

impl SymbolStore {
    /// Create a new symbol store at the given path
    pub fn new(db_path: &Path) -> Result<Self, SymbolStoreError> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.create_schema()?;
        Ok(store)
    }

    /// Create an in-memory symbol store (for testing)
    pub fn in_memory() -> Result<Self, SymbolStoreError> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.create_schema()?;
        Ok(store)
    }

    /// Create database schema
    fn create_schema(&self) -> Result<(), SymbolStoreError> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            r#"
            -- Main symbols table
            CREATE TABLE IF NOT EXISTS symbols (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                qualified_name TEXT NOT NULL DEFAULT '',
                symbol_type TEXT NOT NULL,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                start_char INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_char INTEGER NOT NULL,
                byte_offset INTEGER NOT NULL DEFAULT 0,
                byte_length INTEGER NOT NULL DEFAULT 0,
                parent_id TEXT,
                docstring TEXT,
                signature TEXT,
                content_hash TEXT NOT NULL DEFAULT '',
                indexed_at INTEGER NOT NULL
            );

            -- Full-text search using FTS5
            CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
                name,
                docstring,
                content=symbols,
                content_rowid=rowid
            );

            -- Triggers to keep FTS in sync
            CREATE TRIGGER IF NOT EXISTS symbols_ai AFTER INSERT ON symbols BEGIN
                INSERT INTO symbols_fts(rowid, name, docstring)
                VALUES (new.rowid, new.name, new.docstring);
            END;

            CREATE TRIGGER IF NOT EXISTS symbols_ad AFTER DELETE ON symbols BEGIN
                INSERT INTO symbols_fts(symbols_fts, rowid, name, docstring)
                VALUES ('delete', old.rowid, old.name, old.docstring);
            END;

            CREATE TRIGGER IF NOT EXISTS symbols_au AFTER UPDATE ON symbols BEGIN
                INSERT INTO symbols_fts(symbols_fts, rowid, name, docstring)
                VALUES ('delete', old.rowid, old.name, old.docstring);
                INSERT INTO symbols_fts(rowid, name, docstring)
                VALUES (new.rowid, new.name, new.docstring);
            END;

            -- File metadata for tracking indexing status
            CREATE TABLE IF NOT EXISTS indexed_files (
                file_path TEXT PRIMARY KEY,
                file_hash TEXT,
                indexed_at INTEGER NOT NULL,
                symbol_count INTEGER NOT NULL,
                file_size INTEGER,
                line_count INTEGER,
                modified_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS symbol_relationships (
                source_symbol_id TEXT NOT NULL,
                source_file_path TEXT NOT NULL,
                target_name TEXT NOT NULL,
                target_symbol_id TEXT,
                relationship_type TEXT NOT NULL,
                line INTEGER NOT NULL,
                PRIMARY KEY (source_symbol_id, target_name, relationship_type, line)
            );


            CREATE TABLE IF NOT EXISTS semantic_anchors (
                id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                kind TEXT NOT NULL,
                value TEXT NOT NULL,
                line INTEGER NOT NULL,
                character INTEGER NOT NULL,
                preview TEXT NOT NULL,
                confidence REAL NOT NULL,
                indexed_at INTEGER NOT NULL
            );
            "#,
        )?;

        ensure_column(
            &conn,
            "symbols",
            "qualified_name",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            &conn,
            "symbols",
            "byte_offset",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            &conn,
            "symbols",
            "byte_length",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(&conn, "symbols", "content_hash", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(&conn, "symbol_relationships", "target_symbol_id", "TEXT")?;
        ensure_column(&conn, "indexed_files", "file_size", "INTEGER")?;
        ensure_column(&conn, "indexed_files", "line_count", "INTEGER")?;
        ensure_column(&conn, "indexed_files", "modified_at", "INTEGER")?;
        ensure_column(&conn, "indexed_files", "extractor_version", "INTEGER")?;

        conn.execute_batch(
            r#"
            -- Indexes for common queries
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_qualified_name ON symbols(qualified_name);
            CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
            CREATE INDEX IF NOT EXISTS idx_symbols_type ON symbols(symbol_type);
            CREATE INDEX IF NOT EXISTS idx_symbols_parent ON symbols(parent_id);
            CREATE INDEX IF NOT EXISTS idx_symbols_indexed ON symbols(indexed_at);
            CREATE INDEX IF NOT EXISTS idx_symbol_relationships_source ON symbol_relationships(source_symbol_id);
            CREATE INDEX IF NOT EXISTS idx_symbol_relationships_file ON symbol_relationships(source_file_path);
            CREATE INDEX IF NOT EXISTS idx_symbol_relationships_target ON symbol_relationships(target_name);
            CREATE INDEX IF NOT EXISTS idx_symbol_relationships_target_symbol_id ON symbol_relationships(target_symbol_id);
            CREATE INDEX IF NOT EXISTS idx_semantic_anchors_value ON semantic_anchors(value);
            CREATE INDEX IF NOT EXISTS idx_semantic_anchors_file ON semantic_anchors(file_path);
            CREATE INDEX IF NOT EXISTS idx_semantic_anchors_kind ON semantic_anchors(kind);
            "#,
        )?;

        // Apply ordered, versioned migrations AFTER the base schema is ensured.
        // This framework owns NEW schema going forward (M2.3); the legacy
        // `ensure_column` probes above continue to own the CURRENT columns.
        run_migrations(&conn)?;

        Ok(())
    }

    /// Insert or update symbols for a file
    pub fn upsert_symbols(&self, symbols: &[Symbol]) -> Result<usize, SymbolStoreError> {
        if symbols.is_empty() {
            return Ok(0);
        }

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        for symbol in symbols {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO symbols 
                (id, name, qualified_name, symbol_type, file_path, start_line, start_char, end_line, end_char,
                 byte_offset, byte_length, parent_id, docstring, signature, content_hash, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
                params![
                    &symbol.id,
                    &symbol.name,
                    &symbol.qualified_name,
                    symbol.symbol_type.to_string(),
                    &symbol.file_path,
                    symbol.range.start.line,
                    symbol.range.start.character,
                    symbol.range.end.line,
                    symbol.range.end.character,
                    symbol.byte_offset as i64,
                    symbol.byte_length as i64,
                    symbol.parent_id.as_deref(),
                    symbol.docstring.as_deref(),
                    symbol.signature.as_deref(),
                    &symbol.content_hash,
                    now,
                ],
            )?;
        }

        tx.commit()?;
        Ok(symbols.len())
    }

    pub fn replace_semantic_anchors_for_file(
        &self,
        file_path: &str,
        anchors: &[SemanticAnchor],
    ) -> Result<usize, SymbolStoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        tx.execute(
            "DELETE FROM semantic_anchors WHERE file_path = ?1",
            params![file_path],
        )?;

        for anchor in anchors {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO semantic_anchors
                (id, file_path, kind, value, line, character, preview, confidence, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
                params![
                    &anchor.id,
                    &anchor.file_path,
                    &anchor.kind,
                    &anchor.value,
                    anchor.line,
                    anchor.character,
                    &anchor.preview,
                    anchor.confidence,
                    now,
                ],
            )?;
        }

        tx.commit()?;
        Ok(anchors.len())
    }

    pub fn search_semantic_anchors(
        &self,
        query: &str,
        file_path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SemanticAnchorResult>, SymbolStoreError> {
        let trimmed = query.trim();
        if trimmed.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let query_pattern = format!("%{}%", trimmed);
        let conn = self.conn.lock().unwrap();
        let (sql, values) = if let Some(file_path) = file_path {
            (
                r#"
                SELECT id, file_path, kind, value, line, character, preview, confidence
                FROM semantic_anchors
                WHERE file_path = ?1 AND (value LIKE ?2 OR preview LIKE ?2)
                ORDER BY CASE
                    WHEN lower(value) = lower(?3) THEN 0
                    WHEN lower(value) LIKE lower(?4) THEN 1
                    ELSE 2
                END, confidence DESC, file_path, line, character
                LIMIT ?5
                "#,
                vec![
                    Value::Text(file_path.to_string()),
                    Value::Text(query_pattern.clone()),
                    Value::Text(trimmed.to_string()),
                    Value::Text(format!("{}%", trimmed)),
                    Value::Integer(limit as i64),
                ],
            )
        } else {
            (
                r#"
                SELECT id, file_path, kind, value, line, character, preview, confidence
                FROM semantic_anchors
                WHERE value LIKE ?1 OR preview LIKE ?1
                ORDER BY CASE
                    WHEN lower(value) = lower(?2) THEN 0
                    WHEN lower(value) LIKE lower(?3) THEN 1
                    ELSE 2
                END, confidence DESC, file_path, line, character
                LIMIT ?4
                "#,
                vec![
                    Value::Text(query_pattern.clone()),
                    Value::Text(trimmed.to_string()),
                    Value::Text(format!("{}%", trimmed)),
                    Value::Integer(limit as i64),
                ],
            )
        };
        let mut stmt = conn.prepare(sql)?;
        let anchors = stmt
            .query_map(params_from_iter(values), |row| {
                let anchor = row_to_semantic_anchor(row)?;
                let score = semantic_anchor_score(&anchor, trimmed);
                Ok(SemanticAnchorResult { anchor, score })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(anchors)
    }

    pub fn get_semantic_anchors_in_file(
        &self,
        file_path: &str,
        limit: usize,
    ) -> Result<Vec<SemanticAnchor>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, file_path, kind, value, line, character, preview, confidence
            FROM semantic_anchors
            WHERE file_path = ?1
            ORDER BY line, character, value
            LIMIT ?2
            "#,
        )?;
        let anchors = stmt
            .query_map(params![file_path, limit as i64], |row| {
                row_to_semantic_anchor(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(anchors)
    }

    pub fn replace_relationships_for_file(
        &self,
        file_path: &str,
        relationships: &[SymbolRelationship],
    ) -> Result<usize, SymbolStoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        tx.execute(
            "DELETE FROM symbol_relationships WHERE source_file_path = ?1",
            params![file_path],
        )?;

        for relationship in relationships {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO symbol_relationships
                (source_symbol_id, source_file_path, target_name, target_symbol_id, relationship_type, line)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    &relationship.source_symbol_id,
                    &relationship.source_file_path,
                    &relationship.target_name,
                    relationship.target_symbol_id.as_deref(),
                    relationship.relationship_type.to_string(),
                    relationship.line,
                ],
            )?;
        }

        tx.commit()?;
        Ok(relationships.len())
    }

    pub fn replace_file_index(
        &self,
        file_path: &str,
        file_hash: &str,
        file_size: Option<u64>,
        line_count: Option<usize>,
        modified_at: Option<i64>,
        extractor_version: Option<u32>,
        symbols: &[Symbol],
        anchors: &[SemanticAnchor],
        relationships: &[SymbolRelationship],
    ) -> Result<(), SymbolStoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        tx.execute(
            "DELETE FROM symbol_relationships WHERE source_file_path = ?1",
            params![file_path],
        )?;
        tx.execute(
            "DELETE FROM semantic_anchors WHERE file_path = ?1",
            params![file_path],
        )?;
        tx.execute(
            "DELETE FROM symbols WHERE file_path = ?1",
            params![file_path],
        )?;

        for symbol in symbols {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO symbols
                (id, name, qualified_name, symbol_type, file_path, start_line, start_char, end_line, end_char,
                 byte_offset, byte_length, parent_id, docstring, signature, content_hash, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
                params![
                    &symbol.id,
                    &symbol.name,
                    &symbol.qualified_name,
                    symbol.symbol_type.to_string(),
                    &symbol.file_path,
                    symbol.range.start.line,
                    symbol.range.start.character,
                    symbol.range.end.line,
                    symbol.range.end.character,
                    symbol.byte_offset as i64,
                    symbol.byte_length as i64,
                    symbol.parent_id.as_deref(),
                    symbol.docstring.as_deref(),
                    symbol.signature.as_deref(),
                    &symbol.content_hash,
                    now,
                ],
            )?;
        }

        for anchor in anchors {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO semantic_anchors
                (id, file_path, kind, value, line, character, preview, confidence, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
                params![
                    &anchor.id,
                    &anchor.file_path,
                    &anchor.kind,
                    &anchor.value,
                    anchor.line,
                    anchor.character,
                    &anchor.preview,
                    anchor.confidence,
                    now,
                ],
            )?;
        }

        for relationship in relationships {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO symbol_relationships
                (source_symbol_id, source_file_path, target_name, target_symbol_id, relationship_type, line)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    &relationship.source_symbol_id,
                    &relationship.source_file_path,
                    &relationship.target_name,
                    relationship.target_symbol_id.as_deref(),
                    relationship.relationship_type.to_string(),
                    relationship.line,
                ],
            )?;
        }

        tx.execute(
            r#"
            INSERT OR REPLACE INTO indexed_files
            (file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                file_path,
                file_hash,
                now,
                symbols.len() as i64,
                file_size.map(|size| size as i64),
                line_count.map(|count| count as i64),
                modified_at,
                extractor_version.map(i64::from)
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn replace_file_indexes(&self, files: &[FileIndexRecord]) -> Result<(), SymbolStoreError> {
        if files.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        {
            let mut delete_relationships =
                tx.prepare("DELETE FROM symbol_relationships WHERE source_file_path = ?1")?;
            let mut delete_anchors =
                tx.prepare("DELETE FROM semantic_anchors WHERE file_path = ?1")?;
            let mut delete_symbols = tx.prepare("DELETE FROM symbols WHERE file_path = ?1")?;

            for file in files {
                delete_relationships.execute(params![&file.file_path])?;
                delete_anchors.execute(params![&file.file_path])?;
                delete_symbols.execute(params![&file.file_path])?;
            }
        }

        {
            let mut insert_symbol = tx.prepare(
                r#"
                INSERT OR REPLACE INTO symbols
                (id, name, qualified_name, symbol_type, file_path, start_line, start_char, end_line, end_char,
                 byte_offset, byte_length, parent_id, docstring, signature, content_hash, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
            )?;

            for file in files {
                for symbol in &file.symbols {
                    insert_symbol.execute(params![
                        &symbol.id,
                        &symbol.name,
                        &symbol.qualified_name,
                        symbol.symbol_type.to_string(),
                        &symbol.file_path,
                        symbol.range.start.line,
                        symbol.range.start.character,
                        symbol.range.end.line,
                        symbol.range.end.character,
                        symbol.byte_offset as i64,
                        symbol.byte_length as i64,
                        symbol.parent_id.as_deref(),
                        symbol.docstring.as_deref(),
                        symbol.signature.as_deref(),
                        &symbol.content_hash,
                        now,
                    ])?;
                }
            }
        }

        {
            let mut insert_anchor = tx.prepare(
                r#"
                INSERT OR REPLACE INTO semantic_anchors
                (id, file_path, kind, value, line, character, preview, confidence, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
            )?;

            for file in files {
                for anchor in &file.anchors {
                    insert_anchor.execute(params![
                        &anchor.id,
                        &anchor.file_path,
                        &anchor.kind,
                        &anchor.value,
                        anchor.line,
                        anchor.character,
                        &anchor.preview,
                        anchor.confidence,
                        now,
                    ])?;
                }
            }
        }

        {
            let mut insert_relationship = tx.prepare(
                r#"
                INSERT OR REPLACE INTO symbol_relationships
                (source_symbol_id, source_file_path, target_name, target_symbol_id, relationship_type, line)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
            )?;

            for file in files {
                for relationship in &file.relationships {
                    insert_relationship.execute(params![
                        &relationship.source_symbol_id,
                        &relationship.source_file_path,
                        &relationship.target_name,
                        relationship.target_symbol_id.as_deref(),
                        relationship.relationship_type.to_string(),
                        relationship.line,
                    ])?;
                }
            }
        }

        {
            let mut insert_indexed_file = tx.prepare(
                r#"
                INSERT OR REPLACE INTO indexed_files
                (file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
            )?;

            for file in files {
                insert_indexed_file.execute(params![
                    &file.file_path,
                    &file.file_hash,
                    now,
                    file.symbols.len() as i64,
                    file.file_size.map(|size| size as i64),
                    file.line_count.map(|count| count as i64),
                    file.modified_at,
                    file.extractor_version.map(i64::from)
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn replace_relationships_for_files(
        &self,
        files: &[FileRelationshipRecord],
    ) -> Result<(), SymbolStoreError> {
        if files.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        {
            let mut delete_relationships =
                tx.prepare("DELETE FROM symbol_relationships WHERE source_file_path = ?1")?;
            for file in files {
                delete_relationships.execute(params![&file.file_path])?;
            }
        }

        {
            let mut insert_relationship = tx.prepare(
                r#"
                INSERT OR REPLACE INTO symbol_relationships
                (source_symbol_id, source_file_path, target_name, target_symbol_id, relationship_type, line)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
            )?;

            for file in files {
                for relationship in &file.relationships {
                    insert_relationship.execute(params![
                        &relationship.source_symbol_id,
                        &relationship.source_file_path,
                        &relationship.target_name,
                        relationship.target_symbol_id.as_deref(),
                        relationship.relationship_type.to_string(),
                        relationship.line,
                    ])?;
                }
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// M2.4 — set-based relationship back-fill.
    ///
    /// Resolves `symbol_relationships.target_symbol_id` for rows that are still
    /// NULL by matching `target_name` against the GLOBAL symbol set in a single
    /// `UPDATE`, replacing the old per-reference `search_symbols_contextual`
    /// round-trips (the cold-index bottleneck: 81–97% of wall time).
    ///
    /// CORRECTNESS GUARD: a target is written ONLY when the name matches
    /// EXACTLY ONE candidate symbol globally (`COUNT(*) = 1`). An ambiguous name
    /// (`> 1` match) is left NULL — a wrong `target_symbol_id` silently corrupts
    /// the knowledge graph (symbol_trace / edit_impact). Already-resolved
    /// (non-NULL) rows are never overwritten, and `import` edges are left to the
    /// dedicated import canonicalization. Candidate symbols exclude `import`
    /// placeholders and the synthetic `__file__` root, mirroring the in-memory
    /// resolver's filter. The `COUNT(*) = 1` predicate is byte-for-byte the same
    /// candidate filter as the value `SELECT`, so the guard cannot disagree with
    /// the write.
    ///
    /// Idempotent: re-running only ever turns NULL into a unique match. Returns
    /// the number of rows newly resolved.
    pub fn backfill_unresolved_relationship_targets(&self) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let resolved = conn.execute(BACKFILL_GLOBAL_UNIQUE_SQL, [])?;
        Ok(resolved)
    }

    pub fn get_relationship_targets(
        &self,
        source_symbol_id: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<String>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT target_name
            FROM symbol_relationships
            WHERE source_symbol_id = ?1 AND relationship_type = ?2
            ORDER BY line, target_name
            LIMIT ?3
            "#,
        )?;

        let targets = stmt
            .query_map(
                params![
                    source_symbol_id,
                    relationship_type.to_string(),
                    limit as i64
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(targets)
    }

    pub fn get_file_relationship_targets(
        &self,
        source_file_path: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<String>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT target_name
            FROM symbol_relationships
            WHERE source_file_path = ?1 AND relationship_type = ?2
            ORDER BY target_name
            LIMIT ?3
            "#,
        )?;

        let targets = stmt
            .query_map(
                params![
                    source_file_path,
                    relationship_type.to_string(),
                    limit as i64
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(targets)
    }

    pub fn find_references_to_target(
        &self,
        target_name: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<SymbolReference>, SymbolStoreError> {
        let rows = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                r#"
                SELECT s.id, s.name, s.qualified_name, s.symbol_type, s.file_path, s.start_line, s.start_char,
                       s.end_line, s.end_char, s.byte_offset, s.byte_length, s.parent_id, s.docstring, s.signature,
                       s.content_hash, r.relationship_type, r.target_name, r.target_symbol_id, r.line
                FROM symbol_relationships r
                JOIN symbols s ON s.id = r.source_symbol_id
                WHERE r.target_name = ?1 AND r.relationship_type = ?2
                ORDER BY s.file_path, r.line, s.start_line, s.start_char
                LIMIT ?3
                "#,
            )?;

            let rows = stmt
                .query_map(
                    params![target_name, relationship_type.to_string(), limit as i64],
                    |row| {
                        Ok((
                            row_to_symbol(row)?,
                            row.get::<_, String>(15)?,
                            row.get::<_, String>(16)?,
                            row.get::<_, Option<String>>(17)?,
                            row.get::<_, i64>(18)? as u32,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;

            rows
        };

        self.hydrate_symbol_references(rows)
    }

    pub fn find_references_to_symbol_id(
        &self,
        target_symbol_id: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<SymbolReference>, SymbolStoreError> {
        let rows = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                r#"
                SELECT s.id, s.name, s.qualified_name, s.symbol_type, s.file_path, s.start_line, s.start_char,
                       s.end_line, s.end_char, s.byte_offset, s.byte_length, s.parent_id, s.docstring, s.signature,
                       s.content_hash, r.relationship_type, r.target_name, r.target_symbol_id, r.line
                FROM symbol_relationships r
                JOIN symbols s ON s.id = r.source_symbol_id
                WHERE r.target_symbol_id = ?1 AND r.relationship_type = ?2
                ORDER BY s.file_path, r.line, s.start_line, s.start_char
                LIMIT ?3
                "#,
            )?;

            let rows = stmt
                .query_map(
                    params![
                        target_symbol_id,
                        relationship_type.to_string(),
                        limit as i64
                    ],
                    |row| {
                        Ok((
                            row_to_symbol(row)?,
                            row.get::<_, String>(15)?,
                            row.get::<_, String>(16)?,
                            row.get::<_, Option<String>>(17)?,
                            row.get::<_, i64>(18)? as u32,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;

            rows
        };

        self.hydrate_symbol_references(rows)
    }

    pub fn get_relationship_edges_from_source(
        &self,
        source_symbol_id: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<SymbolReference>, SymbolStoreError> {
        let Some(source_symbol) = self.get_symbol(source_symbol_id)? else {
            return Ok(Vec::new());
        };

        let rows = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                r#"
                SELECT relationship_type, target_name, target_symbol_id, line
                FROM symbol_relationships
                WHERE source_symbol_id = ?1 AND relationship_type = ?2
                ORDER BY line, target_name
                LIMIT ?3
                "#,
            )?;

            let rows = stmt
                .query_map(
                    params![
                        source_symbol_id,
                        relationship_type.to_string(),
                        limit as i64
                    ],
                    |row| {
                        Ok((
                            source_symbol.clone(),
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, i64>(3)? as u32,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;

            rows
        };

        self.hydrate_symbol_references(rows)
    }

    /// Get a symbol by ID
    pub fn get_symbol(&self, id: &str) -> Result<Option<Symbol>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols WHERE id = ?1
            "#,
        )?;

        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_symbol(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get all symbols in a file
    pub fn get_symbols_in_file(&self, file_path: &str) -> Result<Vec<Symbol>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols WHERE file_path = ?1
            ORDER BY start_line, start_char
            "#,
        )?;

        let symbols = stmt
            .query_map(params![file_path], |row| row_to_symbol(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Search symbols by name (with fuzzy matching)
    pub fn search_by_name(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Symbol>, SymbolStoreError> {
        let fts_query = symbol_search_fts_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().unwrap();

        // Use FTS5 for searching
        let mut stmt = conn.prepare(
            r#"
            SELECT s.id, s.name, s.qualified_name, s.symbol_type, s.file_path, s.start_line, s.start_char,
                   s.end_line, s.end_char, s.byte_offset, s.byte_length, s.parent_id, s.docstring, s.signature, s.content_hash
            FROM symbols s
            JOIN symbols_fts fts ON s.rowid = fts.rowid
            WHERE symbols_fts MATCH ?1
            ORDER BY bm25(symbols_fts), length(s.name), s.name
            LIMIT ?2
            "#,
        )?;

        let symbols = stmt
            .query_map(params![fts_query, limit as i64], |row| row_to_symbol(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Search symbols with LIKE pattern (fallback for simple queries)
    pub fn search_by_name_like(
        &self,
        pattern: &str,
        limit: usize,
    ) -> Result<Vec<Symbol>, SymbolStoreError> {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().unwrap();
        let mut symbols = Vec::new();
        let mut seen = HashSet::new();

        append_symbol_query_results(
            &conn,
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols
            WHERE name = ?1 OR qualified_name = ?1
            ORDER BY length(name), name
            LIMIT ?2
            "#,
            vec![
                Value::Text(trimmed.to_string()),
                Value::Integer(limit as i64),
            ],
            &mut symbols,
            &mut seen,
            limit,
        )?;
        if !symbols.is_empty() {
            return Ok(symbols);
        }

        let prefix_pattern = format!("{}%", trimmed);
        append_symbol_query_results(
            &conn,
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols
            WHERE name LIKE ?1 OR qualified_name LIKE ?1
            ORDER BY CASE
                WHEN name LIKE ?2 THEN 0
                WHEN qualified_name LIKE ?2 THEN 1
                ELSE 2
            END, length(name), name
            LIMIT ?3
            "#,
            vec![
                Value::Text(prefix_pattern.clone()),
                Value::Text(prefix_pattern),
                Value::Integer(limit as i64),
            ],
            &mut symbols,
            &mut seen,
            limit,
        )?;
        if symbols.len() >= limit {
            return Ok(symbols);
        }

        let mut search_patterns = Vec::new();
        search_patterns.push(format!("%{}%", trimmed));
        for term in symbol_search_terms(trimmed) {
            let candidate = format!("%{}%", term);
            if !search_patterns
                .iter()
                .any(|existing| existing == &candidate)
            {
                search_patterns.push(candidate);
            }
        }

        let where_clause = std::iter::repeat(
            "(name LIKE ? OR qualified_name LIKE ? OR signature LIKE ? OR docstring LIKE ?)",
        )
        .take(search_patterns.len())
        .collect::<Vec<_>>()
        .join(" OR ");
        let sql = format!(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols 
            WHERE {}
            ORDER BY CASE
                WHEN lower(qualified_name) = lower(?) THEN 0
                WHEN lower(name) = lower(?) THEN 1
                WHEN lower(qualified_name) LIKE lower(?) THEN 2
                WHEN lower(name) LIKE lower(?) THEN 3
                WHEN lower(signature) LIKE lower(?) THEN 4
                WHEN lower(docstring) LIKE lower(?) THEN 5
                ELSE 6
            END, length(name), name
            LIMIT ?
            "#,
            where_clause
        );

        let mut values = Vec::with_capacity(search_patterns.len() * 4 + 7);
        for search_pattern in search_patterns {
            values.push(Value::Text(search_pattern.clone()));
            values.push(Value::Text(search_pattern.clone()));
            values.push(Value::Text(search_pattern.clone()));
            values.push(Value::Text(search_pattern));
        }
        values.push(Value::Text(trimmed.to_string()));
        values.push(Value::Text(trimmed.to_string()));
        values.push(Value::Text(format!("{}%", trimmed)));
        values.push(Value::Text(format!("{}%", trimmed)));
        values.push(Value::Text(format!("%{}%", trimmed)));
        values.push(Value::Text(format!("%{}%", trimmed)));
        values.push(Value::Integer(limit as i64));

        append_symbol_query_results(&conn, &sql, values, &mut symbols, &mut seen, limit)?;
        Ok(symbols)
    }

    /// Get symbol at a specific position in a file
    pub fn get_symbol_at(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<Symbol>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols 
            WHERE file_path = ?1
              AND start_line <= ?2 AND end_line >= ?2
              AND (start_line < ?2 OR start_char <= ?3)
              AND (end_line > ?2 OR end_char >= ?3)
            ORDER BY (end_line - start_line), (end_char - start_char)
            LIMIT 1
            "#,
        )?;

        let mut rows = stmt.query(params![file_path, line, character])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_symbol(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get symbols by type
    pub fn get_symbols_by_type(
        &self,
        symbol_type: SymbolType,
        limit: usize,
    ) -> Result<Vec<Symbol>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols 
            WHERE symbol_type = ?1
            ORDER BY name
            LIMIT ?2
            "#,
        )?;

        let symbols = stmt
            .query_map(params![symbol_type.to_string(), limit as i64], |row| {
                row_to_symbol(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Delete all symbols for a file
    pub fn delete_file_symbols(&self, file_path: &str) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM symbol_relationships WHERE source_file_path = ?1",
            params![file_path],
        )?;
        conn.execute(
            "DELETE FROM semantic_anchors WHERE file_path = ?1",
            params![file_path],
        )?;
        let count = conn.execute(
            "DELETE FROM symbols WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(count)
    }

    /// Delete indexing metadata for a file
    pub fn delete_indexed_file(&self, file_path: &str) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let count = conn.execute(
            "DELETE FROM indexed_files WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(count)
    }

    /// Delete generated index data only.
    ///
    /// This must not include user-authored metadata tables. If user-authored
    /// data is ever stored in this database, keep it outside
    /// `GENERATED_INDEX_CLEAR_STATEMENTS` or explicitly preserve and restore it
    /// around destructive rebuilds.
    pub fn clear_generated_index_data(&self) -> Result<(), SymbolStoreError> {
        debug_assert_eq!(
            GENERATED_INDEX_CLEAR_STATEMENTS.len(),
            GENERATED_INDEX_TABLES.len()
        );
        let conn = self.conn.lock().unwrap();
        for statement in GENERATED_INDEX_CLEAR_STATEMENTS {
            conn.execute(statement, [])?;
        }
        Ok(())
    }

    /// Delete generated index data.
    pub fn clear(&self) -> Result<(), SymbolStoreError> {
        self.clear_generated_index_data()
    }

    /// Get total symbol count
    pub fn count(&self) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn list_indexed_files(
        &self,
        limit: usize,
    ) -> Result<Vec<IndexedFileRecord>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version
            FROM indexed_files
            ORDER BY symbol_count DESC, file_path ASC
            LIMIT ?1
            "#,
        )?;

        let records = stmt
            .query_map(params![limit as i64], |row| {
                indexed_file_record_from_row(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    pub fn indexed_file_record(
        &self,
        file_path: &str,
    ) -> Result<Option<IndexedFileRecord>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let record = conn
            .query_row(
                r#"
                SELECT file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version
                FROM indexed_files
                WHERE file_path = ?1
                "#,
                params![file_path],
                indexed_file_record_from_row,
            )
            .optional()?;

        Ok(record)
    }

    pub fn list_all_indexed_files(&self) -> Result<Vec<IndexedFileRecord>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version
            FROM indexed_files
            ORDER BY file_path ASC
            "#,
        )?;

        let records = stmt
            .query_map([], |row| indexed_file_record_from_row(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    /// Get count of indexed files
    pub fn file_count(&self) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(DISTINCT file_path) FROM symbols", [], |row| {
                row.get(0)
            })?;
        Ok(count as usize)
    }

    /// Record file as indexed
    pub fn mark_file_indexed(
        &self,
        file_path: &str,
        file_hash: &str,
        symbol_count: usize,
    ) -> Result<(), SymbolStoreError> {
        self.mark_file_indexed_with_metadata(file_path, file_hash, symbol_count, None, None, None)
    }

    pub fn mark_file_indexed_with_metadata(
        &self,
        file_path: &str,
        file_hash: &str,
        symbol_count: usize,
        file_size: Option<u64>,
        line_count: Option<usize>,
        modified_at: Option<i64>,
    ) -> Result<(), SymbolStoreError> {
        self.mark_file_indexed_with_metadata_and_extractor_version(
            file_path,
            file_hash,
            symbol_count,
            file_size,
            line_count,
            modified_at,
            None,
        )
    }

    pub fn mark_file_indexed_with_metadata_and_extractor_version(
        &self,
        file_path: &str,
        file_hash: &str,
        symbol_count: usize,
        file_size: Option<u64>,
        line_count: Option<usize>,
        modified_at: Option<i64>,
        extractor_version: Option<u32>,
    ) -> Result<(), SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            r#"
            INSERT OR REPLACE INTO indexed_files
            (file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                file_path,
                file_hash,
                now,
                symbol_count as i64,
                file_size.map(|size| size as i64),
                line_count.map(|count| count as i64),
                modified_at,
                extractor_version.map(i64::from)
            ],
        )?;
        Ok(())
    }

    /// Check if file needs reindexing
    pub fn needs_reindex(
        &self,
        file_path: &str,
        file_hash: &str,
    ) -> Result<bool, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let result: Option<String> = conn
            .query_row(
                "SELECT file_hash FROM indexed_files WHERE file_path = ?1",
                params![file_path],
                |row| row.get(0),
            )
            .ok();

        match result {
            Some(stored_hash) => Ok(stored_hash != file_hash),
            None => Ok(true), // Not indexed yet
        }
    }

    pub fn needs_reindex_for_metadata(
        &self,
        file_path: &str,
        file_size: u64,
        modified_at: i64,
    ) -> Result<Option<bool>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let result: Option<(Option<i64>, Option<i64>)> = conn
            .query_row(
                "SELECT file_size, modified_at FROM indexed_files WHERE file_path = ?1",
                params![file_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        match result {
            Some((Some(stored_size), Some(stored_modified_at))) => Ok(Some(
                stored_size != file_size as i64 || stored_modified_at != modified_at,
            )),
            Some(_) => Ok(None),
            None => Ok(Some(true)),
        }
    }

    pub fn relationship_integrity_stats(
        &self,
    ) -> Result<RelationshipIntegrityStats, SymbolStoreError> {
        self.relationship_integrity_stats_for_scope(None)
    }

    pub fn relationship_integrity_stats_for_scope(
        &self,
        scope_path: Option<&str>,
    ) -> Result<RelationshipIntegrityStats, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let scope_like = scope_path.map(|path| format!("{path}/%"));
        let scoped = scope_path.zip(scope_like.as_deref());

        let total_relationships = query_scoped_count(
            &conn,
            "SELECT COUNT(*) FROM symbol_relationships",
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships
            WHERE source_file_path = ?1 OR source_file_path LIKE ?2
            "#,
            scoped,
        )?;
        let resolved_relationships = query_scoped_count(
            &conn,
            "SELECT COUNT(*) FROM symbol_relationships WHERE target_symbol_id IS NOT NULL",
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships
            WHERE target_symbol_id IS NOT NULL
              AND (source_file_path = ?1 OR source_file_path LIKE ?2)
            "#,
            scoped,
        )?;
        let unresolved_symbol_relationships = query_scoped_count(
            &conn,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships
            WHERE target_symbol_id IS NULL AND relationship_type <> 'import'
            "#,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships
            WHERE target_symbol_id IS NULL
              AND relationship_type <> 'import'
              AND (source_file_path = ?1 OR source_file_path LIKE ?2)
            "#,
            scoped,
        )?;
        let missing_source_symbols = query_scoped_count(
            &conn,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships r
            LEFT JOIN symbols s ON s.id = r.source_symbol_id
            WHERE s.id IS NULL
            "#,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships r
            LEFT JOIN symbols s ON s.id = r.source_symbol_id
            WHERE s.id IS NULL
              AND (r.source_file_path = ?1 OR r.source_file_path LIKE ?2)
            "#,
            scoped,
        )?;
        let missing_target_symbols = query_scoped_count(
            &conn,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships r
            LEFT JOIN symbols s ON s.id = r.target_symbol_id
            WHERE r.target_symbol_id IS NOT NULL AND s.id IS NULL
            "#,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships r
            LEFT JOIN symbols s ON s.id = r.target_symbol_id
            WHERE r.target_symbol_id IS NOT NULL
              AND s.id IS NULL
              AND (r.source_file_path = ?1 OR r.source_file_path LIKE ?2)
            "#,
            scoped,
        )?;
        let by_type = {
            match scoped {
                Some((scope, like)) => {
                    let mut stmt = conn.prepare(
                        r#"
                        SELECT relationship_type,
                               COUNT(*),
                               SUM(CASE WHEN target_symbol_id IS NOT NULL THEN 1 ELSE 0 END),
                               SUM(CASE WHEN target_symbol_id IS NULL AND relationship_type <> 'import' THEN 1 ELSE 0 END)
                        FROM symbol_relationships
                        WHERE source_file_path = ?1 OR source_file_path LIKE ?2
                        GROUP BY relationship_type
                        ORDER BY COUNT(*) DESC, relationship_type ASC
                        "#,
                    )?;
                    let rows = stmt.query_map(params![scope, like], |row| {
                        Ok(RelationshipTypeStats {
                            relationship_type: row.get(0)?,
                            total_relationships: row.get::<_, i64>(1)? as usize,
                            resolved_relationships: row.get::<_, i64>(2)? as usize,
                            unresolved_symbol_relationships: row.get::<_, i64>(3)? as usize,
                        })
                    })?;
                    rows.collect::<Result<Vec<_>, _>>()?
                }
                None => {
                    let mut stmt = conn.prepare(
                        r#"
                        SELECT relationship_type,
                               COUNT(*),
                               SUM(CASE WHEN target_symbol_id IS NOT NULL THEN 1 ELSE 0 END),
                               SUM(CASE WHEN target_symbol_id IS NULL AND relationship_type <> 'import' THEN 1 ELSE 0 END)
                        FROM symbol_relationships
                        GROUP BY relationship_type
                        ORDER BY COUNT(*) DESC, relationship_type ASC
                        "#,
                    )?;
                    let rows = stmt.query_map([], |row| {
                        Ok(RelationshipTypeStats {
                            relationship_type: row.get(0)?,
                            total_relationships: row.get::<_, i64>(1)? as usize,
                            resolved_relationships: row.get::<_, i64>(2)? as usize,
                            unresolved_symbol_relationships: row.get::<_, i64>(3)? as usize,
                        })
                    })?;
                    rows.collect::<Result<Vec<_>, _>>()?
                }
            }
        };
        let top_unresolved_targets = {
            match scoped {
                Some((scope, like)) => {
                    let mut stmt = conn.prepare(
                        r#"
                        SELECT relationship_type, target_name, COUNT(*), MIN(source_file_path)
                        FROM symbol_relationships
                        WHERE target_symbol_id IS NULL
                          AND relationship_type <> 'import'
                          AND (source_file_path = ?1 OR source_file_path LIKE ?2)
                        GROUP BY relationship_type, target_name
                        ORDER BY COUNT(*) DESC, relationship_type ASC, target_name ASC
                        LIMIT 12
                        "#,
                    )?;
                    let rows = stmt.query_map(params![scope, like], |row| {
                        Ok(UnresolvedRelationshipTarget {
                            relationship_type: row.get(0)?,
                            target_name: row.get(1)?,
                            count: row.get::<_, i64>(2)? as usize,
                            example_source_file: row.get(3)?,
                        })
                    })?;
                    rows.collect::<Result<Vec<_>, _>>()?
                }
                None => {
                    let mut stmt = conn.prepare(
                        r#"
                        SELECT relationship_type, target_name, COUNT(*), MIN(source_file_path)
                        FROM symbol_relationships
                        WHERE target_symbol_id IS NULL AND relationship_type <> 'import'
                        GROUP BY relationship_type, target_name
                        ORDER BY COUNT(*) DESC, relationship_type ASC, target_name ASC
                        LIMIT 12
                        "#,
                    )?;
                    let rows = stmt.query_map([], |row| {
                        Ok(UnresolvedRelationshipTarget {
                            relationship_type: row.get(0)?,
                            target_name: row.get(1)?,
                            count: row.get::<_, i64>(2)? as usize,
                            example_source_file: row.get(3)?,
                        })
                    })?;
                    rows.collect::<Result<Vec<_>, _>>()?
                }
            }
        };

        Ok(RelationshipIntegrityStats {
            total_relationships: total_relationships as usize,
            resolved_relationships: resolved_relationships as usize,
            unresolved_symbol_relationships: unresolved_symbol_relationships as usize,
            missing_source_symbols: missing_source_symbols as usize,
            missing_target_symbols: missing_target_symbols as usize,
            by_type,
            top_unresolved_targets,
        })
    }

    /// Per-language × per-kind matrix of symbol and relationship counts, with an
    /// explicit `0` row back-filled for every `SymbolType` / `SymbolRelationshipType`
    /// that did not appear for a language, plus the per-language "% of
    /// Function/Method symbols carrying a non-null signature".
    ///
    /// Read-only diagnostic; does not change any extraction behavior.
    //
    // NOTE: language derived from file path until M2.3 adds a real `language` column; re-base then.
    pub fn coverage_histogram(&self) -> Result<CoverageHistogram, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();

        // language -> (symbol_type string -> count)
        let mut symbol_counts: BTreeMap<String, HashMap<String, u64>> = BTreeMap::new();
        // language -> (function+method total, function+method with non-empty signature)
        let mut signature_totals: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        // language -> (relationship_type string -> count)
        let mut relationship_counts: BTreeMap<String, HashMap<String, u64>> = BTreeMap::new();

        let function_key = SymbolType::Function.to_string();
        let method_key = SymbolType::Method.to_string();

        {
            let mut stmt = conn.prepare("SELECT file_path, symbol_type, signature FROM symbols")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let file_path: String = row.get(0)?;
                let symbol_type: String = row.get(1)?;
                let signature: Option<String> = row.get(2)?;
                let language = coverage_language_label(&file_path);

                *symbol_counts
                    .entry(language.clone())
                    .or_default()
                    .entry(symbol_type.clone())
                    .or_insert(0) += 1;

                if symbol_type == function_key || symbol_type == method_key {
                    let entry = signature_totals.entry(language).or_insert((0, 0));
                    entry.0 += 1;
                    if signature.is_some_and(|sig| !sig.trim().is_empty()) {
                        entry.1 += 1;
                    }
                }
            }
        }

        {
            let mut stmt =
                conn.prepare("SELECT source_file_path, relationship_type FROM symbol_relationships")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let file_path: String = row.get(0)?;
                let relationship_type: String = row.get(1)?;
                let language = coverage_language_label(&file_path);

                *relationship_counts
                    .entry(language)
                    .or_default()
                    .entry(relationship_type)
                    .or_insert(0) += 1;
            }
        }

        // Union of every language that produced a symbol or a relationship.
        let mut languages: BTreeSet<String> = BTreeSet::new();
        languages.extend(symbol_counts.keys().cloned());
        languages.extend(relationship_counts.keys().cloned());

        let per_language = languages
            .into_iter()
            .map(|language| {
                let observed_symbols = symbol_counts.get(&language);
                // Back-fill an explicit 0 for every SymbolType variant that is absent.
                let symbol_counts = ALL_SYMBOL_TYPES
                    .iter()
                    .map(|kind| {
                        let key = kind.to_string();
                        let count = observed_symbols
                            .and_then(|counts| counts.get(&key))
                            .copied()
                            .unwrap_or(0);
                        (key, count)
                    })
                    .collect();

                let observed_relationships = relationship_counts.get(&language);
                // Back-fill an explicit 0 for every relationship variant that is absent.
                let relationship_counts = ALL_RELATIONSHIP_TYPES
                    .iter()
                    .map(|relationship_type| {
                        let key = relationship_type.to_string();
                        let count = observed_relationships
                            .and_then(|counts| counts.get(&key))
                            .copied()
                            .unwrap_or(0);
                        (key, count)
                    })
                    .collect();

                let function_method_signature_pct = match signature_totals.get(&language) {
                    Some((total, with_signature)) if *total > 0 => {
                        *with_signature as f64 / *total as f64
                    }
                    _ => 0.0,
                };

                LanguageCoverage {
                    language,
                    symbol_counts,
                    relationship_counts,
                    function_method_signature_pct,
                }
            })
            .collect();

        Ok(CoverageHistogram { per_language })
    }

    pub fn symbol_type_counts(&self) -> Result<Vec<(String, usize)>, SymbolStoreError> {
        self.symbol_type_counts_for_scope(None)
    }

    pub fn symbol_type_counts_for_scope(
        &self,
        scope_path: Option<&str>,
    ) -> Result<Vec<(String, usize)>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        match scope_path {
            Some(scope) => {
                let like = format!("{scope}/%");
                let mut stmt = conn.prepare(
                    r#"
                    SELECT symbol_type, COUNT(*)
                    FROM symbols
                    WHERE file_path = ?1 OR file_path LIKE ?2
                    GROUP BY symbol_type
                    ORDER BY COUNT(*) DESC, symbol_type ASC
                    "#,
                )?;
                let rows = stmt
                    .query_map(params![scope, like], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
            None => {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT symbol_type, COUNT(*)
                    FROM symbols
                    GROUP BY symbol_type
                    ORDER BY COUNT(*) DESC, symbol_type ASC
                    "#,
                )?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
        }
    }

    pub fn semantic_anchor_kind_counts(&self) -> Result<Vec<(String, usize)>, SymbolStoreError> {
        self.semantic_anchor_kind_counts_for_scope(None)
    }

    pub fn semantic_anchor_kind_counts_for_scope(
        &self,
        scope_path: Option<&str>,
    ) -> Result<Vec<(String, usize)>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        match scope_path {
            Some(scope) => {
                let like = format!("{scope}/%");
                let mut stmt = conn.prepare(
                    r#"
                    SELECT kind, COUNT(*)
                    FROM semantic_anchors
                    WHERE file_path = ?1 OR file_path LIKE ?2
                    GROUP BY kind
                    ORDER BY COUNT(*) DESC, kind ASC
                    "#,
                )?;
                let rows = stmt
                    .query_map(params![scope, like], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
            None => {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT kind, COUNT(*)
                    FROM semantic_anchors
                    GROUP BY kind
                    ORDER BY COUNT(*) DESC, kind ASC
                    "#,
                )?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
        }
    }

    fn hydrate_symbol_references(
        &self,
        rows: Vec<(Symbol, String, String, Option<String>, u32)>,
    ) -> Result<Vec<SymbolReference>, SymbolStoreError> {
        // M2.4 — hydrate target symbols with ONE batched lookup keyed on the
        // distinct target ids, instead of N+1 `get_symbol` round-trips (one per
        // edge). The map below is the JOIN, resolved in a single query.
        let target_symbols = self.get_symbols_by_ids(
            rows.iter()
                .filter_map(|(_, _, _, target_symbol_id, _)| target_symbol_id.as_deref()),
        )?;

        let mut references = Vec::with_capacity(rows.len());

        for (source_symbol, relationship_type, target_name, target_symbol_id, line) in rows {
            let relationship_type = relationship_type
                .parse::<SymbolRelationshipType>()
                .map_err(|error| {
                    SymbolStoreError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        error,
                    ))
                })?;
            let target_symbol = target_symbol_id
                .as_deref()
                .and_then(|id| target_symbols.get(id).cloned());

            references.push(SymbolReference {
                source_symbol,
                relationship_type,
                target_name,
                target_symbol_id,
                target_symbol,
                line,
            });
        }

        Ok(references)
    }

    /// Fetch the given symbol ids in a single query, returning an `id -> Symbol`
    /// map. Ids are de-duplicated and chunked under SQLite's bound-parameter
    /// ceiling so one logical lookup replaces N `get_symbol` calls.
    fn get_symbols_by_ids<'a, I>(
        &self,
        ids: I,
    ) -> Result<HashMap<String, Symbol>, SymbolStoreError>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let unique_ids: Vec<String> = ids
            .into_iter()
            .collect::<BTreeSet<&str>>()
            .into_iter()
            .map(str::to_string)
            .collect();
        if unique_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let conn = self.conn.lock().unwrap();
        let mut symbols = HashMap::with_capacity(unique_ids.len());
        // Stay well under SQLite's default 999 bound-parameter limit per query.
        for chunk in unique_ids.chunks(900) {
            let placeholders = std::iter::repeat("?")
                .take(chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let mut stmt = conn.prepare(&format!(
                r#"
                SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                       end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
                FROM symbols WHERE id IN ({placeholders})
                "#,
            ))?;
            let chunk_symbols = stmt
                .query_map(params_from_iter(chunk.iter()), row_to_symbol)?
                .collect::<Result<Vec<_>, _>>()?;
            for symbol in chunk_symbols {
                symbols.insert(symbol.id.clone(), symbol);
            }
        }

        Ok(symbols)
    }
}

fn indexed_file_record_from_row(row: &rusqlite::Row) -> rusqlite::Result<IndexedFileRecord> {
    Ok(IndexedFileRecord {
        file_path: row.get::<_, String>(0)?,
        file_hash: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        indexed_at: row.get::<_, i64>(2)?,
        symbol_count: row.get::<_, i64>(3)? as usize,
        file_size: row
            .get::<_, Option<i64>>(4)?
            .and_then(|size| u64::try_from(size).ok()),
        line_count: row
            .get::<_, Option<i64>>(5)?
            .and_then(|count| usize::try_from(count).ok()),
        modified_at: row.get::<_, Option<i64>>(6)?,
        extractor_version: row
            .get::<_, Option<i64>>(7)?
            .and_then(|version| u32::try_from(version).ok()),
    })
}

/// Convert a database row to a Symbol
fn row_to_semantic_anchor(row: &rusqlite::Row) -> rusqlite::Result<SemanticAnchor> {
    Ok(SemanticAnchor {
        id: row.get(0)?,
        file_path: row.get(1)?,
        kind: row.get(2)?,
        value: row.get(3)?,
        line: row.get::<_, i64>(4)? as u32,
        character: row.get::<_, i64>(5)? as u32,
        preview: row.get(6)?,
        confidence: row.get::<_, f64>(7)? as f32,
    })
}

fn semantic_anchor_score(anchor: &SemanticAnchor, query: &str) -> f32 {
    let value = anchor.value.to_lowercase();
    let query = query.to_lowercase();
    if value == query {
        1.0
    } else if value.starts_with(&query) {
        0.9 * anchor.confidence
    } else if value.contains(&query) {
        0.75 * anchor.confidence
    } else {
        0.45 * anchor.confidence
    }
}

fn row_to_symbol(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
    use crate::tree_sitter::{Position, Range};

    let symbol_type_str: String = row.get(3)?;
    let symbol_type = symbol_type_str
        .parse::<SymbolType>()
        .unwrap_or(SymbolType::Function);

    Ok(Symbol {
        id: row.get(0)?,
        name: row.get(1)?,
        qualified_name: row.get(2)?,
        symbol_type,
        file_path: row.get(4)?,
        range: Range {
            start: Position {
                line: row.get::<_, i32>(5)? as u32,
                character: row.get::<_, i32>(6)? as u32,
            },
            end: Position {
                line: row.get::<_, i32>(7)? as u32,
                character: row.get::<_, i32>(8)? as u32,
            },
        },
        byte_offset: row.get::<_, i64>(9)? as usize,
        byte_length: row.get::<_, i64>(10)? as usize,
        parent_id: row.get(11)?,
        docstring: row.get(12)?,
        signature: row.get(13)?,
        content_hash: row.get(14)?,
    })
}

fn append_symbol_query_results(
    conn: &Connection,
    sql: &str,
    values: Vec<Value>,
    symbols: &mut Vec<Symbol>,
    seen: &mut HashSet<String>,
    limit: usize,
) -> Result<(), SymbolStoreError> {
    if symbols.len() >= limit {
        return Ok(());
    }

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params_from_iter(values), |row| row_to_symbol(row))?
        .collect::<Result<Vec<_>, _>>()?;

    for symbol in rows {
        if seen.insert(symbol.id.clone()) {
            symbols.push(symbol);
            if symbols.len() >= limit {
                break;
            }
        }
    }

    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table_name: &str,
    column_name: &str,
    definition: &str,
) -> Result<(), SymbolStoreError> {
    let pragma = format!("PRAGMA table_info({})", table_name);
    let mut stmt = conn.prepare(&pragma)?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;

    if !columns.iter().any(|existing| existing == column_name) {
        let alter = format!(
            "ALTER TABLE {} ADD COLUMN {} {}",
            table_name, column_name, definition
        );
        conn.execute(&alter, [])?;
    }

    Ok(())
}

/// Latest schema version this binary knows how to produce.
///
/// Equal to `MIGRATIONS.len()`. A freshly-migrated database ends with
/// `PRAGMA user_version` set to this value.
// Currently only asserted by the migration tests; the next column-adding
// milestone (M2.4) will read it from production code.
#[cfg_attr(not(test), allow(dead_code))]
const LATEST_SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// Ordered, **append-only** schema migrations keyed on `PRAGMA user_version`.
///
/// `MIGRATIONS[n]` migrates the database from `user_version = n` to
/// `user_version = n + 1` (so `MIGRATIONS[0]` takes v0 → v1). NEVER edit,
/// reorder, or remove a shipped step — only append new ones. This framework
/// owns NEW schema going forward; the legacy `ensure_column` probes in
/// `create_schema` keep owning the CURRENT columns (converting them is riskier
/// than letting the two coexist), so the two intentionally overlap with no
/// conflict.
///
/// N7 — no column without an in-phase consumer: v1 adds ONLY the three
/// `symbol_relationships` columns that M2.4 (set-based relationship back-fill)
/// will consume to tag each back-filled edge with how it was resolved + a
/// confidence, so wrong/ambiguous back-fills stay auditable.
///
/// DEFERRED per N7 (do NOT add here): the other columns from the plan's M2.3
/// list — `language`, `is_lexical`, `return_type`, `visibility`, `modifiers` on
/// `symbols`, and the `routes` table — are deferred to their consuming
/// milestones (M0.2 re-base / M4.x). They land as new appended migration steps
/// when (and only when) their consumer ships.
const MIGRATIONS: &[fn(&Connection) -> Result<(), SymbolStoreError>] =
    &[migration_v1_relationship_resolution_columns];

/// Apply every pending migration step, advancing `PRAGMA user_version`.
///
/// Reads the current `user_version`, then runs each not-yet-applied step from
/// `MIGRATIONS` inside its own transaction, bumping `user_version` in the same
/// transaction so the version can never run ahead of the schema. Idempotent:
/// once `user_version == LATEST_SCHEMA_VERSION` this is a no-op, so running it
/// repeatedly (e.g. on every `SymbolStore::new`) is safe.
fn run_migrations(conn: &Connection) -> Result<(), SymbolStoreError> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    while (version as usize) < MIGRATIONS.len() {
        let step = MIGRATIONS[version as usize];
        // `unchecked_transaction` works on `&Connection`; on any early return
        // (the `?` below) the guard drops and rolls the step back, so a failed
        // migration never leaves `user_version` ahead of the real schema.
        let tx = conn.unchecked_transaction()?;
        step(&tx)?;
        let next = version + 1;
        // `user_version` is an integer we fully control and cannot be bound as a
        // parameter, so format it directly; it is transactional with the DDL above.
        tx.execute_batch(&format!("PRAGMA user_version = {next}"))?;
        tx.commit()?;
        version = next;
    }

    Ok(())
}

/// Migration v1: add the relationship-resolution audit columns M2.4 consumes.
///
/// `ensure_column` pragma-checks `table_info` before issuing `ALTER TABLE … ADD
/// COLUMN`, which makes each add idempotent: re-running this migration, or
/// running it on a DB where a prior ad-hoc `ensure_column` already added one of
/// these columns, is a safe no-op (no "duplicate column name" error).
fn migration_v1_relationship_resolution_columns(
    conn: &Connection,
) -> Result<(), SymbolStoreError> {
    ensure_column(conn, "symbol_relationships", "resolution_strategy", "TEXT")?;
    ensure_column(conn, "symbol_relationships", "confidence", "REAL")?;
    ensure_column(conn, "symbol_relationships", "metadata_json", "TEXT")?;
    Ok(())
}

fn symbol_search_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .map(str::trim)
        .filter(|term| term.len() >= 2)
        .map(ToString::to_string)
        .collect()
}

fn symbol_search_fts_query(query: &str) -> String {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for term in symbol_search_terms(query) {
        let term = term.to_lowercase();
        if seen.insert(term.clone()) {
            terms.push(format!("{}*", term));
        }
    }
    terms.join(" OR ")
}

/// Error type for symbol store operations
#[derive(Debug)]
pub enum SymbolStoreError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for SymbolStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolStoreError::Sqlite(e) => write!(f, "SQLite error: {}", e),
            SymbolStoreError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for SymbolStoreError {}

impl From<rusqlite::Error> for SymbolStoreError {
    fn from(err: rusqlite::Error) -> Self {
        SymbolStoreError::Sqlite(err)
    }
}

impl From<std::io::Error> for SymbolStoreError {
    fn from(err: std::io::Error) -> Self {
        SymbolStoreError::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter::{Position, Range};

    fn create_test_symbol(name: &str, file_path: &str) -> Symbol {
        Symbol {
            id: format!("{}::{}#function", file_path, name),
            name: name.to_string(),
            qualified_name: name.to_string(),
            symbol_type: SymbolType::Function,
            file_path: file_path.to_string(),
            range: Range {
                start: Position {
                    line: 1,
                    character: 0,
                },
                end: Position {
                    line: 10,
                    character: 0,
                },
            },
            byte_offset: 0,
            byte_length: 0,
            parent_id: None,
            docstring: Some("Test function".to_string()),
            signature: Some("(param: string): void".to_string()),
            content_hash: "hash".to_string(),
        }
    }

    fn create_test_relationship(
        source_symbol_id: &str,
        file_path: &str,
        target_name: &str,
    ) -> SymbolRelationship {
        SymbolRelationship {
            source_symbol_id: source_symbol_id.to_string(),
            source_file_path: file_path.to_string(),
            target_name: target_name.to_string(),
            target_symbol_id: None,
            relationship_type: SymbolRelationshipType::Call,
            line: 3,
        }
    }

    #[test]
    fn test_create_store() {
        let store = SymbolStore::in_memory().unwrap();
        assert_eq!(store.count().unwrap(), 0);
    }

    /// M2.4 correctness gate: the set-based back-fill resolves a relationship
    /// ONLY when its `target_name` matches exactly one symbol globally. A name
    /// shared by symbols in different files is ambiguous and must stay NULL — a
    /// wrong `target_symbol_id` silently corrupts the knowledge graph.
    #[test]
    fn backfill_resolves_unique_targets_but_never_ambiguous_names() {
        let store = SymbolStore::in_memory().unwrap();

        // "Shared" exists in TWO different files (ambiguous); "OnlyOne" exists
        // in exactly one file (unambiguous).
        store
            .upsert_symbols(&[
                create_test_symbol("Shared", "a.ts"),
                create_test_symbol("Shared", "b.ts"),
                create_test_symbol("OnlyOne", "c.ts"),
                create_test_symbol("caller", "caller.ts"),
            ])
            .unwrap();

        let caller_id = "caller.ts::caller#function";
        store
            .replace_relationships_for_file(
                "caller.ts",
                &[
                    create_test_relationship(caller_id, "caller.ts", "Shared"),
                    create_test_relationship(caller_id, "caller.ts", "OnlyOne"),
                ],
            )
            .unwrap();

        // Both edges start NULL; exactly one (the unique "OnlyOne") resolves.
        let resolved = store.backfill_unresolved_relationship_targets().unwrap();
        assert_eq!(resolved, 1);

        let conn = store.conn.lock().unwrap();

        // Ambiguous name → still NULL, untagged (no wrong back-fill).
        let (shared_target, shared_strategy): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT target_symbol_id, resolution_strategy
                 FROM symbol_relationships WHERE target_name = 'Shared'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(
            shared_target.is_none(),
            "ambiguous name must NOT be back-filled, got {shared_target:?}"
        );
        assert!(shared_strategy.is_none());

        // Unique name → resolved to the single matching symbol and tagged.
        let (unique_target, unique_strategy, unique_confidence): (
            Option<String>,
            Option<String>,
            Option<f64>,
        ) = conn
            .query_row(
                "SELECT target_symbol_id, resolution_strategy, confidence
                 FROM symbol_relationships WHERE target_name = 'OnlyOne'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(unique_target.as_deref(), Some("c.ts::OnlyOne#function"));
        assert_eq!(unique_strategy.as_deref(), Some("global_unique"));
        assert_eq!(unique_confidence, Some(0.5));
    }

    /// The back-fill never overwrites an already-resolved (non-NULL) target and
    /// is idempotent on a second run.
    #[test]
    fn backfill_preserves_existing_targets_and_is_idempotent() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                create_test_symbol("OnlyOne", "c.ts"),
                create_test_symbol("decoy", "d.ts"),
                create_test_symbol("caller", "caller.ts"),
            ])
            .unwrap();

        let caller_id = "caller.ts::caller#function";
        // Pre-resolved to a deliberately "wrong" id: the back-fill must leave it.
        let mut pinned = create_test_relationship(caller_id, "caller.ts", "OnlyOne");
        pinned.target_symbol_id = Some("d.ts::decoy#function".to_string());
        store
            .replace_relationships_for_file("caller.ts", &[pinned])
            .unwrap();

        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 0);
        // Second run is also a no-op.
        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 0);

        let conn = store.conn.lock().unwrap();
        let target: Option<String> = conn
            .query_row(
                "SELECT target_symbol_id FROM symbol_relationships WHERE target_name = 'OnlyOne'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(target.as_deref(), Some("d.ts::decoy#function"));
    }

    fn coverage_symbol(
        id: &str,
        name: &str,
        file_path: &str,
        symbol_type: SymbolType,
        signature: Option<&str>,
    ) -> Symbol {
        Symbol {
            id: id.to_string(),
            name: name.to_string(),
            qualified_name: name.to_string(),
            symbol_type,
            file_path: file_path.to_string(),
            range: Range {
                start: Position {
                    line: 1,
                    character: 0,
                },
                end: Position {
                    line: 2,
                    character: 0,
                },
            },
            byte_offset: 0,
            byte_length: 0,
            parent_id: None,
            docstring: None,
            signature: signature.map(|s| s.to_string()),
            content_hash: "hash".to_string(),
        }
    }

    fn coverage_count(counts: &[(String, u64)], key: &str) -> u64 {
        counts
            .iter()
            .find(|(label, _)| label == key)
            .map(|(_, count)| *count)
            // `.expect` proves the zero rows are *present* (back-filled), not just defaulted.
            .unwrap_or_else(|| panic!("expected a back-filled row for `{key}`"))
    }

    #[test]
    fn test_coverage_histogram_backfills_zeros() {
        let store = SymbolStore::in_memory().unwrap();

        // Rust: 3 functions (2 with signatures), 1 method (with signature), 1 struct.
        // function+method total = 4, with non-empty signature = 3 -> pct = 0.75.
        let rust_file = "src/lib.rs";
        store
            .upsert_symbols(&[
                coverage_symbol(
                    "r1",
                    "func_a",
                    rust_file,
                    SymbolType::Function,
                    Some("(x: i32) -> i32"),
                ),
                coverage_symbol(
                    "r2",
                    "func_b",
                    rust_file,
                    SymbolType::Function,
                    Some("() -> ()"),
                ),
                coverage_symbol("r3", "func_c", rust_file, SymbolType::Function, None),
                coverage_symbol(
                    "r4",
                    "method_a",
                    rust_file,
                    SymbolType::Method,
                    Some("(&self)"),
                ),
                coverage_symbol("r5", "StructX", rust_file, SymbolType::Struct, None),
            ])
            .unwrap();

        // One Rust relationship of a single kind (implements); all others must be 0.
        store
            .replace_relationships_for_file(
                rust_file,
                &[SymbolRelationship {
                    source_symbol_id: "r5".to_string(),
                    source_file_path: rust_file.to_string(),
                    target_name: "Renderable".to_string(),
                    target_symbol_id: None,
                    relationship_type: SymbolRelationshipType::Implements,
                    line: 1,
                }],
            )
            .unwrap();

        // TypeScript: a single Class, no functions/methods (pct must be 0.0).
        let ts_file = "src/app.ts";
        store
            .upsert_symbols(&[coverage_symbol(
                "t1",
                "AppService",
                ts_file,
                SymbolType::Class,
                None,
            )])
            .unwrap();

        let histogram = store.coverage_histogram().unwrap();
        assert_eq!(histogram.per_language.len(), 2);

        let rust = histogram
            .per_language
            .iter()
            .find(|coverage| coverage.language == "Rust")
            .expect("Rust language row present");
        let typescript = histogram
            .per_language
            .iter()
            .find(|coverage| coverage.language == "TypeScript")
            .expect("TypeScript language row present");

        // Present kinds counted correctly.
        assert_eq!(coverage_count(&rust.symbol_counts, "function"), 3);
        assert_eq!(coverage_count(&rust.symbol_counts, "method"), 1);
        assert_eq!(coverage_count(&rust.symbol_counts, "struct"), 1);

        // Absent kinds appear as explicit 0 rows (the load-bearing back-fill).
        assert_eq!(coverage_count(&rust.symbol_counts, "class"), 0);
        assert_eq!(coverage_count(&rust.symbol_counts, "trait"), 0);
        assert_eq!(coverage_count(&rust.symbol_counts, "enum"), 0);
        assert_eq!(coverage_count(&rust.symbol_counts, "css_selector"), 0);
        // Every SymbolType variant is represented exactly once.
        assert_eq!(rust.symbol_counts.len(), ALL_SYMBOL_TYPES.len());

        // Relationship back-fill: implements observed, everything else 0.
        assert_eq!(coverage_count(&rust.relationship_counts, "implements"), 1);
        assert_eq!(coverage_count(&rust.relationship_counts, "call"), 0);
        assert_eq!(coverage_count(&rust.relationship_counts, "import"), 0);
        assert_eq!(
            rust.relationship_counts.len(),
            ALL_RELATIONSHIP_TYPES.len()
        );

        // Signature percentage: 3 of 4 Function/Method symbols carry a signature.
        assert!((rust.function_method_signature_pct - 0.75).abs() < 1e-9);

        // Second language shows zeros for the first language's kinds, and vice versa.
        assert_eq!(coverage_count(&typescript.symbol_counts, "class"), 1);
        assert_eq!(coverage_count(&typescript.symbol_counts, "struct"), 0);
        assert_eq!(coverage_count(&typescript.symbol_counts, "function"), 0);
        assert_eq!(typescript.symbol_counts.len(), ALL_SYMBOL_TYPES.len());
        // No Function/Method symbols -> pct defaults to 0.0 (no divide-by-zero).
        assert_eq!(typescript.function_method_signature_pct, 0.0);
    }

    #[test]
    fn test_upsert_and_get() {
        let store = SymbolStore::in_memory().unwrap();
        let symbol = create_test_symbol("authenticate", "auth.ts");

        store.upsert_symbols(&[symbol.clone()]).unwrap();

        let retrieved = store.get_symbol(&symbol.id).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "authenticate");
    }

    #[test]
    fn test_get_symbols_in_file() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("func1", "test.ts");
        let sym2 = create_test_symbol("func2", "test.ts");
        let sym3 = create_test_symbol("other", "other.ts");

        store.upsert_symbols(&[sym1, sym2, sym3]).unwrap();

        let symbols = store.get_symbols_in_file("test.ts").unwrap();
        assert_eq!(symbols.len(), 2);
    }

    #[test]
    fn test_search_by_name_like() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("authenticate", "auth.ts");
        let sym2 = create_test_symbol("authorize", "auth.ts");
        let sym3 = create_test_symbol("validate", "valid.ts");

        store.upsert_symbols(&[sym1, sym2, sym3]).unwrap();

        let results = store.search_by_name_like("auth", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_by_name_like_returns_exact_camel_case_symbol() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("GitCommitMessage", "git.ts");
        let sym2 = create_test_symbol("GitCommitMessageEditor", "editor.ts");

        store.upsert_symbols(&[sym1, sym2]).unwrap();

        let results = store.search_by_name_like("GitCommitMessage", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "GitCommitMessage");
    }

    #[test]
    fn test_search_by_name_like_matches_multi_word_queries() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("authenticateUser", "auth.ts");
        let sym2 = create_test_symbol("UserService", "service.ts");
        let sym3 = create_test_symbol("validateToken", "valid.ts");

        store.upsert_symbols(&[sym1, sym2, sym3]).unwrap();

        let results = store.search_by_name_like("auth user", 10).unwrap();
        assert!(results
            .iter()
            .any(|symbol| symbol.name == "authenticateUser"));
        assert!(results.iter().any(|symbol| symbol.name == "UserService"));
    }

    #[test]
    fn test_search_by_name_uses_fts_identifier_terms() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("--accent-ai", "style.css");
        let sym2 = create_test_symbol(".chat-message", "style.css");
        let sym3 = create_test_symbol("UserService", "service.ts");

        store.upsert_symbols(&[sym1, sym2, sym3]).unwrap();

        let accent_results = store.search_by_name("--accent", 10).unwrap();
        assert!(accent_results
            .iter()
            .any(|symbol| symbol.name == "--accent-ai"));

        let selector_results = store.search_by_name("chat message", 10).unwrap();
        assert!(selector_results
            .iter()
            .any(|symbol| symbol.name == ".chat-message"));
    }

    #[test]
    fn test_delete_file_symbols() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("func1", "test.ts");
        let sym2 = create_test_symbol("func2", "test.ts");

        store.upsert_symbols(&[sym1, sym2]).unwrap();
        assert_eq!(store.count().unwrap(), 2);

        store.delete_file_symbols("test.ts").unwrap();
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn test_clear_generated_index_data_preserves_user_authored_tables() {
        let store = SymbolStore::in_memory().unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "CREATE TABLE user_project_notes (id TEXT PRIMARY KEY, body TEXT NOT NULL)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO user_project_notes (id, body) VALUES ('adr-1', 'keep me')",
                [],
            )
            .unwrap();
        }

        let symbol = create_test_symbol("func1", "test.ts");
        store.upsert_symbols(std::slice::from_ref(&symbol)).unwrap();
        store
            .replace_relationships_for_file(
                "test.ts",
                &[create_test_relationship(&symbol.id, "test.ts", "helper")],
            )
            .unwrap();
        store
            .replace_semantic_anchors_for_file(
                "test.ts",
                &[SemanticAnchor {
                    id: "test.ts::anchor:route:1:0:abc".to_string(),
                    file_path: "test.ts".to_string(),
                    kind: "route".to_string(),
                    value: "/settings".to_string(),
                    line: 1,
                    character: 0,
                    preview: "/settings".to_string(),
                    confidence: 0.9,
                }],
            )
            .unwrap();
        store.mark_file_indexed("test.ts", "hash", 1).unwrap();

        store.clear_generated_index_data().unwrap();

        assert_eq!(store.count().unwrap(), 0);
        assert!(store.indexed_file_record("test.ts").unwrap().is_none());
        assert!(store
            .get_semantic_anchors_in_file("test.ts", 10)
            .unwrap()
            .is_empty());
        assert!(store
            .get_relationship_targets(&symbol.id, SymbolRelationshipType::Call, 10)
            .unwrap()
            .is_empty());

        let preserved_body = {
            let conn = store.conn.lock().unwrap();
            conn.query_row(
                "SELECT body FROM user_project_notes WHERE id = 'adr-1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        assert_eq!(preserved_body, "keep me");
    }

    #[test]
    fn test_generated_index_table_inventory_stays_intentional() {
        assert_eq!(
            GENERATED_INDEX_TABLES,
            &[
                "symbol_relationships",
                "semantic_anchors",
                "symbols",
                "indexed_files"
            ]
        );
        assert_eq!(
            GENERATED_INDEX_CLEAR_STATEMENTS.len(),
            GENERATED_INDEX_TABLES.len()
        );
    }

    /// Column names of `table`, in declaration order, via `PRAGMA table_info`.
    fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    const NEW_RELATIONSHIP_COLUMNS: [&str; 3] =
        ["resolution_strategy", "confidence", "metadata_json"];

    #[test]
    fn migration_forward_from_empty_reaches_latest_with_writable_columns() {
        // A fresh in-memory store runs `create_schema` -> `run_migrations`.
        let store = SymbolStore::in_memory().unwrap();
        let conn = store.conn.lock().unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);

        let columns = table_columns(&conn, "symbol_relationships");
        for new_column in NEW_RELATIONSHIP_COLUMNS {
            assert!(
                columns.iter().any(|column| column == new_column),
                "expected migrated column `{new_column}`"
            );
        }

        // The new columns are writable + queryable.
        conn.execute(
            "INSERT INTO symbol_relationships
             (source_symbol_id, source_file_path, target_name, target_symbol_id,
              relationship_type, line, resolution_strategy, confidence, metadata_json)
             VALUES ('s1', 'f.rs', 'helper', 's2', 'call', 7,
                     'unique_global_name', 0.95, '{\"source\":\"backfill\"}')",
            [],
        )
        .unwrap();

        let (strategy, confidence, metadata): (String, f64, String) = conn
            .query_row(
                "SELECT resolution_strategy, confidence, metadata_json
                 FROM symbol_relationships WHERE source_symbol_id = 's1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(strategy, "unique_global_name");
        assert_eq!(confidence, 0.95);
        assert_eq!(metadata, "{\"source\":\"backfill\"}");
    }

    #[test]
    fn migration_forward_from_current_pre_migration_db() {
        // Simulate a DB created BEFORE M2.3: the base `symbol_relationships`
        // table, `user_version = 0`, none of the new columns present.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE symbol_relationships (
                source_symbol_id TEXT NOT NULL,
                source_file_path TEXT NOT NULL,
                target_name TEXT NOT NULL,
                target_symbol_id TEXT,
                relationship_type TEXT NOT NULL,
                line INTEGER NOT NULL,
                PRIMARY KEY (source_symbol_id, target_name, relationship_type, line)
            );",
        )
        .unwrap();

        let start_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(start_version, 0);
        let before = table_columns(&conn, "symbol_relationships");
        for new_column in NEW_RELATIONSHIP_COLUMNS {
            assert!(!before.iter().any(|column| column == new_column));
        }

        run_migrations(&conn).unwrap();

        let migrated_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(migrated_version, LATEST_SCHEMA_VERSION);
        let after = table_columns(&conn, "symbol_relationships");
        for new_column in NEW_RELATIONSHIP_COLUMNS {
            assert!(
                after.iter().any(|column| column == new_column),
                "expected migrated column `{new_column}`"
            );
        }

        // Running again is a no-op: version stays put and no columns are added.
        run_migrations(&conn).unwrap();
        let rerun_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rerun_version, LATEST_SCHEMA_VERSION);
        assert_eq!(after.len(), table_columns(&conn, "symbol_relationships").len());
    }

    #[test]
    fn migration_idempotency_no_duplicate_columns() {
        // `create_schema` already migrated once; run it twice more.
        let store = SymbolStore::in_memory().unwrap();
        let conn = store.conn.lock().unwrap();

        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);

        let columns = table_columns(&conn, "symbol_relationships");
        for new_column in NEW_RELATIONSHIP_COLUMNS {
            let occurrences = columns.iter().filter(|column| *column == new_column).count();
            assert_eq!(occurrences, 1, "column `{new_column}` was duplicated");
        }
    }

    #[test]
    fn test_replace_and_get_relationship_targets() {
        let store = SymbolStore::in_memory().unwrap();
        let symbol = create_test_symbol("caller", "test.ts");
        store.upsert_symbols(&[symbol.clone()]).unwrap();

        store
            .replace_relationships_for_file(
                "test.ts",
                &[
                    create_test_relationship(&symbol.id, "test.ts", "helperOne"),
                    create_test_relationship(&symbol.id, "test.ts", "helperTwo"),
                ],
            )
            .unwrap();

        let targets = store
            .get_relationship_targets(&symbol.id, SymbolRelationshipType::Call, 10)
            .unwrap();

        assert_eq!(targets.len(), 2);
        assert!(targets.iter().any(|target| target == "helperOne"));
        assert!(targets.iter().any(|target| target == "helperTwo"));
    }

    #[test]
    fn test_find_references_to_target() {
        let store = SymbolStore::in_memory().unwrap();
        let caller = create_test_symbol("caller", "main.ts");
        let helper = create_test_symbol("helper", "utils.ts");
        store.upsert_symbols(&[caller.clone(), helper]).unwrap();

        store
            .replace_relationships_for_file(
                "main.ts",
                &[create_test_relationship(&caller.id, "main.ts", "helper")],
            )
            .unwrap();

        let references = store
            .find_references_to_target("helper", SymbolRelationshipType::Call, 10)
            .unwrap();

        assert_eq!(references.len(), 1);
        assert_eq!(references[0].source_symbol.id, caller.id);
        assert_eq!(references[0].target_name, "helper");
        assert!(references[0].target_symbol_id.is_none());
        assert!(references[0].target_symbol.is_none());
        assert_eq!(
            references[0].relationship_type,
            SymbolRelationshipType::Call
        );
    }

    #[test]
    fn test_find_references_to_symbol_id_and_get_outgoing_edges() {
        let store = SymbolStore::in_memory().unwrap();
        let caller = create_test_symbol("caller", "main.ts");
        let helper = create_test_symbol("helper", "utils.ts");
        store
            .upsert_symbols(&[caller.clone(), helper.clone()])
            .unwrap();

        store
            .replace_relationships_for_file(
                "main.ts",
                &[SymbolRelationship {
                    source_symbol_id: caller.id.clone(),
                    source_file_path: "main.ts".to_string(),
                    target_name: helper.name.clone(),
                    target_symbol_id: Some(helper.id.clone()),
                    relationship_type: SymbolRelationshipType::Call,
                    line: 3,
                }],
            )
            .unwrap();

        let incoming = store
            .find_references_to_symbol_id(&helper.id, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].source_symbol.id, caller.id);
        assert_eq!(
            incoming[0].target_symbol_id.as_deref(),
            Some(helper.id.as_str())
        );
        assert_eq!(
            incoming[0]
                .target_symbol
                .as_ref()
                .map(|symbol| symbol.id.as_str()),
            Some(helper.id.as_str())
        );

        let outgoing = store
            .get_relationship_edges_from_source(&caller.id, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].source_symbol.id, caller.id);
        assert_eq!(
            outgoing[0].target_symbol_id.as_deref(),
            Some(helper.id.as_str())
        );
        assert_eq!(
            outgoing[0]
                .target_symbol
                .as_ref()
                .map(|symbol| symbol.id.as_str()),
            Some(helper.id.as_str())
        );
    }

    #[test]
    fn test_relationship_integrity_stats_counts_missing_edges() {
        let store = SymbolStore::in_memory().unwrap();

        store
            .replace_relationships_for_file(
                "main.ts",
                &[
                    SymbolRelationship {
                        source_symbol_id: "missing-source".to_string(),
                        source_file_path: "main.ts".to_string(),
                        target_name: "helper".to_string(),
                        target_symbol_id: Some("missing-target".to_string()),
                        relationship_type: SymbolRelationshipType::Call,
                        line: 3,
                    },
                    SymbolRelationship {
                        source_symbol_id: "missing-source".to_string(),
                        source_file_path: "main.ts".to_string(),
                        target_name: "dynamicCall".to_string(),
                        target_symbol_id: None,
                        relationship_type: SymbolRelationshipType::Call,
                        line: 4,
                    },
                ],
            )
            .unwrap();

        let stats = store.relationship_integrity_stats().unwrap();

        assert_eq!(stats.total_relationships, 2);
        assert_eq!(stats.resolved_relationships, 1);
        assert_eq!(stats.unresolved_symbol_relationships, 1);
        assert_eq!(stats.missing_source_symbols, 2);
        assert_eq!(stats.missing_target_symbols, 1);
        assert!(stats.by_type.iter().any(|stats| {
            stats.relationship_type == "call"
                && stats.total_relationships == 2
                && stats.resolved_relationships == 1
                && stats.unresolved_symbol_relationships == 1
        }));
        assert!(stats.top_unresolved_targets.iter().any(|target| {
            target.relationship_type == "call"
                && target.target_name == "dynamicCall"
                && target.count == 1
                && target.example_source_file == "main.ts"
        }));
    }

    #[test]
    fn test_delete_file_symbols_removes_relationships() {
        let store = SymbolStore::in_memory().unwrap();
        let symbol = create_test_symbol("caller", "test.ts");
        store.upsert_symbols(&[symbol.clone()]).unwrap();
        store
            .replace_relationships_for_file(
                "test.ts",
                &[create_test_relationship(&symbol.id, "test.ts", "helperOne")],
            )
            .unwrap();

        store.delete_file_symbols("test.ts").unwrap();

        let targets = store
            .get_relationship_targets(&symbol.id, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert!(targets.is_empty());
    }

    #[test]
    fn test_get_file_relationship_targets() {
        let store = SymbolStore::in_memory().unwrap();
        let symbol = create_test_symbol("caller", "test.ts");
        store.upsert_symbols(&[symbol.clone()]).unwrap();
        store
            .replace_relationships_for_file(
                "test.ts",
                &[
                    SymbolRelationship {
                        source_symbol_id: format!("{}::import1#import", symbol.file_path),
                        source_file_path: "test.ts".to_string(),
                        target_name: "./utils".to_string(),
                        target_symbol_id: None,
                        relationship_type: SymbolRelationshipType::Import,
                        line: 1,
                    },
                    SymbolRelationship {
                        source_symbol_id: format!("{}::import2#import", symbol.file_path),
                        source_file_path: "test.ts".to_string(),
                        target_name: "./helpers".to_string(),
                        target_symbol_id: None,
                        relationship_type: SymbolRelationshipType::Import,
                        line: 2,
                    },
                ],
            )
            .unwrap();

        let targets = store
            .get_file_relationship_targets("test.ts", SymbolRelationshipType::Import, 10)
            .unwrap();

        assert_eq!(targets.len(), 2);
        assert!(targets.iter().any(|target| target == "./utils"));
        assert!(targets.iter().any(|target| target == "./helpers"));
    }

    #[test]
    fn test_file_indexing_tracking() {
        let store = SymbolStore::in_memory().unwrap();

        // Initially needs reindex
        assert!(store.needs_reindex("test.ts", "abc123").unwrap());

        // Mark as indexed
        store.mark_file_indexed("test.ts", "abc123", 5).unwrap();

        // Same hash, no reindex needed
        assert!(!store.needs_reindex("test.ts", "abc123").unwrap());

        // Different hash, needs reindex
        assert!(store.needs_reindex("test.ts", "def456").unwrap());
    }

    #[test]
    fn test_file_indexing_metadata_tracking() {
        let store = SymbolStore::in_memory().unwrap();

        assert_eq!(
            store
                .needs_reindex_for_metadata("test.ts", 128, 1_234)
                .unwrap(),
            Some(true)
        );

        store
            .mark_file_indexed_with_metadata(
                "test.ts",
                "abc123",
                5,
                Some(128),
                Some(12),
                Some(1_234),
            )
            .unwrap();

        let record = store.indexed_file_record("test.ts").unwrap().unwrap();
        assert_eq!(record.file_size, Some(128));
        assert_eq!(record.line_count, Some(12));
        assert_eq!(record.extractor_version, None);

        store
            .mark_file_indexed_with_metadata_and_extractor_version(
                "test.ts",
                "abc123",
                5,
                Some(128),
                Some(12),
                Some(1_234),
                Some(7),
            )
            .unwrap();
        let record = store.indexed_file_record("test.ts").unwrap().unwrap();
        assert_eq!(record.extractor_version, Some(7));

        assert_eq!(
            store
                .needs_reindex_for_metadata("test.ts", 128, 1_234)
                .unwrap(),
            Some(false)
        );
        assert_eq!(
            store
                .needs_reindex_for_metadata("test.ts", 129, 1_234)
                .unwrap(),
            Some(true)
        );
        assert_eq!(
            store
                .needs_reindex_for_metadata("test.ts", 128, 1_235)
                .unwrap(),
            Some(true)
        );
    }
}
