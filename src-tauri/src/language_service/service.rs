//! Unified Language Service
//!
//! Combines tree-sitter parsing and symbol indexing
//! into a single coherent API for ZLP.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use crate::buffer_snapshot::{BufferSnapshot, BufferSnapshotStore};
use crate::gitignore_filter::GitignoreFilter;
use crate::project_settings;
use crate::symbol_index::{SearchQuery, SearchResult, SymbolReference, SymbolStore};
use crate::tree_sitter::{
    extract_symbol_relationships, extract_symbols, Language, Position, Range, Symbol,
    SymbolRelationship, SymbolRelationshipType, SymbolType, TreeSitterParser,
};
use crate::worktree::WorktreeStore;
use serde::{Deserialize, Serialize};

/// Unified language service
pub struct LanguageService {
    /// Workspace root path
    workspace_root: PathBuf,
    /// Tree-sitter parser for AST analysis
    parser: Mutex<TreeSitterParser>,
    /// Symbol index for persistent storage
    symbol_store: Arc<SymbolStore>,
    /// Shared in-memory worktree snapshot/index
    worktree_store: RwLock<Option<Arc<WorktreeStore>>>,
    buffer_snapshots: BufferSnapshotStore,

    /// In-memory cache of recently parsed files
    file_cache: RwLock<HashMap<String, CachedFile>>,
    index_health: RwLock<IndexHealthSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IndexHealthStatus {
    Unknown,
    Checking,
    Fresh,
    Indexing,
    Partial,
    Stale,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexHealthSnapshot {
    pub status: IndexHealthStatus,
    pub indexed_files: usize,
    pub supported_files: usize,
    pub stale_files: usize,
    pub missing_files: usize,
    pub orphaned_files: usize,
    pub queued_files: usize,
    pub active_workers: usize,
    pub symbol_count: usize,
    pub last_full_scan_ms: Option<u64>,
    pub last_incremental_update_ms: Option<u64>,
    pub message: String,
}

impl Default for IndexHealthSnapshot {
    fn default() -> Self {
        Self {
            status: IndexHealthStatus::Unknown,
            indexed_files: 0,
            supported_files: 0,
            stale_files: 0,
            missing_files: 0,
            orphaned_files: 0,
            queued_files: 0,
            active_workers: 0,
            symbol_count: 0,
            last_full_scan_ms: None,
            last_incremental_update_ms: None,
            message: "Code intelligence status unknown".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IndexReconciliationReport {
    pub health: IndexHealthSnapshot,
    pub files_indexed: usize,
    pub files_removed: usize,
    pub duration_ms: u64,
}

/// Cached file data
#[derive(Clone)]
struct CachedFile {
    /// Content hash for change detection
    hash: String,
    _snapshot: Arc<BufferSnapshot>,
    /// Extracted symbols
    symbols: Vec<Symbol>,
}

/// Error type for language service operations
#[derive(Debug)]
pub enum LanguageError {
    Parse(String),
    Index(String),

    Io(std::io::Error),
    NotSupported(String),
}

impl std::fmt::Display for LanguageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LanguageError::Parse(msg) => write!(f, "Parse error: {}", msg),
            LanguageError::Index(msg) => write!(f, "Index error: {}", msg),

            LanguageError::Io(e) => write!(f, "IO error: {}", e),
            LanguageError::NotSupported(msg) => write!(f, "Not supported: {}", msg),
        }
    }
}

fn symbol_line_text(content: &str, byte_offset: usize) -> &str {
    let safe_offset = byte_offset.min(content.len());
    let before = &content[..safe_offset];
    let line_start = before.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let line_end = content[safe_offset..]
        .find('\n')
        .map(|idx| safe_offset + idx)
        .unwrap_or(content.len());
    &content[line_start..line_end]
}

fn typescript_named_export_clauses(content: &str) -> Vec<(String, String, Option<String>, u32)> {
    let mut clauses = Vec::new();
    let mut search_from = 0usize;

    while let Some(relative_start) = content[search_from..].find("export {") {
        let start = search_from + relative_start;
        let clause_start = start + "export {".len();
        let Some(relative_end) = content[clause_start..].find('}') else {
            break;
        };
        let clause_end = clause_start + relative_end;
        let clause = &content[clause_start..clause_end];
        let trailing = &content[clause_end + 1..];
        let module_target = trailing
            .split_once(" from ")
            .and_then(|(_, rest)| extract_quoted_literal(rest));
        let line = content[..start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count() as u32;

        for specifier in clause.split(',') {
            let specifier = specifier.trim();
            if specifier.is_empty() {
                continue;
            }

            let mut parts = specifier.splitn(2, " as ").map(str::trim);
            let local_name = parts.next().filter(|value| !value.is_empty());
            let exported_name = parts
                .next()
                .filter(|value| !value.is_empty())
                .or(local_name);

            if let (Some(local_name), Some(exported_name)) = (local_name, exported_name) {
                clauses.push((
                    local_name.to_string(),
                    exported_name.to_string(),
                    module_target.clone(),
                    line,
                ));
            }
        }

        search_from = clause_end + 1;
    }

    clauses
}

fn typescript_export_star_targets(content: &str) -> Vec<(String, u32)> {
    let mut exports = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("export * from ") else {
            continue;
        };
        if let Some(module_target) = extract_quoted_literal(rest) {
            exports.push((module_target, line_index as u32));
        }
    }

    exports
}

fn rust_pub_use_plain_module_reexports(content: &str) -> Vec<(String, String, u32)> {
    let mut exports = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("pub use ") else {
            continue;
        };
        let rest = rest.trim_end_matches(';').trim();
        if rest.is_empty() || rest.contains(" as ") || rest.contains("::{") || rest.ends_with("::*")
        {
            continue;
        }

        let Some((_, exported_name)) = rest.rsplit_once("::") else {
            continue;
        };
        let exported_name = exported_name.trim();
        if exported_name.is_empty() {
            continue;
        }

        exports.push((
            rest.to_string(),
            exported_name.to_string(),
            line_index as u32,
        ));
    }

    exports
}

fn rust_grouped_pub_use_module_reexports(content: &str) -> Vec<(String, String, u32)> {
    let mut exports = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("pub use ") else {
            continue;
        };
        let rest = rest.trim_end_matches(';').trim();
        let Some((module_prefix, grouped)) = rest.split_once("::{") else {
            continue;
        };
        let Some(group_end) = grouped.find('}') else {
            continue;
        };
        let module_prefix = module_prefix.trim();
        if module_prefix.is_empty() {
            continue;
        }

        for entry in grouped[..group_end].split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }

            let Some(alias_name) = entry.strip_prefix("self as ").map(str::trim) else {
                continue;
            };
            if alias_name.is_empty() {
                continue;
            }

            exports.push((
                module_prefix.to_string(),
                alias_name.to_string(),
                line_index as u32,
            ));
        }
    }

    exports
}

fn rust_pub_use_module_reexports(content: &str) -> Vec<(String, String, u32)> {
    let mut exports = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("pub use ") else {
            continue;
        };
        let rest = rest.trim_end_matches(';').trim();
        if rest.is_empty() || rest.contains("::{") || rest.ends_with("::*") {
            continue;
        }

        let Some((target_path, alias_name)) = rest.split_once(" as ") else {
            continue;
        };
        let target_path = target_path.trim();
        let alias_name = alias_name.trim();
        if target_path.is_empty() || alias_name.is_empty() {
            continue;
        }

        exports.push((
            target_path.to_string(),
            alias_name.to_string(),
            line_index as u32,
        ));
    }

    exports
}

fn typescript_namespace_export_clauses(content: &str) -> Vec<(String, String, u32)> {
    let mut clauses = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("export * as ") else {
            continue;
        };
        let Some((exported_name, module_part)) = rest.split_once(" from ") else {
            continue;
        };
        let exported_name = exported_name.trim();
        if exported_name.is_empty() {
            continue;
        }
        let Some(module_target) = extract_quoted_literal(module_part) else {
            continue;
        };
        clauses.push((exported_name.to_string(), module_target, line_index as u32));
    }

    clauses
}

fn python_from_import_clauses(content: &str) -> Vec<(String, String, String, u32)> {
    let mut imports = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(after_from) = trimmed.strip_prefix("from ") else {
            continue;
        };
        let Some((module_target, imported_part)) = after_from.split_once(" import ") else {
            continue;
        };
        let module_target = module_target.trim();
        if module_target.is_empty() {
            continue;
        }

        for entry in imported_part.split(',') {
            let entry = entry.trim();
            if entry.is_empty() || entry == "*" {
                continue;
            }

            let mut parts = entry.splitn(2, " as ").map(str::trim);
            let local_name = parts.next().filter(|value| !value.is_empty());
            let exported_name = parts
                .next()
                .filter(|value| !value.is_empty())
                .or(local_name);

            if let (Some(local_name), Some(exported_name)) = (local_name, exported_name) {
                imports.push((
                    module_target.to_string(),
                    local_name.to_string(),
                    exported_name.to_string(),
                    line_index as u32,
                ));
            }
        }
    }

    imports
}

fn python_import_module_clauses(content: &str) -> Vec<(String, String, u32)> {
    let mut imports = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(after_import) = trimmed.strip_prefix("import ") else {
            continue;
        };

        for entry in after_import.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }

            let mut parts = entry.splitn(2, " as ").map(str::trim);
            let module_target = parts.next().filter(|value| !value.is_empty());
            let alias_name = parts.next().filter(|value| !value.is_empty());
            let Some(module_target) = module_target else {
                continue;
            };

            let (resolution_target, exported_name) = if let Some(alias_name) = alias_name {
                (module_target, alias_name)
            } else if let Some((package_name, _)) = module_target.split_once('.') {
                (package_name, package_name)
            } else {
                (module_target, module_target)
            };

            imports.push((
                resolution_target.to_string(),
                exported_name.to_string(),
                line_index as u32,
            ));
        }
    }

    imports
}

fn python_from_import_star_targets(content: &str) -> Vec<(String, u32)> {
    let mut imports = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(after_from) = trimmed.strip_prefix("from ") else {
            continue;
        };
        let Some((module_target, imported_part)) = after_from.split_once(" import ") else {
            continue;
        };
        if imported_part.trim() != "*" {
            continue;
        }
        let module_target = module_target.trim();
        if module_target.is_empty() {
            continue;
        }
        imports.push((module_target.to_string(), line_index as u32));
    }

    imports
}

fn python_is_exported_name(content: &str, name: &str) -> bool {
    python_dunder_all_names(content)
        .map(|exported| exported.contains(name))
        .unwrap_or_else(|| !name.starts_with('_'))
}

fn python_join_module_target(module_target: &str, local_name: &str) -> String {
    if module_target.is_empty() {
        return local_name.to_string();
    }

    if module_target.ends_with('.') {
        return format!("{module_target}{local_name}");
    }

    format!("{module_target}.{local_name}")
}

fn extract_quoted_literal(text: &str) -> Option<String> {
    let quote_start = text.find(['\'', '"'])?;
    let quote = text[quote_start..].chars().next()?;
    let rest = &text[quote_start + quote.len_utf8()..];
    let quote_end = rest.find(quote)?;
    Some(rest[..quote_end].to_string())
}

fn rust_pub_use_reexports(content: &str) -> Vec<(String, String, String, u32)> {
    let mut exports = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("pub use ") else {
            continue;
        };
        let rest = rest.trim_end_matches(';').trim();
        if rest.is_empty() {
            continue;
        }

        if let Some((module_prefix, grouped)) = rest.split_once("::{") {
            let Some(group_end) = grouped.find('}') else {
                continue;
            };
            let module_prefix = module_prefix.trim();
            for entry in grouped[..group_end].split(',') {
                let entry = entry.trim();
                if entry.is_empty() {
                    continue;
                }
                let mut parts = entry.splitn(2, " as ").map(str::trim);
                let symbol_name = parts.next().filter(|value| !value.is_empty());
                let exported_name = parts
                    .next()
                    .filter(|value| !value.is_empty())
                    .or(symbol_name);
                if let (Some(symbol_name), Some(exported_name)) = (symbol_name, exported_name) {
                    exports.push((
                        module_prefix.to_string(),
                        symbol_name.to_string(),
                        exported_name.to_string(),
                        line_index as u32,
                    ));
                }
            }
            continue;
        }

        let mut parts = rest.splitn(2, " as ").map(str::trim);
        let target_path = parts.next().filter(|value| !value.is_empty());
        let alias_name = parts.next().filter(|value| !value.is_empty());
        let Some(target_path) = target_path else {
            continue;
        };
        let Some((module_path, symbol_name)) = target_path.rsplit_once("::") else {
            continue;
        };
        if symbol_name == "*" {
            continue;
        }

        exports.push((
            module_path.to_string(),
            symbol_name.to_string(),
            alias_name.unwrap_or(symbol_name).to_string(),
            line_index as u32,
        ));
    }

    exports
}

fn rust_pub_use_glob_reexports(content: &str) -> Vec<(String, u32)> {
    let mut exports = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("pub use ") else {
            continue;
        };
        let rest = rest.trim_end_matches(';').trim();
        let Some(module_path) = rest.strip_suffix("::*") else {
            continue;
        };
        exports.push((module_path.trim().to_string(), line_index as u32));
    }

    exports
}

fn python_dunder_all_names(content: &str) -> Option<HashSet<String>> {
    let marker = "__all__";
    let start = content.find(marker)?;
    let rest = &content[start + marker.len()..];
    let equals = rest.find('=')?;
    let assigned = rest[equals + 1..].trim_start();
    let (open, close) = if assigned.starts_with('[') {
        ('[', ']')
    } else if assigned.starts_with('(') {
        ('(', ')')
    } else {
        return None;
    };

    let inner_start = assigned.find(open)? + 1;
    let inner = &assigned[inner_start..];
    let inner_end = inner.find(close)?;
    let values = &inner[..inner_end];
    let mut exported = HashSet::new();

    for entry in values.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if let Some(value) = extract_quoted_literal(entry) {
            exported.insert(value);
        }
    }

    Some(exported)
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
            worktree_store: RwLock::new(None),
            buffer_snapshots: BufferSnapshotStore::new(),

            file_cache: RwLock::new(HashMap::new()),
            index_health: RwLock::new(IndexHealthSnapshot::default()),
        })
    }

    pub fn set_worktree_store(&self, store: Arc<WorktreeStore>) {
        *self.worktree_store.write().unwrap() = Some(store);
    }

    pub fn index_health_snapshot(&self) -> IndexHealthSnapshot {
        self.index_health.read().unwrap().clone()
    }

    pub fn set_index_health(&self, health: IndexHealthSnapshot) {
        *self.index_health.write().unwrap() = health;
    }

    pub fn audit_index_health(&self) -> Result<IndexHealthSnapshot, LanguageError> {
        let started = std::time::Instant::now();
        let supported_files = self.supported_language_files(".");
        let supported_set = supported_files.iter().cloned().collect::<HashSet<_>>();
        let indexed_files = self.symbol_store.list_all_indexed_files()?;
        let indexed_map = indexed_files
            .iter()
            .map(|record| (record.file_path.clone(), record.clone()))
            .collect::<HashMap<_, _>>();
        let mut stale_files = 0usize;
        let mut missing_files = 0usize;
        let mut orphaned_files = 0usize;

        for file_path in &supported_files {
            let Some(record) = indexed_map.get(file_path) else {
                missing_files += 1;
                continue;
            };
            let resolved = self.resolve_path(file_path);
            let current_hash = std::fs::read_to_string(resolved)
                .map(|content| compute_hash(&content))
                .unwrap_or_default();
            if current_hash.is_empty() || current_hash != record.file_hash {
                stale_files += 1;
            }
        }

        for record in &indexed_files {
            if !supported_set.contains(&record.file_path) {
                orphaned_files += 1;
            }
        }

        let queued_files = stale_files + missing_files;
        let status = if supported_files.is_empty() {
            IndexHealthStatus::Fresh
        } else if indexed_files.is_empty() {
            IndexHealthStatus::Partial
        } else if queued_files > 0 || orphaned_files > 0 {
            IndexHealthStatus::Stale
        } else {
            IndexHealthStatus::Fresh
        };
        let message = match status {
            IndexHealthStatus::Fresh => "Code intelligence ready".to_string(),
            IndexHealthStatus::Partial => {
                format!("Code intelligence partial: {} files pending", queued_files)
            }
            IndexHealthStatus::Stale => {
                format!(
                    "Refreshing code intelligence: {} files pending",
                    queued_files
                )
            }
            _ => "Checking symbol index".to_string(),
        };

        Ok(IndexHealthSnapshot {
            status,
            indexed_files: indexed_files.len(),
            supported_files: supported_files.len(),
            stale_files,
            missing_files,
            orphaned_files,
            queued_files,
            active_workers: 0,
            symbol_count: self.symbol_store.count()?,
            last_full_scan_ms: Some(started.elapsed().as_millis() as u64),
            last_incremental_update_ms: None,
            message,
        })
    }

    pub fn reconcile_index(&self) -> Result<IndexReconciliationReport, LanguageError> {
        self.reconcile_index_with_progress(|_| {})
    }

    pub fn reconcile_index_with_progress<F>(
        &self,
        mut progress: F,
    ) -> Result<IndexReconciliationReport, LanguageError>
    where
        F: FnMut(&IndexHealthSnapshot),
    {
        let started = std::time::Instant::now();
        let mut health = self.audit_index_health()?;
        health.status = IndexHealthStatus::Checking;
        health.message = "Checking symbol index".to_string();
        self.set_index_health(health.clone());
        progress(&health);

        let supported_files = self.supported_language_files(".");
        let supported_set = supported_files.iter().cloned().collect::<HashSet<_>>();
        let indexed_files = self.symbol_store.list_all_indexed_files()?;
        let indexed_map = indexed_files
            .iter()
            .map(|record| (record.file_path.clone(), record.clone()))
            .collect::<HashMap<_, _>>();
        let mut queued_files = Vec::new();
        let mut files_removed = 0usize;

        for record in &indexed_files {
            if !supported_set.contains(&record.file_path) {
                self.remove_file(&record.file_path)?;
                files_removed += 1;
            }
        }

        for file_path in supported_files {
            let needs_index = match indexed_map.get(&file_path) {
                Some(record) => {
                    let current_hash = std::fs::read_to_string(self.resolve_path(&file_path))
                        .map(|content| compute_hash(&content))
                        .unwrap_or_default();
                    current_hash.is_empty() || current_hash != record.file_hash
                }
                None => true,
            };
            if needs_index {
                queued_files.push(file_path);
            }
        }

        let total_queued = queued_files.len();
        let mut files_indexed = 0usize;
        if total_queued > 0 || files_removed > 0 {
            health.status = IndexHealthStatus::Indexing;
            health.queued_files = total_queued;
            health.active_workers = usize::from(total_queued > 0);
            health.message = format!("Building symbol index... 0/{} files", total_queued);
            self.set_index_health(health.clone());
            progress(&health);
        }

        for file_path in queued_files {
            match self.index_file(&file_path) {
                Ok(_) => {
                    files_indexed += 1;
                }
                Err(error) => {
                    eprintln!("[LanguageService] Failed to index {}: {}", file_path, error);
                }
            }
            health.queued_files = total_queued.saturating_sub(files_indexed);
            health.active_workers = usize::from(health.queued_files > 0);
            health.message = format!(
                "Building symbol index... {}/{} files",
                files_indexed, total_queued
            );
            self.set_index_health(health.clone());
            progress(&health);
        }

        let mut final_health = self.audit_index_health()?;
        final_health.last_full_scan_ms = Some(started.elapsed().as_millis() as u64);
        final_health.last_incremental_update_ms = Some(started.elapsed().as_millis() as u64);
        final_health.active_workers = 0;
        final_health.queued_files = final_health.stale_files + final_health.missing_files;
        final_health.status = if final_health.queued_files == 0 && final_health.orphaned_files == 0
        {
            IndexHealthStatus::Fresh
        } else {
            IndexHealthStatus::Partial
        };
        final_health.message = if final_health.status == IndexHealthStatus::Fresh {
            "Code intelligence ready".to_string()
        } else {
            format!(
                "Code intelligence partial: {} files pending",
                final_health.queued_files
            )
        };
        self.set_index_health(final_health.clone());
        progress(&final_health);

        Ok(IndexReconciliationReport {
            health: final_health,
            files_indexed,
            files_removed,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }

    pub fn get_buffer_snapshot(
        &self,
        file_path: &str,
    ) -> Result<Arc<BufferSnapshot>, LanguageError> {
        self.load_buffer_snapshot(file_path)
    }

    pub fn get_file_content(&self, file_path: &str) -> Result<String, LanguageError> {
        Ok(self.load_buffer_snapshot(file_path)?.to_string())
    }

    pub fn get_cursor_excerpt(
        &self,
        file_path: &str,
        line: u32,
        padding: usize,
    ) -> Result<String, LanguageError> {
        Ok(self
            .load_buffer_snapshot(file_path)?
            .excerpt_around_line(line, padding))
    }

    pub fn get_symbol_byte_range(
        &self,
        symbol: &Symbol,
        file_path: &str,
    ) -> Result<(usize, usize), LanguageError> {
        self.load_buffer_snapshot(file_path)?
            .symbol_byte_range(symbol)
            .map_err(LanguageError::Index)
    }

    pub fn get_symbol_identifier_byte_range(
        &self,
        symbol: &Symbol,
        file_path: &str,
    ) -> Result<(usize, usize), LanguageError> {
        let snapshot = self.load_buffer_snapshot(file_path)?;
        let symbol_start = symbol.byte_offset;
        let symbol_end = symbol
            .byte_offset
            .saturating_add(symbol.byte_length)
            .min(snapshot.content().len());

        if symbol_start >= symbol_end {
            return self.get_symbol_byte_range(symbol, file_path);
        }

        let symbol_span = &snapshot.content()[symbol_start..symbol_end];
        if let Some(relative_start) = symbol_span.find(&symbol.name) {
            let start = symbol_start + relative_start;
            let end = start + symbol.name.len();
            return Ok((start, end));
        }

        self.get_symbol_byte_range(symbol, file_path)
    }

    pub fn get_symbol_excerpt(
        &self,
        symbol: &Symbol,
        file_path: &str,
    ) -> Result<String, LanguageError> {
        let snapshot = self.load_buffer_snapshot(file_path)?;
        let (start, end) = if symbol.byte_length > 0 {
            let start = symbol.byte_offset.min(snapshot.content().len());
            let end = symbol
                .byte_offset
                .saturating_add(symbol.byte_length)
                .min(snapshot.content().len());
            if start < end {
                (start, end)
            } else {
                self.get_symbol_byte_range(symbol, file_path)?
            }
        } else {
            self.get_symbol_byte_range(symbol, file_path)?
        };

        Ok(snapshot.content()[start..end].to_string())
    }

    pub fn get_symbol_inner_byte_range(
        &self,
        symbol: &Symbol,
        file_path: &str,
    ) -> Result<(usize, usize), LanguageError> {
        let snapshot = self.load_buffer_snapshot(file_path)?;
        let symbol_start = symbol.byte_offset.min(snapshot.content().len());
        let symbol_end = symbol
            .byte_offset
            .saturating_add(symbol.byte_length)
            .min(snapshot.content().len());

        if symbol_start >= symbol_end {
            return self.get_symbol_byte_range(symbol, file_path);
        }

        let symbol_span = &snapshot.content()[symbol_start..symbol_end];
        let body_start = symbol_span
            .find('{')
            .map(|idx| symbol_start + idx + 1)
            .or_else(|| symbol_span.find(':').map(|idx| symbol_start + idx + 1));
        let body_end = symbol_span
            .rfind('}')
            .map(|idx| symbol_start + idx)
            .or_else(|| symbol_span.rfind('\n').map(|idx| symbol_start + idx));

        match (body_start, body_end) {
            (Some(start), Some(end)) if start <= end => Ok((start, end)),
            _ => self.get_symbol_byte_range(symbol, file_path),
        }
    }

    pub fn get_line_byte_range(
        &self,
        file_path: &str,
        start_line: u32,
        end_line: u32,
    ) -> Result<(usize, usize), LanguageError> {
        self.load_buffer_snapshot(file_path)?
            .line_byte_range(start_line, end_line)
            .map_err(LanguageError::Index)
    }

    // =========================================================================
    // Symbol Operations (Tree-sitter + Index)
    // =========================================================================

    /// Index a single file
    pub fn index_file(&self, file_path: &str) -> Result<Vec<Symbol>, LanguageError> {
        let snapshot = self.load_snapshot_for_indexing(file_path)?;
        let content = snapshot.content();
        let hash = snapshot.hash().to_string();

        // Check if reindexing is needed
        if !self.symbol_store.needs_reindex(file_path, &hash)? {
            // Return cached symbols from database
            let symbols = self.get_file_symbols_raw(file_path)?;
            return Ok(self.filter_visible_symbols(file_path, symbols));
        }

        // Detect language and parse
        let language = snapshot
            .language()
            .or_else(|| Language::from_path(file_path))
            .ok_or_else(|| {
                LanguageError::NotSupported(format!("Unknown language for: {}", file_path))
            })?;

        // Extract symbols
        let (extracted_symbols, mut relationships) = if matches!(language, Language::Markdown) {
            (
                extract_markdown_header_symbols(file_path, &content),
                Vec::new(),
            )
        } else {
            let tree = {
                let mut parser = self.parser.lock().unwrap();
                parser
                    .parse(&content, language)
                    .map_err(|e| LanguageError::Parse(e.to_string()))?
            };
            let extracted_symbols = extract_symbols(&tree, &content, language, file_path);
            let relationships = extract_symbol_relationships(
                &tree,
                &content,
                language,
                file_path,
                &extracted_symbols,
            );
            (extracted_symbols, relationships)
        };
        let symbols = self.with_file_root_symbol(file_path, &content, extracted_symbols);
        self.canonicalize_import_relationships(file_path, &mut relationships);
        self.append_module_export_relationships(
            file_path,
            &content,
            language,
            &symbols,
            &mut relationships,
        );

        // Delete old symbols and insert new ones
        self.symbol_store.delete_file_symbols(file_path)?;
        self.symbol_store.upsert_symbols(&symbols)?;
        self.resolve_relationship_targets(file_path, &symbols, &mut relationships)?;
        self.symbol_store
            .replace_relationships_for_file(file_path, &relationships)?;
        self.symbol_store
            .mark_file_indexed(file_path, &hash, symbols.len())?;

        // Update cache
        {
            let mut cache = self.file_cache.write().unwrap();
            cache.insert(
                file_path.to_string(),
                CachedFile {
                    hash,
                    _snapshot: snapshot,
                    symbols: symbols.clone(),
                },
            );
        }

        Ok(self.filter_visible_symbols(file_path, symbols))
    }

    /// Index an entire directory recursively
    pub fn index_directory(&self, dir_path: &str) -> Result<IndexStats, LanguageError> {
        let mut stats = IndexStats::default();
        let start = std::time::Instant::now();

        if let Some(store) = self.worktree_store.read().unwrap().clone() {
            for relative_path in store.supported_language_files(dir_path) {
                match self.index_file(&relative_path) {
                    Ok(symbols) => {
                        stats.files_indexed += 1;
                        stats.symbols_extracted += symbols.len();
                    }
                    Err(_) => {
                        stats.files_failed += 1;
                    }
                }
            }
        } else {
            let full_path = self.resolve_path(dir_path);

            // Create gitignore filter if enabled
            let gitignore_filter = self.create_gitignore_filter();

            self.index_directory_recursive(&full_path, "", &mut stats, gitignore_filter.as_ref())?;
        }

        stats.duration_ms = start.elapsed().as_millis() as u64;
        Ok(stats)
    }

    /// Create a GitignoreFilter if gitignore filtering is enabled
    fn create_gitignore_filter(&self) -> Option<GitignoreFilter> {
        let settings = project_settings::load_project_settings_or_default(&self.workspace_root);

        // If allow_gitignored_files is true, don't create a filter (allow all files)
        if settings.allow_gitignored_files {
            return None;
        }

        // Create filter to respect .gitignore
        Some(GitignoreFilter::new(&self.workspace_root))
    }

    fn index_directory_recursive(
        &self,
        base_path: &Path,
        relative_path: &str,
        stats: &mut IndexStats,
        gitignore_filter: Option<&GitignoreFilter>,
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

            // Skip hidden files/dirs (always skip .git regardless of gitignore setting)
            if file_name.starts_with('.') {
                continue;
            }

            // Check gitignore filter
            if let Some(filter) = gitignore_filter {
                if filter.should_ignore(&path) {
                    continue;
                }
            }

            let relative = if relative_path.is_empty() {
                file_name.to_string()
            } else {
                format!("{}/{}", relative_path, file_name)
            };

            if path.is_dir() {
                self.index_directory_recursive(base_path, &relative, stats, gitignore_filter)?;
            } else if path.is_file() {
                // Check if it's a supported language
                if Language::from_path(&relative).is_some() {
                    match self.index_file(&relative) {
                        Ok(symbols) => {
                            stats.files_indexed += 1;
                            stats.symbols_extracted += symbols.len();
                        }
                        Err(_) => {
                            stats.files_failed += 1;
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
        Ok(self.filter_visible_search_results(results))
    }

    pub fn search_symbols_contextual(
        &self,
        query: &str,
        limit: usize,
        active_file: Option<&str>,
        preferred_files: &[String],
    ) -> Result<Vec<SearchResult>, LanguageError> {
        let mut search_query = SearchQuery::text(query).with_limit(limit);
        let preferred_directories = crate::symbol_index::search::collect_preferred_directories(
            active_file,
            preferred_files,
        );

        if let Some(path) = active_file {
            search_query = search_query.with_active_file(path);
        }

        if !preferred_files.is_empty() {
            search_query = search_query.with_preferred_files(preferred_files.to_vec());
        }

        if !preferred_directories.is_empty() {
            search_query = search_query.with_preferred_directories(preferred_directories);
        }

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
        if let Some(path) = file_path {
            self.ensure_file_fresh(path)?;
        }

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
        self.ensure_file_fresh(file_path)?;
        let symbol = self
            .symbol_store
            .get_symbol_at(file_path, line, character)?
            .and_then(|symbol| self.normalize_visible_symbol(file_path, symbol));
        Ok(symbol)
    }

    pub fn get_symbol(&self, id: &str) -> Result<Option<Symbol>, LanguageError> {
        Ok(self.symbol_store.get_symbol(id)?)
    }

    pub fn get_file_module_symbol(&self, file_path: &str) -> Result<Option<Symbol>, LanguageError> {
        self.ensure_file_fresh(file_path)?;
        Ok(self
            .symbol_store
            .get_symbol(&Self::synthetic_file_root_id(file_path))?)
    }

    pub fn get_relationship_targets(
        &self,
        source_symbol_id: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<String>, LanguageError> {
        Ok(self.symbol_store.get_relationship_targets(
            source_symbol_id,
            relationship_type,
            limit,
        )?)
    }

    pub fn get_file_relationship_targets(
        &self,
        source_file_path: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<String>, LanguageError> {
        Ok(self.symbol_store.get_file_relationship_targets(
            source_file_path,
            relationship_type,
            limit,
        )?)
    }

    pub fn find_references_to_symbol(
        &self,
        symbol: &Symbol,
        limit: usize,
    ) -> Result<Vec<SymbolReference>, LanguageError> {
        let expanded_limit = limit.saturating_mul(8).max(limit);
        let mut references = self.find_relationship_references_to_symbol(
            symbol,
            SymbolRelationshipType::Call,
            limit,
        )?;
        let mut seen = references
            .iter()
            .map(|reference| {
                (
                    reference.source_symbol.id.clone(),
                    reference.relationship_type,
                    reference.line,
                )
            })
            .collect::<HashSet<_>>();

        if references.len() < limit {
            for reference in self.symbol_store.find_references_to_target(
                &symbol.file_path,
                SymbolRelationshipType::Import,
                expanded_limit,
            )? {
                let key = (
                    reference.source_symbol.id.clone(),
                    reference.relationship_type,
                    reference.line,
                );

                if seen.insert(key) {
                    references.push(reference);
                    if references.len() >= limit {
                        break;
                    }
                }
            }
        }

        Ok(references)
    }

    /// Get all symbols in a file
    pub fn get_file_symbols(&self, file_path: &str) -> Result<Vec<Symbol>, LanguageError> {
        self.ensure_file_fresh(file_path)?;
        let symbols = self.get_file_symbols_raw(file_path)?;
        Ok(self.filter_visible_symbols(file_path, symbols))
    }

    // =========================================================================
    // Document Synchronization
    // =========================================================================

    /// Notify that a document was opened
    pub fn did_open(&self, file_path: &str, content: &str) -> Result<(), LanguageError> {
        let snapshot_key = self.snapshot_key(file_path);
        self.buffer_snapshots
            .upsert_live(&snapshot_key, None, content);

        if should_allow_non_indexed_live_sync(file_path) {
            return Ok(());
        }

        // Index the file
        let _ = self.index_file_content(file_path, None, content)?;

        Ok(())
    }

    /// Notify that a document changed
    pub fn did_change(
        &self,
        file_path: &str,
        version: i32,
        content: &str,
    ) -> Result<(), LanguageError> {
        let snapshot_key = self.snapshot_key(file_path);
        self.buffer_snapshots
            .upsert_live(&snapshot_key, Some(version), content);

        if should_allow_non_indexed_live_sync(file_path) {
            return Ok(());
        }

        // Re-index the file
        let _ = self.index_file_content(file_path, Some(version), content)?;

        Ok(())
    }

    /// Notify that a document was closed
    pub fn did_close(&self, file_path: &str) -> Result<(), LanguageError> {
        // Remove from cache
        {
            let mut cache = self.file_cache.write().unwrap();
            cache.remove(file_path);
        }
        self.buffer_snapshots.remove(&self.snapshot_key(file_path));

        Ok(())
    }

    /// Remove a file from the symbol index and cache
    pub fn remove_file(&self, file_path: &str) -> Result<(), LanguageError> {
        {
            let mut cache = self.file_cache.write().unwrap();
            cache.remove(file_path);
        }
        self.buffer_snapshots.remove(&self.snapshot_key(file_path));

        self.symbol_store.delete_file_symbols(file_path)?;
        self.symbol_store.delete_indexed_file(file_path)?;

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

    fn canonicalize_import_relationships(
        &self,
        file_path: &str,
        relationships: &mut [SymbolRelationship],
    ) {
        for relationship in relationships.iter_mut() {
            if relationship.relationship_type != SymbolRelationshipType::Import {
                continue;
            }

            if let Some(resolved) = self.resolve_import_target(file_path, &relationship.target_name)
            {
                relationship.target_name = resolved;
            }
        }
    }

    fn resolve_import_target(&self, file_path: &str, import_target: &str) -> Option<String> {
        if import_target.is_empty() {
            return None;
        }

        let direct = self.resolve_path(import_target);
        if direct.exists() {
            return Some(self.path_to_workspace_relative(&direct));
        }

        let base_file = self.resolve_path(file_path);
        let parent = base_file.parent()?;

        if import_target.starts_with('.') {
            let normalized = parent.join(import_target);
            return self.find_existing_import_candidate(&normalized);
        }

        if import_target.contains("::") {
            let crate_relative = import_target
                .trim_start_matches("crate::")
                .trim_start_matches("self::")
                .trim_start_matches("super::")
                .replace("::", "/");
            return self.find_existing_import_candidate(&self.resolve_path(&crate_relative));
        }

        if import_target.contains('.') {
            let dotted = import_target.replace('.', "/");
            return self.find_existing_import_candidate(&self.resolve_path(&dotted));
        }

        None
    }

    fn find_existing_import_candidate(&self, base_path: &Path) -> Option<String> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        if base_path.extension().is_some() {
            candidates.push(base_path.to_path_buf());
        } else {
            for extension in ["ts", "tsx", "js", "jsx", "py", "rs", "go"] {
                candidates.push(base_path.with_extension(extension));
            }

            for index_name in [
                "index.ts",
                "index.tsx",
                "index.js",
                "index.jsx",
                "main.go",
                "mod.rs",
                "__init__.py",
            ] {
                candidates.push(base_path.join(index_name));
            }
        }

        candidates.into_iter().find_map(|candidate| {
            candidate
                .exists()
                .then(|| self.path_to_workspace_relative(&candidate))
        })
    }

    fn path_to_workspace_relative(&self, path: &Path) -> String {
        match path.strip_prefix(&self.workspace_root) {
            Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
            Err(_) => path.to_string_lossy().replace('\\', "/"),
        }
    }

    fn ensure_file_fresh(&self, file_path: &str) -> Result<(), LanguageError> {
        if self
            .buffer_snapshots
            .contains_live(&self.snapshot_key(file_path))
        {
            return Ok(());
        }

        let resolved = self.resolve_path(file_path);
        if !resolved.exists() {
            self.remove_file(file_path)?;
            return Ok(());
        }

        if resolved.is_file() && Language::from_path(file_path).is_some() {
            let _ = self.index_file(file_path)?;
        }

        Ok(())
    }

    fn ensure_scope_index_fresh(
        &self,
        scope_root: Option<&Path>,
        probe_limit: usize,
    ) -> Result<(), LanguageError> {
        let indexed_files = self.symbol_store.list_indexed_files(probe_limit.max(32))?;
        let scope_has_any = match scope_root {
            Some(scope_root) => indexed_files
                .iter()
                .any(|record| self.resolve_path(&record.file_path).starts_with(scope_root)),
            None => !indexed_files.is_empty(),
        };

        if scope_has_any {
            return Ok(());
        }

        match scope_root {
            Some(scope_root) => {
                let relative = match scope_root.strip_prefix(&self.workspace_root) {
                    Ok(path) => path.to_string_lossy().replace('\\', "/"),
                    Err(_) => String::new(),
                };
                let _ = self.index_directory(&relative)?;
            }
            None => {
                let _ = self.index_directory("")?;
            }
        }

        Ok(())
    }

    fn supported_language_files(&self, scope: &str) -> Vec<String> {
        if let Some(store) = self.worktree_store.read().unwrap().clone() {
            return store.supported_language_files(scope);
        }

        let mut files = Vec::new();
        let root = self.resolve_path(scope);
        let scope_prefix = if scope.is_empty() || scope == "." {
            String::new()
        } else {
            scope.trim_matches('/').to_string()
        };
        self.collect_supported_language_files_recursive(&root, &scope_prefix, &mut files);
        files
    }

    fn collect_supported_language_files_recursive(
        &self,
        dir_path: &Path,
        relative_path: &str,
        files: &mut Vec<String>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir_path) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.')
                || matches!(
                    file_name.as_str(),
                    "node_modules" | "target" | "dist" | "build" | "vendor"
                )
            {
                continue;
            }

            let relative = if relative_path.is_empty() {
                file_name
            } else {
                format!("{}/{}", relative_path, file_name)
            };

            if path.is_dir() {
                self.collect_supported_language_files_recursive(&path, &relative, files);
            } else if path.is_file() && Language::from_path(&relative).is_some() {
                files.push(relative);
            }
        }
    }

    fn refresh_stale_indexed_files(
        &self,
        records: &[crate::symbol_index::store::IndexedFileRecord],
    ) -> Result<(), LanguageError> {
        for record in records {
            let resolved = self.resolve_path(&record.file_path);
            if !resolved.exists() {
                self.remove_file(&record.file_path)?;
                continue;
            }
            if !resolved.is_file() || Language::from_path(&record.file_path).is_none() {
                continue;
            }

            let modified_after_index = resolved
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64 > record.indexed_at)
                .unwrap_or(false);

            if modified_after_index {
                let _ = self.index_file(&record.file_path)?;
            }
        }

        Ok(())
    }

    fn reference_matches_symbol(
        &self,
        reference: &SymbolReference,
        symbol: &Symbol,
    ) -> Result<bool, LanguageError> {
        match reference.relationship_type {
            SymbolRelationshipType::Import => Ok(reference.target_name == symbol.file_path),
            SymbolRelationshipType::Call
            | SymbolRelationshipType::Export
            | SymbolRelationshipType::Extends
            | SymbolRelationshipType::Implements => {
                if reference.target_symbol_id.as_deref() == Some(symbol.id.as_str()) {
                    return Ok(true);
                }

                let resolved = self.resolve_reference_symbols_in_neighborhood(
                    &reference.target_name,
                    &reference.source_symbol.file_path,
                )?;
                Ok(resolved.iter().any(|candidate| candidate.id == symbol.id))
            }
            SymbolRelationshipType::Contains => {
                Ok(reference.target_symbol_id.as_deref() == Some(symbol.id.as_str()))
            }
        }
    }

    fn resolve_reference_symbols_in_neighborhood(
        &self,
        reference_name: &str,
        file_path: &str,
    ) -> Result<Vec<Symbol>, LanguageError> {
        let mut resolved = Vec::new();
        let mut seen = HashSet::new();
        let file_symbols = self.get_file_symbols(file_path)?;

        self.collect_matching_symbols(&file_symbols, reference_name, &mut resolved, &mut seen);

        for imported_file in
            self.get_file_relationship_targets(file_path, SymbolRelationshipType::Import, 24)?
        {
            let imported_symbols = self.get_file_symbols(&imported_file)?;
            self.collect_matching_symbols(
                &imported_symbols,
                reference_name,
                &mut resolved,
                &mut seen,
            );
        }

        Ok(resolved)
    }

    fn find_relationship_references_to_symbol(
        &self,
        symbol: &Symbol,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<SymbolReference>, LanguageError> {
        let mut references = Vec::new();
        let mut seen = HashSet::new();
        let expanded_limit = limit.saturating_mul(8).max(limit);

        for reference in self.symbol_store.find_references_to_symbol_id(
            &symbol.id,
            relationship_type,
            expanded_limit,
        )? {
            let key = (
                reference.source_symbol.id.clone(),
                reference.relationship_type,
                reference.line,
            );

            if seen.insert(key) {
                references.push(reference);
                if references.len() >= limit {
                    return Ok(references);
                }
            }
        }

        for reference in self.symbol_store.find_references_to_target(
            &symbol.name,
            relationship_type,
            expanded_limit,
        )? {
            if !self.reference_matches_symbol(&reference, symbol)? {
                continue;
            }

            let key = (
                reference.source_symbol.id.clone(),
                reference.relationship_type,
                reference.line,
            );

            if seen.insert(key) {
                references.push(reference);
                if references.len() >= limit {
                    return Ok(references);
                }
            }
        }

        Ok(references)
    }

    fn resolve_relationship_targets(
        &self,
        file_path: &str,
        file_symbols: &[Symbol],
        relationships: &mut [SymbolRelationship],
    ) -> Result<(), LanguageError> {
        let imported_files = relationships
            .iter()
            .filter(|relationship| relationship.relationship_type == SymbolRelationshipType::Import)
            .map(|relationship| relationship.target_name.clone())
            .collect::<Vec<_>>();

        for relationship in relationships.iter_mut() {
            if relationship.relationship_type == SymbolRelationshipType::Import {
                continue;
            }

            if relationship.target_symbol_id.is_some() {
                continue;
            }

            relationship.target_symbol_id = self.resolve_relationship_symbol_id(
                &relationship.target_name,
                file_path,
                file_symbols,
                &imported_files,
            )?;
        }

        Ok(())
    }

    fn resolve_relationship_symbol_id(
        &self,
        reference_name: &str,
        file_path: &str,
        file_symbols: &[Symbol],
        imported_files: &[String],
    ) -> Result<Option<String>, LanguageError> {
        let mut same_file = Vec::new();
        let mut seen = HashSet::new();
        self.collect_matching_symbols(file_symbols, reference_name, &mut same_file, &mut seen);

        if same_file.len() == 1 {
            return Ok(Some(same_file[0].id.clone()));
        }
        if same_file.len() > 1 {
            return Ok(None);
        }

        let mut imported_matches = Vec::new();
        let mut imported_seen = HashSet::new();
        for imported_file in imported_files {
            let imported_symbols = self.get_file_symbols(imported_file)?;
            self.collect_matching_symbols(
                &imported_symbols,
                reference_name,
                &mut imported_matches,
                &mut imported_seen,
            );
        }

        if imported_matches.len() == 1 {
            return Ok(Some(imported_matches[0].id.clone()));
        }
        if imported_matches.len() > 1 {
            return Ok(None);
        }

        let preferred_files = imported_files.to_vec();
        let contextual =
            self.search_symbols_contextual(reference_name, 8, Some(file_path), &preferred_files)?;
        let exact = contextual
            .into_iter()
            .filter(|result| {
                result.symbol.name == reference_name
                    && result.symbol.symbol_type != SymbolType::Import
            })
            .map(|result| result.symbol)
            .collect::<Vec<_>>();

        if exact.len() == 1 {
            Ok(Some(exact[0].id.clone()))
        } else {
            Ok(None)
        }
    }

    fn collect_matching_symbols(
        &self,
        symbols: &[Symbol],
        reference_name: &str,
        resolved: &mut Vec<Symbol>,
        seen: &mut HashSet<String>,
    ) {
        for symbol in symbols {
            if symbol.name != reference_name
                || symbol.symbol_type == SymbolType::Import
                || Self::is_synthetic_file_root_symbol(symbol)
            {
                continue;
            }

            if seen.insert(symbol.id.clone()) {
                resolved.push(symbol.clone());
            }
        }
    }

    fn index_file_content(
        &self,
        file_path: &str,
        version: Option<i32>,
        content: &str,
    ) -> Result<Vec<Symbol>, LanguageError> {
        let snapshot =
            self.buffer_snapshots
                .upsert_live(&self.snapshot_key(file_path), version, content);
        let hash = snapshot.hash().to_string();

        // Check cache first
        {
            let cache = self.file_cache.read().unwrap();
            if let Some(cached) = cache.get(file_path) {
                if cached.hash == hash {
                    return Ok(self.filter_visible_symbols(file_path, cached.symbols.clone()));
                }
            }
        }

        // Detect language and parse
        let language = snapshot
            .language()
            .or_else(|| Language::from_path(file_path))
            .ok_or_else(|| {
                LanguageError::NotSupported(format!("Unknown language for: {}", file_path))
            })?;

        // Extract symbols
        let (extracted_symbols, mut relationships) = if matches!(language, Language::Markdown) {
            (
                extract_markdown_header_symbols(file_path, snapshot.content()),
                Vec::new(),
            )
        } else {
            let tree = {
                let mut parser = self.parser.lock().unwrap();
                parser
                    .parse(snapshot.content(), language)
                    .map_err(|e| LanguageError::Parse(e.to_string()))?
            };
            let extracted_symbols = extract_symbols(&tree, snapshot.content(), language, file_path);
            let relationships = extract_symbol_relationships(
                &tree,
                snapshot.content(),
                language,
                file_path,
                &extracted_symbols,
            );
            (extracted_symbols, relationships)
        };
        let symbols = self.with_file_root_symbol(file_path, snapshot.content(), extracted_symbols);
        self.canonicalize_import_relationships(file_path, &mut relationships);
        self.append_module_export_relationships(
            file_path,
            snapshot.content(),
            language,
            &symbols,
            &mut relationships,
        );

        // Delete old symbols and insert new ones
        self.symbol_store.delete_file_symbols(file_path)?;
        self.symbol_store.upsert_symbols(&symbols)?;
        self.resolve_relationship_targets(file_path, &symbols, &mut relationships)?;
        self.symbol_store
            .replace_relationships_for_file(file_path, &relationships)?;
        self.symbol_store
            .mark_file_indexed(file_path, &hash, symbols.len())?;

        // Update cache
        {
            let mut cache = self.file_cache.write().unwrap();
            cache.insert(
                file_path.to_string(),
                CachedFile {
                    hash,
                    _snapshot: snapshot,
                    symbols: symbols.clone(),
                },
            );
        }

        Ok(symbols)
    }

    fn snapshot_key(&self, file_path: &str) -> String {
        let path = Path::new(file_path);
        if path.is_absolute() {
            self.path_to_workspace_relative(path)
        } else {
            file_path.replace('\\', "/")
        }
    }

    fn load_snapshot_for_indexing(
        &self,
        file_path: &str,
    ) -> Result<Arc<BufferSnapshot>, LanguageError> {
        let key = self.snapshot_key(file_path);
        if let Some(snapshot) = self.buffer_snapshots.get(&key) {
            if snapshot.is_live() {
                return Ok(snapshot);
            }
        }

        let full_path = self.resolve_path(file_path);
        let content = std::fs::read_to_string(&full_path)?;
        Ok(self.buffer_snapshots.upsert_disk(&key, &content))
    }

    fn load_buffer_snapshot(&self, file_path: &str) -> Result<Arc<BufferSnapshot>, LanguageError> {
        let key = self.snapshot_key(file_path);
        if let Some(snapshot) = self.buffer_snapshots.get(&key) {
            return Ok(snapshot);
        }

        let full_path = self.resolve_path(file_path);
        let content = std::fs::read_to_string(&full_path)?;
        Ok(self.buffer_snapshots.upsert_disk(&key, &content))
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

    pub fn build_semantic_project_overview(
        &self,
        scope_root: Option<&Path>,
        max_modules: usize,
        max_symbols_per_module: usize,
    ) -> Result<Option<String>, LanguageError> {
        let scope_root = scope_root.and_then(|path| std::fs::canonicalize(path).ok());
        self.ensure_scope_index_fresh(
            scope_root.as_deref(),
            max_modules.saturating_mul(8).max(64),
        )?;
        let stats = self.stats()?;
        let mut indexed_files = self
            .symbol_store
            .list_indexed_files(max_modules.saturating_mul(8).max(64))?;

        if let Some(scope_root) = scope_root.as_ref() {
            indexed_files
                .retain(|record| self.resolve_path(&record.file_path).starts_with(scope_root));
        }

        self.refresh_stale_indexed_files(&indexed_files)?;

        indexed_files = self
            .symbol_store
            .list_indexed_files(max_modules.saturating_mul(8).max(64))?;
        if let Some(scope_root) = scope_root.as_ref() {
            indexed_files
                .retain(|record| self.resolve_path(&record.file_path).starts_with(scope_root));
        }

        if indexed_files.is_empty() {
            return Ok(None);
        }

        #[derive(Debug, Clone)]
        struct ModuleSummary {
            file_path: String,
            symbol_count: usize,
            import_count: usize,
            export_count: usize,
            key_symbols: Vec<String>,
            score: usize,
        }

        let mut subsystem_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut subsystem_examples: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut important_modules = Vec::<ModuleSummary>::new();
        let mut entrypoints = Vec::<String>::new();
        let mut notable_tests = Vec::<String>::new();
        let mut notable_configs = Vec::<String>::new();

        for record in indexed_files {
            let subsystem = subsystem_name_for_path(&record.file_path);
            *subsystem_counts.entry(subsystem.clone()).or_insert(0) += 1;
            push_unique_limited(
                subsystem_examples.entry(subsystem).or_default(),
                record.file_path.clone(),
                3,
            );

            let symbols = self.get_file_symbols(&record.file_path).unwrap_or_default();
            let key_symbols = summarize_key_symbols(&symbols, max_symbols_per_module);
            let import_count = self
                .get_file_relationship_targets(
                    &record.file_path,
                    SymbolRelationshipType::Import,
                    64,
                )
                .map(|targets| targets.len())
                .unwrap_or(0);
            let export_count = match self.get_file_module_symbol(&record.file_path)? {
                Some(module_symbol) => self
                    .symbol_store
                    .get_relationship_edges_from_source(
                        &module_symbol.id,
                        SymbolRelationshipType::Export,
                        64,
                    )
                    .map(|references| references.len())
                    .unwrap_or(0),
                None => 0,
            };
            let score =
                record.symbol_count + (import_count * 2) + (export_count * 3) + key_symbols.len();

            if is_probable_entrypoint(&record.file_path, &symbols, export_count) {
                push_unique_limited(&mut entrypoints, record.file_path.clone(), 12);
            }
            if is_test_path(&record.file_path) {
                push_unique_limited(&mut notable_tests, record.file_path.clone(), 12);
            }
            if is_config_path(&record.file_path) {
                push_unique_limited(&mut notable_configs, record.file_path.clone(), 12);
            }

            important_modules.push(ModuleSummary {
                file_path: record.file_path,
                symbol_count: record.symbol_count,
                import_count,
                export_count,
                key_symbols,
                score,
            });
        }

        important_modules.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| b.export_count.cmp(&a.export_count))
                .then_with(|| b.import_count.cmp(&a.import_count))
                .then_with(|| a.file_path.cmp(&b.file_path))
        });
        let important_modules = important_modules
            .into_iter()
            .take(max_modules)
            .collect::<Vec<_>>();

        let project_name = scope_root
            .as_ref()
            .and_then(|path| path.file_name())
            .or_else(|| self.workspace_root.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("project");

        let mut output = String::new();
        output.push_str(&format!(
            "# Semantic Project Overview: {}\n\n",
            project_name
        ));
        output.push_str("## Index Summary\n\n");
        output.push_str(&format!("- Indexed files: {}\n", stats.files_indexed));
        output.push_str(&format!("- Indexed symbols: {}\n", stats.symbols_extracted));
        if let Some(scope_root) = scope_root.as_ref() {
            output.push_str(&format!(
                "- Scope root: {}\n",
                self.path_to_workspace_relative(scope_root)
            ));
        }
        output.push('\n');

        output.push_str("## Major Directories\n\n");
        let mut subsystems = subsystem_counts.into_iter().collect::<Vec<_>>();
        subsystems.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for (name, count) in subsystems.into_iter().take(10) {
            let examples = subsystem_examples.remove(&name).unwrap_or_default();
            if examples.is_empty() {
                output.push_str(&format!("- {} ({} files)\n", name, count));
            } else {
                output.push_str(&format!(
                    "- {} ({} files): {}\n",
                    name,
                    count,
                    examples.join(", ")
                ));
            }
        }
        output.push('\n');

        output.push_str("## Top Entrypoints\n\n");
        if entrypoints.is_empty() {
            output.push_str("- (none inferred from indexed files)\n");
        } else {
            for entry in entrypoints.iter().take(12) {
                output.push_str(&format!("- {}\n", entry));
            }
        }
        output.push('\n');

        output.push_str("## Important Modules\n\n");
        for module in &important_modules {
            output.push_str(&format!("### {}\n\n", module.file_path));
            output.push_str(&format!("- Symbols: {}\n", module.symbol_count));
            output.push_str(&format!("- Imports: {}\n", module.import_count));
            output.push_str(&format!("- Exports: {}\n", module.export_count));
            if module.key_symbols.is_empty() {
                output.push_str("- Key symbols: (none indexed)\n\n");
            } else {
                output.push_str(&format!(
                    "- Key symbols: {}\n\n",
                    module.key_symbols.join(", ")
                ));
            }
        }

        output.push_str("## Notable Tests\n\n");
        if notable_tests.is_empty() {
            output.push_str("- (none surfaced)\n");
        } else {
            for path in notable_tests.iter().take(10) {
                output.push_str(&format!("- {}\n", path));
            }
        }
        output.push('\n');

        output.push_str("## Notable Configs\n\n");
        if notable_configs.is_empty() {
            output.push_str("- (none surfaced)\n");
        } else {
            for path in notable_configs.iter().take(10) {
                output.push_str(&format!("- {}\n", path));
            }
        }
        output.push('\n');

        Ok(Some(output))
    }

    pub fn get_symbol_graph(
        &self,
        symbol: &Symbol,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<SymbolGraph, LanguageError> {
        let incoming = match relationship_type {
            SymbolRelationshipType::Call => self.find_references_to_symbol(symbol, limit)?,
            SymbolRelationshipType::Import => self.symbol_store.find_references_to_target(
                &symbol.file_path,
                SymbolRelationshipType::Import,
                limit,
            )?,
            SymbolRelationshipType::Export
            | SymbolRelationshipType::Extends
            | SymbolRelationshipType::Implements => {
                self.find_relationship_references_to_symbol(symbol, relationship_type, limit)?
            }
            SymbolRelationshipType::Contains => self.get_containment_incoming(symbol)?,
        };
        let outgoing = match relationship_type {
            SymbolRelationshipType::Contains => self.get_containment_outgoing(symbol, limit)?,
            _ => self.symbol_store.get_relationship_edges_from_source(
                &symbol.id,
                relationship_type,
                limit,
            )?,
        };

        Ok(SymbolGraph {
            symbol: symbol.clone(),
            incoming,
            outgoing,
        })
    }

    fn get_containment_incoming(
        &self,
        symbol: &Symbol,
    ) -> Result<Vec<SymbolReference>, LanguageError> {
        let Some(parent_id) = symbol.parent_id.as_deref() else {
            return Ok(Vec::new());
        };

        let Some(parent_symbol) = self.symbol_store.get_symbol(parent_id)? else {
            return Ok(Vec::new());
        };

        Ok(vec![SymbolReference {
            source_symbol: parent_symbol,
            relationship_type: SymbolRelationshipType::Contains,
            target_name: symbol.name.clone(),
            target_symbol_id: Some(symbol.id.clone()),
            target_symbol: Some(symbol.clone()),
            line: symbol.range.start.line,
        }])
    }

    fn get_containment_outgoing(
        &self,
        symbol: &Symbol,
        limit: usize,
    ) -> Result<Vec<SymbolReference>, LanguageError> {
        let children = self
            .get_file_symbols_raw(&symbol.file_path)?
            .into_iter()
            .filter(|candidate| candidate.parent_id.as_deref() == Some(symbol.id.as_str()))
            .take(limit)
            .collect::<Vec<_>>();

        Ok(children
            .into_iter()
            .map(|child| SymbolReference {
                source_symbol: symbol.clone(),
                relationship_type: SymbolRelationshipType::Contains,
                target_name: child.name.clone(),
                target_symbol_id: Some(child.id.clone()),
                target_symbol: Some(child.clone()),
                line: child.range.start.line,
            })
            .collect())
    }

    fn get_file_symbols_raw(&self, file_path: &str) -> Result<Vec<Symbol>, LanguageError> {
        Ok(self.symbol_store.get_symbols_in_file(file_path)?)
    }

    fn resolve_exported_symbol_from_module(
        &self,
        file_path: &str,
        export_name: &str,
    ) -> Result<Option<Symbol>, LanguageError> {
        let Some(module_symbol) = self.get_file_module_symbol(file_path)? else {
            return Ok(None);
        };

        let references = self.symbol_store.get_relationship_edges_from_source(
            &module_symbol.id,
            SymbolRelationshipType::Export,
            256,
        )?;

        Ok(references
            .into_iter()
            .find(|reference| reference.target_name == export_name)
            .and_then(|reference| reference.target_symbol))
    }

    fn get_module_export_references(
        &self,
        file_path: &str,
    ) -> Result<Vec<SymbolReference>, LanguageError> {
        let Some(module_symbol) = self.get_file_module_symbol(file_path)? else {
            return Ok(Vec::new());
        };

        Ok(self.symbol_store.get_relationship_edges_from_source(
            &module_symbol.id,
            SymbolRelationshipType::Export,
            256,
        )?)
    }

    fn resolve_python_module_target(&self, file_path: &str, module_target: &str) -> Option<String> {
        let base_file = self.resolve_path(file_path);
        let parent = base_file.parent()?;

        if module_target.starts_with('.') {
            let depth = module_target
                .chars()
                .take_while(|character| *character == '.')
                .count();
            let remainder = module_target[depth..].trim();
            let mut anchor = parent.to_path_buf();
            for _ in 1..depth {
                anchor = anchor.parent()?.to_path_buf();
            }

            if remainder.is_empty() {
                return self.find_existing_import_candidate(&anchor);
            }

            let normalized = remainder.replace('.', "/");
            return self
                .find_existing_import_candidate(&anchor.join(&normalized))
                .or_else(|| self.find_existing_import_candidate(&self.resolve_path(&normalized)));
        }

        if let Some(resolved) = self.resolve_import_target(file_path, module_target) {
            let resolved_path = self.resolve_path(&resolved);
            if resolved_path.is_dir() {
                return self.find_existing_import_candidate(&resolved_path);
            }
            return Some(resolved);
        }

        let normalized = module_target.replace('.', "/");

        self.find_existing_import_candidate(&parent.join(&normalized))
            .or_else(|| self.find_existing_import_candidate(&self.resolve_path(&normalized)))
    }

    fn append_module_export_relationships(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        symbols: &[Symbol],
        relationships: &mut Vec<SymbolRelationship>,
    ) {
        let root_id = Self::synthetic_file_root_id(file_path);
        let Some(root_symbol) = symbols.iter().find(|symbol| symbol.id == root_id) else {
            return;
        };
        let mut seen = HashSet::new();

        for symbol in symbols {
            if symbol.parent_id.as_deref() != Some(root_id.as_str()) {
                continue;
            }
            if symbol.symbol_type == SymbolType::Import
                || Self::is_synthetic_file_root_symbol(symbol)
            {
                continue;
            }
            let Some(exported_name) = Self::direct_export_name(content, language, symbol) else {
                continue;
            };

            let key = (exported_name.clone(), symbol.id.clone());
            if !seen.insert(key) {
                continue;
            }

            relationships.push(SymbolRelationship {
                source_symbol_id: root_symbol.id.clone(),
                source_file_path: file_path.to_string(),
                target_name: exported_name,
                target_symbol_id: Some(symbol.id.clone()),
                relationship_type: SymbolRelationshipType::Export,
                line: symbol.range.start.line,
            });
        }

        if matches!(
            language,
            Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx
        ) {
            for (local_name, exported_name, module_target, line) in
                typescript_named_export_clauses(content)
            {
                let matching_symbol = if let Some(module_target) = module_target {
                    let Some(resolved_file) = self.resolve_import_target(file_path, &module_target)
                    else {
                        continue;
                    };
                    self.resolve_exported_symbol_from_module(&resolved_file, &local_name)
                        .ok()
                        .flatten()
                        .or_else(|| {
                            let imported_symbols =
                                self.get_file_symbols_raw(&resolved_file).ok()?;
                            imported_symbols.into_iter().find(|symbol| {
                                symbol.name == local_name
                                    && symbol.symbol_type != SymbolType::Import
                                    && !Self::is_synthetic_file_root_symbol(symbol)
                            })
                        })
                } else {
                    symbols
                        .iter()
                        .find(|symbol| {
                            symbol.parent_id.as_deref() == Some(root_id.as_str())
                                && symbol.name == local_name
                                && symbol.symbol_type != SymbolType::Import
                                && !Self::is_synthetic_file_root_symbol(symbol)
                        })
                        .cloned()
                };
                let Some(target_symbol) = matching_symbol else {
                    continue;
                };

                let key = (exported_name.clone(), target_symbol.id.clone());
                if !seen.insert(key) {
                    continue;
                }

                relationships.push(SymbolRelationship {
                    source_symbol_id: root_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: exported_name,
                    target_symbol_id: Some(target_symbol.id.clone()),
                    relationship_type: SymbolRelationshipType::Export,
                    line,
                });
            }

            for (exported_name, module_target, line) in typescript_namespace_export_clauses(content)
            {
                let Some(resolved_file) = self.resolve_import_target(file_path, &module_target)
                else {
                    continue;
                };
                let Some(target_symbol) =
                    self.get_file_module_symbol(&resolved_file).ok().flatten()
                else {
                    continue;
                };

                let key = (exported_name.clone(), target_symbol.id.clone());
                if !seen.insert(key) {
                    continue;
                }

                relationships.push(SymbolRelationship {
                    source_symbol_id: root_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: exported_name,
                    target_symbol_id: Some(target_symbol.id.clone()),
                    relationship_type: SymbolRelationshipType::Export,
                    line,
                });
            }

            for (module_target, line) in typescript_export_star_targets(content) {
                let Some(resolved_file) = self.resolve_import_target(file_path, &module_target)
                else {
                    continue;
                };
                let references = match self.get_module_export_references(&resolved_file) {
                    Ok(references) => references,
                    Err(_) => continue,
                };

                for reference in references {
                    if reference.target_name == "default" {
                        continue;
                    }
                    let Some(target_symbol) = reference.target_symbol else {
                        continue;
                    };

                    let key = (reference.target_name.clone(), target_symbol.id.clone());
                    if !seen.insert(key) {
                        continue;
                    }

                    relationships.push(SymbolRelationship {
                        source_symbol_id: root_symbol.id.clone(),
                        source_file_path: file_path.to_string(),
                        target_name: reference.target_name,
                        target_symbol_id: Some(target_symbol.id.clone()),
                        relationship_type: SymbolRelationshipType::Export,
                        line,
                    });
                }
            }
        }

        if matches!(language, Language::Rust) {
            for (module_path, exported_name, line) in rust_pub_use_plain_module_reexports(content) {
                let Some(resolved_file) = self.resolve_import_target(file_path, &module_path)
                else {
                    continue;
                };
                let Some(target_symbol) =
                    self.get_file_module_symbol(&resolved_file).ok().flatten()
                else {
                    continue;
                };

                let key = (exported_name.clone(), target_symbol.id.clone());
                if !seen.insert(key) {
                    continue;
                }

                relationships.push(SymbolRelationship {
                    source_symbol_id: root_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: exported_name,
                    target_symbol_id: Some(target_symbol.id.clone()),
                    relationship_type: SymbolRelationshipType::Export,
                    line,
                });
            }

            for (module_path, exported_name, line) in rust_grouped_pub_use_module_reexports(content)
            {
                let Some(resolved_file) = self.resolve_import_target(file_path, &module_path)
                else {
                    continue;
                };
                let Some(target_symbol) =
                    self.get_file_module_symbol(&resolved_file).ok().flatten()
                else {
                    continue;
                };

                let key = (exported_name.clone(), target_symbol.id.clone());
                if !seen.insert(key) {
                    continue;
                }

                relationships.push(SymbolRelationship {
                    source_symbol_id: root_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: exported_name,
                    target_symbol_id: Some(target_symbol.id.clone()),
                    relationship_type: SymbolRelationshipType::Export,
                    line,
                });
            }

            for (module_path, exported_name, line) in rust_pub_use_module_reexports(content) {
                let Some(resolved_file) = self.resolve_import_target(file_path, &module_path)
                else {
                    continue;
                };
                let Some(target_symbol) =
                    self.get_file_module_symbol(&resolved_file).ok().flatten()
                else {
                    continue;
                };

                let key = (exported_name.clone(), target_symbol.id.clone());
                if !seen.insert(key) {
                    continue;
                }

                relationships.push(SymbolRelationship {
                    source_symbol_id: root_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: exported_name,
                    target_symbol_id: Some(target_symbol.id.clone()),
                    relationship_type: SymbolRelationshipType::Export,
                    line,
                });
            }

            for (module_path, symbol_name, exported_name, line) in rust_pub_use_reexports(content) {
                let Some(resolved_file) = self.resolve_import_target(file_path, &module_path)
                else {
                    continue;
                };
                let imported_symbols = match self.get_file_symbols_raw(&resolved_file) {
                    Ok(symbols) => symbols,
                    Err(_) => continue,
                };
                let matching_symbol = imported_symbols.into_iter().find(|symbol| {
                    symbol.name == symbol_name
                        && symbol.symbol_type != SymbolType::Import
                        && !Self::is_synthetic_file_root_symbol(symbol)
                });
                let Some(target_symbol) = matching_symbol else {
                    continue;
                };

                let key = (exported_name.clone(), target_symbol.id.clone());
                if !seen.insert(key) {
                    continue;
                }

                relationships.push(SymbolRelationship {
                    source_symbol_id: root_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: exported_name,
                    target_symbol_id: Some(target_symbol.id.clone()),
                    relationship_type: SymbolRelationshipType::Export,
                    line,
                });
            }

            for (module_path, line) in rust_pub_use_glob_reexports(content) {
                let Some(resolved_file) = self.resolve_import_target(file_path, &module_path)
                else {
                    continue;
                };
                let references = match self.get_module_export_references(&resolved_file) {
                    Ok(references) => references,
                    Err(_) => continue,
                };

                for reference in references {
                    let Some(target_symbol) = reference.target_symbol else {
                        continue;
                    };

                    let key = (reference.target_name.clone(), target_symbol.id.clone());
                    if !seen.insert(key) {
                        continue;
                    }

                    relationships.push(SymbolRelationship {
                        source_symbol_id: root_symbol.id.clone(),
                        source_file_path: file_path.to_string(),
                        target_name: reference.target_name,
                        target_symbol_id: Some(target_symbol.id.clone()),
                        relationship_type: SymbolRelationshipType::Export,
                        line,
                    });
                }
            }
        }

        if matches!(language, Language::Python) {
            for (module_target, exported_name, line) in python_import_module_clauses(content) {
                if !python_is_exported_name(content, &exported_name) {
                    continue;
                }
                let Some(resolved_file) =
                    self.resolve_python_module_target(file_path, &module_target)
                else {
                    continue;
                };
                let Some(target_symbol) =
                    self.get_file_module_symbol(&resolved_file).ok().flatten()
                else {
                    continue;
                };

                let key = (exported_name.clone(), target_symbol.id.clone());
                if !seen.insert(key) {
                    continue;
                }

                relationships.push(SymbolRelationship {
                    source_symbol_id: root_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: exported_name,
                    target_symbol_id: Some(target_symbol.id.clone()),
                    relationship_type: SymbolRelationshipType::Export,
                    line,
                });
            }

            for (module_target, local_name, exported_name, line) in
                python_from_import_clauses(content)
            {
                if !python_is_exported_name(content, &exported_name) {
                    continue;
                }
                let Some(resolved_file) =
                    self.resolve_python_module_target(file_path, &module_target)
                else {
                    continue;
                };
                let matching_symbol = self
                    .resolve_exported_symbol_from_module(&resolved_file, &local_name)
                    .ok()
                    .flatten()
                    .or_else(|| {
                        let submodule_target =
                            python_join_module_target(&module_target, &local_name);
                        let submodule_file =
                            self.resolve_python_module_target(file_path, &submodule_target)?;
                        self.get_file_module_symbol(&submodule_file).ok().flatten()
                    })
                    .or_else(|| {
                        let imported_symbols = self.get_file_symbols_raw(&resolved_file).ok()?;
                        imported_symbols.into_iter().find(|symbol| {
                            symbol.name == local_name
                                && symbol.symbol_type != SymbolType::Import
                                && !Self::is_synthetic_file_root_symbol(symbol)
                        })
                    });
                let Some(target_symbol) = matching_symbol else {
                    continue;
                };

                let key = (exported_name.clone(), target_symbol.id.clone());
                if !seen.insert(key) {
                    continue;
                }

                relationships.push(SymbolRelationship {
                    source_symbol_id: root_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: exported_name,
                    target_symbol_id: Some(target_symbol.id.clone()),
                    relationship_type: SymbolRelationshipType::Export,
                    line,
                });
            }

            for (module_target, line) in python_from_import_star_targets(content) {
                let Some(resolved_file) =
                    self.resolve_python_module_target(file_path, &module_target)
                else {
                    continue;
                };
                let references = match self.get_module_export_references(&resolved_file) {
                    Ok(references) => references,
                    Err(_) => continue,
                };

                for reference in references {
                    if !python_is_exported_name(content, &reference.target_name) {
                        continue;
                    }
                    let Some(target_symbol) = reference.target_symbol else {
                        continue;
                    };

                    let key = (reference.target_name.clone(), target_symbol.id.clone());
                    if !seen.insert(key) {
                        continue;
                    }

                    relationships.push(SymbolRelationship {
                        source_symbol_id: root_symbol.id.clone(),
                        source_file_path: file_path.to_string(),
                        target_name: reference.target_name,
                        target_symbol_id: Some(target_symbol.id.clone()),
                        relationship_type: SymbolRelationshipType::Export,
                        line,
                    });
                }
            }
        }
    }

    fn filter_visible_search_results(&self, results: Vec<SearchResult>) -> Vec<SearchResult> {
        results
            .into_iter()
            .filter_map(|mut result| {
                let file_path = result.symbol.file_path.clone();
                result.symbol = self.normalize_visible_symbol(&file_path, result.symbol)?;
                Some(result)
            })
            .collect()
    }

    fn filter_visible_symbols(&self, file_path: &str, symbols: Vec<Symbol>) -> Vec<Symbol> {
        symbols
            .into_iter()
            .filter_map(|symbol| self.normalize_visible_symbol(file_path, symbol))
            .collect()
    }

    fn normalize_visible_symbol(&self, file_path: &str, mut symbol: Symbol) -> Option<Symbol> {
        let root_id = Self::synthetic_file_root_id(file_path);
        if symbol.id == root_id && Self::is_synthetic_file_root_symbol(&symbol) {
            return None;
        }

        if symbol.parent_id.as_deref() == Some(root_id.as_str()) {
            symbol.parent_id = None;
        }

        Some(symbol)
    }

    fn with_file_root_symbol(
        &self,
        file_path: &str,
        content: &str,
        mut symbols: Vec<Symbol>,
    ) -> Vec<Symbol> {
        let root = Self::synthetic_file_root_symbol(file_path, content);
        let root_id = root.id.clone();

        for symbol in symbols.iter_mut() {
            if symbol.parent_id.is_none() {
                symbol.parent_id = Some(root_id.clone());
            }
        }

        symbols.push(root);
        symbols.sort_by_key(|symbol| {
            (
                symbol.range.start.line,
                symbol.range.start.character,
                symbol.byte_offset,
            )
        });
        symbols
    }

    fn synthetic_file_root_symbol(file_path: &str, content: &str) -> Symbol {
        let file_name = Path::new(file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(file_path)
            .to_string();
        let (end_line, end_character) = file_end_position(content);

        Symbol {
            id: Self::synthetic_file_root_id(file_path),
            name: file_name,
            qualified_name: "__file__".to_string(),
            symbol_type: SymbolType::Module,
            file_path: file_path.to_string(),
            range: crate::tree_sitter::Range {
                start: crate::tree_sitter::Position::new(0, 0),
                end: crate::tree_sitter::Position::new(end_line, end_character),
            },
            byte_offset: 0,
            byte_length: content.len(),
            parent_id: None,
            docstring: None,
            signature: None,
            content_hash: compute_hash(content),
        }
    }

    fn synthetic_file_root_id(file_path: &str) -> String {
        format!("{}::__file__#{}", file_path, SymbolType::Module)
    }

    fn is_synthetic_file_root_symbol(symbol: &Symbol) -> bool {
        symbol.symbol_type == SymbolType::Module && symbol.qualified_name == "__file__"
    }

    fn direct_export_name(content: &str, language: Language, symbol: &Symbol) -> Option<String> {
        let line = symbol_line_text(content, symbol.byte_offset).trim_start();
        match language {
            Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
                if line.starts_with("export default ") {
                    Some("default".to_string())
                } else if line.starts_with("export ") {
                    Some(symbol.name.clone())
                } else {
                    None
                }
            }
            Language::Rust => line.starts_with("pub ").then(|| symbol.name.clone()),
            Language::Python => {
                python_is_exported_name(content, &symbol.name).then(|| symbol.name.clone())
            }
            Language::Go => go_is_exported_name(&symbol.name).then(|| symbol.name.clone()),
            Language::Markdown => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SymbolGraph {
    pub symbol: Symbol,
    pub incoming: Vec<SymbolReference>,
    pub outgoing: Vec<SymbolReference>,
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

fn extract_markdown_header_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut byte_offset = 0usize;

    for (line_index, segment) in content.split_inclusive('\n').enumerate() {
        let line_start = byte_offset;
        byte_offset += segment.len();

        let line_without_lf = segment.strip_suffix('\n').unwrap_or(segment);
        let line = line_without_lf
            .strip_suffix('\r')
            .unwrap_or(line_without_lf);

        let Some((level, name)) = parse_markdown_header(line) else {
            continue;
        };

        let line_number = line_index as u32;
        let character_count = line.chars().count() as u32;
        let qualified_name = format!("h{}:{}:{}", level, line_number + 1, name);

        symbols.push(Symbol {
            id: format!("{}::{}#{}", file_path, qualified_name, SymbolType::Heading),
            name,
            qualified_name,
            symbol_type: SymbolType::Heading,
            file_path: file_path.to_string(),
            range: Range {
                start: Position::new(line_number, 0),
                end: Position::new(line_number, character_count),
            },
            byte_offset: line_start,
            byte_length: line.len(),
            parent_id: None,
            docstring: None,
            signature: Some(format!("h{}", level)),
            content_hash: compute_hash(line),
        });
    }

    symbols
}

fn parse_markdown_header(line: &str) -> Option<(usize, String)> {
    let level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&level) {
        return None;
    }

    let after_hashes = &line[level..];
    if after_hashes
        .chars()
        .next()
        .is_some_and(|character| !character.is_whitespace())
    {
        return None;
    }

    let name = after_hashes.trim().trim_end_matches('#').trim().to_string();
    if name.is_empty() {
        return None;
    }

    Some((level, name))
}

fn push_unique_limited(items: &mut Vec<String>, value: String, limit: usize) {
    if items.len() >= limit || items.iter().any(|existing| existing == &value) {
        return;
    }
    items.push(value);
}

fn subsystem_name_for_path(file_path: &str) -> String {
    let trimmed = file_path.trim_matches('/');
    if trimmed.is_empty() {
        return "(root)".to_string();
    }
    trimmed.split('/').next().unwrap_or("(root)").to_string()
}

fn summarize_key_symbols(symbols: &[Symbol], limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for symbol in symbols {
        if symbol.parent_id.is_some() {
            continue;
        }
        match symbol.symbol_type {
            SymbolType::Import | SymbolType::Module | SymbolType::Namespace => continue,
            _ => {}
        }

        let label = format!("{} ({})", symbol.name, symbol.symbol_type);
        if seen.insert(label.clone()) {
            out.push(label);
        }
        if out.len() >= limit {
            break;
        }
    }

    out
}

fn is_probable_entrypoint(file_path: &str, symbols: &[Symbol], export_count: usize) -> bool {
    let lower = file_path.to_lowercase();
    if [
        "main.rs",
        "main.ts",
        "main.tsx",
        "main.js",
        "main.jsx",
        "main.go",
        "app.rs",
        "app.ts",
        "app.tsx",
        "app.js",
        "app.go",
        "server.ts",
        "server.js",
        "server.go",
        "index.ts",
        "index.tsx",
        "index.js",
        "index.jsx",
        "lib.rs",
        "mod.rs",
        "__init__.py",
        "cli.rs",
        "cli.go",
    ]
    .iter()
    .any(|suffix| lower.ends_with(suffix))
    {
        return true;
    }

    export_count > 0
        && symbols.iter().any(|symbol| {
            symbol.parent_id.is_none()
                && matches!(
                    symbol.symbol_type,
                    SymbolType::Function
                        | SymbolType::Class
                        | SymbolType::Struct
                        | SymbolType::Interface
                        | SymbolType::Trait
                )
        })
}

fn is_test_path(file_path: &str) -> bool {
    let lower = file_path.to_lowercase();
    lower.contains("/test")
        || lower.contains("/tests/")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_test.py")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".test.js")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".spec.tsx")
        || lower.ends_with(".spec.js")
}

fn is_config_path(file_path: &str) -> bool {
    let lower = file_path.to_lowercase();
    lower.ends_with("package.json")
        || lower.ends_with("cargo.toml")
        || lower.ends_with("tsconfig.json")
        || lower.ends_with("pyproject.toml")
        || lower.ends_with("requirements.txt")
        || lower.ends_with("dockerfile")
        || lower.ends_with("docker-compose.yml")
        || lower.ends_with("docker-compose.yaml")
        || lower.ends_with("vite.config.ts")
        || lower.ends_with("vite.config.js")
        || lower.ends_with("next.config.js")
        || lower.ends_with("next.config.mjs")
        || lower.ends_with("jest.config.js")
        || lower.ends_with("jest.config.ts")
        || lower.ends_with("pytest.ini")
        || lower.ends_with(".env.example")
}

fn should_allow_non_indexed_live_sync(file_path: &str) -> bool {
    if Language::from_path(file_path).is_some() {
        return false;
    }

    let lower = file_path.to_lowercase();
    lower.ends_with(".astro")
        || lower.ends_with(".json")
        || lower.ends_with(".jsonc")
        || lower.ends_with(".json5")
        || is_dot_file_path(&lower)
}

fn is_dot_file_path(file_path: &str) -> bool {
    file_path
        .rsplit(['/', '\\'])
        .next()
        .map(|name| name.starts_with('.') && name.len() > 1)
        .unwrap_or(false)
}

fn go_is_exported_name(name: &str) -> bool {
    name.chars()
        .next()
        .map(|ch| ch.is_uppercase())
        .unwrap_or(false)
}

fn file_end_position(content: &str) -> (u32, u32) {
    if content.is_empty() {
        return (0, 0);
    }

    let mut last_line = 0u32;
    let mut last_width = 0u32;
    for (idx, line) in content.lines().enumerate() {
        last_line = idx as u32;
        last_width = line.chars().count() as u32;
    }

    if content.ends_with('\n') {
        (last_line.saturating_add(1), 0)
    } else {
        (last_line, last_width)
    }
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
    fn test_dot_files_allow_non_indexed_live_sync() {
        assert!(should_allow_non_indexed_live_sync(".gitignore"));
        assert!(should_allow_non_indexed_live_sync("config/.env.local"));
        assert!(should_allow_non_indexed_live_sync("nested/.dockerignore"));
        assert!(!should_allow_non_indexed_live_sync("src/main.rs"));
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
    fn test_index_markdown_headers() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("planning.md"),
            "# Stage 2 Planning\n\nBody text\n\n## Goals\n\n### Details ###\n\nNot a symbol\n",
        )
        .unwrap();

        let symbols = service.index_file("planning.md").unwrap();

        assert!(symbols.iter().any(|s| s.name == "Stage 2 Planning"));
        assert!(symbols.iter().any(|s| s.name == "Goals"));
        assert!(symbols.iter().any(|s| s.name == "Details"));
        assert!(!symbols.iter().any(|s| s.name == "Body text"));
        assert!(!symbols.iter().any(|s| s.name == "Not a symbol"));
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
    fn audit_index_health_detects_missing_and_stale_files() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("auth.ts"),
            "export function authenticate() {}",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("git.ts"),
            "export function oldName() {}",
        )
        .unwrap();
        service.index_file("git.ts").unwrap();
        fs::write(
            temp_dir.path().join("git.ts"),
            "export function GitCommitMessage() {}",
        )
        .unwrap();

        let health = service.audit_index_health().unwrap();

        assert_eq!(health.status, IndexHealthStatus::Stale);
        assert_eq!(health.supported_files, 2);
        assert_eq!(health.missing_files, 1);
        assert_eq!(health.stale_files, 1);
        assert_eq!(health.queued_files, 2);
    }

    #[test]
    fn reconcile_index_refreshes_stale_files_and_removes_orphans() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("old.ts"),
            "export function oldSymbol() {}",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("current.ts"),
            "export function beforeChange() {}",
        )
        .unwrap();
        service.index_file("old.ts").unwrap();
        service.index_file("current.ts").unwrap();
        fs::remove_file(temp_dir.path().join("old.ts")).unwrap();
        fs::write(
            temp_dir.path().join("current.ts"),
            "export function GitCommitMessage() {}",
        )
        .unwrap();

        let report = service.reconcile_index().unwrap();
        let health = service.index_health_snapshot();
        let results = service.search_symbols("GitCommitMessage", 10).unwrap();

        assert_eq!(report.files_removed, 1);
        assert_eq!(report.files_indexed, 1);
        assert_eq!(health.status, IndexHealthStatus::Fresh);
        assert_eq!(health.queued_files, 0);
        assert!(results
            .iter()
            .any(|result| result.symbol.name == "GitCommitMessage"));
        assert!(service.get_file_symbols_raw("old.ts").unwrap().is_empty());
    }

    #[test]
    fn test_index_file_persists_call_relationships() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("main.ts"),
            r#"
            function helperName(): string {
                return "helper";
            }

            function greetUser(): string {
                return helperName();
            }
        "#,
        )
        .unwrap();

        let symbols = service.index_file("main.ts").unwrap();
        let caller = symbols
            .iter()
            .find(|symbol| symbol.name == "greetUser")
            .unwrap();

        let targets = service
            .get_relationship_targets(&caller.id, SymbolRelationshipType::Call, 10)
            .unwrap();

        assert!(targets.iter().any(|target| target == "helperName"));

        let graph = service
            .get_symbol_graph(caller, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(graph.outgoing.len(), 1);
        assert_eq!(
            graph.outgoing[0].target_symbol_id.as_deref(),
            Some("main.ts::helperName#function")
        );
    }

    #[test]
    fn test_index_file_persists_import_relationships() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("utils.ts"),
            "export function helper() { return 'ok'; }",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("main.ts"),
            r#"
            import { helper } from "./utils";

            function run(): string {
                return helper();
            }
        "#,
        )
        .unwrap();

        service.index_file("utils.ts").unwrap();
        service.index_file("main.ts").unwrap();

        let targets = service
            .get_file_relationship_targets("main.ts", SymbolRelationshipType::Import, 10)
            .unwrap();

        assert!(targets.iter().any(|target| target == "utils.ts"));
    }

    #[test]
    fn test_build_semantic_project_overview_surfaces_modules_and_tests() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        fs::create_dir_all(temp_dir.path().join("tests")).unwrap();

        fs::write(
            temp_dir.path().join("src").join("main.ts"),
            r#"
            import { helper } from "./utils";

            export function runApp(): string {
                return helper();
            }
        "#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src").join("utils.ts"),
            r#"
            export function helper(): string {
                return "ok";
            }
        "#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("tests").join("main.test.ts"),
            r#"
            import { runApp } from "../src/main";

            export function testRunApp(): string {
                return runApp();
            }
        "#,
        )
        .unwrap();

        service.index_file("src/main.ts").unwrap();
        service.index_file("src/utils.ts").unwrap();
        service.index_file("tests/main.test.ts").unwrap();

        let overview = service
            .build_semantic_project_overview(None, 8, 4)
            .unwrap()
            .unwrap();

        assert!(overview.contains("# Semantic Project Overview:"));
        assert!(overview.contains("## Major Directories"));
        assert!(overview.contains("src/main.ts"));
        assert!(overview.contains("src/utils.ts"));
        assert!(overview.contains("tests/main.test.ts"));
        assert!(overview.contains("runApp (function)"));
        assert!(overview.contains("helper (function)"));
    }

    #[test]
    fn test_build_semantic_project_overview_bootstraps_unindexed_scope() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        fs::write(
            temp_dir.path().join("src").join("bootstrap.ts"),
            r#"
            export function bootstrapApp(): string {
                return "ok";
            }
        "#,
        )
        .unwrap();

        let overview = service
            .build_semantic_project_overview(None, 8, 4)
            .unwrap()
            .unwrap();

        assert!(overview.contains("src/bootstrap.ts"));
        assert!(overview.contains("bootstrapApp (function)"));
    }

    #[test]
    fn test_get_file_symbols_refreshes_after_disk_change() {
        let (service, temp_dir) = create_test_service();

        let file_path = temp_dir.path().join("fresh.ts");
        fs::write(
            &file_path,
            r#"
            export function firstName(): string {
                return "first";
            }
        "#,
        )
        .unwrap();

        service.index_file("fresh.ts").unwrap();

        fs::write(
            &file_path,
            r#"
            export function updatedName(): string {
                return "updated";
            }
        "#,
        )
        .unwrap();

        let symbols = service.get_file_symbols("fresh.ts").unwrap();
        assert!(symbols.iter().any(|symbol| symbol.name == "updatedName"));
        assert!(!symbols.iter().any(|symbol| symbol.name == "firstName"));
    }

    #[test]
    fn test_find_references_to_symbol_uses_neighborhood_resolution() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("utils.ts"),
            r#"
            export function helperName(): string {
                return "imported";
            }
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("main.ts"),
            r#"
            import { helperName } from "./utils";

            export function greetUser(): string {
                return helperName();
            }
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("other.ts"),
            r#"
            function helperName(): string {
                return "local";
            }

            export function unrelated(): string {
                return helperName();
            }
        "#,
        )
        .unwrap();

        let utils_symbols = service.index_file("utils.ts").unwrap();
        service.index_file("main.ts").unwrap();
        service.index_file("other.ts").unwrap();

        let helper = utils_symbols
            .iter()
            .find(|symbol| symbol.name == "helperName")
            .unwrap();

        let references = service.find_references_to_symbol(helper, 10).unwrap();

        assert_eq!(references.len(), 2);
        assert!(references.iter().any(|reference| {
            reference.source_symbol.name == "greetUser"
                && reference.source_symbol.file_path == "main.ts"
                && reference.relationship_type == SymbolRelationshipType::Call
                && reference.target_symbol_id.as_deref() == Some(helper.id.as_str())
        }));
        assert!(references.iter().any(|reference| {
            reference.source_symbol.symbol_type == SymbolType::Import
                && reference.source_symbol.file_path == "main.ts"
                && reference.relationship_type == SymbolRelationshipType::Import
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_incoming_and_outgoing_calls() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("main.ts"),
            r#"
            function helperName(): string {
                return "helper";
            }

            function greetUser(): string {
                return helperName();
            }

            function greetAgain(): string {
                return greetUser();
            }
        "#,
        )
        .unwrap();

        let symbols = service.index_file("main.ts").unwrap();
        let greet_user = symbols
            .iter()
            .find(|symbol| symbol.name == "greetUser")
            .unwrap();

        let graph = service
            .get_symbol_graph(greet_user, SymbolRelationshipType::Call, 10)
            .unwrap();

        assert!(graph
            .incoming
            .iter()
            .any(|reference| reference.source_symbol.name == "greetAgain"));
        assert!(graph.outgoing.iter().any(|reference| reference
            .target_symbol
            .as_ref()
            .map(|symbol| symbol.name.as_str())
            == Some("helperName")));
    }

    #[test]
    fn test_get_symbol_graph_returns_structural_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            interface BaseService {}
            interface Service extends BaseService {}

            class CoreService {}

            class UserService extends CoreService implements Service {
                run() {}
            }
        "#,
        )
        .unwrap();

        let symbols = service.index_file("service.ts").unwrap();
        let user_service = symbols
            .iter()
            .find(|symbol| symbol.name == "UserService")
            .unwrap();
        let service_interface = symbols
            .iter()
            .find(|symbol| symbol.name == "Service" && symbol.symbol_type == SymbolType::Interface)
            .unwrap();

        let extends_graph = service
            .get_symbol_graph(user_service, SymbolRelationshipType::Extends, 10)
            .unwrap();
        assert!(extends_graph.outgoing.iter().any(|reference| {
            reference.target_name == "CoreService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("CoreService")
        }));

        let implements_graph = service
            .get_symbol_graph(service_interface, SymbolRelationshipType::Implements, 10)
            .unwrap();
        assert!(implements_graph.incoming.iter().any(|reference| {
            reference.source_symbol.name == "UserService"
                && reference.target_symbol_id.as_deref() == Some(service_interface.id.as_str())
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_containment_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            class UserService {
                getUser(id: string): string {
                    return id;
                }
            }
        "#,
        )
        .unwrap();

        let symbols = service.index_file("service.ts").unwrap();
        let user_service = symbols
            .iter()
            .find(|symbol| symbol.name == "UserService")
            .unwrap();
        let get_user = symbols
            .iter()
            .find(|symbol| symbol.name == "getUser")
            .unwrap();

        let parent_graph = service
            .get_symbol_graph(user_service, SymbolRelationshipType::Contains, 10)
            .unwrap();
        assert!(parent_graph.outgoing.iter().any(|reference| {
            reference
                .target_symbol
                .as_ref()
                .map(|symbol| symbol.name.as_str())
                == Some("getUser")
        }));

        let child_graph = service
            .get_symbol_graph(get_user, SymbolRelationshipType::Contains, 10)
            .unwrap();
        assert!(child_graph.incoming.iter().any(|reference| {
            reference.source_symbol.name == "UserService"
                && reference.target_symbol_id.as_deref() == Some(get_user.id.as_str())
        }));
    }

    #[test]
    fn test_get_file_symbols_hides_synthetic_file_root() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            class UserService {
                getUser(id: string): string {
                    return id;
                }
            }
        "#,
        )
        .unwrap();

        let visible_symbols = service.index_file("service.ts").unwrap();
        assert!(visible_symbols
            .iter()
            .all(|symbol| symbol.qualified_name != "__file__"));
        assert!(visible_symbols
            .iter()
            .all(|symbol| symbol.parent_id.as_deref() != Some("service.ts::__file__#module")));

        let stored_symbols = service.get_file_symbols_raw("service.ts").unwrap();
        assert!(stored_symbols
            .iter()
            .any(|symbol| symbol.qualified_name == "__file__"));
    }

    #[test]
    fn test_search_symbols_hides_synthetic_file_root() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            class UserService {
                getUser(id: string): string {
                    return id;
                }
            }
        "#,
        )
        .unwrap();

        service.index_file("service.ts").unwrap();
        let results = service.search_symbols("service", 10).unwrap();

        assert!(results
            .iter()
            .all(|result| result.symbol.qualified_name != "__file__"));
        assert!(results
            .iter()
            .any(|result| result.symbol.name == "UserService"));
    }

    #[test]
    fn test_get_symbol_graph_returns_module_export_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            export class UserService {
                getUser(id: string): string {
                    return id;
                }
            }

            class InternalService {}
        "#,
        )
        .unwrap();

        service.index_file("service.ts").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.ts")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "UserService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
        assert!(!graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "InternalService"));
    }

    #[test]
    fn test_get_symbol_graph_returns_named_module_export_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            class UserService {
                getUser(id: string): string {
                    return id;
                }
            }

            class InternalService {}

            export { UserService as Service };
        "#,
        )
        .unwrap();

        service.index_file("service.ts").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.ts")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "Service"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
        assert!(!graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "InternalService"));
    }

    #[test]
    fn test_get_symbol_graph_returns_named_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.ts"),
            r#"
            export class UserService {
                getUser(id: string): string {
                    return id;
                }
            }
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            export { UserService as Service } from "./base";
        "#,
        )
        .unwrap();

        service.index_file("base.ts").unwrap();
        service.index_file("service.ts").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.ts")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "Service"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.ts")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
    }

    #[test]
    fn test_get_symbol_graph_propagates_typescript_export_star_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.ts"),
            r#"
            export default class HiddenService {}

            export class UserService {}
            export class AuditService {}
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            export * from "./base";
        "#,
        )
        .unwrap();

        service.index_file("base.ts").unwrap();
        service.index_file("service.ts").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.ts")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "UserService"));
        assert!(graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "AuditService"));
        assert!(!graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "default"));
    }

    #[test]
    fn test_get_symbol_graph_returns_typescript_namespace_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.ts"),
            r#"
            export class UserService {}
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            export * as api from "./base";
        "#,
        )
        .unwrap();

        service.index_file("base.ts").unwrap();
        service.index_file("service.ts").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.ts")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "api"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.ts")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.qualified_name.as_str())
                    == Some("__file__")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.symbol_type)
                    == Some(SymbolType::Module)
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_default_export_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            export default class UserService {
                getUser(id: string): string {
                    return id;
                }
            }
        "#,
        )
        .unwrap();

        service.index_file("service.ts").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.ts")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "default"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
    }

    #[test]
    fn test_get_symbol_graph_resolves_default_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.ts"),
            r#"
            export default class UserService {
                getUser(id: string): string {
                    return id;
                }
            }
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            export { default as Service } from "./base";
        "#,
        )
        .unwrap();

        service.index_file("base.ts").unwrap();
        service.index_file("service.ts").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.ts")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "Service"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.ts")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_rust_pub_use_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.rs"),
            r#"
            pub struct UserService;
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("lib.rs"),
            r#"
            pub use crate::base::UserService;
        "#,
        )
        .unwrap();

        service.index_file("base.rs").unwrap();
        service.index_file("lib.rs").unwrap();
        let module_symbol = service.get_file_module_symbol("lib.rs").unwrap().unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "UserService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.rs")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_rust_direct_module_export_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.rs"),
            r#"
            pub struct UserService;
            struct InternalService;
        "#,
        )
        .unwrap();

        service.index_file("base.rs").unwrap();
        let module_symbol = service.get_file_module_symbol("base.rs").unwrap().unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "UserService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
        assert!(!graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "InternalService"));
    }

    #[test]
    fn test_get_symbol_graph_preserves_rust_pub_use_alias_names() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.rs"),
            r#"
            pub struct UserService;
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("lib.rs"),
            r#"
            pub use crate::base::UserService as Service;
        "#,
        )
        .unwrap();

        service.index_file("base.rs").unwrap();
        service.index_file("lib.rs").unwrap();
        let module_symbol = service.get_file_module_symbol("lib.rs").unwrap().unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "Service"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_rust_module_alias_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.rs"),
            r#"
            pub struct UserService;
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("lib.rs"),
            r#"
            pub use crate::base as api;
        "#,
        )
        .unwrap();

        service.index_file("base.rs").unwrap();
        service.index_file("lib.rs").unwrap();
        let module_symbol = service.get_file_module_symbol("lib.rs").unwrap().unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "api"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.rs")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.qualified_name.as_str())
                    == Some("__file__")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.symbol_type)
                    == Some(SymbolType::Module)
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_rust_plain_module_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.rs"),
            r#"
            pub struct UserService;
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("lib.rs"),
            r#"
            pub use crate::base;
        "#,
        )
        .unwrap();

        service.index_file("base.rs").unwrap();
        service.index_file("lib.rs").unwrap();
        let module_symbol = service.get_file_module_symbol("lib.rs").unwrap().unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "base"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.rs")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.qualified_name.as_str())
                    == Some("__file__")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.symbol_type)
                    == Some(SymbolType::Module)
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_grouped_rust_module_alias_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.rs"),
            r#"
            pub struct UserService;
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("lib.rs"),
            r#"
            pub use crate::base::{self as api, UserService};
        "#,
        )
        .unwrap();

        service.index_file("base.rs").unwrap();
        service.index_file("lib.rs").unwrap();
        let module_symbol = service.get_file_module_symbol("lib.rs").unwrap().unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "api"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.rs")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.qualified_name.as_str())
                    == Some("__file__")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.symbol_type)
                    == Some(SymbolType::Module)
        }));
        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "UserService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_rust_grouped_pub_use_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.rs"),
            r#"
            pub struct UserService;
            pub struct AuditService;
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("lib.rs"),
            r#"
            pub use crate::base::{UserService, AuditService};
        "#,
        )
        .unwrap();

        service.index_file("base.rs").unwrap();
        service.index_file("lib.rs").unwrap();
        let module_symbol = service.get_file_module_symbol("lib.rs").unwrap().unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "UserService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.rs")
        }));
        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "AuditService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.rs")
        }));
    }

    #[test]
    fn test_get_symbol_graph_propagates_rust_glob_pub_use_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.rs"),
            r#"
            pub struct UserService;
            pub struct AuditService;
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("lib.rs"),
            r#"
            pub use crate::base::*;
        "#,
        )
        .unwrap();

        service.index_file("base.rs").unwrap();
        service.index_file("lib.rs").unwrap();
        let module_symbol = service.get_file_module_symbol("lib.rs").unwrap().unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "UserService"));
        assert!(graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "AuditService"));
    }

    #[test]
    fn test_get_symbol_graph_returns_python_dunder_all_exports() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("service.py"),
            r#"
__all__ = ["UserService"]

class UserService:
    pass

class _InternalService:
    pass
        "#,
        )
        .unwrap();

        service.index_file("service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "UserService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
        assert!(!graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "_InternalService"));
    }

    #[test]
    fn test_get_symbol_graph_returns_python_imported_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.py"),
            r#"
class UserService:
    pass

class _InternalService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.py"),
            r#"
from base import UserService
        "#,
        )
        .unwrap();

        service.index_file("base.py").unwrap();
        service.index_file("service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "UserService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.py")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_python_relative_imported_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("pkg")).unwrap();
        fs::write(temp_dir.path().join("pkg").join("__init__.py"), "").unwrap();
        fs::write(
            temp_dir.path().join("pkg").join("base.py"),
            r#"
class UserService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("pkg").join("service.py"),
            r#"
from .base import UserService
        "#,
        )
        .unwrap();

        service.index_file("pkg/base.py").unwrap();
        service.index_file("pkg/service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("pkg/service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "UserService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("pkg/base.py")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_python_relative_submodule_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("pkg")).unwrap();
        fs::write(temp_dir.path().join("pkg").join("__init__.py"), "").unwrap();
        fs::write(
            temp_dir.path().join("pkg").join("base.py"),
            r#"
class UserService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("pkg").join("service.py"),
            r#"
from . import base
        "#,
        )
        .unwrap();

        service.index_file("pkg/base.py").unwrap();
        service.index_file("pkg/service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("pkg/service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "base"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("pkg/base.py")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.qualified_name.as_str())
                    == Some("__file__")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.symbol_type)
                    == Some(SymbolType::Module)
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_python_relative_submodule_alias_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("pkg")).unwrap();
        fs::write(temp_dir.path().join("pkg").join("__init__.py"), "").unwrap();
        fs::write(
            temp_dir.path().join("pkg").join("base.py"),
            r#"
class UserService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("pkg").join("service.py"),
            r#"
__all__ = ["api"]

from . import base as api
        "#,
        )
        .unwrap();

        service.index_file("pkg/base.py").unwrap();
        service.index_file("pkg/service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("pkg/service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "api"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("pkg/base.py")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.qualified_name.as_str())
                    == Some("__file__")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.symbol_type)
                    == Some(SymbolType::Module)
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_python_imported_alias_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.py"),
            r#"
class UserService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.py"),
            r#"
__all__ = ["Service"]

from base import UserService as Service
        "#,
        )
        .unwrap();

        service.index_file("base.py").unwrap();
        service.index_file("service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "Service"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.py")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
    }

    #[test]
    fn test_get_symbol_graph_propagates_python_import_star_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.py"),
            r#"
class UserService:
    pass

class AuditService:
    pass

class _InternalService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.py"),
            r#"
from base import *
        "#,
        )
        .unwrap();

        service.index_file("base.py").unwrap();
        service.index_file("service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "UserService"));
        assert!(graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "AuditService"));
        assert!(!graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "_InternalService"));
    }

    #[test]
    fn test_get_symbol_graph_propagates_python_relative_import_star_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("pkg")).unwrap();
        fs::write(temp_dir.path().join("pkg").join("__init__.py"), "").unwrap();
        fs::write(
            temp_dir.path().join("pkg").join("base.py"),
            r#"
class UserService:
    pass

class AuditService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("pkg").join("service.py"),
            r#"
from .base import *
        "#,
        )
        .unwrap();

        service.index_file("pkg/base.py").unwrap();
        service.index_file("pkg/service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("pkg/service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "UserService"));
        assert!(graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "AuditService"));
    }

    #[test]
    fn test_get_symbol_graph_filters_python_import_star_reexports_with_dunder_all() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.py"),
            r#"
class UserService:
    pass

class AuditService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.py"),
            r#"
__all__ = ["AuditService"]

from base import *
        "#,
        )
        .unwrap();

        service.index_file("base.py").unwrap();
        service.index_file("service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "AuditService"));
        assert!(!graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "UserService"));
    }

    #[test]
    fn test_get_symbol_graph_returns_python_import_module_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.py"),
            r#"
class UserService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.py"),
            r#"
import base
        "#,
        )
        .unwrap();

        service.index_file("base.py").unwrap();
        service.index_file("service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "base"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.py")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.qualified_name.as_str())
                    == Some("__file__")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.symbol_type)
                    == Some(SymbolType::Module)
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_python_dotted_import_package_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("pkg")).unwrap();
        fs::write(temp_dir.path().join("pkg").join("__init__.py"), "").unwrap();
        fs::write(
            temp_dir.path().join("pkg").join("base.py"),
            r#"
class UserService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.py"),
            r#"
import pkg.base
        "#,
        )
        .unwrap();

        service.index_file("pkg/__init__.py").unwrap();
        service.index_file("pkg/base.py").unwrap();
        service.index_file("service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "pkg"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("pkg/__init__.py")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.qualified_name.as_str())
                    == Some("__file__")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.symbol_type)
                    == Some(SymbolType::Module)
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_python_import_module_alias_reexport_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("base.py"),
            r#"
class UserService:
    pass
        "#,
        )
        .unwrap();

        fs::write(
            temp_dir.path().join("service.py"),
            r#"
__all__ = ["api"]

import base as api
        "#,
        )
        .unwrap();

        service.index_file("base.py").unwrap();
        service.index_file("service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "api"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.as_str())
                    == Some("base.py")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.qualified_name.as_str())
                    == Some("__file__")
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.symbol_type)
                    == Some(SymbolType::Module)
        }));
    }

    #[test]
    fn test_get_symbol_graph_returns_python_public_top_level_exports() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("service.py"),
            r#"
class UserService:
    pass

class _InternalService:
    pass
        "#,
        )
        .unwrap();

        service.index_file("service.py").unwrap();
        let module_symbol = service
            .get_file_module_symbol("service.py")
            .unwrap()
            .unwrap();

        let graph = service
            .get_symbol_graph(&module_symbol, SymbolRelationshipType::Export, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference.target_name == "UserService"
                && reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("UserService")
        }));
        assert!(!graph
            .outgoing
            .iter()
            .any(|reference| reference.target_name == "_InternalService"));
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

    #[test]
    fn test_index_go_file_extracts_exported_symbols() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("main.go"),
            r#"
package main

import "fmt"

type Server struct{}

func Run() {
    fmt.Println("ok")
}

func helper() {}
"#,
        )
        .unwrap();

        let symbols = service.index_file("main.go").unwrap();

        assert!(symbols
            .iter()
            .any(|symbol| symbol.name == "Server" && symbol.symbol_type == SymbolType::Struct));
        assert!(symbols
            .iter()
            .any(|symbol| symbol.name == "Run" && symbol.symbol_type == SymbolType::Function));
    }

    #[test]
    fn test_live_sync_allows_common_non_indexed_documents() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("src/pages")).unwrap();
        fs::write(
            temp_dir.path().join("src/pages/changelog.astro"),
            "<Layout><h1>Release notes</h1></Layout>",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("package.json"),
            "{ \"name\": \"demo\" }",
        )
        .unwrap();

        assert!(service
            .did_open(
                "src/pages/changelog.astro",
                "<Layout><h1>Release notes</h1></Layout>",
            )
            .is_ok());
        assert!(service
            .did_change(
                "package.json",
                2,
                "{ \"name\": \"demo\", \"private\": true }"
            )
            .is_ok());
    }

    #[test]
    fn test_non_indexed_documents_still_do_not_enter_symbol_index() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("package.json"),
            "{ \"name\": \"demo\" }",
        )
        .unwrap();

        let error = service
            .index_file("package.json")
            .expect_err("json files should not be symbol-indexed yet");

        assert!(matches!(error, LanguageError::NotSupported(_)));
    }
}
