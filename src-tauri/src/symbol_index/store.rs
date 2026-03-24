//! Symbol storage using SQLite
//!
//! Persistent storage for extracted code symbols with efficient
//! indexing and retrieval.

use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

use crate::tree_sitter::{Symbol, SymbolRelationship, SymbolRelationshipType, SymbolType};

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
    pub indexed_at: i64,
    pub symbol_count: usize,
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
                symbol_count INTEGER NOT NULL
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
            "#,
        )?;

        ensure_column(&conn, "symbols", "qualified_name", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(&conn, "symbols", "byte_offset", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "symbols", "byte_length", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "symbols", "content_hash", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(&conn, "symbol_relationships", "target_symbol_id", "TEXT")?;

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
            "#,
        )?;

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
                params![source_symbol_id, relationship_type.to_string(), limit as i64],
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
                params![source_file_path, relationship_type.to_string(), limit as i64],
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

            let rows = stmt.query_map(
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

            let rows = stmt.query_map(
                params![target_symbol_id, relationship_type.to_string(), limit as i64],
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

            let rows = stmt.query_map(
                params![source_symbol_id, relationship_type.to_string(), limit as i64],
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
        let conn = self.conn.lock().unwrap();

        // Use FTS5 for searching
        let mut stmt = conn.prepare(
            r#"
            SELECT s.id, s.name, s.qualified_name, s.symbol_type, s.file_path, s.start_line, s.start_char,
                   s.end_line, s.end_char, s.byte_offset, s.byte_length, s.parent_id, s.docstring, s.signature, s.content_hash
            FROM symbols s
            JOIN symbols_fts fts ON s.rowid = fts.rowid
            WHERE symbols_fts MATCH ?1
            ORDER BY rank
            LIMIT ?2
            "#,
        )?;

        // FTS5 query syntax: prefix matching with *
        let fts_query = format!("{}*", query.replace(' ', " OR "));

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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols 
            WHERE name LIKE ?1 OR qualified_name LIKE ?1
            ORDER BY CASE
                WHEN qualified_name = ?2 THEN 0
                WHEN name = ?2 THEN 1
                WHEN qualified_name LIKE ?3 THEN 2
                WHEN name LIKE ?3 THEN 3
                ELSE 4
            END, name
            LIMIT ?4
            "#,
        )?;

        let like_pattern = format!("%{}%", pattern);
        let prefix_pattern = format!("{}%", pattern);
        let symbols = stmt
            .query_map(params![like_pattern, pattern, prefix_pattern, limit as i64], |row| {
                row_to_symbol(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;

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

    /// Delete all symbols
    pub fn clear(&self) -> Result<(), SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM symbol_relationships", [])?;
        conn.execute("DELETE FROM symbols", [])?;
        conn.execute("DELETE FROM indexed_files", [])?;
        Ok(())
    }

    /// Get total symbol count
    pub fn count(&self) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn list_indexed_files(&self, limit: usize) -> Result<Vec<IndexedFileRecord>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT file_path, indexed_at, symbol_count
            FROM indexed_files
            ORDER BY symbol_count DESC, file_path ASC
            LIMIT ?1
            "#,
        )?;

        let records = stmt
            .query_map(params![limit as i64], |row| {
                Ok(IndexedFileRecord {
                    file_path: row.get::<_, String>(0)?,
                    indexed_at: row.get::<_, i64>(1)?,
                    symbol_count: row.get::<_, i64>(2)? as usize,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    /// Get count of indexed files
    pub fn file_count(&self) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(DISTINCT file_path) FROM symbols", [], |row| {
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
        let conn = self.conn.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            r#"
            INSERT OR REPLACE INTO indexed_files (file_path, file_hash, indexed_at, symbol_count)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![file_path, file_hash, now, symbol_count as i64],
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

    fn hydrate_symbol_references(
        &self,
        rows: Vec<(Symbol, String, String, Option<String>, u32)>,
    ) -> Result<Vec<SymbolReference>, SymbolStoreError> {
        let mut references = Vec::with_capacity(rows.len());

        for (source_symbol, relationship_type, target_name, target_symbol_id, line) in rows {
            let relationship_type = relationship_type
                .parse::<SymbolRelationshipType>()
                .map_err(|error| {
                    SymbolStoreError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
                })?;
            let target_symbol = match target_symbol_id.as_deref() {
                Some(id) => self.get_symbol(id)?,
                None => None,
            };

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
}

/// Convert a database row to a Symbol
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

fn ensure_column(conn: &Connection, table_name: &str, column_name: &str, definition: &str) -> Result<(), SymbolStoreError> {
    let pragma = format!("PRAGMA table_info({})", table_name);
    let mut stmt = conn.prepare(&pragma)?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;

    if !columns.iter().any(|existing| existing == column_name) {
        let alter = format!("ALTER TABLE {} ADD COLUMN {} {}", table_name, column_name, definition);
        conn.execute(&alter, [])?;
    }

    Ok(())
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

    fn create_test_relationship(source_symbol_id: &str, file_path: &str, target_name: &str) -> SymbolRelationship {
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
        store
            .upsert_symbols(&[caller.clone(), helper])
            .unwrap();

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
        assert_eq!(references[0].relationship_type, SymbolRelationshipType::Call);
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
        assert_eq!(incoming[0].target_symbol_id.as_deref(), Some(helper.id.as_str()));
        assert_eq!(incoming[0].target_symbol.as_ref().map(|symbol| symbol.id.as_str()), Some(helper.id.as_str()));

        let outgoing = store
            .get_relationship_edges_from_source(&caller.id, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].source_symbol.id, caller.id);
        assert_eq!(outgoing[0].target_symbol_id.as_deref(), Some(helper.id.as_str()));
        assert_eq!(outgoing[0].target_symbol.as_ref().map(|symbol| symbol.id.as_str()), Some(helper.id.as_str()));
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
}
