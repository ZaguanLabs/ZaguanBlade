//! Unified Language Service
//!
//! Combines tree-sitter parsing, symbol indexing, and LSP features
//! into a single coherent API.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use crate::lsp::LspManager;
use crate::symbol_index::{SearchQuery, SearchResult, SymbolStore};
use crate::tree_sitter::{extract_symbols, Language, Symbol, SymbolType, TreeSitterParser};

/// Unified language service
pub struct LanguageService {
    /// Workspace root path
    workspace_root: PathBuf,
    /// Tree-sitter parser for AST analysis
    parser: Mutex<TreeSitterParser>,
    /// Symbol index for persistent storage
    symbol_store: Arc<SymbolStore>,
    /// LSP manager for language server features
    lsp_manager: RwLock<Option<LspManager>>,
    /// In-memory cache of recently parsed files
    file_cache: RwLock<HashMap<String, CachedFile>>,
}

/// Cached file data
#[derive(Clone)]
struct CachedFile {
    /// File content
    content: String,
    /// Content hash for change detection
    hash: String,
    /// Extracted symbols
    symbols: Vec<Symbol>,
    /// Last modified timestamp
    modified_at: u64,
}

/// Error type for language service operations
#[derive(Debug)]
pub enum LanguageError {
    Parse(String),
    Index(String),
    Lsp(String),
    Io(std::io::Error),
    NotSupported(String),
}

impl std::fmt::Display for LanguageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LanguageError::Parse(msg) => write!(f, "Parse error: {}", msg),
            LanguageError::Index(msg) => write!(f, "Index error: {}", msg),
            LanguageError::Lsp(msg) => write!(f, "LSP error: {}", msg),
            LanguageError::Io(e) => write!(f, "IO error: {}", e),
            LanguageError::NotSupported(msg) => write!(f, "Not supported: {}", msg),
        }
    }
}

impl std::error::Error for LanguageError {}

impl From<std::io::Error> for LanguageError {
    fn from(e: std::io::Error) -> Self {
        LanguageError::Io(e)
    }
}

impl From<crate::symbol_index::store::SymbolStoreError> for LanguageError {
    fn from(e: crate::symbol_index::store::SymbolStoreError) -> Self {
        LanguageError::Index(e.to_string())
    }
}

impl From<crate::lsp::LspError> for LanguageError {
    fn from(e: crate::lsp::LspError) -> Self {
        LanguageError::Lsp(e.to_string())
    }
}

impl LanguageService {
    /// Create a new language service for a workspace
    pub fn new(
        workspace_root: PathBuf,
        symbol_store: Arc<SymbolStore>,
    ) -> Result<Self, LanguageError> {
        let parser = TreeSitterParser::new().map_err(|e| LanguageError::Parse(e.to_string()))?;

        Ok(Self {
            workspace_root,
            parser: Mutex::new(parser),
            symbol_store,
            lsp_manager: RwLock::new(None),
            file_cache: RwLock::new(HashMap::new()),
        })
    }

    /// Enable LSP support
    pub fn enable_lsp(&self) -> Result<(), LanguageError> {
        let mut lsp = self.lsp_manager.write().unwrap();
        if lsp.is_none() {
            let manager = LspManager::new(self.workspace_root.to_string_lossy().as_ref());
            *lsp = Some(manager);
            eprintln!("[LanguageService] LSP support enabled");
        }
        Ok(())
    }

    /// Disable LSP support
    pub fn disable_lsp(&self) {
        let mut lsp = self.lsp_manager.write().unwrap();
        *lsp = None;
        eprintln!("[LanguageService] LSP support disabled");
    }

    /// Check if LSP is enabled
    pub fn is_lsp_enabled(&self) -> bool {
        self.lsp_manager.read().unwrap().is_some()
    }

    // =========================================================================
    // Symbol Operations (Tree-sitter + Index)
    // =========================================================================

    /// Index a single file
    pub fn index_file(&self, file_path: &str) -> Result<Vec<Symbol>, LanguageError> {
        let full_path = self.resolve_path(file_path);
        let content = std::fs::read_to_string(&full_path)?;
        let hash = compute_hash(&content);

        // Check if reindexing is needed
        if !self.symbol_store.needs_reindex(file_path, &hash)? {
            // Return cached symbols from database
            return Ok(self.symbol_store.get_symbols_in_file(file_path)?);
        }

        // Detect language and parse
        let language = Language::from_path(file_path).ok_or_else(|| {
            LanguageError::NotSupported(format!("Unknown language for: {}", file_path))
        })?;

        let tree = {
            let mut parser = self.parser.lock().unwrap();
            parser
                .parse(&content, language)
                .map_err(|e| LanguageError::Parse(e.to_string()))?
        };

        // Extract symbols
        let symbols = extract_symbols(&tree, &content, language, file_path);

        // Delete old symbols and insert new ones
        self.symbol_store.delete_file_symbols(file_path)?;
        self.symbol_store.upsert_symbols(&symbols)?;
        self.symbol_store
            .mark_file_indexed(file_path, &hash, symbols.len())?;

        // Update cache
        {
            let mut cache = self.file_cache.write().unwrap();
            cache.insert(
                file_path.to_string(),
                CachedFile {
                    content,
                    hash,
                    symbols: symbols.clone(),
                    modified_at: now_timestamp(),
                },
            );
        }

        eprintln!(
            "[LanguageService] Indexed {} symbols in {}",
            symbols.len(),
            file_path
        );
        Ok(symbols)
    }

    /// Index an entire directory recursively
    pub fn index_directory(&self, dir_path: &str) -> Result<IndexStats, LanguageError> {
        let full_path = self.resolve_path(dir_path);
        let mut stats = IndexStats::default();
        let start = std::time::Instant::now();

        self.index_directory_recursive(&full_path, "", &mut stats)?;

        stats.duration_ms = start.elapsed().as_millis() as u64;
        eprintln!(
            "[LanguageService] Indexed {} files, {} symbols in {}ms",
            stats.files_indexed, stats.symbols_extracted, stats.duration_ms
        );

        Ok(stats)
    }

    fn index_directory_recursive(
        &self,
        base_path: &Path,
        relative_path: &str,
        stats: &mut IndexStats,
    ) -> Result<(), LanguageError> {
        let dir_path = if relative_path.is_empty() {
            base_path.to_path_buf()
        } else {
            base_path.join(relative_path)
        };

        if !dir_path.exists() || !dir_path.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&dir_path)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            // Skip hidden files/dirs and common ignore patterns
            if file_name.starts_with('.')
                || file_name == "node_modules"
                || file_name == "target"
                || file_name == "dist"
                || file_name == "__pycache__"
                || file_name == ".git"
            {
                continue;
            }

            let relative = if relative_path.is_empty() {
                file_name.to_string()
            } else {
                format!("{}/{}", relative_path, file_name)
            };

            if path.is_dir() {
                self.index_directory_recursive(base_path, &relative, stats)?;
            } else if path.is_file() {
                // Check if it's a supported language
                if Language::from_path(&relative).is_some() {
                    match self.index_file(&relative) {
                        Ok(symbols) => {
                            stats.files_indexed += 1;
                            stats.symbols_extracted += symbols.len();
                        }
                        Err(e) => {
                            stats.files_failed += 1;
                            eprintln!("[LanguageService] Failed to index {}: {}", relative, e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Search symbols by query
    pub fn search_symbols(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>, LanguageError> {
        let search_query = SearchQuery::text(query).with_limit(limit);
        let results =
            crate::symbol_index::search::execute_search(&self.symbol_store, &search_query)?;
        Ok(results)
    }

    /// Search symbols with filters
    pub fn search_symbols_filtered(
        &self,
        query: &str,
        file_path: Option<&str>,
        symbol_types: Option<Vec<SymbolType>>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, LanguageError> {
        let mut search_query = SearchQuery::text(query).with_limit(limit);

        if let Some(path) = file_path {
            search_query = search_query.with_file(path);
        }

        if let Some(types) = symbol_types {
            search_query = search_query.with_types(types);
        }

        let results =
            crate::symbol_index::search::execute_search(&self.symbol_store, &search_query)?;
        Ok(results)
    }

    /// Get symbol at position
    pub fn get_symbol_at(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<Symbol>, LanguageError> {
        Ok(self
            .symbol_store
            .get_symbol_at(file_path, line, character)?)
    }

    /// Get all symbols in a file
    pub fn get_file_symbols(&self, file_path: &str) -> Result<Vec<Symbol>, LanguageError> {
        Ok(self.symbol_store.get_symbols_in_file(file_path)?)
    }

    // =========================================================================
    // LSP Operations (pass-through to LSP manager)
    // =========================================================================

    /// Get completions at position
    pub fn get_completions(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<crate::lsp::types::CompletionItem>, LanguageError> {
        let mut lsp = self.lsp_manager.write().unwrap();
        match lsp.as_mut() {
            Some(manager) => {
                let full_path = self.resolve_path(file_path);
                let items =
                    manager.completion(full_path.to_string_lossy().as_ref(), line, character)?;
                Ok(items)
            }
            None => Ok(vec![]),
        }
    }

    /// Get hover information at position
    pub fn get_hover(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<crate::lsp::types::Hover>, LanguageError> {
        let mut lsp = self.lsp_manager.write().unwrap();
        match lsp.as_mut() {
            Some(manager) => {
                let full_path = self.resolve_path(file_path);
                let hover = manager.hover(full_path.to_string_lossy().as_ref(), line, character)?;
                Ok(hover)
            }
            None => Ok(None),
        }
    }

    /// Go to definition
    pub fn get_definition(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<crate::lsp::types::Location>, LanguageError> {
        let mut lsp = self.lsp_manager.write().unwrap();
        match lsp.as_mut() {
            Some(manager) => {
                let full_path = self.resolve_path(file_path);
                let locations =
                    manager.definition(full_path.to_string_lossy().as_ref(), line, character)?;
                Ok(locations)
            }
            None => Ok(vec![]),
        }
    }

    /// Find references
    pub fn get_references(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<crate::lsp::types::Location>, LanguageError> {
        let mut lsp = self.lsp_manager.write().unwrap();
        match lsp.as_mut() {
            Some(manager) => {
                let full_path = self.resolve_path(file_path);
                let locations = manager.references(
                    full_path.to_string_lossy().as_ref(),
                    line,
                    character,
                    include_declaration,
                )?;
                Ok(locations)
            }
            None => Ok(vec![]),
        }
    }

    /// Get document symbols from LSP
    pub fn get_document_symbols_lsp(
        &self,
        file_path: &str,
    ) -> Result<Vec<crate::lsp::types::DocumentSymbol>, LanguageError> {
        let mut lsp = self.lsp_manager.write().unwrap();
        match lsp.as_mut() {
            Some(manager) => {
                let full_path = self.resolve_path(file_path);
                let symbols = manager.document_symbols(full_path.to_string_lossy().as_ref())?;
                Ok(symbols)
            }
            None => Ok(vec![]),
        }
    }

    /// Get diagnostics for a file
    pub fn get_diagnostics(&self, file_path: &str) -> Vec<crate::lsp::types::Diagnostic> {
        let lsp = self.lsp_manager.read().unwrap();
        match lsp.as_ref() {
            Some(manager) => {
                let full_path = self.resolve_path(file_path);
                manager.get_diagnostics(full_path.to_string_lossy().as_ref())
            }
            None => vec![],
        }
    }

    // =========================================================================
    // Document Synchronization
    // =========================================================================

    /// Notify that a document was opened
    pub fn did_open(&self, file_path: &str, content: &str) -> Result<(), LanguageError> {
        // Index the file
        let _ = self.index_file_content(file_path, content)?;

        // Notify LSP if enabled
        let mut lsp = self.lsp_manager.write().unwrap();
        if let Some(manager) = lsp.as_mut() {
            let full_path = self.resolve_path(file_path);
            let _ = manager.did_open(full_path.to_string_lossy().as_ref(), content);
        }

        Ok(())
    }

    /// Notify that a document changed
    pub fn did_change(
        &self,
        file_path: &str,
        version: i32,
        content: &str,
    ) -> Result<(), LanguageError> {
        // Re-index the file
        let _ = self.index_file_content(file_path, content)?;

        // Notify LSP if enabled
        let mut lsp = self.lsp_manager.write().unwrap();
        if let Some(manager) = lsp.as_mut() {
            let full_path = self.resolve_path(file_path);
            let _ = manager.did_change(full_path.to_string_lossy().as_ref(), version, content);
        }

        Ok(())
    }

    /// Notify that a document was closed
    pub fn did_close(&self, file_path: &str) -> Result<(), LanguageError> {
        // Remove from cache
        {
            let mut cache = self.file_cache.write().unwrap();
            cache.remove(file_path);
        }

        // Notify LSP if enabled
        let mut lsp = self.lsp_manager.write().unwrap();
        if let Some(manager) = lsp.as_mut() {
            let full_path = self.resolve_path(file_path);
            let _ = manager.did_close(full_path.to_string_lossy().as_ref());
        }

        Ok(())
    }

    // =========================================================================
    // Helper Methods
    // =========================================================================

    pub fn resolve_path(&self, file_path: &str) -> PathBuf {
        if Path::new(file_path).is_absolute() {
            PathBuf::from(file_path)
        } else {
            self.workspace_root.join(file_path)
        }
    }

    fn index_file_content(
        &self,
        file_path: &str,
        content: &str,
    ) -> Result<Vec<Symbol>, LanguageError> {
        let hash = compute_hash(content);

        // Check cache first
        {
            let cache = self.file_cache.read().unwrap();
            if let Some(cached) = cache.get(file_path) {
                if cached.hash == hash {
                    return Ok(cached.symbols.clone());
                }
            }
        }

        // Detect language and parse
        let language = Language::from_path(file_path).ok_or_else(|| {
            LanguageError::NotSupported(format!("Unknown language for: {}", file_path))
        })?;

        let tree = {
            let mut parser = self.parser.lock().unwrap();
            parser
                .parse(content, language)
                .map_err(|e| LanguageError::Parse(e.to_string()))?
        };

        // Extract symbols
        let symbols = extract_symbols(&tree, content, language, file_path);

        // Delete old symbols and insert new ones
        self.symbol_store.delete_file_symbols(file_path)?;
        self.symbol_store.upsert_symbols(&symbols)?;
        self.symbol_store
            .mark_file_indexed(file_path, &hash, symbols.len())?;

        // Update cache
        {
            let mut cache = self.file_cache.write().unwrap();
            cache.insert(
                file_path.to_string(),
                CachedFile {
                    content: content.to_string(),
                    hash,
                    symbols: symbols.clone(),
                    modified_at: now_timestamp(),
                },
            );
        }

        Ok(symbols)
    }

    /// Get statistics about the index
    pub fn stats(&self) -> Result<IndexStats, LanguageError> {
        Ok(IndexStats {
            files_indexed: self.symbol_store.file_count()?,
            symbols_extracted: self.symbol_store.count()?,
            files_failed: 0,
            duration_ms: 0,
        })
    }
}

/// Statistics about indexing operations
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub symbols_extracted: usize,
    pub files_failed: usize,
    pub duration_ms: u64,
}

/// Compute a simple hash of content for change detection
fn compute_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Get current timestamp in seconds
fn now_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_service() -> (LanguageService, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("symbols.db");
        let store = Arc::new(SymbolStore::new(&db_path).unwrap());
        let service = LanguageService::new(temp_dir.path().to_path_buf(), store).unwrap();
        (service, temp_dir)
    }

    #[test]
    fn test_index_typescript_file() {
        let (service, temp_dir) = create_test_service();

        // Create a test file
        let file_path = temp_dir.path().join("test.ts");
        fs::write(
            &file_path,
            r#"
            function authenticate(token: string): boolean {
                return token.length > 0;
            }
            
            class UserService {
                getUser(id: string): User | undefined {
                    return undefined;
                }
            }
        "#,
        )
        .unwrap();

        let symbols = service.index_file("test.ts").unwrap();

        // Should find function and class
        assert!(symbols.iter().any(|s| s.name == "authenticate"));
        assert!(symbols.iter().any(|s| s.name == "UserService"));
    }

    #[test]
    fn test_search_symbols() {
        let (service, temp_dir) = create_test_service();

        // Create test files
        fs::write(
            temp_dir.path().join("auth.ts"),
            r#"
            function authenticate() {}
            function authorize() {}
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("utils.ts"),
            r#"
            function validateToken() {}
        "#,
        )
        .unwrap();

        service.index_file("auth.ts").unwrap();
        service.index_file("utils.ts").unwrap();

        let results = service.search_symbols("auth", 10).unwrap();

        // Should find authenticate and authorize
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_stats() {
        let (service, temp_dir) = create_test_service();

        fs::write(temp_dir.path().join("test.ts"), "function test() {}").unwrap();
        service.index_file("test.ts").unwrap();

        let stats = service.stats().unwrap();
        assert_eq!(stats.files_indexed, 1);
        assert!(stats.symbols_extracted > 0);
    }
}
