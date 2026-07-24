//! Unified Language Service
//!
//! Combines tree-sitter parsing and symbol indexing
//! into a single coherent API for ZLP.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, RwLock};

use crate::buffer_snapshot::{BufferSnapshot, BufferSnapshotSource, BufferSnapshotStore};
use crate::gitignore_filter::GitignoreFilter;
use crate::project_settings;
use crate::symbol_index::store::{
    AnchorQueryMode, SemanticAnchorSearchOutcome, SYMBOL_STORE_SCHEMA_VERSION,
};
use crate::symbol_index::{
    FileIndexRecord, FileRelationshipRecord, ModuleRelationshipAggregate,
    RelationshipIntegrityStats, RelationshipObservationKind, SearchQuery, SearchResult,
    SemanticAnchor, SemanticAnchorResult, SymbolReference, SymbolStore,
    UnresolvedRelationshipTarget,
};
use crate::tree_sitter::{
    call_form, collect_extraction_facts, extract_symbol_relationships_with_facts,
    extract_symbols_with_facts, stable_symbol_id, Language, Position, Range, Symbol,
    SymbolRelationship, SymbolRelationshipType, SymbolType, TreeSitterParser, unresolved_reason,
};
use crate::worktree::{normalize_path, WorktreeStore};
use serde::{Deserialize, Serialize};

use super::rust_project;

thread_local! {
    static INDEXING_PARSER: RefCell<Option<TreeSitterParser>> = RefCell::new(None);
}

const ANCHOR_ONLY_EXTRACTOR_VERSION: u32 = 1;
// Stored extractor versions combine the language extractor with cross-language
// semantic-anchor behavior, so a rationale/link extraction change invalidates
// every affected file exactly once without rewriting each language capability.
const SEMANTIC_CONTEXT_EXTRACTOR_VERSION: u32 = 3;

/// Unified language service
pub struct LanguageService {
    /// Workspace root path
    workspace_root: PathBuf,
    /// Symbol index for persistent storage
    symbol_store: Arc<SymbolStore>,
    /// Shared in-memory worktree snapshot/index
    worktree_store: RwLock<Option<Arc<WorktreeStore>>>,
    buffer_snapshots: BufferSnapshotStore,
    overlays: RwLock<HashMap<String, OverlayDocument>>,

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
    pub current_file: Option<String>,
    pub message: String,
    pub timings: IndexTimingSnapshot,
    pub discovery: IndexDiscoverySnapshot,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexTimingSnapshot {
    pub last_discovery_ms: Option<u64>,
    pub last_file_path: Option<String>,
    pub last_file_total_ms: Option<u64>,
    pub last_file_load_ms: Option<u64>,
    pub last_file_freshness_check_ms: Option<u64>,
    pub last_file_parse_extract_ms: Option<u64>,
    pub last_file_relationship_enrichment_ms: Option<u64>,
    pub last_file_db_write_ms: Option<u64>,
    pub last_file_cache_update_ms: Option<u64>,
    pub last_batch_load_ms: Option<u64>,
    pub last_batch_freshness_check_ms: Option<u64>,
    pub last_batch_parse_extract_ms: Option<u64>,
    pub last_batch_relationship_enrichment_ms: Option<u64>,
    pub last_batch_db_write_ms: Option<u64>,
    pub last_batch_cache_update_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexLanguageCount {
    pub language: String,
    pub count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexSkipCount {
    pub reason: String,
    pub count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexDiscoverySnapshot {
    pub last_scope: Option<String>,
    pub last_discovered_files: usize,
    pub last_supported_files: usize,
    pub last_indexed_files: usize,
    pub last_failed_files: usize,
    pub last_fresh_files: usize,
    pub last_reindexed_files: usize,
    pub last_symbols_extracted: usize,
    pub last_anchors_extracted: usize,
    pub last_relationships_extracted: usize,
    pub supported_by_language: Vec<IndexLanguageCount>,
    pub skipped_by_reason: Vec<IndexSkipCount>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexSchemaCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexSchemaLanguageCount {
    pub language: String,
    pub support_level: String,
    pub count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexSchemaTotals {
    pub indexed_files: usize,
    pub symbols: usize,
    pub relationships: usize,
    pub semantic_anchors: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexSchemaScope {
    pub requested_path: String,
    pub normalized_path: String,
    pub root_totals: IndexSchemaTotals,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexSchemaSnapshot {
    pub totals: IndexSchemaTotals,
    pub scope: Option<IndexSchemaScope>,
    pub files_by_extension: Vec<IndexSchemaCount>,
    pub files_by_language: Vec<IndexSchemaLanguageCount>,
    pub files_by_support_level: Vec<IndexSchemaCount>,
    pub symbols_by_type: Vec<IndexSchemaCount>,
    pub relationships: RelationshipIntegrityStats,
    pub semantic_anchors_by_kind: Vec<IndexSchemaCount>,
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
            current_file: None,
            message: "Code intelligence status unknown".to_string(),
            timings: IndexTimingSnapshot::default(),
            discovery: IndexDiscoverySnapshot::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IndexReconciliationReport {
    pub health: IndexHealthSnapshot,
    pub files_indexed: usize,
    pub files_removed: usize,
    pub duration_ms: u64,
    pub graph_quality: IndexGraphQualityReport,
    /// M6.1 — true when this reconcile short-circuited via the no-change fast path
    /// (the worktree proved unchanged since the last fully-healthy reconcile, so no
    /// discovery/freshness/audit work ran). False for a full reconcile.
    pub fast_path: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexGraphQualityReport {
    pub total_relationships: usize,
    pub resolved_relationships: usize,
    pub unresolved_symbol_relationships: usize,
    pub suppressed_external_relationships: usize,
    pub missing_source_symbols: usize,
    pub missing_target_symbols: usize,
    pub indexed_files_missing_root_symbol: usize,
    pub by_type: Vec<IndexRelationshipTypeQuality>,
    pub top_unresolved_targets: Vec<UnresolvedRelationshipTarget>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexRelationshipTypeQuality {
    pub relationship_type: String,
    pub total_relationships: usize,
    pub resolved_relationships: usize,
    pub unresolved_symbol_relationships: usize,
}

impl IndexGraphQualityReport {
    fn from_relationship_stats(
        stats: RelationshipIntegrityStats,
        indexed_files_missing_root_symbol: usize,
    ) -> Self {
        Self {
            total_relationships: stats.total_relationships,
            resolved_relationships: stats.resolved_relationships,
            unresolved_symbol_relationships: stats.unresolved_symbol_relationships,
            suppressed_external_relationships: 0,
            missing_source_symbols: stats.missing_source_symbols,
            missing_target_symbols: stats.missing_target_symbols,
            indexed_files_missing_root_symbol,
            by_type: stats
                .by_type
                .into_iter()
                .map(|stats| IndexRelationshipTypeQuality {
                    relationship_type: stats.relationship_type,
                    total_relationships: stats.total_relationships,
                    resolved_relationships: stats.resolved_relationships,
                    unresolved_symbol_relationships: stats.unresolved_symbol_relationships,
                })
                .collect(),
            top_unresolved_targets: stats.top_unresolved_targets,
        }
    }
}

/// M6.1 — persisted reconcile fast-path checkpoint. Written at the end of a fully
/// `Fresh` reconcile and reused on the next reconcile when the worktree provably has
/// not changed. Stored as JSON in the `index_meta` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReconcileCheckpoint {
    fingerprint: String,
    health: IndexHealthSnapshot,
    graph_quality: IndexGraphQualityReport,
}

const RECONCILE_CHECKPOINT_KEY: &str = "reconcile_checkpoint_v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolLiteralMatch {
    pub file_path: String,
    pub line: u32,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSearchHealingReport {
    pub enabled: bool,
    pub triggered: bool,
    pub reason: Option<String>,
    pub confidence: String,
    pub initial_result_count: usize,
    pub initial_top_score: Option<f32>,
    pub reran_after_reindex: bool,
    pub reindexed_files: Vec<String>,
    pub removed_files: Vec<String>,
    pub literal_matches: Vec<SymbolLiteralMatch>,
    pub semantic_anchor_matches: Vec<SemanticAnchorResult>,
    pub diagnostics: Vec<String>,
    pub health_before: Option<IndexHealthSnapshot>,
    pub health_after: Option<IndexHealthSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSearchOutcome {
    pub results: Vec<SearchResult>,
    pub healing: SymbolSearchHealingReport,
}

/// Cached file data
#[derive(Clone)]
#[allow(dead_code)] // Retained for indexed-file cache accounting; overlays use separate authority.
struct CachedFile {
    /// Content hash for change detection
    hash: String,
    _snapshot: Arc<BufferSnapshot>,
    /// Extracted symbols
    symbols: Vec<Symbol>,
}

#[derive(Clone)]
struct OverlayDocument {
    _version: Option<i32>,
    symbols: Vec<Symbol>,
    relationships: Vec<SymbolRelationship>,
    anchors: Vec<SemanticAnchor>,
}

struct SymbolExtraction<'a> {
    symbols: Vec<Symbol>,
    relationships: Vec<SymbolRelationship>,
    content: Cow<'a, str>,
    language: Language,
}

/// M5.7 — full-reconcile streaming. The reconcile stages each file's extraction
/// (symbols + the full source `String` + an `Arc<BufferSnapshot>`) before writing
/// it. Accumulating ALL files before a single commit blew memory to 14 GiB on the
/// Linux kernel (77k files). Instead we commit in bounded batches of this many
/// files and drop each batch, so peak memory is O(batch), not O(repo).
///
/// M5.15 — raised 1_000 → 4_000. On repos of many SMALL files (Firefox: ~5 KB
/// avg) the old cap bound long before `BATCH_BYTE_BUDGET`, making batches tiny and
/// the extract→commit barrier fire constantly (cores idle each serial commit).
/// Bigger batches amortize the barrier; `BATCH_BYTE_BUDGET` still bounds memory.
/// M5.17 — 4_000 → 10_000. Now that the WAL auto-checkpoint no longer stalls the
/// committer every few MB (see `configure_index_pragmas`), the committer keeps up
/// with extraction, so larger batches (fewer inter-batch barriers) pay off;
/// `BATCH_BYTE_BUDGET` still caps staged memory regardless of file count.
const RECONCILE_BATCH_SIZE: usize = 10_000;

/// M5.7 — defensive per-file cap. A pathological/generated file can declare an
/// absurd number of symbols; we truncate (with a log) so a single file cannot
/// spike a batch's memory. Real source files are far below this (the kernel's
/// largest hand-written files are in the hundreds).
const MAX_SYMBOLS_PER_FILE: usize = 25_000;

/// M5.7 — a single batch never stages more than this many BYTES of file content
/// (whichever comes first with `RECONCILE_BATCH_SIZE`). Batching by file COUNT
/// alone does not bound memory when file sizes vary by 1000× — the kernel's
/// generated multi-MB headers mean a 1000-file batch landing in such a directory
/// holds gigabytes. Bounding by content bytes keeps peak memory flat regardless
/// of how large files are distributed.
///
/// M5.15 — 24 → 48 MiB. With the overlap committer (extract↔commit pipeline) at
/// most ~3 batches are in flight, so peak staged content is ~3 × this; 48 MiB
/// keeps that ~144 MiB — trivial — while making each parallel-extract phase longer
/// relative to its commit, so the cores stay busy.
const BATCH_BYTE_BUDGET: u64 = 48 * 1024 * 1024;

/// M5.4 — minimum wall-clock between progress (IPC) emits during the parallel
/// extraction pass. With the worker pool saturating every core, the main drain
/// thread sees a firehose of start/finish events (Firefox ≈ 450k); emitting one
/// IPC status per event would melt the main thread and flood the webview. We
/// keep the in-memory health snapshot current per event (cheap) but throttle the
/// expensive string-build + emit to ~10/s. Batch boundaries always emit.
const UI_EMIT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// M5.7 — files larger than this are indexed ANCHOR-ONLY (the recursive symbol
/// walk is skipped). They are almost always generated data — e.g. the kernel's
/// AMD register-mask headers, 2–24 MB with 50k–190k `#define`s — whose macro
/// "symbols" bloat the DB (to multiple GB), spike memory, and have no
/// navigational value. The file still gets a root symbol so it stays
/// discoverable by path. Real hand-written source is virtually always well under
/// 1 MiB, so this spares real code while catching generated data.
const MAX_EXTRACT_BYTES: usize = 1024 * 1024;

/// M5.7b — recognize files whose NAME marks them as generated (a complement to the
/// size heuristic, ported from the inspiration project's protobuf/codegen skip).
/// These are indexed anchor-only: their thousands of generated symbols are search
/// noise, and this catches *small* generated files that the size cap misses (and
/// never wrongly skips a large hand-written file). Match on the file name only.
fn is_generated_path(file_path: &str) -> bool {
    let lower = file_path.to_ascii_lowercase();
    let name = lower.rsplit(['/', '\\']).next().unwrap_or(lower.as_str());
    name.contains("zz_generated")
        || name.contains(".generated.")
        || name.contains("_generated.")
        || name.ends_with(".pb.go")
        || name.ends_with(".pb.cc")
        || name.ends_with(".pb.h")
        || name.ends_with("_pb2.py")
        || name.ends_with("_pb2_grpc.py")
        || name.ends_with(".min.js")
        || name.ends_with(".min.css")
        || name.ends_with(".g.dart")
        || name.ends_with(".freezed.dart")
        || name.ends_with(".designer.cs")
}

struct StagedFileIndex {
    file_path: String,
    hash: String,
    file_size: Option<u64>,
    line_count: usize,
    modified_at: Option<i64>,
    extractor_version: Option<u32>,
    symbols: Vec<Symbol>,
    anchors: Vec<SemanticAnchor>,
    relationships: Vec<SymbolRelationship>,
    extraction_content: String,
    extraction_language: Language,
    source_language: Language,
}

#[derive(Debug, Clone, Default)]
struct DiscoveryReport {
    files: Vec<String>,
    discovered_files: usize,
    skipped_by_reason: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Default)]
struct IndexFileMetrics {
    anchors: usize,
    total_ms: u64,
    load_ms: u64,
    freshness_check_ms: u64,
    parse_extract_ms: u64,
    relationship_enrichment_ms: u64,
    db_write_ms: u64,
    cache_update_ms: u64,
}

struct StagedFileIndexOutcome {
    staged: StagedFileIndex,
    metrics: IndexFileMetrics,
}

#[derive(Debug, Clone, Default)]
struct CommitStagedFileMetrics {
    suppressed_external_relationships: usize,
    relationship_count: usize,
    relationship_enrichment_ms: u64,
    db_write_ms: u64,
    cache_update_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct FileIndexMetadata {
    file_size: u64,
    modified_at: i64,
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

/// Track F — the relationship kinds each language's extractor emits
/// observations for AT ALL (observational coverage).
///
/// A kind's presence in a list means "this extractor produces SOME edges of
/// this kind" — it is explicitly NOT a claim of exhaustive coverage. Known
/// gaps exist inside listed kinds: Rust `call` does not see calls buried in
/// macro token trees, and Rust `usage` (Track B) covers only FILE-LOCAL
/// constant references, not cross-file ones. Because listed kinds can still
/// miss real edges, the tools layer treats an EMPTY relationship result for a
/// listed kind as `partial_coverage` — evidence of absence is never claimed
/// from these lists alone.
///
/// Release-gate honesty metadata consumed by empty-result trust reporting: an
/// empty reference set for a kind a language never models must read as "not
/// modelled", never as "unused". Each list is derived from the extraction code,
/// not aspiration:
/// - `call`: the unified relationship walk for every full grammar (TS family
///   including JSX component elements, Python, Rust including macro
///   invocations, Go);
/// - `import`: derived from extracted Import symbols (all full grammars);
/// - `uses_type` / `reads_env`: the M4.1/M4.2 concerns of the shared walk (all
///   full grammars);
/// - structural: TS family `extends` + `implements`; Python `extends` (class
///   bases); Rust `implements` (`impl Trait for Type`) plus `usage` (Track B
///   file-local constant references); Go `contains` (receiver→method) and
///   `extends` (struct embedding) plus `implements` (Track C post-index
///   implicit-interface mining);
/// - `handles`: requires route detection, which only Python and the TS family
///   support.
///
/// Languages whose capability declares `relationships: false` (C/C++ is
/// definitions-only, Markdown and every scanner language extract no edges)
/// honestly model NOTHING — the empty list. A language that later gains
/// `relationships: true` without an audited entry below also reports the empty
/// list (prefer the false negative over claiming unverified coverage).
pub fn modelled_relationship_kinds(language: Language) -> &'static [&'static str] {
    if !language.capability().extracts.relationships {
        return &[];
    }
    match language {
        Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx => &[
            "call",
            "import",
            "extends",
            "implements",
            "uses_type",
            "reads_env",
            "handles",
        ],
        Language::Python => &[
            "call",
            "import",
            "extends",
            "uses_type",
            "reads_env",
            "handles",
        ],
        Language::Rust => &[
            "call",
            "import",
            "implements",
            "usage",
            "uses_type",
            "reads_env",
        ],
        Language::Go => &[
            "call",
            "import",
            "contains",
            "extends",
            "implements",
            "uses_type",
            "reads_env",
        ],
        _ => &[],
    }
}

impl LanguageService {
    /// Create a new language service for a workspace
    pub fn new(
        workspace_root: PathBuf,
        symbol_store: Arc<SymbolStore>,
    ) -> Result<Self, LanguageError> {
        Ok(Self {
            workspace_root,
            symbol_store,
            worktree_store: RwLock::new(None),
            buffer_snapshots: BufferSnapshotStore::new(),
            overlays: RwLock::new(HashMap::new()),

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

    pub fn index_schema_snapshot(&self) -> Result<IndexSchemaSnapshot, LanguageError> {
        self.index_schema_snapshot_for_path(None)
    }

    pub fn index_schema_snapshot_for_path(
        &self,
        scope_path: Option<&str>,
    ) -> Result<IndexSchemaSnapshot, LanguageError> {
        let all_indexed_files = self.symbol_store.list_all_indexed_files()?;
        let normalized_scope = scope_path.and_then(normalize_schema_scope_path);
        let indexed_files = match normalized_scope.as_deref() {
            Some(scope) => all_indexed_files
                .iter()
                .filter(|record| schema_path_matches_scope(&record.file_path, scope))
                .cloned()
                .collect::<Vec<_>>(),
            None => all_indexed_files.clone(),
        };
        let symbols_by_type = self
            .symbol_store
            .symbol_type_counts_for_scope(normalized_scope.as_deref())?
            .into_iter()
            .map(|(name, count)| IndexSchemaCount { name, count })
            .collect::<Vec<_>>();
        let semantic_anchors_by_kind = self
            .symbol_store
            .semantic_anchor_kind_counts_for_scope(normalized_scope.as_deref())?
            .into_iter()
            .map(|(name, count)| IndexSchemaCount { name, count })
            .collect::<Vec<_>>();
        let relationships = self
            .symbol_store
            .relationship_integrity_stats_for_scope(normalized_scope.as_deref())?;

        let mut extension_counts = BTreeMap::<String, usize>::new();
        let mut language_counts = BTreeMap::<(String, String), usize>::new();
        let mut support_counts = BTreeMap::<String, usize>::new();

        for record in &indexed_files {
            let extension = Path::new(&record.file_path)
                .extension()
                .and_then(|extension| extension.to_str())
                .filter(|extension| !extension.is_empty())
                .map(|extension| extension.to_ascii_lowercase())
                .unwrap_or_else(|| "(none)".to_string());
            *extension_counts.entry(extension).or_default() += 1;

            match Language::capability_for_path(&record.file_path) {
                Some(capability) => {
                    let support_level = support_level_label(capability.support).to_string();
                    *language_counts
                        .entry((capability.display_name.to_string(), support_level.clone()))
                        .or_default() += 1;
                    *support_counts.entry(support_level).or_default() += 1;
                }
                None => {
                    *language_counts
                        .entry(("Unsupported/unknown".to_string(), "unsupported".to_string()))
                        .or_default() += 1;
                    *support_counts.entry("unsupported".to_string()).or_default() += 1;
                }
            }
        }

        let symbol_total = symbols_by_type.iter().map(|entry| entry.count).sum();
        let semantic_anchor_total = semantic_anchors_by_kind
            .iter()
            .map(|entry| entry.count)
            .sum();
        let totals = IndexSchemaTotals {
            indexed_files: indexed_files.len(),
            symbols: symbol_total,
            relationships: relationships.total_relationships,
            semantic_anchors: semantic_anchor_total,
        };

        let scope = match (scope_path, normalized_scope.as_deref()) {
            (Some(requested_path), Some(normalized_path)) => Some(IndexSchemaScope {
                requested_path: requested_path.to_string(),
                normalized_path: normalized_path.to_string(),
                root_totals: index_schema_root_totals(all_indexed_files.len(), &self.symbol_store)?,
            }),
            _ => None,
        };

        Ok(IndexSchemaSnapshot {
            totals,
            scope,
            files_by_extension: extension_counts
                .into_iter()
                .map(|(name, count)| IndexSchemaCount { name, count })
                .collect(),
            files_by_language: language_counts
                .into_iter()
                .map(
                    |((language, support_level), count)| IndexSchemaLanguageCount {
                        language,
                        support_level,
                        count,
                    },
                )
                .collect(),
            files_by_support_level: support_counts
                .into_iter()
                .map(|(name, count)| IndexSchemaCount { name, count })
                .collect(),
            symbols_by_type,
            relationships,
            semantic_anchors_by_kind,
        })
    }

    fn update_index_timings<F>(&self, update: F)
    where
        F: FnOnce(&mut IndexTimingSnapshot),
    {
        let mut health = self.index_health.write().unwrap();
        update(&mut health.timings);
    }

    fn update_index_discovery(&self, discovery: IndexDiscoverySnapshot) {
        let mut health = self.index_health.write().unwrap();
        health.discovery = discovery;
    }

    fn indexed_file_needs_refresh(
        &self,
        file_path: &str,
        record: &crate::symbol_index::store::IndexedFileRecord,
        refresh_metadata_when_hash_matches: bool,
    ) -> Result<bool, LanguageError> {
        if self
            .buffer_snapshots
            .contains_live(&self.snapshot_key(file_path))
        {
            return Ok(false);
        }

        let resolved = self.resolve_path(file_path);
        let Ok(metadata) = file_index_metadata(&resolved) else {
            return Ok(true);
        };

        let extractor_version = Self::extractor_version_for_index_file(file_path);
        if !Self::indexed_extractor_version_matches(record, extractor_version) {
            return Ok(true);
        }

        if record.file_size == Some(metadata.file_size)
            && record.modified_at == Some(metadata.modified_at)
            && record.line_count.is_some()
        {
            return Ok(false);
        }

        let Ok(content) = std::fs::read_to_string(&resolved) else {
            return Ok(true);
        };
        let current_hash = compute_hash(&content);
        if current_hash.is_empty() || current_hash != record.file_hash {
            return Ok(true);
        }

        if refresh_metadata_when_hash_matches {
            self.symbol_store
                .mark_file_indexed_with_metadata_and_extractor_version(
                    file_path,
                    &record.file_hash,
                    record.symbol_count,
                    Some(metadata.file_size),
                    Some(source_line_count(&content)),
                    Some(metadata.modified_at),
                    extractor_version,
                )?;
        }

        Ok(false)
    }

    fn indexed_extractor_version_matches(
        record: &crate::symbol_index::store::IndexedFileRecord,
        expected: Option<u32>,
    ) -> bool {
        match expected {
            Some(version) => record.extractor_version == Some(version),
            None => true,
        }
    }

    fn extractor_version_for_index_file(file_path: &str) -> Option<u32> {
        Language::capability_for_path(file_path)
            .map(|capability| capability.extractor_version)
            .or_else(|| {
                if is_anchor_only_index_file(file_path) {
                    Some(ANCHOR_ONLY_EXTRACTOR_VERSION)
                } else {
                    None
                }
            })
            .map(|base_version| {
                base_version
                    .saturating_mul(100)
                    .saturating_add(SEMANTIC_CONTEXT_EXTRACTOR_VERSION)
            })
    }

    pub fn audit_index_health(&self) -> Result<IndexHealthSnapshot, LanguageError> {
        let started = std::time::Instant::now();
        let current_health = self.index_health_snapshot();
        let timings = current_health.timings;
        let discovery = current_health.discovery;
        let supported_files = self.supported_language_files(".");
        let supported_set = supported_files
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let indexed_files = self.symbol_store.list_all_indexed_files()?;
        let indexed_map = indexed_files
            .iter()
            .map(|record| (record.file_path.as_str(), record))
            .collect::<HashMap<_, _>>();
        let mut stale_files = 0usize;
        let mut missing_files = 0usize;
        let mut orphaned_files = 0usize;

        for file_path in &supported_files {
            let Some(record) = indexed_map.get(file_path.as_str()) else {
                missing_files += 1;
                continue;
            };
            if self.indexed_file_needs_refresh(file_path, record, false)? {
                stale_files += 1;
            }
        }

        for record in &indexed_files {
            if !supported_set.contains(record.file_path.as_str()) {
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
            current_file: None,
            message,
            timings,
            discovery,
        })
    }

    pub fn audit_index_graph_quality(&self) -> Result<IndexGraphQualityReport, LanguageError> {
        let relationship_stats = self.symbol_store.relationship_integrity_stats()?;
        let indexed_files = self.symbol_store.list_all_indexed_files()?;
        let mut indexed_files_missing_root_symbol = 0usize;

        for record in indexed_files {
            if is_anchor_only_index_file(&record.file_path) {
                continue;
            }
            if self
                .symbol_store
                .get_symbol(&Self::synthetic_file_root_id(&record.file_path))?
                .is_none()
            {
                indexed_files_missing_root_symbol += 1;
            }
        }

        Ok(IndexGraphQualityReport::from_relationship_stats(
            relationship_stats,
            indexed_files_missing_root_symbol,
        ))
    }

    pub fn reconcile_index(&self) -> Result<IndexReconciliationReport, LanguageError> {
        self.reconcile_index_with_progress(|_| {})
    }

    pub fn reconcile_index_with_progress<F>(
        &self,
        progress: F,
    ) -> Result<IndexReconciliationReport, LanguageError>
    where
        F: FnMut(&IndexHealthSnapshot),
    {
        self.reconcile_index_with_progress_inner(progress, true)
    }

    fn reconcile_index_with_progress_inner<F>(
        &self,
        mut progress: F,
        allow_full_rebuild: bool,
    ) -> Result<IndexReconciliationReport, LanguageError>
    where
        F: FnMut(&IndexHealthSnapshot),
    {
        let started = std::time::Instant::now();

        // M6.1 — discover the supported-file set ONCE, up front. It feeds the
        // no-change fast path below and, on a miss, the full reconcile — replacing the
        // second tree walk that used to happen here.
        let supported_files = self.supported_language_files(".");

        // M6.1 — no-change fast path. Skipped when `allow_full_rebuild` is false, which
        // only happens on the forced graph-integrity rebuild recursion (the index was
        // just cleared, so there is real work to do and no checkpoint to trust).
        let (fast_report, mut reconcile_fingerprint) = if allow_full_rebuild {
            self.try_no_change_reconcile(&supported_files, started)
        } else {
            (None, self.compute_reconcile_fingerprint(&supported_files))
        };
        if let Some(report) = fast_report {
            self.set_index_health(report.health.clone());
            progress(&report.health);
            return Ok(report);
        }
        // Guarantee a fingerprint for end-of-reconcile storage even when no checkpoint
        // existed (cold start) — computed once here, from the pre-reconcile disk state.
        if reconcile_fingerprint.is_none() {
            reconcile_fingerprint = self.compute_reconcile_fingerprint(&supported_files);
        }

        let mut health = self.audit_index_health()?;
        health.status = IndexHealthStatus::Checking;
        health.message = "Checking symbol index".to_string();
        self.set_index_health(health.clone());
        progress(&health);

        let supported_set = supported_files
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let indexed_files = self.symbol_store.list_all_indexed_files()?;
        let indexed_map = indexed_files
            .iter()
            .map(|record| (record.file_path.as_str(), record))
            .collect::<HashMap<_, _>>();
        let mut queued_files = Vec::new();
        let mut files_removed = 0usize;

        for record in &indexed_files {
            if !supported_set.contains(record.file_path.as_str()) {
                self.remove_file(&record.file_path)?;
                files_removed += 1;
            }
        }

        for file_path in supported_files {
            let needs_index = match indexed_map.get(file_path.as_str()) {
                Some(record) => self.indexed_file_needs_refresh(&file_path, record, true)?,
                None => true,
            };
            if needs_index {
                queued_files.push(file_path);
            }
        }

        let total_queued = queued_files.len();
        let mut files_indexed = 0usize;
        let mut suppressed_external_relationships = 0usize;
        let mut derived_relationships_refreshed = false;
        if total_queued > 0 || files_removed > 0 {
            health.status = IndexHealthStatus::Indexing;
            health.queued_files = total_queued;
            health.active_workers = usize::from(total_queued > 0);
            health.message = format!("Building symbol index... 0/{} files", total_queued);
            self.set_index_health(health.clone());
            progress(&health);
        }

        if total_queued > 0 {
            enum IndexWorkerEvent {
                Started(String),
                Finished(String, Result<StagedFileIndex, String>),
            }

            let mut completed_files = 0usize;
            let mut committed_any = false;
            // M5.10 — files that errored during the PARALLEL bulk pass (parse
            // timeout, transient load/extraction failure). They get a sequential
            // second pass below — single-threaded, so no worker contention and the
            // full per-thread stack/memory — which recovers files that failed only
            // under parallel pressure. Deliberate anchor-only skips are NOT here
            // (those are indexed with a root symbol; re-extracting them would just
            // re-add the generated-data bloat we skipped on purpose).
            let mut failed_files: Vec<String> = Vec::new();

            // M5.7 — stream the reconcile in bounded batches: index a chunk in
            // parallel, COMMIT it, then drop its staged data before the next chunk.
            // Peak memory is O(RECONCILE_BATCH_SIZE), not O(total_queued) — what
            // kept the Linux kernel (77k files) from blowing to 14 GiB. Each batch
            // is its own committed transaction, so an interruption leaves a valid,
            // partially-populated DB rather than a corrupt stub.
            // M5.15 — OVERLAP extract↔commit. A dedicated committer thread owns the
            // single-writer DB commit; while it writes batch N, the extraction
            // workers already stage batch N+1, so the cores are no longer idle
            // during each serial commit. The bounded channel gives backpressure
            // (peak memory stays O(batch)), and FIFO delivery preserves the
            // cross-file resolution order the commit-time enrichment relies on.
            // Staging never touches the store (workers only parse), so it races
            // safely against the committer's writes.
            let commit_outcome = std::thread::scope(
                |commit_scope| -> Result<usize, LanguageError> {
                    let (commit_tx, commit_rx) =
                        std::sync::mpsc::sync_channel::<Vec<StagedFileIndex>>(1);
                    // Large stack like the extraction workers: commit-time enrichment
                    // resolves module re-exports by recursively indexing target files
                    // (bounded by the M5.11 re-entrancy guard, but still deep on big
                    // re-export chains), which the default ~2 MB thread stack can't hold.
                    let committer = std::thread::Builder::new()
                        .stack_size(256 * 1024 * 1024)
                        .spawn_scoped(commit_scope, move || -> Result<usize, LanguageError> {
                            let mut suppressed = 0usize;
                            while let Ok(staged) = commit_rx.recv() {
                                suppressed += self.commit_staged_file_indexes(&staged)?;
                            }
                            Ok(suppressed)
                        })
                        .expect("spawn index committer thread");

                    for (batch_start, batch_end) in self.size_bounded_batch_ranges(&queued_files) {
                        let batch = &queued_files[batch_start..batch_end];
                        let worker_count = indexing_worker_count(batch.len());
                        let (tx, rx) = mpsc::channel::<IndexWorkerEvent>();
                        let mut active_files = HashSet::new();
                        let mut staged_files = Vec::with_capacity(batch.len());
                        let mut batch_completed = 0usize;
                        // M5.4 — throttle the IPC progress emits (reset per batch so each
                        // batch's first event paints immediately).
                        let mut last_emit: Option<std::time::Instant> = None;

                        // Shared work cursor instead of a static skip/step_by
                        // split: a worker that draws several unusually
                        // expensive files no longer strands its pre-assigned
                        // tail while the rest of the pool sits idle. Declared
                        // outside the scope so scoped workers may borrow it.
                        let next_file = std::sync::atomic::AtomicUsize::new(0);
                        std::thread::scope(|scope| {
                            let next_file = &next_file;
                            for _ in 0..worker_count {
                                let tx = tx.clone();
                                // M5.7 — index workers get a large (virtual, lazily-committed)
                                // stack: symbol/relationship extraction walks the AST
                                // recursively, and real C files (e.g. the kernel's deeply
                                // nested generated tables / macro expansions) can be deep
                                // enough to overflow the default 2 MB thread stack and abort
                                // the process.
                                std::thread::Builder::new()
                                    .stack_size(256 * 1024 * 1024)
                                    .spawn_scoped(scope, move || loop {
                                        let index = next_file
                                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                        let Some(file_path) = batch.get(index) else {
                                            break;
                                        };
                                        let _ =
                                            tx.send(IndexWorkerEvent::Started(file_path.clone()));
                                        let result = self
                                            .stage_file_index(file_path)
                                            .map_err(|error| error.to_string());
                                        let _ = tx.send(IndexWorkerEvent::Finished(
                                            file_path.clone(),
                                            result,
                                        ));
                                    })
                                    .expect("spawn index worker thread");
                            }
                            drop(tx);

                            while batch_completed < batch.len() {
                                let Ok(event) = rx.recv() else {
                                    break;
                                };
                                // Keep the drain loop LEAN. With the pool saturating every
                                // core this thread must keep up with the event firehose, so
                                // per-event work is just cheap bookkeeping — no string
                                // formatting, no IPC. The expensive status build + emit is
                                // throttled below (UI_EMIT_INTERVAL).
                                match event {
                                    IndexWorkerEvent::Started(file_path) => {
                                        active_files.insert(file_path.clone());
                                        health.current_file = Some(file_path);
                                    }
                                    IndexWorkerEvent::Finished(file_path, result) => {
                                        active_files.remove(&file_path);
                                        batch_completed += 1;
                                        match result {
                                            Ok(staged_file) => {
                                                staged_files.push(staged_file);
                                                files_indexed += 1;
                                            }
                                            Err(error) => {
                                                eprintln!(
                                            "[LanguageService] Failed to index {} (parallel pass): {}",
                                            file_path, error
                                        );
                                                // Defer for the sequential second pass.
                                                failed_files.push(file_path.clone());
                                            }
                                        }
                                        health.current_file = active_files.iter().next().cloned();
                                    }
                                }

                                // Throttled UI update (~10/s) — plus an unconditional emit on
                                // the batch's final event so the bar lands exactly on N/N.
                                let final_event = batch_completed == batch.len();
                                let should_emit = final_event
                                    || last_emit
                                        .map_or(true, |at| at.elapsed() >= UI_EMIT_INTERVAL);
                                if should_emit {
                                    let done = completed_files + batch_completed;
                                    health.queued_files = total_queued.saturating_sub(done);
                                    health.active_workers = active_files.len();
                                    health.message = match &health.current_file {
                                        Some(current_file) => format!(
                                            "Indexing {}... {}/{} files ({} workers)",
                                            current_file, done, total_queued, worker_count
                                        ),
                                        None => format!(
                                            "Building symbol index... {}/{} files",
                                            done, total_queued
                                        ),
                                    };
                                    self.set_index_health(health.clone());
                                    progress(&health);
                                    last_emit = Some(std::time::Instant::now());
                                }
                            }
                        });

                        completed_files += batch_completed;

                        if !staged_files.is_empty() {
                            // Hand this batch to the background committer and immediately
                            // begin staging the next. `send` blocks ONLY while the committer
                            // is still writing the previous batch (bounded channel =
                            // backpressure, capping staged memory). A send error means the
                            // committer stopped on a DB error — surfaced at `join` below.
                            if commit_tx.send(staged_files).is_err() {
                                break;
                            }
                            committed_any = true;
                        }
                    }

                    // Close the queue and wait for the committer to drain it; its
                    // Result (total suppressed edges, or the first DB error) is the
                    // scope's value.
                    drop(commit_tx);
                    committer.join().expect("index committer thread panicked")
                },
            );
            suppressed_external_relationships += commit_outcome?;

            // M5.10 — SECOND PASS: retry the files that errored in the parallel
            // pass, ONE AT A TIME. Sequential means no worker contention and the
            // full per-thread stack/memory per file, so a file that failed only
            // under parallel pressure can now succeed. Runs BEFORE the global
            // resolution passes so any recovered symbols participate. `failed_files`
            // is an exception list (normally empty / a handful). A file that fails
            // AGAIN is left unindexed + logged — it stays "missing" and is retried
            // on the next reconcile.
            if !failed_files.is_empty() {
                health.status = IndexHealthStatus::Indexing;
                health.active_workers = 1;
                health.current_file = None;
                health.queued_files = failed_files.len();
                health.message = format!("Retrying {} deferred file(s)...", failed_files.len());
                self.set_index_health(health.clone());
                progress(&health);

                let mut retried_staged = Vec::new();
                let mut recovered = 0usize;
                let mut still_failed = 0usize;
                for file_path in &failed_files {
                    match self.stage_file_index(file_path) {
                        Ok(staged) => {
                            retried_staged.push(staged);
                            recovered += 1;
                        }
                        Err(error) => {
                            still_failed += 1;
                            eprintln!(
                                "[LanguageService] {} failed again on the sequential retry: {}",
                                file_path, error
                            );
                        }
                    }
                }
                files_indexed += recovered;
                if !retried_staged.is_empty() {
                    suppressed_external_relationships +=
                        self.commit_staged_file_indexes(&retried_staged)?;
                    committed_any = true;
                }
                eprintln!(
                    "[LanguageService] Second pass: recovered {} / {} deferred file(s); {} still failing",
                    recovered,
                    failed_files.len(),
                    still_failed
                );
            }

            if committed_any {
                // Surface the post-extraction RESOLUTION phase. These two global
                // passes run after the worker loop and can take minutes on a large
                // repo; without a status update the UI froze on the last
                // "Indexing… N/N files" message and looked hung (owner report on the
                // Linux kernel). There is no intra-pass progress, but the phase
                // labels make it clear work is still happening.
                health.status = IndexHealthStatus::Indexing;
                health.active_workers = 1;
                health.current_file = None;
                health.queued_files = 0;
                health.message = "Resolving symbol relationships...".to_string();
                self.set_index_health(health.clone());
                progress(&health);

                let rust_stats =
                    rust_project::resolve_qualified_calls(&self.workspace_root, &self.symbol_store)?;
                if rust_stats.observations > 0 {
                    eprintln!(
                        "[SYMBOLS][RUST_QUALIFIED] observations={} resolved={} unresolved={} candidates_examined={} candidate_p50={} candidate_p95={} candidate_p99={} max_candidates={} comparisons_avoided={} duration_ms={} forms={:?} strategies={:?} unresolved_reasons={:?}",
                        rust_stats.observations,
                        rust_stats.resolved,
                        rust_stats.unresolved,
                        rust_stats.candidates_examined,
                        rust_stats.candidate_p50,
                        rust_stats.candidate_p95,
                        rust_stats.candidate_p99,
                        rust_stats.max_candidates,
                        rust_stats.estimated_comparisons_avoided,
                        rust_stats.duration_ms,
                        rust_stats.by_form,
                        rust_stats.by_strategy,
                        rust_stats.by_unresolved_reason,
                    );
                }

                // M2.4 — run the global-unique back-fill exactly once, after EVERY
                // batch has committed. Only now is COUNT(*) truly global; running it
                // per-batch would resolve against an incomplete symbol set
                // (order-dependent).
                self.symbol_store
                    .backfill_unresolved_relationship_targets()?;

                health.message = "Resolving cross-file method calls...".to_string();
                self.set_index_health(health.clone());
                progress(&health);

                // M5.1b — then mine the STILL-NULL call edges that carry a confident
                // recv_type via the GLOBAL cross-file receiver-type registry. Runs
                // last so it sees the fully-committed symbol+edge set and only
                // touches edges the prior two passes left NULL.
                self.symbol_store
                    .mine_receiver_type_relationship_targets()?;

                health.message = "Mining Go interface implementations...".to_string();
                self.set_index_health(health.clone());
                progress(&health);

                // Track C — derive implicit Go interface satisfaction (`implements`
                // edges) from the fully-committed global method sets. Go has no
                // explicit `implements` syntax, so this post-index mining pass is
                // the only source of those edges.
                self.symbol_store.mine_go_interface_implementations()?;
                self.symbol_store.resolve_semantic_anchor_targets(None)?;
                derived_relationships_refreshed = true;
            }
        }

        // Reaching this point means the no-change checkpoint was not trusted.
        // Refresh derived relationships even when every source row itself was
        // fresh: resolver/store projection upgrades and removed Cargo/module
        // files can change targets without making an individual Rust file stale.
        if !derived_relationships_refreshed {
            let rust_stats =
                rust_project::resolve_qualified_calls(&self.workspace_root, &self.symbol_store)?;
            if rust_stats.observations > 0 {
                eprintln!(
                    "[SYMBOLS][RUST_QUALIFIED] observations={} resolved={} unresolved={} candidates_examined={} candidate_p50={} candidate_p95={} candidate_p99={} max_candidates={} comparisons_avoided={} duration_ms={} forms={:?} strategies={:?} unresolved_reasons={:?}",
                    rust_stats.observations,
                    rust_stats.resolved,
                    rust_stats.unresolved,
                    rust_stats.candidates_examined,
                    rust_stats.candidate_p50,
                    rust_stats.candidate_p95,
                    rust_stats.candidate_p99,
                    rust_stats.max_candidates,
                    rust_stats.estimated_comparisons_avoided,
                    rust_stats.duration_ms,
                    rust_stats.by_form,
                    rust_stats.by_strategy,
                    rust_stats.by_unresolved_reason,
                );
            }
            self.symbol_store
                .backfill_unresolved_relationship_targets()?;
            self.symbol_store
                .mine_receiver_type_relationship_targets()?;
            self.symbol_store.mine_go_interface_implementations()?;
            self.symbol_store.resolve_semantic_anchor_targets(None)?;
        }

        if total_queued > 0 {
            // The closing health/graph-quality audits scan the whole symbol DB
            // (COUNT + integrity), which is non-trivial on a multi-GB index — label
            // the window so the UI does not look stalled here either.
            health.message = "Finalizing index...".to_string();
            self.set_index_health(health.clone());
            progress(&health);
        }

        let mut final_health = self.audit_index_health()?;
        final_health.last_full_scan_ms = Some(started.elapsed().as_millis() as u64);
        final_health.last_incremental_update_ms = Some(started.elapsed().as_millis() as u64);
        final_health.active_workers = 0;
        final_health.current_file = None;
        final_health.queued_files = final_health.stale_files + final_health.missing_files;
        let mut graph_quality = self.audit_index_graph_quality()?;
        graph_quality.suppressed_external_relationships = suppressed_external_relationships;
        let has_hard_graph_issues = graph_quality.missing_source_symbols > 0
            || graph_quality.missing_target_symbols > 0
            || graph_quality.indexed_files_missing_root_symbol > 0;

        if has_hard_graph_issues && allow_full_rebuild {
            let mut rebuild_health = final_health.clone();
            rebuild_health.status = IndexHealthStatus::Indexing;
            rebuild_health.active_workers = 1;
            rebuild_health.current_file = None;
            rebuild_health.message = format!(
                "Rebuilding symbol index after graph integrity issues ({} missing sources, {} missing targets, {} files missing roots)",
                graph_quality.missing_source_symbols,
                graph_quality.missing_target_symbols,
                graph_quality.indexed_files_missing_root_symbol
            );
            self.set_index_health(rebuild_health.clone());
            progress(&rebuild_health);

            self.file_cache.write().unwrap().clear();
            self.symbol_store.clear_generated_index_data()?;

            return self.reconcile_index_with_progress_inner(progress, false);
        }

        // M5.13 — every write for this reconcile is now committed (batches +
        // backfill + receiver-type mining ran above; the rebuild branch returned
        // its own recursion). Fold the WAL back into the main DB and truncate the
        // sidecar so it does not grow unbounded across runs and slow the next
        // open. Best-effort: only after real work, and a failure never fails the
        // index. (The rebuild branch above `return`s, so this runs exactly once,
        // on the terminal path.)
        if total_queued > 0 {
            if let Err(error) = self.symbol_store.checkpoint() {
                eprintln!(
                    "[LanguageService] post-index WAL checkpoint failed (non-fatal): {error}"
                );
            }
        }

        final_health.status = if final_health.queued_files == 0
            && final_health.orphaned_files == 0
            && !has_hard_graph_issues
        {
            IndexHealthStatus::Fresh
        } else {
            IndexHealthStatus::Partial
        };
        final_health.message = if has_hard_graph_issues {
            format!(
                "Code intelligence partial: graph integrity issues detected ({} missing sources, {} missing targets, {} files missing roots)",
                graph_quality.missing_source_symbols,
                graph_quality.missing_target_symbols,
                graph_quality.indexed_files_missing_root_symbol
            )
        } else if final_health.status == IndexHealthStatus::Fresh {
            format!(
                "Code intelligence ready: {}/{} symbol relationships resolved",
                graph_quality.resolved_relationships, graph_quality.total_relationships
            )
        } else {
            format!(
                "Code intelligence partial: {} files pending",
                final_health.queued_files
            )
        };
        self.set_index_health(final_health.clone());
        progress(&final_health);

        // M6.1 — persist (or invalidate) the no-change checkpoint. Store ONLY when the
        // index ended fully Fresh; any partial/stale/graph-broken end clears the prior
        // checkpoint so the next reopen re-verifies instead of trusting a stale
        // "Fresh". A vanished-file fingerprint (None) also clears it.
        if final_health.status == IndexHealthStatus::Fresh && !has_hard_graph_issues {
            match reconcile_fingerprint {
                Some(fingerprint) => self.store_reconcile_checkpoint(&ReconcileCheckpoint {
                    fingerprint,
                    health: final_health.clone(),
                    graph_quality: graph_quality.clone(),
                }),
                None => self.clear_reconcile_checkpoint(),
            }
        } else {
            self.clear_reconcile_checkpoint();
        }

        Ok(IndexReconciliationReport {
            health: final_health,
            files_indexed,
            files_removed,
            duration_ms: started.elapsed().as_millis() as u64,
            graph_quality,
            fast_path: false,
        })
    }

    /// M6.1 — cheap proof-of-no-change fingerprint over the supported-file set: a
    /// hash of every file's (path, size, mtime, extractor_version) — exactly the
    /// signals `indexed_file_needs_refresh` trusts to call a file fresh WITHOUT
    /// reading its contents. Returns None if any file cannot be stat'd (e.g. it
    /// vanished mid-walk), which forces the full reconcile.
    fn compute_reconcile_fingerprint(&self, supported_files: &[String]) -> Option<String> {
        let mut entries: Vec<(&str, u64, i64, Option<u32>)> =
            Vec::with_capacity(supported_files.len());
        for file_path in supported_files {
            let resolved = self.resolve_path(file_path);
            let metadata = file_index_metadata(&resolved).ok()?;
            entries.push((
                file_path.as_str(),
                metadata.file_size,
                metadata.modified_at,
                Self::extractor_version_for_index_file(file_path),
            ));
        }
        // `supported_language_files` order is not guaranteed → sort for determinism.
        entries.sort();
        let mut canonical = String::with_capacity(entries.len() * 56 + 48);
        canonical.push_str("store_schema\u{1f}");
        canonical.push_str(&SYMBOL_STORE_SCHEMA_VERSION.to_string());
        canonical.push_str("\u{1f}rust_qualified_resolver\u{1f}");
        canonical.push_str(&rust_project::RUST_QUALIFIED_RESOLVER_VERSION.to_string());
        canonical.push('\n');
        canonical.push_str(&entries.len().to_string());
        for (path, size, mtime, extractor_version) in &entries {
            canonical.push('\n');
            canonical.push_str(path);
            canonical.push('\u{1f}');
            canonical.push_str(&size.to_string());
            canonical.push('\u{1f}');
            canonical.push_str(&mtime.to_string());
            canonical.push('\u{1f}');
            match extractor_version {
                Some(version) => canonical.push_str(&version.to_string()),
                None => canonical.push('-'),
            }
        }
        Some(compute_hash(&canonical))
    }

    /// M6.1 — attempt the no-change fast path. Returns `(Some(report), fingerprint)`
    /// when the worktree provably has not changed since the last fully-healthy
    /// reconcile (reusing that snapshot verbatim); otherwise `(None, fingerprint)` to
    /// fall through to the full reconcile. The returned fingerprint, when present, is
    /// the freshly-computed one so the caller can reuse it for end-of-reconcile
    /// storage without walking the tree twice. Fail-safe: every uncertainty (no
    /// checkpoint, a prior snapshot that was not fully Fresh, a mutated index-file
    /// count, or an un-stattable file) yields a miss.
    fn try_no_change_reconcile(
        &self,
        supported_files: &[String],
        started: std::time::Instant,
    ) -> (Option<IndexReconciliationReport>, Option<String>) {
        let Some(checkpoint) = self.load_reconcile_checkpoint() else {
            return (None, None);
        };
        let prior = &checkpoint.health;
        let prior_is_fully_fresh = matches!(prior.status, IndexHealthStatus::Fresh)
            && prior.stale_files == 0
            && prior.missing_files == 0
            && prior.orphaned_files == 0
            && prior.queued_files == 0;
        if !prior_is_fully_fresh {
            return (None, None);
        }
        // The index must still hold exactly the files the healthy snapshot recorded. A
        // cheap COUNT catches any out-of-band index mutation (e.g. a file removed from
        // the index) that a disk-only fingerprint would miss.
        match self.symbol_store.indexed_file_count() {
            Ok(count) if count == prior.indexed_files => {}
            _ => return (None, None),
        }
        let Some(fingerprint) = self.compute_reconcile_fingerprint(supported_files) else {
            return (None, None);
        };
        if fingerprint != checkpoint.fingerprint {
            // Changed — full path, but hand the fresh fingerprint back for storage.
            return (None, Some(fingerprint));
        }

        // Provably unchanged → reuse the last fully-healthy snapshot verbatim.
        let mut health = checkpoint.health;
        health.active_workers = 0;
        health.current_file = None;
        health.queued_files = 0;
        health.last_incremental_update_ms = Some(started.elapsed().as_millis() as u64);
        let report = IndexReconciliationReport {
            health,
            files_indexed: 0,
            files_removed: 0,
            duration_ms: started.elapsed().as_millis() as u64,
            graph_quality: checkpoint.graph_quality,
            fast_path: true,
        };
        (Some(report), Some(fingerprint))
    }

    /// M6.1 — load the persisted reconcile checkpoint (best-effort; any read or
    /// deserialization failure yields None → full reconcile).
    fn load_reconcile_checkpoint(&self) -> Option<ReconcileCheckpoint> {
        let raw = self
            .symbol_store
            .get_index_meta(RECONCILE_CHECKPOINT_KEY)
            .ok()??;
        serde_json::from_str(&raw).ok()
    }

    /// M6.1 — persist the reconcile checkpoint (best-effort; a write failure just
    /// means the next reconcile does full work).
    fn store_reconcile_checkpoint(&self, checkpoint: &ReconcileCheckpoint) {
        if let Ok(raw) = serde_json::to_string(checkpoint) {
            let _ = self
                .symbol_store
                .set_index_meta(RECONCILE_CHECKPOINT_KEY, &raw);
        }
    }

    /// M6.1 — invalidate the reconcile checkpoint.
    fn clear_reconcile_checkpoint(&self) {
        let _ = self
            .symbol_store
            .delete_index_meta(RECONCILE_CHECKPOINT_KEY);
    }

    pub fn get_file_content(&self, file_path: &str) -> Result<String, LanguageError> {
        Ok(self.load_buffer_snapshot(file_path)?.to_string())
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

    fn extract_file_symbols_and_relationships<'a>(
        &self,
        file_path: &str,
        content: &'a str,
        language: Language,
    ) -> Result<SymbolExtraction<'a>, LanguageError> {
        if matches!(language, Language::Markdown) {
            return Ok(SymbolExtraction {
                symbols: extract_markdown_header_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_stylesheet_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_css_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if matches!(language, Language::Vue | Language::Svelte) {
            return self.extract_component_file_symbols_and_relationships(file_path, content);
        }
        if language.is_markup_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_markup_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_config_scanner() {
            let symbols = extract_config_symbols(file_path, content, language);
            // M4.3 PART 2 — surface kustomize `Import` symbols as Import edges.
            let relationships = derive_config_import_relationships(file_path, &symbols);
            return Ok(SymbolExtraction {
                symbols,
                relationships,
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_php_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_php_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_java_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_java_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_csharp_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_csharp_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_kotlin_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_kotlin_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_ruby_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_ruby_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_shell_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_shell_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_dockerfile_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_dockerfile_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_sql_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_sql_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }
        if language.is_build_script_scanner() {
            return Ok(SymbolExtraction {
                symbols: extract_build_script_symbols(file_path, content),
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language,
            });
        }

        let (extraction_content, extraction_language) = if matches!(language, Language::Astro) {
            (Cow::Owned(astro_script_projection(content)), Language::Tsx)
        } else {
            (Cow::Borrowed(content), language)
        };

        let tree =
            parse_with_thread_local_parser(extraction_content.as_ref(), extraction_language)?;
        // Per-file facts (routes + module constants) are collected ONCE here and
        // shared by both extraction passes (the separate `extract_symbols` /
        // `extract_symbol_relationships` entry points would each re-collect
        // them). The Astro component symbol must still be appended BETWEEN the
        // passes: it participates in relationship extraction (e.g. as a by-name
        // route-handler candidate).
        let facts = collect_extraction_facts(
            &tree,
            extraction_content.as_ref(),
            extraction_language,
        );
        let mut symbols = extract_symbols_with_facts(
            &tree,
            extraction_content.as_ref(),
            extraction_language,
            file_path,
            &facts,
        );
        if matches!(language, Language::Astro) {
            if let Some(component_symbol) = astro_component_symbol(file_path, content) {
                symbols.push(component_symbol);
            }
        }
        let relationships = extract_symbol_relationships_with_facts(
            &tree,
            extraction_content.as_ref(),
            extraction_language,
            file_path,
            &symbols,
            &facts,
        );

        Ok(SymbolExtraction {
            symbols,
            relationships,
            content: extraction_content,
            language: extraction_language,
        })
    }

    fn extract_component_file_symbols_and_relationships<'a>(
        &self,
        file_path: &str,
        content: &'a str,
    ) -> Result<SymbolExtraction<'a>, LanguageError> {
        let mut symbols = extract_markup_symbols(file_path, content);

        if let Some(style_projection) = tag_body_projection(content, "style") {
            symbols.extend(extract_css_symbols(file_path, &style_projection));
        }

        let Some(script_projection) = tag_body_projection(content, "script") else {
            return Ok(SymbolExtraction {
                symbols,
                relationships: Vec::new(),
                content: Cow::Borrowed(content),
                language: Language::Html,
            });
        };

        let tree = parse_with_thread_local_parser(&script_projection, Language::Tsx)?;
        // Same shared per-file-facts shape as the tree-sitter file path above.
        let facts = collect_extraction_facts(&tree, &script_projection, Language::Tsx);
        let mut script_symbols = extract_symbols_with_facts(
            &tree,
            &script_projection,
            Language::Tsx,
            file_path,
            &facts,
        );
        let relationships = extract_symbol_relationships_with_facts(
            &tree,
            &script_projection,
            Language::Tsx,
            file_path,
            &script_symbols,
            &facts,
        );
        symbols.append(&mut script_symbols);

        Ok(SymbolExtraction {
            symbols,
            relationships,
            content: Cow::Owned(script_projection),
            language: Language::Tsx,
        })
    }

    fn enrich_symbol_relationships(
        &self,
        file_path: &str,
        extraction_content: &str,
        extraction_language: Language,
        source_language: Language,
        symbols: &[Symbol],
        relationships: &mut Vec<SymbolRelationship>,
    ) -> Result<usize, LanguageError> {
        self.canonicalize_import_relationships(file_path, relationships);
        self.append_stylesheet_usage_relationships(
            file_path,
            extraction_content,
            extraction_language,
            symbols,
            relationships,
        );
        self.append_stylesheet_custom_property_usage_relationships(
            file_path,
            extraction_content,
            extraction_language,
            symbols,
            relationships,
        );
        self.append_module_export_relationships(
            file_path,
            extraction_content,
            extraction_language,
            symbols,
            relationships,
        );
        if matches!(source_language, Language::Astro) {
            self.append_astro_component_export_relationship(file_path, symbols, relationships);
        }
        let translation_call_aliases = extract_translation_call_aliases(extraction_content);
        self.resolve_relationship_targets(symbols, relationships)?;
        Ok(Self::suppress_known_external_relationships(
            extraction_language,
            relationships,
            &translation_call_aliases,
        ))
    }

    /// Index a single file
    pub fn index_file(&self, file_path: &str) -> Result<Vec<Symbol>, LanguageError> {
        self.index_file_with_timings(file_path)
    }

    fn index_file_with_timings(&self, file_path: &str) -> Result<Vec<Symbol>, LanguageError> {
        // M5.11 — re-entrancy guard against cyclic module graphs.
        //
        // `enrich_symbol_relationships` resolves module re-exports
        // (`export * from './sibling'`, Python `__init__` package re-exports) by
        // indexing the target module file: `get_file_module_symbol` →
        // `ensure_file_fresh` → `index_file`, which itself enriches and resolves
        // *its* re-exports. A cycle in the module graph (JS barrel files,
        // `__init__.py` packages — both pervasive in e.g. Firefox) makes this
        // recurse without bound: it overflows the worker stack on small stacks and
        // grinds at 100% CPU "stuck on one file" on the 256 MiB worker stacks.
        //
        // Track the files currently being indexed on THIS thread. If asked to
        // index one that is already in flight higher on the stack, do not
        // re-enter — return whatever symbols it has committed so far. The in-flight
        // file's own top-level call finishes its indexing; the only effect is that
        // a back-edge in a re-export *cycle* may be skipped (acceptable — the
        // forward edge is still recorded, and there is no hang).
        thread_local! {
            static INDEXING_STACK: std::cell::RefCell<std::collections::HashSet<String>> =
                std::cell::RefCell::new(std::collections::HashSet::new());
        }
        struct StackGuard(String);
        impl Drop for StackGuard {
            fn drop(&mut self) {
                INDEXING_STACK.with(|stack| {
                    stack.borrow_mut().remove(&self.0);
                });
            }
        }
        let newly_entered =
            INDEXING_STACK.with(|stack| stack.borrow_mut().insert(file_path.to_string()));
        if !newly_entered {
            return Ok(self.get_file_symbols_raw(file_path).unwrap_or_default());
        }
        let _stack_guard = StackGuard(file_path.to_string());

        let total_start = std::time::Instant::now();
        let load_start = std::time::Instant::now();
        let disk_metadata = file_index_metadata(&self.resolve_path(file_path)).ok();
        let snapshot = self.load_snapshot_for_indexing(file_path)?;
        let content = snapshot.content();
        let hash = snapshot.hash().to_string();
        let index_metadata = if snapshot.is_live() {
            None
        } else {
            disk_metadata
        };
        let load_ms = load_start.elapsed().as_millis() as u64;

        // Check if reindexing is needed
        let freshness_start = std::time::Instant::now();
        let extractor_version = Self::extractor_version_for_index_file(file_path);
        let existing_index_record = self.symbol_store.indexed_file_record(file_path)?;
        let is_fresh = existing_index_record.as_ref().is_some_and(|record| {
            record.file_hash == hash
                && Self::indexed_extractor_version_matches(record, extractor_version)
        });
        if is_fresh {
            let mut db_write_ms = None;
            let symbols = self.get_file_symbols_raw(file_path)?;
            if self
                .symbol_store
                .get_semantic_anchors_in_file(file_path, 1)?
                .is_empty()
            {
                let mut anchors = extract_semantic_anchors(file_path, &content);
                attach_semantic_anchor_context(&mut anchors, &symbols);
                let db_write_start = std::time::Instant::now();
                self.symbol_store
                    .replace_semantic_anchors_for_file(file_path, &anchors)?;
                db_write_ms = Some(db_write_start.elapsed().as_millis() as u64);
            }
            self.update_index_timings(|timings| {
                timings.last_file_path = Some(file_path.to_string());
                timings.last_file_total_ms = Some(total_start.elapsed().as_millis() as u64);
                timings.last_file_load_ms = Some(load_ms);
                timings.last_file_freshness_check_ms =
                    Some(freshness_start.elapsed().as_millis() as u64);
                timings.last_file_parse_extract_ms = Some(0);
                timings.last_file_relationship_enrichment_ms = Some(0);
                timings.last_file_db_write_ms = db_write_ms.or(Some(0));
                timings.last_file_cache_update_ms = Some(0);
            });
            let visible_symbols = self.filter_visible_symbols(file_path, symbols);
            return Ok(visible_symbols);
        }
        let freshness_ms = freshness_start.elapsed().as_millis() as u64;

        // Detect language and parse
        let Some(language) = snapshot
            .language()
            .or_else(|| Language::from_path(file_path))
        else {
            if is_anchor_only_index_file(file_path) {
                let metrics = IndexFileMetrics {
                    load_ms,
                    freshness_check_ms: freshness_ms,
                    ..IndexFileMetrics::default()
                };
                return self.index_anchor_only_file(
                    file_path,
                    &hash,
                    index_metadata,
                    snapshot.clone(),
                    content,
                    total_start,
                    metrics,
                );
            }
            return Err(LanguageError::NotSupported(format!(
                "Unknown language for: {}",
                file_path
            )));
        };

        let parse_extract_start = std::time::Instant::now();
        let SymbolExtraction {
            symbols: extracted_symbols,
            mut relationships,
            content: extraction_content,
            language: extraction_language,
        } = self.extract_file_symbols_and_relationships(file_path, &content, language)?;
        // M5.7 — cap pathological files before adding the file-root symbol (so the
        // root is always retained). Truncated symbols simply aren't indexed; any
        // relationship pointing at a dropped symbol stays unresolved (handled).
        let mut extracted_symbols = extracted_symbols;
        if extracted_symbols.len() > MAX_SYMBOLS_PER_FILE {
            eprintln!(
                "[LanguageService] {} produced {} symbols; capping to {} (pathological/generated file)",
                file_path,
                extracted_symbols.len(),
                MAX_SYMBOLS_PER_FILE
            );
            extracted_symbols.truncate(MAX_SYMBOLS_PER_FILE);
        }
        let symbols = self.with_file_root_symbol(file_path, &content, extracted_symbols);
        let parse_extract_ms = parse_extract_start.elapsed().as_millis() as u64;

        let relationship_start = std::time::Instant::now();
        self.enrich_symbol_relationships(
            file_path,
            extraction_content.as_ref(),
            extraction_language,
            language,
            &symbols,
            &mut relationships,
        )?;

        // Delete old symbols and insert new ones
        let mut semantic_anchors = extract_semantic_anchors(file_path, &content);
        attach_semantic_anchor_context(&mut semantic_anchors, &symbols);
        let relationship_ms = relationship_start.elapsed().as_millis() as u64;

        let db_write_start = std::time::Instant::now();
        self.symbol_store.replace_file_index(
            file_path,
            &hash,
            index_metadata.map(|metadata| metadata.file_size),
            Some(source_line_count(&content)),
            index_metadata.map(|metadata| metadata.modified_at),
            extractor_version,
            &symbols,
            &semantic_anchors,
            &relationships,
        )?;
        self.symbol_store
            .resolve_semantic_anchor_targets(Some(file_path))?;
        if is_rust_project_resolution_input(file_path) {
            rust_project::resolve_qualified_calls(&self.workspace_root, &self.symbol_store)?;
        }
        // M2.4 — incremental single-file reindex resolves edges same-file/imported
        // only; the global-unique back-fill is deferred to the next full reindex
        // so its COUNT(*) stays truly global (not "unique among files seen so far").
        //
        // Track C — but mined Go `implements` edges MUST be re-derived here:
        // `replace_file_index` deletes relationships by source_file_path, which
        // wipes this file's types' mined implicit-interface edges, and nothing
        // else re-mines them before the next full workspace index.
        self.remine_go_interface_implementations_after_single_file_index(file_path);
        let db_write_ms = db_write_start.elapsed().as_millis() as u64;

        // Update cache
        let cache_start = std::time::Instant::now();
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
        let cache_update_ms = cache_start.elapsed().as_millis() as u64;
        self.update_index_timings(|timings| {
            timings.last_file_path = Some(file_path.to_string());
            timings.last_file_total_ms = Some(total_start.elapsed().as_millis() as u64);
            timings.last_file_load_ms = Some(load_ms);
            timings.last_file_freshness_check_ms = Some(freshness_ms);
            timings.last_file_parse_extract_ms = Some(parse_extract_ms);
            timings.last_file_relationship_enrichment_ms = Some(relationship_ms);
            timings.last_file_db_write_ms = Some(db_write_ms);
            timings.last_file_cache_update_ms = Some(cache_update_ms);
        });

        let visible_symbols = self.filter_visible_symbols(file_path, symbols);
        Ok(visible_symbols)
    }

    fn index_anchor_only_file(
        &self,
        file_path: &str,
        hash: &str,
        index_metadata: Option<FileIndexMetadata>,
        snapshot: Arc<BufferSnapshot>,
        content: &str,
        total_start: std::time::Instant,
        mut metrics: IndexFileMetrics,
    ) -> Result<Vec<Symbol>, LanguageError> {
        let anchors = extract_semantic_anchors(file_path, content);
        let db_write_start = std::time::Instant::now();
        self.symbol_store.replace_file_index(
            file_path,
            hash,
            index_metadata.as_ref().map(|metadata| metadata.file_size),
            Some(source_line_count(content)),
            index_metadata.as_ref().map(|metadata| metadata.modified_at),
            Self::extractor_version_for_index_file(file_path),
            &[],
            &anchors,
            &[],
        )?;
        if is_rust_project_resolution_input(file_path) {
            rust_project::resolve_qualified_calls(&self.workspace_root, &self.symbol_store)?;
        }
        metrics.db_write_ms = db_write_start.elapsed().as_millis() as u64;

        let cache_start = std::time::Instant::now();
        let mut cache = self.file_cache.write().unwrap();
        cache.insert(
            file_path.to_string(),
            CachedFile {
                hash: hash.to_string(),
                _snapshot: snapshot,
                symbols: Vec::new(),
            },
        );
        metrics.cache_update_ms = cache_start.elapsed().as_millis() as u64;
        metrics.anchors = anchors.len();
        metrics.total_ms = total_start.elapsed().as_millis() as u64;

        self.update_index_timings(|timings| {
            timings.last_file_path = Some(file_path.to_string());
            timings.last_file_total_ms = Some(metrics.total_ms);
            timings.last_file_load_ms = Some(metrics.load_ms);
            timings.last_file_freshness_check_ms = Some(metrics.freshness_check_ms);
            timings.last_file_parse_extract_ms = Some(metrics.parse_extract_ms);
            timings.last_file_relationship_enrichment_ms = Some(metrics.relationship_enrichment_ms);
            timings.last_file_db_write_ms = Some(metrics.db_write_ms);
            timings.last_file_cache_update_ms = Some(metrics.cache_update_ms);
        });

        Ok(Vec::new())
    }

    /// M5.7 — split `files` (in order) into contiguous batches each bounded by
    /// `BATCH_BYTE_BUDGET` of on-disk content AND `RECONCILE_BATCH_SIZE` files,
    /// whichever comes first, returning `(start, end)` index ranges into `files`.
    /// A single file larger than the byte budget becomes its own batch. Bounding by
    /// bytes (not just count) keeps peak staging memory flat even where file sizes
    /// vary by 1000× (e.g. the kernel's generated multi-MB headers).
    fn size_bounded_batch_ranges(&self, files: &[String]) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut start = 0usize;
        let mut bytes: u64 = 0;
        for (i, file) in files.iter().enumerate() {
            let size = self
                .resolve_path(file)
                .metadata()
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            let count = i - start;
            if count > 0 && (bytes + size > BATCH_BYTE_BUDGET || count >= RECONCILE_BATCH_SIZE) {
                ranges.push((start, i));
                start = i;
                bytes = 0;
            }
            bytes += size;
        }
        if start < files.len() {
            ranges.push((start, files.len()));
        }
        ranges
    }

    fn stage_file_index(&self, file_path: &str) -> Result<StagedFileIndex, LanguageError> {
        Ok(self.stage_file_index_with_metrics(file_path)?.staged)
    }

    fn stage_file_index_with_metrics(
        &self,
        file_path: &str,
    ) -> Result<StagedFileIndexOutcome, LanguageError> {
        let total_start = std::time::Instant::now();
        let load_start = std::time::Instant::now();
        let disk_metadata = file_index_metadata(&self.resolve_path(file_path)).ok();
        let snapshot = self.load_snapshot_for_indexing(file_path)?;
        let content = snapshot.content().to_string();
        let hash = snapshot.hash().to_string();
        let index_metadata = if snapshot.is_live() {
            None
        } else {
            disk_metadata
        };
        let load_ms = load_start.elapsed().as_millis() as u64;

        let Some(language) = snapshot
            .language()
            .or_else(|| Language::from_path(file_path))
        else {
            if is_anchor_only_index_file(file_path) {
                let parse_extract_start = std::time::Instant::now();
                let anchors = extract_semantic_anchors(file_path, &content);
                let parse_extract_ms = parse_extract_start.elapsed().as_millis() as u64;
                let staged = StagedFileIndex {
                    file_path: file_path.to_string(),
                    hash,
                    file_size: index_metadata.map(|metadata| metadata.file_size),
                    line_count: source_line_count(&content),
                    modified_at: index_metadata.map(|metadata| metadata.modified_at),
                    extractor_version: Self::extractor_version_for_index_file(file_path),
                    symbols: Vec::new(),
                    anchors,
                    relationships: Vec::new(),
                    extraction_content: content,
                    extraction_language: Language::Markdown,
                    source_language: Language::Markdown,
                };
                return Ok(StagedFileIndexOutcome {
                    metrics: IndexFileMetrics {
                        anchors: staged.anchors.len(),
                        total_ms: total_start.elapsed().as_millis() as u64,
                        load_ms,
                        parse_extract_ms,
                        ..IndexFileMetrics::default()
                    },
                    staged,
                });
            }
            return Err(LanguageError::NotSupported(format!(
                "Unknown language for: {}",
                file_path
            )));
        };

        let parse_extract_start = std::time::Instant::now();
        // M5.7 — oversized files (typically generated data, e.g. the kernel's
        // multi-megabyte AMD register-mask headers with 50k–190k `#define`s) are
        // indexed ANCHOR-ONLY: skip the recursive symbol walk entirely. This avoids
        // the huge transient symbol Vec + the parse cost, keeps the symbol DB from
        // bloating with unsearchable register macros, and still records the file
        // (via its root symbol) so it stays discoverable by path.
        let skip_extraction = content.len() > MAX_EXTRACT_BYTES || is_generated_path(file_path);
        let (symbols, mut relationships, extraction_content, extraction_language) =
            if skip_extraction {
                eprintln!(
                    "[LanguageService] {} — indexing anchor-only (skipping symbol extraction; {})",
                    file_path,
                    if content.len() > MAX_EXTRACT_BYTES {
                        format!(
                            "{:.1} MiB, likely generated",
                            content.len() as f64 / (1024.0 * 1024.0)
                        )
                    } else {
                        "generated-file name pattern".to_string()
                    }
                );
                (
                    self.with_file_root_symbol(file_path, &content, Vec::new()),
                    Vec::new(),
                    std::borrow::Cow::Borrowed(content.as_str()),
                    language,
                )
            } else {
                let SymbolExtraction {
                    symbols: extracted_symbols,
                    relationships,
                    content: extraction_content,
                    language: extraction_language,
                } = self.extract_file_symbols_and_relationships(file_path, &content, language)?;
                // M5.7 — cap pathological files before adding the file-root symbol (so
                // the root is always retained). Truncated symbols are simply not
                // indexed; any relationship pointing at a dropped symbol stays
                // unresolved (handled).
                let mut extracted_symbols = extracted_symbols;
                if extracted_symbols.len() > MAX_SYMBOLS_PER_FILE {
                    eprintln!(
                    "[LanguageService] {} produced {} symbols; capping to {} (pathological/generated file)",
                    file_path,
                    extracted_symbols.len(),
                    MAX_SYMBOLS_PER_FILE
                );
                    extracted_symbols.truncate(MAX_SYMBOLS_PER_FILE);
                }
                (
                    self.with_file_root_symbol(file_path, &content, extracted_symbols),
                    relationships,
                    extraction_content,
                    extraction_language,
                )
            };
        self.canonicalize_import_relationships(file_path, &mut relationships);
        let mut anchors = if skip_extraction {
            Vec::new()
        } else {
            extract_semantic_anchors(file_path, &content)
        };
        attach_semantic_anchor_context(&mut anchors, &symbols);
        let parse_extract_ms = parse_extract_start.elapsed().as_millis() as u64;

        let staged = StagedFileIndex {
            file_path: file_path.to_string(),
            hash,
            file_size: index_metadata.map(|metadata| metadata.file_size),
            line_count: source_line_count(&content),
            modified_at: index_metadata.map(|metadata| metadata.modified_at),
            extractor_version: Self::extractor_version_for_index_file(file_path),
            symbols,
            anchors,
            relationships,
            extraction_content: extraction_content.into_owned(),
            extraction_language,
            source_language: language,
        };
        Ok(StagedFileIndexOutcome {
            metrics: IndexFileMetrics {
                anchors: staged.anchors.len(),
                total_ms: total_start.elapsed().as_millis() as u64,
                load_ms,
                parse_extract_ms,
                ..IndexFileMetrics::default()
            },
            staged,
        })
    }

    fn commit_staged_file_indexes(
        &self,
        staged_files: &[StagedFileIndex],
    ) -> Result<usize, LanguageError> {
        Ok(self
            .commit_staged_file_indexes_with_metrics(staged_files)?
            .suppressed_external_relationships)
    }

    fn commit_staged_file_indexes_with_metrics(
        &self,
        staged_files: &[StagedFileIndex],
    ) -> Result<CommitStagedFileMetrics, LanguageError> {
        if staged_files.is_empty() {
            return Ok(CommitStagedFileMetrics::default());
        }

        let initial_records = staged_files
            .iter()
            .map(|file| {
                let mut relationships = file.relationships.clone();
                self.append_direct_module_export_relationships(
                    &file.file_path,
                    &file.extraction_content,
                    file.extraction_language,
                    &file.symbols,
                    &mut relationships,
                );
                if matches!(file.source_language, Language::Astro) {
                    self.append_astro_component_export_relationship(
                        &file.file_path,
                        &file.symbols,
                        &mut relationships,
                    );
                }

                FileIndexRecord {
                    file_path: file.file_path.clone(),
                    file_hash: file.hash.clone(),
                    file_size: file.file_size,
                    line_count: Some(file.line_count),
                    modified_at: file.modified_at,
                    extractor_version: file.extractor_version,
                    symbols: file.symbols.clone(),
                    anchors: file.anchors.clone(),
                    relationships,
                }
            })
            .collect::<Vec<_>>();

        let initial_db_write_start = std::time::Instant::now();
        self.symbol_store.replace_file_indexes(&initial_records)?;
        let mut db_write_ms = initial_db_write_start.elapsed().as_millis() as u64;

        let mut final_relationship_records = Vec::with_capacity(staged_files.len());
        let mut suppressed_external_relationships = 0usize;
        let relationship_start = std::time::Instant::now();
        for file in staged_files {
            let mut relationships = file.relationships.clone();
            suppressed_external_relationships += self.enrich_symbol_relationships(
                &file.file_path,
                &file.extraction_content,
                file.extraction_language,
                file.source_language,
                &file.symbols,
                &mut relationships,
            )?;
            final_relationship_records.push(FileRelationshipRecord {
                file_path: file.file_path.clone(),
                relationships,
            });
        }
        let relationship_enrichment_ms = relationship_start.elapsed().as_millis() as u64;
        let relationship_count = final_relationship_records
            .iter()
            .map(|record| record.relationships.len())
            .sum::<usize>();

        let relationship_db_write_start = std::time::Instant::now();
        self.symbol_store
            .replace_relationships_for_files(&final_relationship_records)?;
        // M2.4 — the set-based global-unique back-fill is NOT run here: this
        // helper commits one staged batch, and the global COUNT(*) must see every
        // file. The full-index callers (`reconcile_index_with_progress_inner`,
        // `index_directory`) run it exactly once after all files are committed.
        db_write_ms += relationship_db_write_start.elapsed().as_millis() as u64;

        // M5.15 — do NOT populate `file_cache` from the bulk commit path. This
        // helper only runs during full indexing (reconcile / index_directory), and
        // caching every committed file retained its source text (via the buffer
        // snapshot) plus a clone of its symbols for the whole run — a monotonic RAM
        // climb (~2 GB of source text + ~5M symbols across the 469k-file Firefox
        // index) with no payoff: the data is already in the DB, and this cache only
        // accelerates INTERACTIVE re-lookups of OPEN files, which `index_file` /
        // `did_open` still populate on demand.
        let cache_update_ms = 0;

        Ok(CommitStagedFileMetrics {
            suppressed_external_relationships,
            relationship_count,
            relationship_enrichment_ms,
            db_write_ms,
            cache_update_ms,
        })
    }

    /// Index an entire directory recursively
    pub fn index_directory(&self, dir_path: &str) -> Result<IndexStats, LanguageError> {
        let mut stats = IndexStats::default();
        let start = std::time::Instant::now();
        let discovery_start = std::time::Instant::now();
        let discovery_report = self.supported_language_discovery(dir_path);
        let discovery_ms = discovery_start.elapsed().as_millis() as u64;
        let files = discovery_report.files;

        self.update_index_timings(|timings| {
            timings.last_discovery_ms = Some(discovery_ms);
        });

        stats.files_discovered = discovery_report.discovered_files;
        stats.supported_files = files.len();
        stats.supported_by_language = language_counts_for_paths(&files);
        stats.skipped_by_reason = skip_counts_from_map(&discovery_report.skipped_by_reason);
        let indexed_files = self.symbol_store.list_all_indexed_files()?;
        let indexed_map = indexed_files
            .iter()
            .map(|record| (record.file_path.as_str(), record))
            .collect::<HashMap<_, _>>();
        let mut files_to_stage = Vec::new();

        for relative_path in &files {
            if let Some(record) = indexed_map.get(relative_path.as_str()) {
                let freshness_start = std::time::Instant::now();
                if !self.indexed_file_needs_refresh(relative_path, record, true)? {
                    stats.files_indexed += 1;
                    stats.symbols_extracted += record.symbol_count;
                    stats.files_fresh += 1;
                    stats.freshness_check_ms += freshness_start.elapsed().as_millis() as u64;
                    continue;
                }
            }

            files_to_stage.push(relative_path.clone());
        }

        // M5.7 — stream in bounded batches (same rationale as the reconcile path):
        // index a chunk, COMMIT it, then DROP its staged data so peak memory is
        // O(RECONCILE_BATCH_SIZE), not O(repo). Workers get a large (virtual,
        // lazily-committed) stack because symbol extraction walks the AST
        // recursively and deep real C files (the kernel) can overflow the default
        // 2 MB thread stack and abort the process.
        enum IndexDirectoryStageEvent {
            Finished(String, Result<StagedFileIndexOutcome, String>),
        }
        let mut committed_any = false;

        for (batch_start, batch_end) in self.size_bounded_batch_ranges(&files_to_stage) {
            let batch = &files_to_stage[batch_start..batch_end];
            let worker_count = indexing_worker_count(batch.len());
            let (tx, rx) = mpsc::channel::<IndexDirectoryStageEvent>();
            let mut completed_files = 0usize;
            let mut staged_files = Vec::with_capacity(batch.len());

            // Shared work cursor instead of a static skip/step_by split (same
            // rationale as the reconcile pool above).
            let next_file = std::sync::atomic::AtomicUsize::new(0);
            std::thread::scope(|scope| {
                let next_file = &next_file;
                for _ in 0..worker_count {
                    let tx = tx.clone();
                    std::thread::Builder::new()
                        .stack_size(256 * 1024 * 1024)
                        .spawn_scoped(scope, move || loop {
                            let index =
                                next_file.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(file_path) = batch.get(index) else {
                                break;
                            };
                            let result = self
                                .stage_file_index_with_metrics(file_path)
                                .map_err(|error| error.to_string());
                            let _ = tx.send(IndexDirectoryStageEvent::Finished(
                                file_path.clone(),
                                result,
                            ));
                        })
                        .expect("spawn index worker thread");
                }
                drop(tx);

                while completed_files < batch.len() {
                    let Ok(event) = rx.recv() else {
                        break;
                    };
                    match event {
                        IndexDirectoryStageEvent::Finished(relative_path, result) => {
                            completed_files += 1;
                            match result {
                                Ok(outcome) => {
                                    stats.files_indexed += 1;
                                    stats.symbols_extracted += Self::count_visible_symbols(
                                        &relative_path,
                                        &outcome.staged.symbols,
                                    );
                                    stats.files_reindexed += 1;
                                    stats.anchors_extracted += outcome.metrics.anchors;
                                    stats.load_ms += outcome.metrics.load_ms;
                                    stats.freshness_check_ms += outcome.metrics.freshness_check_ms;
                                    stats.parse_extract_ms += outcome.metrics.parse_extract_ms;
                                    staged_files.push(outcome.staged);
                                }
                                Err(_) => {
                                    stats.files_failed += 1;
                                }
                            }
                        }
                    }
                }
            });

            if !staged_files.is_empty() {
                let commit_metrics = self.commit_staged_file_indexes_with_metrics(&staged_files)?;
                stats.relationships_extracted += commit_metrics.relationship_count;
                stats.relationship_enrichment_ms += commit_metrics.relationship_enrichment_ms;
                stats.db_write_ms += commit_metrics.db_write_ms;
                stats.cache_update_ms += commit_metrics.cache_update_ms;
                committed_any = true;
            }
            // Free this batch's staged data before the next chunk.
            drop(staged_files);
        }

        if committed_any {
            let rust_stats =
                rust_project::resolve_qualified_calls(&self.workspace_root, &self.symbol_store)?;
            if rust_stats.observations > 0 {
                eprintln!(
                    "[SYMBOLS][RUST_QUALIFIED] observations={} resolved={} unresolved={} candidates_examined={} candidate_p50={} candidate_p95={} candidate_p99={} max_candidates={} comparisons_avoided={} duration_ms={}",
                    rust_stats.observations,
                    rust_stats.resolved,
                    rust_stats.unresolved,
                    rust_stats.candidates_examined,
                    rust_stats.candidate_p50,
                    rust_stats.candidate_p95,
                    rust_stats.candidate_p99,
                    rust_stats.max_candidates,
                    rust_stats.estimated_comparisons_avoided,
                    rust_stats.duration_ms,
                );
            }
            // M2.4 — run the global-unique back-fill exactly once, after EVERY
            // batch has committed (true global COUNT(*)).
            self.symbol_store
                .backfill_unresolved_relationship_targets()?;
            // M5.1b — mine the still-NULL recv_type-carrying call edges against
            // the GLOBAL receiver-type registry (runs after the back-fill).
            self.symbol_store
                .mine_receiver_type_relationship_targets()?;
            // Track C — derive implicit Go interface satisfaction (`implements`
            // edges) from the committed global method sets.
            self.symbol_store.mine_go_interface_implementations()?;
            self.symbol_store.resolve_semantic_anchor_targets(None)?;
        }

        stats.duration_ms = start.elapsed().as_millis() as u64;
        self.update_index_discovery(IndexDiscoverySnapshot {
            last_scope: Some(dir_path.to_string()),
            last_discovered_files: stats.files_discovered,
            last_supported_files: stats.supported_files,
            last_indexed_files: stats.files_indexed,
            last_failed_files: stats.files_failed,
            last_fresh_files: stats.files_fresh,
            last_reindexed_files: stats.files_reindexed,
            last_symbols_extracted: stats.symbols_extracted,
            last_anchors_extracted: stats.anchors_extracted,
            last_relationships_extracted: stats.relationships_extracted,
            supported_by_language: stats.supported_by_language.clone(),
            skipped_by_reason: stats.skipped_by_reason.clone(),
        });
        self.update_index_timings(|timings| {
            timings.last_batch_load_ms = Some(stats.load_ms);
            timings.last_batch_freshness_check_ms = Some(stats.freshness_check_ms);
            timings.last_batch_parse_extract_ms = Some(stats.parse_extract_ms);
            timings.last_batch_relationship_enrichment_ms = Some(stats.relationship_enrichment_ms);
            timings.last_batch_db_write_ms = Some(stats.db_write_ms);
            timings.last_batch_cache_update_ms = Some(stats.cache_update_ms);
        });
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

    /// Search symbols by query
    pub fn search_symbols(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>, LanguageError> {
        let search_query = SearchQuery::text(query).with_limit(limit);
        let results =
            crate::symbol_index::search::execute_search(&self.symbol_store, &search_query)?;
        Ok(self.filter_visible_search_results(
            self.merge_overlay_search_results(results, &search_query),
        ))
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
        Ok(self.merge_overlay_search_results(results, &search_query))
    }

    /// Search symbols with filters
    pub fn search_symbols_filtered(
        &self,
        query: &str,
        file_path: Option<&str>,
        symbol_types: Option<Vec<SymbolType>>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, LanguageError> {
        self.search_symbols_filtered_with_patterns(
            query,
            file_path,
            symbol_types,
            None,
            None,
            None,
            limit,
        )
    }

    pub fn search_symbols_filtered_with_patterns(
        &self,
        query: &str,
        file_path: Option<&str>,
        symbol_types: Option<Vec<SymbolType>>,
        file_pattern: Option<&str>,
        name_pattern: Option<&str>,
        qualified_name_pattern: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, LanguageError> {
        if let Some(path) = file_path {
            self.ensure_file_fresh(path)?;
        } else {
            let indexed_files = self.symbol_store.list_all_indexed_files()?;
            self.refresh_stale_indexed_files(&indexed_files)?;
        }

        let mut search_query = SearchQuery::text(query).with_limit(limit);

        if let Some(path) = file_path {
            search_query = search_query.with_file(path);
        }

        if let Some(types) = symbol_types {
            search_query = search_query.with_types(types);
        }

        if let Some(pattern) = file_pattern {
            search_query = search_query.with_file_pattern(pattern);
        }

        if let Some(pattern) = name_pattern {
            search_query = search_query.with_name_pattern(pattern);
        }

        if let Some(pattern) = qualified_name_pattern {
            search_query = search_query.with_qualified_name_pattern(pattern);
        }

        let results =
            crate::symbol_index::search::execute_search(&self.symbol_store, &search_query)?;
        Ok(self.merge_overlay_search_results(results, &search_query))
    }

    fn merge_overlay_search_results(
        &self,
        mut results: Vec<SearchResult>,
        query: &SearchQuery,
    ) -> Vec<SearchResult> {
        let overlays = self.overlays.read().unwrap();
        if overlays.is_empty() {
            return results;
        }

        let overlay_paths = overlays.keys().map(String::as_str).collect::<HashSet<_>>();
        results.retain(|result| !overlay_paths.contains(result.symbol.file_path.as_str()));
        let text = query.text.as_deref().unwrap_or_default();
        for document in overlays.values() {
            for symbol in &document.symbols {
                if Self::is_synthetic_file_root_symbol(symbol)
                    || !crate::symbol_index::search::symbol_matches_query_filters(symbol, query)
                {
                    continue;
                }
                let Some((score, breakdown)) =
                    crate::symbol_index::search::score_symbol_query(symbol, text)
                else {
                    continue;
                };
                let mut result = SearchResult::with_score(symbol.clone(), score);
                if query.explain {
                    result.score_breakdown = Some(breakdown);
                }
                results.push(result);
            }
        }
        results.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| {
                    left.symbol
                        .qualified_name
                        .cmp(&right.symbol.qualified_name)
                })
                .then_with(|| left.symbol.file_path.cmp(&right.symbol.file_path))
                .then_with(|| left.symbol.id.cmp(&right.symbol.id))
        });
        results.truncate(query.limit.unwrap_or(50).clamp(1, 100));
        results
    }

    pub fn search_symbols_filtered_self_healing(
        &self,
        query: &str,
        file_path: Option<&str>,
        symbol_types: Option<Vec<SymbolType>>,
        limit: usize,
    ) -> Result<SymbolSearchOutcome, LanguageError> {
        let initial_results =
            self.search_symbols_filtered(query, file_path, symbol_types.clone(), limit)?;
        let initial_results = self.filter_visible_search_results(initial_results);
        let initial_top_score = initial_results.first().map(|result| result.score);
        let mut healing = SymbolSearchHealingReport {
            enabled: file_path.is_none() && !query.trim().is_empty(),
            triggered: false,
            reason: None,
            confidence: search_confidence(&initial_results),
            initial_result_count: initial_results.len(),
            initial_top_score,
            reran_after_reindex: false,
            reindexed_files: Vec::new(),
            removed_files: Vec::new(),
            literal_matches: Vec::new(),
            semantic_anchor_matches: Vec::new(),
            diagnostics: Vec::new(),
            health_before: None,
            health_after: None,
        };

        if !healing.enabled {
            return Ok(SymbolSearchOutcome {
                results: initial_results,
                healing,
            });
        }

        let should_heal = initial_results.is_empty() || initial_top_score.unwrap_or(0.0) < 0.55;
        if !should_heal {
            return Ok(SymbolSearchOutcome {
                results: initial_results,
                healing,
            });
        }

        healing.triggered = true;
        healing.reason = Some(if initial_results.is_empty() {
            "empty_results".to_string()
        } else {
            "low_confidence_results".to_string()
        });

        let health_before = self.audit_index_health()?;
        self.set_index_health(health_before.clone());
        healing.health_before = Some(health_before.clone());
        if health_before.status != IndexHealthStatus::Fresh {
            healing.diagnostics.push(format!(
                "Symbol index health is {:?}: {} stale, {} missing, {} orphaned",
                health_before.status,
                health_before.stale_files,
                health_before.missing_files,
                health_before.orphaned_files
            ));
        }

        let repair_candidates = self.literal_repair_candidates(query, 16)?;
        for repair_path in repair_candidates {
            match self.index_file(&repair_path) {
                Ok(_) => healing.reindexed_files.push(repair_path),
                Err(error) => healing
                    .diagnostics
                    .push(format!("Failed to reindex {}: {}", repair_path, error)),
            }
        }

        if !healing.reindexed_files.is_empty() {
            healing.reran_after_reindex = true;
            let rerun_results = self.search_symbols_filtered(query, None, symbol_types, limit)?;
            let rerun_results = self.filter_visible_search_results(rerun_results);
            healing.confidence = search_confidence(&rerun_results);
            let health_after = self.audit_index_health()?;
            self.set_index_health(health_after.clone());
            healing.health_after = Some(health_after);
            if !rerun_results.is_empty() {
                return Ok(SymbolSearchOutcome {
                    results: rerun_results,
                    healing,
                });
            }
        } else {
            let health_after = self.audit_index_health()?;
            self.set_index_health(health_after.clone());
            healing.health_after = Some(health_after);
        }

        // AnyTerms, not the legacy contiguous-phrase match: healing queries are
        // natural language ("navigation toolbar"), and requiring the whole
        // phrase to appear verbatim manufactures false-empty fallbacks — the
        // exact failure mode the anchor query modes were added to eliminate.
        healing.semantic_anchor_matches = self
            .search_semantic_anchors_mode(query, None, 12, AnchorQueryMode::AnyTerms)?
            .results;
        healing.literal_matches = self.literal_symbol_search_fallback(query, 12)?;
        if initial_results.is_empty()
            && healing.semantic_anchor_matches.is_empty()
            && healing.literal_matches.is_empty()
        {
            healing.diagnostics.push(
                "No indexed symbols, semantic anchors, or exact literal fallback matches found"
                    .to_string(),
            );
        } else if initial_results.is_empty() {
            healing.diagnostics.push(format!(
                "Found {} semantic anchor matches and {} exact literal fallback matches outside symbol names",
                healing.semantic_anchor_matches.len(),
                healing.literal_matches.len()
            ));
        }

        Ok(SymbolSearchOutcome {
            results: initial_results,
            healing,
        })
    }

    /// Get symbol at position
    pub fn search_semantic_anchors(
        &self,
        query: &str,
        file_path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SemanticAnchorResult>, LanguageError> {
        Ok(self
            .search_semantic_anchors_mode(query, file_path, limit, AnchorQueryMode::Phrase)?
            .results)
    }

    /// Track H — mode-aware anchor search (`phrase` / `all_terms` / `any_terms`)
    /// with outcome metadata, so an empty phrase search is distinguishable from
    /// "no term matched". Same freshness pre-step as `search_semantic_anchors`.
    pub fn search_semantic_anchors_mode(
        &self,
        query: &str,
        file_path: Option<&str>,
        limit: usize,
        mode: AnchorQueryMode,
    ) -> Result<SemanticAnchorSearchOutcome, LanguageError> {
        if let Some(path) = file_path {
            self.ensure_file_fresh(path)?;
        }
        let persisted = self
            .symbol_store
            .search_semantic_anchors_mode(query, file_path, limit, mode)?;
        Ok(self.merge_overlay_anchor_results(persisted, query, file_path, limit, mode))
    }

    fn merge_overlay_anchor_results(
        &self,
        mut outcome: SemanticAnchorSearchOutcome,
        query: &str,
        file_path: Option<&str>,
        limit: usize,
        mode: AnchorQueryMode,
    ) -> SemanticAnchorSearchOutcome {
        let overlays = self.overlays.read().unwrap();
        if overlays.is_empty() {
            return outcome;
        }
        outcome.results.retain(|result| {
            !overlays.contains_key(&result.anchor.file_path)
                && file_path.is_none_or(|path| result.anchor.file_path == path)
        });
        let phrase = query.trim().to_ascii_lowercase();
        let terms = phrase
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        for (path, document) in &*overlays {
            if file_path.is_some_and(|scope| scope != path) {
                continue;
            }
            for anchor in &document.anchors {
                let haystack = format!("{} {}", anchor.value, anchor.preview).to_ascii_lowercase();
                let matched = terms
                    .iter()
                    .filter(|term| haystack.contains(term.as_str()))
                    .count();
                let accepted = match mode {
                    AnchorQueryMode::Phrase => !phrase.is_empty() && haystack.contains(&phrase),
                    AnchorQueryMode::AllTerms => !terms.is_empty() && matched == terms.len(),
                    AnchorQueryMode::AnyTerms => matched > 0,
                };
                if !accepted {
                    continue;
                }
                let coverage = if terms.is_empty() {
                    0.0
                } else {
                    matched as f32 / terms.len() as f32
                };
                outcome.results.push(SemanticAnchorResult {
                    anchor: anchor.clone(),
                    score: anchor.confidence * coverage.max(0.5),
                    matched_terms: (mode != AnchorQueryMode::Phrase).then_some(matched),
                    total_terms: (mode != AnchorQueryMode::Phrase).then_some(terms.len()),
                });
            }
        }
        outcome.results.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.anchor.file_path.cmp(&right.anchor.file_path))
                .then_with(|| left.anchor.line.cmp(&right.anchor.line))
        });
        outcome.results.truncate(limit.clamp(1, 100));
        outcome
    }

    pub fn get_file_semantic_anchors(
        &self,
        file_path: &str,
        limit: usize,
    ) -> Result<Vec<SemanticAnchor>, LanguageError> {
        if let Some(document) = self.overlays.read().unwrap().get(file_path) {
            return Ok(document.anchors.iter().take(limit).cloned().collect());
        }
        self.ensure_file_fresh(file_path)?;
        Ok(self
            .symbol_store
            .get_semantic_anchors_in_file(file_path, limit)?)
    }

    pub fn get_semantic_context_for_symbols(
        &self,
        symbol_ids: &[String],
        limit: usize,
    ) -> Result<Vec<SemanticAnchor>, LanguageError> {
        Ok(self
            .symbol_store
            .get_semantic_context_for_symbol_ids(symbol_ids, limit)?)
    }

    pub fn get_symbol_at(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<Symbol>, LanguageError> {
        if let Some(document) = self.overlays.read().unwrap().get(file_path) {
            let mut matches = document
                .symbols
                .iter()
                .filter(|symbol| {
                    let starts_before = (symbol.range.start.line, symbol.range.start.character)
                        <= (line, character);
                    let ends_after =
                        (symbol.range.end.line, symbol.range.end.character) >= (line, character);
                    starts_before && ends_after
                })
                .cloned()
                .collect::<Vec<_>>();
            matches.sort_by_key(|symbol| symbol.byte_length);
            return Ok(matches.into_iter().next());
        }
        self.ensure_file_fresh(file_path)?;
        let symbol = self
            .symbol_store
            .get_symbol_at(file_path, line, character)?
            .and_then(|symbol| self.normalize_visible_symbol(file_path, symbol));
        Ok(symbol)
    }

    pub fn get_symbol(&self, id: &str) -> Result<Option<Symbol>, LanguageError> {
        let overlays = self.overlays.read().unwrap();
        if let Some(symbol) = overlays
            .values()
            .flat_map(|document| &document.symbols)
            .find(|symbol| symbol.id == id)
        {
            return Ok(Some(symbol.clone()));
        }
        let persisted = self.symbol_store.get_symbol(id)?;
        Ok(persisted.filter(|symbol| !overlays.contains_key(&symbol.file_path)))
    }

    pub fn get_file_module_symbol(&self, file_path: &str) -> Result<Option<Symbol>, LanguageError> {
        if let Some(document) = self.overlays.read().unwrap().get(file_path) {
            return Ok(document
                .symbols
                .iter()
                .find(|symbol| Self::is_synthetic_file_root_symbol(symbol))
                .cloned());
        }
        self.ensure_file_fresh(file_path)?;
        Ok(self
            .symbol_store
            .get_symbol(&Self::synthetic_file_root_id(file_path))?)
    }

    fn overlay_references(
        &self,
        symbol: &Symbol,
        relationship_type: SymbolRelationshipType,
        incoming: bool,
    ) -> Vec<SymbolReference> {
        let overlays = self.overlays.read().unwrap();
        let find_symbol = |id: &str| {
            overlays
                .values()
                .flat_map(|document| &document.symbols)
                .find(|candidate| candidate.id == id)
                .cloned()
                .or_else(|| {
                    self.symbol_store
                        .get_symbol(id)
                        .ok()
                        .flatten()
                        .filter(|candidate| !overlays.contains_key(&candidate.file_path))
                })
        };
        let mut references = Vec::new();
        for document in overlays.values() {
            for relationship in &document.relationships {
                if relationship.relationship_type != relationship_type {
                    continue;
                }
                let matches = if incoming {
                    relationship.target_symbol_id.as_deref() == Some(symbol.id.as_str())
                        || (relationship.target_symbol_id.is_none()
                            && relationship.target_name == symbol.name)
                        || (relationship_type == SymbolRelationshipType::Import
                            && relationship.target_name == symbol.file_path)
                } else {
                    relationship.source_symbol_id == symbol.id
                };
                if !matches {
                    continue;
                }
                let Some(source_symbol) = find_symbol(&relationship.source_symbol_id) else {
                    continue;
                };
                let target_symbol = relationship
                    .target_symbol_id
                    .as_deref()
                    .and_then(&find_symbol)
                    .or_else(|| incoming.then(|| symbol.clone()));
                let target_symbol_id = target_symbol
                    .as_ref()
                    .map(|target| target.id.clone())
                    .or_else(|| relationship.target_symbol_id.clone());
                references.push(SymbolReference {
                    source_symbol,
                    relationship_type,
                    target_name: relationship.target_name.clone(),
                    target_symbol_id,
                    target_symbol,
                    line: relationship.line,
                    observation_kind: RelationshipObservationKind::SyntaxExtracted,
                    resolution_strategy: relationship
                        .resolution_strategy
                        .clone()
                        .or_else(|| Some("overlay".to_string())),
                    resolution_confidence: relationship.confidence.or(Some(0.9)),
                    receiver_type: relationship.recv_type.clone(),
                    receiver_is_self: relationship.recv_self,
                    import_path: relationship.import_path.clone(),
                    imported_name: relationship.imported_name.clone(),
                    byte_offset: relationship.byte_offset,
                    qualifier_segments: relationship.qualifier_segments.clone(),
                    call_form: relationship.call_form.clone(),
                    unresolved_reason: relationship.unresolved_reason.clone(),
                });
            }
        }
        references
    }

    fn remove_shadowed_persisted_references(&self, references: &mut Vec<SymbolReference>) {
        let overlays = self.overlays.read().unwrap();
        references.retain(|reference| !overlays.contains_key(&reference.source_symbol.file_path));
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
                    reference.byte_offset,
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
                    reference.byte_offset,
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

    pub fn get_related_symbols(
        &self,
        symbol: &Symbol,
        limit: usize,
    ) -> Result<Vec<RelatedSymbol>, LanguageError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        self.ensure_file_fresh(&symbol.file_path)?;
        let seed = self
            .symbol_store
            .get_symbol(&symbol.id)?
            .unwrap_or_else(|| symbol.clone());
        let expanded_limit = limit.saturating_mul(4).max(limit).max(16);
        let per_relationship_limit = limit.max(8);
        let mut related = Vec::new();
        let mut seen = HashSet::new();

        for relationship in [
            SymbolRelationshipType::Call,
            SymbolRelationshipType::Import,
            SymbolRelationshipType::Export,
            SymbolRelationshipType::Extends,
            SymbolRelationshipType::Implements,
            SymbolRelationshipType::Contains,
            SymbolRelationshipType::Usage,
        ] {
            let graph = self.get_symbol_graph(&seed, relationship, per_relationship_limit)?;

            for reference in graph.incoming {
                let reason = format!(
                    "{} has an incoming {} relationship to {}.",
                    reference.source_symbol.name, relationship, seed.name
                );
                Self::push_related_symbol(
                    &seed,
                    reference.source_symbol,
                    format!("incoming_{}", relationship),
                    reason,
                    direct_relationship_score(relationship),
                    1,
                    &mut related,
                    &mut seen,
                );
            }

            for reference in graph.outgoing {
                if let Some(target_symbol) = reference.target_symbol {
                    let reason = format!(
                        "{} has an outgoing {} relationship to {}.",
                        seed.name, relationship, target_symbol.name
                    );
                    Self::push_related_symbol(
                        &seed,
                        target_symbol,
                        format!("outgoing_{}", relationship),
                        reason,
                        direct_relationship_score(relationship).saturating_sub(4),
                        1,
                        &mut related,
                        &mut seen,
                    );
                } else if relationship == SymbolRelationshipType::Import {
                    if let Some(module_symbol) = self
                        .get_file_module_symbol(&reference.target_name)
                        .ok()
                        .flatten()
                    {
                        let reason =
                            format!("{} imports module {}.", seed.name, reference.target_name);
                        Self::push_related_symbol(
                            &seed,
                            module_symbol,
                            "outgoing_import".to_string(),
                            reason,
                            72,
                            1,
                            &mut related,
                            &mut seen,
                        );
                    }
                }
            }
        }

        let module_exports = self.get_module_export_references(&seed.file_path)?;
        let sibling_exports = module_exports
            .into_iter()
            .filter_map(|reference| reference.target_symbol)
            .filter(|candidate| candidate.id != seed.id)
            .collect::<Vec<_>>();

        for sibling in &sibling_exports {
            let reason = format!(
                "{} is exported from the same module as {}.",
                sibling.name, seed.name
            );
            Self::push_related_symbol(
                &seed,
                sibling.clone(),
                "same_module_export".to_string(),
                reason,
                68,
                1,
                &mut related,
                &mut seen,
            );
        }

        for reference in self.symbol_store.find_references_to_target(
            &seed.file_path,
            SymbolRelationshipType::Import,
            expanded_limit,
        )? {
            let reason = format!("Imports the module that defines {}.", seed.name);
            Self::push_related_symbol(
                &seed,
                reference.source_symbol,
                "module_importer".to_string(),
                reason,
                58,
                2,
                &mut related,
                &mut seen,
            );
        }

        for sibling in sibling_exports.iter().take(12) {
            for reference in self.find_references_to_symbol(sibling, 8)? {
                if reference.source_symbol.file_path == seed.file_path {
                    continue;
                }
                let reason = format!(
                    "Uses sibling export {} from the same module as {}.",
                    sibling.name, seed.name
                );
                Self::push_related_symbol(
                    &seed,
                    reference.source_symbol,
                    "sibling_export_consumer".to_string(),
                    reason,
                    62,
                    2,
                    &mut related,
                    &mut seen,
                );
            }
        }

        self.push_lexical_related_symbols(&seed, expanded_limit, &mut related, &mut seen)?;

        related.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.distance.cmp(&b.distance))
                .then_with(|| a.symbol.file_path.cmp(&b.symbol.file_path))
                .then_with(|| a.symbol.name.cmp(&b.symbol.name))
        });
        related.truncate(limit);
        Ok(related)
    }

    /// Get all symbols in a file
    pub fn get_file_symbols(&self, file_path: &str) -> Result<Vec<Symbol>, LanguageError> {
        if let Some(document) = self.overlays.read().unwrap().get(file_path) {
            return Ok(self.filter_visible_symbols(file_path, document.symbols.clone()));
        }
        self.ensure_file_fresh(file_path)?;
        let symbols = self.get_file_symbols_raw(file_path)?;
        Ok(self.filter_visible_symbols(file_path, symbols))
    }

    pub fn indexed_file_record(
        &self,
        file_path: &str,
    ) -> Result<Option<crate::symbol_index::store::IndexedFileRecord>, LanguageError> {
        Ok(self.symbol_store.indexed_file_record(file_path)?)
    }

    // =========================================================================
    // Document Synchronization
    // =========================================================================

    fn update_overlay(
        &self,
        file_path: &str,
        version: Option<i32>,
        content: &str,
    ) -> Result<(), LanguageError> {
        let language = Language::from_path(file_path).ok_or_else(|| {
            LanguageError::NotSupported(format!("Unknown language for: {file_path}"))
        })?;
        let SymbolExtraction {
            symbols: extracted_symbols,
            mut relationships,
            content: extraction_content,
            language: extraction_language,
        } = self.extract_file_symbols_and_relationships(file_path, content, language)?;
        let symbols = self.with_file_root_symbol(file_path, content, extracted_symbols);
        self.enrich_symbol_relationships(
            file_path,
            extraction_content.as_ref(),
            extraction_language,
            language,
            &symbols,
            &mut relationships,
        )?;
        let anchors = extract_semantic_anchors(file_path, content);

        self.overlays.write().unwrap().insert(
            file_path.to_string(),
            OverlayDocument {
                _version: version,
                symbols,
                relationships,
                anchors,
            },
        );
        Ok(())
    }

    /// Notify that a document was opened
    pub fn did_open(&self, file_path: &str, content: &str) -> Result<(), LanguageError> {
        let snapshot_key = self.snapshot_key(file_path);
        if self
            .buffer_snapshots
            .get(&snapshot_key)
            .map(|snapshot| snapshot.is_live() && snapshot.content() == content)
            .unwrap_or(false)
        {
            return Ok(());
        }

        self.buffer_snapshots
            .upsert_live(&snapshot_key, None, content);

        if should_allow_non_indexed_live_sync(file_path) {
            return Ok(());
        }

        self.update_overlay(file_path, None, content)
    }

    /// Notify that a document changed
    pub fn did_change(
        &self,
        file_path: &str,
        version: i32,
        content: &str,
    ) -> Result<(), LanguageError> {
        let snapshot_key = self.snapshot_key(file_path);
        if let Some(snapshot) = self.buffer_snapshots.get(&snapshot_key) {
            if snapshot.is_live() {
                if snapshot
                    .version()
                    .map(|existing_version| version < existing_version)
                    .unwrap_or(false)
                {
                    return Ok(());
                }

                if snapshot.content() == content {
                    if snapshot.version() != Some(version) {
                        self.buffer_snapshots
                            .upsert_live(&snapshot_key, Some(version), content);
                    }
                    return Ok(());
                }
            }
        }

        self.buffer_snapshots
            .upsert_live(&snapshot_key, Some(version), content);

        if should_allow_non_indexed_live_sync(file_path) {
            return Ok(());
        }

        self.update_overlay(file_path, Some(version), content)
    }

    /// Notify that a document was closed
    pub fn did_close(&self, file_path: &str) -> Result<(), LanguageError> {
        // Remove from cache
        {
            let mut cache = self.file_cache.write().unwrap();
            cache.remove(file_path);
        }
        self.buffer_snapshots.remove(&self.snapshot_key(file_path));
        self.overlays.write().unwrap().remove(file_path);

        Ok(())
    }

    /// Persist a saved document into the index and retire any unsaved overlay.
    pub fn did_save(&self, file_path: &str, content: &str) -> Result<(), LanguageError> {
        if should_allow_non_indexed_live_sync(file_path) {
            self.overlays.write().unwrap().remove(file_path);
            self.buffer_snapshots.remove(&self.snapshot_key(file_path));
            return Ok(());
        }

        self.index_file_content(file_path, None, content)?;
        self.overlays.write().unwrap().remove(file_path);
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

    fn append_stylesheet_usage_relationships(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        symbols: &[Symbol],
        relationships: &mut Vec<SymbolRelationship>,
    ) {
        if !matches!(
            language,
            Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx
        ) {
            return;
        }

        let css_module_aliases = stylesheet_module_aliases(content);
        let mut seen = relationships
            .iter()
            .map(|relationship| {
                (
                    relationship.source_symbol_id.clone(),
                    relationship.target_name.clone(),
                    relationship.relationship_type,
                    relationship.line,
                )
            })
            .collect::<HashSet<_>>();
        let symbol_resolver = SymbolIdentityResolver::new(symbols);

        for usage in stylesheet_class_usages(content, &css_module_aliases) {
            let Some(source_symbol) = symbol_resolver.source_for_usage_line(usage.line) else {
                continue;
            };
            let key = (
                source_symbol.id.clone(),
                usage.selector.clone(),
                SymbolRelationshipType::Usage,
                usage.line,
            );
            if seen.insert(key) {
                relationships.push(SymbolRelationship {
                    source_symbol_id: source_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: usage.selector,
                    target_symbol_id: None,
                    relationship_type: SymbolRelationshipType::Usage,
                    line: usage.line,
                    ..Default::default()
                });
            }
        }
    }

    fn append_stylesheet_custom_property_usage_relationships(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        symbols: &[Symbol],
        relationships: &mut Vec<SymbolRelationship>,
    ) {
        if !language.is_stylesheet_scanner() {
            return;
        }

        let mut seen = relationships
            .iter()
            .map(|relationship| {
                (
                    relationship.source_symbol_id.clone(),
                    relationship.target_name.clone(),
                    relationship.relationship_type,
                    relationship.line,
                )
            })
            .collect::<HashSet<_>>();
        let symbol_resolver = SymbolIdentityResolver::new(symbols);

        for usage in stylesheet_custom_property_usages(content) {
            let Some(source_symbol) = symbol_resolver
                .stylesheet_source_for_custom_property_usage(usage.line, &usage.name)
            else {
                continue;
            };
            if source_symbol.name == usage.name {
                continue;
            }

            let key = (
                source_symbol.id.clone(),
                usage.name.clone(),
                SymbolRelationshipType::Usage,
                usage.line,
            );
            if seen.insert(key) {
                relationships.push(SymbolRelationship {
                    source_symbol_id: source_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: usage.name,
                    target_symbol_id: None,
                    relationship_type: SymbolRelationshipType::Usage,
                    line: usage.line,
                    ..Default::default()
                });
            }
        }
    }

    pub(crate) fn resolve_import_target(
        &self,
        file_path: &str,
        import_target: &str,
    ) -> Option<String> {
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

        if base_path.is_file() {
            return Some(self.path_to_workspace_relative(base_path));
        }

        if base_path.extension().is_none() {
            for extension in Language::all_extensions() {
                candidates.push(base_path.with_extension(extension));
            }

            for extension in Language::all_extensions() {
                candidates.push(base_path.join(format!("index.{extension}")));
            }

            for index_name in ["main.go", "mod.rs", "__init__.py"] {
                candidates.push(base_path.join(index_name));
            }
        }

        candidates.into_iter().find_map(|candidate| {
            candidate
                .is_file()
                .then(|| self.path_to_workspace_relative(&candidate))
        })
    }

    fn path_to_workspace_relative(&self, path: &Path) -> String {
        let normalized = std::fs::canonicalize(path).unwrap_or_else(|_| normalize_path(path));
        match normalized.strip_prefix(&self.workspace_root) {
            Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
            Err(_) => normalized.to_string_lossy().replace('\\', "/"),
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

        if resolved.is_file() && is_supported_index_file(file_path) {
            if let Some(record) = self.symbol_store.indexed_file_record(file_path)? {
                if !self.indexed_file_needs_refresh(file_path, &record, true)? {
                    return Ok(());
                }
            }
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
        self.supported_language_discovery(scope).files
    }

    fn supported_language_discovery(&self, scope: &str) -> DiscoveryReport {
        if let Some(store) = self.worktree_store.read().unwrap().clone() {
            let files = store.supported_language_files(scope);
            return DiscoveryReport {
                discovered_files: files.len(),
                files,
                skipped_by_reason: BTreeMap::new(),
            };
        }

        let mut report = DiscoveryReport::default();
        let mut files = Vec::new();
        let root = self.resolve_path(scope);
        let scope_prefix = if scope.is_empty() || scope == "." {
            String::new()
        } else {
            scope.trim_matches('/').to_string()
        };
        let gitignore_filter = self.create_gitignore_filter();
        self.collect_supported_language_files_recursive(
            &root,
            &scope_prefix,
            gitignore_filter.as_ref(),
            &mut files,
            &mut report,
        );
        report.files = files;
        report
    }

    fn collect_supported_language_files_recursive(
        &self,
        dir_path: &Path,
        relative_path: &str,
        gitignore_filter: Option<&GitignoreFilter>,
        files: &mut Vec<String>,
        report: &mut DiscoveryReport,
    ) {
        let Ok(entries) = std::fs::read_dir(dir_path) else {
            *report
                .skipped_by_reason
                .entry("read_dir_failed".to_string())
                .or_default() += 1;
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            // M6.2 — dotenv files (`.env`, `.env.local`, …) start with `.` but are
            // valid key/value config we index; let them through while still ignoring
            // every other dotfile/dot-dir.
            let is_dotenv_file =
                path.is_file() && (file_name == ".env" || file_name.starts_with(".env."));
            if (file_name.starts_with('.') && !is_dotenv_file)
                || matches!(
                    file_name.as_str(),
                    "node_modules" | "target" | "dist" | "build" | "vendor"
                )
            {
                *report
                    .skipped_by_reason
                    .entry("ignored_directory".to_string())
                    .or_default() += 1;
                continue;
            }
            if let Some(filter) = gitignore_filter {
                if filter.should_ignore(&path) {
                    *report
                        .skipped_by_reason
                        .entry("gitignored".to_string())
                        .or_default() += 1;
                    continue;
                }
            }

            let relative = if relative_path.is_empty() {
                file_name
            } else {
                format!("{}/{}", relative_path, file_name)
            };

            if path.is_dir() {
                self.collect_supported_language_files_recursive(
                    &path,
                    &relative,
                    gitignore_filter,
                    files,
                    report,
                );
            } else if path.is_file() {
                report.discovered_files += 1;
                if is_supported_index_file(&relative)
                    || extensionless_shebang_is_supported(&path, &relative)
                {
                    files.push(relative);
                } else {
                    *report
                        .skipped_by_reason
                        .entry("unsupported_language".to_string())
                        .or_default() += 1;
                }
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
            if !resolved.is_file() || !is_supported_index_file(&record.file_path) {
                continue;
            }

            if self.indexed_file_needs_refresh(&record.file_path, record, true)? {
                let _ = self.index_file(&record.file_path)?;
            }
        }

        Ok(())
    }

    fn literal_repair_candidates(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, LanguageError> {
        let query = query.trim().to_lowercase();
        if query.len() < 2 || limit == 0 {
            return Ok(Vec::new());
        }

        let indexed_files = self.symbol_store.list_all_indexed_files()?;
        let indexed_map = indexed_files
            .iter()
            .map(|record| (record.file_path.as_str(), record))
            .collect::<HashMap<_, _>>();
        let mut candidates = Vec::new();

        for file_path in self.supported_language_files(".") {
            if candidates.len() >= limit {
                break;
            }
            let resolved = self.resolve_path(&file_path);
            let Ok(content) = std::fs::read_to_string(&resolved) else {
                continue;
            };
            if !content.to_lowercase().contains(&query) {
                continue;
            }

            let needs_reindex = match indexed_map.get(file_path.as_str()) {
                Some(record) => compute_hash(&content) != record.file_hash,
                None => true,
            };
            if needs_reindex {
                candidates.push(file_path);
            }
        }

        Ok(candidates)
    }

    fn literal_symbol_search_fallback(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SymbolLiteralMatch>, LanguageError> {
        let query = query.trim().to_lowercase();
        if query.len() < 2 || limit == 0 {
            return Ok(Vec::new());
        }

        let mut matches = Vec::new();
        for file_path in self.supported_language_files(".") {
            if matches.len() >= limit {
                break;
            }
            let Ok(content) = std::fs::read_to_string(self.resolve_path(&file_path)) else {
                continue;
            };
            for (line_index, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&query) {
                    matches.push(SymbolLiteralMatch {
                        file_path: file_path.clone(),
                        line: line_index as u32,
                        preview: line.trim().chars().take(240).collect(),
                    });
                    if matches.len() >= limit {
                        break;
                    }
                }
            }
        }

        Ok(matches)
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
            | SymbolRelationshipType::Implements
            | SymbolRelationshipType::Usage
            | SymbolRelationshipType::UsesType
            | SymbolRelationshipType::ReadsEnv
            | SymbolRelationshipType::Handles => {
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
        let mut references = self.overlay_references(symbol, relationship_type, true);
        references.truncate(limit);
        let mut seen = references
            .iter()
            .map(|reference| {
                (
                    reference.source_symbol.id.clone(),
                    reference.relationship_type,
                    reference.line,
                    reference.byte_offset,
                )
            })
            .collect::<HashSet<_>>();
        if references.len() >= limit {
            return Ok(references);
        }
        let expanded_limit = limit.saturating_mul(8).max(limit);

        for reference in self.symbol_store.find_references_to_symbol_id(
            &symbol.id,
            relationship_type,
            expanded_limit,
        )? {
            if self
                .overlays
                .read()
                .unwrap()
                .contains_key(&reference.source_symbol.file_path)
            {
                continue;
            }
            let key = (
                reference.source_symbol.id.clone(),
                reference.relationship_type,
                reference.line,
                reference.byte_offset,
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
            if self
                .overlays
                .read()
                .unwrap()
                .contains_key(&reference.source_symbol.file_path)
            {
                continue;
            }
            if !self.reference_matches_symbol(&reference, symbol)? {
                continue;
            }

            let key = (
                reference.source_symbol.id.clone(),
                reference.relationship_type,
                reference.line,
                reference.byte_offset,
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
        file_symbols: &[Symbol],
        relationships: &mut [SymbolRelationship],
    ) -> Result<(), LanguageError> {
        let imported_files = relationships
            .iter()
            .filter(|relationship| relationship.relationship_type == SymbolRelationshipType::Import)
            .map(|relationship| relationship.target_name.clone())
            .collect::<Vec<_>>();
        let mut imported_symbol_cache = HashMap::new();

        // M5.1: build the same-file inheritance index ONCE for this file from its
        // own Extends/Implements edges. Used only to widen a receiver type to its
        // supertypes during disambiguation; never to invent a target.
        let receiver_index = ReceiverTypeIndex::from_file(file_symbols, relationships);

        for relationship in relationships.iter_mut() {
            if relationship.relationship_type == SymbolRelationshipType::Import {
                continue;
            }

            if relationship.target_symbol_id.is_some() {
                continue;
            }

            // Qualified Rust call resolution: run the precision-first lanes
            // before the generic terminal-name resolver. A qualified call
            // (`Type::method`, `Self::method`, `crate::…::method`) carries
            // stronger evidence than a bare terminal name, so it gets its own
            // resolution path. If the qualified resolver cannot prove a
            // unique target, it sets `unresolved_reason` and the relationship
            // is NOT retried by the generic resolver — fail-closed per the
            // feature contract.
            if let Some(form) = relationship.call_form.as_deref() {
                if form != call_form::BARE && form != call_form::RECEIVER {
                    self.resolve_qualified_rust_call(relationship, file_symbols);
                    continue;
                }
            }

            // Named JS/TS imports carry the original module specifier and
            // exported name. Resolve only inside that module; if it is not yet
            // indexed, leave the edge NULL for the post-batch provenance
            // resolver instead of searching unrelated imported files.
            if let (Some(import_path), Some(imported_name)) = (
                relationship.import_path.as_deref(),
                relationship.imported_name.as_deref(),
            ) {
                if let Some(imported_file) =
                    self.resolve_import_target(&relationship.source_file_path, import_path)
                {
                    if !imported_symbol_cache.contains_key(&imported_file) {
                        let imported_symbols = self.get_file_symbols(&imported_file)?;
                        imported_symbol_cache.insert(imported_file.clone(), imported_symbols);
                    }
                    if let Some(imported_symbols) = imported_symbol_cache.get(&imported_file) {
                        let mut matches = Vec::new();
                        let mut seen = HashSet::new();
                        self.collect_matching_symbols(
                            imported_symbols,
                            imported_name,
                            &mut matches,
                            &mut seen,
                        );
                        matches.retain(|symbol| symbol.parent_id.is_none());
                        if let [resolved] = matches.as_slice() {
                            relationship.target_symbol_id = Some(resolved.id.clone());
                            relationship.resolution_strategy = Some("import_binding".to_string());
                            relationship.confidence = Some(0.9);
                        }
                    }
                }
                continue;
            }

            // M5.1: carry the receiver type (if any) into the resolver so the two
            // ambiguity branches can narrow — Unknown/None changes nothing.
            let recv_type = relationship.recv_type.clone();
            if let Some(resolved) = self.resolve_relationship_symbol_id(
                &relationship.target_name,
                recv_type.as_deref(),
                file_symbols,
                &imported_files,
                &mut imported_symbol_cache,
                &receiver_index,
            )? {
                relationship.target_symbol_id = Some(resolved.id);
                // Every successful resolver path carries an audit tag. The
                // global-unique and global receiver miners tag their own rows later.
                if let Some(strategy) = resolved.strategy {
                    relationship.resolution_strategy = Some(strategy.to_string());
                    relationship.confidence = resolved.confidence;
                }
            }
        }

        Ok(())
    }

    /// Cheap, precise per-reference resolution: same-file first, then symbols of
    /// explicitly-imported files. Anything not uniquely pinned here is left
    /// NULL and resolved later by the set-based global-unique back-fill (M2.4,
    /// `SymbolStore::backfill_unresolved_relationship_targets`) once the whole
    /// batch's symbols exist. The old per-reference `search_symbols_contextual`
    /// fallback — the cold-index bottleneck (81–97% of wall time) — is gone.
    ///
    /// M5.1 (strict superset): when a candidate set is AMBIGUOUS (`same_file > 1`
    /// or `imported > 1`) AND the edge carries a known `recv_type`, narrow the
    /// EXISTING candidate set to those whose parent class (or a supertype) equals
    /// the receiver type. If exactly one survives, resolve to it and tag
    /// `receiver_type`. Otherwise (no `recv_type`, 0 or >1 survivors) fall through
    /// to today's behavior. The returned id is ALWAYS drawn from the existing
    /// candidate set — never invented — and the already-resolved (`== 1`) branches
    /// are untouched.
    fn resolve_relationship_symbol_id(
        &self,
        reference_name: &str,
        recv_type: Option<&str>,
        file_symbols: &[Symbol],
        imported_files: &[String],
        imported_symbol_cache: &mut HashMap<String, Vec<Symbol>>,
        receiver_index: &ReceiverTypeIndex,
    ) -> Result<Option<ResolvedTarget>, LanguageError> {
        let mut same_file = Vec::new();
        let mut seen = HashSet::new();
        self.collect_matching_symbols(file_symbols, reference_name, &mut same_file, &mut seen);

        if same_file.len() == 1 {
            return Ok(Some(ResolvedTarget::same_file(same_file[0].id.clone())));
        }
        if same_file.len() > 1 {
            // M5.1 ambiguity branch #1: narrow by receiver type before bailing.
            if let Some(recv) = recv_type {
                if let Some(id) = disambiguate_by_receiver(&same_file, recv, receiver_index) {
                    return Ok(Some(ResolvedTarget::receiver(id)));
                }
            }
            return Ok(None);
        }

        let mut imported_matches = Vec::new();
        let mut imported_seen = HashSet::new();
        for imported_file in imported_files {
            if !imported_symbol_cache.contains_key(imported_file) {
                let imported_symbols = self.get_file_symbols(imported_file)?;
                imported_symbol_cache.insert(imported_file.clone(), imported_symbols);
            }
            let Some(imported_symbols) = imported_symbol_cache.get(imported_file) else {
                continue;
            };
            self.collect_matching_symbols(
                imported_symbols,
                reference_name,
                &mut imported_matches,
                &mut imported_seen,
            );
        }

        if imported_matches.len() == 1 {
            return Ok(Some(ResolvedTarget::imported(
                imported_matches[0].id.clone(),
            )));
        }
        if imported_matches.len() > 1 {
            // M5.1 ambiguity branch #2: narrow by receiver type before bailing.
            if let Some(recv) = recv_type {
                if let Some(id) = disambiguate_by_receiver(&imported_matches, recv, receiver_index)
                {
                    return Ok(Some(ResolvedTarget::receiver(id)));
                }
            }
        }

        Ok(None)
    }

    /// Precision-first qualified Rust call resolver.
    ///
    /// Runs the strongest semantic lanes before weaker ones, per the feature
    /// contract:
    ///
    /// 1. `Self` owner — derive the enclosing impl/trait owner and match the
    ///    terminal method only below that owner.
    /// 2. Crate-rooted path — interpret `crate::…` from the source crate root
    ///    through the indexed module graph (best-effort same-file match).
    /// 3. Module-relative path — interpret `self::…` and `super::…` from the
    ///    source module (best-effort same-file match).
    /// 4. Visible unqualified owner — for `Type::method`, consider types
    ///    declared in the source file and match the method under that owner.
    ///
    /// The resolver is fail-closed: if no unique target is proven, the
    /// relationship stays unresolved and an `unresolved_reason` is set.
    fn resolve_qualified_rust_call(
        &self,
        relationship: &mut SymbolRelationship,
        file_symbols: &[Symbol],
    ) {
        let Some(form) = relationship.call_form.clone() else {
            return;
        };
        let Some(qualifier) = relationship.qualifier_segments.clone() else {
            return;
        };
        let terminal = relationship.target_name.clone();

        match form.as_str() {
            call_form::SELF_PATH => {
                self.resolve_qualified_self_call(relationship, &terminal, file_symbols);
            }
            call_form::ASSOCIATED => {
                self.resolve_qualified_associated_call(
                    relationship,
                    &qualifier,
                    &terminal,
                    file_symbols,
                );
            }
            call_form::CRATE_PATH | call_form::MODULE_PATH => {
                self.resolve_qualified_path_call(
                    relationship,
                    &qualifier,
                    &terminal,
                    file_symbols,
                );
            }
            call_form::UFCS => {
                // UFCS requires type, trait, impl, and callable all
                // unambiguous — not yet supported without a Cargo module graph.
                relationship.unresolved_reason =
                    Some(unresolved_reason::UNSUPPORTED.to_string());
            }
            _ => {
                // Bare and receiver calls fall through to the generic resolver.
            }
        }
    }

    /// Lane 1: `Self::method()` — resolve only from a proven enclosing
    /// impl or trait owner.
    ///
    /// Blade's extractor parents methods to the type (struct/trait), not to
    /// the impl block, so `Self` resolution walks to the source method's
    /// parent type and matches the terminal method among that type's children.
    fn resolve_qualified_self_call(
        &self,
        relationship: &mut SymbolRelationship,
        terminal: &str,
        file_symbols: &[Symbol],
    ) {
        // Find the source symbol (the enclosing function/method).
        let source = file_symbols.iter().find(|s| s.id == relationship.source_symbol_id);
        let Some(source) = source else {
            relationship.unresolved_reason =
                Some(unresolved_reason::SELF_WITHOUT_OWNER.to_string());
            return;
        };

        // Walk up the parent chain to find an owner type (struct, trait,
        // class, enum, type alias). The extractor parents methods to the
        // type definition, not to the impl block.
        let owner_id = self.find_enclosing_owner_type(source, file_symbols);
        let Some(owner_id) = owner_id else {
            relationship.unresolved_reason =
                Some(unresolved_reason::SELF_WITHOUT_OWNER.to_string());
            return;
        };

        // Match the terminal method among children of the owner type.
        let candidates: Vec<&Symbol> = file_symbols
            .iter()
            .filter(|s| s.parent_id.as_deref() == Some(owner_id.as_str()))
            .filter(|s| s.name == terminal)
            .collect();

        match candidates.as_slice() {
            [single] => {
                relationship.target_symbol_id = Some(single.id.clone());
                relationship.resolution_strategy =
                    Some("rust_self_owner".to_string());
                relationship.confidence = Some(1.0);
            }
            [] => {
                relationship.unresolved_reason =
                    Some(unresolved_reason::UNRESOLVED_METHOD.to_string());
            }
            _ => {
                relationship.unresolved_reason =
                    Some(unresolved_reason::AMBIGUOUS.to_string());
            }
        }
    }

    /// Lane 5: `Type::method()` — consider types declared in the source file
    /// and match the method under that owner type.
    ///
    /// Blade's extractor parents methods to the type definition (struct/trait),
    /// not to individual impl blocks, so we resolve by finding the named type
    /// and then matching the terminal method among its children.
    fn resolve_qualified_associated_call(
        &self,
        relationship: &mut SymbolRelationship,
        qualifier: &[String],
        terminal: &str,
        file_symbols: &[Symbol],
    ) {
        // The owner type name is the last qualifier segment.
        // For `SymbolStore::new`, qualifier is `["SymbolStore"]`.
        let owner_name = qualifier.last().cloned().unwrap_or_default();
        if owner_name.is_empty() {
            relationship.unresolved_reason =
                Some(unresolved_reason::UNSUPPORTED.to_string());
            return;
        }

        // Find the type definition (struct, trait, class, enum, type alias)
        // matching the owner name. The extractor deduplicates impl blocks,
        // so there is at most one type symbol per name per file.
        let owner_candidates: Vec<&Symbol> = file_symbols
            .iter()
            .filter(|s| {
                s.name == owner_name
                    && matches!(
                        s.symbol_type,
                        SymbolType::Struct
                            | SymbolType::Class
                            | SymbolType::Trait
                            | SymbolType::Type
                            | SymbolType::Enum
                    )
            })
            .collect();

        if owner_candidates.is_empty() {
            relationship.unresolved_reason =
                Some(unresolved_reason::UNRESOLVED_OWNER.to_string());
            return;
        }

        // Find methods matching the terminal name under any candidate owner.
        let mut candidates: Vec<&Symbol> = Vec::new();
        for owner in &owner_candidates {
            for sym in file_symbols {
                if sym.parent_id.as_deref() == Some(owner.id.as_str())
                    && sym.name == terminal
                {
                    candidates.push(sym);
                }
            }
        }

        // Deduplicate by symbol ID. If the raw count exceeds the deduplicated
        // count, multiple definitions share the same qualified name (e.g.
        // inherent method + trait impl method) — that is ambiguity.
        let raw_count = candidates.len();
        let mut seen_ids = HashSet::new();
        candidates.retain(|s| seen_ids.insert(s.id.clone()));

        if raw_count > candidates.len() {
            relationship.unresolved_reason =
                Some(unresolved_reason::AMBIGUOUS.to_string());
            return;
        }

        match candidates.as_slice() {
            [single] => {
                relationship.target_symbol_id = Some(single.id.clone());
                relationship.resolution_strategy =
                    Some("rust_visible_owner".to_string());
                relationship.confidence = Some(1.0);
            }
            [] => {
                relationship.unresolved_reason =
                    Some(unresolved_reason::UNRESOLVED_METHOD.to_string());
            }
            _ => {
                relationship.unresolved_reason =
                    Some(unresolved_reason::AMBIGUOUS.to_string());
            }
        }
    }

    /// Lanes 2-3: `crate::…::Type::method()` and `self::…::Type::method()` —
    /// best-effort same-file resolution by matching the full qualifier path
    /// against symbol qualified names. Without a full Cargo module graph,
    /// we can only resolve when the owner and method are in the same file.
    fn resolve_qualified_path_call(
        &self,
        relationship: &mut SymbolRelationship,
        qualifier: &[String],
        terminal: &str,
        file_symbols: &[Symbol],
    ) {
        // The owner type is the segment before the terminal.
        // For `crate::store::SymbolStore::new`, qualifier is
        // `["crate", "store", "SymbolStore"]` and terminal is `new`.
        // The owner name is the last qualifier segment.
        let owner_name = qualifier.last().cloned().unwrap_or_default();
        if owner_name.is_empty() {
            relationship.unresolved_reason =
                Some(unresolved_reason::UNSUPPORTED.to_string());
            return;
        }

        // Build the expected path suffix from the qualifier (excluding
        // keyword prefixes like `crate`, `self`, `super`).
        let path_segments: Vec<&str> = qualifier
            .iter()
            .filter(|s| !matches!(s.as_str(), "crate" | "self" | "super" | "Self"))
            .map(String::as_str)
            .collect();

        // Find type/trait/struct symbols matching the owner name.
        let owner_candidates: Vec<&Symbol> = file_symbols
            .iter()
            .filter(|s| {
                matches!(
                    s.symbol_type,
                    SymbolType::Struct
                        | SymbolType::Class
                        | SymbolType::Trait
                        | SymbolType::Type
                        | SymbolType::Enum
                ) && s.name == owner_name
            })
            .filter(|s| {
                // If we have intermediate path segments, check that the
                // qualified_name contains them as a suffix.
                if path_segments.len() <= 1 {
                    return true;
                }
                // Check qualified_name ends with the path segments joined by ::
                let expected_suffix = path_segments.join("::");
                s.qualified_name.ends_with(&expected_suffix)
                    || s.qualified_name.contains(&expected_suffix)
            })
            .collect();

        if owner_candidates.is_empty() {
            // Without a Cargo module graph, we can't resolve cross-file
            // crate/module paths. Mark as missing project context.
            relationship.unresolved_reason =
                Some(unresolved_reason::MISSING_PROJECT_CONTEXT.to_string());
            return;
        }

        // Find methods matching the terminal name under any candidate owner.
        let mut candidates: Vec<&Symbol> = Vec::new();
        for owner in &owner_candidates {
            for sym in file_symbols {
                if sym.parent_id.as_deref() == Some(owner.id.as_str())
                    && sym.name == terminal
                {
                    candidates.push(sym);
                }
            }
        }

        // Deduplicate by symbol ID.
        let mut seen_ids = HashSet::new();
        candidates.retain(|s| seen_ids.insert(s.id.clone()));

        match candidates.as_slice() {
            [single] => {
                relationship.target_symbol_id = Some(single.id.clone());
                let strategy = if relationship.call_form.as_deref() == Some(call_form::CRATE_PATH) {
                    "rust_crate_path"
                } else {
                    "rust_module_path"
                };
                relationship.resolution_strategy = Some(strategy.to_string());
                relationship.confidence = Some(1.0);
            }
            [] => {
                relationship.unresolved_reason =
                    Some(unresolved_reason::UNRESOLVED_METHOD.to_string());
            }
            _ => {
                relationship.unresolved_reason =
                    Some(unresolved_reason::AMBIGUOUS.to_string());
            }
        }
    }

    /// Find the enclosing owner type (struct, trait, class, enum, type)
    /// for a source symbol by walking up its `parent_id` chain.
    /// The extractor parents methods to the type definition, not to the
    /// impl block, so we look for a type symbol rather than an impl.
    fn find_enclosing_owner_type(
        &self,
        source: &Symbol,
        file_symbols: &[Symbol],
    ) -> Option<String> {
        let mut current_id = source.parent_id.as_ref()?;
        loop {
            let current = file_symbols.iter().find(|s| s.id == *current_id)?;
            if matches!(
                current.symbol_type,
                SymbolType::Struct
                    | SymbolType::Class
                    | SymbolType::Trait
                    | SymbolType::Type
                    | SymbolType::Enum
            ) {
                return Some(current.id.clone());
            }
            // If we hit an impl, dig into its name to find the type name,
            // then look for the corresponding type symbol.
            if current.symbol_type == SymbolType::Impl {
                // `impl Foo` → look for a symbol named `Foo`.
                let type_name = current
                    .name
                    .strip_prefix("impl ")
                    .map(|rest| {
                        // `impl Trait for Type` → take the last type name.
                        if let Some(pos) = rest.find(" for ") {
                            &rest[pos + 5..]
                        } else {
                            rest
                        }
                        .trim()
                    })
                    .unwrap_or(&current.name);
                let type_sym = file_symbols.iter().find(|s| {
                    s.name == type_name
                        && matches!(
                            s.symbol_type,
                            SymbolType::Struct
                                | SymbolType::Class
                                | SymbolType::Trait
                                | SymbolType::Type
                                | SymbolType::Enum
                        )
                });
                if let Some(ts) = type_sym {
                    return Some(ts.id.clone());
                }
            }
            current_id = match current.parent_id.as_ref() {
                Some(id) => id,
                None => return None,
            };
        }
    }

    fn collect_matching_symbols(
        &self,
        symbols: &[Symbol],
        reference_name: &str,
        resolved: &mut Vec<Symbol>,
        seen: &mut HashSet<String>,
    ) {
        SymbolIdentityResolver::new(symbols).collect_matching_named_symbols(
            reference_name,
            resolved,
            seen,
        );
    }

    // ---- M5.1 receiver-type disambiguation (resolution side) ----------------

    fn suppress_known_external_relationships(
        language: Language,
        relationships: &mut Vec<SymbolRelationship>,
        translation_call_aliases: &HashSet<String>,
    ) -> usize {
        let before = relationships.len();
        relationships.retain(|relationship| {
            !Self::is_known_external_unresolved_call(
                language,
                relationship,
                translation_call_aliases,
            )
        });
        before - relationships.len()
    }

    fn is_known_external_unresolved_call(
        language: Language,
        relationship: &SymbolRelationship,
        translation_call_aliases: &HashSet<String>,
    ) -> bool {
        // Qualified Rust calls with an `unresolved_reason` are intentionally
        // fail-closed observations — they must survive suppression so they
        // remain queryable for diagnostics (feature contract invariant 9).
        // Only bare/receiver calls that fell through to the generic resolver
        // and remained unresolved are eligible for known-external suppression.
        if relationship.call_form.is_some()
            && relationship.unresolved_reason.is_some()
        {
            return false;
        }
        // A known external/library/builtin call name is always suppressed when it
        // is still unresolved after same-file/imported resolution — regardless of
        // whether its bare name happens to be a globally-unique project symbol.
        // Distinguishing a project `parse()` from `JSON.parse()` needs receiver/
        // type context and is deferred to M5.1; a bare-name back-fill cannot do it
        // safely, so we never let such a call survive to be mis-wired.
        relationship.relationship_type == SymbolRelationshipType::Call
            && relationship.target_symbol_id.is_none()
            && (Self::is_known_external_call_name(language, &relationship.target_name)
                || translation_call_aliases.contains(&relationship.target_name))
    }

    fn is_known_external_call_name(language: Language, target_name: &str) -> bool {
        let name = target_name.rsplit("::").next().unwrap_or(target_name);
        match language {
            Language::TypeScript
            | Language::Tsx
            | Language::Astro
            | Language::JavaScript
            | Language::Jsx => matches!(
                name,
                "Array"
                    | "Boolean"
                    | "Date"
                    | "Error"
                    | "JSON"
                    | "Map"
                    | "Math"
                    | "Number"
                    | "Object"
                    | "Promise"
                    | "RegExp"
                    | "Set"
                    | "String"
                    | "add"
                    | "aggregate"
                    | "all"
                    | "append"
                    | "at"
                    | "catch"
                    | "concat"
                    | "count"
                    | "create"
                    | "createMany"
                    | "debug"
                    | "delete"
                    | "deleteMany"
                    | "endsWith"
                    | "error"
                    | "every"
                    | "filter"
                    | "find"
                    | "findIndex"
                    | "findFirst"
                    | "findFirstOrThrow"
                    | "findMany"
                    | "findUnique"
                    | "findUniqueOrThrow"
                    | "finally"
                    | "flat"
                    | "flatMap"
                    | "forEach"
                    | "get"
                    | "getTime"
                    | "groupBy"
                    | "has"
                    | "includes"
                    | "info"
                    | "join"
                    | "log"
                    | "map"
                    | "notFound"
                    | "parse"
                    | "push"
                    | "redirect"
                    | "reduce"
                    | "revalidatePath"
                    | "replace"
                    | "round"
                    | "set"
                    | "slice"
                    | "some"
                    | "sort"
                    | "split"
                    | "startsWith"
                    | "stringify"
                    | "table"
                    | "then"
                    | "toISOString"
                    | "toLowerCase"
                    | "toString"
                    | "toUpperCase"
                    | "trim"
                    | "update"
                    | "updateMany"
                    | "upsert"
                    | "useCallback"
                    | "useEffect"
                    | "useMemo"
                    | "useRef"
                    | "useState"
                    | "warn"
            ),
            Language::Rust => matches!(
                name,
                "Err"
                    | "None"
                    | "Ok"
                    | "Some"
                    | "and_then"
                    | "as_ref"
                    | "clone"
                    | "collect"
                    | "contains"
                    | "expect"
                    | "filter"
                    | "format"
                    | "get"
                    | "insert"
                    | "is_empty"
                    | "iter"
                    | "join"
                    | "len"
                    | "map"
                    | "new"
                    | "or_else"
                    | "path"
                    | "push"
                    | "remove"
                    | "to_path_buf"
                    | "to_string"
                    | "to_string_lossy"
                    | "unwrap"
                    | "unwrap_or"
                    | "unwrap_or_default"
                    | "write"
            ),
            Language::Go => matches!(
                name,
                "Contains"
                    | "Error"
                    | "Errorf"
                    | "Fatal"
                    | "Fatalf"
                    | "Marshal"
                    | "New"
                    | "Printf"
                    | "Println"
                    | "Sprintf"
                    | "TrimSpace"
                    | "Unmarshal"
                    | "WriteString"
                    | "append"
                    | "cap"
                    | "close"
                    | "copy"
                    | "delete"
                    | "len"
                    | "make"
                    | "new"
                    | "panic"
                    | "recover"
            ),
            Language::Python => matches!(
                name,
                "append"
                    | "dict"
                    | "enumerate"
                    | "filter"
                    | "format"
                    | "get"
                    | "int"
                    | "items"
                    | "join"
                    | "keys"
                    | "len"
                    | "list"
                    | "map"
                    | "open"
                    | "print"
                    | "range"
                    | "set"
                    | "split"
                    | "str"
                    | "strip"
                    | "values"
            ),
            Language::Markdown
            | Language::Css
            | Language::Scss
            | Language::Sass
            | Language::Less
            | Language::Html
            | Language::Vue
            | Language::Svelte
            | Language::Json
            | Language::Yaml
            | Language::Toml
            | Language::Php
            | Language::Java
            | Language::CSharp
            | Language::Kotlin
            | Language::Ruby
            | Language::Cpp
            | Language::Shell
            | Language::Dockerfile
            | Language::Sql
            | Language::BuildScript => false,
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

        let SymbolExtraction {
            symbols: extracted_symbols,
            mut relationships,
            content: extraction_content,
            language: extraction_language,
        } = self.extract_file_symbols_and_relationships(file_path, snapshot.content(), language)?;
        let symbols = self.with_file_root_symbol(file_path, snapshot.content(), extracted_symbols);
        self.enrich_symbol_relationships(
            file_path,
            extraction_content.as_ref(),
            extraction_language,
            language,
            &symbols,
            &mut relationships,
        )?;

        // Delete old symbols and insert new ones
        self.symbol_store.delete_file_symbols(file_path)?;
        self.symbol_store.upsert_symbols(&symbols)?;
        self.symbol_store
            .replace_relationships_for_file(file_path, &relationships)?;
        // M2.4 — incremental single-file path: no global-unique back-fill here;
        // edges resolve same-file/imported and are globally back-filled on the
        // next full reindex (keeps the back-fill's COUNT(*) truly global).
        //
        // Track C — mined Go `implements` edges are re-derived, though: the
        // replace above deletes them by source_file_path (see
        // `remine_go_interface_implementations_after_single_file_index`).
        self.remine_go_interface_implementations_after_single_file_index(file_path);
        self.symbol_store
            .mark_file_indexed_with_metadata_and_extractor_version(
                file_path,
                &hash,
                symbols.len(),
                None,
                Some(source_line_count(snapshot.content())),
                None,
                Self::extractor_version_for_index_file(file_path),
            )?;

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

    /// Track C — re-mine Go implicit-interface `implements` edges after a
    /// SINGLE-FILE index commit of a Go file.
    ///
    /// Every single-file commit path (`replace_file_index` in
    /// `index_file_with_timings`, `replace_relationships_for_file` in
    /// `index_file_content`) deletes the file's relationships by
    /// source_file_path — including the mined `go_implicit_interface` edges of
    /// the types defined in that file. Only the full workspace index re-mines,
    /// so without this call a single-file re-index silently drops those edges
    /// until the next full index.
    ///
    /// Gated on the `.go` extension so every other language pays one string
    /// check. A mining failure must NEVER fail the index operation: the index
    /// commit already succeeded, so log and degrade (the edges stay absent
    /// until the next full index — the pre-existing behavior).
    fn remine_go_interface_implementations_after_single_file_index(&self, file_path: &str) {
        if !file_path.ends_with(".go") {
            return;
        }
        if let Err(error) = self.symbol_store.mine_go_interface_implementations() {
            eprintln!(
                "[LanguageService] Go interface re-mining after indexing {} failed \
                 (implements edges may be stale until the next full index): {}",
                file_path, error
            );
        }
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
        // Build a TRANSIENT disk snapshot for indexing — do NOT cache it in
        // `buffer_snapshots`. That store is never evicted, so caching every indexed
        // file's content (including the multi-MB anchor-only register headers we
        // still load for hashing) accumulated GIGABYTES of live memory on huge
        // repos — the kernel held ~5 GiB resident after indexing — and defeated the
        // byte-budget batching (the batch's snapshot was dropped but the cache kept
        // a copy). This snapshot lives only as long as the file's batch. The cache
        // is still consulted above for a LIVE (open/edited) buffer, which stays
        // authoritative.
        Ok(Arc::new(BufferSnapshot::new(
            file_path,
            None,
            content,
            BufferSnapshotSource::Disk,
        )))
    }

    fn load_buffer_snapshot(&self, file_path: &str) -> Result<Arc<BufferSnapshot>, LanguageError> {
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

    /// Get statistics about the index
    pub fn stats(&self) -> Result<IndexStats, LanguageError> {
        let supported_files = self.supported_language_files(".");
        Ok(IndexStats {
            files_indexed: self.symbol_store.file_count()?,
            symbols_extracted: self.symbol_store.count()?,
            files_failed: 0,
            duration_ms: 0,
            files_discovered: supported_files.len(),
            supported_files: supported_files.len(),
            files_fresh: 0,
            files_reindexed: 0,
            anchors_extracted: 0,
            relationships_extracted: 0,
            load_ms: 0,
            freshness_check_ms: 0,
            parse_extract_ms: 0,
            relationship_enrichment_ms: 0,
            db_write_ms: 0,
            cache_update_ms: 0,
            supported_by_language: language_counts_for_paths(&supported_files),
            skipped_by_reason: Vec::new(),
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
            line_count: Option<usize>,
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
        let mut indexed_line_count = 0usize;

        for record in indexed_files {
            if let Some(line_count) = record.line_count {
                indexed_line_count += line_count;
            }
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
                line_count: record.line_count,
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
        if indexed_line_count > 0 {
            output.push_str(&format!("- Indexed lines: {}\n", indexed_line_count));
        }
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
            if let Some(line_count) = module.line_count {
                output.push_str(&format!("- Lines: {}\n", line_count));
            }
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

    pub fn build_architecture_snapshot(
        &self,
        scope_path: Option<&str>,
        relationship_types: &[SymbolRelationshipType],
        min_confidence: f32,
        max_modules: usize,
        max_edges: usize,
        max_communities: usize,
    ) -> Result<ArchitectureSnapshot, LanguageError> {
        let max_modules = max_modules.clamp(2, 1_000);
        let max_edges = max_edges.clamp(1, 2_000);
        let max_communities = max_communities.clamp(1, 50);
        let min_confidence = min_confidence.clamp(0.0, 1.0);
        let mut normalized_scope = scope_path
            .map(|scope| scope.replace('\\', "/"))
            .map(|scope| scope.trim_matches('/').to_string())
            .filter(|scope| !scope.is_empty() && scope != ".");
        if let Some(scope) = normalized_scope.clone() {
            let resolved = self.resolve_path(&scope);
            if resolved.is_file() {
                self.ensure_file_fresh(&scope)?;
                normalized_scope = Path::new(&scope)
                    .parent()
                    .map(|path| path.to_string_lossy().replace('\\', "/"))
                    .filter(|path| !path.is_empty() && path != ".");
            } else {
                self.ensure_scope_index_fresh(
                    Some(&resolved),
                    max_modules.saturating_mul(4).max(64),
                )?;
            }
        }

        let raw_limit = max_edges.saturating_mul(4).clamp(max_edges + 1, 5_000);
        let mut aggregates = self.symbol_store.module_relationship_aggregates(
            normalized_scope.as_deref(),
            relationship_types,
            min_confidence,
            raw_limit.saturating_add(1),
        )?;
        let raw_aggregates_truncated = aggregates.len() > raw_limit;
        aggregates.truncate(raw_limit);
        let raw_aggregate_count = aggregates.len();

        #[derive(Default)]
        struct PairAccumulator {
            relationship_counts: BTreeMap<String, usize>,
            edge_count: usize,
            confidence_sum: f32,
            weight: f32,
        }

        let mut pairs = BTreeMap::<(String, String), PairAccumulator>::new();
        for ModuleRelationshipAggregate {
            source_file_path,
            target_file_path,
            relationship_type,
            edge_count,
            average_confidence,
        } in aggregates
        {
            if source_file_path == target_file_path || edge_count == 0 {
                continue;
            }
            let pair = pairs
                .entry((source_file_path, target_file_path))
                .or_default();
            *pair
                .relationship_counts
                .entry(relationship_type.to_string())
                .or_default() += edge_count;
            pair.edge_count += edge_count;
            pair.confidence_sum += average_confidence * edge_count as f32;
            pair.weight += architecture_relationship_weight(relationship_type)
                * average_confidence
                * edge_count as f32;
        }
        let candidate_edge_count = pairs.len();
        let mut candidate_degrees = HashMap::<String, f32>::new();
        for ((source, target), pair) in &pairs {
            *candidate_degrees.entry(source.clone()).or_default() += pair.weight;
            *candidate_degrees.entry(target.clone()).or_default() += pair.weight;
        }
        let candidate_module_count = candidate_degrees.len();
        let mut ranked_paths = candidate_degrees.into_iter().collect::<Vec<_>>();
        ranked_paths.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        ranked_paths.truncate(max_modules);
        let selected_paths = ranked_paths
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<HashSet<_>>();

        let mut edges = pairs
            .into_iter()
            .filter(|((source, target), _)| {
                selected_paths.contains(source) && selected_paths.contains(target)
            })
            .map(
                |((source_file_path, target_file_path), pair)| ArchitectureEdge {
                    source_file_path,
                    target_file_path,
                    relationship_counts: pair.relationship_counts,
                    edge_count: pair.edge_count,
                    average_confidence: rounded_architecture_value(
                        pair.confidence_sum / pair.edge_count.max(1) as f32,
                    ),
                    weight: rounded_architecture_value(pair.weight),
                },
            )
            .collect::<Vec<_>>();
        edges.sort_by(|left, right| {
            right
                .weight
                .partial_cmp(&left.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.edge_count.cmp(&left.edge_count))
                .then_with(|| left.source_file_path.cmp(&right.source_file_path))
                .then_with(|| left.target_file_path.cmp(&right.target_file_path))
        });
        let edges_truncated = edges.len() > max_edges;
        edges.truncate(max_edges);

        #[derive(Default)]
        struct ModuleAccumulator {
            incoming_edge_count: usize,
            outgoing_edge_count: usize,
            weighted_degree: f32,
            neighbors: HashSet<String>,
        }

        let mut module_stats = HashMap::<String, ModuleAccumulator>::new();
        for edge in &edges {
            let source = module_stats
                .entry(edge.source_file_path.clone())
                .or_default();
            source.outgoing_edge_count += edge.edge_count;
            source.weighted_degree += edge.weight;
            source.neighbors.insert(edge.target_file_path.clone());

            let target = module_stats
                .entry(edge.target_file_path.clone())
                .or_default();
            target.incoming_edge_count += edge.edge_count;
            target.weighted_degree += edge.weight;
            target.neighbors.insert(edge.source_file_path.clone());
        }
        let mut module_paths = module_stats.keys().cloned().collect::<Vec<_>>();
        module_paths.sort();
        let labels = architecture_community_labels(&module_paths, &edges);

        #[derive(Default)]
        struct RawCommunity {
            modules: Vec<String>,
            internal_weight: f32,
            cross_community_weight: f32,
            relationship_counts: BTreeMap<String, usize>,
        }

        let mut raw_communities = HashMap::<usize, RawCommunity>::new();
        for path in &module_paths {
            raw_communities
                .entry(labels.get(path).copied().unwrap_or_default())
                .or_default()
                .modules
                .push(path.clone());
        }
        for edge in &edges {
            let source_label = labels
                .get(&edge.source_file_path)
                .copied()
                .unwrap_or_default();
            let target_label = labels
                .get(&edge.target_file_path)
                .copied()
                .unwrap_or_default();
            if source_label == target_label {
                let community = raw_communities.entry(source_label).or_default();
                community.internal_weight += edge.weight;
                for (relationship, count) in &edge.relationship_counts {
                    *community
                        .relationship_counts
                        .entry(relationship.clone())
                        .or_default() += *count;
                }
            } else {
                raw_communities
                    .entry(source_label)
                    .or_default()
                    .cross_community_weight += edge.weight;
                raw_communities
                    .entry(target_label)
                    .or_default()
                    .cross_community_weight += edge.weight;
            }
        }
        for community in raw_communities.values_mut() {
            community.modules.sort_by(|left, right| {
                module_stats
                    .get(right)
                    .map(|stats| stats.weighted_degree)
                    .unwrap_or_default()
                    .partial_cmp(
                        &module_stats
                            .get(left)
                            .map(|stats| stats.weighted_degree)
                            .unwrap_or_default(),
                    )
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| left.cmp(right))
            });
        }
        let mut community_order = raw_communities.keys().copied().collect::<Vec<_>>();
        community_order.sort_by(|left, right| {
            let left_community = &raw_communities[left];
            let right_community = &raw_communities[right];
            right_community
                .internal_weight
                .partial_cmp(&left_community.internal_weight)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    right_community
                        .modules
                        .len()
                        .cmp(&left_community.modules.len())
                })
                .then_with(|| {
                    left_community
                        .modules
                        .first()
                        .cmp(&right_community.modules.first())
                })
        });
        let community_ids = community_order
            .iter()
            .enumerate()
            .map(|(index, label)| (*label, format!("community-{}", index + 1)))
            .collect::<HashMap<_, _>>();
        let module_community_ids = labels
            .iter()
            .map(|(path, label)| {
                (
                    path.clone(),
                    community_ids
                        .get(label)
                        .cloned()
                        .unwrap_or_else(|| "community-unknown".to_string()),
                )
            })
            .collect::<HashMap<_, _>>();

        let metadata = self
            .symbol_store
            .indexed_file_records_for_paths(&module_paths)?;
        let mut modules = module_stats
            .into_iter()
            .map(|(file_path, stats)| {
                let record = metadata.get(&file_path);
                ArchitectureModule {
                    community_id: module_community_ids
                        .get(&file_path)
                        .cloned()
                        .unwrap_or_else(|| "community-unknown".to_string()),
                    symbol_count: record.map(|record| record.symbol_count).unwrap_or_default(),
                    line_count: record.and_then(|record| record.line_count),
                    incoming_edge_count: stats.incoming_edge_count,
                    outgoing_edge_count: stats.outgoing_edge_count,
                    weighted_degree: rounded_architecture_value(stats.weighted_degree),
                    distinct_neighbors: stats.neighbors.len(),
                    file_path,
                }
            })
            .collect::<Vec<_>>();
        modules.sort_by(|left, right| {
            right
                .weighted_degree
                .partial_cmp(&left.weighted_degree)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.distinct_neighbors.cmp(&left.distinct_neighbors))
                .then_with(|| left.file_path.cmp(&right.file_path))
        });
        let hubs = modules.iter().take(12).cloned().collect::<Vec<_>>();

        let community_count = community_order.len();
        let mut communities = community_order
            .iter()
            .filter_map(|label| {
                let community = raw_communities.get(label)?;
                Some(ArchitectureCommunity {
                    id: community_ids.get(label)?.clone(),
                    module_count: community.modules.len(),
                    internal_weight: rounded_architecture_value(community.internal_weight),
                    cross_community_weight: rounded_architecture_value(
                        community.cross_community_weight,
                    ),
                    top_modules: community.modules.iter().take(8).cloned().collect(),
                    relationship_counts: community.relationship_counts.clone(),
                })
            })
            .collect::<Vec<_>>();
        communities.truncate(max_communities);

        let mut bridge_edges = edges
            .iter()
            .filter(|edge| {
                module_community_ids.get(&edge.source_file_path)
                    != module_community_ids.get(&edge.target_file_path)
            })
            .cloned()
            .collect::<Vec<_>>();
        bridge_edges.truncate(24);
        let mut bridge_stats = HashMap::<String, (f32, HashSet<String>)>::new();
        for edge in &edges {
            let Some(source_community) = module_community_ids.get(&edge.source_file_path) else {
                continue;
            };
            let Some(target_community) = module_community_ids.get(&edge.target_file_path) else {
                continue;
            };
            if source_community == target_community {
                continue;
            }
            let source = bridge_stats
                .entry(edge.source_file_path.clone())
                .or_default();
            source.0 += edge.weight;
            source.1.insert(target_community.clone());
            let target = bridge_stats
                .entry(edge.target_file_path.clone())
                .or_default();
            target.0 += edge.weight;
            target.1.insert(source_community.clone());
        }
        let mut bridge_modules = bridge_stats
            .into_iter()
            .map(|(file_path, (cross_community_weight, communities))| {
                let mut connected_communities = communities.into_iter().collect::<Vec<_>>();
                connected_communities.sort();
                ArchitectureBridgeModule {
                    community_id: module_community_ids
                        .get(&file_path)
                        .cloned()
                        .unwrap_or_else(|| "community-unknown".to_string()),
                    file_path,
                    cross_community_weight: rounded_architecture_value(cross_community_weight),
                    connected_communities,
                }
            })
            .collect::<Vec<_>>();
        bridge_modules.sort_by(|left, right| {
            right
                .cross_community_weight
                .partial_cmp(&left.cross_community_weight)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.file_path.cmp(&right.file_path))
        });
        bridge_modules.truncate(16);

        Ok(ArchitectureSnapshot {
            scope: normalized_scope,
            modules,
            edges,
            communities,
            hubs,
            bridge_modules,
            bridge_edges,
            candidate_module_count,
            candidate_edge_count,
            raw_aggregate_count,
            min_confidence,
            truncated: raw_aggregates_truncated
                || candidate_module_count > max_modules
                || edges_truncated
                || community_count > max_communities,
        })
    }

    pub fn get_symbol_graph(
        &self,
        symbol: &Symbol,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<SymbolGraph, LanguageError> {
        let incoming = match relationship_type {
            SymbolRelationshipType::Call => self.find_references_to_symbol(symbol, limit)?,
            SymbolRelationshipType::Import => {
                let mut references = self.overlay_references(symbol, relationship_type, true);
                let mut persisted = self.symbol_store.find_references_to_target(
                    &symbol.file_path,
                    SymbolRelationshipType::Import,
                    limit,
                )?;
                self.remove_shadowed_persisted_references(&mut persisted);
                references.extend(persisted);
                references.truncate(limit);
                references
            }
            SymbolRelationshipType::Export
            | SymbolRelationshipType::Extends
            | SymbolRelationshipType::Implements
            | SymbolRelationshipType::Usage
            | SymbolRelationshipType::UsesType
            | SymbolRelationshipType::ReadsEnv
            | SymbolRelationshipType::Handles => {
                self.find_relationship_references_to_symbol(symbol, relationship_type, limit)?
            }
            SymbolRelationshipType::Contains => self.get_containment_incoming(symbol)?,
        };
        let outgoing = match relationship_type {
            SymbolRelationshipType::Contains => self.get_containment_outgoing(symbol, limit)?,
            _ => {
                let mut references = self.overlay_references(symbol, relationship_type, false);
                let mut persisted = self.symbol_store.get_relationship_edges_from_source(
                    &symbol.id,
                    relationship_type,
                    limit,
                )?;
                self.remove_shadowed_persisted_references(&mut persisted);
                references.extend(persisted);
                references.truncate(limit);
                references
            }
        };

        Ok(SymbolGraph {
            symbol: symbol.clone(),
            incoming,
            outgoing,
        })
    }

    pub fn trace_symbol_graph(
        &self,
        seed: &Symbol,
        relationship_types: &[SymbolRelationshipType],
        direction: SymbolTraceDirection,
        max_depth: usize,
        edge_limit: usize,
        per_node_limit: usize,
    ) -> Result<SymbolTrace, LanguageError> {
        let max_depth = max_depth.min(4);
        let edge_limit = edge_limit.min(200);
        let per_node_limit = per_node_limit.min(50);
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut visited_nodes = HashSet::new();
        let mut seen_edges = HashSet::new();
        let mut queue = VecDeque::new();
        let mut truncated = false;
        let mut unresolved_edges = 0usize;

        visited_nodes.insert(seed.id.clone());
        nodes.push(SymbolTraceNode {
            symbol: seed.clone(),
            depth: 0,
        });
        queue.push_back((seed.clone(), 0usize));

        while let Some((symbol, depth)) = queue.pop_front() {
            if depth >= max_depth || edges.len() >= edge_limit {
                continue;
            }

            for relationship_type in relationship_types {
                if edges.len() >= edge_limit {
                    truncated = true;
                    break;
                }

                let graph = self.get_symbol_graph(&symbol, *relationship_type, per_node_limit)?;
                if graph.incoming.len() >= per_node_limit || graph.outgoing.len() >= per_node_limit
                {
                    truncated = true;
                }

                if direction.includes_incoming() {
                    for reference in graph.incoming {
                        if edges.len() >= edge_limit {
                            truncated = true;
                            break;
                        }

                        let resolved = Self::trace_reference_is_resolved(&reference);
                        if !resolved {
                            unresolved_edges += 1;
                        }
                        let edge_key = (
                            reference.source_symbol.id.clone(),
                            symbol.id.clone(),
                            reference.relationship_type,
                            reference.line,
                            reference.byte_offset,
                        );
                        if seen_edges.insert(edge_key) {
                            edges.push(SymbolTraceEdge {
                                source_symbol: reference.source_symbol.clone(),
                                target_symbol: Some(symbol.clone()),
                                target_name: reference.target_name.clone(),
                                relationship_type: reference.relationship_type,
                                direction: SymbolTraceDirection::Incoming,
                                depth: depth + 1,
                                line: reference.line,
                                resolved,
                                observation_kind: reference.observation_kind,
                                resolution_strategy: reference.resolution_strategy.clone(),
                                resolution_confidence: reference.resolution_confidence,
                                receiver_type: reference.receiver_type.clone(),
                                receiver_is_self: reference.receiver_is_self,
                                byte_offset: reference.byte_offset,
                                qualifier_segments: reference.qualifier_segments.clone(),
                                call_form: reference.call_form.clone(),
                                unresolved_reason: reference.unresolved_reason.clone(),
                            });
                        }

                        if visited_nodes.insert(reference.source_symbol.id.clone()) {
                            nodes.push(SymbolTraceNode {
                                symbol: reference.source_symbol.clone(),
                                depth: depth + 1,
                            });
                            queue.push_back((reference.source_symbol, depth + 1));
                        }
                    }
                }

                if direction.includes_outgoing() {
                    for reference in graph.outgoing {
                        if edges.len() >= edge_limit {
                            truncated = true;
                            break;
                        }

                        let resolved = Self::trace_reference_is_resolved(&reference);
                        if !resolved {
                            unresolved_edges += 1;
                        }
                        let target_key = reference
                            .target_symbol
                            .as_ref()
                            .map(|symbol| symbol.id.clone())
                            .or_else(|| reference.target_symbol_id.clone())
                            .unwrap_or_else(|| {
                                format!(
                                    "unresolved:{}:{}:{}",
                                    reference.target_name,
                                    reference.relationship_type,
                                    reference.line
                                )
                            });
                        let edge_key = (
                            reference.source_symbol.id.clone(),
                            target_key,
                            reference.relationship_type,
                            reference.line,
                            reference.byte_offset,
                        );
                        if seen_edges.insert(edge_key) {
                            edges.push(SymbolTraceEdge {
                                source_symbol: reference.source_symbol.clone(),
                                target_symbol: reference.target_symbol.clone(),
                                target_name: reference.target_name.clone(),
                                relationship_type: reference.relationship_type,
                                direction: SymbolTraceDirection::Outgoing,
                                depth: depth + 1,
                                line: reference.line,
                                resolved,
                                observation_kind: reference.observation_kind,
                                resolution_strategy: reference.resolution_strategy.clone(),
                                resolution_confidence: reference.resolution_confidence,
                                receiver_type: reference.receiver_type.clone(),
                                receiver_is_self: reference.receiver_is_self,
                                byte_offset: reference.byte_offset,
                                qualifier_segments: reference.qualifier_segments.clone(),
                                call_form: reference.call_form.clone(),
                                unresolved_reason: reference.unresolved_reason.clone(),
                            });
                        }

                        if let Some(target_symbol) = reference.target_symbol {
                            if visited_nodes.insert(target_symbol.id.clone()) {
                                nodes.push(SymbolTraceNode {
                                    symbol: target_symbol.clone(),
                                    depth: depth + 1,
                                });
                                queue.push_back((target_symbol, depth + 1));
                            }
                        }
                    }
                }
            }
        }

        Ok(SymbolTrace {
            seed: seed.clone(),
            nodes,
            edges,
            max_depth,
            truncated,
            unresolved_edges,
        })
    }

    pub fn find_symbol_path(
        &self,
        source: &Symbol,
        target: &Symbol,
        relationship_types: &[SymbolRelationshipType],
        direction: SymbolTraceDirection,
        max_hops: usize,
        edge_limit: usize,
        per_node_limit: usize,
        min_confidence: f32,
    ) -> Result<SymbolPath, LanguageError> {
        let max_hops = max_hops.clamp(1, 8);
        let edge_limit = edge_limit.clamp(1, 500);
        let per_node_limit = per_node_limit.clamp(1, 50);
        let min_confidence = min_confidence.clamp(0.0, 1.0);
        let mut frontier = vec![PathCandidate {
            cost: 0,
            symbol: source.clone(),
            edges: Vec::new(),
        }];
        let mut best_cost = HashMap::from([(source.id.clone(), 0u32)]);
        let mut seen_edges = HashSet::new();
        let mut considered_edges = 0usize;
        let mut truncated = false;

        while !frontier.is_empty() {
            let next_index = frontier
                .iter()
                .enumerate()
                .min_by_key(|(_, candidate)| (candidate.cost, candidate.edges.len()))
                .map(|(index, _)| index)
                .unwrap_or(0);
            let candidate = frontier.swap_remove(next_index);

            if candidate.symbol.id == target.id {
                return Ok(SymbolPath {
                    source: source.clone(),
                    target: target.clone(),
                    edges: candidate.edges,
                    total_cost: candidate.cost,
                    visited_nodes: best_cost.len(),
                    considered_edges,
                    truncated,
                });
            }
            if candidate.edges.len() >= max_hops {
                continue;
            }
            if candidate.cost
                > best_cost
                    .get(&candidate.symbol.id)
                    .copied()
                    .unwrap_or(u32::MAX)
            {
                continue;
            }

            for relationship_type in relationship_types {
                if considered_edges >= edge_limit {
                    truncated = true;
                    break;
                }
                let graph =
                    self.get_symbol_graph(&candidate.symbol, *relationship_type, per_node_limit)?;
                if graph.incoming.len() >= per_node_limit || graph.outgoing.len() >= per_node_limit
                {
                    truncated = true;
                }

                if direction.includes_incoming() {
                    for reference in graph.incoming {
                        if considered_edges >= edge_limit {
                            truncated = true;
                            break;
                        }
                        considered_edges += 1;
                        let resolved = Self::trace_reference_is_resolved(&reference);
                        let confidence = Self::reference_effective_confidence(&reference, resolved);
                        if confidence < min_confidence {
                            continue;
                        }
                        let edge_key = (
                            reference.source_symbol.id.clone(),
                            candidate.symbol.id.clone(),
                            reference.relationship_type,
                            reference.line,
                            reference.byte_offset,
                        );
                        if !seen_edges.insert(edge_key) {
                            continue;
                        }
                        let edge_cost = Self::symbol_path_edge_cost(
                            reference.relationship_type,
                            confidence,
                            reference.observation_kind,
                        );
                        let total_cost = candidate.cost.saturating_add(edge_cost);
                        let neighbor = reference.source_symbol.clone();
                        if total_cost >= best_cost.get(&neighbor.id).copied().unwrap_or(u32::MAX) {
                            continue;
                        }
                        best_cost.insert(neighbor.id.clone(), total_cost);
                        let mut edges = candidate.edges.clone();
                        edges.push(SymbolPathEdge::from_reference(
                            &reference,
                            Some(candidate.symbol.clone()),
                            SymbolTraceDirection::Incoming,
                            confidence,
                            edge_cost,
                        ));
                        frontier.push(PathCandidate {
                            cost: total_cost,
                            symbol: neighbor,
                            edges,
                        });
                    }
                }

                if direction.includes_outgoing() {
                    for reference in graph.outgoing {
                        if considered_edges >= edge_limit {
                            truncated = true;
                            break;
                        }
                        considered_edges += 1;
                        let resolved = Self::trace_reference_is_resolved(&reference);
                        let confidence = Self::reference_effective_confidence(&reference, resolved);
                        if confidence < min_confidence {
                            continue;
                        }
                        let Some(neighbor) = reference.target_symbol.clone() else {
                            continue;
                        };
                        let edge_key = (
                            reference.source_symbol.id.clone(),
                            neighbor.id.clone(),
                            reference.relationship_type,
                            reference.line,
                            reference.byte_offset,
                        );
                        if !seen_edges.insert(edge_key) {
                            continue;
                        }
                        let edge_cost = Self::symbol_path_edge_cost(
                            reference.relationship_type,
                            confidence,
                            reference.observation_kind,
                        );
                        let total_cost = candidate.cost.saturating_add(edge_cost);
                        if total_cost >= best_cost.get(&neighbor.id).copied().unwrap_or(u32::MAX) {
                            continue;
                        }
                        best_cost.insert(neighbor.id.clone(), total_cost);
                        let mut edges = candidate.edges.clone();
                        edges.push(SymbolPathEdge::from_reference(
                            &reference,
                            Some(neighbor.clone()),
                            SymbolTraceDirection::Outgoing,
                            confidence,
                            edge_cost,
                        ));
                        frontier.push(PathCandidate {
                            cost: total_cost,
                            symbol: neighbor,
                            edges,
                        });
                    }
                }
            }
        }

        Ok(SymbolPath {
            source: source.clone(),
            target: target.clone(),
            edges: Vec::new(),
            total_cost: 0,
            visited_nodes: best_cost.len(),
            considered_edges,
            truncated,
        })
    }

    fn reference_effective_confidence(reference: &SymbolReference, resolved: bool) -> f32 {
        reference
            .resolution_confidence
            .unwrap_or(if resolved { 0.75 } else { 0.0 })
            .clamp(0.0, 1.0)
    }

    fn symbol_path_edge_cost(
        relationship_type: SymbolRelationshipType,
        confidence: f32,
        observation_kind: RelationshipObservationKind,
    ) -> u32 {
        let relation_cost = match relationship_type {
            SymbolRelationshipType::Handles => 5,
            SymbolRelationshipType::Extends | SymbolRelationshipType::Implements => 7,
            SymbolRelationshipType::Call => 10,
            SymbolRelationshipType::Usage => 12,
            SymbolRelationshipType::UsesType => 14,
            SymbolRelationshipType::Export => 16,
            SymbolRelationshipType::Import => 18,
            SymbolRelationshipType::Contains => 24,
            SymbolRelationshipType::ReadsEnv => 30,
        };
        let confidence_penalty = ((1.0 - confidence.clamp(0.0, 1.0)) * 20.0).round() as u32;
        let observation_penalty = match observation_kind {
            RelationshipObservationKind::SyntaxExtracted => 0,
            RelationshipObservationKind::IndexStructural => 6,
        };
        relation_cost + confidence_penalty + observation_penalty
    }

    fn trace_reference_is_resolved(reference: &SymbolReference) -> bool {
        reference.target_symbol_id.is_some()
            || reference.target_symbol.is_some()
            || reference.relationship_type == SymbolRelationshipType::Import
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
            observation_kind: RelationshipObservationKind::IndexStructural,
            resolution_strategy: Some("parent_id".to_string()),
            resolution_confidence: Some(1.0),
            receiver_type: None,
            receiver_is_self: false,
            import_path: None,
            imported_name: None,
            byte_offset: None,
            qualifier_segments: None,
            call_form: None,
            unresolved_reason: None,
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
                observation_kind: RelationshipObservationKind::IndexStructural,
                resolution_strategy: Some("parent_id".to_string()),
                resolution_confidence: Some(1.0),
                receiver_type: None,
                receiver_is_self: false,
                import_path: None,
                imported_name: None,
                byte_offset: None,
                qualifier_segments: None,
                call_form: None,
                unresolved_reason: None,
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

    fn append_direct_module_export_relationships(
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
                ..Default::default()
            });
        }
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
                ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
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
                        ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
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
                        ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
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
                        ..Default::default()
                    });
                }
            }
        }
    }

    fn append_astro_component_export_relationship(
        &self,
        file_path: &str,
        symbols: &[Symbol],
        relationships: &mut Vec<SymbolRelationship>,
    ) {
        let root_id = Self::synthetic_file_root_id(file_path);
        let Some(root_symbol) = symbols.iter().find(|symbol| symbol.id == root_id) else {
            return;
        };
        let Some(component_symbol) = symbols.iter().find(|symbol| {
            symbol.file_path == file_path
                && symbol.symbol_type == SymbolType::Function
                && symbol.signature.as_deref() == Some("Astro component")
        }) else {
            return;
        };

        relationships.push(SymbolRelationship {
            source_symbol_id: root_symbol.id.clone(),
            source_file_path: file_path.to_string(),
            target_name: "default".to_string(),
            target_symbol_id: Some(component_symbol.id.clone()),
            relationship_type: SymbolRelationshipType::Export,
            line: component_symbol.range.start.line,
            ..Default::default()
        });
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

    /// The number of symbols `filter_visible_symbols` would keep, without
    /// cloning them: only the synthetic file-root symbol is hidden, and the
    /// `parent_id` normalization in `normalize_visible_symbol` never changes
    /// the count.
    fn count_visible_symbols(file_path: &str, symbols: &[Symbol]) -> usize {
        let root_id = Self::synthetic_file_root_id(file_path);
        symbols
            .iter()
            .filter(|symbol| {
                symbol.id != root_id || !Self::is_synthetic_file_root_symbol(symbol)
            })
            .count()
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

    fn push_related_symbol(
        seed: &Symbol,
        candidate: Symbol,
        relationship: String,
        reason: String,
        score: u32,
        distance: u8,
        related: &mut Vec<RelatedSymbol>,
        seen: &mut HashSet<String>,
    ) {
        if candidate.id == seed.id || Self::is_synthetic_file_root_symbol(&candidate) {
            return;
        }

        let mut symbol = candidate;
        let root_id = Self::synthetic_file_root_id(&symbol.file_path);
        if symbol.parent_id.as_deref() == Some(root_id.as_str()) {
            symbol.parent_id = None;
        }

        if seen.insert(symbol.id.clone()) {
            related.push(RelatedSymbol {
                symbol,
                relationship,
                reason,
                score,
                distance,
            });
        }
    }

    fn push_lexical_related_symbols(
        &self,
        seed: &Symbol,
        expanded_limit: usize,
        related: &mut Vec<RelatedSymbol>,
        seen: &mut HashSet<String>,
    ) -> Result<(), LanguageError> {
        let seed_tokens = related_identifier_tokens(seed);
        if seed_tokens.is_empty() {
            return Ok(());
        }

        let mut candidate_files = self.symbol_store.list_all_indexed_files()?;
        candidate_files.retain(|record| is_nearby_related_file(&seed.file_path, &record.file_path));
        candidate_files.sort_by(|a, b| {
            nearby_file_rank(&seed.file_path, &a.file_path)
                .cmp(&nearby_file_rank(&seed.file_path, &b.file_path))
                .then_with(|| a.file_path.cmp(&b.file_path))
        });
        candidate_files.truncate(expanded_limit.clamp(16, 48));

        let mut lexical_candidates = Vec::<(Symbol, f32)>::new();
        for record in candidate_files {
            for candidate in self.get_file_symbols(&record.file_path)? {
                if candidate.id == seed.id || seen.contains(&candidate.id) {
                    continue;
                }

                let score =
                    lexical_related_score(&seed_tokens, &related_identifier_tokens(&candidate));
                if score >= 0.5 {
                    lexical_candidates.push((candidate, score));
                }
            }
        }

        lexical_candidates.sort_by(|(a, a_score), (b, b_score)| {
            b_score
                .partial_cmp(a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    nearby_file_rank(&seed.file_path, &a.file_path)
                        .cmp(&nearby_file_rank(&seed.file_path, &b.file_path))
                })
                .then_with(|| a.file_path.cmp(&b.file_path))
                .then_with(|| a.name.cmp(&b.name))
        });
        lexical_candidates.truncate(12);

        for (candidate, lexical_score) in lexical_candidates {
            let score = (50.0 + (lexical_score * 14.0)).round() as u32;
            let reason = format!(
                "{} shares identifier tokens with {} in a nearby indexed file.",
                candidate.name, seed.name
            );
            Self::push_related_symbol(
                seed,
                candidate,
                "lexical_similarity".to_string(),
                reason,
                score.min(64),
                3,
                related,
                seen,
            );
        }

        Ok(())
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
        stable_symbol_id(file_path, "__file__", SymbolType::Module)
    }

    fn is_synthetic_file_root_symbol(symbol: &Symbol) -> bool {
        symbol.symbol_type == SymbolType::Module && symbol.qualified_name == "__file__"
    }

    fn direct_export_name(content: &str, language: Language, symbol: &Symbol) -> Option<String> {
        let line = symbol_line_text(content, symbol.byte_offset).trim_start();
        match language {
            Language::TypeScript
            | Language::Tsx
            | Language::Astro
            | Language::JavaScript
            | Language::Jsx => {
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
            Language::Markdown
            | Language::Css
            | Language::Scss
            | Language::Sass
            | Language::Less
            | Language::Html
            | Language::Vue
            | Language::Svelte
            | Language::Json
            | Language::Yaml
            | Language::Toml
            | Language::Php
            | Language::Java
            | Language::CSharp
            | Language::Kotlin
            | Language::Ruby
            | Language::Cpp
            | Language::Shell
            | Language::Dockerfile
            | Language::Sql
            | Language::BuildScript => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SymbolGraph {
    pub symbol: Symbol,
    pub incoming: Vec<SymbolReference>,
    pub outgoing: Vec<SymbolReference>,
}

#[derive(Debug, Clone)]
pub struct SymbolTrace {
    pub seed: Symbol,
    pub nodes: Vec<SymbolTraceNode>,
    pub edges: Vec<SymbolTraceEdge>,
    pub max_depth: usize,
    pub truncated: bool,
    pub unresolved_edges: usize,
}

#[derive(Debug, Clone)]
pub struct SymbolTraceNode {
    pub symbol: Symbol,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct SymbolTraceEdge {
    pub source_symbol: Symbol,
    pub target_symbol: Option<Symbol>,
    pub target_name: String,
    pub relationship_type: SymbolRelationshipType,
    pub direction: SymbolTraceDirection,
    pub depth: usize,
    pub line: u32,
    pub resolved: bool,
    pub observation_kind: RelationshipObservationKind,
    pub resolution_strategy: Option<String>,
    pub resolution_confidence: Option<f32>,
    pub receiver_type: Option<String>,
    pub receiver_is_self: bool,
    /// Qualified Rust call observation: byte offset for exact call-site identity.
    pub byte_offset: Option<u32>,
    /// Qualified Rust call observation: normalized qualifier segments.
    pub qualifier_segments: Option<Vec<String>>,
    /// Qualified Rust call observation: syntactic call form.
    pub call_form: Option<String>,
    /// Qualified Rust call observation: stable unresolved reason category.
    pub unresolved_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SymbolPath {
    pub source: Symbol,
    pub target: Symbol,
    pub edges: Vec<SymbolPathEdge>,
    pub total_cost: u32,
    pub visited_nodes: usize,
    pub considered_edges: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureSnapshot {
    pub scope: Option<String>,
    pub modules: Vec<ArchitectureModule>,
    pub edges: Vec<ArchitectureEdge>,
    pub communities: Vec<ArchitectureCommunity>,
    pub hubs: Vec<ArchitectureModule>,
    pub bridge_modules: Vec<ArchitectureBridgeModule>,
    pub bridge_edges: Vec<ArchitectureEdge>,
    pub candidate_module_count: usize,
    pub candidate_edge_count: usize,
    pub raw_aggregate_count: usize,
    pub min_confidence: f32,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureModule {
    pub file_path: String,
    pub symbol_count: usize,
    pub line_count: Option<usize>,
    pub incoming_edge_count: usize,
    pub outgoing_edge_count: usize,
    pub weighted_degree: f32,
    pub distinct_neighbors: usize,
    pub community_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureEdge {
    pub source_file_path: String,
    pub target_file_path: String,
    pub relationship_counts: BTreeMap<String, usize>,
    pub edge_count: usize,
    pub average_confidence: f32,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureCommunity {
    pub id: String,
    pub module_count: usize,
    pub internal_weight: f32,
    pub cross_community_weight: f32,
    pub top_modules: Vec<String>,
    pub relationship_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureBridgeModule {
    pub file_path: String,
    pub community_id: String,
    pub cross_community_weight: f32,
    pub connected_communities: Vec<String>,
}

fn architecture_relationship_weight(relationship_type: SymbolRelationshipType) -> f32 {
    match relationship_type {
        SymbolRelationshipType::Handles => 1.5,
        SymbolRelationshipType::Extends | SymbolRelationshipType::Implements => 1.4,
        SymbolRelationshipType::Call => 1.25,
        SymbolRelationshipType::Usage => 1.0,
        SymbolRelationshipType::UsesType => 0.9,
        SymbolRelationshipType::Import | SymbolRelationshipType::Export => 0.75,
        SymbolRelationshipType::Contains => 0.4,
        SymbolRelationshipType::ReadsEnv => 0.3,
    }
}

fn rounded_architecture_value(value: f32) -> f32 {
    (value * 1_000.0).round() / 1_000.0
}

fn architecture_community_labels(
    module_paths: &[String],
    edges: &[ArchitectureEdge],
) -> HashMap<String, usize> {
    let mut sorted_paths = module_paths.to_vec();
    sorted_paths.sort();
    let path_indexes = sorted_paths
        .iter()
        .enumerate()
        .map(|(index, path)| (path.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut undirected_weights = HashMap::<(usize, usize), f64>::new();
    for edge in edges {
        let (Some(&source), Some(&target)) = (
            path_indexes.get(edge.source_file_path.as_str()),
            path_indexes.get(edge.target_file_path.as_str()),
        ) else {
            continue;
        };
        if source == target {
            continue;
        }
        let key = if source < target {
            (source, target)
        } else {
            (target, source)
        };
        *undirected_weights.entry(key).or_default() += edge.weight as f64;
    }

    let mut adjacency = vec![Vec::<(usize, f64)>::new(); sorted_paths.len()];
    for ((source, target), weight) in undirected_weights {
        adjacency[source].push((target, weight));
        adjacency[target].push((source, weight));
    }
    let degrees = adjacency
        .iter()
        .map(|neighbors| neighbors.iter().map(|(_, weight)| *weight).sum::<f64>())
        .collect::<Vec<_>>();
    let total_degree = degrees.iter().sum::<f64>();
    let mut labels = (0..sorted_paths.len()).collect::<Vec<_>>();
    if total_degree > f64::EPSILON {
        let mut community_degrees = degrees.clone();
        for _ in 0..12 {
            let mut moved = false;
            for node in 0..sorted_paths.len() {
                let degree = degrees[node];
                if degree <= f64::EPSILON {
                    continue;
                }
                let current = labels[node];
                community_degrees[current] = (community_degrees[current] - degree).max(0.0);
                let mut weights_by_community = HashMap::<usize, f64>::new();
                for (neighbor, weight) in &adjacency[node] {
                    *weights_by_community.entry(labels[*neighbor]).or_default() += *weight;
                }
                weights_by_community.entry(current).or_default();

                let mut best = current;
                let mut best_score = f64::NEG_INFINITY;
                for (candidate, internal_weight) in weights_by_community {
                    let score =
                        internal_weight - degree * community_degrees[candidate] / total_degree;
                    if score > best_score + 1e-9
                        || (score - best_score).abs() <= 1e-9 && candidate < best
                    {
                        best = candidate;
                        best_score = score;
                    }
                }
                labels[node] = best;
                community_degrees[best] += degree;
                moved |= best != current;
            }
            if !moved {
                break;
            }
        }
    }

    sorted_paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| (path, labels[index]))
        .collect()
}

#[derive(Debug, Clone)]
pub struct SymbolPathEdge {
    pub source_symbol: Symbol,
    pub target_symbol: Option<Symbol>,
    pub target_name: String,
    pub relationship_type: SymbolRelationshipType,
    pub traversal_direction: SymbolTraceDirection,
    pub line: u32,
    pub observation_kind: RelationshipObservationKind,
    pub resolution_strategy: Option<String>,
    pub resolution_confidence: Option<f32>,
    pub receiver_type: Option<String>,
    pub receiver_is_self: bool,
    pub effective_confidence: f32,
    pub cost: u32,
    /// Qualified Rust call observation: byte offset for exact call-site identity.
    pub byte_offset: Option<u32>,
    /// Qualified Rust call observation: normalized qualifier segments.
    pub qualifier_segments: Option<Vec<String>>,
    /// Qualified Rust call observation: syntactic call form.
    pub call_form: Option<String>,
    /// Qualified Rust call observation: stable unresolved reason category.
    pub unresolved_reason: Option<String>,
}

impl SymbolPathEdge {
    fn from_reference(
        reference: &SymbolReference,
        target_symbol: Option<Symbol>,
        traversal_direction: SymbolTraceDirection,
        effective_confidence: f32,
        cost: u32,
    ) -> Self {
        Self {
            source_symbol: reference.source_symbol.clone(),
            target_symbol,
            target_name: reference.target_name.clone(),
            relationship_type: reference.relationship_type,
            traversal_direction,
            line: reference.line,
            observation_kind: reference.observation_kind,
            resolution_strategy: reference.resolution_strategy.clone(),
            resolution_confidence: reference.resolution_confidence,
            receiver_type: reference.receiver_type.clone(),
            receiver_is_self: reference.receiver_is_self,
            effective_confidence,
            cost,
            byte_offset: reference.byte_offset,
            qualifier_segments: reference.qualifier_segments.clone(),
            call_form: reference.call_form.clone(),
            unresolved_reason: reference.unresolved_reason.clone(),
        }
    }
}

struct PathCandidate {
    cost: u32,
    symbol: Symbol,
    edges: Vec<SymbolPathEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolTraceDirection {
    Incoming,
    Outgoing,
    Both,
}

impl SymbolTraceDirection {
    pub fn includes_incoming(self) -> bool {
        matches!(self, Self::Incoming | Self::Both)
    }

    pub fn includes_outgoing(self) -> bool {
        matches!(self, Self::Outgoing | Self::Both)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incoming => "incoming",
            Self::Outgoing => "outgoing",
            Self::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedSymbol {
    pub symbol: Symbol,
    pub relationship: String,
    pub reason: String,
    pub score: u32,
    pub distance: u8,
}

/// Statistics about indexing operations
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub symbols_extracted: usize,
    pub files_failed: usize,
    pub duration_ms: u64,
    pub files_discovered: usize,
    pub supported_files: usize,
    pub files_fresh: usize,
    pub files_reindexed: usize,
    pub anchors_extracted: usize,
    pub relationships_extracted: usize,
    pub load_ms: u64,
    pub freshness_check_ms: u64,
    pub parse_extract_ms: u64,
    pub relationship_enrichment_ms: u64,
    pub db_write_ms: u64,
    pub cache_update_ms: u64,
    pub supported_by_language: Vec<IndexLanguageCount>,
    pub skipped_by_reason: Vec<IndexSkipCount>,
}

/// Compute a simple hash of content for change detection
fn astro_component_symbol(file_path: &str, content: &str) -> Option<Symbol> {
    let name = Path::new(file_path)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())?
        .to_string();
    let (end_line, end_character) = file_end_position(content);

    Some(Symbol {
        id: format!("{}::{}#{}", file_path, name, SymbolType::Function),
        name: name.clone(),
        qualified_name: name,
        symbol_type: SymbolType::Function,
        file_path: file_path.to_string(),
        range: Range {
            start: Position::new(0, 0),
            end: Position::new(end_line, end_character),
        },
        byte_offset: 0,
        byte_length: content.len(),
        parent_id: None,
        docstring: None,
        signature: Some("Astro component".to_string()),
        content_hash: compute_hash(content),
    })
}

fn astro_script_projection(content: &str) -> String {
    let mut projected = content
        .bytes()
        .map(|byte| if byte == b'\n' { b'\n' } else { b' ' })
        .collect::<Vec<_>>();

    if let Some((start, end)) = astro_frontmatter_body_range(content) {
        projected[start..end].copy_from_slice(&content.as_bytes()[start..end]);
    }

    for (start, end) in astro_script_body_ranges(content) {
        projected[start..end].copy_from_slice(&content.as_bytes()[start..end]);
    }

    String::from_utf8(projected).unwrap_or_default()
}

fn astro_frontmatter_body_range(content: &str) -> Option<(usize, usize)> {
    let first_line = content.lines().next()?;
    if first_line.trim() != "---" {
        return None;
    }

    let body_start = content.find('\n').map(|index| index + 1)?;
    let mut offset = body_start;
    for line in content[body_start..].split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        if line_without_newline.trim() == "---" {
            return Some((body_start, offset));
        }
        offset += line.len();
    }

    None
}

fn astro_script_body_ranges(content: &str) -> Vec<(usize, usize)> {
    tag_body_ranges(content, "script")
}

fn tag_body_projection(content: &str, tag_name: &str) -> Option<String> {
    let ranges = tag_body_ranges(content, tag_name);
    if ranges.is_empty() {
        return None;
    }

    let mut projected = content
        .bytes()
        .map(|byte| if byte == b'\n' { b'\n' } else { b' ' })
        .collect::<Vec<_>>();

    for (start, end) in ranges {
        projected[start..end].copy_from_slice(&content.as_bytes()[start..end]);
    }

    String::from_utf8(projected).ok()
}

fn tag_body_ranges(content: &str, tag_name: &str) -> Vec<(usize, usize)> {
    let lower = content.to_ascii_lowercase();
    let mut ranges = Vec::new();
    let mut search_start = 0usize;
    let open_tag = format!("<{tag_name}");
    let close_tag = format!("</{tag_name}>");

    while let Some(relative_start) = lower[search_start..].find(&open_tag) {
        let tag_start = search_start + relative_start;
        let Some(relative_tag_end) = lower[tag_start..].find('>') else {
            break;
        };
        let body_start = tag_start + relative_tag_end + 1;
        let Some(relative_body_end) = lower[body_start..].find(&close_tag) else {
            break;
        };
        let body_end = body_start + relative_body_end;
        if body_start < body_end {
            ranges.push((body_start, body_end));
        }
        search_start = body_end.saturating_add(close_tag.len());
    }

    ranges
}

fn extract_semantic_anchors(file_path: &str, content: &str) -> Vec<SemanticAnchor> {
    // Credentials/key-material files are all secret and carry no useful anchors —
    // never index their contents (secret values in normal files are scrubbed at
    // context-pack emission via `redact_secret_tokens`). See `crate::secrets`.
    if crate::secrets::is_secret_file(file_path) {
        return Vec::new();
    }

    let mut anchors = Vec::new();
    let mut seen = HashSet::new();
    let limit = semantic_anchor_limit_for_file(file_path);

    extract_rationale_anchors(file_path, content, &mut anchors, &mut seen, limit);
    if anchors.len() >= limit {
        return anchors;
    }

    if matches!(Language::from_path(file_path), Some(Language::Markdown)) {
        extract_markdown_document_anchors(file_path, content, &mut anchors, &mut seen, limit);
        if anchors.len() >= limit {
            return anchors;
        }
    }

    if is_translation_resource_path(file_path) {
        extract_translation_definition_anchors(file_path, content, &mut anchors, &mut seen, limit);
        if anchors.len() >= limit {
            return anchors;
        }
    }

    extract_translation_usage_anchors(file_path, content, &mut anchors, &mut seen, limit);
    if anchors.len() >= limit {
        return anchors;
    }

    extract_yaml_route_config_anchors(file_path, content, &mut anchors, &mut seen, limit);
    if anchors.len() >= limit {
        return anchors;
    }

    extract_jsx_section_label_anchors(file_path, content, &mut anchors, &mut seen, limit);
    if anchors.len() >= limit {
        return anchors;
    }

    // M5.14 — the generic quoted-literal / token scan below only adds value on
    // front-end, style and markup files, where string literals and tokens are
    // genuine cross-references (routes, CSS custom properties, event/command/
    // service names). On systems / back-end / config files it is redundant with
    // SYMBOLS (which own code navigation) and produces mostly noise plus
    // misclassification — on Firefox it minted ~5M anchors from C++, tagging
    // header names `translation_key`, `--x` decrements `css_token`, and class
    // names `config_key`. Skip it there; the precise translation/route extractors
    // above already ran for every file, so real i18n and route cross-references
    // are still captured.
    if !generic_literal_anchor_scan_applies(file_path) {
        return anchors;
    }

    for (line_index, line) in content.lines().enumerate() {
        let preview = line.trim().chars().take(240).collect::<String>();
        for (value, character) in extract_quoted_values(line)
            .into_iter()
            .chain(extract_css_tokens(line))
            .chain(extract_unquoted_keys(line))
        {
            let value = value.trim().to_string();
            if !is_semantic_anchor_value(&value) {
                continue;
            }
            let kind = semantic_anchor_kind(&value, line);
            let confidence = semantic_anchor_confidence(&value, line);
            push_semantic_anchor(
                &mut anchors,
                &mut seen,
                file_path,
                kind,
                value,
                line_index as u32,
                character as u32,
                preview.clone(),
                confidence,
                limit,
            );
            if anchors.len() >= limit {
                return anchors;
            }
        }
    }

    anchors
}

fn extract_rationale_anchors(
    file_path: &str,
    content: &str,
    anchors: &mut Vec<SemanticAnchor>,
    seen: &mut HashSet<(String, String, u32, u32)>,
    limit: usize,
) {
    const MARKERS: [&str; 7] = [
        "why:",
        "rationale:",
        "reason:",
        "decision:",
        "tradeoff:",
        "note:",
        "hack:",
    ];

    for (line_index, line) in content.lines().enumerate() {
        if anchors.len() >= limit {
            return;
        }
        let lower = line.to_ascii_lowercase();
        let Some((marker, character)) = MARKERS
            .iter()
            .filter_map(|marker| lower.find(marker).map(|index| (*marker, index)))
            .min_by_key(|(_, index)| *index)
        else {
            continue;
        };
        let prefix = line[..character].trim();
        if !prefix.is_empty()
            && !prefix.chars().all(|character| {
                character.is_whitespace()
                    || matches!(
                        character,
                        '/' | '*' | '#' | '-' | '!' | '<' | '>' | ';' | '[' | ']' | '(' | ')'
                    )
            })
        {
            continue;
        }
        let rationale = line[character + marker.len()..].trim();
        let value = if rationale.is_empty() {
            line.trim()
        } else {
            rationale
        };
        push_semantic_anchor(
            anchors,
            seen,
            file_path,
            "rationale",
            value.chars().take(240).collect::<String>(),
            line_index as u32,
            character as u32,
            line.trim().chars().take(240).collect::<String>(),
            if matches!(marker, "why:" | "rationale:" | "decision:") {
                0.98
            } else {
                0.9
            },
            limit,
        );
    }
}

/// Track H — index standalone single-line JSX section comments
/// (`{/* Mobile navigation */}`) as `section_label` anchors, so a known-file
/// query can land on precise internal lines even when a 600-line React
/// component is structurally one function. The TRIMMED line must be exactly ONE
/// such comment (nothing else on the line) — an inline/trailing comment next to
/// JSX is not a section boundary, so it is skipped (prefer the false negative).
/// Section labels are semantic evidence, NEVER structural graph edges:
/// `target_name`/`target_file_path` stay `None`, which also keeps them out of
/// the anchor-target backfill (it keys on `target_name`).
///
/// Template-literal guard (HEURISTIC, false-negative direction): a
/// `{/* ... */}` line INSIDE a backtick template literal (e.g. a test fixture
/// embedding JSX source as a string) is data, not a section boundary. We track
/// the running parity of unescaped backticks in the preceding content and skip
/// candidate lines while the parity is odd. A stray backtick in ordinary code
/// can therefore suppress later labels (acceptable false negative), but
/// template-literal contents can never mint an anchor.
fn extract_jsx_section_label_anchors(
    file_path: &str,
    content: &str,
    anchors: &mut Vec<SemanticAnchor>,
    seen: &mut HashSet<(String, String, u32, u32)>,
    limit: usize,
) {
    if !jsx_section_label_scan_applies(file_path) {
        return;
    }

    let mut inside_template_literal = false;
    for (line_index, line) in content.lines().enumerate() {
        if anchors.len() >= limit {
            return;
        }
        // Parity as of the START of this line decides whether the line is
        // template-literal data; the line's own backticks flip the parity for
        // the lines after it.
        let started_inside_template_literal = inside_template_literal;
        if count_unescaped_backticks(line) % 2 == 1 {
            inside_template_literal = !inside_template_literal;
        }
        if started_inside_template_literal {
            continue;
        }
        let trimmed = line.trim();
        let Some(inner) = trimmed
            .strip_prefix("{/*")
            .and_then(|rest| rest.strip_suffix("*/}"))
        else {
            continue;
        };
        // An embedded comment boundary means the line is NOT one single comment
        // (e.g. `{/* a */}{/* b */}`) — skip rather than mint a garbled label.
        if inner.contains("*/") || inner.contains("/*") {
            continue;
        }
        let label = inner.trim();
        if label.is_empty() || label.chars().count() > 120 {
            continue;
        }
        // Column of the opening `{` (byte offset, matching the other extractors).
        let character = (line.len() - line.trim_start().len()) as u32;
        push_semantic_anchor(
            anchors,
            seen,
            file_path,
            "section_label",
            label,
            line_index as u32,
            character,
            label.chars().take(240).collect::<String>(),
            0.9,
            limit,
        );
    }
}

/// Count backticks on a line that are not escaped by a preceding odd run of
/// backslashes. Used for the cheap template-literal parity heuristic in
/// `extract_jsx_section_label_anchors`.
fn count_unescaped_backticks(line: &str) -> usize {
    let mut count = 0usize;
    let mut backslash_run = 0usize;
    for character in line.chars() {
        if character == '\\' {
            backslash_run += 1;
            continue;
        }
        if character == '`' && backslash_run.is_multiple_of(2) {
            count += 1;
        }
        backslash_run = 0;
    }
    count
}

fn extract_markdown_document_anchors(
    file_path: &str,
    content: &str,
    anchors: &mut Vec<SemanticAnchor>,
    seen: &mut HashSet<(String, String, u32, u32)>,
    limit: usize,
) {
    for (line_index, line) in content.lines().enumerate() {
        if anchors.len() >= limit {
            return;
        }
        let preview = line.trim().chars().take(240).collect::<String>();

        for (raw_target, character) in extract_markdown_link_targets(line) {
            let Some((target_file_path, target_name)) =
                resolve_markdown_reference(file_path, &raw_target, false)
            else {
                continue;
            };
            push_linked_semantic_anchor(
                anchors,
                seen,
                file_path,
                "documentation_link",
                raw_target,
                line_index as u32,
                character as u32,
                preview.clone(),
                0.97,
                target_file_path,
                target_name,
                limit,
            );
        }

        for (raw_target, character) in extract_wikilink_targets(line) {
            let Some((target_file_path, target_name)) =
                resolve_markdown_reference(file_path, &raw_target, true)
            else {
                continue;
            };
            push_linked_semantic_anchor(
                anchors,
                seen,
                file_path,
                "documentation_link",
                raw_target,
                line_index as u32,
                character as u32,
                preview.clone(),
                0.94,
                target_file_path,
                target_name,
                limit,
            );
        }

        for (reference, character) in extract_design_reference_tokens(line) {
            push_linked_semantic_anchor(
                anchors,
                seen,
                file_path,
                "design_reference",
                reference.clone(),
                line_index as u32,
                character as u32,
                preview.clone(),
                0.92,
                None,
                Some(reference),
                limit,
            );
        }

        for (reference, character) in extract_inline_code_references(line) {
            push_linked_semantic_anchor(
                anchors,
                seen,
                file_path,
                "design_symbol_reference",
                reference.clone(),
                line_index as u32,
                character as u32,
                preview.clone(),
                0.78,
                None,
                Some(reference),
                limit,
            );
        }
    }
}

fn extract_markdown_link_targets(line: &str) -> Vec<(String, usize)> {
    let mut targets = Vec::new();
    let mut search_start = 0usize;
    while let Some(relative) = line[search_start..].find("](") {
        let target_start = search_start + relative + 2;
        let Some(relative_end) = line[target_start..].find(')') else {
            break;
        };
        let target_end = target_start + relative_end;
        let raw = line[target_start..target_end].trim();
        let raw = if raw.starts_with('<') && raw.ends_with('>') {
            &raw[1..raw.len().saturating_sub(1)]
        } else {
            raw.split_whitespace().next().unwrap_or_default()
        };
        if !raw.is_empty() {
            targets.push((raw.to_string(), target_start));
        }
        search_start = target_end.saturating_add(1);
    }
    targets
}

fn extract_wikilink_targets(line: &str) -> Vec<(String, usize)> {
    let mut targets = Vec::new();
    let mut search_start = 0usize;
    while let Some(relative) = line[search_start..].find("[[") {
        let target_start = search_start + relative + 2;
        let Some(relative_end) = line[target_start..].find("]]") else {
            break;
        };
        let target_end = target_start + relative_end;
        let raw = line[target_start..target_end]
            .split('|')
            .next()
            .unwrap_or_default()
            .trim();
        if !raw.is_empty() {
            targets.push((raw.to_string(), target_start));
        }
        search_start = target_end.saturating_add(2);
    }
    targets
}

fn resolve_markdown_reference(
    file_path: &str,
    raw_target: &str,
    wikilink: bool,
) -> Option<(Option<String>, Option<String>)> {
    let target = raw_target.trim();
    let lower = target.to_ascii_lowercase();
    if target.is_empty()
        || lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("data:")
        || target.starts_with('/')
    {
        return None;
    }

    let (path_part, fragment) = target
        .split_once('#')
        .map(|(path, fragment)| (path, Some(fragment)))
        .unwrap_or((target, None));
    let target_file_path = if path_part.is_empty() {
        Some(file_path.to_string())
    } else {
        let path_part = path_part.split('?').next().unwrap_or(path_part);
        let mut relative = PathBuf::from(path_part);
        if wikilink && relative.extension().is_none() {
            relative.set_extension("md");
        }
        let parent = Path::new(file_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        Some(
            normalize_path(&parent.join(relative))
                .to_string_lossy()
                .replace('\\', "/"),
        )
    };
    let target_name = fragment
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    Some((target_file_path, target_name))
}

fn extract_design_reference_tokens(line: &str) -> Vec<(String, usize)> {
    let upper = line.to_ascii_uppercase();
    let mut references = Vec::new();
    for prefix in ["ADR", "RFC"] {
        let mut search_start = 0usize;
        while let Some(relative) = upper[search_start..].find(prefix) {
            let start = search_start + relative;
            let boundary_ok = start == 0 || !upper.as_bytes()[start - 1].is_ascii_alphanumeric();
            let mut index = start + prefix.len();
            while index < upper.len()
                && matches!(upper.as_bytes()[index], b'-' | b'_' | b' ' | b':')
            {
                index += 1;
            }
            let digits_start = index;
            while index < upper.len() && upper.as_bytes()[index].is_ascii_digit() {
                index += 1;
            }
            if boundary_ok && index > digits_start {
                references.push((format!("{}-{}", prefix, &upper[digits_start..index]), start));
            }
            search_start = (start + prefix.len()).min(upper.len());
        }
    }
    references
}

fn extract_inline_code_references(line: &str) -> Vec<(String, usize)> {
    let mut references = Vec::new();
    let mut search_start = 0usize;
    while let Some(relative_start) = line[search_start..].find('`') {
        let start = search_start + relative_start;
        if line[start..].starts_with("```") {
            search_start = start.saturating_add(3);
            continue;
        }
        let value_start = start + 1;
        let Some(relative_end) = line[value_start..].find('`') else {
            break;
        };
        let end = value_start + relative_end;
        let value = normalize_inline_symbol_reference(&line[value_start..end]);
        if let Some(value) = value {
            references.push((value, value_start));
        }
        search_start = end.saturating_add(1);
    }
    references
}

fn normalize_inline_symbol_reference(value: &str) -> Option<String> {
    let value = value
        .trim()
        .strip_suffix("()")
        .unwrap_or(value.trim())
        .trim();
    let lower = value.to_ascii_lowercase();
    if value.len() < 3
        || value.len() > 120
        || value.contains(char::is_whitespace)
        || value.contains('/')
        || matches!(
            lower.as_str(),
            "true"
                | "false"
                | "null"
                | "none"
                | "undefined"
                | "rust"
                | "typescript"
                | "javascript"
                | "python"
                | "markdown"
        )
        || !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '_' | ':' | '.' | '#' | '-' | '$')
        })
    {
        return None;
    }
    Some(value.to_string())
}

/// M5.14 — languages for which the generic quoted-literal / token anchor scan is
/// worthwhile: front-end, style and markup, where string literals and tokens are
/// real cross-references. Systems / back-end languages (C/C++, Rust, Go, Python,
/// Java, …) and config formats (JSON/YAML/TOML) are excluded — their navigation is
/// owned by symbols (and, for config, by the precise route/translation extractors),
/// and the generic scan there is pure noise. Unknown extensions are excluded.
fn generic_literal_anchor_scan_applies(file_path: &str) -> bool {
    matches!(
        Language::capability_for_path(file_path).map(|capability| capability.language),
        Some(
            Language::TypeScript
                | Language::Tsx
                | Language::JavaScript
                | Language::Jsx
                | Language::Astro
                | Language::Vue
                | Language::Svelte
                | Language::Css
                | Language::Scss
                | Language::Sass
                | Language::Less
                | Language::Html
                | Language::Markdown
        )
    )
}

/// Track H — languages in which a `{/* … */}` JSX comment can actually occur:
/// TSX, JSX, JavaScript-with-JSX (`.js` files may contain JSX) and Astro. Plain
/// TypeScript (`.ts`) cannot contain JSX, and everywhere else the `{/* … */}`
/// shape is not a comment at all — excluded so no false section labels are
/// minted from unrelated syntax.
fn jsx_section_label_scan_applies(file_path: &str) -> bool {
    matches!(
        Language::capability_for_path(file_path).map(|capability| capability.language),
        Some(Language::Tsx | Language::Jsx | Language::JavaScript | Language::Astro)
    )
}

fn extract_yaml_route_config_anchors(
    file_path: &str,
    content: &str,
    anchors: &mut Vec<SemanticAnchor>,
    seen: &mut HashSet<(String, String, u32, u32)>,
    limit: usize,
) {
    if !matches!(Language::from_path(file_path), Some(Language::Yaml)) {
        return;
    }
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(content) else {
        return;
    };

    collect_yaml_route_config_anchor_values(file_path, content, &value, None, anchors, seen, limit);
}

fn collect_yaml_route_config_anchor_values(
    file_path: &str,
    content: &str,
    value: &serde_yaml::Value,
    key_hint: Option<&str>,
    anchors: &mut Vec<SemanticAnchor>,
    seen: &mut HashSet<(String, String, u32, u32)>,
    limit: usize,
) {
    if anchors.len() >= limit {
        return;
    }

    match value {
        serde_yaml::Value::Mapping(map) => {
            for (key, value) in map {
                if anchors.len() >= limit {
                    return;
                }
                let key_hint = key.as_str();
                collect_yaml_route_config_anchor_values(
                    file_path, content, value, key_hint, anchors, seen, limit,
                );
            }
        }
        serde_yaml::Value::Sequence(items) => {
            for item in items {
                if anchors.len() >= limit {
                    return;
                }
                collect_yaml_route_config_anchor_values(
                    file_path, content, item, key_hint, anchors, seen, limit,
                );
            }
        }
        serde_yaml::Value::String(text)
            if key_hint.is_some_and(is_route_config_key) && is_route_anchor_value(text) =>
        {
            let location = locate_anchor_value(content, text);
            let preview = content
                .lines()
                .nth(location.line as usize)
                .unwrap_or_default()
                .trim()
                .chars()
                .take(240)
                .collect::<String>();
            push_semantic_anchor(
                anchors,
                seen,
                file_path,
                "route",
                text,
                location.line,
                location.character,
                preview,
                0.95,
                limit,
            );
        }
        _ => {}
    }
}

fn is_route_config_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "path" | "route" | "source" | "destination" | "redirect" | "rewrite" | "url"
    )
}

fn is_route_anchor_value(value: &str) -> bool {
    value.starts_with('/') && !value.contains(char::is_whitespace) && value.len() <= 160
}

fn locate_anchor_value(content: &str, value: &str) -> AnchorLocation {
    let quoted = format!("\"{value}\"");
    let single_quoted = format!("'{value}'");
    for (line_index, line) in content.lines().enumerate() {
        if let Some(character) = line
            .find(&quoted)
            .map(|idx| idx + 1)
            .or_else(|| line.find(&single_quoted).map(|idx| idx + 1))
            .or_else(|| line.find(value))
        {
            return AnchorLocation {
                line: line_index as u32,
                character: character as u32,
            };
        }
    }
    AnchorLocation {
        line: 0,
        character: 0,
    }
}

fn semantic_anchor_limit_for_file(file_path: &str) -> usize {
    if is_translation_resource_path(file_path) {
        2048
    } else {
        256
    }
}

#[allow(clippy::too_many_arguments)]
fn push_semantic_anchor(
    anchors: &mut Vec<SemanticAnchor>,
    seen: &mut HashSet<(String, String, u32, u32)>,
    file_path: &str,
    kind: impl Into<String>,
    value: impl Into<String>,
    line: u32,
    character: u32,
    preview: String,
    confidence: f32,
    limit: usize,
) -> bool {
    if anchors.len() >= limit {
        return false;
    }
    let kind = kind.into();
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return true;
    }
    let key = (kind.clone(), value.to_string(), line, character);
    if !seen.insert(key) {
        return true;
    }

    anchors.push(SemanticAnchor {
        id: format!(
            "{}::anchor:{}:{}:{}:{}",
            file_path,
            kind,
            line,
            character,
            compute_hash(value)
        ),
        file_path: file_path.to_string(),
        kind,
        value: value.to_string(),
        line,
        character,
        preview,
        confidence,
        owner_symbol_id: None,
        target_file_path: None,
        target_name: None,
        target_symbol_id: None,
    });
    true
}

#[allow(clippy::too_many_arguments)]
fn push_linked_semantic_anchor(
    anchors: &mut Vec<SemanticAnchor>,
    seen: &mut HashSet<(String, String, u32, u32)>,
    file_path: &str,
    kind: impl Into<String>,
    value: impl Into<String>,
    line: u32,
    character: u32,
    preview: String,
    confidence: f32,
    target_file_path: Option<String>,
    target_name: Option<String>,
    limit: usize,
) -> bool {
    let before = anchors.len();
    let inserted = push_semantic_anchor(
        anchors, seen, file_path, kind, value, line, character, preview, confidence, limit,
    );
    if inserted && anchors.len() > before {
        if let Some(anchor) = anchors.last_mut() {
            anchor.target_file_path = target_file_path;
            anchor.target_name = target_name;
        }
    }
    inserted
}

fn attach_semantic_anchor_context(anchors: &mut [SemanticAnchor], symbols: &[Symbol]) {
    let file_root = symbols
        .iter()
        .find(|symbol| LanguageService::is_synthetic_file_root_symbol(symbol));
    let headings = symbols
        .iter()
        .filter(|symbol| symbol.symbol_type == SymbolType::Heading)
        .collect::<Vec<_>>();

    for anchor in anchors {
        let owner = if !headings.is_empty() {
            headings
                .iter()
                .copied()
                .filter(|heading| heading.range.start.line <= anchor.line)
                .max_by_key(|heading| heading.range.start.line)
                .or(file_root)
        } else {
            symbols
                .iter()
                .filter(|symbol| {
                    !LanguageService::is_synthetic_file_root_symbol(symbol)
                        && symbol.range.start.line <= anchor.line
                        && symbol.range.end.line >= anchor.line
                })
                .min_by_key(|symbol| {
                    (
                        symbol
                            .range
                            .end
                            .line
                            .saturating_sub(symbol.range.start.line),
                        std::cmp::Reverse(symbol.range.start.line),
                    )
                })
                .or_else(|| {
                    symbols
                        .iter()
                        .filter(|symbol| {
                            !LanguageService::is_synthetic_file_root_symbol(symbol)
                                && symbol.range.start.line >= anchor.line
                                && symbol.range.start.line.saturating_sub(anchor.line) <= 3
                        })
                        .min_by_key(|symbol| symbol.range.start.line)
                })
                .or(file_root)
        };
        anchor.owner_symbol_id = owner.map(|symbol| symbol.id.clone());

        if anchor.target_symbol_id.is_none()
            && anchor.target_file_path.as_deref() == Some(anchor.file_path.as_str())
        {
            if let Some(target_name) = anchor.target_name.as_deref() {
                let candidates = symbols
                    .iter()
                    .filter(|symbol| semantic_anchor_target_matches(symbol, target_name))
                    .collect::<Vec<_>>();
                if candidates.len() == 1 {
                    anchor.target_symbol_id = Some(candidates[0].id.clone());
                }
            }
        }
    }
}

fn semantic_anchor_target_matches(symbol: &Symbol, target_name: &str) -> bool {
    symbol.name.eq_ignore_ascii_case(target_name)
        || symbol.qualified_name.eq_ignore_ascii_case(target_name)
        || symbol.symbol_type == SymbolType::Heading
            && markdown_heading_slug(&symbol.name).eq_ignore_ascii_case(target_name)
}

fn markdown_heading_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '_' {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(character);
        } else if character.is_whitespace() || character == '-' {
            pending_dash = true;
        }
    }
    slug
}

fn extract_translation_definition_anchors(
    file_path: &str,
    content: &str,
    anchors: &mut Vec<SemanticAnchor>,
    seen: &mut HashSet<(String, String, u32, u32)>,
    limit: usize,
) {
    let mut entries = Vec::new();
    if file_path.to_lowercase().ends_with(".json") {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
            collect_json_translation_entries(&value, &mut Vec::new(), &mut entries);
        }
    } else if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(content) {
        collect_yaml_translation_entries(&value, &mut Vec::new(), &mut entries);
    }

    if entries.is_empty() {
        collect_line_based_translation_entries(content, &mut entries);
    }

    for entry in entries {
        if anchors.len() >= limit {
            return;
        }
        let location =
            locate_translation_entry(content, &entry.key_path, &entry.leaf_key, &entry.text);
        let preview = format_translation_preview(&entry.key_path, &entry.text);
        push_semantic_anchor(
            anchors,
            seen,
            file_path,
            "translation_definition_key",
            entry.key_path.clone(),
            location.line,
            location.character,
            preview.clone(),
            0.98,
            limit,
        );
        if is_translation_text_value(&entry.text) {
            push_semantic_anchor(
                anchors,
                seen,
                file_path,
                "translation_text",
                entry.text,
                location.line,
                location.character,
                preview,
                0.96,
                limit,
            );
        }
    }
}

#[derive(Debug)]
struct TranslationEntry {
    key_path: String,
    leaf_key: String,
    text: String,
}

#[derive(Debug, Clone, Copy)]
struct AnchorLocation {
    line: u32,
    character: u32,
}

fn collect_json_translation_entries(
    value: &serde_json::Value,
    path: &mut Vec<String>,
    entries: &mut Vec<TranslationEntry>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if !is_translation_key_segment(key) {
                    continue;
                }
                path.push(key.clone());
                collect_json_translation_entries(value, path, entries);
                path.pop();
            }
        }
        serde_json::Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                path.push(index.to_string());
                collect_json_translation_entries(value, path, entries);
                path.pop();
            }
        }
        serde_json::Value::String(text) => push_translation_entry(path, text, entries),
        _ => {}
    }
}

fn collect_yaml_translation_entries(
    value: &serde_yaml::Value,
    path: &mut Vec<String>,
    entries: &mut Vec<TranslationEntry>,
) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (key, value) in map {
                let Some(key) = key.as_str() else {
                    continue;
                };
                if !is_translation_key_segment(key) {
                    continue;
                }
                path.push(key.to_string());
                collect_yaml_translation_entries(value, path, entries);
                path.pop();
            }
        }
        serde_yaml::Value::Sequence(items) => {
            for (index, value) in items.iter().enumerate() {
                path.push(index.to_string());
                collect_yaml_translation_entries(value, path, entries);
                path.pop();
            }
        }
        serde_yaml::Value::String(text) => push_translation_entry(path, text, entries),
        _ => {}
    }
}

fn push_translation_entry(path: &[String], text: &str, entries: &mut Vec<TranslationEntry>) {
    if path.is_empty() || !is_translation_text_value(text) {
        return;
    }
    let key_path = path.join(".");
    let leaf_key = path.last().cloned().unwrap_or_default();
    entries.push(TranslationEntry {
        key_path,
        leaf_key,
        text: text.to_string(),
    });
}

fn collect_line_based_translation_entries(content: &str, entries: &mut Vec<TranslationEntry>) {
    let mut stack: Vec<(usize, String)> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        let Some((raw_key, raw_value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = raw_key.trim().trim_matches(['"', '\'']);
        if !is_translation_key_segment(key) {
            continue;
        }
        let indent = line.len().saturating_sub(line.trim_start().len());
        while stack.last().is_some_and(|(level, _)| *level >= indent) {
            stack.pop();
        }
        let mut path = stack
            .iter()
            .map(|(_, segment)| segment.clone())
            .collect::<Vec<_>>();
        path.push(key.to_string());

        let value = raw_value
            .trim()
            .trim_end_matches(',')
            .trim()
            .trim_matches(['"', '\'']);
        if value.is_empty() || matches!(value, "{" | "[" | "|" | ">") {
            stack.push((indent, key.to_string()));
            continue;
        }
        push_translation_entry(&path, value, entries);
    }
}

fn locate_translation_entry(
    content: &str,
    key_path: &str,
    leaf_key: &str,
    text: &str,
) -> AnchorLocation {
    let quoted_key = format!("\"{}\"", leaf_key);
    let quoted_text = format!("\"{}\"", text);
    for (line_index, line) in content.lines().enumerate() {
        if line.contains(&quoted_key)
            || line.contains(leaf_key)
                && (line.contains(&quoted_text) || line.contains(text) || line.contains(':'))
            || line.contains(key_path)
        {
            let character = line.find(leaf_key).or_else(|| line.find(text)).unwrap_or(0);
            return AnchorLocation {
                line: line_index as u32,
                character: character as u32,
            };
        }
    }
    AnchorLocation {
        line: 0,
        character: 0,
    }
}

fn format_translation_preview(key_path: &str, text: &str) -> String {
    let text = text.trim().chars().take(180).collect::<String>();
    format!("{} = {}", key_path, text)
}

fn extract_translation_usage_anchors(
    file_path: &str,
    content: &str,
    anchors: &mut Vec<SemanticAnchor>,
    seen: &mut HashSet<(String, String, u32, u32)>,
    limit: usize,
) {
    if !is_translation_usage_source_path(file_path) {
        return;
    }

    let alias_namespaces = extract_translation_call_alias_namespaces(content);
    let namespaces = extract_translation_namespaces(content);
    for namespace in &namespaces {
        if anchors.len() >= limit {
            return;
        }
        let location = locate_text(content, namespace);
        push_semantic_anchor(
            anchors,
            seen,
            file_path,
            "translation_namespace",
            namespace.clone(),
            location.line,
            location.character,
            format!("translation namespace {}", namespace),
            0.9,
            limit,
        );
    }

    for (line_index, line) in content.lines().enumerate() {
        let preview = line.trim().chars().take(240).collect::<String>();
        for (value, character) in extract_quoted_values(line) {
            let context = &line[..character.min(line.len())];
            let call_namespace =
                translation_alias_namespace_for_context(context, &alias_namespaces);
            if !is_translation_usage_literal_context(context) && call_namespace.is_none() {
                continue;
            }
            if !is_probable_translation_usage_key(&value) {
                continue;
            }
            push_semantic_anchor(
                anchors,
                seen,
                file_path,
                "translation_usage_key",
                value.clone(),
                line_index as u32,
                character as u32,
                preview.clone(),
                0.93,
                limit,
            );
            let namespace =
                call_namespace.or_else(|| (namespaces.len() == 1).then(|| namespaces[0].as_str()));
            if let Some(namespace) = namespace {
                let qualified = qualify_translation_key(namespace, &value);
                push_semantic_anchor(
                    anchors,
                    seen,
                    file_path,
                    "translation_usage_key",
                    qualified,
                    line_index as u32,
                    character as u32,
                    preview.clone(),
                    0.95,
                    limit,
                );
            }
            if anchors.len() >= limit {
                return;
            }
        }
    }
}

fn extract_translation_namespaces(content: &str) -> Vec<String> {
    let mut namespaces = Vec::new();
    for line in content.lines() {
        if !line.contains("useTranslations") && !line.contains("getTranslations") {
            continue;
        }
        for (value, _) in extract_quoted_values(line) {
            let Some(character) = line
                .find(&format!("\"{}\"", value))
                .or_else(|| line.find(&format!("'{}'", value)))
            else {
                continue;
            };
            let context = line[..character].trim_end();
            if !context.ends_with("useTranslations(") && !context.ends_with("getTranslations(") {
                continue;
            }
            if is_probable_translation_usage_key(&value) && !namespaces.contains(&value) {
                namespaces.push(value);
            }
            if namespaces.len() >= 4 {
                return namespaces;
            }
        }
    }
    namespaces
}

fn extract_translation_call_aliases(content: &str) -> HashSet<String> {
    extract_translation_call_alias_namespaces(content)
        .into_keys()
        .collect()
}

fn extract_translation_call_alias_namespaces(content: &str) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    for line in content.lines() {
        let Some(helper_index) = line
            .find("useTranslations(")
            .or_else(|| line.find("getTranslations("))
        else {
            continue;
        };
        let before_helper = &line[..helper_index];
        let Some((left, _)) = before_helper.rsplit_once('=') else {
            continue;
        };
        let Some(alias) = trailing_identifier(left) else {
            continue;
        };
        let Some(namespace) = extract_quoted_values(&line[helper_index..])
            .into_iter()
            .next()
            .map(|(value, _)| value)
        else {
            continue;
        };
        if is_probable_translation_usage_key(&namespace) {
            aliases.insert(alias, namespace);
        }
    }
    aliases
}

fn translation_alias_namespace_for_context<'a>(
    context: &str,
    aliases: &'a HashMap<String, String>,
) -> Option<&'a str> {
    let context = context.trim_end();
    aliases.iter().find_map(|(alias, namespace)| {
        context
            .ends_with(&format!("{}(", alias))
            .then_some(namespace.as_str())
    })
}

fn qualify_translation_key(namespace: &str, key: &str) -> String {
    if key == namespace || key.starts_with(&format!("{}.", namespace)) {
        key.to_string()
    } else {
        format!("{}.{}", namespace, key)
    }
}

fn trailing_identifier(text: &str) -> Option<String> {
    let mut end = None;
    let mut start = None;

    for (index, ch) in text.char_indices().rev() {
        if end.is_none() {
            if is_identifier_continue(ch) {
                end = Some(index + ch.len_utf8());
                start = Some(index);
            }
            continue;
        }

        if is_identifier_continue(ch) {
            start = Some(index);
        } else {
            break;
        }
    }

    let (Some(start), Some(end)) = (start, end) else {
        return None;
    };
    let identifier = &text[start..end];
    is_identifier_start(identifier.chars().next()?).then(|| identifier.to_string())
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphabetic()
}

fn is_identifier_continue(ch: char) -> bool {
    is_identifier_start(ch) || ch.is_ascii_digit()
}

fn locate_text(content: &str, needle: &str) -> AnchorLocation {
    for (line_index, line) in content.lines().enumerate() {
        if let Some(character) = line.find(needle) {
            return AnchorLocation {
                line: line_index as u32,
                character: character as u32,
            };
        }
    }
    AnchorLocation {
        line: 0,
        character: 0,
    }
}

fn is_translation_usage_literal_context(context: &str) -> bool {
    let context = context.trim_end();
    context.ends_with("t(")
        || context.ends_with(".t(")
        || context.ends_with("i18n.t(")
        || context.ends_with("formatMessage({ id:")
        || context.ends_with("formatMessage({id:")
        || context.ends_with("id:")
        || context.ends_with("i18nKey=")
        || context.ends_with("i18nKey =")
}

fn is_probable_translation_usage_key(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.len() < 2 || trimmed.len() > 160 || trimmed.contains(char::is_whitespace) {
        return false;
    }
    trimmed
        .split('.')
        .all(|segment| is_translation_key_segment(segment))
}

fn is_translation_key_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 80
        && segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        && segment.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn is_translation_text_value(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 500
        && !value.chars().any(|ch| ch.is_control())
        && value.chars().any(|ch| ch.is_alphabetic())
}

fn extract_quoted_values(line: &str) -> Vec<(String, usize)> {
    let mut values = Vec::new();
    let chars = line.char_indices().collect::<Vec<_>>();
    let mut index = 0usize;
    while index < chars.len() {
        let (start_byte, quote) = chars[index];
        if quote != '\'' && quote != '"' && quote != '`' {
            index += 1;
            continue;
        }
        let mut value = String::new();
        let mut escaped = false;
        let mut end_index = index + 1;
        while end_index < chars.len() {
            let (_, ch) = chars[end_index];
            if escaped {
                value.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                break;
            } else {
                value.push(ch);
            }
            end_index += 1;
        }
        if end_index < chars.len() && !value.is_empty() {
            values.push((value, start_byte));
        }
        index = end_index.saturating_add(1);
    }
    values
}

fn extract_css_tokens(line: &str) -> Vec<(String, usize)> {
    let mut values = Vec::new();
    let bytes = line.as_bytes();
    let mut index = 0usize;
    while index + 2 < bytes.len() {
        if bytes[index] == b'-' && bytes[index + 1] == b'-' {
            let start = index;
            index += 2;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric()
                    || bytes[index] == b'-'
                    || bytes[index] == b'_')
            {
                index += 1;
            }
            if index > start + 3 {
                values.push((line[start..index].to_string(), start));
            }
        } else {
            index += 1;
        }
    }
    values
}

fn extract_unquoted_keys(line: &str) -> Vec<(String, usize)> {
    // Comment prose like `// WHY: ...` is not an object/config key; rationale
    // extraction owns those lines, and heading/selector `#` lines carry no keys.
    let trimmed = line.trim_start();
    if trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with('#')
        || trimmed.starts_with("<!--")
    {
        return Vec::new();
    }
    let Some(colon_index) = line.find(':') else {
        return Vec::new();
    };
    let prefix = line[..colon_index].trim_end();
    let token_start = prefix
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| {
            (!ch.is_alphanumeric() && ch != '_' && ch != '-').then_some(idx + ch.len_utf8())
        })
        .unwrap_or(0);
    let value = prefix[token_start..].trim();
    if value.len() >= 3 && value.chars().any(|ch| ch.is_alphabetic()) {
        vec![(value.to_string(), token_start)]
    } else {
        Vec::new()
    }
}

fn is_semantic_anchor_value(value: &str) -> bool {
    if value.len() < 3 || value.len() > 160 || value.chars().any(|ch| ch.is_control()) {
        return false;
    }
    if value.split_whitespace().count() > 6 {
        return false;
    }
    value.chars().any(|ch| ch.is_alphabetic())
        && (value.contains('/')
            || value.contains('.')
            || value.contains('-')
            || value.contains('_')
            || value.contains(':')
            || value.contains("--")
            || value.chars().any(|ch| ch.is_uppercase()))
}

fn is_supported_index_file(file_path: &str) -> bool {
    if Language::capability_for_path(file_path).is_some() {
        return !is_known_non_config_resource_path(&file_path.replace('\\', "/").to_lowercase());
    }
    is_anchor_only_index_file(file_path)
}

/// M6.2 — leading bytes read to sniff a shebang line.
const SHEBANG_SNIFF_HEAD_BYTES: usize = 256;

/// M6.2 — an extensionless file is a supported script when its first line is a `#!`
/// shebang naming a known interpreter. Only extensionless, path-unclassifiable files
/// reach here (the caller checks `is_supported_index_file` first), so at most the
/// first 256 bytes of those files are read.
fn extensionless_shebang_is_supported(path: &Path, relative: &str) -> bool {
    if Path::new(relative).extension().is_some() {
        return false;
    }
    let Some(head) = read_file_leading_bytes(path, SHEBANG_SNIFF_HEAD_BYTES) else {
        return false;
    };
    let text = String::from_utf8_lossy(&head);
    Language::detect_by_shebang(&text).is_some()
}

fn read_file_leading_bytes(path: &Path, max_bytes: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut buffer = Vec::new();
    file.take(max_bytes as u64).read_to_end(&mut buffer).ok()?;
    Some(buffer)
}

fn is_anchor_only_index_file(file_path: &str) -> bool {
    Language::capability_for_path(file_path).is_none() && is_translation_resource_path(file_path)
}

fn is_rust_project_resolution_input(file_path: &str) -> bool {
    file_path.to_ascii_lowercase().ends_with(".rs")
        || Path::new(file_path)
            .file_name()
            .is_some_and(|name| name == "Cargo.toml")
}

fn is_translation_resource_path(file_path: &str) -> bool {
    let lower = file_path.replace('\\', "/").to_lowercase();
    let extension_supported =
        lower.ends_with(".json") || lower.ends_with(".yaml") || lower.ends_with(".yml");
    if !extension_supported || is_known_non_translation_resource_path(&lower) {
        return false;
    }

    let components = lower.split('/').collect::<Vec<_>>();
    let in_translation_dir = components.iter().any(|component| {
        matches!(
            *component,
            "i18n"
                | "intl"
                | "l10n"
                | "lang"
                | "langs"
                | "locale"
                | "locales"
                | "messages"
                | "translations"
                | "dictionaries"
                | "dictionary"
        )
    });
    let file_name = components.last().copied().unwrap_or_default();
    in_translation_dir
        || file_name.contains("translation")
        || file_name.contains("message")
        || is_locale_resource_file_name(file_name)
}

fn is_translation_usage_source_path(file_path: &str) -> bool {
    matches!(
        Language::from_path(file_path),
        Some(
            Language::TypeScript
                | Language::Tsx
                | Language::JavaScript
                | Language::Jsx
                | Language::Astro
                | Language::Python
        )
    )
}

fn is_known_non_translation_resource_path(lower_path: &str) -> bool {
    [
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "jsconfig.json",
        "composer.json",
        "deno.json",
        "deno.lock",
        "bun.lockb",
    ]
    .iter()
    .any(|suffix| lower_path.ends_with(suffix))
}

fn is_known_non_config_resource_path(lower_path: &str) -> bool {
    [
        "package-lock.json",
        "npm-shrinkwrap.json",
        "composer.lock",
        "deno.lock",
        "bun.lock",
        "bun.lockb",
        "pnpm-lock.yaml",
        "pnpm-lock.yml",
        "yarn.lock",
    ]
    .iter()
    .any(|suffix| lower_path.ends_with(suffix))
}

fn is_locale_resource_file_name(file_name: &str) -> bool {
    let stem = file_name
        .strip_suffix(".json")
        .or_else(|| file_name.strip_suffix(".yaml"))
        .or_else(|| file_name.strip_suffix(".yml"))
        .unwrap_or(file_name);
    let parts = stem
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    matches!(parts.as_slice(), [language] if is_locale_part(language, 2, 3))
        || matches!(parts.as_slice(), [language, region]
            if is_locale_part(language, 2, 3) && is_locale_part(region, 2, 4))
}

fn is_locale_part(part: &str, min_len: usize, max_len: usize) -> bool {
    (min_len..=max_len).contains(&part.len()) && part.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn semantic_anchor_kind(value: &str, line: &str) -> String {
    if value.starts_with("--") {
        "css_token".to_string()
    } else if value.starts_with('/') && !value.contains(char::is_whitespace) {
        "route".to_string()
    } else if value.contains('.') && value.split('.').all(is_anchor_segment) {
        "translation_key".to_string()
    } else if value.contains(':') && !value.contains(char::is_whitespace) {
        "protocol_tag".to_string()
    } else if line.contains("command") || line.contains("Command") {
        "command".to_string()
    } else if line.contains("event") || line.contains("Event") {
        "event_name".to_string()
    } else if line.contains("service") || line.contains("Service") {
        "service_name".to_string()
    } else if line.contains("config") || line.contains("Config") || line.contains('=') {
        "config_key".to_string()
    } else {
        "semantic_literal".to_string()
    }
}

fn is_anchor_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
}

fn semantic_anchor_confidence(value: &str, line: &str) -> f32 {
    if value.starts_with("--") || value.starts_with('/') {
        0.95
    } else if line.contains("command")
        || line.contains("Command")
        || line.contains("event")
        || line.contains("Event")
        || line.contains("service")
        || line.contains("Service")
    {
        0.9
    } else if value.contains('.') || value.contains(':') {
        0.85
    } else {
        0.75
    }
}

fn search_confidence(results: &[SearchResult]) -> String {
    let top_score = results.first().map(|result| result.score).unwrap_or(0.0);
    if results.is_empty() {
        "empty".to_string()
    } else if top_score >= 0.9 {
        "high".to_string()
    } else if top_score >= 0.55 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

#[derive(Debug, Clone)]
struct StylesheetClassUsage {
    selector: String,
    line: u32,
}

#[derive(Debug, Clone)]
struct StylesheetCustomPropertyUsage {
    name: String,
    line: u32,
}

fn stylesheet_module_aliases(content: &str) -> HashSet<String> {
    let mut aliases = HashSet::new();
    for line in content.lines() {
        if !line.contains(".module.") || !line.contains(" from ") {
            continue;
        }
        let trimmed = line.trim_start();
        let Some(import_tail) = trimmed.strip_prefix("import ") else {
            continue;
        };
        let Some((binding, _)) = import_tail.split_once(" from ") else {
            continue;
        };
        let binding = binding.trim();
        if let Some(alias) = binding.strip_prefix("* as ") {
            if let Some(alias) = css_module_alias_token(alias) {
                aliases.insert(alias.to_string());
            }
        } else if let Some(alias) =
            css_module_alias_token(binding.split(',').next().unwrap_or(binding))
        {
            aliases.insert(alias.to_string());
        }
    }
    aliases
}

fn css_module_alias_token(value: &str) -> Option<&str> {
    let token = value.trim();
    let end = token
        .char_indices()
        .find_map(|(index, ch)| {
            (!ch.is_ascii_alphanumeric() && ch != '_' && ch != '$').then_some(index)
        })
        .unwrap_or(token.len());
    let token = &token[..end];
    (!token.is_empty()).then_some(token)
}

fn stylesheet_class_usages(
    content: &str,
    css_module_aliases: &HashSet<String>,
) -> Vec<StylesheetClassUsage> {
    let mut usages = Vec::new();
    let mut seen = HashSet::<(String, u32)>::new();

    for (line_index, line) in content.lines().enumerate() {
        let line_number = line_index as u32;
        for class_name in class_name_literal_tokens(line) {
            let selector = format!(".{}", class_name.trim_start_matches('.'));
            if seen.insert((selector.clone(), line_number)) {
                usages.push(StylesheetClassUsage {
                    selector,
                    line: line_number,
                });
            }
        }

        for alias in css_module_aliases {
            for class_name in css_module_member_tokens(line, alias) {
                let selector = format!(".{}", class_name);
                if seen.insert((selector.clone(), line_number)) {
                    usages.push(StylesheetClassUsage {
                        selector,
                        line: line_number,
                    });
                }
            }
        }

        for class_name in class_composition_helper_tokens(line) {
            let selector = format!(".{}", class_name.trim_start_matches('.'));
            if seen.insert((selector.clone(), line_number)) {
                usages.push(StylesheetClassUsage {
                    selector,
                    line: line_number,
                });
            }
        }
    }

    usages
}

fn stylesheet_custom_property_usages(content: &str) -> Vec<StylesheetCustomPropertyUsage> {
    let mut usages = Vec::new();
    let mut seen = HashSet::<(String, u32)>::new();

    for (line_index, line) in content.lines().enumerate() {
        let line_number = line_index as u32;
        for name in css_var_function_tokens(line) {
            if seen.insert((name.clone(), line_number)) {
                usages.push(StylesheetCustomPropertyUsage {
                    name,
                    line: line_number,
                });
            }
        }
    }

    usages
}

fn css_var_function_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut search_start = 0usize;

    while let Some(relative_index) = line[search_start..].find("var(") {
        let value_start = search_start + relative_index + "var(".len();
        let Some(value_end_relative) = line[value_start..].find(')') else {
            break;
        };
        let value = &line[value_start..value_start + value_end_relative];
        let token = value
            .split(',')
            .next()
            .unwrap_or_default()
            .trim()
            .trim_matches(['\'', '"']);
        if is_css_custom_property_token(token) {
            tokens.push(token.to_string());
        }
        search_start = value_start + value_end_relative + 1;
    }

    tokens
}

fn class_name_literal_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut search_start = 0usize;
    while let Some(relative_index) = line[search_start..].find("className") {
        let class_name_index = search_start + relative_index;
        let Some(equals_relative) = line[class_name_index..].find('=') else {
            break;
        };
        let after_equals = class_name_index + equals_relative + 1;
        let Some((quote_index, quote)) = first_quote_after(&line[after_equals..]) else {
            search_start = after_equals;
            continue;
        };
        let value_start = after_equals + quote_index + quote.len_utf8();
        let Some(value_end_relative) = line[value_start..].find(quote) else {
            search_start = value_start;
            continue;
        };
        let value = &line[value_start..value_start + value_end_relative];
        for token in value
            .split_whitespace()
            .filter(|token| is_css_class_token(token))
        {
            tokens.push(token.trim_start_matches('.').to_string());
        }
        search_start = value_start + value_end_relative + quote.len_utf8();
    }
    tokens
}

fn first_quote_after(value: &str) -> Option<(usize, char)> {
    value
        .char_indices()
        .find_map(|(index, ch)| (ch == '\'' || ch == '"' || ch == '`').then_some((index, ch)))
}

fn class_composition_helper_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for helper in ["clsx", "classNames", "cn"] {
        let call_prefix = format!("{helper}(");
        let mut search_start = 0usize;
        while let Some(relative_index) = line[search_start..].find(&call_prefix) {
            let args_start = search_start + relative_index + call_prefix.len();
            let args_end = matching_call_end(line, args_start).unwrap_or(line.len());
            let args = &line[args_start..args_end];
            for value in quoted_values(args) {
                for token in value
                    .split_whitespace()
                    .filter(|token| is_css_class_token(token))
                {
                    tokens.push(token.trim_start_matches('.').to_string());
                }
            }
            tokens.extend(class_composition_object_key_tokens(args));
            search_start = args_end;
        }
    }
    tokens
}

fn matching_call_end(line: &str, args_start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut quote: Option<char> = None;

    for (relative_index, ch) in line[args_start..].char_indices() {
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(args_start + relative_index);
                }
            }
            _ => {}
        }
    }

    None
}

fn quoted_values(value: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut remainder = value;

    while let Some((quote_index, quote)) = first_quote_after(remainder) {
        let value_start = quote_index + quote.len_utf8();
        let Some(value_end_relative) = remainder[value_start..].find(quote) else {
            break;
        };
        values.push(&remainder[value_start..value_start + value_end_relative]);
        remainder = &remainder[value_start + value_end_relative + quote.len_utf8()..];
    }

    values
}

fn class_composition_object_key_tokens(args: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut search_start = 0usize;

    while let Some(open_relative) = args[search_start..].find('{') {
        let object_start = search_start + open_relative + 1;
        let Some(close_relative) = args[object_start..].find('}') else {
            break;
        };
        let object = &args[object_start..object_start + close_relative];
        for entry in object.split(',') {
            let Some((key, _)) = entry.split_once(':') else {
                continue;
            };
            let key = key.trim().trim_matches(['\'', '"', '`']);
            if is_css_class_token(key) {
                tokens.push(key.trim_start_matches('.').to_string());
            }
        }
        search_start = object_start + close_relative + 1;
    }

    tokens
}

fn css_module_member_tokens(line: &str, alias: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let dot_prefix = format!("{alias}.");
    let mut search_start = 0usize;
    while let Some(relative_index) = line[search_start..].find(&dot_prefix) {
        let token_start = search_start + relative_index + dot_prefix.len();
        let token_end = token_start
            + line[token_start..]
                .char_indices()
                .find_map(|(index, ch)| (!is_css_identifier_char(ch)).then_some(index))
                .unwrap_or(line[token_start..].len());
        let token = &line[token_start..token_end];
        if is_css_class_token(token) {
            tokens.push(token.to_string());
        }
        search_start = token_end;
    }

    for quote in ['\'', '"'] {
        let bracket_prefix = format!("{alias}[{quote}");
        let mut search_start = 0usize;
        while let Some(relative_index) = line[search_start..].find(&bracket_prefix) {
            let token_start = search_start + relative_index + bracket_prefix.len();
            let Some(token_end_relative) = line[token_start..].find(quote) else {
                break;
            };
            let token = &line[token_start..token_start + token_end_relative];
            if is_css_class_token(token) {
                tokens.push(token.to_string());
            }
            search_start = token_start + token_end_relative + quote.len_utf8();
        }
    }

    tokens
}

fn is_css_class_token(token: &str) -> bool {
    let token = token.trim().trim_start_matches('.');
    !token.is_empty()
        && token.chars().any(|ch| ch.is_ascii_alphabetic())
        && token.chars().all(is_css_identifier_char)
        && !token.contains("${")
}

fn is_css_custom_property_token(token: &str) -> bool {
    let Some(name) = token.strip_prefix("--") else {
        return false;
    };
    name.len() >= 2
        && name.chars().any(|ch| ch.is_ascii_alphabetic())
        && name.chars().all(is_css_identifier_char)
}

fn is_css_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

/// Auditable outcome of resolving one relationship target. Every successful
/// resolver path records its strategy and confidence so downstream graph tools
/// can distinguish syntax evidence from target-identity certainty.
struct ResolvedTarget {
    id: String,
    strategy: Option<&'static str>,
    confidence: Option<f32>,
}

impl ResolvedTarget {
    fn same_file(id: String) -> Self {
        Self {
            id,
            strategy: Some("same_file_unique"),
            confidence: Some(1.0),
        }
    }

    fn imported(id: String) -> Self {
        Self {
            id,
            strategy: Some("imported_unique"),
            confidence: Some(0.95),
        }
    }

    fn receiver(id: String) -> Self {
        Self {
            id,
            // Confidence above `global_unique`'s 0.5: a scope/type-derived match
            // is stronger than a name-only global-uniqueness guess.
            strategy: Some("receiver_type"),
            confidence: Some(0.8),
        }
    }
}

/// M5.1 same-file inheritance index: `subtype simple-name → direct supertype
/// simple-names`, built once per file from its own `Extends`/`Implements` edges.
/// Used ONLY to widen a receiver type to the set of class names whose method a
/// receiver of that type could be calling (a method may be defined on a
/// supertype). Cross-file supertype chains beyond this file's edges are not
/// followed (bounded slice); an unknown chain simply yields the singleton set, so
/// disambiguation falls through — never a regression.
struct ReceiverTypeIndex {
    supertypes: HashMap<String, Vec<String>>,
}

impl ReceiverTypeIndex {
    fn from_file(file_symbols: &[Symbol], relationships: &[SymbolRelationship]) -> Self {
        let id_to_name: HashMap<&str, &str> = file_symbols
            .iter()
            .map(|symbol| (symbol.id.as_str(), symbol.name.as_str()))
            .collect();
        let mut supertypes: HashMap<String, Vec<String>> = HashMap::new();
        for relationship in relationships {
            if !matches!(
                relationship.relationship_type,
                SymbolRelationshipType::Extends | SymbolRelationshipType::Implements
            ) {
                continue;
            }
            let Some(sub_name) = id_to_name.get(relationship.source_symbol_id.as_str()) else {
                continue;
            };
            let sub = simple_type_name(sub_name);
            let sup = simple_type_name(&relationship.target_name);
            if sub == sup {
                continue;
            }
            supertypes.entry(sub).or_default().push(sup);
        }
        Self { supertypes }
    }

    /// The receiver type plus all transitive supertypes (simple names), with a
    /// hard iteration cap and a visited-set cycle guard.
    fn supertype_closure(&self, recv_type: &str) -> HashSet<String> {
        let start = simple_type_name(recv_type);
        let mut result = HashSet::new();
        result.insert(start.clone());
        let mut stack = vec![start];
        let mut steps = 0usize;
        while let Some(current) = stack.pop() {
            steps += 1;
            if steps > 256 {
                break;
            }
            if let Some(supers) = self.supertypes.get(&current) {
                for supertype in supers {
                    if result.insert(supertype.clone()) {
                        stack.push(supertype.clone());
                    }
                }
            }
        }
        result
    }
}

/// M5.1 (strict superset): narrow an EXISTING ambiguous candidate set to those
/// whose parent class (or a supertype of the receiver) equals `recv_type`. Returns
/// `Some(id)` ONLY when EXACTLY ONE candidate survives — and that id is always one
/// of the input candidates. 0 or >1 survivors → `None` (today's behavior).
fn disambiguate_by_receiver(
    candidates: &[Symbol],
    recv_type: &str,
    index: &ReceiverTypeIndex,
) -> Option<String> {
    let allowed = index.supertype_closure(recv_type);
    let mut matched = candidates.iter().filter(|candidate| {
        candidate_parent_class_name(candidate)
            .map(|parent| allowed.contains(&parent))
            .unwrap_or(false)
    });
    let first = matched.next()?;
    if matched.next().is_some() {
        // More than one candidate matched the receiver type → still ambiguous.
        return None;
    }
    Some(first.id.clone())
}

/// The simple (last-segment) name of the class a method candidate belongs to,
/// derived from its qualified name by dropping the final `.`/`::` member segment.
/// `A.run`/`A::run`/`mod.A.run` → `A`; a non-member name (`run`) → `None` (a free
/// function has no receiver class, so it never matches a typed receiver).
fn candidate_parent_class_name(symbol: &Symbol) -> Option<String> {
    let normalized = symbol.qualified_name.replace("::", ".");
    let segments: Vec<&str> = normalized.split('.').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }
    Some(segments[segments.len() - 2].to_string())
}

/// The simple (last-segment) name of a possibly-qualified type name.
/// `mod::Foo`/`pkg.Foo`/`Foo` → `Foo`.
fn simple_type_name(name: &str) -> String {
    let normalized = name.replace("::", ".");
    normalized
        .rsplit('.')
        .find(|s| !s.is_empty())
        .unwrap_or(name)
        .to_string()
}

struct SymbolIdentityResolver<'a> {
    symbols: &'a [Symbol],
}

impl<'a> SymbolIdentityResolver<'a> {
    fn new(symbols: &'a [Symbol]) -> Self {
        Self { symbols }
    }

    fn file_root(&self) -> Option<&'a Symbol> {
        self.symbols
            .iter()
            .find(|symbol| LanguageService::is_synthetic_file_root_symbol(symbol))
    }

    fn source_for_usage_line(&self, line: u32) -> Option<&'a Symbol> {
        self.symbols
            .iter()
            .filter(|symbol| {
                symbol.symbol_type != SymbolType::Import
                    && !LanguageService::is_synthetic_file_root_symbol(symbol)
                    && symbol.range.start.line <= line
                    && symbol.range.end.line >= line
            })
            .min_by_key(|symbol| {
                (
                    symbol
                        .range
                        .end
                        .line
                        .saturating_sub(symbol.range.start.line),
                    symbol.range.start.character,
                )
            })
            .or_else(|| self.file_root())
    }

    fn stylesheet_source_for_custom_property_usage(
        &self,
        line: u32,
        target_name: &str,
    ) -> Option<&'a Symbol> {
        if let Some(symbol) = self.symbols.iter().find(|symbol| {
            symbol.symbol_type == SymbolType::CssCustomProperty
                && symbol.name != target_name
                && symbol.range.start.line == line
        }) {
            return Some(symbol);
        }

        self.symbols
            .iter()
            .filter(|symbol| {
                matches!(
                    symbol.symbol_type,
                    SymbolType::CssSelector
                        | SymbolType::CssKeyframes
                        | SymbolType::CssAtRule
                        | SymbolType::CssLayer
                        | SymbolType::CssFontFace
                ) && symbol.range.start.line <= line
            })
            .max_by_key(|symbol| (symbol.range.start.line, symbol.range.start.character))
            .or_else(|| self.file_root())
    }

    fn collect_matching_named_symbols(
        &self,
        reference_name: &str,
        resolved: &mut Vec<Symbol>,
        seen: &mut HashSet<String>,
    ) {
        for symbol in self.symbols {
            if symbol.name != reference_name
                || symbol.symbol_type == SymbolType::Import
                || LanguageService::is_synthetic_file_root_symbol(symbol)
            {
                continue;
            }

            if seen.insert(symbol.id.clone()) {
                resolved.push(symbol.clone());
            }
        }
    }
}

fn compute_hash(content: &str) -> String {
    // M6 CL1 — version-stable across toolchains (was SipHash `DefaultHasher`).
    crate::stable_hash::stable_hash_hex(content.as_bytes())
}

fn source_line_count(content: &str) -> usize {
    content.lines().count()
}

fn language_counts_for_paths(paths: &[String]) -> Vec<IndexLanguageCount> {
    let mut counts = BTreeMap::<String, usize>::new();
    for path in paths {
        let language = Language::capability_for_path(path)
            .map(|capability| capability.display_name)
            .unwrap_or("Anchor-only")
            .to_string();
        *counts.entry(language).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(language, count)| IndexLanguageCount { language, count })
        .collect()
}

fn support_level_label(level: crate::tree_sitter::SupportLevel) -> &'static str {
    match level {
        crate::tree_sitter::SupportLevel::Full => "full",
        crate::tree_sitter::SupportLevel::Partial => "partial",
    }
}

fn index_schema_root_totals(
    indexed_files: usize,
    symbol_store: &SymbolStore,
) -> Result<IndexSchemaTotals, LanguageError> {
    let symbols = symbol_store
        .symbol_type_counts()?
        .into_iter()
        .map(|(_, count)| count)
        .sum();
    let relationships = symbol_store
        .relationship_integrity_stats()?
        .total_relationships;
    let semantic_anchors = symbol_store
        .semantic_anchor_kind_counts()?
        .into_iter()
        .map(|(_, count)| count)
        .sum();

    Ok(IndexSchemaTotals {
        indexed_files,
        symbols,
        relationships,
        semantic_anchors,
    })
}

fn normalize_schema_scope_path(path: &str) -> Option<String> {
    let mut normalized = path.trim().replace('\\', "/");
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped.to_string();
    }
    while let Some(stripped) = normalized.strip_prefix('/') {
        normalized = stripped.to_string();
    }

    let mut collapsed = String::with_capacity(normalized.len());
    let mut previous_was_slash = false;
    for ch in normalized.chars() {
        if ch == '/' {
            if !previous_was_slash {
                collapsed.push(ch);
            }
            previous_was_slash = true;
        } else {
            collapsed.push(ch);
            previous_was_slash = false;
        }
    }

    let normalized = collapsed.trim_end_matches('/').trim().to_string();
    (!normalized.is_empty()).then_some(normalized)
}

fn schema_path_matches_scope(file_path: &str, scope: &str) -> bool {
    let Some(file_path) = normalize_schema_scope_path(file_path) else {
        return false;
    };
    file_path == scope
        || file_path
            .strip_prefix(scope)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn skip_counts_from_map(counts: &BTreeMap<String, usize>) -> Vec<IndexSkipCount> {
    counts
        .iter()
        .map(|(reason, count)| IndexSkipCount {
            reason: reason.clone(),
            count: *count,
        })
        .collect()
}

fn file_index_metadata(path: &Path) -> std::io::Result<FileIndexMetadata> {
    let metadata = std::fs::metadata(path)?;
    Ok(FileIndexMetadata {
        file_size: metadata.len(),
        modified_at: metadata_modified_at(&metadata),
    })
}

fn metadata_modified_at(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn parse_with_thread_local_parser(
    content: &str,
    language: Language,
) -> Result<tree_sitter::Tree, LanguageError> {
    INDEXING_PARSER.with(|slot| {
        let mut parser = slot.borrow_mut();
        if parser.is_none() {
            *parser = Some(
                TreeSitterParser::new().map_err(|error| LanguageError::Parse(error.to_string()))?,
            );
        }
        parser
            .as_mut()
            .expect("thread-local parser initialized")
            .parse(content, language)
            .map_err(|error| LanguageError::Parse(error.to_string()))
    })
}

fn indexing_worker_count(total_queued: usize) -> usize {
    let cpu_count = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4);
    // M5.4 — saturate the machine for the CPU-bound extraction pass. The old hard
    // cap of 8 left most of a modern many-core CPU idle (e.g. 24 of 32 threads on
    // a Ryzen 5950X). Reserve ONE core for the main/drain thread so the channel
    // keeps draining and the UI stays responsive while indexing. The per-batch
    // byte budget (BATCH_BYTE_BUDGET) bounds peak memory regardless of worker
    // count, and oversized files are anchor-only, so more workers does not blow
    // the transient-AST footprint. Still bounded by the batch size.
    //
    // `ZBLADE_INDEX_WORKERS` overrides the pool size (benchmarking / tuning on
    // memory-constrained machines); a value of 0 or an unparseable value falls
    // back to the auto default.
    let workers = std::env::var("ZBLADE_INDEX_WORKERS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|count| *count > 0)
        .unwrap_or_else(|| cpu_count.saturating_sub(1).max(1));
    total_queued.min(workers).max(1)
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

fn extract_css_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();
    let mut byte_offset = 0usize;
    let mut pending_selector = String::new();
    let mut pending_selector_start_line = 0u32;
    let mut pending_selector_start_byte = 0usize;
    let mut brace_depth = 0i32;
    let mut font_face_depth = 0i32;
    let sass_indented = file_path.to_ascii_lowercase().ends_with(".sass");

    // Comment/string-blanked structural view (byte-aligned with `content`): used
    // only for brace counting and selector boundary detection, so a `{`/`}`
    // hiding inside a `content: "}"` value or a `/* */` block is not miscounted.
    // Symbol names are still read from the original line.
    let blanked = blank_noncode_spans(content, &CSS_LEX);

    for (line_index, (segment, blanked_segment)) in content
        .split_inclusive('\n')
        .zip(blanked.split_inclusive('\n'))
        .enumerate()
    {
        let line_start = byte_offset;
        byte_offset += segment.len();

        let line_without_lf = segment.strip_suffix('\n').unwrap_or(segment);
        let line = line_without_lf
            .strip_suffix('\r')
            .unwrap_or(line_without_lf);
        let blanked_without_lf = blanked_segment
            .strip_suffix('\n')
            .unwrap_or(blanked_segment);
        let line_code = blanked_without_lf
            .strip_suffix('\r')
            .unwrap_or(blanked_without_lf);
        let line_number = line_index as u32;

        for (name, start_char, end_char) in css_custom_properties_in_line(line) {
            push_css_symbol(
                &mut symbols,
                &mut seen,
                file_path,
                name,
                SymbolType::CssCustomProperty,
                line_number,
                start_char,
                end_char,
                line_start,
                line,
            );
        }

        if let Some((name, start_char, end_char)) = css_keyframes_in_line(line) {
            push_css_symbol(
                &mut symbols,
                &mut seen,
                file_path,
                name,
                SymbolType::CssKeyframes,
                line_number,
                start_char,
                end_char,
                line_start,
                line,
            );
        }

        if let Some((name, start_char, end_char)) = css_layer_in_line(line) {
            push_css_symbol(
                &mut symbols,
                &mut seen,
                file_path,
                name,
                SymbolType::CssLayer,
                line_number,
                start_char,
                end_char,
                line_start,
                line,
            );
        }

        if let Some((name, start_char, end_char)) = css_at_rule_anchor_in_line(line) {
            push_css_symbol(
                &mut symbols,
                &mut seen,
                file_path,
                name,
                SymbolType::CssAtRule,
                line_number,
                start_char,
                end_char,
                line_start,
                line,
            );
        }

        let starts_font_face = css_font_face_starts_in_line(line);
        if starts_font_face || font_face_depth > 0 {
            if let Some((name, start_char, end_char)) = css_font_family_in_line(line) {
                push_css_symbol(
                    &mut symbols,
                    &mut seen,
                    file_path,
                    name,
                    SymbolType::CssFontFace,
                    line_number,
                    start_char,
                    end_char,
                    line_start,
                    line,
                );
            }
        }
        if starts_font_face {
            font_face_depth = css_next_brace_depth(0, line_code);
        } else if font_face_depth > 0 {
            font_face_depth = css_next_brace_depth(font_face_depth, line_code);
        }

        let trimmed = line_code.trim();
        if trimmed.is_empty() || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            brace_depth = css_next_brace_depth(brace_depth, line_code);
            continue;
        }

        if sass_indented && (trimmed.starts_with('.') || trimmed.starts_with('#')) {
            let indent_chars = line_code.len().saturating_sub(line_code.trim_start().len());
            for (name, start_char, end_char) in css_selectors_in_text(trimmed) {
                push_css_symbol(
                    &mut symbols,
                    &mut seen,
                    file_path,
                    name,
                    SymbolType::CssSelector,
                    line_number,
                    indent_chars.saturating_add(start_char),
                    indent_chars.saturating_add(end_char),
                    line_start,
                    line,
                );
            }
            continue;
        }

        if brace_depth == 0 {
            if pending_selector.is_empty() {
                pending_selector_start_line = line_number;
                pending_selector_start_byte = line_start;
            } else {
                pending_selector.push(' ');
            }
            pending_selector.push_str(trimmed);

            if let Some(open_brace_index) = pending_selector.find('{') {
                let selector_text = pending_selector[..open_brace_index].to_string();
                for (name, start_char, end_char) in css_selectors_in_text(&selector_text) {
                    push_css_symbol(
                        &mut symbols,
                        &mut seen,
                        file_path,
                        name,
                        SymbolType::CssSelector,
                        pending_selector_start_line,
                        start_char,
                        end_char,
                        pending_selector_start_byte,
                        &selector_text,
                    );
                }
                pending_selector.clear();
            }
        }

        brace_depth = css_next_brace_depth(brace_depth, line_code);
    }

    symbols
}

fn extract_markup_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();
    let mut byte_offset = 0usize;

    for (line_index, segment) in content.split_inclusive('\n').enumerate() {
        let line_start = byte_offset;
        byte_offset += segment.len();

        let line_without_lf = segment.strip_suffix('\n').unwrap_or(segment);
        let line = line_without_lf
            .strip_suffix('\r')
            .unwrap_or(line_without_lf);
        let line_number = line_index as u32;

        for (name, start_char, end_char) in markup_selector_attributes_in_line(line, "class", '.') {
            push_css_symbol(
                &mut symbols,
                &mut seen,
                file_path,
                name,
                SymbolType::CssSelector,
                line_number,
                start_char,
                end_char,
                line_start,
                line,
            );
        }

        for (name, start_char, end_char) in markup_selector_attributes_in_line(line, "id", '#') {
            push_css_symbol(
                &mut symbols,
                &mut seen,
                file_path,
                name,
                SymbolType::CssSelector,
                line_number,
                start_char,
                end_char,
                line_start,
                line,
            );
        }
    }

    symbols
}

fn markup_selector_attributes_in_line(
    line: &str,
    attribute: &str,
    selector_prefix: char,
) -> Vec<(String, usize, usize)> {
    let mut values = Vec::new();
    let mut search_start = 0usize;

    while let Some(relative_index) = line[search_start..].find(attribute) {
        let attribute_start = search_start + relative_index;
        let attribute_end = attribute_start + attribute.len();

        if !is_markup_attribute_boundary(line, attribute_start, attribute_end) {
            search_start = attribute_end;
            continue;
        }

        let mut cursor = attribute_end;
        while cursor < line.len() && line.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= line.len() || line.as_bytes()[cursor] != b'=' {
            search_start = attribute_end;
            continue;
        }
        cursor += 1;
        while cursor < line.len() && line.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= line.len() {
            break;
        }

        let quote = line.as_bytes()[cursor];
        if quote != b'\'' && quote != b'"' {
            search_start = cursor + 1;
            continue;
        }
        let value_start = cursor + 1;
        let Some(value_end_relative) = line[value_start..].find(quote as char) else {
            break;
        };
        let value_end = value_start + value_end_relative;
        values.extend(markup_selector_tokens(
            &line[value_start..value_end],
            value_start,
            selector_prefix,
        ));
        search_start = value_end + 1;
    }

    values
}

fn is_markup_attribute_boundary(line: &str, start: usize, end: usize) -> bool {
    let before_valid = start == 0
        || !line.as_bytes()[start - 1].is_ascii_alphanumeric()
            && line.as_bytes()[start - 1] != b'-'
            && line.as_bytes()[start - 1] != b'_';
    let after_valid = end >= line.len()
        || line.as_bytes()[end].is_ascii_whitespace()
        || line.as_bytes()[end] == b'=';
    before_valid && after_valid
}

fn markup_selector_tokens(
    value: &str,
    value_start: usize,
    selector_prefix: char,
) -> Vec<(String, usize, usize)> {
    let mut tokens = Vec::new();
    let mut token_start = None;

    for (index, ch) in value.char_indices() {
        if ch.is_whitespace() {
            if let Some(start) = token_start.take() {
                push_markup_selector_token(
                    &mut tokens,
                    value,
                    value_start,
                    selector_prefix,
                    start,
                    index,
                );
            }
        } else if token_start.is_none() {
            token_start = Some(index);
        }
    }

    if let Some(start) = token_start {
        push_markup_selector_token(
            &mut tokens,
            value,
            value_start,
            selector_prefix,
            start,
            value.len(),
        );
    }

    tokens
}

fn push_markup_selector_token(
    tokens: &mut Vec<(String, usize, usize)>,
    value: &str,
    value_start: usize,
    selector_prefix: char,
    start: usize,
    end: usize,
) {
    let token = &value[start..end];
    if is_css_class_token(token) {
        tokens.push((
            format!("{selector_prefix}{}", token.trim_start_matches(['.', '#'])),
            value_start + start,
            value_start + end,
        ));
    }
}

#[derive(Debug)]
struct ConfigKeyEntry {
    key_path: String,
    leaf_key: String,
    /// Exact (line, char) of the key when a span-preserving parser resolved it
    /// (M4.3: YAML via `marked-yaml`, TOML via the line scan). `None` falls back
    /// to the legacy `locate_config_key` re-find — retained only for JSON, whose
    /// span-accuracy is deferred (§13: no clean spanned JSON reader without
    /// another dependency).
    position: Option<(u32, u32)>,
}

const CONFIG_SYMBOL_LIMIT: usize = 2048;

fn extract_config_symbols(file_path: &str, content: &str, language: Language) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut entries = Vec::with_capacity(CONFIG_SYMBOL_LIMIT.min(256));
    match language {
        Language::Json => {
            if let Some(value) = parse_json_config_value(file_path, content) {
                if is_package_json_path(file_path) {
                    collect_package_json_config_keys(&value, &mut entries);
                } else if is_tsconfig_json_path(file_path) {
                    collect_tsconfig_json_config_keys(&value, &mut entries);
                } else {
                    collect_json_config_keys(&value, &mut Vec::new(), &mut entries);
                }
            }
        }
        Language::Yaml => {
            // M4.3 PART 1/2 — Kubernetes manifests collapse to a single `Resource`
            // node ("<kind>/<metadata.name>") instead of a bag of repeated
            // spec/metadata keys; Kustomize overlays expand to `Import`s. (A
            // `kustomization.yaml` also carries apiVersion/kind but is handled as
            // imports below, not a Resource.)
            if is_kustomization_path(file_path) {
                // M4.3 PART 2 — Kustomize overlays emit an Import per entry in the
                // `resources`/`bases`/`components` lists (target = the path), PLUS
                // their flat config keys (apiVersion/kind/namespace, …).
                symbols.extend(collect_kustomize_import_symbols(file_path, content));
                collect_yaml_config_entries(file_path, content, &mut entries);
            } else {
                let resources = collect_k8s_resource_symbols(file_path, content);
                if resources.is_empty() {
                    // No manifest docs — span-accurate flat config keys over the
                    // whole file. Covers a single top-level mapping AND, via the
                    // serde fallback inside `collect_yaml_config_entries`, a
                    // top-level sequence/scalar that `marked-yaml`'s mapping-root
                    // loader rejects (BUG 1 regression fix: those keys must still
                    // be extracted).
                    collect_yaml_config_entries(file_path, content, &mut entries);
                } else {
                    // BUG 2 — a multi-document file with at least one manifest doc:
                    // emit a `Resource` per manifest doc AND still extract the flat
                    // config keys from the NON-manifest docs (a pure-manifest file
                    // adds no extra keys). Positions for those keys are best-effort
                    // (`locate_config_key`): a whole-file `marked-yaml` parse only
                    // spans the first document, so per-doc spans aren't recoverable.
                    symbols.extend(resources);
                    collect_non_manifest_doc_config_keys(content, &mut entries);
                }
            }
        }
        Language::Toml => {
            collect_toml_config_keys(content, &mut entries);
        }
        _ => {}
    }

    let mut seen = HashSet::new();
    let lines = content.lines().collect::<Vec<_>>();
    let line_start_offsets = line_start_offsets(content);
    for entry in entries {
        // Span-accurate position when a span-preserving parser resolved it
        // (YAML/TOML); JSON still falls back to the legacy re-find (§13 deferral).
        let (line, character) = entry.position.unwrap_or_else(|| {
            let location = locate_config_key(content, &entry.key_path, &entry.leaf_key);
            (location.line, location.character)
        });
        let line_text = lines.get(line as usize).copied().unwrap_or_default();
        let line_start_byte = line_start_offsets.get(line as usize).copied().unwrap_or(0);
        push_config_symbol(
            &mut symbols,
            &mut seen,
            file_path,
            &entry.key_path,
            line,
            character as usize,
            line_start_byte,
            line_text,
        );
    }
    symbols
}

/// M4.3 PART 3 — span-accurate flat config keys for a (single-document) YAML
/// file, appended to `entries`.
///
/// Parses with `marked-yaml` (per-node line/col, duplicate-key tolerant),
/// converts to a `serde_yaml::Value` so the existing collectors produce the
/// exact same key set, then resolves each newly-added key's real (line, col)
/// from the marked tree — replacing the wrong-line `locate_config_key` re-find.
///
/// BUG 1 regression fix: `marked_yaml::parse_yaml` defaults to a mapping root and
/// returns `Err` for a top-level SEQUENCE or scalar (e.g. an Ansible playbook or
/// any top-level-list `.yml`). The pre-M4.3 path used `serde_yaml::from_str` and
/// still extracted those keys, so on that error we FALL BACK to the same
/// serde-based extraction — the keys are preserved (positions degrade to the
/// legacy `locate_config_key` re-find, acceptable for non-mapping roots).
fn collect_yaml_config_entries(file_path: &str, content: &str, entries: &mut Vec<ConfigKeyEntry>) {
    let start = entries.len();
    match marked_yaml::parse_yaml(0usize, content) {
        Ok(root) => {
            let value = marked_yaml_to_serde(&root);
            dispatch_yaml_config_collector(file_path, &value, entries);
            let mut positions = HashMap::new();
            collect_marked_yaml_key_positions(&root, &mut Vec::new(), &mut positions);
            for entry in &mut entries[start..] {
                if let Some(position) = positions
                    .get_mut(&entry.key_path)
                    .and_then(VecDeque::pop_front)
                {
                    entry.position = Some(position);
                }
            }
        }
        Err(_) => {
            // Non-mapping root: marked-yaml rejects it. Preserve the key set via
            // the legacy serde path (positions left `None` → `locate_config_key`).
            if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(content) {
                dispatch_yaml_config_collector(file_path, &value, entries);
            }
        }
    }
}

/// Route a parsed YAML value to the right flat-key collector (GitHub Actions /
/// Docker Compose specializations, else the generic walk). Shared by the
/// span-accurate path and the BUG 1 serde fallback so both produce one key set.
fn dispatch_yaml_config_collector(
    file_path: &str,
    value: &serde_yaml::Value,
    entries: &mut Vec<ConfigKeyEntry>,
) {
    if is_github_actions_workflow_path(file_path) {
        collect_github_actions_workflow_config_keys(value, entries);
    } else if is_docker_compose_path(file_path) {
        collect_docker_compose_config_keys(value, entries);
    } else {
        collect_yaml_config_keys(value, &mut Vec::new(), entries);
    }
}

/// BUG 2 — for a multi-document YAML file that contains at least one Kubernetes
/// manifest doc (each already represented by a `Resource` symbol), extract flat
/// config keys from the NON-manifest documents. Positions are best-effort (left
/// `None` → `locate_config_key`), since a whole-file `marked-yaml` parse only
/// covers the first document.
fn collect_non_manifest_doc_config_keys(content: &str, entries: &mut Vec<ConfigKeyEntry>) {
    for document in serde_yaml::Deserializer::from_str(content) {
        let Ok(value) = serde_yaml::Value::deserialize(document) else {
            // M5.12 — STOP on a parse error, never `continue`. `serde_yaml`'s
            // Deserializer does NOT advance past a syntax error: it yields the same
            // `Err` on every poll, so `continue` here is an infinite loop. (Firefox's
            // `StaticPrefList.yaml` has `value: @IS_XP_MACOSX@`; `@` is a reserved
            // YAML indicator → a hard parse error → a 100% CPU spin.) Once the
            // document stream errors, no later document is recoverable, so break.
            break;
        };
        if yaml_value_is_manifest(&value) {
            continue;
        }
        collect_yaml_config_keys(&value, &mut Vec::new(), entries);
    }
}

/// A YAML document is a Kubernetes manifest when its top-level mapping carries
/// BOTH a non-empty `apiVersion` and `kind` (the same test used to mint a
/// `Resource` symbol in `collect_k8s_resource_symbols`).
fn yaml_value_is_manifest(value: &serde_yaml::Value) -> bool {
    let Some(map) = value.as_mapping() else {
        return false;
    };
    let api_version = yaml_mapping_get(map, "apiVersion")
        .and_then(serde_yaml::Value::as_str)
        .map(str::trim);
    let kind = yaml_mapping_get(map, "kind")
        .and_then(serde_yaml::Value::as_str)
        .map(str::trim);
    matches!((api_version, kind), (Some(api), Some(kind)) if !api.is_empty() && !kind.is_empty())
}

/// M4.3 PART 1 — emit one `Resource` symbol per Kubernetes manifest document.
///
/// A document is a manifest when its top-level mapping has BOTH `apiVersion`
/// and `kind`. The symbol is named `"<kind>/<metadata.name>"` (falling back to
/// `"<kind>/<unnamed>"` when `metadata.name` is absent). Multi-document files
/// (`---` separated) yield one `Resource` per manifest doc. Spans are not
/// required for resources, so each is anchored at the file start (line 0).
fn collect_k8s_resource_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();
    for document in serde_yaml::Deserializer::from_str(content) {
        let Ok(value) = serde_yaml::Value::deserialize(document) else {
            // M5.12 — break, never `continue`: a `serde_yaml` parse error is sticky
            // (the Deserializer re-yields the same `Err` forever), so `continue`
            // spins at 100% CPU. See `collect_non_manifest_doc_config_keys`.
            break;
        };
        let Some(map) = value.as_mapping() else {
            continue;
        };
        let api_version = yaml_mapping_get(map, "apiVersion").and_then(serde_yaml::Value::as_str);
        let kind = yaml_mapping_get(map, "kind").and_then(serde_yaml::Value::as_str);
        let (Some(api_version), Some(kind)) = (api_version, kind) else {
            continue;
        };
        if api_version.trim().is_empty() || kind.trim().is_empty() {
            continue;
        }
        let name = yaml_mapping_get(map, "metadata")
            .and_then(serde_yaml::Value::as_mapping)
            .and_then(|metadata| yaml_mapping_get(metadata, "name"))
            .and_then(serde_yaml::Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("<unnamed>");
        let resource_name = format!("{kind}/{name}");
        if seen.insert(resource_name.clone()) {
            symbols.push(make_config_node_symbol(
                file_path,
                &resource_name,
                SymbolType::Resource,
                0,
                0,
                0,
            ));
        }
    }
    symbols
}

/// M4.3 PART 2 — expand a `kustomization.yaml`'s `resources`/`bases`/`components`
/// list entries into one `Import` symbol each (target = the referenced path),
/// pinned to the exact list-item line/col via `marked-yaml`.
fn collect_kustomize_import_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();
    let Ok(root) = marked_yaml::parse_yaml(0usize, content) else {
        return symbols;
    };
    let Some(map) = root.as_mapping() else {
        return symbols;
    };
    let line_start_offsets = line_start_offsets(content);
    for section in ["resources", "bases", "components"] {
        let Some(sequence) = map
            .get_node(section)
            .and_then(marked_yaml::types::Node::as_sequence)
        else {
            continue;
        };
        for item in sequence.iter() {
            let Some(scalar) = item.as_scalar() else {
                continue;
            };
            let target = scalar.as_str().trim();
            if target.is_empty() || target.len() > 240 {
                continue;
            }
            if !seen.insert(target.to_string()) {
                continue;
            }
            let (line, character) = marked_marker_position(scalar.span().start());
            let line_start_byte = line_start_offsets.get(line as usize).copied().unwrap_or(0);
            symbols.push(make_config_node_symbol(
                file_path,
                target,
                SymbolType::Import,
                line,
                character,
                line_start_byte,
            ));
        }
    }
    symbols
}

/// Build a `Resource`/`Import` node symbol for the config scanner.
fn make_config_node_symbol(
    file_path: &str,
    name: &str,
    symbol_type: SymbolType,
    line: u32,
    character: u32,
    line_start_byte: usize,
) -> Symbol {
    let end_char = character.saturating_add(name.len() as u32);
    Symbol {
        id: format!("{}::{}#{}", file_path, name, symbol_type),
        name: name.to_string(),
        qualified_name: name.to_string(),
        symbol_type,
        file_path: file_path.to_string(),
        range: Range {
            start: Position::new(line, character),
            end: Position::new(line, end_char),
        },
        byte_offset: line_start_byte.saturating_add(character as usize),
        byte_length: name.len(),
        parent_id: None,
        docstring: None,
        signature: None,
        content_hash: compute_hash(name),
    }
}

/// M4.3 PART 2 — Import edges for the config scanner: mirror the tree-sitter
/// import-relationship derivation so kustomize `Import` symbols also surface as
/// `Import` relationships (target = the referenced path). Other config symbols
/// (`Property`/`Resource`) are not imports and produce no edges.
fn derive_config_import_relationships(
    file_path: &str,
    symbols: &[Symbol],
) -> Vec<SymbolRelationship> {
    let mut relationships = Vec::new();
    let mut seen = HashSet::new();
    for symbol in symbols {
        if symbol.symbol_type != SymbolType::Import || symbol.name.is_empty() {
            continue;
        }
        if !seen.insert((
            symbol.id.clone(),
            symbol.name.clone(),
            symbol.range.start.line,
        )) {
            continue;
        }
        relationships.push(SymbolRelationship {
            source_symbol_id: symbol.id.clone(),
            source_file_path: file_path.to_string(),
            target_name: symbol.name.clone(),
            target_symbol_id: None,
            relationship_type: SymbolRelationshipType::Import,
            line: symbol.range.start.line,
            ..Default::default()
        });
    }
    relationships
}

/// Recursively convert a `marked-yaml` node into a `serde_yaml::Value` (dropping
/// the span markers), so the existing serde-based config-key collectors run
/// unchanged on YAML that `serde_yaml::from_str` would reject (e.g. duplicate
/// keys). All scalars become strings — the collectors key off structure and
/// mapping keys, never scalar value types.
fn marked_yaml_to_serde(node: &marked_yaml::types::Node) -> serde_yaml::Value {
    use marked_yaml::types::Node;
    match node {
        Node::Scalar(scalar) => serde_yaml::Value::String(scalar.as_str().to_string()),
        Node::Mapping(map) => {
            let mut mapping = serde_yaml::Mapping::new();
            for (key, value) in map.iter() {
                mapping.insert(
                    serde_yaml::Value::String(key.as_str().to_string()),
                    marked_yaml_to_serde(value),
                );
            }
            serde_yaml::Value::Mapping(mapping)
        }
        Node::Sequence(sequence) => {
            serde_yaml::Value::Sequence(sequence.iter().map(marked_yaml_to_serde).collect())
        }
    }
}

/// Build a `key_path -> [(line, char), …]` index from a `marked-yaml` tree,
/// mirroring `collect_yaml_config_keys`' traversal (same `is_config_key_segment`
/// filter, mappings + sequences) so each config key resolves to its OWN source
/// position. Positions are queued in document order; the consumer pops the front
/// so repeated paths (e.g. the same leaf key in two sub-trees) line up with the
/// collector's document-order entries.
fn collect_marked_yaml_key_positions(
    node: &marked_yaml::types::Node,
    path: &mut Vec<String>,
    positions: &mut HashMap<String, VecDeque<(u32, u32)>>,
) {
    use marked_yaml::types::Node;
    match node {
        Node::Mapping(map) => {
            for (key, value) in map.iter() {
                let segment = key.as_str();
                if !is_config_key_segment(segment) {
                    continue;
                }
                path.push(segment.to_string());
                let position = marked_marker_position(key.span().start());
                positions
                    .entry(path.join("."))
                    .or_default()
                    .push_back(position);
                collect_marked_yaml_key_positions(value, path, positions);
                path.pop();
            }
        }
        Node::Sequence(sequence) => {
            for item in sequence.iter() {
                collect_marked_yaml_key_positions(item, path, positions);
            }
        }
        Node::Scalar(_) => {}
    }
}

/// Convert a `marked-yaml` start marker to the crate's 0-indexed (line, char).
/// `marked-yaml` reports 1-indexed line and column.
fn marked_marker_position(marker: Option<&marked_yaml::Marker>) -> (u32, u32) {
    marker
        .map(|marker| {
            (
                marker.line().saturating_sub(1) as u32,
                marker.column().saturating_sub(1) as u32,
            )
        })
        .unwrap_or((0, 0))
}

/// A `kustomization.yaml`/`.yml` (routed to YAML by M1.3) — the Kustomize
/// overlay entry-point whose `resources`/`bases`/`components` lists are imports.
fn is_kustomization_path(file_path: &str) -> bool {
    matches!(
        config_file_name(file_path).as_str(),
        "kustomization.yaml" | "kustomization.yml"
    )
}

fn parse_json_config_value(file_path: &str, content: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .or_else(|| {
            is_jsonc_path(file_path).then(|| {
                let without_comments = strip_json_comments(content);
                let without_trailing_commas = strip_json_trailing_commas(&without_comments);
                serde_json::from_str::<serde_json::Value>(&without_trailing_commas).ok()
            })?
        })
}

fn strip_json_comments(content: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                output.push(ch);
            }
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                output.push(' ');
                output.push(' ');
                for comment_ch in chars.by_ref() {
                    if comment_ch == '\n' {
                        output.push('\n');
                        break;
                    }
                    output.push(' ');
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                output.push(' ');
                output.push(' ');
                let mut previous = '\0';
                for comment_ch in chars.by_ref() {
                    if comment_ch == '\n' {
                        output.push('\n');
                    } else {
                        output.push(' ');
                    }
                    if previous == '*' && comment_ch == '/' {
                        break;
                    }
                    previous = comment_ch;
                }
            }
            _ => output.push(ch),
        }
    }

    output
}

fn strip_json_trailing_commas(content: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                output.push(ch);
            }
            ',' => {
                let mut lookahead = chars.clone();
                while matches!(lookahead.peek(), Some(next) if next.is_whitespace()) {
                    lookahead.next();
                }
                if matches!(lookahead.peek(), Some('}' | ']')) {
                    output.push(' ');
                } else {
                    output.push(ch);
                }
            }
            _ => output.push(ch),
        }
    }

    output
}

fn collect_json_config_keys(
    value: &serde_json::Value,
    path: &mut Vec<String>,
    entries: &mut Vec<ConfigKeyEntry>,
) {
    if entries.len() >= CONFIG_SYMBOL_LIMIT {
        return;
    }

    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if entries.len() >= CONFIG_SYMBOL_LIMIT {
                    break;
                }
                if !is_config_key_segment(key) {
                    continue;
                }
                path.push(key.clone());
                push_config_key_entry(path, key, entries);
                collect_json_config_keys(value, path, entries);
                path.pop();
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                if entries.len() >= CONFIG_SYMBOL_LIMIT {
                    break;
                }
                collect_json_config_keys(value, path, entries);
            }
        }
        _ => {}
    }
}

fn collect_package_json_config_keys(value: &serde_json::Value, entries: &mut Vec<ConfigKeyEntry>) {
    let Some(root) = value.as_object() else {
        return;
    };

    for key in [
        "name",
        "version",
        "type",
        "main",
        "module",
        "browser",
        "packageManager",
        "engines",
        "bin",
        "exports",
        "imports",
    ] {
        if root.contains_key(key) {
            push_config_key_entry(&[key.to_string()], key, entries);
        }
    }

    for section in [
        "scripts",
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        collect_json_object_child_keys(root, section, entries);
    }

    if let Some(workspaces) = root.get("workspaces") {
        if workspaces.is_array() || workspaces.is_object() {
            push_config_key_entry(&["workspaces".to_string()], "workspaces", entries);
        }
    }
}

fn collect_tsconfig_json_config_keys(value: &serde_json::Value, entries: &mut Vec<ConfigKeyEntry>) {
    let Some(root) = value.as_object() else {
        return;
    };

    for key in ["extends", "include", "exclude", "files"] {
        if root.contains_key(key) {
            push_config_key_entry(&[key.to_string()], key, entries);
        }
    }

    if let Some(compiler_options) = root
        .get("compilerOptions")
        .and_then(serde_json::Value::as_object)
    {
        for key in [
            "baseUrl",
            "paths",
            "types",
            "typeRoots",
            "jsx",
            "lib",
            "module",
            "moduleResolution",
            "target",
            "strict",
            "noEmit",
            "outDir",
            "rootDir",
        ] {
            if compiler_options.contains_key(key) {
                push_config_key_entry(
                    &["compilerOptions".to_string(), key.to_string()],
                    key,
                    entries,
                );
            }
        }
        collect_json_nested_object_child_keys(
            compiler_options,
            &["compilerOptions".to_string(), "paths".to_string()],
            "paths",
            entries,
        );
    }

    if let Some(references) = root.get("references").and_then(serde_json::Value::as_array) {
        for reference in references {
            if reference.get("path").is_some() {
                push_config_key_entry(
                    &["references".to_string(), "path".to_string()],
                    "path",
                    entries,
                );
                break;
            }
        }
    }
}

fn collect_json_object_child_keys(
    root: &serde_json::Map<String, serde_json::Value>,
    section: &str,
    entries: &mut Vec<ConfigKeyEntry>,
) {
    let Some(section_value) = root.get(section).and_then(serde_json::Value::as_object) else {
        return;
    };
    for key in section_value
        .keys()
        .filter(|key| is_config_key_segment(key))
    {
        push_config_key_entry(&[section.to_string(), key.to_string()], key, entries);
    }
}

fn collect_json_nested_object_child_keys(
    root: &serde_json::Map<String, serde_json::Value>,
    parent_path: &[String],
    section: &str,
    entries: &mut Vec<ConfigKeyEntry>,
) {
    let Some(section_value) = root.get(section).and_then(serde_json::Value::as_object) else {
        return;
    };
    for key in section_value
        .keys()
        .filter(|key| is_config_key_segment(key))
    {
        let mut path = parent_path.to_vec();
        path.push(key.to_string());
        push_config_key_entry(&path, key, entries);
    }
}

fn collect_yaml_config_keys(
    value: &serde_yaml::Value,
    path: &mut Vec<String>,
    entries: &mut Vec<ConfigKeyEntry>,
) {
    if entries.len() >= CONFIG_SYMBOL_LIMIT {
        return;
    }

    match value {
        serde_yaml::Value::Mapping(map) => {
            for (key, value) in map {
                if entries.len() >= CONFIG_SYMBOL_LIMIT {
                    break;
                }
                let Some(key) = key.as_str() else {
                    continue;
                };
                if !is_config_key_segment(key) {
                    continue;
                }
                path.push(key.to_string());
                push_config_key_entry(path, key, entries);
                collect_yaml_config_keys(value, path, entries);
                path.pop();
            }
        }
        serde_yaml::Value::Sequence(items) => {
            for value in items {
                if entries.len() >= CONFIG_SYMBOL_LIMIT {
                    break;
                }
                collect_yaml_config_keys(value, path, entries);
            }
        }
        _ => {}
    }
}

fn collect_github_actions_workflow_config_keys(
    value: &serde_yaml::Value,
    entries: &mut Vec<ConfigKeyEntry>,
) {
    let Some(root) = value.as_mapping() else {
        return;
    };

    for key in ["name", "on", "permissions", "concurrency"] {
        if yaml_mapping_get(root, key).is_some() {
            push_config_key_entry(&[key.to_string()], key, entries);
        }
    }

    if let Some(triggers) = yaml_mapping_get(root, "on") {
        collect_github_actions_triggers(triggers, entries);
    }

    let Some(jobs) = yaml_mapping_get(root, "jobs").and_then(serde_yaml::Value::as_mapping) else {
        return;
    };

    for (job_key, job_value) in jobs {
        let Some(job_id) = job_key.as_str().filter(|key| is_config_key_segment(key)) else {
            continue;
        };
        push_config_key_entry(&["jobs".to_string(), job_id.to_string()], job_id, entries);

        let Some(job) = job_value.as_mapping() else {
            continue;
        };
        for key in [
            "name",
            "runs-on",
            "needs",
            "if",
            "uses",
            "permissions",
            "strategy",
            "environment",
        ] {
            if yaml_mapping_get(job, key).is_some() {
                push_config_key_entry(
                    &["jobs".to_string(), job_id.to_string(), key.to_string()],
                    key,
                    entries,
                );
            }
        }

        if let Some(steps) = yaml_mapping_get(job, "steps").and_then(serde_yaml::Value::as_sequence)
        {
            collect_github_actions_step_keys(job_id, steps, entries);
        }
    }
}

fn collect_docker_compose_config_keys(
    value: &serde_yaml::Value,
    entries: &mut Vec<ConfigKeyEntry>,
) {
    let Some(root) = value.as_mapping() else {
        return;
    };

    for key in ["name", "version"] {
        if yaml_mapping_get(root, key).is_some() {
            push_config_key_entry(&[key.to_string()], key, entries);
        }
    }

    for section in ["networks", "volumes", "secrets", "configs"] {
        collect_yaml_top_level_child_keys(root, section, entries);
    }

    let Some(services) = yaml_mapping_get(root, "services").and_then(serde_yaml::Value::as_mapping)
    else {
        return;
    };

    for (service_key, service_value) in services {
        let Some(service_name) = service_key
            .as_str()
            .filter(|key| is_config_key_segment(key))
        else {
            continue;
        };
        push_config_key_entry(
            &["services".to_string(), service_name.to_string()],
            service_name,
            entries,
        );

        let Some(service) = service_value.as_mapping() else {
            continue;
        };
        for key in [
            "image",
            "build",
            "command",
            "ports",
            "environment",
            "env_file",
            "depends_on",
            "volumes",
            "networks",
            "profiles",
            "healthcheck",
        ] {
            if yaml_mapping_get(service, key).is_some() {
                push_config_key_entry(
                    &[
                        "services".to_string(),
                        service_name.to_string(),
                        key.to_string(),
                    ],
                    key,
                    entries,
                );
            }
        }
    }
}

fn collect_yaml_top_level_child_keys(
    root: &serde_yaml::Mapping,
    section: &str,
    entries: &mut Vec<ConfigKeyEntry>,
) {
    let Some(section_value) =
        yaml_mapping_get(root, section).and_then(serde_yaml::Value::as_mapping)
    else {
        return;
    };
    for key in section_value
        .keys()
        .filter_map(serde_yaml::Value::as_str)
        .filter(|key| is_config_key_segment(key))
    {
        push_config_key_entry(&[section.to_string(), key.to_string()], key, entries);
    }
}

fn collect_github_actions_triggers(value: &serde_yaml::Value, entries: &mut Vec<ConfigKeyEntry>) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (trigger_key, _) in map {
                let Some(trigger) = trigger_key
                    .as_str()
                    .filter(|key| is_config_key_segment(key))
                else {
                    continue;
                };
                push_config_key_entry(&["on".to_string(), trigger.to_string()], trigger, entries);
            }
        }
        serde_yaml::Value::Sequence(items) => {
            for trigger in items.iter().filter_map(serde_yaml::Value::as_str) {
                if is_config_key_segment(trigger) {
                    push_config_key_entry(
                        &["on".to_string(), trigger.to_string()],
                        trigger,
                        entries,
                    );
                }
            }
        }
        _ => {}
    }
}

fn collect_github_actions_step_keys(
    job_id: &str,
    steps: &[serde_yaml::Value],
    entries: &mut Vec<ConfigKeyEntry>,
) {
    // NOTE (§13, best-effort): the leaf segment here is the step's VALUE (its
    // `name`/`uses`/`run`), not a literal mapping key, so `jobs.<id>.steps.<name>`
    // is NOT present in the literal key-path index built by
    // `collect_marked_yaml_key_positions`. These entries therefore carry no span
    // and fall back to `locate_config_key` — value-keyed GitHub-Actions step
    // positions remain best-effort. (The GENERIC repeated-leaf-key case — the same
    // literal key under two sub-trees — IS span-accurate via the position queue.)
    for step in steps {
        let Some(step_map) = step.as_mapping() else {
            continue;
        };
        let step_name = yaml_mapping_get(step_map, "name")
            .and_then(serde_yaml::Value::as_str)
            .or_else(|| yaml_mapping_get(step_map, "uses").and_then(serde_yaml::Value::as_str))
            .or_else(|| yaml_mapping_get(step_map, "run").and_then(serde_yaml::Value::as_str));
        let Some(step_name) = step_name.filter(|value| is_workflow_step_symbol_segment(value))
        else {
            continue;
        };
        push_config_key_entry(
            &[
                "jobs".to_string(),
                job_id.to_string(),
                "steps".to_string(),
                step_name.to_string(),
            ],
            step_name,
            entries,
        );
    }
}

fn yaml_mapping_get<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    map.get(&serde_yaml::Value::String(key.to_string()))
}

fn collect_toml_config_keys(content: &str, entries: &mut Vec<ConfigKeyEntry>) {
    // M4.3 PART 3 — span-accurate by construction: this is a physical line scan,
    // so each key entry carries the line it was seen on (and the column where the
    // key starts), instead of being re-found by `locate_config_key` (which pins
    // every duplicate-named key to its first occurrence).
    let mut table_path = Vec::<String>::new();
    for (line_index, line) in content.lines().enumerate() {
        if entries.len() >= CONFIG_SYMBOL_LIMIT {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(table) = toml_table_path(trimmed) {
            table_path = table;
            continue;
        }

        let Some((raw_key, _)) = trimmed.split_once('=') else {
            continue;
        };
        let key_segments = toml_key_segments(raw_key);
        if key_segments.is_empty() {
            continue;
        }

        let mut path = table_path.clone();
        path.extend(key_segments);
        let Some(leaf_key) = path.last().cloned() else {
            continue;
        };
        let column = line.len().saturating_sub(line.trim_start().len()) as u32;
        push_config_key_entry_at(&path, &leaf_key, Some((line_index as u32, column)), entries);
    }
}

fn push_config_key_entry(path: &[String], leaf_key: &str, entries: &mut Vec<ConfigKeyEntry>) {
    push_config_key_entry_at(path, leaf_key, None, entries);
}

fn push_config_key_entry_at(
    path: &[String],
    leaf_key: &str,
    position: Option<(u32, u32)>,
    entries: &mut Vec<ConfigKeyEntry>,
) {
    if entries.len() >= CONFIG_SYMBOL_LIMIT || path.is_empty() {
        return;
    }

    entries.push(ConfigKeyEntry {
        key_path: path.join("."),
        leaf_key: leaf_key.to_string(),
        position,
    });
}

fn toml_table_path(line: &str) -> Option<Vec<String>> {
    let inner = line
        .strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
        .or_else(|| {
            line.strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
        })?;
    let path = toml_key_segments(inner);
    (!path.is_empty()).then_some(path)
}

fn toml_key_segments(raw_key: &str) -> Vec<String> {
    raw_key
        .split('.')
        .map(|segment| segment.trim().trim_matches(['"', '\'']))
        .filter(|segment| is_config_key_segment(segment))
        .map(ToString::to_string)
        .collect()
}

fn is_config_key_segment(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 120
        && !key.chars().any(|ch| ch.is_control())
        && key.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn is_workflow_step_symbol_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && !value.chars().any(|ch| ch.is_control())
        && value.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn is_package_json_path(file_path: &str) -> bool {
    config_file_name(file_path) == "package.json"
}

fn is_tsconfig_json_path(file_path: &str) -> bool {
    matches!(
        config_file_name(file_path).as_str(),
        "tsconfig.json" | "tsconfig.jsonc" | "jsconfig.json" | "jsconfig.jsonc"
    )
}

fn config_file_name(file_path: &str) -> String {
    file_path
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_github_actions_workflow_path(file_path: &str) -> bool {
    let lower = file_path.replace('\\', "/").to_ascii_lowercase();
    (lower.ends_with(".yml") || lower.ends_with(".yaml"))
        && (lower.starts_with(".github/workflows/") || lower.contains("/.github/workflows/"))
}

fn is_docker_compose_path(file_path: &str) -> bool {
    matches!(
        config_file_name(file_path).as_str(),
        "compose.yaml" | "compose.yml" | "docker-compose.yaml" | "docker-compose.yml"
    )
}

fn is_jsonc_path(file_path: &str) -> bool {
    config_file_name(file_path).ends_with(".jsonc")
}

fn locate_config_key(content: &str, key_path: &str, leaf_key: &str) -> AnchorLocation {
    let quoted_leaf = format!("\"{}\"", leaf_key);
    let quoted_leaf_single = format!("'{}'", leaf_key);
    for (line_index, line) in content.lines().enumerate() {
        if line.contains(&quoted_leaf)
            || line.contains(&quoted_leaf_single)
            || line.trim_start().starts_with(&format!("{leaf_key}:"))
            || line.contains(key_path)
        {
            let character = line
                .find(leaf_key)
                .or_else(|| line.find(key_path))
                .unwrap_or(0);
            return AnchorLocation {
                line: line_index as u32,
                character: character as u32,
            };
        }
    }
    AnchorLocation {
        line: 0,
        character: 0,
    }
}

fn push_config_symbol(
    symbols: &mut Vec<Symbol>,
    seen: &mut HashSet<(String, SymbolType, u32)>,
    file_path: &str,
    key_path: &str,
    line_number: u32,
    start_char: usize,
    line_start_byte: usize,
    line_text: &str,
) {
    if key_path.is_empty()
        || !seen.insert((key_path.to_string(), SymbolType::Property, line_number))
    {
        return;
    }

    let end_char = start_char.saturating_add(key_path.len());
    symbols.push(Symbol {
        id: format!("{}::{}#{}", file_path, key_path, SymbolType::Property),
        name: key_path.to_string(),
        qualified_name: key_path.to_string(),
        symbol_type: SymbolType::Property,
        file_path: file_path.to_string(),
        range: Range {
            start: Position::new(line_number, start_char as u32),
            end: Position::new(line_number, end_char as u32),
        },
        byte_offset: line_start_byte.saturating_add(start_char),
        byte_length: key_path.len(),
        parent_id: None,
        docstring: None,
        signature: None,
        content_hash: compute_hash(line_text),
    });
}

// ============================================================================
// M1.4 — shared comment/string/heredoc-aware preprocessing for line scanners.
//
// ZBlade's line-oriented scanner languages carry no cross-line lexical state, so
// a `class`/`def`/`function` keyword sitting inside a `/* … */` block, a string
// literal, or a heredoc was extracted as a real symbol, and a trailing
// `// class Foo` comment minted a bogus symbol. `blank_noncode_spans` replaces
// the bytes of commented / quoted / heredoc spans with spaces — preserving
// newlines and the exact byte length, so every downstream line/column/byte
// calculation is unchanged — BEFORE the per-line declaration scanners run.
//
// ONE implementation, parameterized per language by `LexSpec`. Strings are
// always *tracked* (so a comment marker inside a string is never mistaken for a
// comment), but only *blanked* when `blank_strings` is set: several scanners
// read an identifier out of a string literal (Ruby `require "x"`, shell
// `source "x"`, C/C++ `#include "x"`), and blanking those would erase real
// symbols.
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum HeredocStyle {
    None,
    /// PHP `<<<LABEL` / `<<<'LABEL'` heredoc & nowdoc (`<<<` is an unambiguous
    /// opener, unlike the shell/Ruby `<<` which collides with operators).
    Php,
}

#[derive(Clone, Copy)]
struct StringDelim {
    open: u8,
    close: u8,
}

/// Per-language lexical surface used by `blank_noncode_spans`.
struct LexSpec {
    /// Line-comment markers (e.g. `//`, `#`, `--`). First match wins.
    line_comments: &'static [&'static str],
    /// Block-comment open/close (e.g. `("/*", "*/")`); spans lines.
    block_comment: Option<(&'static str, &'static str)>,
    /// String delimiters to track. A backslash escapes the next byte.
    strings: &'static [StringDelim],
    /// Blank string interiors too — only for languages that never read an
    /// identifier out of a string literal.
    blank_strings: bool,
    heredoc: HeredocStyle,
    /// PHP 8 attributes start with `#[`; never treat that `#` as a line comment.
    attr_hash_guard: bool,
}

const STR_DQ_SQ: &[StringDelim] = &[
    StringDelim {
        open: b'"',
        close: b'"',
    },
    StringDelim {
        open: b'\'',
        close: b'\'',
    },
];
const STR_SQ_ONLY: &[StringDelim] = &[StringDelim {
    open: b'\'',
    close: b'\'',
}];
const STR_DQ_ONLY: &[StringDelim] = &[StringDelim {
    open: b'"',
    close: b'"',
}];

const PHP_LEX: LexSpec = LexSpec {
    line_comments: &["//", "#"],
    block_comment: Some(("/*", "*/")),
    strings: STR_DQ_SQ,
    blank_strings: true,
    heredoc: HeredocStyle::Php,
    attr_hash_guard: true,
};
const JAVA_LEX: LexSpec = LexSpec {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    strings: STR_DQ_SQ,
    blank_strings: true,
    heredoc: HeredocStyle::None,
    attr_hash_guard: false,
};
const CSHARP_LEX: LexSpec = LexSpec {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    strings: STR_DQ_SQ,
    blank_strings: true,
    heredoc: HeredocStyle::None,
    attr_hash_guard: false,
};
const KOTLIN_LEX: LexSpec = LexSpec {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    strings: STR_DQ_SQ,
    blank_strings: true,
    heredoc: HeredocStyle::None,
    attr_hash_guard: false,
};
// Ruby reads `require "x"` out of a string → track but do not blank strings.
const RUBY_LEX: LexSpec = LexSpec {
    line_comments: &["#"],
    block_comment: None,
    strings: STR_DQ_SQ,
    blank_strings: false,
    heredoc: HeredocStyle::None,
    attr_hash_guard: false,
};
// Shell reads `source "x"` out of a string → track but do not blank strings.
const SHELL_LEX: LexSpec = LexSpec {
    line_comments: &["#"],
    block_comment: None,
    strings: STR_DQ_SQ,
    blank_strings: false,
    heredoc: HeredocStyle::None,
    attr_hash_guard: false,
};
const DOCKERFILE_LEX: LexSpec = LexSpec {
    line_comments: &["#"],
    block_comment: None,
    strings: STR_DQ_SQ,
    blank_strings: false,
    heredoc: HeredocStyle::None,
    attr_hash_guard: false,
};
// SQL: `'` is a string, `"` is a (quoted) identifier — track only `'`.
const SQL_LEX: LexSpec = LexSpec {
    line_comments: &["--"],
    block_comment: Some(("/*", "*/")),
    strings: STR_SQ_ONLY,
    blank_strings: false,
    heredoc: HeredocStyle::None,
    attr_hash_guard: false,
};
const BUILD_SCRIPT_LEX: LexSpec = LexSpec {
    line_comments: &["#"],
    block_comment: None,
    strings: STR_DQ_ONLY,
    blank_strings: false,
    heredoc: HeredocStyle::None,
    attr_hash_guard: false,
};
// CSS/SCSS/Sass/Less: blank `/* */` blocks and strings so a `{`/`}` hiding in a
// `content: "}"` value (or a comment) is not mis-counted as a real brace. Line
// comments are intentionally omitted: a bare `//` is not a CSS comment and would
// wrongly swallow `url(http://…)`. Used only for the structural brace/selector
// view; symbol names are still read from the original line.
const CSS_LEX: LexSpec = LexSpec {
    line_comments: &[],
    block_comment: Some(("/*", "*/")),
    strings: STR_DQ_SQ,
    blank_strings: true,
    heredoc: HeredocStyle::None,
    attr_hash_guard: false,
};

/// Blank comment / string / heredoc spans of `content`, preserving byte length
/// and newlines so existing line/offset math is unchanged.
fn blank_noncode_spans(content: &str, spec: &LexSpec) -> String {
    let src = content.as_bytes();
    let mut out = src.to_vec();
    let mut in_block = false;
    let mut heredoc_label: Option<Vec<u8>> = None;
    let mut pos = 0usize;

    for segment in content.split_inclusive('\n') {
        let line_start = pos;
        let line_end = pos + segment.len();
        pos = line_end;
        scrub_line(
            src,
            &mut out,
            line_start,
            line_end,
            spec,
            &mut in_block,
            &mut heredoc_label,
        );
    }

    String::from_utf8(out).unwrap_or_else(|_| content.to_string())
}

#[inline]
fn blank_span_byte(out: &mut [u8], src: &[u8], i: usize) {
    if src[i] != b'\n' && src[i] != b'\r' {
        out[i] = b' ';
    }
}

#[inline]
fn bytes_match_at(src: &[u8], i: usize, needle: &str) -> bool {
    let n = needle.as_bytes();
    src.len() >= i + n.len() && &src[i..i + n.len()] == n
}

fn line_comment_len_at(src: &[u8], i: usize, spec: &LexSpec) -> Option<usize> {
    for marker in spec.line_comments {
        if bytes_match_at(src, i, marker) {
            if spec.attr_hash_guard && *marker == "#" && src.get(i + 1) == Some(&b'[') {
                continue; // PHP 8 attribute `#[…]`, not a comment.
            }
            return Some(marker.len());
        }
    }
    None
}

fn string_open_at(src: &[u8], i: usize, spec: &LexSpec) -> Option<u8> {
    let b = src[i];
    spec.strings
        .iter()
        .find(|delim| delim.open == b)
        .map(|delim| delim.close)
}

/// Detect a PHP heredoc/nowdoc opener `<<<LABEL` / `<<<'LABEL'` at `i`. Returns
/// the label and the number of bytes consumed by the opener token.
fn php_heredoc_open_at(src: &[u8], i: usize, end: usize) -> Option<(Vec<u8>, usize)> {
    if !bytes_match_at(src, i, "<<<") {
        return None;
    }
    let mut j = i + 3;
    while j < end && (src[j] == b' ' || src[j] == b'\t') {
        j += 1;
    }
    let quote = if j < end && (src[j] == b'\'' || src[j] == b'"') {
        let q = src[j];
        j += 1;
        Some(q)
    } else {
        None
    };
    let label_start = j;
    while j < end && (src[j] == b'_' || src[j].is_ascii_alphanumeric()) {
        j += 1;
    }
    if j == label_start || src[label_start].is_ascii_digit() {
        return None; // labels are non-empty and cannot start with a digit
    }
    let label = src[label_start..j].to_vec();
    if let Some(q) = quote {
        if src.get(j) != Some(&q) {
            return None; // unterminated quoted label
        }
        j += 1;
    }
    Some((label, j - i))
}

/// Is this line a heredoc closing marker for `label`? (PHP 7.3+ allows the
/// closing label to be indented and immediately followed by a non-identifier.)
fn line_is_heredoc_terminator(line: &[u8], label: &[u8]) -> bool {
    let mut endp = line.len();
    while endp > 0 && (line[endp - 1] == b'\n' || line[endp - 1] == b'\r') {
        endp -= 1;
    }
    let line = &line[..endp];
    let mut s = 0usize;
    while s < line.len() && (line[s] == b' ' || line[s] == b'\t') {
        s += 1;
    }
    let rest = &line[s..];
    if !rest.starts_with(label) {
        return false;
    }
    match rest.get(label.len()) {
        None => true,
        Some(c) => !(c.is_ascii_alphanumeric() || *c == b'_'),
    }
}

/// Scrub one line `src[start..end]` (the trailing `\n`, if any, is inside the
/// range), updating the cross-line `in_block` / `heredoc_label` state.
fn scrub_line(
    src: &[u8],
    out: &mut [u8],
    start: usize,
    end: usize,
    spec: &LexSpec,
    in_block: &mut bool,
    heredoc_label: &mut Option<Vec<u8>>,
) {
    // Inside an open heredoc: blank body lines until the terminator line.
    if let Some(label) = heredoc_label.as_ref() {
        if line_is_heredoc_terminator(&src[start..end], label) {
            *heredoc_label = None;
        } else {
            for i in start..end {
                blank_span_byte(out, src, i);
            }
        }
        return;
    }

    let mut pending_heredoc: Option<Vec<u8>> = None;
    let mut cur_string: Option<u8> = None; // line-local; reset every line
    let mut i = start;

    while i < end {
        let b = src[i];
        if b == b'\n' {
            break;
        }

        if *in_block {
            if let Some((_, close)) = spec.block_comment {
                if bytes_match_at(src, i, close) {
                    for k in i..i + close.len() {
                        blank_span_byte(out, src, k);
                    }
                    i += close.len();
                    *in_block = false;
                    continue;
                }
            }
            blank_span_byte(out, src, i);
            i += 1;
            continue;
        }

        if let Some(close) = cur_string {
            if b == b'\\' {
                if spec.blank_strings {
                    blank_span_byte(out, src, i);
                    if i + 1 < end {
                        blank_span_byte(out, src, i + 1);
                    }
                }
                i += if i + 1 < end { 2 } else { 1 };
                continue;
            }
            if spec.blank_strings {
                blank_span_byte(out, src, i);
            }
            if b == close {
                cur_string = None;
            }
            i += 1;
            continue;
        }

        // Normal state.
        if let Some((open, _)) = spec.block_comment {
            if bytes_match_at(src, i, open) {
                for k in i..i + open.len() {
                    blank_span_byte(out, src, k);
                }
                i += open.len();
                *in_block = true;
                continue;
            }
        }

        if line_comment_len_at(src, i, spec).is_some() {
            while i < end && src[i] != b'\n' {
                blank_span_byte(out, src, i);
                i += 1;
            }
            continue;
        }

        if let Some(close) = string_open_at(src, i, spec) {
            if spec.blank_strings {
                blank_span_byte(out, src, i);
            }
            cur_string = Some(close);
            i += 1;
            continue;
        }

        if spec.heredoc == HeredocStyle::Php {
            if let Some((label, consumed)) = php_heredoc_open_at(src, i, end) {
                pending_heredoc = Some(label);
                i += consumed;
                continue;
            }
        }

        i += 1;
    }

    if let Some(label) = pending_heredoc {
        *heredoc_label = Some(label);
    }
}

/// Run the non-tree-sitter line/regex SCANNER extractor for a scanner-backed
/// language and return the symbols it would index. The golden harness uses this
/// to exercise the scanner path directly, which the public tree-sitter
/// `extract_symbols` cannot reach. Returns an empty vec for tree-sitter
/// languages and for Vue/Svelte (whose component extraction needs a
/// `LanguageService`).
pub fn extract_scanner_symbols(file_path: &str, content: &str, language: Language) -> Vec<Symbol> {
    if matches!(language, Language::Markdown) {
        return extract_markdown_header_symbols(file_path, content);
    }
    if language.is_stylesheet_scanner() {
        return extract_css_symbols(file_path, content);
    }
    if language.is_markup_scanner() && !matches!(language, Language::Vue | Language::Svelte) {
        return extract_markup_symbols(file_path, content);
    }
    if language.is_config_scanner() {
        return extract_config_symbols(file_path, content, language);
    }
    if language.is_php_scanner() {
        return extract_php_symbols(file_path, content);
    }
    if language.is_java_scanner() {
        return extract_java_symbols(file_path, content);
    }
    if language.is_csharp_scanner() {
        return extract_csharp_symbols(file_path, content);
    }
    if language.is_kotlin_scanner() {
        return extract_kotlin_symbols(file_path, content);
    }
    if language.is_ruby_scanner() {
        return extract_ruby_symbols(file_path, content);
    }
    if language.is_shell_scanner() {
        return extract_shell_symbols(file_path, content);
    }
    if language.is_dockerfile_scanner() {
        return extract_dockerfile_symbols(file_path, content);
    }
    if language.is_sql_scanner() {
        return extract_sql_symbols(file_path, content);
    }
    if language.is_build_script_scanner() {
        return extract_build_script_symbols(file_path, content);
    }
    Vec::new()
}

fn extract_php_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    extract_line_scanner_symbols(file_path, content, &PHP_LEX, php_declarations_in_line)
}

#[derive(Debug, Clone)]
struct LineScannerDeclaration {
    name: String,
    symbol_type: SymbolType,
    start_char: usize,
    end_char: usize,
}

fn extract_line_scanner_symbols(
    file_path: &str,
    content: &str,
    spec: &LexSpec,
    declarations_in_line: fn(&str) -> Vec<LineScannerDeclaration>,
) -> Vec<Symbol> {
    // Blank commented/quoted/heredoc spans first (byte-length and newlines
    // preserved), so a keyword hiding in a comment/string/heredoc is not
    // mis-extracted. Offsets computed below are identical to the original file.
    let blanked = blank_noncode_spans(content, spec);
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();
    let mut byte_offset = 0usize;

    for (line_index, segment) in blanked.split_inclusive('\n').enumerate() {
        let line_start = byte_offset;
        byte_offset += segment.len();

        let line_without_lf = segment.strip_suffix('\n').unwrap_or(segment);
        let line = line_without_lf
            .strip_suffix('\r')
            .unwrap_or(line_without_lf);
        let line_number = line_index as u32;

        for declaration in declarations_in_line(line) {
            push_line_scanner_symbol(
                &mut symbols,
                &mut seen,
                file_path,
                declaration,
                line_number,
                line_start,
                line,
            );
        }
    }

    symbols
}

fn php_declarations_in_line(line: &str) -> Vec<LineScannerDeclaration> {
    let code = php_line_code(line);
    if code.is_empty() {
        return Vec::new();
    }
    let code_start = line.find(code).unwrap_or(0);

    if let Some(declaration) = php_namespace_declaration(code, code_start) {
        return vec![declaration];
    }
    if let Some(declaration) = php_import_declaration(code, code_start) {
        return vec![declaration];
    }

    let mut declarations = Vec::new();
    for (keyword, symbol_type) in [
        ("class", SymbolType::Class),
        ("interface", SymbolType::Interface),
        ("trait", SymbolType::Trait),
    ] {
        if let Some(declaration) = php_keyword_declaration(code, code_start, keyword, symbol_type) {
            declarations.push(declaration);
        }
    }
    if let Some(declaration) = php_function_declaration(code, code_start) {
        declarations.push(declaration);
    }
    declarations
}

fn php_line_code(line: &str) -> &str {
    let trimmed = line.trim_start();
    let trimmed = trimmed
        .strip_prefix("<?php")
        .unwrap_or(trimmed)
        .trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with('*') {
        ""
    } else {
        trimmed
    }
}

fn php_namespace_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code.strip_prefix("namespace ")?;
    let name_start = code.len().saturating_sub(rest.len());
    let (name, name_len) = php_read_qualified_name(rest.trim_start())?;
    let leading_ws = rest.len().saturating_sub(rest.trim_start().len());
    let start_char = code_start + name_start + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Namespace,
        start_char,
        end_char: start_char + name_len,
    })
}

fn php_import_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code.strip_prefix("use ")?;
    if rest.trim_start().starts_with('(') {
        return None;
    }
    let rest = rest
        .trim_start()
        .strip_prefix("function ")
        .or_else(|| rest.trim_start().strip_prefix("const "))
        .unwrap_or(rest.trim_start());
    let leading_ws = code.len().saturating_sub(rest.len());
    let (name, name_len) = php_read_qualified_name(rest)?;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Import,
        start_char: code_start + leading_ws,
        end_char: code_start + leading_ws + name_len,
    })
}

fn php_keyword_declaration(
    code: &str,
    code_start: usize,
    keyword: &str,
    symbol_type: SymbolType,
) -> Option<LineScannerDeclaration> {
    let keyword_start = php_find_keyword(code, keyword)?;
    let after_keyword = keyword_start + keyword.len();
    let rest = code[after_keyword..].trim_start();
    let leading_ws = code[after_keyword..].len().saturating_sub(rest.len());
    let (name, name_len) = php_read_identifier(rest)?;
    let start_char = code_start + after_keyword + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type,
        start_char,
        end_char: start_char + name_len,
    })
}

fn php_function_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let keyword_start = php_find_keyword(code, "function")?;
    let after_keyword = keyword_start + "function".len();
    let mut rest = code[after_keyword..].trim_start();
    if let Some(after_ref) = rest.strip_prefix('&') {
        rest = after_ref.trim_start();
    }
    let leading_ws = code[after_keyword..].len().saturating_sub(rest.len());
    let (name, name_len) = php_read_identifier(rest)?;
    let before_keyword = code[..keyword_start].trim();
    let symbol_type = if before_keyword
        .split_whitespace()
        .any(|token| matches!(token, "public" | "protected" | "private"))
    {
        SymbolType::Method
    } else {
        SymbolType::Function
    };
    let start_char = code_start + after_keyword + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type,
        start_char,
        end_char: start_char + name_len,
    })
}

fn php_find_keyword(code: &str, keyword: &str) -> Option<usize> {
    let mut search_start = 0usize;
    while let Some(relative_index) = code[search_start..].find(keyword) {
        let start = search_start + relative_index;
        let end = start + keyword.len();
        let before_ok = code[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !is_php_identifier_char(ch));
        let after_ok = code[end..]
            .chars()
            .next()
            .is_none_or(|ch| !is_php_identifier_char(ch));
        if before_ok && after_ok {
            return Some(start);
        }
        search_start = end;
    }
    None
}

fn php_read_identifier(text: &str) -> Option<(String, usize)> {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if index == 0 && !(ch == '_' || ch.is_ascii_alphabetic()) {
            return None;
        }
        if is_php_identifier_char(ch) {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    (end > 0).then(|| (text[..end].to_string(), end))
}

fn php_read_qualified_name(text: &str) -> Option<(String, usize)> {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if index == 0 && !(ch == '\\' || ch == '_' || ch.is_ascii_alphabetic()) {
            return None;
        }
        if is_php_identifier_char(ch) || ch == '\\' {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    (end > 0).then(|| (text[..end].trim_start_matches('\\').to_string(), end))
}

fn is_php_identifier_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn extract_java_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    extract_line_scanner_symbols(file_path, content, &JAVA_LEX, java_declarations_in_line)
}

fn java_declarations_in_line(line: &str) -> Vec<LineScannerDeclaration> {
    let code = java_line_code(line);
    if code.is_empty() {
        return Vec::new();
    }
    let code_start = line.find(code).unwrap_or(0);

    if let Some(declaration) = java_package_declaration(code, code_start) {
        return vec![declaration];
    }
    if let Some(declaration) = java_import_declaration(code, code_start) {
        return vec![declaration];
    }

    let mut declarations = Vec::new();
    for (keyword, symbol_type) in [
        ("class", SymbolType::Class),
        ("interface", SymbolType::Interface),
        ("enum", SymbolType::Enum),
        ("record", SymbolType::Struct),
    ] {
        if let Some(declaration) = java_keyword_declaration(code, code_start, keyword, symbol_type)
        {
            declarations.push(declaration);
        }
    }

    if declarations.is_empty() {
        if let Some(declaration) = java_method_declaration(code, code_start) {
            declarations.push(declaration);
        }
    }

    declarations
}

fn java_line_code(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//")
        || trimmed.starts_with('*')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('@')
    {
        ""
    } else {
        trimmed.split("//").next().unwrap_or(trimmed).trim_end()
    }
}

fn java_package_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code.strip_prefix("package ")?;
    let name_start = code.len().saturating_sub(rest.len());
    let rest = rest.trim_start();
    let leading_ws = code[name_start..].len().saturating_sub(rest.len());
    let (name, name_len) = java_read_qualified_name(rest, false)?;
    let start_char = code_start + name_start + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Namespace,
        start_char,
        end_char: start_char + name_len,
    })
}

fn java_import_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code.strip_prefix("import ")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("static ").unwrap_or(rest).trim_start();
    let leading_ws = code.len().saturating_sub(rest.len());
    let (name, name_len) = java_read_qualified_name(rest, true)?;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Import,
        start_char: code_start + leading_ws,
        end_char: code_start + leading_ws + name_len,
    })
}

fn java_keyword_declaration(
    code: &str,
    code_start: usize,
    keyword: &str,
    symbol_type: SymbolType,
) -> Option<LineScannerDeclaration> {
    let keyword_start = java_find_keyword(code, keyword)?;
    let after_keyword = keyword_start + keyword.len();
    let rest = code[after_keyword..].trim_start();
    let leading_ws = code[after_keyword..].len().saturating_sub(rest.len());
    let (name, name_len) = java_read_identifier(rest)?;
    let start_char = code_start + after_keyword + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type,
        start_char,
        end_char: start_char + name_len,
    })
}

fn java_method_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let paren_index = code.find('(')?;
    let before_paren = code[..paren_index].trim_end();
    if before_paren.contains('=')
        || before_paren.contains(" -> ")
        || java_starts_with_statement_keyword(before_paren)
    {
        return None;
    }

    let (name, name_start) = java_read_identifier_before(before_paren)?;
    if matches!(
        name.as_str(),
        "if" | "for"
            | "while"
            | "switch"
            | "catch"
            | "return"
            | "new"
            | "throw"
            | "assert"
            | "synchronized"
    ) {
        return None;
    }

    let before_name = before_paren[..name_start].trim_end();
    if before_name.is_empty() || before_name.ends_with('.') {
        return None;
    }

    let start_char = code_start + name_start;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Method,
        start_char,
        end_char: start_char + before_paren[name_start..].len(),
    })
}

fn java_find_keyword(code: &str, keyword: &str) -> Option<usize> {
    let mut search_start = 0usize;
    while let Some(relative_index) = code[search_start..].find(keyword) {
        let start = search_start + relative_index;
        let end = start + keyword.len();
        let before_ok = code[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !is_java_identifier_char(ch));
        let after_ok = code[end..]
            .chars()
            .next()
            .is_none_or(|ch| !is_java_identifier_char(ch));
        if before_ok && after_ok {
            return Some(start);
        }
        search_start = end;
    }
    None
}

fn java_read_identifier(text: &str) -> Option<(String, usize)> {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if index == 0 && !(ch == '_' || ch == '$' || ch.is_ascii_alphabetic()) {
            return None;
        }
        if is_java_identifier_char(ch) {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    (end > 0).then(|| (text[..end].to_string(), end))
}

fn java_read_identifier_before(text: &str) -> Option<(String, usize)> {
    let trimmed_len = text.trim_end().len();
    let trimmed = &text[..trimmed_len];
    let mut start = trimmed.len();
    for (index, ch) in trimmed.char_indices().rev() {
        if is_java_identifier_char(ch) {
            start = index;
        } else {
            break;
        }
    }
    if start == trimmed.len() {
        return None;
    }
    let name = &trimmed[start..];
    name.chars()
        .next()
        .filter(|ch| *ch == '_' || *ch == '$' || ch.is_ascii_alphabetic())?;
    Some((name.to_string(), start))
}

fn java_read_qualified_name(text: &str, allow_wildcard: bool) -> Option<(String, usize)> {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if index == 0 && !(ch == '_' || ch == '$' || ch.is_ascii_alphabetic()) {
            return None;
        }
        if is_java_identifier_char(ch) || ch == '.' || (allow_wildcard && ch == '*') {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    (end > 0).then(|| (text[..end].to_string(), end))
}

fn java_starts_with_statement_keyword(text: &str) -> bool {
    [
        "if", "for", "while", "switch", "catch", "return", "throw", "assert", "new",
    ]
    .iter()
    .any(|keyword| text.trim_start().starts_with(keyword))
}

fn is_java_identifier_char(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphanumeric()
}

fn extract_csharp_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    extract_line_scanner_symbols(file_path, content, &CSHARP_LEX, csharp_declarations_in_line)
}

fn csharp_declarations_in_line(line: &str) -> Vec<LineScannerDeclaration> {
    let code = csharp_line_code(line);
    if code.is_empty() {
        return Vec::new();
    }
    let code_start = line.find(code).unwrap_or(0);

    if let Some(declaration) = csharp_namespace_declaration(code, code_start) {
        return vec![declaration];
    }
    if let Some(declaration) = csharp_using_declaration(code, code_start) {
        return vec![declaration];
    }

    let mut declarations = Vec::new();
    for (keyword, symbol_type) in [
        ("class", SymbolType::Class),
        ("interface", SymbolType::Interface),
        ("struct", SymbolType::Struct),
        ("enum", SymbolType::Enum),
    ] {
        if let Some(declaration) =
            csharp_keyword_declaration(code, code_start, keyword, symbol_type)
        {
            declarations.push(declaration);
        }
    }
    if let Some(declaration) = csharp_record_declaration(code, code_start) {
        declarations.push(declaration);
    }

    if declarations.is_empty() {
        if let Some(declaration) = csharp_method_declaration(code, code_start) {
            declarations.push(declaration);
        }
    }

    declarations
}

fn csharp_line_code(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//")
        || trimmed.starts_with('*')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('[')
    {
        ""
    } else {
        trimmed.split("//").next().unwrap_or(trimmed).trim_end()
    }
}

fn csharp_namespace_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code.strip_prefix("namespace ")?;
    let name_start = code.len().saturating_sub(rest.len());
    let rest = rest.trim_start();
    let leading_ws = code[name_start..].len().saturating_sub(rest.len());
    let (name, name_len) = java_read_qualified_name(rest, false)?;
    let start_char = code_start + name_start + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Namespace,
        start_char,
        end_char: start_char + name_len,
    })
}

fn csharp_using_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code.strip_prefix("using ")?;
    let mut rest = rest.trim_start();
    if rest.starts_with('(') || rest.starts_with("var ") {
        return None;
    }
    rest = rest.strip_prefix("static ").unwrap_or(rest).trim_start();
    if let Some(alias_split) = rest.find('=') {
        rest = rest[alias_split + 1..].trim_start();
    }
    let leading_ws = code.len().saturating_sub(rest.len());
    let (name, name_len) = java_read_qualified_name(rest, false)?;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Import,
        start_char: code_start + leading_ws,
        end_char: code_start + leading_ws + name_len,
    })
}

fn csharp_keyword_declaration(
    code: &str,
    code_start: usize,
    keyword: &str,
    symbol_type: SymbolType,
) -> Option<LineScannerDeclaration> {
    let keyword_start = java_find_keyword(code, keyword)?;
    let after_keyword = keyword_start + keyword.len();
    let rest = code[after_keyword..].trim_start();
    let leading_ws = code[after_keyword..].len().saturating_sub(rest.len());
    let (name, name_len) = java_read_identifier(rest)?;
    let start_char = code_start + after_keyword + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type,
        start_char,
        end_char: start_char + name_len,
    })
}

fn csharp_record_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let keyword_start = java_find_keyword(code, "record")?;
    let after_keyword = keyword_start + "record".len();
    let mut rest = code[after_keyword..].trim_start();
    if let Some(after_kind) = rest
        .strip_prefix("class ")
        .or_else(|| rest.strip_prefix("struct "))
    {
        rest = after_kind.trim_start();
    }
    let leading_ws = code[after_keyword..].len().saturating_sub(rest.len());
    let (name, name_len) = java_read_identifier(rest)?;
    let start_char = code_start + after_keyword + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Struct,
        start_char,
        end_char: start_char + name_len,
    })
}

fn csharp_method_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let mut declaration = java_method_declaration(code, code_start)?;
    if matches!(
        declaration.name.as_str(),
        "nameof" | "typeof" | "sizeof" | "default"
    ) {
        return None;
    }
    declaration.symbol_type = SymbolType::Method;
    Some(declaration)
}

fn extract_kotlin_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    extract_line_scanner_symbols(file_path, content, &KOTLIN_LEX, kotlin_declarations_in_line)
}

fn kotlin_declarations_in_line(line: &str) -> Vec<LineScannerDeclaration> {
    let code = kotlin_line_code(line);
    if code.is_empty() {
        return Vec::new();
    }
    let code_start = line.find(code).unwrap_or(0);

    if let Some(declaration) = kotlin_package_declaration(code, code_start) {
        return vec![declaration];
    }
    if let Some(declaration) = kotlin_import_declaration(code, code_start) {
        return vec![declaration];
    }

    let mut declarations = Vec::new();
    let has_special_class = ["enum class", "data class", "value class", "sealed class"]
        .iter()
        .any(|pattern| code.contains(pattern));
    for (keyword, symbol_type) in [
        ("class", SymbolType::Class),
        ("interface", SymbolType::Interface),
        ("object", SymbolType::Module),
    ] {
        if keyword == "class" && has_special_class {
            continue;
        }
        if let Some(declaration) =
            kotlin_keyword_declaration(code, code_start, keyword, symbol_type)
        {
            declarations.push(declaration);
        }
    }
    if let Some(declaration) = kotlin_enum_declaration(code, code_start) {
        declarations.push(declaration);
    }
    if let Some(declaration) = kotlin_record_like_declaration(code, code_start) {
        declarations.push(declaration);
    }
    if declarations.is_empty() {
        if let Some(declaration) = kotlin_function_declaration(code, code_start) {
            declarations.push(declaration);
        }
    }

    declarations
}

fn kotlin_line_code(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//")
        || trimmed.starts_with('*')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('@')
    {
        ""
    } else {
        trimmed.split("//").next().unwrap_or(trimmed).trim_end()
    }
}

fn kotlin_package_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code.strip_prefix("package ")?;
    let name_start = code.len().saturating_sub(rest.len());
    let rest = rest.trim_start();
    let leading_ws = code[name_start..].len().saturating_sub(rest.len());
    let (name, name_len) = java_read_qualified_name(rest, false)?;
    let start_char = code_start + name_start + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Namespace,
        start_char,
        end_char: start_char + name_len,
    })
}

fn kotlin_import_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code.strip_prefix("import ")?;
    let rest = rest.trim_start();
    let leading_ws = code.len().saturating_sub(rest.len());
    let (name, name_len) = java_read_qualified_name(rest, true)?;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Import,
        start_char: code_start + leading_ws,
        end_char: code_start + leading_ws + name_len,
    })
}

fn kotlin_keyword_declaration(
    code: &str,
    code_start: usize,
    keyword: &str,
    symbol_type: SymbolType,
) -> Option<LineScannerDeclaration> {
    let keyword_start = java_find_keyword(code, keyword)?;
    let after_keyword = keyword_start + keyword.len();
    let rest = code[after_keyword..].trim_start();
    let leading_ws = code[after_keyword..].len().saturating_sub(rest.len());
    let (name, name_len) = java_read_identifier(rest)?;
    let start_char = code_start + after_keyword + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type,
        start_char,
        end_char: start_char + name_len,
    })
}

fn kotlin_enum_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let enum_start = java_find_keyword(code, "enum")?;
    let after_enum = enum_start + "enum".len();
    let rest = code[after_enum..].trim_start();
    let rest = rest.strip_prefix("class ")?;
    let leading_ws = code[after_enum..].len().saturating_sub(rest.len());
    let (name, name_len) = java_read_identifier(rest)?;
    let start_char = code_start + after_enum + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Enum,
        start_char,
        end_char: start_char + name_len,
    })
}

fn kotlin_record_like_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let keyword_start = java_find_keyword(code, "data")
        .or_else(|| java_find_keyword(code, "value"))
        .or_else(|| java_find_keyword(code, "sealed"))?;
    let after_keyword = keyword_start + code[keyword_start..].split_whitespace().next()?.len();
    let rest = code[after_keyword..].trim_start();
    let rest = rest.strip_prefix("class ")?;
    let leading_ws = code[after_keyword..].len().saturating_sub(rest.len());
    let (name, name_len) = java_read_identifier(rest)?;
    let start_char = code_start + after_keyword + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Struct,
        start_char,
        end_char: start_char + name_len,
    })
}

fn kotlin_function_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let keyword_start = java_find_keyword(code, "fun")?;
    let after_keyword = keyword_start + "fun".len();
    let mut rest = code[after_keyword..].trim_start();
    if let Some(generic_end) = rest
        .strip_prefix('<')
        .and_then(|generic| generic.find('>').map(|end| end + 1))
    {
        rest = rest[generic_end..].trim_start();
    }
    if let Some(receiver_split) = rest.find('.') {
        let paren_index = rest.find('(')?;
        if receiver_split < paren_index {
            rest = rest[receiver_split + 1..].trim_start();
        }
    }
    let leading_ws = code[after_keyword..].len().saturating_sub(rest.len());
    let (name, name_len) = java_read_identifier(rest)?;
    let symbol_type = if code_start > 0 {
        SymbolType::Method
    } else {
        SymbolType::Function
    };
    let start_char = code_start + after_keyword + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type,
        start_char,
        end_char: start_char + name_len,
    })
}

fn extract_ruby_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    extract_line_scanner_symbols(file_path, content, &RUBY_LEX, ruby_declarations_in_line)
}

fn ruby_declarations_in_line(line: &str) -> Vec<LineScannerDeclaration> {
    let code = ruby_line_code(line);
    if code.is_empty() {
        return Vec::new();
    }
    let code_start = line.find(code).unwrap_or(0);

    if let Some(declaration) = ruby_require_declaration(code, code_start) {
        return vec![declaration];
    }

    let mut declarations = Vec::new();
    if let Some(declaration) = ruby_module_declaration(code, code_start) {
        declarations.push(declaration);
    }
    if let Some(declaration) = ruby_class_declaration(code, code_start) {
        declarations.push(declaration);
    }
    if declarations.is_empty() {
        if let Some(declaration) = ruby_method_declaration(code, code_start) {
            declarations.push(declaration);
        }
    }

    declarations
}

fn ruby_line_code(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        ""
    } else {
        trimmed.split('#').next().unwrap_or(trimmed).trim_end()
    }
}

fn ruby_require_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code
        .strip_prefix("require_relative ")
        .or_else(|| code.strip_prefix("require "))?;
    let rest = rest.trim_start();
    let quote = rest.chars().next().filter(|ch| *ch == '"' || *ch == '\'')?;
    let name_start = rest.find(quote)? + quote.len_utf8();
    let after_quote = &rest[name_start..];
    let name_end = after_quote.find(quote)?;
    let name = after_quote[..name_end].to_string();
    let leading_ws = code.len().saturating_sub(rest.len());
    let start_char = code_start + leading_ws + name_start;
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Import,
        start_char,
        end_char: start_char + name_end,
    })
}

fn ruby_module_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    ruby_keyword_declaration(code, code_start, "module", SymbolType::Namespace)
}

fn ruby_class_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    ruby_keyword_declaration(code, code_start, "class", SymbolType::Class)
}

fn ruby_keyword_declaration(
    code: &str,
    code_start: usize,
    keyword: &str,
    symbol_type: SymbolType,
) -> Option<LineScannerDeclaration> {
    let keyword_start = java_find_keyword(code, keyword)?;
    let after_keyword = keyword_start + keyword.len();
    let rest = code[after_keyword..].trim_start();
    let leading_ws = code[after_keyword..].len().saturating_sub(rest.len());
    let (name, name_len) = ruby_read_qualified_name(rest)?;
    let start_char = code_start + after_keyword + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type,
        start_char,
        end_char: start_char + name_len,
    })
}

fn ruby_method_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code.strip_prefix("def ")?;
    let rest = rest.trim_start();
    let receiver_offset = rest.rfind('.').map_or(0, |index| index + 1);
    let method_text = &rest[receiver_offset..];
    let (name, name_len) = ruby_read_method_name(method_text)?;
    let leading_ws = code.len().saturating_sub(rest.len());
    let start_char = code_start + leading_ws + receiver_offset;
    Some(LineScannerDeclaration {
        name,
        symbol_type: if code_start > 0 {
            SymbolType::Method
        } else {
            SymbolType::Function
        },
        start_char,
        end_char: start_char + name_len,
    })
}

fn ruby_read_qualified_name(text: &str) -> Option<(String, usize)> {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if index == 0 && !(ch == '_' || ch.is_ascii_alphabetic()) {
            return None;
        }
        if ch == ':' || ch == '_' || ch.is_ascii_alphanumeric() {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    (end > 0).then(|| (text[..end].to_string(), end))
}

fn ruby_read_method_name(text: &str) -> Option<(String, usize)> {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if index == 0 && !(ch == '_' || ch.is_ascii_alphabetic()) {
            return None;
        }
        if ch == '_' || ch.is_ascii_alphanumeric() || (index > 0 && matches!(ch, '?' | '!' | '=')) {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    (end > 0).then(|| (text[..end].to_string(), end))
}

fn extract_shell_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    extract_line_scanner_symbols(file_path, content, &SHELL_LEX, shell_declarations_in_line)
}

fn shell_declarations_in_line(line: &str) -> Vec<LineScannerDeclaration> {
    let code = shell_line_code(line);
    if code.is_empty() {
        return Vec::new();
    }
    let code_start = line.find(code).unwrap_or(0);

    if let Some(declaration) = shell_source_declaration(code, code_start) {
        return vec![declaration];
    }
    shell_function_declaration(code, code_start)
        .into_iter()
        .collect()
}

fn shell_line_code(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        ""
    } else {
        trimmed.split('#').next().unwrap_or(trimmed).trim_end()
    }
}

fn shell_source_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code
        .strip_prefix("source ")
        .or_else(|| code.strip_prefix(". "))?;
    let rest = rest.trim_start();
    let (name, name_len) = shell_read_word(rest)?;
    let leading_ws = code.len().saturating_sub(rest.len());
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Import,
        start_char: code_start + leading_ws,
        end_char: code_start + leading_ws + name_len,
    })
}

fn shell_function_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    if let Some(rest) = code.strip_prefix("function ") {
        let rest = rest.trim_start();
        let (name, name_len) = shell_read_identifier(rest)?;
        let leading_ws = code.len().saturating_sub(rest.len());
        return Some(LineScannerDeclaration {
            name,
            symbol_type: SymbolType::Function,
            start_char: code_start + leading_ws,
            end_char: code_start + leading_ws + name_len,
        });
    }

    let paren_index = code.find("()")?;
    let before_paren = code[..paren_index].trim_end();
    let (name, name_start) = shell_read_identifier_before(before_paren)?;
    if matches!(
        name.as_str(),
        "if" | "for" | "while" | "case" | "until" | "then" | "do"
    ) {
        return None;
    }
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Function,
        start_char: code_start + name_start,
        end_char: code_start + before_paren.len(),
    })
}

fn shell_read_word(text: &str) -> Option<(String, usize)> {
    let quote = text.chars().next().filter(|ch| *ch == '"' || *ch == '\'');
    if let Some(quote) = quote {
        let after_quote = &text[quote.len_utf8()..];
        let end = after_quote.find(quote)?;
        return Some((after_quote[..end].to_string(), end + quote.len_utf8() * 2));
    }

    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if ch.is_whitespace() || ch == ';' {
            break;
        }
        end = index + ch.len_utf8();
    }
    (end > 0).then(|| (text[..end].to_string(), end))
}

fn shell_read_identifier(text: &str) -> Option<(String, usize)> {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if index == 0 && !(ch == '_' || ch.is_ascii_alphabetic()) {
            return None;
        }
        if ch == '_' || ch == '-' || ch.is_ascii_alphanumeric() {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    (end > 0).then(|| (text[..end].to_string(), end))
}

fn shell_read_identifier_before(text: &str) -> Option<(String, usize)> {
    let trimmed_len = text.trim_end().len();
    let trimmed = &text[..trimmed_len];
    let mut start = trimmed.len();
    for (index, ch) in trimmed.char_indices().rev() {
        if ch == '_' || ch == '-' || ch.is_ascii_alphanumeric() {
            start = index;
        } else {
            break;
        }
    }
    if start == trimmed.len() {
        return None;
    }
    let name = &trimmed[start..];
    name.chars()
        .next()
        .filter(|ch| *ch == '_' || ch.is_ascii_alphabetic())?;
    Some((name.to_string(), start))
}

fn extract_dockerfile_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    extract_line_scanner_symbols(
        file_path,
        content,
        &DOCKERFILE_LEX,
        dockerfile_declarations_in_line,
    )
}

fn dockerfile_declarations_in_line(line: &str) -> Vec<LineScannerDeclaration> {
    let code = dockerfile_line_code(line);
    if code.is_empty() {
        return Vec::new();
    }
    let code_start = line.find(code).unwrap_or(0);
    let mut parts = code.split_whitespace();
    let Some(instruction) = parts.next() else {
        return Vec::new();
    };
    let rest = code[instruction.len()..].trim_start();
    match instruction.to_ascii_uppercase().as_str() {
        "FROM" => dockerfile_from_declarations(rest, code_start + instruction.len()),
        "ARG" => dockerfile_key_declaration(rest, code, code_start, SymbolType::Constant),
        "ENV" => dockerfile_key_declaration(rest, code, code_start, SymbolType::Constant),
        "EXPOSE" | "WORKDIR" => dockerfile_value_declaration(rest, code, code_start),
        "COPY" | "ADD" => dockerfile_from_flag_declaration(rest, code, code_start),
        _ => Vec::new(),
    }
}

fn dockerfile_line_code(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        ""
    } else {
        trimmed.split('#').next().unwrap_or(trimmed).trim_end()
    }
}

fn dockerfile_from_declarations(rest: &str, rest_start: usize) -> Vec<LineScannerDeclaration> {
    let mut declarations = Vec::new();
    if let Some((image, image_len)) = shell_read_word(rest) {
        let leading_ws = rest.len().saturating_sub(rest.trim_start().len());
        declarations.push(LineScannerDeclaration {
            name: image,
            symbol_type: SymbolType::Import,
            start_char: rest_start + leading_ws,
            end_char: rest_start + leading_ws + image_len,
        });
    }

    let words = rest.split_whitespace().collect::<Vec<_>>();
    for pair in words.windows(2) {
        if pair[0].eq_ignore_ascii_case("AS") {
            let stage = pair[1].trim_matches('"').to_string();
            if let Some(relative_start) = rest.find(pair[1]) {
                declarations.push(LineScannerDeclaration {
                    name: stage,
                    symbol_type: SymbolType::Module,
                    start_char: rest_start + relative_start,
                    end_char: rest_start + relative_start + pair[1].len(),
                });
            }
            break;
        }
    }

    declarations
}

fn dockerfile_key_declaration(
    rest: &str,
    code: &str,
    code_start: usize,
    symbol_type: SymbolType,
) -> Vec<LineScannerDeclaration> {
    let rest = rest.trim_start();
    let Some((name, name_len)) = dockerfile_read_key(rest) else {
        return Vec::new();
    };
    let leading_ws = code.len().saturating_sub(rest.len());
    vec![LineScannerDeclaration {
        name,
        symbol_type,
        start_char: code_start + leading_ws,
        end_char: code_start + leading_ws + name_len,
    }]
}

fn dockerfile_value_declaration(
    rest: &str,
    code: &str,
    code_start: usize,
) -> Vec<LineScannerDeclaration> {
    let rest = rest.trim_start();
    let Some((name, name_len)) = shell_read_word(rest) else {
        return Vec::new();
    };
    let leading_ws = code.len().saturating_sub(rest.len());
    vec![LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Property,
        start_char: code_start + leading_ws,
        end_char: code_start + leading_ws + name_len,
    }]
}

fn dockerfile_from_flag_declaration(
    rest: &str,
    code: &str,
    code_start: usize,
) -> Vec<LineScannerDeclaration> {
    let Some(flag_start) = rest.find("--from=") else {
        return Vec::new();
    };
    let value_start = flag_start + "--from=".len();
    let value = &rest[value_start..];
    let Some((name, name_len)) = shell_read_word(value) else {
        return Vec::new();
    };
    let leading_ws = code.len().saturating_sub(rest.len());
    vec![LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Import,
        start_char: code_start + leading_ws + value_start,
        end_char: code_start + leading_ws + value_start + name_len,
    }]
}

fn dockerfile_read_key(text: &str) -> Option<(String, usize)> {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if ch.is_whitespace() || ch == '=' {
            break;
        }
        end = index + ch.len_utf8();
    }
    (end > 0).then(|| (text[..end].to_string(), end))
}

fn extract_sql_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    extract_line_scanner_symbols(file_path, content, &SQL_LEX, sql_declarations_in_line)
}

fn sql_declarations_in_line(line: &str) -> Vec<LineScannerDeclaration> {
    let code = sql_line_code(line);
    if code.is_empty() {
        return Vec::new();
    }
    let code_start = line.find(code).unwrap_or(0);

    if let Some(declaration) = sql_include_declaration(code, code_start) {
        return vec![declaration];
    }
    sql_create_declaration(code, code_start)
        .into_iter()
        .collect()
}

fn sql_line_code(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with("--") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
        ""
    } else {
        trimmed.split("--").next().unwrap_or(trimmed).trim_end()
    }
}

fn sql_include_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let rest = code
        .strip_prefix("\\i ")
        .or_else(|| code.strip_prefix("\\include "))
        .or_else(|| code.strip_prefix(".read "))?;
    let rest = rest.trim_start();
    let (name, name_len) = shell_read_word(rest)?;
    let leading_ws = code.len().saturating_sub(rest.len());
    Some(LineScannerDeclaration {
        name,
        symbol_type: SymbolType::Import,
        start_char: code_start + leading_ws,
        end_char: code_start + leading_ws + name_len,
    })
}

fn sql_create_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let create_start = sql_find_keyword(code, "create")?;
    let mut cursor = create_start + "create".len();
    let (kind, kind_end) = loop {
        let (word, _start, end) = sql_read_word_at(code, cursor)?;
        let normalized = word.to_ascii_lowercase();
        cursor = end;
        if matches!(
            normalized.as_str(),
            "or" | "replace" | "temporary" | "temp" | "unlogged" | "unique" | "materialized"
        ) {
            continue;
        }
        if matches!(
            normalized.as_str(),
            "schema" | "table" | "view" | "function" | "procedure" | "trigger" | "index"
        ) {
            break (normalized, end);
        }
        return None;
    };

    cursor = sql_skip_if_not_exists(code, kind_end);
    let (name, name_start, name_end) = sql_read_identifier_at(code, cursor)?;
    let symbol_type = match kind.as_str() {
        "schema" => SymbolType::Namespace,
        "table" => SymbolType::Struct,
        "view" => SymbolType::Type,
        "function" | "procedure" => SymbolType::Function,
        "trigger" => SymbolType::Method,
        "index" => SymbolType::Property,
        _ => return None,
    };

    Some(LineScannerDeclaration {
        name,
        symbol_type,
        start_char: code_start + name_start,
        end_char: code_start + name_end,
    })
}

fn sql_skip_if_not_exists(code: &str, cursor: usize) -> usize {
    let Some((first, _first_start, first_end)) = sql_read_word_at(code, cursor) else {
        return cursor;
    };
    if !first.eq_ignore_ascii_case("if") {
        return cursor;
    }
    let Some((second, _second_start, second_end)) = sql_read_word_at(code, first_end) else {
        return cursor;
    };
    if !second.eq_ignore_ascii_case("not") {
        return cursor;
    }
    let Some((third, _third_start, third_end)) = sql_read_word_at(code, second_end) else {
        return cursor;
    };
    if third.eq_ignore_ascii_case("exists") {
        third_end
    } else {
        cursor
    }
}

fn sql_find_keyword(code: &str, keyword: &str) -> Option<usize> {
    let lower = code.to_ascii_lowercase();
    java_find_keyword(&lower, keyword)
}

fn sql_read_word_at(text: &str, cursor: usize) -> Option<(String, usize, usize)> {
    let rest = text.get(cursor..)?;
    let leading_ws = rest.len().saturating_sub(rest.trim_start().len());
    let start = cursor + leading_ws;
    let rest = text.get(start..)?;
    let mut end = 0usize;
    for (index, ch) in rest.char_indices() {
        if ch == '_' || ch == '$' || ch.is_ascii_alphanumeric() {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    (end > 0).then(|| (rest[..end].to_string(), start, start + end))
}

fn sql_read_identifier_at(text: &str, cursor: usize) -> Option<(String, usize, usize)> {
    let rest = text.get(cursor..)?;
    let leading_ws = rest.len().saturating_sub(rest.trim_start().len());
    let start = cursor + leading_ws;
    let rest = text.get(start..)?;
    if let Some(after_quote) = rest.strip_prefix('"') {
        let end = after_quote.find('"')? + 2;
        return Some((after_quote[..end - 2].to_string(), start, start + end));
    }

    let mut end = 0usize;
    for (index, ch) in rest.char_indices() {
        if ch == '_' || ch == '$' || ch == '.' || ch.is_ascii_alphanumeric() {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    (end > 0).then(|| {
        (
            rest[..end].trim_end_matches('.').to_string(),
            start,
            start + end,
        )
    })
}

fn extract_build_script_symbols(file_path: &str, content: &str) -> Vec<Symbol> {
    extract_line_scanner_symbols(
        file_path,
        content,
        &BUILD_SCRIPT_LEX,
        build_script_declarations_in_line,
    )
}

fn build_script_declarations_in_line(line: &str) -> Vec<LineScannerDeclaration> {
    let code = build_script_line_code(line);
    if code.is_empty() {
        return Vec::new();
    }
    let code_start = line.find(code).unwrap_or(0);

    if let Some(declaration) = cmake_declaration(code, code_start) {
        return vec![declaration];
    }
    make_declaration(code, code_start).into_iter().collect()
}

fn build_script_line_code(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with('\t') {
        ""
    } else {
        trimmed.split('#').next().unwrap_or(trimmed).trim_end()
    }
}

fn cmake_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    let paren_index = code.find('(')?;
    let command = code[..paren_index].trim();
    if command.is_empty() || command.chars().any(char::is_whitespace) {
        return None;
    }
    let rest = code[paren_index + 1..].trim_start();
    let command_lower = command.to_ascii_lowercase();
    let symbol_type = match command_lower.as_str() {
        "project" => SymbolType::Module,
        "include" => SymbolType::Import,
        "set" | "option" => SymbolType::Constant,
        "function" | "macro" | "add_executable" => SymbolType::Function,
        "add_library" => SymbolType::Struct,
        _ => return None,
    };
    let (name, name_len) = build_script_read_word(rest)?;
    let leading_ws = code[paren_index + 1..].len().saturating_sub(rest.len());
    let start_char = code_start + paren_index + 1 + leading_ws;
    Some(LineScannerDeclaration {
        name,
        symbol_type,
        start_char,
        end_char: start_char + name_len,
    })
}

fn make_declaration(code: &str, code_start: usize) -> Option<LineScannerDeclaration> {
    if let Some(rest) = code
        .strip_prefix("include ")
        .or_else(|| code.strip_prefix("-include "))
        .or_else(|| code.strip_prefix("sinclude "))
    {
        let rest = rest.trim_start();
        let (name, name_len) = build_script_read_word(rest)?;
        let leading_ws = code.len().saturating_sub(rest.len());
        return Some(LineScannerDeclaration {
            name,
            symbol_type: SymbolType::Import,
            start_char: code_start + leading_ws,
            end_char: code_start + leading_ws + name_len,
        });
    }

    if let Some((operator_start, _operator_len)) = make_assignment_operator(code) {
        let name = code[..operator_start].trim();
        if !name.is_empty() && name.chars().all(|ch| !ch.is_whitespace()) {
            let leading_ws = code.len().saturating_sub(code.trim_start().len());
            return Some(LineScannerDeclaration {
                name: name.to_string(),
                symbol_type: SymbolType::Constant,
                start_char: code_start + leading_ws,
                end_char: code_start + operator_start,
            });
        }
    }

    let colon_index = code.find(':')?;
    let target = code[..colon_index].trim();
    if target.is_empty()
        || target.starts_with('.')
        || target.contains('=')
        || target.contains('$')
        || target.contains('%')
    {
        return None;
    }
    let first_target = target.split_whitespace().next()?;
    let start_char = code_start + code.find(first_target).unwrap_or(0);
    Some(LineScannerDeclaration {
        name: first_target.to_string(),
        symbol_type: SymbolType::Function,
        start_char,
        end_char: start_char + first_target.len(),
    })
}

fn make_assignment_operator(code: &str) -> Option<(usize, usize)> {
    [":=", "?=", "+=", "="]
        .iter()
        .filter_map(|operator| code.find(operator).map(|index| (index, operator.len())))
        .min_by_key(|(index, _len)| *index)
}

fn build_script_read_word(text: &str) -> Option<(String, usize)> {
    let quote = text.chars().next().filter(|ch| *ch == '"' || *ch == '\'');
    if let Some(quote) = quote {
        let after_quote = &text[quote.len_utf8()..];
        let end = after_quote.find(quote)?;
        return Some((after_quote[..end].to_string(), end + quote.len_utf8() * 2));
    }

    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if ch.is_whitespace() || matches!(ch, ')' | ';' | ',') {
            break;
        }
        end = index + ch.len_utf8();
    }
    (end > 0).then(|| (text[..end].to_string(), end))
}

fn push_line_scanner_symbol(
    symbols: &mut Vec<Symbol>,
    seen: &mut HashSet<(String, SymbolType, u32)>,
    file_path: &str,
    declaration: LineScannerDeclaration,
    line_number: u32,
    line_start_byte: usize,
    line_text: &str,
) {
    if declaration.name.is_empty()
        || !seen.insert((
            declaration.name.clone(),
            declaration.symbol_type,
            line_number,
        ))
    {
        return;
    }

    symbols.push(Symbol {
        id: format!(
            "{}::{}#{}",
            file_path, declaration.name, declaration.symbol_type
        ),
        name: declaration.name.clone(),
        qualified_name: declaration.name,
        symbol_type: declaration.symbol_type,
        file_path: file_path.to_string(),
        range: Range {
            start: Position::new(line_number, declaration.start_char as u32),
            end: Position::new(line_number, declaration.end_char as u32),
        },
        byte_offset: line_start_byte.saturating_add(declaration.start_char),
        byte_length: declaration.end_char.saturating_sub(declaration.start_char),
        parent_id: None,
        docstring: None,
        signature: Some(line_text.trim().to_string()),
        content_hash: compute_hash(line_text),
    });
}

fn line_start_offsets(content: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut byte_offset = 0usize;
    for segment in content.split_inclusive('\n') {
        offsets.push(byte_offset);
        byte_offset += segment.len();
    }
    if content.is_empty() {
        offsets.push(0);
    }
    offsets
}

fn css_next_brace_depth(current_depth: i32, line: &str) -> i32 {
    let opens = line.bytes().filter(|byte| *byte == b'{').count() as i32;
    let closes = line.bytes().filter(|byte| *byte == b'}').count() as i32;
    current_depth
        .saturating_add(opens)
        .saturating_sub(closes)
        .max(0)
}

fn push_css_symbol(
    symbols: &mut Vec<Symbol>,
    seen: &mut HashSet<(String, SymbolType, u32)>,
    file_path: &str,
    name: String,
    symbol_type: SymbolType,
    line_number: u32,
    start_char: usize,
    end_char: usize,
    line_start_byte: usize,
    line_text: &str,
) {
    if name.len() < 2 || !seen.insert((name.clone(), symbol_type, line_number)) {
        return;
    }

    let start_char_u32 = start_char as u32;
    let end_char_u32 = end_char.max(start_char + name.len()) as u32;
    let qualified_name = name.clone();
    let byte_offset = line_start_byte.saturating_add(start_char);
    let byte_length = end_char.saturating_sub(start_char).max(name.len());

    symbols.push(Symbol {
        id: format!("{}::{}#{}", file_path, qualified_name, symbol_type),
        name,
        qualified_name,
        symbol_type,
        file_path: file_path.to_string(),
        range: Range {
            start: Position::new(line_number, start_char_u32),
            end: Position::new(line_number, end_char_u32),
        },
        byte_offset,
        byte_length,
        parent_id: None,
        docstring: None,
        signature: None,
        content_hash: compute_hash(line_text),
    });
}

fn css_custom_properties_in_line(line: &str) -> Vec<(String, usize, usize)> {
    let mut values = Vec::new();
    let bytes = line.as_bytes();
    let mut index = 0usize;
    while index + 2 < bytes.len() {
        if bytes[index] == b'-' && bytes[index + 1] == b'-' {
            let start = index;
            index += 2;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric()
                    || bytes[index] == b'-'
                    || bytes[index] == b'_')
            {
                index += 1;
            }
            if index > start + 3 && line[index..].trim_start().starts_with(':') {
                values.push((line[start..index].to_string(), start, index));
            }
        } else {
            index += 1;
        }
    }
    values
}

fn css_layer_in_line(line: &str) -> Option<(String, usize, usize)> {
    let marker = "@layer";
    let marker_index = line.find(marker)?;
    let mut start = marker_index + marker.len();
    while start < line.len() && line.as_bytes()[start].is_ascii_whitespace() {
        start += 1;
    }
    if start >= line.len() || line.as_bytes()[start] == b'{' || line.as_bytes()[start] == b';' {
        return None;
    }

    let mut end = start;
    while end < line.len() {
        let byte = line.as_bytes()[end];
        if byte == b'{' || byte == b';' || byte == b',' || byte.is_ascii_whitespace() {
            break;
        }
        end += 1;
    }
    (end > start).then(|| (line[start..end].trim().to_string(), start, end))
}

fn css_at_rule_anchor_in_line(line: &str) -> Option<(String, usize, usize)> {
    let trimmed = line.trim_start();
    let indent = line.len().saturating_sub(trimmed.len());
    for marker in ["@media", "@container"] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            let rest_start = indent + marker.len();
            let query_end = rest
                .find('{')
                .or_else(|| rest.find(';'))
                .map(|index| rest_start + index)
                .unwrap_or(line.len());
            let query = line[rest_start..query_end].trim();
            if query.is_empty() {
                return Some((marker.to_string(), indent, indent + marker.len()));
            }
            return Some((format!("{} {}", marker, query), indent, query_end));
        }
    }
    None
}

fn css_font_face_starts_in_line(line: &str) -> bool {
    line.trim_start().starts_with("@font-face")
}

fn css_font_family_in_line(line: &str) -> Option<(String, usize, usize)> {
    let property_index = line.find("font-family")?;
    let colon_index = line[property_index..].find(':')? + property_index;
    let mut start = colon_index + 1;
    while start < line.len()
        && (line.as_bytes()[start].is_ascii_whitespace()
            || line.as_bytes()[start] == b'\''
            || line.as_bytes()[start] == b'"')
    {
        start += 1;
    }
    let mut end = start;
    while end < line.len() {
        let byte = line.as_bytes()[end];
        if byte == b';' || byte == b'\'' || byte == b'"' {
            break;
        }
        end += 1;
    }
    let name = line[start..end].trim();
    (!name.is_empty()).then(|| (name.to_string(), start, end))
}

fn css_keyframes_in_line(line: &str) -> Option<(String, usize, usize)> {
    let trimmed_start = line.find("@keyframes")?;
    let after = trimmed_start + "@keyframes".len();
    let name_start = after
        + line[after..]
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .count();
    let mut name_end = name_start;
    for ch in line[name_start..].chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            name_end += ch.len_utf8();
        } else {
            break;
        }
    }
    (name_end > name_start).then(|| (line[name_start..name_end].to_string(), name_start, name_end))
}

fn css_selectors_in_text(selector_text: &str) -> Vec<(String, usize, usize)> {
    let bytes = selector_text.as_bytes();
    let mut values = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let marker = bytes[index];
        if marker != b'.' && marker != b'#' {
            index += 1;
            continue;
        }
        if index > 0 {
            let previous = bytes[index - 1];
            if previous.is_ascii_alphanumeric() || previous == b'-' || previous == b'_' {
                index += 1;
                continue;
            }
        }

        let start = index;
        index += 1;
        while index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric()
                || bytes[index] == b'-'
                || bytes[index] == b'_')
        {
            index += 1;
        }
        if index > start + 1 {
            values.push((selector_text[start..index].to_string(), start, index));
        }
    }
    values
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
    if Language::capability_for_path(file_path).is_some() {
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

fn direct_relationship_score(relationship_type: SymbolRelationshipType) -> u32 {
    match relationship_type {
        SymbolRelationshipType::Call => 92,
        SymbolRelationshipType::Extends | SymbolRelationshipType::Implements => 88,
        SymbolRelationshipType::Contains => 82,
        SymbolRelationshipType::Export => 78,
        SymbolRelationshipType::Usage => 76,
        SymbolRelationshipType::Import => 74,
        SymbolRelationshipType::Handles => 73,
        SymbolRelationshipType::UsesType => 72,
        SymbolRelationshipType::ReadsEnv => 70,
    }
}

fn related_identifier_tokens(symbol: &Symbol) -> HashSet<String> {
    let mut tokens = identifier_tokens(&symbol.name);
    if !symbol.qualified_name.is_empty() {
        tokens.extend(identifier_tokens(&symbol.qualified_name));
    }
    tokens
}

fn identifier_tokens(text: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut current = String::new();
    let mut previous_was_lower_or_digit = false;
    let mut previous_was_upper = false;

    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if !ch.is_ascii_alphanumeric() {
            push_identifier_token(&mut tokens, &mut current);
            previous_was_lower_or_digit = false;
            previous_was_upper = false;
            continue;
        }

        if ch.is_ascii_uppercase() && !current.is_empty() {
            if previous_was_lower_or_digit
                || (previous_was_upper
                    && chars.peek().is_some_and(|next| next.is_ascii_lowercase()))
            {
                push_identifier_token(&mut tokens, &mut current);
            }
        }

        current.push(ch.to_ascii_lowercase());
        previous_was_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        previous_was_upper = ch.is_ascii_uppercase();
    }

    push_identifier_token(&mut tokens, &mut current);
    tokens
}

fn push_identifier_token(tokens: &mut HashSet<String>, current: &mut String) {
    if current.len() >= 2 {
        tokens.insert(std::mem::take(current));
    } else {
        current.clear();
    }
}

fn lexical_related_score(seed_tokens: &HashSet<String>, candidate_tokens: &HashSet<String>) -> f32 {
    if seed_tokens.is_empty() || candidate_tokens.is_empty() {
        return 0.0;
    }

    let shared_count = seed_tokens.intersection(candidate_tokens).count();
    if shared_count == 0 {
        return 0.0;
    }

    if seed_tokens.len() > 1 && shared_count < 2 {
        return 0.0;
    }

    let union_count = seed_tokens.union(candidate_tokens).count();
    if union_count == 0 {
        return 0.0;
    }

    shared_count as f32 / union_count as f32
}

fn is_nearby_related_file(seed_path: &str, candidate_path: &str) -> bool {
    if seed_path == candidate_path {
        return true;
    }

    let seed = Path::new(seed_path);
    let candidate = Path::new(candidate_path);
    if seed.parent() == candidate.parent() {
        return true;
    }

    seed.file_stem().is_some_and(|seed_stem| {
        candidate
            .file_stem()
            .is_some_and(|candidate_stem| seed_stem == candidate_stem)
    })
}

fn nearby_file_rank(seed_path: &str, candidate_path: &str) -> u8 {
    if seed_path == candidate_path {
        return 0;
    }

    let seed = Path::new(seed_path);
    let candidate = Path::new(candidate_path);
    if seed.file_stem().is_some_and(|seed_stem| {
        candidate
            .file_stem()
            .is_some_and(|candidate_stem| seed_stem == candidate_stem)
    }) {
        return 1;
    }

    if seed.parent() == candidate.parent() {
        return 2;
    }

    3
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
    use crate::tree_sitter::{extract_symbol_relationships, extract_symbols};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_test_service() -> (LanguageService, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("symbols.db");
        let store = Arc::new(SymbolStore::new(&db_path).unwrap());
        let service = LanguageService::new(temp_dir.path().to_path_buf(), store).unwrap();
        (service, temp_dir)
    }

    /// Regression (M5.11): a cyclic module re-export graph must terminate.
    ///
    /// `export * from './x'` resolution indexes the target module, re-entering
    /// `index_file` → enrich → resolve → index_file. Two files that re-export each
    /// other (JS barrel files, Python `__init__` packages — both pervasive in e.g.
    /// Firefox) used to recurse without bound: a stack overflow on small worker
    /// stacks, a 100% CPU "stuck on one file" spin on the 256 MiB worker stacks.
    /// The per-thread re-entrancy guard in `index_file_with_timings` breaks it.
    #[test]
    fn cyclic_module_reexport_terminates() {
        let (service, temp_dir) = create_test_service();
        fs::write(
            temp_dir.path().join("a.js"),
            "export * from './b';\nexport const fromA = 1;\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("b.js"),
            "export * from './a';\nexport const fromB = 2;\n",
        )
        .unwrap();

        // Without the guard, either of these never returns (overflow / spin).
        let symbols_a = service
            .index_file("a.js")
            .expect("indexing a.js must terminate");
        assert!(
            symbols_a.iter().any(|s| s.name == "fromA"),
            "a.js's own symbol must still be extracted"
        );
        let symbols_b = service
            .index_file("b.js")
            .expect("indexing b.js must terminate");
        assert!(symbols_b.iter().any(|s| s.name == "fromB"));
    }

    /// Regression (M5.11): the degenerate self-cycle (a file re-exporting itself)
    /// must also terminate.
    #[test]
    fn self_module_reexport_terminates() {
        let (service, temp_dir) = create_test_service();
        fs::write(
            temp_dir.path().join("a.js"),
            "export * from './a';\nexport const fromA = 1;\n",
        )
        .unwrap();
        let symbols = service
            .index_file("a.js")
            .expect("self re-export must terminate");
        assert!(symbols.iter().any(|s| s.name == "fromA"));
    }

    /// Regression (M5.12): a YAML config file that `serde_yaml` cannot parse must
    /// index without spinning. `serde_yaml`'s document `Deserializer` re-yields the
    /// same `Err` on every poll, so a `continue`-on-error loop is infinite. Firefox's
    /// `StaticPrefList.yaml` carries `value: @IS_XP_MACOSX@` (a build-time placeholder;
    /// `@` is a reserved YAML indicator → a hard parse error) and pinned a core at
    /// 100% CPU. The config collectors now `break` on the first parse error.
    #[test]
    fn unparseable_yaml_config_terminates() {
        let (service, temp_dir) = create_test_service();
        fs::write(
            temp_dir.path().join("prefs.yaml"),
            "- name: accessibility.tabfocus\n  type: int32_t\n  value: 7\n  mirror: always\n\
             \n- name: accessibility.tabfocus_applies_to_xul\n  type: bool\n  value: @IS_XP_MACOSX@\n  mirror: always\n",
        )
        .unwrap();
        // Before the fix this never returned (100% CPU spin in the Deserializer loop).
        service
            .index_file("prefs.yaml")
            .expect("unparseable YAML must index without hanging");
    }

    /// M5.1 receiver-type dispatch (the "type_slice"). Each test indexes a single
    /// file whose `run` method name is AMBIGUOUS (defined on two classes), so the
    /// resolver MUST use the receiver type to pick the right target — otherwise it
    /// bails to NULL exactly as before. Named `receiver_type_slice_*` inside module
    /// `resolution_slice` so `cargo test --lib {resolution_slice,receiver_type,
    /// type_slice}` all select them.
    mod resolution_slice {
        use super::*;

        /// Find a freshly-extracted symbol by its exact qualified name.
        fn sym<'a>(symbols: &'a [Symbol], qn: &str) -> &'a Symbol {
            symbols
                .iter()
                .find(|s| s.qualified_name == qn)
                .unwrap_or_else(|| panic!("no symbol with qualified_name {qn:?}"))
        }

        /// The set of RESOLVED target ids of call edges named `target_name` leaving
        /// `source` (a method/function symbol).
        fn resolved_call_targets(
            service: &LanguageService,
            source: &Symbol,
            target_name: &str,
        ) -> std::collections::HashSet<String> {
            let graph = service
                .get_symbol_graph(source, SymbolRelationshipType::Call, 50)
                .unwrap();
            graph
                .outgoing
                .iter()
                .filter(|edge| {
                    edge.relationship_type == SymbolRelationshipType::Call
                        && edge.target_name == target_name
                })
                .filter_map(|edge| edge.target_symbol_id.clone())
                .collect()
        }

        fn index_source(file: &str, source: &str) -> (LanguageService, TempDir, Vec<Symbol>) {
            let (service, temp_dir) = create_test_service();
            fs::write(temp_dir.path().join(file), source).unwrap();
            let symbols = if file.ends_with(".rs") {
                fs::write(
                    temp_dir.path().join("Cargo.toml"),
                    format!(
                        "[package]\nname = \"qualified-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"{file}\"\n"
                    ),
                )
                .unwrap();
                service.index_directory("").unwrap();
                service.get_file_symbols(file).unwrap()
            } else {
                service.index_file(file).unwrap()
            };
            (service, temp_dir, symbols)
        }

        /// M5.1b END-TO-END: extraction → store → M2.4 back-fill → GLOBAL mining.
        /// `Dog(Animal)` lives in a different file from `Animal`; inside `Dog.bark`
        /// the `self.speak()` call captures recv_type `Dog`, but `speak` is NOT a
        /// same-file/imported candidate, so the per-file resolver leaves it NULL.
        /// A decoy `Plant.speak` makes the name ambiguous so the M2.4 name-only
        /// back-fill cannot resolve it either — only the cross-file supertype walk
        /// can. After the two global passes it resolves to `Animal.speak` (NOT the
        /// decoy `Plant.speak`), proving the registry + recv_type mine win.
        #[test]
        fn receiver_global_cross_file_inheritance_mines_supertype_call() {
            let (service, temp_dir) = create_test_service();
            fs::write(
                temp_dir.path().join("base.py"),
                "class Animal:\n    def speak(self):\n        return 1\n",
            )
            .unwrap();
            fs::write(
                temp_dir.path().join("decoy.py"),
                "class Plant:\n    def speak(self):\n        return 9\n",
            )
            .unwrap();
            fs::write(
                temp_dir.path().join("derived.py"),
                "from base import Animal\n\n\nclass Dog(Animal):\n    def bark(self):\n        return self.speak()\n",
            )
            .unwrap();

            let base_syms = service.index_file("base.py").unwrap();
            let _decoy_syms = service.index_file("decoy.py").unwrap();
            let derived_syms = service.index_file("derived.py").unwrap();

            let dog_bark = sym(&derived_syms, "Dog.bark");
            let animal_speak = sym(&base_syms, "Animal.speak");

            // Per-file + ambiguous name → speak() is NULL before the global passes.
            let before = resolved_call_targets(&service, dog_bark, "speak");
            assert!(
                !before.contains(&animal_speak.id),
                "speak() must be unresolved before the global mining pass; got {before:?}"
            );

            service
                .symbol_store
                .backfill_unresolved_relationship_targets()
                .unwrap();
            service
                .symbol_store
                .mine_receiver_type_relationship_targets()
                .unwrap();
            service
                .symbol_store
                .mine_go_interface_implementations()
                .unwrap();

            let after = resolved_call_targets(&service, dog_bark, "speak");
            assert!(
                after.contains(&animal_speak.id),
                "self.speak() in Dog(Animal) must mine-resolve to Animal.speak; got {after:?}"
            );
        }

        /// M5.1b PRECISION BLOCKER, the reviewer's EXACT scenario, end-to-end:
        /// `from pathlib import Path` … `def use(p: Path): return p.compute()`, and a
        /// PROJECT `class Path: def compute(self)` in another file. The `p: Path`
        /// receiver type is a SIMPLE NAME inferred from the annotation (NOT `self`),
        /// so extraction tags it `recv_self = false`. The correct answer for
        /// `p.compute()` is NULL (`p` is the library `pathlib.Path`).
        ///
        /// A decoy `Other.compute` makes the method name `compute` AMBIGUOUS so the
        /// M2.4 name-only back-fill cannot resolve it — the ONLY pass that could is
        /// the GLOBAL receiver-type mining, which is exactly where the reviewer's
        /// blocker lived (the pre-fix miner resolved recv_type `Path` to the unique
        /// project class `Path` and tagged `p.compute()` as `Path.compute`). After
        /// the fix the provenance gate defers the non-self recv_type, so the edge
        /// stays NULL through the FULL pipeline.
        #[test]
        fn null_mining_library_typed_param_does_not_resolve_to_project_class() {
            let (service, temp_dir) = create_test_service();
            fs::write(
                temp_dir.path().join("model.py"),
                "class Path:\n    def compute(self):\n        return 1\n",
            )
            .unwrap();
            // Decoy: a second `compute` so the name is NOT globally unique → the
            // M2.4 back-fill leaves `p.compute()` NULL, isolating the mining pass.
            fs::write(
                temp_dir.path().join("decoy.py"),
                "class Other:\n    def compute(self):\n        return 9\n",
            )
            .unwrap();
            fs::write(
                temp_dir.path().join("user.py"),
                "from pathlib import Path\n\n\ndef use(p: Path):\n    return p.compute()\n",
            )
            .unwrap();

            let model_syms = service.index_file("model.py").unwrap();
            let _decoy_syms = service.index_file("decoy.py").unwrap();
            let user_syms = service.index_file("user.py").unwrap();

            let use_fn = sym(&user_syms, "use");
            let project_compute = sym(&model_syms, "Path.compute");

            // Run BOTH global passes — the edge must STILL be NULL afterwards.
            service
                .symbol_store
                .backfill_unresolved_relationship_targets()
                .unwrap();
            service
                .symbol_store
                .mine_receiver_type_relationship_targets()
                .unwrap();
            service
                .symbol_store
                .mine_go_interface_implementations()
                .unwrap();

            let after = resolved_call_targets(&service, use_fn, "compute");
            assert!(
                !after.contains(&project_compute.id),
                "p.compute() on a library-typed `p: Path` param must NOT mine-resolve \
                 to the project Path.compute; got {after:?}"
            );
            // The receiver IS a library type; with `compute` ambiguous nothing should
            // confidently resolve it (the pre-fix miner wrongly tagged it).
            assert!(
                after.is_empty(),
                "no compute() target should be resolved for the library-typed param; got {after:?}"
            );
        }

        // A Python file where `run` is defined on BOTH `A` and `B`; `A.go` calls
        // `self.run()` (→ A), `b = B(); b.run()` (→ B), `unique_helper()` (unique),
        // and `loose(y)` calls `y.run()` on an UNTYPED param (unknown receiver).
        const AMBIG_PY: &str = "\
class A:
    def run(self):
        return 1

    def go(self):
        self.run()
        b = B()
        b.run()
        unique_helper()

class B:
    def run(self):
        return 2

def unique_helper():
    return 0

def loose(y):
    y.run()
";

        #[test]
        fn receiver_type_slice_resolves_self_to_enclosing_class_method() {
            let (service, _tmp, symbols) = index_source("recv.py", AMBIG_PY);
            let go = sym(&symbols, "A.go");
            let a_run = sym(&symbols, "A.run");
            let b_run = sym(&symbols, "B.run");

            let resolved = resolved_call_targets(&service, go, "run");
            assert!(
                resolved.contains(&a_run.id),
                "self.run() must resolve to A.run via the enclosing class; got {resolved:?}"
            );
            // And NOT to B.run — `self` is an `A`.
            assert!(
                !(resolved.len() == 1 && resolved.contains(&b_run.id)),
                "self.run() must not resolve to B.run"
            );
        }

        #[test]
        fn receiver_type_slice_resolves_this_in_typescript() {
            const TS: &str = "\
class A {
  run(): number {
    return 1;
  }
  go(): number {
    return this.run();
  }
}
class B {
  run(): number {
    return 2;
  }
}
";
            let (service, _tmp, symbols) = index_source("recv.ts", TS);
            let go = sym(&symbols, "A.go");
            let a_run = sym(&symbols, "A.run");

            let resolved = resolved_call_targets(&service, go, "run");
            assert!(
                resolved.contains(&a_run.id),
                "this.run() must resolve to A.run; got {resolved:?}"
            );
        }

        #[test]
        fn receiver_type_slice_resolves_constructor_typed_local() {
            let (service, _tmp, symbols) = index_source("recv.py", AMBIG_PY);
            let go = sym(&symbols, "A.go");
            let b_run = sym(&symbols, "B.run");

            let resolved = resolved_call_targets(&service, go, "run");
            assert!(
                resolved.contains(&b_run.id),
                "b.run() where `b = B()` must resolve to B.run; got {resolved:?}"
            );
        }

        #[test]
        fn receiver_type_slice_disambiguates_same_method_name_across_classes() {
            let (service, _tmp, symbols) = index_source("recv.py", AMBIG_PY);
            let go = sym(&symbols, "A.go");
            let a_run = sym(&symbols, "A.run");
            let b_run = sym(&symbols, "B.run");

            let resolved = resolved_call_targets(&service, go, "run");
            // EXACTLY the two correct, distinct targets — self→A.run, b→B.run.
            let expected: std::collections::HashSet<String> =
                [a_run.id.clone(), b_run.id.clone()].into_iter().collect();
            assert_eq!(
                resolved, expected,
                "the two `run` calls must resolve to A.run and B.run respectively"
            );
        }

        #[test]
        fn receiver_type_slice_unknown_receiver_stays_unresolved() {
            let (service, _tmp, symbols) = index_source("recv.py", AMBIG_PY);
            let loose = sym(&symbols, "loose");
            let a_run = sym(&symbols, "A.run");
            let b_run = sym(&symbols, "B.run");

            // `y` is untyped → recv_type Unknown → the ambiguous `run` falls through
            // to today's behavior: it must NOT be mis-resolved to either class.
            let resolved = resolved_call_targets(&service, loose, "run");
            assert!(
                !resolved.contains(&a_run.id) && !resolved.contains(&b_run.id),
                "unknown-receiver y.run() must not be mis-resolved; got {resolved:?}"
            );
        }

        #[test]
        fn receiver_type_slice_unique_named_call_is_unchanged() {
            // Regression guard: a uniquely-named call still resolves via the
            // untouched `len() == 1` path (no receiver type involved).
            let (service, _tmp, symbols) = index_source("recv.py", AMBIG_PY);
            let go = sym(&symbols, "A.go");
            let helper = sym(&symbols, "unique_helper");

            let resolved = resolved_call_targets(&service, go, "unique_helper");
            assert!(
                resolved.contains(&helper.id),
                "unique_helper() must still resolve to its single definition"
            );
        }

        #[test]
        fn receiver_type_slice_resolves_rust_self_and_constructor() {
            const RS: &str = "\
struct A;
struct B;

impl A {
    fn run(&self) -> i32 {
        1
    }
    fn go(&self) -> i32 {
        let b = B::new();
        let x = self.run();
        let y = b.run();
        x + y
    }
}

impl B {
    fn new() -> B {
        B
    }
    fn run(&self) -> i32 {
        2
    }
}
";
            let (service, _tmp, symbols) = index_source("recv.rs", RS);
            let go = sym(&symbols, "A::go");
            let a_run = sym(&symbols, "A::run");
            let b_run = sym(&symbols, "B::run");

            let resolved = resolved_call_targets(&service, go, "run");
            let expected: std::collections::HashSet<String> =
                [a_run.id.clone(), b_run.id.clone()].into_iter().collect();
            assert_eq!(
                resolved, expected,
                "Rust self.run()→A::run and (b = B::new()) b.run()→B::run"
            );
        }

        // ---- M5.1 CONSERVATISM probes (adversarial review) ------------------
        // Each mirrors a reviewer probe where the OLD typing was too eager and
        // produced a WRONG confidence-0.8 resolution. The fixed typing yields
        // `Unknown`, which falls through to today's resolver → the ambiguous
        // `run` call stays UNRESOLVED (never mis-resolved to the wrong class).
        // (Every fixture has `run` defined on BOTH `Widget` and `Gadget`, so only
        // a CORRECT receiver type could ever resolve it.)

        /// Raw (pre-resolution) extraction so a test can inspect `recv_type`.
        fn extract_raw(
            file: &str,
            source: &str,
            language: Language,
        ) -> (Vec<Symbol>, Vec<SymbolRelationship>) {
            let tree = parse_with_thread_local_parser(source, language).unwrap();
            let symbols = extract_symbols(&tree, source, language, file);
            let relationships =
                extract_symbol_relationships(&tree, source, language, file, &symbols);
            (symbols, relationships)
        }

        /// The `recv_type`s carried by `target_name` Call edges leaving `source_qn`.
        fn recv_types_for_call(
            symbols: &[Symbol],
            relationships: &[SymbolRelationship],
            source_qn: &str,
            target_name: &str,
        ) -> Vec<Option<String>> {
            let source = sym(symbols, source_qn);
            relationships
                .iter()
                .filter(|r| {
                    r.relationship_type == SymbolRelationshipType::Call
                        && r.source_symbol_id == source.id
                        && r.target_name == target_name
                })
                .map(|r| r.recv_type.clone())
                .collect()
        }

        // FIX 1: `x = Widget(); for x in gadgets(): x.run()` — the loop target
        // rebinds `x`, so its `Widget` type is stale. `x.run()` must NOT resolve to
        // `Widget.run`.
        #[test]
        fn receiver_type_slice_loop_rebind_drops_stale_type() {
            let source = "\
class Widget:
    def run(self):
        return 1

class Gadget:
    def run(self):
        return 2

def gadgets():
    return []

def loop_rebind():
    x = Widget()
    for x in gadgets():
        x.run()
";
            let (service, _tmp, symbols) = index_source("recv_loop.py", source);
            let loop_rebind = sym(&symbols, "loop_rebind");
            let widget_run = sym(&symbols, "Widget.run");

            let resolved = resolved_call_targets(&service, loop_rebind, "run");
            assert!(
                !resolved.contains(&widget_run.id),
                "loop-rebound x.run() must not resolve to the stale Widget.run; got {resolved:?}"
            );
            assert!(
                resolved.is_empty(),
                "the rebound x has no confident type → run stays unresolved; got {resolved:?}"
            );

            // And the captured receiver type is Unknown (no recv_type emitted).
            let (raw_syms, raw_rels) = extract_raw("recv_loop.py", source, Language::Python);
            assert_eq!(
                recv_types_for_call(&raw_syms, &raw_rels, "loop_rebind", "run"),
                vec![None],
                "loop-rebound receiver must carry no recv_type"
            );
        }

        // FIX 2: `if c: x = Widget() else: x = Gadget(); x.run()` — conflicting
        // arms with no CFG → ambiguous. `x.run()` must NOT resolve to either arm.
        #[test]
        fn receiver_type_slice_if_else_conflict_is_unresolved() {
            let source = "\
class Widget:
    def run(self):
        return 1

class Gadget:
    def run(self):
        return 2

def cond():
    return True

def if_else_conflict():
    if cond():
        x = Widget()
    else:
        x = Gadget()
    x.run()
";
            let (service, _tmp, symbols) = index_source("recv_if.py", source);
            let if_else = sym(&symbols, "if_else_conflict");

            let resolved = resolved_call_targets(&service, if_else, "run");
            assert!(
                resolved.is_empty(),
                "conflicting if/else x.run() must stay unresolved; got {resolved:?}"
            );

            let (raw_syms, raw_rels) = extract_raw("recv_if.py", source, Language::Python);
            assert_eq!(
                recv_types_for_call(&raw_syms, &raw_rels, "if_else_conflict", "run"),
                vec![None],
                "conflicting reassignment must poison the type to Unknown (no recv_type)"
            );
        }

        // FIX 3: `def outer(): w = Widget(); def inner(w): w.run()` — `inner`'s
        // untyped param `w` shadows the outer `Widget`. `w.run()` inside `inner`
        // must NOT resolve to the outer `Widget.run`.
        #[test]
        fn receiver_type_slice_inner_param_shadows_outer_type() {
            let source = "\
class Widget:
    def run(self):
        return 1

class Gadget:
    def run(self):
        return 2

def outer():
    w = Widget()
    def inner(w):
        w.run()
    return inner
";
            let (service, _tmp, symbols) = index_source("recv_shadow.py", source);
            let inner = sym(&symbols, "outer.inner");
            let widget_run = sym(&symbols, "Widget.run");

            let resolved = resolved_call_targets(&service, inner, "run");
            assert!(
                !resolved.contains(&widget_run.id),
                "shadowed inner w.run() must not leak the outer Widget type; got {resolved:?}"
            );
            assert!(
                resolved.is_empty(),
                "inner untyped param has no confident type → run unresolved; got {resolved:?}"
            );

            let (raw_syms, raw_rels) = extract_raw("recv_shadow.py", source, Language::Python);
            assert_eq!(
                recv_types_for_call(&raw_syms, &raw_rels, "outer.inner", "run"),
                vec![None],
                "the untyped inner param must shadow the outer type (no recv_type)"
            );
        }

        // FIX 4: `def Widget(): return Gadget(); x = Widget(); x.run()` — `Widget`
        // is a FACTORY FUNCTION, not a class, so `x` must not be typed `Widget`.
        // The receiver carries no recv_type and the call stays unresolved.
        #[test]
        fn receiver_type_slice_python_factory_not_constructor_typed() {
            const FACTORY_PY: &str = "\
class Gadget:
    def run(self):
        return 2

class Other:
    def run(self):
        return 3

def Widget():
    return Gadget()

def factory_user():
    x = Widget()
    x.run()
";
            // Raw extraction: `Widget` is a function → `x = Widget()` is NOT
            // constructor-typed → no recv_type on `x.run()` (old code wrongly
            // captured "Widget").
            let (raw_syms, raw_rels) = extract_raw("factory.py", FACTORY_PY, Language::Python);
            assert_eq!(
                recv_types_for_call(&raw_syms, &raw_rels, "factory_user", "run"),
                vec![None],
                "a factory function call must not be constructor-typed"
            );

            // End-to-end: with no receiver type, the ambiguous `run` stays
            // unresolved rather than being mis-attributed.
            let (service, _tmp, symbols) = index_source("factory.py", FACTORY_PY);
            let factory_user = sym(&symbols, "factory_user");
            let resolved = resolved_call_targets(&service, factory_user, "run");
            assert!(
                resolved.is_empty(),
                "factory-typed x.run() must stay unresolved; got {resolved:?}"
            );
        }

        // Minor (TS): a `this` inside a nested NON-arrow `function` is rebound at
        // runtime, so it must NOT be typed as the enclosing class. The ambiguous
        // `run` (defined on A and B) therefore stays unresolved from that call.
        #[test]
        fn receiver_type_slice_ts_nested_function_this_is_unknown() {
            const TS: &str = "\
class A {
  run(): number {
    return 1;
  }
  go(): number {
    function inner(): number {
      return this.run();
    }
    return inner();
  }
}
class B {
  run(): number {
    return 2;
  }
}
";
            // The nested-function `this.run()` is attributed to `inner`; it must
            // carry no recv_type (a plain function rebinds `this`).
            let (raw_syms, raw_rels) = extract_raw("nested_this.ts", TS, Language::TypeScript);
            let inner = raw_syms
                .iter()
                .find(|s| s.name == "inner")
                .expect("inner fn");
            let recv_types: Vec<Option<String>> = raw_rels
                .iter()
                .filter(|r| {
                    r.relationship_type == SymbolRelationshipType::Call
                        && r.source_symbol_id == inner.id
                        && r.target_name == "run"
                })
                .map(|r| r.recv_type.clone())
                .collect();
            assert_eq!(
                recv_types,
                vec![None],
                "this inside a nested non-arrow function must not be typed as the class"
            );
        }

        // ---- Qualified Rust call resolution tests ---------------------------

        /// Helper: get all call edges from `source_qn` targeting `target_name`,
        /// returning (resolved_id, strategy, unresolved_reason) tuples.
        fn call_edge_metadata(
            service: &LanguageService,
            source: &Symbol,
            target_name: &str,
        ) -> Vec<(Option<String>, Option<String>, Option<String>)> {
            let graph = service
                .get_symbol_graph(source, SymbolRelationshipType::Call, 50)
                .unwrap();
            graph
                .outgoing
                .iter()
                .filter(|edge| {
                    edge.relationship_type == SymbolRelationshipType::Call
                        && edge.target_name == target_name
                })
                .map(|edge| {
                    (
                        edge.target_symbol_id.clone(),
                        edge.resolution_strategy.clone(),
                        edge.unresolved_reason.clone(),
                    )
                })
                .collect()
        }

        /// `Self::method()` inside an inherent impl resolves to the method on
        /// that same impl block (lane 1: rust_self_owner).
        #[test]
        fn qualified_self_call_resolves_to_enclosing_impl_method() {
            const RS: &str = "\
struct Store;

impl Store {
    fn new() -> Store { Store }
    fn open(&self) -> Store {
        Self::new()
    }
}
";
            let (service, _tmp, symbols) = index_source("self_call.rs", RS);
            let open = sym(&symbols, "Store::open");
            let new = sym(&symbols, "Store::new");

            let edges = call_edge_metadata(&service, open, "new");
            assert_eq!(
                edges.len(),
                1,
                "expected one call edge for `new` from `open`"
            );
            let (resolved, strategy, reason) = &edges[0];
            assert_eq!(
                resolved.as_deref(),
                Some(new.id.as_str()),
                "Self::new() must resolve to Store::new"
            );
            assert_eq!(
                strategy.as_deref(),
                Some("rust_self_owner"),
                "strategy must be rust_self_owner"
            );
            assert!(reason.is_none(), "resolved edges must have no unresolved_reason");
        }

        /// `Type::method()` — associated path resolves to the method under the
        /// matching impl block (lane 5: rust_visible_owner).
        #[test]
        fn qualified_associated_call_resolves_to_impl_method() {
            const RS: &str = "\
struct Store;
struct Other;

impl Store {
    fn build() -> Store { Store }
}

impl Other {
    fn build() -> Other { Other }
    fn use_store() -> Store {
        Store::build()
    }
}
";
            let (service, _tmp, symbols) = index_source("assoc.rs", RS);
            let use_store = sym(&symbols, "Other::use_store");
            let store_build = sym(&symbols, "Store::build");

            let edges = call_edge_metadata(&service, use_store, "build");
            assert_eq!(edges.len(), 1);
            let (resolved, strategy, reason) = &edges[0];
            assert_eq!(
                resolved.as_deref(),
                Some(store_build.id.as_str()),
                "Store::build() must resolve to Store::build, not Other::build"
            );
            assert_eq!(
                strategy.as_deref(),
                Some("rust_visible_owner"),
            );
            assert!(reason.is_none());
        }

        /// Inherent associated methods take precedence over same-named trait
        /// impl methods. Trait methods require explicit trait evidence (UFCS).
        #[test]
        fn qualified_associated_call_prefers_inherent_over_trait_impl() {
            const RS: &str = "\
struct Foo;

impl Foo {
    fn run(&self) -> i32 { 1 }
}

trait Runner {
    fn run(&self) -> i32;
}

impl Runner for Foo {
    fn run(&self) -> i32 { 2 }
}

impl Foo {
    fn go(&self) -> i32 {
        Foo::run()
    }
}
";
            let (service, _tmp, symbols) = index_source("ambig.rs", RS);
            let go = sym(&symbols, "Foo::go");
            let inherent_run = sym(&symbols, "Foo::run");
            let edges = call_edge_metadata(&service, go, "run");
            assert_eq!(edges.len(), 1);
            let (resolved, strategy, reason) = &edges[0];
            assert_eq!(
                resolved.as_deref(),
                Some(inherent_run.id.as_str()),
                "Foo::run must select the inherent method without guessing a trait"
            );
            assert_eq!(strategy.as_deref(), Some("rust_visible_owner"));
            assert!(reason.is_none());
        }

        /// `Self::method()` without an enclosing impl must stay unresolved
        /// with reason `self_without_owner`.
        #[test]
        fn qualified_self_call_without_owner_stays_unresolved() {
            // `Self::new()` inside a free function — no enclosing impl.
            const RS: &str = "\
struct Store;

impl Store {
    fn new() -> Store { Store }
}

fn make_store() -> Store {
    Self::new()
}
";
            let (service, _tmp, symbols) = index_source("no_owner.rs", RS);
            let make_store = sym(&symbols, "make_store");

            let edges = call_edge_metadata(&service, make_store, "new");
            assert_eq!(edges.len(), 1);
            let (resolved, _strategy, reason) = &edges[0];
            assert!(
                resolved.is_none(),
                "Self::new() in a free function must not resolve; got {resolved:?}"
            );
            assert_eq!(
                reason.as_deref(),
                Some("self_without_owner"),
            );
        }

        /// `crate::module::Type::method()` — crate-rooted path resolves when
        /// the owner and method are in the same file.
        #[test]
        fn qualified_crate_path_resolves_same_file() {
            const RS: &str = "\
pub struct Config;

impl Config {
    pub fn load() -> Config { Config }
}

pub fn init() -> Config {
    crate::Config::load()
}
";
            let (service, _tmp, symbols) = index_source("crate_path.rs", RS);
            let init = sym(&symbols, "init");
            let load = sym(&symbols, "Config::load");

            let edges = call_edge_metadata(&service, init, "load");
            assert_eq!(edges.len(), 1);
            let (resolved, strategy, reason) = &edges[0];
            assert_eq!(
                resolved.as_deref(),
                Some(load.id.as_str()),
                "crate::Config::load() must resolve to Config::load"
            );
            assert_eq!(
                strategy.as_deref(),
                Some("rust_crate_path"),
            );
            assert!(reason.is_none());
        }

        /// Two qualified calls on the same source line must survive as distinct
        /// observations with different byte offsets (feature contract:
        /// exact call-site identity).
        #[test]
        fn qualified_same_line_distinct_byte_offsets() {
            const RS: &str = "\
struct A;
struct B;

impl A {
    fn new() -> A { A }
}

impl B {
    fn new() -> B { B }
}

fn make_both() {
    A::new(); B::new()
}
";
            let (service, _tmp, symbols) = index_source("same_line.rs", RS);
            let make_both = sym(&symbols, "make_both");

            let graph = service
                .get_symbol_graph(make_both, SymbolRelationshipType::Call, 50)
                .unwrap();
            let new_edges: Vec<_> = graph
                .outgoing
                .iter()
                .filter(|e| {
                    e.relationship_type == SymbolRelationshipType::Call
                        && e.target_name == "new"
                })
                .collect();
            assert_eq!(
                new_edges.len(),
                2,
                "two same-line calls must produce two distinct edges; got {}",
                new_edges.len()
            );
            // Verify they have distinct byte offsets.
            let offsets: Vec<_> = new_edges.iter().map(|e| e.byte_offset).collect();
            assert_ne!(
                offsets[0], offsets[1],
                "same-line calls must have distinct byte offsets"
            );
            // Both should resolve to their respective owners.
            let a_new = sym(&symbols, "A::new");
            let b_new = sym(&symbols, "B::new");
            let resolved_ids: Vec<_> = new_edges
                .iter()
                .map(|e| e.target_symbol_id.clone())
                .collect();
            assert!(
                resolved_ids.contains(&Some(a_new.id.clone())),
                "A::new() must resolve to A::new"
            );
            assert!(
                resolved_ids.contains(&Some(b_new.id.clone())),
                "B::new() must resolve to B::new"
            );
        }

        /// `self::Type::method()` module-relative path resolves to the same-file
        /// owner (lane 3: rust_module_path).
        #[test]
        fn qualified_module_path_resolves_same_file() {
            const RS: &str = "\
pub struct Store;

impl Store {
    pub fn open() -> Store { Store }
}

pub fn use_store() {
    self::Store::open()
}
";
            let (service, _tmp, symbols) = index_source("mod_path.rs", RS);
            let use_store = sym(&symbols, "use_store");
            let open = sym(&symbols, "Store::open");

            let edges = call_edge_metadata(&service, use_store, "open");
            assert_eq!(edges.len(), 1);
            let (resolved, strategy, reason) = &edges[0];
            assert_eq!(
                resolved.as_deref(),
                Some(open.id.as_str()),
                "self::Store::open() must resolve to Store::open"
            );
            assert_eq!(
                strategy.as_deref(),
                Some("rust_module_path"),
            );
            assert!(reason.is_none());
        }

        /// A failed qualifier must never fall back to repository-wide
        /// terminal-name uniqueness (fail-closed invariant 1).
        #[test]
        fn failed_qualifier_does_not_fall_back_to_global_name() {
            // `Unknown::new()` — no type named `Unknown` in the file.
            // But there IS a globally unique `new` method on `Store`.
            // The resolver must NOT fall back to resolving `new` globally.
            const RS: &str = "\
struct Store;

impl Store {
    fn new() -> Store { Store }
}

fn caller() {
    Unknown::new()
}
";
            let (service, _tmp, symbols) = index_source("no_fallback.rs", RS);
            let caller = sym(&symbols, "caller");

            let edges = call_edge_metadata(&service, caller, "new");
            assert_eq!(edges.len(), 1);
            let (resolved, _strategy, reason) = &edges[0];
            assert!(
                resolved.is_none(),
                "Unknown::new() must not resolve by falling back to global name; got {resolved:?}"
            );
            assert_eq!(
                reason.as_deref(),
                Some("unresolved_owner"),
                "unresolved reason must be `unresolved_owner`, not a global fallback"
            );
        }

        /// Source-order independence: a type declared *after* its impl
        /// must produce the same owner/member relationship as a type
        /// declared before it (feature contract: source-order-independent
        /// impl ownership).
        #[test]
        fn qualified_call_type_declared_after_impl_resolves() {
            const RS: &str = "\
impl Late {
    fn new() -> Late { Late }
}

struct Late;

fn caller() {
    Late::new()
}
";
            let (service, _tmp, symbols) = index_source("late.rs", RS);
            let caller = sym(&symbols, "caller");
            let new = sym(&symbols, "Late::new");

            let edges = call_edge_metadata(&service, caller, "new");
            assert_eq!(edges.len(), 1);
            let (resolved, strategy, reason) = &edges[0];
            assert_eq!(
                resolved.as_deref(),
                Some(new.id.as_str()),
                "Late::new() must resolve even when impl precedes the struct"
            );
            assert_eq!(strategy.as_deref(), Some("rust_visible_owner"));
            assert!(reason.is_none());
        }

        /// `Self::method()` inside a trait impl must resolve to the method
        /// on the implementing type (lane 1: rust_self_owner via trait impl).
        #[test]
        fn qualified_self_call_in_trait_impl_resolves() {
            const RS: &str = "\
struct Widget;

trait Maker {
    fn make() -> Self;
    fn reuse(&self) -> Self;
}

impl Maker for Widget {
    fn make() -> Widget { Widget }
    fn reuse(&self) -> Widget {
        Self::make()
    }
}
";
            let (service, _tmp, symbols) = index_source("trait_self.rs", RS);
            let reuse = sym(&symbols, "Widget as Maker::reuse");
            let make = sym(&symbols, "Widget as Maker::make");

            let edges = call_edge_metadata(&service, reuse, "make");
            assert_eq!(edges.len(), 1);
            let (resolved, strategy, reason) = &edges[0];
            assert_eq!(
                resolved.as_deref(),
                Some(make.id.as_str()),
                "Self::make() inside trait impl must resolve to Widget::make"
            );
            assert_eq!(strategy.as_deref(), Some("rust_self_owner"));
            assert!(reason.is_none());
        }

        /// `super::Type::method()` — parent-relative path resolves when
        /// the owner and method are in the same file (lane 3: rust_module_path).
        #[test]
        fn qualified_super_path_resolves_same_file() {
            const RS: &str = "\
pub struct Config;

impl Config {
    pub fn load() -> Config { Config }
}

mod inner {
    use super::Config;
    pub fn init() -> Config {
        super::Config::load()
    }
}
";
            let (service, _tmp, symbols) = index_source("super_path.rs", RS);
            let init = symbols
                .iter()
                .find(|symbol| symbol.name == "init")
                .expect("inner init function");
            let load = sym(&symbols, "Config::load");

            let edges = call_edge_metadata(&service, init, "load");
            assert_eq!(edges.len(), 1);
            let (resolved, strategy, reason) = &edges[0];
            assert_eq!(
                resolved.as_deref(),
                Some(load.id.as_str()),
                "super::Config::load() must resolve to Config::load"
            );
            assert_eq!(strategy.as_deref(), Some("rust_module_path"));
            assert!(reason.is_none());
        }

        /// An unresolved method under a valid owner must carry reason
        /// `unresolved_method`, not `unresolved_owner`.
        #[test]
        fn qualified_call_valid_owner_missing_method() {
            const RS: &str = "\
struct Store;
impl Store { fn new() -> Store { Store } }
fn caller() { Store::nonexistent() }
";
            let (service, _tmp, symbols) = index_source("no_method.rs", RS);
            let caller = sym(&symbols, "caller");

            let edges = call_edge_metadata(&service, caller, "nonexistent");
            assert_eq!(edges.len(), 1);
            let (resolved, _strategy, reason) = &edges[0];
            assert!(resolved.is_none());
            assert_eq!(
                reason.as_deref(),
                Some("unresolved_method"),
                "valid owner but missing method must be `unresolved_method`"
            );
        }
    }

    struct ExpectedSymbol {
        name: &'static str,
        symbol_type: SymbolType,
    }

    struct SymbolFixture {
        path: &'static str,
        expected_symbols: &'static [ExpectedSymbol],
    }

    const SCANNER_LANGUAGE_FIXTURE_PATHS: &[&str] = &[
        "css/basic.css",
        "css/modules/button.module.css",
        "css/variants/panel.scss",
        "css/variants/legacy.sass",
        "css/variants/theme.less",
        "html/basic.html",
        "vue/component.vue",
        "svelte/component.svelte",
        "config/package.json",
        "config/github-action.yml",
        "config/app.toml",
        "php/service.php",
        "java/UserService.java",
        "csharp/UserService.cs",
        "kotlin/UserService.kt",
        "ruby/user_service.rb",
        "shell/deploy.sh",
        "docker/Dockerfile",
        "sql/001_users.sql",
        "make/Makefile",
        "cmake/CMakeLists.txt",
    ];

    fn write_symbol_fixture(workspace_root: &Path, relative_path: &str) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/symbol_index_languages")
            .join(relative_path);
        let destination = workspace_root.join(relative_path);
        fs::create_dir_all(destination.parent().expect("fixture path has parent")).unwrap();
        fs::copy(&source, &destination)
            .unwrap_or_else(|error| panic!("failed to copy fixture {}: {error}", source.display()));
    }

    fn write_all_scanner_language_fixtures(workspace_root: &Path) {
        for path in SCANNER_LANGUAGE_FIXTURE_PATHS {
            write_symbol_fixture(workspace_root, path);
        }
    }

    #[test]
    fn test_dot_files_allow_non_indexed_live_sync() {
        assert!(should_allow_non_indexed_live_sync(".gitignore"));
        assert!(should_allow_non_indexed_live_sync("config/.prettierrc"));
        assert!(should_allow_non_indexed_live_sync("nested/.dockerignore"));
        assert!(!should_allow_non_indexed_live_sync("src/main.rs"));
        // M6.2 — dotenv files are now INDEXED (Toml), so they take the normal indexed
        // sync path rather than the non-indexed dotfile path.
        assert!(!should_allow_non_indexed_live_sync("config/.env.local"));
        assert!(!should_allow_non_indexed_live_sync(".env"));
    }

    #[test]
    fn is_generated_path_matches_codegen_and_spares_real_code() {
        // Generated → anchor-only.
        assert!(is_generated_path("api/v1/types.pb.go"));
        assert!(is_generated_path("k8s/zz_generated.deepcopy.go"));
        assert!(is_generated_path("proto/foo_pb2.py"));
        assert!(is_generated_path("proto/foo_pb2_grpc.py"));
        assert!(is_generated_path("web/dist/bundle.min.js"));
        assert!(is_generated_path("styles/app.min.css"));
        assert!(is_generated_path("lib/models/user.freezed.dart"));
        assert!(is_generated_path("Forms/Main.Designer.cs"));
        assert!(is_generated_path("schema.generated.ts"));
        // Real hand-written code → extracted normally.
        assert!(!is_generated_path("kernel/sched/core.c"));
        assert!(!is_generated_path("src/main.rs"));
        assert!(!is_generated_path("include/linux/list.h"));
        assert!(!is_generated_path("pkg/server/handler.go"));
    }

    #[test]
    fn test_config_capability_skips_known_lockfiles_for_directory_indexing() {
        assert!(is_supported_index_file("package.json"));
        assert!(is_supported_index_file("config/app.yaml"));
        assert!(!is_supported_index_file("package-lock.json"));
        assert!(!is_supported_index_file("pnpm-lock.yaml"));
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
    fn test_symbol_index_language_fixtures_cover_scanner_languages() {
        let (service, temp_dir) = create_test_service();
        let fixtures = [
            SymbolFixture {
                path: "css/basic.css",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "--fixture-accent",
                        symbol_type: SymbolType::CssCustomProperty,
                    },
                    ExpectedSymbol {
                        name: ".fixture-card",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: "#fixture-shell",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: "fixture-fade",
                        symbol_type: SymbolType::CssKeyframes,
                    },
                    ExpectedSymbol {
                        name: "fixture-components",
                        symbol_type: SymbolType::CssLayer,
                    },
                    ExpectedSymbol {
                        name: "@media (min-width: 720px)",
                        symbol_type: SymbolType::CssAtRule,
                    },
                    ExpectedSymbol {
                        name: "@container fixture-card (inline-size > 32rem)",
                        symbol_type: SymbolType::CssAtRule,
                    },
                    ExpectedSymbol {
                        name: "Fixture Sans",
                        symbol_type: SymbolType::CssFontFace,
                    },
                ],
            },
            SymbolFixture {
                path: "css/modules/button.module.css",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: ".buttonPrimary",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: ".button-secondary",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: "--button-accent",
                        symbol_type: SymbolType::CssCustomProperty,
                    },
                ],
            },
            SymbolFixture {
                path: "css/variants/panel.scss",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "--panel-gap",
                        symbol_type: SymbolType::CssCustomProperty,
                    },
                    ExpectedSymbol {
                        name: ".panelShell",
                        symbol_type: SymbolType::CssSelector,
                    },
                ],
            },
            SymbolFixture {
                path: "css/variants/legacy.sass",
                expected_symbols: &[ExpectedSymbol {
                    name: ".legacyPanel",
                    symbol_type: SymbolType::CssSelector,
                }],
            },
            SymbolFixture {
                path: "css/variants/theme.less",
                expected_symbols: &[ExpectedSymbol {
                    name: "#themeShell",
                    symbol_type: SymbolType::CssSelector,
                }],
            },
            SymbolFixture {
                path: "html/basic.html",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "#fixture-shell",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: ".fixture-page",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: ".fixtureGrid",
                        symbol_type: SymbolType::CssSelector,
                    },
                ],
            },
            SymbolFixture {
                path: "vue/component.vue",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "#vue-fixture",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: ".saveButton",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: ".vue-panel",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: "--vue-accent",
                        symbol_type: SymbolType::CssCustomProperty,
                    },
                    ExpectedSymbol {
                        name: "saveProfile",
                        symbol_type: SymbolType::Function,
                    },
                ],
            },
            SymbolFixture {
                path: "svelte/component.svelte",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "#svelte-fixture",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: ".dense-grid",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: ".svelte-action",
                        symbol_type: SymbolType::CssSelector,
                    },
                    ExpectedSymbol {
                        name: "--svelte-gap",
                        symbol_type: SymbolType::CssCustomProperty,
                    },
                    ExpectedSymbol {
                        name: "openPanel",
                        symbol_type: SymbolType::Function,
                    },
                ],
            },
            SymbolFixture {
                path: "config/package.json",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "scripts.build",
                        symbol_type: SymbolType::Property,
                    },
                    ExpectedSymbol {
                        name: "scripts.tauri:build",
                        symbol_type: SymbolType::Property,
                    },
                    ExpectedSymbol {
                        name: "dependencies.@tauri-apps/api",
                        symbol_type: SymbolType::Property,
                    },
                ],
            },
            SymbolFixture {
                path: "config/github-action.yml",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "jobs.build",
                        symbol_type: SymbolType::Property,
                    },
                    ExpectedSymbol {
                        name: "jobs.build.runs-on",
                        symbol_type: SymbolType::Property,
                    },
                    ExpectedSymbol {
                        name: "jobs.build.steps.name",
                        symbol_type: SymbolType::Property,
                    },
                ],
            },
            SymbolFixture {
                path: "config/app.toml",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "package.name",
                        symbol_type: SymbolType::Property,
                    },
                    ExpectedSymbol {
                        name: "profile.release.lto",
                        symbol_type: SymbolType::Property,
                    },
                ],
            },
            SymbolFixture {
                path: "php/service.php",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "App\\Service",
                        symbol_type: SymbolType::Namespace,
                    },
                    ExpectedSymbol {
                        name: "App\\Repository\\UserRepository",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "UserService",
                        symbol_type: SymbolType::Class,
                    },
                    ExpectedSymbol {
                        name: "findUser",
                        symbol_type: SymbolType::Method,
                    },
                    ExpectedSymbol {
                        name: "UserFormatter",
                        symbol_type: SymbolType::Interface,
                    },
                    ExpectedSymbol {
                        name: "LogsUsers",
                        symbol_type: SymbolType::Trait,
                    },
                    ExpectedSymbol {
                        name: "normalize_user_id",
                        symbol_type: SymbolType::Function,
                    },
                ],
            },
            SymbolFixture {
                path: "java/UserService.java",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "com.example.users",
                        symbol_type: SymbolType::Namespace,
                    },
                    ExpectedSymbol {
                        name: "java.util.Optional",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "java.util.Collections.emptyList",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "UserService",
                        symbol_type: SymbolType::Class,
                    },
                    ExpectedSymbol {
                        name: "findUser",
                        symbol_type: SymbolType::Method,
                    },
                    ExpectedSymbol {
                        name: "UserFormatter",
                        symbol_type: SymbolType::Interface,
                    },
                    ExpectedSymbol {
                        name: "format",
                        symbol_type: SymbolType::Method,
                    },
                    ExpectedSymbol {
                        name: "UserStatus",
                        symbol_type: SymbolType::Enum,
                    },
                    ExpectedSymbol {
                        name: "UserView",
                        symbol_type: SymbolType::Struct,
                    },
                ],
            },
            SymbolFixture {
                path: "csharp/UserService.cs",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "Example.Users",
                        symbol_type: SymbolType::Namespace,
                    },
                    ExpectedSymbol {
                        name: "System.Collections.Generic",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "System.String",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "System.Text.Json.JsonSerializer",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "UserService",
                        symbol_type: SymbolType::Class,
                    },
                    ExpectedSymbol {
                        name: "FindUser",
                        symbol_type: SymbolType::Method,
                    },
                    ExpectedSymbol {
                        name: "IUserFormatter",
                        symbol_type: SymbolType::Interface,
                    },
                    ExpectedSymbol {
                        name: "Format",
                        symbol_type: SymbolType::Method,
                    },
                    ExpectedSymbol {
                        name: "UserStatus",
                        symbol_type: SymbolType::Enum,
                    },
                    ExpectedSymbol {
                        name: "UserDto",
                        symbol_type: SymbolType::Struct,
                    },
                ],
            },
            SymbolFixture {
                path: "kotlin/UserService.kt",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "com.example.users",
                        symbol_type: SymbolType::Namespace,
                    },
                    ExpectedSymbol {
                        name: "kotlinx.coroutines.Dispatchers",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "com.example.shared.UserId",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "UserService",
                        symbol_type: SymbolType::Class,
                    },
                    ExpectedSymbol {
                        name: "findUser",
                        symbol_type: SymbolType::Method,
                    },
                    ExpectedSymbol {
                        name: "UserFormatter",
                        symbol_type: SymbolType::Interface,
                    },
                    ExpectedSymbol {
                        name: "format",
                        symbol_type: SymbolType::Method,
                    },
                    ExpectedSymbol {
                        name: "UserCache",
                        symbol_type: SymbolType::Module,
                    },
                    ExpectedSymbol {
                        name: "UserStatus",
                        symbol_type: SymbolType::Enum,
                    },
                    ExpectedSymbol {
                        name: "UserView",
                        symbol_type: SymbolType::Struct,
                    },
                    ExpectedSymbol {
                        name: "normalizeUserId",
                        symbol_type: SymbolType::Function,
                    },
                ],
            },
            SymbolFixture {
                path: "ruby/user_service.rb",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "json",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "user_formatter",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "Example::Users",
                        symbol_type: SymbolType::Namespace,
                    },
                    ExpectedSymbol {
                        name: "UserService",
                        symbol_type: SymbolType::Class,
                    },
                    ExpectedSymbol {
                        name: "find_user",
                        symbol_type: SymbolType::Method,
                    },
                    ExpectedSymbol {
                        name: "normalize_user_id",
                        symbol_type: SymbolType::Method,
                    },
                    ExpectedSymbol {
                        name: "active?",
                        symbol_type: SymbolType::Method,
                    },
                    ExpectedSymbol {
                        name: "format_user",
                        symbol_type: SymbolType::Function,
                    },
                ],
            },
            SymbolFixture {
                path: "shell/deploy.sh",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "./lib/common.sh",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "./env.sh",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "deploy_app",
                        symbol_type: SymbolType::Function,
                    },
                    ExpectedSymbol {
                        name: "rollback_app",
                        symbol_type: SymbolType::Function,
                    },
                    ExpectedSymbol {
                        name: "cleanup-trap",
                        symbol_type: SymbolType::Function,
                    },
                ],
            },
            SymbolFixture {
                path: "docker/Dockerfile",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "node:22-alpine",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "builder",
                        symbol_type: SymbolType::Module,
                    },
                    ExpectedSymbol {
                        name: "APP_VERSION",
                        symbol_type: SymbolType::Constant,
                    },
                    ExpectedSymbol {
                        name: "NODE_ENV",
                        symbol_type: SymbolType::Constant,
                    },
                    ExpectedSymbol {
                        name: "/app",
                        symbol_type: SymbolType::Property,
                    },
                    ExpectedSymbol {
                        name: "8080",
                        symbol_type: SymbolType::Property,
                    },
                ],
            },
            SymbolFixture {
                path: "sql/001_users.sql",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "./extensions.sql",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "app",
                        symbol_type: SymbolType::Namespace,
                    },
                    ExpectedSymbol {
                        name: "app.users",
                        symbol_type: SymbolType::Struct,
                    },
                    ExpectedSymbol {
                        name: "app.active_users",
                        symbol_type: SymbolType::Type,
                    },
                    ExpectedSymbol {
                        name: "idx_users_email",
                        symbol_type: SymbolType::Property,
                    },
                    ExpectedSymbol {
                        name: "app.normalize_user_id",
                        symbol_type: SymbolType::Function,
                    },
                    ExpectedSymbol {
                        name: "users_updated_at",
                        symbol_type: SymbolType::Method,
                    },
                ],
            },
            SymbolFixture {
                path: "make/Makefile",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "config.mk",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "APP_NAME",
                        symbol_type: SymbolType::Constant,
                    },
                    ExpectedSymbol {
                        name: "CFLAGS",
                        symbol_type: SymbolType::Constant,
                    },
                    ExpectedSymbol {
                        name: "build",
                        symbol_type: SymbolType::Function,
                    },
                    ExpectedSymbol {
                        name: "clean",
                        symbol_type: SymbolType::Function,
                    },
                ],
            },
            SymbolFixture {
                path: "cmake/CMakeLists.txt",
                expected_symbols: &[
                    ExpectedSymbol {
                        name: "ZBladeFixture",
                        symbol_type: SymbolType::Module,
                    },
                    ExpectedSymbol {
                        name: "GNUInstallDirs",
                        symbol_type: SymbolType::Import,
                    },
                    ExpectedSymbol {
                        name: "ZBLADE_FEATURES",
                        symbol_type: SymbolType::Constant,
                    },
                    ExpectedSymbol {
                        name: "zblade_core",
                        symbol_type: SymbolType::Struct,
                    },
                    ExpectedSymbol {
                        name: "zblade_cli",
                        symbol_type: SymbolType::Function,
                    },
                    ExpectedSymbol {
                        name: "configure_zblade",
                        symbol_type: SymbolType::Function,
                    },
                    ExpectedSymbol {
                        name: "register_zblade_test",
                        symbol_type: SymbolType::Function,
                    },
                ],
            },
        ];

        for fixture in fixtures {
            write_symbol_fixture(temp_dir.path(), fixture.path);
            let symbols = service.index_file(fixture.path).unwrap();
            for expected in fixture.expected_symbols {
                assert!(
                    symbols.iter().any(|symbol| {
                        symbol.name == expected.name && symbol.symbol_type == expected.symbol_type
                    }),
                    "expected {} {:?} in {}",
                    expected.name,
                    expected.symbol_type,
                    fixture.path
                );
            }
        }

        for (query, path, name) in [
            ("fixture-accent", "css/basic.css", "--fixture-accent"),
            (
                "buttonPrimary",
                "css/modules/button.module.css",
                ".buttonPrimary",
            ),
            ("fixtureGrid", "html/basic.html", ".fixtureGrid"),
            ("saveButton", "vue/component.vue", ".saveButton"),
            ("dense-grid", "svelte/component.svelte", ".dense-grid"),
            ("scripts.build", "config/package.json", "scripts.build"),
            (
                "jobs.build.runs-on",
                "config/github-action.yml",
                "jobs.build.runs-on",
            ),
            (
                "profile.release.lto",
                "config/app.toml",
                "profile.release.lto",
            ),
            ("findUser", "java/UserService.java", "findUser"),
            ("FindUser", "csharp/UserService.cs", "FindUser"),
            (
                "normalizeUserId",
                "kotlin/UserService.kt",
                "normalizeUserId",
            ),
            ("active?", "ruby/user_service.rb", "active?"),
            ("cleanup-trap", "shell/deploy.sh", "cleanup-trap"),
            ("APP_VERSION", "docker/Dockerfile", "APP_VERSION"),
            (
                "app.normalize_user_id",
                "sql/001_users.sql",
                "app.normalize_user_id",
            ),
            ("APP_NAME", "make/Makefile", "APP_NAME"),
            (
                "configure_zblade",
                "cmake/CMakeLists.txt",
                "configure_zblade",
            ),
        ] {
            let results = service.search_symbols(query, 10).unwrap();
            assert!(
                results
                    .iter()
                    .any(|result| result.symbol.file_path == path && result.symbol.name == name),
                "expected search for {query} to find {path}::{name}"
            );
        }
    }

    #[test]
    fn test_symbol_index_language_fixtures_record_indexing_measurement() {
        let (service, temp_dir) = create_test_service();
        write_all_scanner_language_fixtures(temp_dir.path());

        let stats = service.index_directory("").unwrap();
        let health = service.index_health_snapshot();

        assert_eq!(stats.files_failed, 0);
        assert_eq!(stats.supported_files, SCANNER_LANGUAGE_FIXTURE_PATHS.len());
        assert_eq!(stats.files_indexed, SCANNER_LANGUAGE_FIXTURE_PATHS.len());
        assert!(stats.symbols_extracted >= 24);
        assert!(stats.files_discovered >= SCANNER_LANGUAGE_FIXTURE_PATHS.len());
        assert!(stats.duration_ms <= 30_000);
        assert!(stats.parse_extract_ms <= 30_000);
        assert!(stats.db_write_ms <= 30_000);

        assert!(health.timings.last_discovery_ms.is_some());
        assert!(health.timings.last_batch_load_ms.is_some());
        assert!(health.timings.last_batch_freshness_check_ms.is_some());
        assert!(health.timings.last_batch_parse_extract_ms.is_some());
        assert!(health.timings.last_batch_db_write_ms.is_some());
        assert!(health.timings.last_batch_cache_update_ms.is_some());
        assert_eq!(health.discovery.last_scope.as_deref(), Some(""));
        assert_eq!(
            health.discovery.last_supported_files,
            SCANNER_LANGUAGE_FIXTURE_PATHS.len()
        );
        assert_eq!(
            health.discovery.last_indexed_files,
            SCANNER_LANGUAGE_FIXTURE_PATHS.len()
        );
        assert!(health.discovery.last_symbols_extracted >= stats.symbols_extracted);

        for (language, count) in [
            ("CSS", 2),
            ("SCSS", 1),
            ("Sass", 1),
            ("Less", 1),
            ("HTML", 1),
            ("Vue", 1),
            ("Svelte", 1),
            ("JSON", 1),
            ("YAML", 1),
            ("TOML", 1),
            ("PHP", 1),
            ("Java", 1),
            ("C#", 1),
            ("Kotlin", 1),
            ("Ruby", 1),
            ("Shell", 1),
            ("Dockerfile", 1),
            ("SQL", 1),
            ("Make/CMake", 2),
        ] {
            assert!(
                stats
                    .supported_by_language
                    .iter()
                    .any(|entry| entry.language == language && entry.count == count),
                "expected {count} indexed {language} fixture(s), got {:?}",
                stats.supported_by_language
            );
        }
    }

    #[test]
    fn test_index_schema_snapshot_reports_counts_and_coverage() {
        let (service, temp_dir) = create_test_service();
        write_all_scanner_language_fixtures(temp_dir.path());

        service.index_directory("").unwrap();
        let schema = service.index_schema_snapshot().unwrap();

        assert_eq!(
            schema.totals.indexed_files,
            SCANNER_LANGUAGE_FIXTURE_PATHS.len()
        );
        assert!(schema.totals.symbols >= 24);
        assert_eq!(
            schema.totals.relationships,
            schema.relationships.total_relationships
        );
        assert!(schema
            .files_by_extension
            .iter()
            .any(|entry| entry.name == "css" && entry.count == 2));
        assert!(schema.files_by_language.iter().any(|entry| {
            entry.language == "CSS" && entry.support_level == "partial" && entry.count == 2
        }));
        assert!(schema
            .files_by_support_level
            .iter()
            .any(|entry| entry.name == "partial"
                && entry.count == SCANNER_LANGUAGE_FIXTURE_PATHS.len()));
        assert!(schema
            .symbols_by_type
            .iter()
            .any(|entry| entry.name == "css_selector" && entry.count >= 12));
        assert!(schema
            .symbols_by_type
            .iter()
            .any(|entry| entry.name == "property" && entry.count >= 6));
    }

    #[test]
    fn test_index_schema_snapshot_scopes_counts_to_path() {
        let (service, temp_dir) = create_test_service();
        write_all_scanner_language_fixtures(temp_dir.path());

        service.index_directory("").unwrap();
        let root_schema = service.index_schema_snapshot().unwrap();
        let scoped_schema = service
            .index_schema_snapshot_for_path(Some("./css//variants/"))
            .unwrap();

        assert_eq!(scoped_schema.totals.indexed_files, 3);
        assert!(scoped_schema.totals.indexed_files < root_schema.totals.indexed_files);
        assert!(scoped_schema.totals.symbols < root_schema.totals.symbols);
        assert!(scoped_schema
            .files_by_extension
            .iter()
            .any(|entry| entry.name == "scss" && entry.count == 1));
        assert!(scoped_schema
            .files_by_language
            .iter()
            .any(|entry| entry.language == "SCSS" && entry.count == 1));
        assert!(scoped_schema
            .files_by_language
            .iter()
            .all(|entry| entry.language != "PHP"));

        let scope = scoped_schema.scope.as_ref().unwrap();
        assert_eq!(scope.requested_path, "./css//variants/");
        assert_eq!(scope.normalized_path, "css/variants");
        assert_eq!(
            scope.root_totals.indexed_files,
            SCANNER_LANGUAGE_FIXTURE_PATHS.len()
        );
        assert_eq!(scope.root_totals.symbols, root_schema.totals.symbols);
    }

    #[test]
    fn test_index_schema_snapshot_scopes_to_exact_file() {
        let (service, temp_dir) = create_test_service();
        write_all_scanner_language_fixtures(temp_dir.path());

        service.index_directory("").unwrap();
        let scoped_schema = service
            .index_schema_snapshot_for_path(Some("/php/service.php"))
            .unwrap();

        assert_eq!(scoped_schema.totals.indexed_files, 1);
        assert!(scoped_schema
            .files_by_extension
            .iter()
            .any(|entry| entry.name == "php" && entry.count == 1));
        assert!(scoped_schema
            .files_by_language
            .iter()
            .any(|entry| entry.language == "PHP" && entry.count == 1));
        assert_eq!(
            scoped_schema.scope.as_ref().unwrap().normalized_path,
            "php/service.php"
        );
    }

    #[test]
    fn test_index_css_symbols() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();

        fs::write(
            temp_dir.path().join("src/index.css"),
            r#"
:root {
    --accent-ai: #62d6ff;
}

.chat-message, #app-shell {
    color: var(--accent-ai);
}

@keyframes fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
}

@layer components {
    .layered { color: red; }
}

@media (min-width: 720px) {
    .wide { display: block; }
}

@container card (inline-size > 32rem) {
    .cardTitle { font-weight: 700; }
}

@font-face {
    font-family: "Blade Sans";
    src: url("/fonts/blade.woff2");
}
"#,
        )
        .unwrap();

        let symbols = service.index_file("src/index.css").unwrap();

        assert!(symbols.iter().any(|symbol| {
            symbol.name == "--accent-ai" && symbol.symbol_type == SymbolType::CssCustomProperty
        }));
        assert!(symbols.iter().any(|symbol| symbol.name == ".chat-message"
            && symbol.symbol_type == SymbolType::CssSelector));
        assert!(symbols.iter().any(|symbol| {
            symbol.name == "#app-shell" && symbol.symbol_type == SymbolType::CssSelector
        }));
        assert!(symbols.iter().any(
            |symbol| symbol.name == "fade-in" && symbol.symbol_type == SymbolType::CssKeyframes
        ));
        assert!(symbols.iter().any(
            |symbol| symbol.name == "components" && symbol.symbol_type == SymbolType::CssLayer
        ));
        assert!(symbols.iter().any(|symbol| {
            symbol.name == "@media (min-width: 720px)"
                && symbol.symbol_type == SymbolType::CssAtRule
        }));
        assert!(symbols.iter().any(|symbol| {
            symbol.name == "@container card (inline-size > 32rem)"
                && symbol.symbol_type == SymbolType::CssAtRule
        }));
        assert!(symbols.iter().any(|symbol| {
            symbol.name == "Blade Sans" && symbol.symbol_type == SymbolType::CssFontFace
        }));

        let results = service.search_symbols("accent", 10).unwrap();
        assert!(results
            .iter()
            .any(|result| result.symbol.name == "--accent-ai"));
    }

    #[test]
    fn test_index_markup_class_and_id_symbols() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("public")).unwrap();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();

        fs::write(
            temp_dir.path().join("public/index.html"),
            r#"
<!doctype html>
<html>
  <body id="app-shell" class="landing-page has-nav">
    <main class='hero-panel featureGrid'>Hello</main>
  </body>
</html>
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/App.vue"),
            r#"
<template>
  <section id="vue-shell" class="vue-layout">
    <button class="saveButton">Save</button>
  </section>
</template>
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/App.svelte"),
            r#"
<script>
  export let title = "Dashboard";
</script>

<main id="svelte-shell" class="svelteLayout dense-grid">
  <h1>{title}</h1>
</main>
"#,
        )
        .unwrap();

        let symbols = service.index_file("public/index.html").unwrap();

        for selector in [
            "#app-shell",
            ".landing-page",
            ".has-nav",
            ".hero-panel",
            ".featureGrid",
        ] {
            assert!(
                symbols.iter().any(|symbol| {
                    symbol.name == selector && symbol.symbol_type == SymbolType::CssSelector
                }),
                "expected HTML selector symbol {selector}"
            );
        }

        let results = service.search_symbols("featureGrid", 10).unwrap();
        assert!(results.iter().any(|result| {
            result.symbol.file_path == "public/index.html" && result.symbol.name == ".featureGrid"
        }));

        let vue_symbols = service.index_file("src/App.vue").unwrap();
        assert!(vue_symbols.iter().any(|symbol| {
            symbol.name == "#vue-shell" && symbol.symbol_type == SymbolType::CssSelector
        }));
        assert!(vue_symbols.iter().any(|symbol| {
            symbol.name == ".saveButton" && symbol.symbol_type == SymbolType::CssSelector
        }));

        let svelte_symbols = service.index_file("src/App.svelte").unwrap();
        assert!(svelte_symbols.iter().any(|symbol| {
            symbol.name == "#svelte-shell" && symbol.symbol_type == SymbolType::CssSelector
        }));
        assert!(svelte_symbols.iter().any(|symbol| {
            symbol.name == ".dense-grid" && symbol.symbol_type == SymbolType::CssSelector
        }));

        let results = service.search_symbols("saveButton", 10).unwrap();
        assert!(results.iter().any(|result| {
            result.symbol.file_path == "src/App.vue" && result.symbol.name == ".saveButton"
        }));
        let results = service.search_symbols("dense-grid", 10).unwrap();
        assert!(results.iter().any(|result| {
            result.symbol.file_path == "src/App.svelte" && result.symbol.name == ".dense-grid"
        }));
    }

    #[test]
    fn test_index_config_key_symbols() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("config")).unwrap();
        fs::create_dir_all(temp_dir.path().join(".github/workflows")).unwrap();

        fs::write(
            temp_dir.path().join("package.json"),
            r#"
{
  "name": "blade-test",
  "scripts": {
    "build": "vite build",
    "tauri:build": "tauri build"
  },
  "dependencies": {
    "@tauri-apps/api": "2.0.0"
  },
  "config": {
    "deep": {
      "noise": true
    }
  }
}
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("tsconfig.json"),
            r#"
{
  "extends": "./tsconfig.base.json",
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@app/*": ["src/app/*"],
      "@ui/*": ["src/ui/*"]
    },
    "target": "ES2022"
  },
  "random": {
    "deep": {
      "noise": true
    }
  }
}
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("tsconfig.jsonc"),
            r#"
{
  // Shared compiler options
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@shared/*": ["src/shared/*"], // alias used by app code
    },
  },
}
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join(".github/workflows/ci.yml"),
            r#"
name: CI
on:
  push:
  pull_request:
jobs:
  build:
    name: Build app
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4
      - name: Build frontend
        run: bun run build
  lint:
    runs-on: ubuntu-latest
    needs: build
    custom:
      deep:
        noise: true
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("docker-compose.yml"),
            r#"
name: blade-stack
services:
  web:
    image: zblade/web:latest
    ports:
      - "3000:3000"
    depends_on:
      - db
    custom:
      deep:
        noise: true
  db:
    image: postgres:16
    volumes:
      - db-data:/var/lib/postgresql/data
volumes:
  db-data:
networks:
  app-net:
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("config/app.yaml"),
            r#"
server:
  port: 5882
features:
  symbolsIndex: true
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("Cargo.toml"),
            r#"
[package]
name = "zblade"
version = "0.8.3"

[dependencies]
serde = "1.0"
"tree-sitter" = "0.24"

[profile.release]
lto = "fat"
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("pyproject.toml"),
            r#"
[tool.poetry]
name = "blade-tools"

[tool.pytest.ini_options]
testpaths = ["tests"]
"#,
        )
        .unwrap();

        let json_symbols = service.index_file("package.json").unwrap();
        assert!(json_symbols.iter().any(|symbol| {
            symbol.name == "scripts.build" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(json_symbols.iter().any(|symbol| {
            symbol.name == "dependencies.@tauri-apps/api"
                && symbol.symbol_type == SymbolType::Property
        }));
        assert!(!json_symbols
            .iter()
            .any(|symbol| symbol.name == "config.deep.noise"));

        let tsconfig_symbols = service.index_file("tsconfig.json").unwrap();
        assert!(tsconfig_symbols.iter().any(|symbol| {
            symbol.name == "extends" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(tsconfig_symbols.iter().any(|symbol| {
            symbol.name == "compilerOptions.baseUrl" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(tsconfig_symbols.iter().any(|symbol| {
            symbol.name == "compilerOptions.paths.@app/*"
                && symbol.symbol_type == SymbolType::Property
        }));
        assert!(!tsconfig_symbols
            .iter()
            .any(|symbol| symbol.name == "random.deep.noise"));

        let tsconfig_jsonc_symbols = service.index_file("tsconfig.jsonc").unwrap();
        assert!(tsconfig_jsonc_symbols.iter().any(|symbol| {
            symbol.name == "compilerOptions.paths.@shared/*"
                && symbol.symbol_type == SymbolType::Property
        }));

        let workflow_symbols = service.index_file(".github/workflows/ci.yml").unwrap();
        assert!(workflow_symbols.iter().any(|symbol| {
            symbol.name == "on.push" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(workflow_symbols.iter().any(|symbol| {
            symbol.name == "jobs.build" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(workflow_symbols.iter().any(|symbol| {
            symbol.name == "jobs.build.runs-on" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(workflow_symbols.iter().any(|symbol| {
            symbol.name == "jobs.build.steps.Build frontend"
                && symbol.symbol_type == SymbolType::Property
        }));
        assert!(workflow_symbols.iter().any(|symbol| {
            symbol.name == "jobs.lint.needs" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(!workflow_symbols
            .iter()
            .any(|symbol| symbol.name == "jobs.lint.custom.deep.noise"));

        let compose_symbols = service.index_file("docker-compose.yml").unwrap();
        assert!(compose_symbols.iter().any(|symbol| {
            symbol.name == "services.web" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(compose_symbols.iter().any(|symbol| {
            symbol.name == "services.web.ports" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(compose_symbols.iter().any(|symbol| {
            symbol.name == "services.web.depends_on" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(compose_symbols.iter().any(|symbol| {
            symbol.name == "services.db.volumes" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(compose_symbols.iter().any(|symbol| {
            symbol.name == "volumes.db-data" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(compose_symbols.iter().any(|symbol| {
            symbol.name == "networks.app-net" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(!compose_symbols
            .iter()
            .any(|symbol| symbol.name == "services.web.custom.deep.noise"));

        let yaml_symbols = service.index_file("config/app.yaml").unwrap();
        assert!(yaml_symbols.iter().any(|symbol| {
            symbol.name == "server.port" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(yaml_symbols.iter().any(|symbol| {
            symbol.name == "features.symbolsIndex" && symbol.symbol_type == SymbolType::Property
        }));

        let cargo_symbols = service.index_file("Cargo.toml").unwrap();
        assert!(cargo_symbols.iter().any(|symbol| {
            symbol.name == "package.name" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(cargo_symbols.iter().any(|symbol| {
            symbol.name == "dependencies.serde" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(cargo_symbols.iter().any(|symbol| {
            symbol.name == "dependencies.tree-sitter" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(cargo_symbols.iter().any(|symbol| {
            symbol.name == "profile.release.lto" && symbol.symbol_type == SymbolType::Property
        }));

        let pyproject_symbols = service.index_file("pyproject.toml").unwrap();
        assert!(pyproject_symbols.iter().any(|symbol| {
            symbol.name == "tool.poetry.name" && symbol.symbol_type == SymbolType::Property
        }));
        assert!(pyproject_symbols.iter().any(|symbol| {
            symbol.name == "tool.pytest.ini_options.testpaths"
                && symbol.symbol_type == SymbolType::Property
        }));

        let results = service.search_symbols("scripts.build", 10).unwrap();
        assert!(results.iter().any(|result| {
            result.symbol.file_path == "package.json" && result.symbol.name == "scripts.build"
        }));
        let results = service.search_symbols("server.port", 10).unwrap();
        assert!(results.iter().any(|result| {
            result.symbol.file_path == "config/app.yaml" && result.symbol.name == "server.port"
        }));
        let results = service.search_symbols("profile.release.lto", 10).unwrap();
        assert!(results.iter().any(|result| {
            result.symbol.file_path == "Cargo.toml" && result.symbol.name == "profile.release.lto"
        }));
    }

    #[test]
    fn test_config_symbol_extraction_is_bounded() {
        let mut content = String::from("{\n");
        for index in 0..(CONFIG_SYMBOL_LIMIT + 128) {
            if index > 0 {
                content.push_str(",\n");
            }
            content.push_str(&format!("  \"key_{index}\": {index}"));
        }
        content.push_str("\n}\n");

        let symbols = extract_config_symbols("large.json", &content, Language::Json);
        assert!(symbols.len() <= CONFIG_SYMBOL_LIMIT);
    }

    // ---- M4.3 — K8s Resource nodes + Kustomize IMPORTS + span-accurate config ----

    #[test]
    fn test_k8s_manifest_emits_single_resource_symbol() {
        let manifest = "\
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  replicas: 2
  template:
    spec:
      containers:
        - name: web
          image: nginx:1.27
";
        let symbols = extract_config_symbols("deploy.yaml", manifest, Language::Yaml);
        // ONE Resource node, not a bag of repeated spec/metadata keys.
        assert_eq!(
            symbols.len(),
            1,
            "K8s manifest should collapse to a single Resource node, got {symbols:?}"
        );
        assert_eq!(symbols[0].symbol_type, SymbolType::Resource);
        assert_eq!(symbols[0].name, "Deployment/web");
        assert_eq!(symbols[0].qualified_name, "Deployment/web");

        // A multi-document file yields one Resource per manifest doc.
        let multi = "\
apiVersion: v1
kind: Service
metadata:
  name: web
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
";
        let resources = collect_k8s_resource_symbols("stack.yaml", multi);
        assert_eq!(resources.len(), 2);
        assert!(resources.iter().any(|s| s.name == "Service/web"));
        assert!(resources.iter().any(|s| s.name == "Deployment/web"));
        assert!(resources
            .iter()
            .all(|s| s.symbol_type == SymbolType::Resource));

        // Missing metadata.name falls back to a stable placeholder.
        let unnamed = collect_k8s_resource_symbols(
            "cm.yaml",
            "apiVersion: v1\nkind: ConfigMap\ndata:\n  key: value\n",
        );
        assert_eq!(unnamed.len(), 1);
        assert_eq!(unnamed[0].name, "ConfigMap/<unnamed>");
    }

    #[test]
    fn test_kustomization_emits_import_symbols_and_edges() {
        let kustomization = "\
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
namespace: prod
resources:
  - ../base
  - service.yaml
bases:
  - ../legacy
components:
  - ../components/logging
";
        let symbols = extract_config_symbols("kustomization.yaml", kustomization, Language::Yaml);
        let imports: Vec<&str> = symbols
            .iter()
            .filter(|s| s.symbol_type == SymbolType::Import)
            .map(|s| s.name.as_str())
            .collect();
        assert!(imports.contains(&"../base"), "imports: {imports:?}");
        assert!(imports.contains(&"service.yaml"), "imports: {imports:?}");
        assert!(imports.contains(&"../legacy"), "imports: {imports:?}");
        assert!(
            imports.contains(&"../components/logging"),
            "imports: {imports:?}"
        );
        // A kustomization is imports, not a Resource manifest.
        assert!(symbols
            .iter()
            .all(|s| s.symbol_type != SymbolType::Resource));

        // Each list entry becomes an Import edge (target = the referenced path).
        let relationships = derive_config_import_relationships("kustomization.yaml", &symbols);
        assert!(relationships
            .iter()
            .all(|r| r.relationship_type == SymbolRelationshipType::Import));
        let targets: Vec<&str> = relationships
            .iter()
            .map(|r| r.target_name.as_str())
            .collect();
        assert_eq!(
            targets.len(),
            4,
            "one Import edge per resources/bases/components entry, got {targets:?}"
        );
        assert!(targets.contains(&"../base"));
        assert!(targets.contains(&"../components/logging"));
    }

    #[test]
    fn test_yaml_duplicate_leaf_key_reports_its_own_line() {
        // The leaf key `name` appears at the top level (line 0) and nested under
        // `service` (line 2). The legacy `locate_config_key` re-find pinned
        // `service.name` to the FIRST `name:` (line 0); the span-preserving
        // parser pins each key to its OWN source line.
        let yaml = "\
name: top
service:
  name: inner
  port: 9090
";
        let symbols = extract_config_symbols("app.yaml", yaml, Language::Yaml);

        let top = symbols
            .iter()
            .find(|s| s.name == "name")
            .expect("top-level `name`");
        assert_eq!(top.range.start.line, 0);
        assert_eq!(top.range.start.character, 0);

        let nested = symbols
            .iter()
            .find(|s| s.name == "service.name")
            .expect("nested `service.name`");
        assert_eq!(
            nested.range.start.line, 2,
            "the second `name` key must report its own line, not the first"
        );
        assert_eq!(nested.range.start.character, 2, "nested key column");
    }

    #[test]
    fn test_top_level_sequence_yaml_still_extracts_keys() {
        // BUG 1 regression: `marked_yaml::parse_yaml` defaults to a mapping root
        // and returns Err for a TOP-LEVEL SEQUENCE. The pre-M4.3 serde path
        // extracted these keys; the span path must fall back to it so the key set
        // is preserved (positions degrade to best-effort — that's acceptable for a
        // non-mapping root). Drives the real scanner entry point.
        let yaml = "\
- name: alpha
  port: 8080
- name: beta
  port: 9090
";
        let symbols = extract_scanner_symbols("list.yaml", yaml, Language::Yaml);
        assert!(
            !symbols.is_empty(),
            "top-level-sequence YAML must still yield config keys, got none"
        );
        assert!(
            symbols.iter().any(|s| s.name == "name"),
            "expected the `name` key, got {symbols:?}"
        );
        assert!(
            symbols.iter().any(|s| s.name == "port"),
            "expected the `port` key, got {symbols:?}"
        );
        assert!(symbols
            .iter()
            .all(|s| s.symbol_type == SymbolType::Property));
    }

    #[test]
    fn test_mixed_multidoc_yaml_emits_resource_and_non_manifest_keys() {
        // BUG 2 regression: a manifest doc collapses to a `Resource`, but the
        // OTHER `---`-separated (non-manifest) doc's flat config keys must still
        // be extracted — not dropped by an early `return resources`.
        let yaml = "\
apiVersion: v1
kind: ConfigMap
metadata:
  name: app-config
---
server:
  port: 8080
  host: localhost
";
        let symbols = extract_config_symbols("mixed.yaml", yaml, Language::Yaml);

        let resource = symbols
            .iter()
            .find(|s| s.symbol_type == SymbolType::Resource)
            .expect("manifest doc must still emit a Resource");
        assert_eq!(resource.name, "ConfigMap/app-config");

        for key in ["server", "server.port", "server.host"] {
            assert!(
                symbols
                    .iter()
                    .any(|s| s.name == key && s.symbol_type == SymbolType::Property),
                "non-manifest doc key `{key}` must be extracted, got {symbols:?}"
            );
        }
    }

    #[test]
    fn test_css_custom_property_usage_relationships_resolve_to_tokens() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();

        fs::write(
            temp_dir.path().join("src/theme.css"),
            r#"
:root {
    --accent-ai: #62d6ff;
    --accent-strong: var(--accent-ai);
}

.button {
    color: var(--accent-strong);
}
"#,
        )
        .unwrap();

        service.index_file("src/theme.css").unwrap();

        let symbols = service.get_file_symbols("src/theme.css").unwrap();
        let accent_symbol = symbols
            .iter()
            .find(|symbol| symbol.name == "--accent-ai")
            .expect("expected base custom property");
        let strong_symbol = symbols
            .iter()
            .find(|symbol| symbol.name == "--accent-strong")
            .expect("expected derived custom property");

        let accent_graph = service
            .get_symbol_graph(accent_symbol, SymbolRelationshipType::Usage, 10)
            .unwrap();
        assert!(accent_graph.incoming.iter().any(|reference| {
            reference.source_symbol.name == "--accent-strong"
                && reference.relationship_type == SymbolRelationshipType::Usage
                && reference.target_symbol_id.as_deref() == Some(accent_symbol.id.as_str())
        }));

        let strong_graph = service
            .get_symbol_graph(strong_symbol, SymbolRelationshipType::Usage, 10)
            .unwrap();
        assert!(strong_graph.incoming.iter().any(|reference| {
            reference.source_symbol.name == ".button"
                && reference.relationship_type == SymbolRelationshipType::Usage
                && reference.target_symbol_id.as_deref() == Some(strong_symbol.id.as_str())
        }));
    }

    #[test]
    fn test_index_stylesheet_variant_symbols() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("src/styles")).unwrap();

        fs::write(
            temp_dir.path().join("src/styles/Button.module.scss"),
            r#"
:root {
    --button-gap: 8px;
}

.buttonPrimary {
    color: var(--button-gap);
}
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/styles/legacy.sass"),
            r#"
.legacyButton
  color: red
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/styles/theme.less"),
            r#"
#appShell {
    color: blue;
}
"#,
        )
        .unwrap();

        let scss_symbols = service.index_file("src/styles/Button.module.scss").unwrap();
        assert!(scss_symbols.iter().any(|symbol| {
            symbol.name == "--button-gap" && symbol.symbol_type == SymbolType::CssCustomProperty
        }));
        assert!(scss_symbols.iter().any(|symbol| {
            symbol.name == ".buttonPrimary" && symbol.symbol_type == SymbolType::CssSelector
        }));

        let sass_symbols = service.index_file("src/styles/legacy.sass").unwrap();
        assert!(sass_symbols.iter().any(|symbol| {
            symbol.name == ".legacyButton" && symbol.symbol_type == SymbolType::CssSelector
        }));

        let less_symbols = service.index_file("src/styles/theme.less").unwrap();
        assert!(less_symbols.iter().any(|symbol| {
            symbol.name == "#appShell" && symbol.symbol_type == SymbolType::CssSelector
        }));

        let results = service.search_symbols("buttonPrimary", 10).unwrap();
        assert!(results.iter().any(|result| result.symbol.file_path
            == "src/styles/Button.module.scss"
            && result.symbol.name == ".buttonPrimary"));
    }

    #[test]
    fn test_css_module_usage_relationships_resolve_to_stylesheet_symbols() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();

        fs::write(
            temp_dir.path().join("src/Button.module.css"),
            r#"
.buttonPrimary {
    color: red;
}
.button-secondary {
    color: blue;
}
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/global.css"),
            r#"
.globalShell {
    display: grid;
}
.extraShell {
    align-items: center;
}
.isActive {
    opacity: 1;
}
.conditionalShell {
    visibility: visible;
}
"#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/Button.tsx"),
            r#"
import clsx from "clsx";
import styles from "./Button.module.css";
import "./global.css";

export function Button() {
    return <button className={clsx(`${styles.buttonPrimary} globalShell`, styles["button-secondary"], "extraShell", isActive && "isActive", { conditionalShell: isActive })}>Save</button>;
}
"#,
        )
        .unwrap();

        service.index_directory("src").unwrap();

        let css_symbol = service
            .get_file_symbols("src/Button.module.css")
            .unwrap()
            .into_iter()
            .find(|symbol| symbol.name == ".buttonPrimary")
            .expect("expected indexed CSS selector");
        let graph = service
            .get_symbol_graph(&css_symbol, SymbolRelationshipType::Usage, 10)
            .unwrap();

        assert!(graph.incoming.iter().any(|reference| {
            reference.source_symbol.file_path == "src/Button.tsx"
                && reference.source_symbol.name == "Button"
                && reference.relationship_type == SymbolRelationshipType::Usage
                && reference.target_symbol_id.as_deref() == Some(css_symbol.id.as_str())
        }));

        let button_symbol = service
            .get_file_symbols("src/Button.tsx")
            .unwrap()
            .into_iter()
            .find(|symbol| symbol.name == "Button")
            .expect("expected indexed component");
        let graph = service
            .get_symbol_graph(&button_symbol, SymbolRelationshipType::Usage, 10)
            .unwrap();

        assert!(graph.outgoing.iter().any(|reference| {
            reference
                .target_symbol
                .as_ref()
                .is_some_and(|symbol| symbol.id == css_symbol.id)
        }));

        let global_symbol = service
            .get_file_symbols("src/global.css")
            .unwrap()
            .into_iter()
            .find(|symbol| symbol.name == ".globalShell")
            .expect("expected indexed global CSS selector");
        let graph = service
            .get_symbol_graph(&global_symbol, SymbolRelationshipType::Usage, 10)
            .unwrap();

        assert!(graph.incoming.iter().any(|reference| {
            reference.source_symbol.file_path == "src/Button.tsx"
                && reference.source_symbol.name == "Button"
                && reference.relationship_type == SymbolRelationshipType::Usage
                && reference.target_symbol_id.as_deref() == Some(global_symbol.id.as_str())
        }));

        for selector in [
            ".button-secondary",
            ".extraShell",
            ".isActive",
            ".conditionalShell",
        ] {
            let symbol = service
                .search_symbols(selector, 10)
                .unwrap()
                .into_iter()
                .map(|result| result.symbol)
                .find(|symbol| symbol.name == selector)
                .unwrap_or_else(|| panic!("expected indexed selector {selector}"));
            let graph = service
                .get_symbol_graph(&symbol, SymbolRelationshipType::Usage, 10)
                .unwrap();
            assert!(
                graph.incoming.iter().any(|reference| {
                    reference.source_symbol.file_path == "src/Button.tsx"
                        && reference.source_symbol.name == "Button"
                        && reference.relationship_type == SymbolRelationshipType::Usage
                        && reference.target_symbol_id.as_deref() == Some(symbol.id.as_str())
                }),
                "expected Button to use {selector}"
            );
        }
    }

    #[test]
    fn test_css_module_usage_source_prefers_narrowest_enclosing_symbol() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();

        fs::write(
            temp_dir.path().join("src/Button.module.css"),
            ".buttonPrimary { color: red; }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/Button.tsx"),
            r#"
import styles from "./Button.module.css";

export function outer() {
    function inner() {
        return styles.buttonPrimary;
    }
    return inner();
}
"#,
        )
        .unwrap();

        service.index_directory("src").unwrap();

        let css_symbol = service
            .get_file_symbols("src/Button.module.css")
            .unwrap()
            .into_iter()
            .find(|symbol| symbol.name == ".buttonPrimary")
            .expect("expected indexed CSS selector");
        let graph = service
            .get_symbol_graph(&css_symbol, SymbolRelationshipType::Usage, 10)
            .unwrap();

        assert!(graph.incoming.iter().any(|reference| {
            reference.source_symbol.file_path == "src/Button.tsx"
                && reference.source_symbol.name == "inner"
                && reference.relationship_type == SymbolRelationshipType::Usage
                && reference.target_symbol_id.as_deref() == Some(css_symbol.id.as_str())
        }));
    }

    #[test]
    fn test_index_file_records_timing_snapshot() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("timed.ts"),
            "export function timed() { return 1; }\n",
        )
        .unwrap();

        service.index_file("timed.ts").unwrap();
        let health = service.index_health_snapshot();

        assert_eq!(health.timings.last_file_path.as_deref(), Some("timed.ts"));
        assert!(health.timings.last_file_total_ms.is_some());
        assert!(health.timings.last_file_load_ms.is_some());
        assert!(health.timings.last_file_freshness_check_ms.is_some());
        assert!(health.timings.last_file_parse_extract_ms.is_some());
        assert!(health
            .timings
            .last_file_relationship_enrichment_ms
            .is_some());
        assert!(health.timings.last_file_db_write_ms.is_some());
        assert!(health.timings.last_file_cache_update_ms.is_some());
    }

    #[test]
    fn test_index_directory_records_discovery_timing() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();

        fs::write(
            temp_dir.path().join("src/one.ts"),
            "export function one() { return 1; }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/two.ts"),
            "export function two() { return 2; }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/theme.css"),
            ".button { --accent-ai: red; }\n",
        )
        .unwrap();
        fs::write(temp_dir.path().join("src/notes.txt"), "not indexed\n").unwrap();

        let stats = service.index_directory("src").unwrap();
        let health = service.index_health_snapshot();

        assert_eq!(stats.files_discovered, 4);
        assert_eq!(stats.supported_files, 3);
        assert_eq!(stats.files_indexed, 3);
        assert_eq!(stats.files_reindexed, 3);
        assert_eq!(stats.files_fresh, 0);
        assert!(stats.symbols_extracted >= 3);
        assert!(stats.anchors_extracted >= 1);
        assert!(stats.parse_extract_ms <= stats.duration_ms);
        assert!(health.timings.last_discovery_ms.is_some());
        assert!(health.timings.last_batch_load_ms.is_some());
        assert!(health.timings.last_batch_parse_extract_ms.is_some());
        assert!(health.timings.last_batch_db_write_ms.is_some());
        assert_eq!(health.discovery.last_scope.as_deref(), Some("src"));
        assert_eq!(health.discovery.last_discovered_files, 4);
        assert_eq!(health.discovery.last_supported_files, 3);
        assert_eq!(health.discovery.last_indexed_files, 3);
        assert_eq!(health.discovery.last_reindexed_files, 3);
        assert_eq!(health.discovery.last_fresh_files, 0);
        assert!(health.discovery.last_anchors_extracted >= 1);
        assert_eq!(
            health.discovery.last_relationships_extracted,
            stats.relationships_extracted
        );
        assert!(health
            .discovery
            .supported_by_language
            .iter()
            .any(|entry| entry.language == "TypeScript" && entry.count == 2));
        assert!(health
            .discovery
            .supported_by_language
            .iter()
            .any(|entry| entry.language == "CSS" && entry.count == 1));
        assert!(health
            .discovery
            .skipped_by_reason
            .iter()
            .any(|entry| entry.reason == "unsupported_language" && entry.count == 1));

        let stats = service.index_directory("src").unwrap();
        let health = service.index_health_snapshot();

        assert_eq!(stats.files_indexed, 3);
        assert_eq!(stats.files_fresh, 3);
        assert_eq!(stats.files_reindexed, 0);
        assert_eq!(stats.parse_extract_ms, 0);
        assert_eq!(stats.db_write_ms, 0);
        assert_eq!(health.discovery.last_fresh_files, 3);
        assert_eq!(health.discovery.last_reindexed_files, 0);
    }

    #[test]
    fn test_index_astro_file_symbols() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("src/pages")).unwrap();

        fs::write(
            temp_dir.path().join("src/pages/Home.astro"),
            r#"---
import Layout from "../components/Layout.astro";

export function loadPosts() {
    return [];
}
---
<Layout>
    <script>
    function hydrateHero() {
        console.log("ready");
    }
    </script>
</Layout>
"#,
        )
        .unwrap();

        let symbols = service.index_file("src/pages/Home.astro").unwrap();

        assert!(symbols.iter().any(|symbol| symbol.name == "Home"
            && symbol.signature.as_deref() == Some("Astro component")));
        assert!(symbols.iter().any(|symbol| symbol.name == "loadPosts"));
        assert!(symbols.iter().any(|symbol| symbol.name == "hydrateHero"));
        assert!(symbols
            .iter()
            .any(|symbol| symbol.symbol_type == SymbolType::Import
                && symbol.name == "../components/Layout.astro"));
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
    fn test_search_symbols_filtered_with_patterns() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();

        fs::write(
            temp_dir.path().join("src/button.css"),
            ".buttonPrimary { color: red; }\n.cardPrimary { color: blue; }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/button.tsx"),
            "export function buttonPrimary() { return null; }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/legacy.css"),
            ".buttonSecondary { color: green; }\n",
        )
        .unwrap();

        service.index_directory("src").unwrap();

        let results = service
            .search_symbols_filtered_with_patterns(
                "button",
                None,
                None,
                Some("src/button.css"),
                Some("*.button*"),
                Some("*.button*"),
                10,
            )
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.file_path, "src/button.css");
        assert_eq!(results[0].symbol.name, ".buttonPrimary");
    }

    #[test]
    fn related_symbols_include_sibling_export_consumers() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("math.ts"),
            r#"
            export function add() {
                return 1;
            }

            export function subtract() {
                return 0;
            }
        "#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("invoice.ts"),
            r#"
            import { subtract } from "./math";

            export function total() {
                return subtract();
            }
        "#,
        )
        .unwrap();

        service.index_file("math.ts").unwrap();
        service.index_file("invoice.ts").unwrap();
        let add = service
            .search_symbols_filtered("add", Some("math.ts"), None, 10)
            .unwrap()
            .into_iter()
            .find(|result| result.symbol.name == "add")
            .unwrap()
            .symbol;

        let related = service.get_related_symbols(&add, 20).unwrap();

        assert!(related.iter().any(
            |item| item.relationship == "same_module_export" && item.symbol.name == "subtract"
        ));
        assert!(related
            .iter()
            .any(|item| item.relationship == "sibling_export_consumer"
                && item.symbol.file_path == "invoice.ts"
                && item.symbol.name == "total"));
    }

    #[test]
    fn related_symbols_respects_zero_limit() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("math.ts"),
            r#"
            export function add() {
                return 1;
            }
        "#,
        )
        .unwrap();

        service.index_file("math.ts").unwrap();
        let add = service
            .search_symbols_filtered("add", Some("math.ts"), None, 10)
            .unwrap()
            .into_iter()
            .find(|result| result.symbol.name == "add")
            .unwrap()
            .symbol;

        let related = service.get_related_symbols(&add, 0).unwrap();

        assert!(related.is_empty());
    }

    #[test]
    fn related_symbols_include_nearby_css_class_name_matches() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();

        fs::write(
            temp_dir.path().join("src/button.tsx"),
            r#"
            export function buttonPrimaryController() {
                return <button />;
            }
        "#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/button.css"),
            ".buttonPrimary { color: red; }\n.cardSecondary { color: blue; }\n",
        )
        .unwrap();

        service.index_directory("src").unwrap();
        let button_primary = service
            .search_symbols_filtered("buttonPrimaryController", Some("src/button.tsx"), None, 10)
            .unwrap()
            .into_iter()
            .find(|result| result.symbol.name == "buttonPrimaryController")
            .unwrap()
            .symbol;

        let related = service.get_related_symbols(&button_primary, 20).unwrap();

        assert!(related
            .iter()
            .any(|item| item.relationship == "lexical_similarity"
                && item.symbol.file_path == "src/button.css"
                && item.symbol.name == ".buttonPrimary"));
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
    fn discovery_includes_dotenv_files() {
        let (service, temp_dir) = create_test_service();
        fs::write(temp_dir.path().join(".env"), "API_KEY=secret\nPORT=8080\n").unwrap();
        fs::write(temp_dir.path().join(".env.local"), "LOCAL_ONLY=1\n").unwrap();
        fs::write(temp_dir.path().join("app.ts"), "export function app() {}").unwrap();
        // A non-dotenv dotfile must still be ignored.
        fs::write(temp_dir.path().join(".gitconfig"), "[user]\n").unwrap();

        let discovered = service.supported_language_files(".");
        assert!(discovered.iter().any(|p| p == ".env"), "{discovered:?}");
        assert!(
            discovered.iter().any(|p| p == ".env.local"),
            "{discovered:?}"
        );
        assert!(discovered.iter().any(|p| p == "app.ts"));
        assert!(
            !discovered.iter().any(|p| p == ".gitconfig"),
            "non-dotenv dotfiles stay ignored: {discovered:?}"
        );
    }

    #[test]
    fn dotenv_file_indexes_key_symbols_end_to_end() {
        let (service, temp_dir) = create_test_service();
        fs::write(
            temp_dir.path().join(".env"),
            "API_KEY=secret\nDATABASE_URL=postgres://localhost\n",
        )
        .unwrap();

        service.index_file(".env").unwrap();
        let symbols = service.get_file_symbols_raw(".env").unwrap();
        assert!(
            symbols.iter().any(|s| s.name == "API_KEY"),
            "expected env keys as symbols, got {symbols:?}"
        );
    }

    #[test]
    fn discovery_includes_extensionless_shebang_scripts() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("bin")).unwrap();
        fs::write(
            temp_dir.path().join("bin/deploy"),
            "#!/usr/bin/env python3\nprint('hi')\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("bin/notes"),
            "just some plain text, no shebang\n",
        )
        .unwrap();
        fs::write(temp_dir.path().join("bin/unknown"), "#!/bin/false\n").unwrap();

        let discovered = service.supported_language_files(".");
        assert!(
            discovered.iter().any(|p| p == "bin/deploy"),
            "{discovered:?}"
        );
        assert!(
            !discovered.iter().any(|p| p == "bin/notes"),
            "no-shebang extensionless files stay excluded: {discovered:?}"
        );
        assert!(
            !discovered.iter().any(|p| p == "bin/unknown"),
            "unknown-interpreter shebangs stay excluded: {discovered:?}"
        );
    }

    #[test]
    fn supported_language_files_respects_gitignore_by_default() {
        let (service, temp_dir) = create_test_service();

        fs::write(temp_dir.path().join(".gitignore"), "generated/\n").unwrap();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        fs::create_dir_all(temp_dir.path().join("generated")).unwrap();
        fs::write(
            temp_dir.path().join("src/main.ts"),
            "export const main = 1;",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("generated/client.ts"),
            "export const generated = 1;",
        )
        .unwrap();

        let files = service.supported_language_files(".");

        assert!(files.iter().any(|path| path == "src/main.ts"));
        assert!(!files.iter().any(|path| path == "generated/client.ts"));
    }

    #[test]
    fn supported_language_files_can_include_gitignored_files_when_allowed() {
        let (service, temp_dir) = create_test_service();
        let mut settings = project_settings::ProjectSettings::default();
        settings.allow_gitignored_files = true;
        project_settings::save_project_settings(temp_dir.path(), &settings).unwrap();

        fs::write(temp_dir.path().join(".gitignore"), "generated/\n").unwrap();
        fs::create_dir_all(temp_dir.path().join("generated")).unwrap();
        fs::write(
            temp_dir.path().join("generated/client.ts"),
            "export const generated = 1;",
        )
        .unwrap();

        let files = service.supported_language_files(".");

        assert!(files.iter().any(|path| path == "generated/client.ts"));
    }

    #[test]
    fn resolve_import_target_normalizes_parent_segments() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("scripts")).unwrap();
        fs::create_dir_all(temp_dir.path().join("src/lib")).unwrap();
        fs::write(temp_dir.path().join("scripts/fix.ts"), "").unwrap();
        fs::write(temp_dir.path().join("src/lib/availability.ts"), "").unwrap();

        let resolved = service
            .resolve_import_target("scripts/fix.ts", "../src/lib/availability")
            .unwrap();

        assert_eq!(resolved, "src/lib/availability.ts");
    }

    #[test]
    fn resolve_import_target_uses_language_capability_extensions() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        fs::write(temp_dir.path().join("src/app.ts"), "").unwrap();
        fs::write(temp_dir.path().join("src/theme.css"), ".button {}").unwrap();
        fs::write(temp_dir.path().join("src/bootstrap.php"), "<?php").unwrap();

        assert_eq!(
            service.resolve_import_target("src/app.ts", "./theme"),
            Some("src/theme.css".to_string())
        );
        assert_eq!(
            service.resolve_import_target("src/app.ts", "./bootstrap"),
            Some("src/bootstrap.php".to_string())
        );
    }

    #[test]
    fn global_search_refreshes_a_stale_file_so_its_renamed_symbol_is_found() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("search.ts"),
            "export function oldSymbol() {}",
        )
        .unwrap();
        service.index_file("search.ts").unwrap();
        fs::write(
            temp_dir.path().join("search.ts"),
            "export function GitCommitMessage() {}",
        )
        .unwrap();

        // A global search (file_path = None) refreshes stale indexed files BEFORE
        // matching (search_symbols_filtered_with_patterns -> refresh_stale_indexed_files),
        // so the renamed symbol is found directly. The literal-repair healing path is
        // a fallback for query text that is not a symbol name — exercised by
        // `self_healing_search_returns_literal_fallback_matches`.
        let outcome = service
            .search_symbols_filtered_self_healing("GitCommitMessage", None, None, 10)
            .unwrap();

        assert!(
            outcome
                .results
                .iter()
                .any(|result| result.symbol.name == "GitCommitMessage"),
            "renamed symbol must be found after its file changed: {:?}",
            outcome.results
        );
        assert!(
            !outcome
                .results
                .iter()
                .any(|result| result.symbol.name == "oldSymbol"),
            "the stale symbol must be gone after the refresh: {:?}",
            outcome.results
        );
    }

    #[test]
    fn self_healing_search_returns_literal_fallback_matches() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("config.ts"),
            "export const endpoint = \"BladeProtocolGateway\";",
        )
        .unwrap();

        let outcome = service
            .search_symbols_filtered_self_healing("BladeProtocolGateway", None, None, 10)
            .unwrap();

        assert!(outcome.healing.triggered);
        assert!(outcome.results.is_empty());
        assert!(outcome
            .healing
            .literal_matches
            .iter()
            .any(|fallback| fallback.file_path == "config.ts"
                && fallback.preview.contains("BladeProtocolGateway")));
    }

    #[test]
    fn semantic_anchors_are_indexed_and_searchable() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("anchors.ts"),
            r#"
            export const commandName = "BladeProtocolGateway";
            export const serviceName = "BladeGatewayService";
            export const route = "/api/blade/events";
            const cssToken = "--accent-ai";
            "#,
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("routes.yaml"),
            r#"
routes:
  - path: /api/blade/events
    destination: /internal/blade/events
metadata:
  label: ignored
"#,
        )
        .unwrap();

        service.index_file("anchors.ts").unwrap();
        service.index_file("routes.yaml").unwrap();
        let anchors = service
            .search_semantic_anchors("BladeProtocolGateway", None, 10)
            .unwrap();

        assert!(anchors
            .iter()
            .any(|result| result.anchor.file_path == "anchors.ts"
                && result.anchor.value == "BladeProtocolGateway"));
        assert!(service
            .search_semantic_anchors("BladeGatewayService", None, 10)
            .unwrap()
            .iter()
            .any(|result| result.anchor.kind == "service_name"));
        assert!(service
            .search_semantic_anchors("/api/blade/events", None, 10)
            .unwrap()
            .iter()
            .any(|result| result.anchor.kind == "route"));
        assert!(service
            .search_semantic_anchors("/internal/blade/events", None, 10)
            .unwrap()
            .iter()
            .any(
                |result| result.anchor.kind == "route" && result.anchor.file_path == "routes.yaml"
            ));
        assert!(service
            .search_semantic_anchors("--accent-ai", None, 10)
            .unwrap()
            .iter()
            .any(|result| result.anchor.kind == "css_token"));
    }

    #[test]
    fn rationale_anchors_attach_to_the_nearest_enclosing_symbol() {
        let (service, temp_dir) = create_test_service();
        fs::write(
            temp_dir.path().join("decision.ts"),
            r#"
export function chooseTransport() {
    // WHY: WebSockets preserve the bidirectional session semantics.
    return "websocket";
}
"#,
        )
        .unwrap();

        service.index_file("decision.ts").unwrap();
        let owner = service
            .search_symbols_filtered("chooseTransport", Some("decision.ts"), None, 5)
            .unwrap()
            .remove(0)
            .symbol;
        let rationales = service
            .search_semantic_anchors("bidirectional session", Some("decision.ts"), 5)
            .unwrap();

        assert_eq!(rationales.len(), 1);
        assert_eq!(rationales[0].anchor.kind, "rationale");
        assert_eq!(
            rationales[0].anchor.owner_symbol_id.as_deref(),
            Some(owner.id.as_str())
        );
    }

    /// Track H — a standalone `{/* … */}` comment inside a component is indexed
    /// as a `section_label` anchor owned by the enclosing component symbol; an
    /// inline (non-standalone) JSX comment on a code line is not.
    #[test]
    fn jsx_section_labels_index_standalone_comments_with_owning_component() {
        let (service, temp_dir) = create_test_service();
        fs::write(
            temp_dir.path().join("nav.tsx"),
            r#"
export function MobileNav() {
    return (
        <nav>
            {/* Mobile navigation */}
            <a href="/">Home</a> {/* inline trailing comment */}
        </nav>
    );
}
"#,
        )
        .unwrap();

        service.index_file("nav.tsx").unwrap();
        let owner = service
            .search_symbols_filtered("MobileNav", Some("nav.tsx"), None, 5)
            .unwrap()
            .remove(0)
            .symbol;

        let labels = service
            .search_semantic_anchors("Mobile navigation", Some("nav.tsx"), 10)
            .unwrap()
            .into_iter()
            .filter(|result| result.anchor.kind == "section_label")
            .collect::<Vec<_>>();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].anchor.value, "Mobile navigation");
        assert_eq!(
            labels[0].anchor.owner_symbol_id.as_deref(),
            Some(owner.id.as_str())
        );
        // Section labels are semantic evidence, never graph-looking links: no
        // target fields, so the anchor-target backfill (keyed on target_name)
        // can never resolve them.
        assert!(labels[0].anchor.target_name.is_none());
        assert!(labels[0].anchor.target_symbol_id.is_none());

        // The trailing comment shares a line with JSX — not a section boundary.
        assert!(service
            .search_semantic_anchors("inline trailing comment", Some("nav.tsx"), 10)
            .unwrap()
            .iter()
            .all(|result| result.anchor.kind != "section_label"));
    }

    /// Track H — section-label boundaries: only exact standalone single-line
    /// comments, labels bounded at 120 chars, and only JSX-capable files.
    #[test]
    fn jsx_section_labels_reject_inline_overlong_and_non_jsx_lines() {
        let long_label = "x".repeat(121);
        let content = format!(
            "{{/* Mobile navigation */}}\n\
             <nav> {{/* not standalone */}}\n\
             {{/* a */}}{{/* b */}}\n\
             {{/* trailing */}} <div>\n\
             {{/*   */}}\n\
             {{/* {long_label} */}}\n"
        );
        let labels = extract_semantic_anchors("app/nav.tsx", &content)
            .into_iter()
            .filter(|anchor| anchor.kind == "section_label")
            .collect::<Vec<_>>();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].value, "Mobile navigation");
        assert_eq!(labels[0].line, 0);
        assert!((labels[0].confidence - 0.9).abs() < 1e-6);

        // Exactly 120 chars is still within bounds.
        let at_limit = "y".repeat(120);
        assert!(
            extract_semantic_anchors("app/nav.tsx", &format!("{{/* {at_limit} */}}\n"))
                .iter()
                .any(|anchor| anchor.kind == "section_label" && anchor.value == at_limit)
        );

        // `.ts` cannot contain JSX, and outside the JSX family the shape is not
        // a comment at all — no section labels there.
        for path in ["app/nav.ts", "notes.md", "styles.css"] {
            assert!(
                extract_semantic_anchors(path, "{/* Mobile navigation */}\n")
                    .iter()
                    .all(|anchor| anchor.kind != "section_label"),
                "{path} must not produce section labels"
            );
        }
    }

    /// Track H — the backtick-parity heuristic: a `{/* ... */}` line inside a
    /// template literal is DATA and must not mint a section label; a real one
    /// after the literal closes still does.
    #[test]
    fn jsx_section_labels_skip_comment_lines_inside_template_literals() {
        let content = "const fixture = `\n\
             {/* Inside template literal */}\n\
             `;\n\
             {/* Real section */}\n";
        let labels = extract_semantic_anchors("app/nav.tsx", content)
            .into_iter()
            .filter(|anchor| anchor.kind == "section_label")
            .collect::<Vec<_>>();
        assert_eq!(labels.len(), 1, "template-literal contents must be skipped");
        assert_eq!(labels[0].value, "Real section");
        assert_eq!(labels[0].line, 3);

        // An escaped backtick does not close the literal; the label two lines
        // later is still inside and must be skipped.
        let escaped = "const fixture = `\n\
             text with \\` escaped backtick\n\
             {/* Still inside */}\n\
             `;\n";
        assert!(
            extract_semantic_anchors("app/nav.tsx", escaped)
                .iter()
                .all(|anchor| anchor.kind != "section_label"),
            "escaped backticks must not close the template literal"
        );

        // A line whose backticks OPEN AND CLOSE a literal (even parity) does
        // not poison subsequent lines.
        let balanced = "const label = `inline`;\n\
             {/* After balanced literal */}\n";
        assert!(extract_semantic_anchors("app/nav.tsx", balanced)
            .iter()
            .any(|anchor| anchor.kind == "section_label"
                && anchor.value == "After balanced literal"));
    }

    /// Track H — the mode-aware wrapper delegates to the store with the same
    /// freshness pre-step as the plain search.
    #[test]
    fn search_semantic_anchors_mode_delegates_to_the_store() {
        let (service, temp_dir) = create_test_service();
        fs::write(
            temp_dir.path().join("toolbar.tsx"),
            r#"
export function Toolbar() {
    return (
        <div>
            {/* Right side actions */}
            <button>Go</button>
        </div>
    );
}
"#,
        )
        .unwrap();
        service.index_file("toolbar.tsx").unwrap();

        let outcome = service
            .search_semantic_anchors_mode(
                "Right side actions",
                Some("toolbar.tsx"),
                10,
                AnchorQueryMode::Phrase,
            )
            .unwrap();
        assert!(!outcome.empty_query);
        assert!(outcome
            .results
            .iter()
            .any(|result| result.anchor.kind == "section_label"
                && result.anchor.value == "Right side actions"));
    }

    /// Track F — `modelled_relationship_kinds` reports the extractor's
    /// per-language OBSERVATIONAL coverage (kinds it emits at all, not
    /// exhaustively), empty for languages that model no edges.
    #[test]
    fn modelled_relationship_kinds_report_honest_per_language_coverage() {
        assert!(modelled_relationship_kinds(Language::Rust).contains(&"call"));
        // Track B: file-local Rust constant references are `usage` edges.
        assert!(modelled_relationship_kinds(Language::Rust).contains(&"usage"));
        // Track C: Go `implements` comes from the implicit-interface mining pass.
        assert!(modelled_relationship_kinds(Language::Go).contains(&"implements"));
        assert!(modelled_relationship_kinds(Language::Tsx).contains(&"handles"));
        assert!(!modelled_relationship_kinds(Language::Python).contains(&"implements"));
        assert!(modelled_relationship_kinds(Language::Markdown).is_empty());
        // C/C++ is definitions-only (N8): the relationship walk is gated off.
        assert!(modelled_relationship_kinds(Language::Cpp).is_empty());
    }

    #[test]
    fn markdown_links_and_code_mentions_resolve_without_guessing_ambiguity() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("docs")).unwrap();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        fs::write(
            temp_dir.path().join("src/service.ts"),
            "export function processOrder() { return true; }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("docs/architecture.md"),
            "# Architecture\n\n## Request Flow\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("docs/decision.md"),
            "# ADR-0042\n\nUse `processOrder`; see [request flow](architecture.md#request-flow).\n",
        )
        .unwrap();

        service.reconcile_index().unwrap();
        let process_order = service
            .search_symbols_filtered("processOrder", Some("src/service.ts"), None, 5)
            .unwrap()
            .remove(0)
            .symbol;
        let request_flow = service
            .search_symbols_filtered("Request Flow", Some("docs/architecture.md"), None, 5)
            .unwrap()
            .remove(0)
            .symbol;
        let code_mentions = service
            .search_semantic_anchors("processOrder", Some("docs/decision.md"), 5)
            .unwrap();
        let doc_links = service
            .search_semantic_anchors("architecture.md#request-flow", Some("docs/decision.md"), 5)
            .unwrap();

        assert!(code_mentions.iter().any(|result| {
            result.anchor.kind == "design_symbol_reference"
                && result.anchor.target_symbol_id.as_deref() == Some(process_order.id.as_str())
                && result.anchor.owner_symbol_id.is_some()
        }));
        assert!(doc_links.iter().any(|result| {
            result.anchor.kind == "documentation_link"
                && result.anchor.target_file_path.as_deref() == Some("docs/architecture.md")
                && result.anchor.target_symbol_id.as_deref() == Some(request_flow.id.as_str())
        }));
    }

    #[test]
    fn ambiguous_markdown_code_mentions_remain_unresolved() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("docs")).unwrap();
        fs::write(
            temp_dir.path().join("first.ts"),
            "export function sharedHandler() {}\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("second.ts"),
            "export function sharedHandler() {}\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("docs/decision.md"),
            "# Decision\n\nCall `sharedHandler`.\n",
        )
        .unwrap();

        service.reconcile_index().unwrap();
        let mentions = service
            .search_semantic_anchors("sharedHandler", Some("docs/decision.md"), 5)
            .unwrap();

        assert!(mentions.iter().any(|result| {
            result.anchor.kind == "design_symbol_reference"
                && result.anchor.target_name.as_deref() == Some("sharedHandler")
                && result.anchor.target_symbol_id.is_none()
        }));
    }

    #[test]
    fn translation_json_resources_are_anchor_indexed_and_searchable() {
        let (service, temp_dir) = create_test_service();

        fs::create_dir_all(temp_dir.path().join("locales")).unwrap();
        fs::write(
            temp_dir.path().join("locales/en.json"),
            r#"{
                "auth": {
                    "login": {
                        "title": "Sign in to continue",
                        "submit": "Continue"
                    }
                }
            }"#,
        )
        .unwrap();

        let report = service.reconcile_index().unwrap();
        let key_matches = service
            .search_semantic_anchors("auth.login.title", None, 10)
            .unwrap();
        let text_matches = service
            .search_semantic_anchors("Sign in to continue", None, 10)
            .unwrap();

        assert!(report.files_indexed >= 1);
        assert_eq!(report.graph_quality.indexed_files_missing_root_symbol, 0);
        assert!(key_matches.iter().any(|result| {
            result.anchor.file_path == "locales/en.json"
                && result.anchor.kind == "translation_definition_key"
                && result.anchor.value == "auth.login.title"
        }));
        assert!(text_matches.iter().any(|result| {
            result.anchor.file_path == "locales/en.json"
                && result.anchor.kind == "translation_text"
                && result.anchor.value == "Sign in to continue"
        }));
    }

    #[test]
    fn translation_usage_keys_are_anchor_indexed() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("page.tsx"),
            r#"
            import { useTranslations } from "next-intl";

            export function Page() {
                const t = useTranslations("auth.login");
                return <button>{t("submit")}</button>;
            }
            "#,
        )
        .unwrap();

        service.index_file("page.tsx").unwrap();
        let anchors = service
            .search_semantic_anchors("auth.login.submit", None, 10)
            .unwrap();

        assert!(anchors.iter().any(|result| {
            result.anchor.file_path == "page.tsx"
                && result.anchor.kind == "translation_usage_key"
                && result.anchor.value == "auth.login.submit"
        }));
    }

    #[test]
    fn translation_call_aliases_are_indexed_and_suppressed_as_graph_noise() {
        let (service, temp_dir) = create_test_service();
        let content = r#"
            import { useTranslations } from "next-intl";

            export function StudioDialog(): string {
                const translateStudio = useTranslations("studio");
                const title = translateStudio("create.title");
                return title;
            }
            "#;

        fs::write(temp_dir.path().join("studio.ts"), content).unwrap();

        let aliases = extract_translation_call_aliases(content);
        let mut relationships = vec![SymbolRelationship {
            source_symbol_id: "studio.ts::StudioDialog#function".to_string(),
            source_file_path: "studio.ts".to_string(),
            target_name: "translateStudio".to_string(),
            target_symbol_id: None,
            relationship_type: SymbolRelationshipType::Call,
            line: 5,
            ..Default::default()
        }];
        assert_eq!(
            LanguageService::suppress_known_external_relationships(
                Language::TypeScript,
                &mut relationships,
                &aliases,
            ),
            1
        );
        assert!(relationships.is_empty());

        service.reconcile_index().unwrap();
        let anchors = service
            .search_semantic_anchors("studio.create.title", None, 10)
            .unwrap();

        assert!(anchors.iter().any(|result| {
            result.anchor.file_path == "studio.ts"
                && result.anchor.kind == "translation_usage_key"
                && result.anchor.value == "studio.create.title"
        }));
    }

    #[test]
    fn semantic_anchor_extraction_handles_multibyte_delimiters() {
        let anchors = extract_semantic_anchors("notes.md", "    The correct focus for €1M:\n");

        assert!(!anchors.iter().any(|anchor| anchor.value == "1M"));
    }

    /// M5.14 — the generic literal/token anchor scan is gated to front-end/style/
    /// markup files. A C++ string literal on an assignment line used to mint a
    /// `config_key` anchor (part of Firefox's ~5M C++ anchor noise); systems files
    /// now produce no generic anchors, while the same literal still anchors in TS.
    #[test]
    fn generic_literal_anchors_are_gated_to_frontend_languages() {
        let cpp = extract_semantic_anchors(
            "src/foo.cpp",
            "const char* kName = \"BladeProtocolGateway\";\n",
        );
        assert!(
            cpp.is_empty(),
            "systems-language files must not mint generic literal anchors (got {})",
            cpp.len()
        );

        let ts =
            extract_semantic_anchors("src/foo.ts", "const kName = \"BladeProtocolGateway\";\n");
        assert!(
            ts.iter()
                .any(|anchor| anchor.value == "BladeProtocolGateway"),
            "front-end files still anchor string-literal cross-references"
        );
    }

    #[test]
    fn self_healing_search_returns_semantic_anchor_matches() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("anchors.ts"),
            "export const endpoint = \"BladeProtocolGateway\";",
        )
        .unwrap();
        service.index_file("anchors.ts").unwrap();

        let outcome = service
            .search_symbols_filtered_self_healing("BladeProtocolGateway", None, None, 10)
            .unwrap();

        assert!(outcome.healing.triggered);
        assert!(outcome.results.is_empty());
        assert!(outcome
            .healing
            .semantic_anchor_matches
            .iter()
            .any(|result| result.anchor.file_path == "anchors.ts"
                && result.anchor.value == "BladeProtocolGateway"));
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
        assert_eq!(report.graph_quality.missing_source_symbols, 0);
        assert_eq!(report.graph_quality.missing_target_symbols, 0);
        assert_eq!(report.graph_quality.indexed_files_missing_root_symbol, 0);
        assert!(results
            .iter()
            .any(|result| result.symbol.name == "GitCommitMessage"));
        assert!(service.get_file_symbols_raw("old.ts").unwrap().is_empty());
    }

    #[test]
    fn reconcile_stores_a_no_change_checkpoint_when_fully_fresh() {
        let (service, temp_dir) = create_test_service();
        fs::write(temp_dir.path().join("a.ts"), "export function alpha() {}").unwrap();
        fs::write(temp_dir.path().join("b.ts"), "export function beta() {}").unwrap();

        let report = service.reconcile_index().unwrap();
        assert_eq!(report.health.status, IndexHealthStatus::Fresh);
        assert!(!report.fast_path, "the first reconcile must do full work");

        // A fully-fresh reconcile persists a checkpoint whose fingerprint matches the
        // current worktree state.
        let checkpoint = service
            .load_reconcile_checkpoint()
            .expect("checkpoint stored after a fresh reconcile");
        let current = service
            .compute_reconcile_fingerprint(&service.supported_language_files("."))
            .expect("fingerprint computable");
        assert_eq!(checkpoint.fingerprint, current);
        assert_eq!(checkpoint.health.status, IndexHealthStatus::Fresh);
    }

    #[test]
    fn reconcile_takes_the_fast_path_when_nothing_changed() {
        let (service, temp_dir) = create_test_service();
        fs::write(temp_dir.path().join("a.ts"), "export function alpha() {}").unwrap();

        let first = service.reconcile_index().unwrap();
        assert!(!first.fast_path);
        assert_eq!(first.health.status, IndexHealthStatus::Fresh);

        let second = service.reconcile_index().unwrap();
        assert!(
            second.fast_path,
            "an unchanged worktree must short-circuit via the fast path"
        );
        assert_eq!(second.files_indexed, 0);
        assert_eq!(second.files_removed, 0);
        assert_eq!(second.health.status, IndexHealthStatus::Fresh);
        assert_eq!(second.health.symbol_count, first.health.symbol_count);
    }

    #[test]
    fn reconcile_skips_the_fast_path_after_a_file_changes() {
        let (service, temp_dir) = create_test_service();
        let file = temp_dir.path().join("a.ts");
        fs::write(&file, "export function alpha() {}").unwrap();

        let first = service.reconcile_index().unwrap();
        assert!(!first.fast_path);
        let fingerprint_before = service
            .compute_reconcile_fingerprint(&service.supported_language_files("."))
            .unwrap();

        // Change the file's content (and therefore its size and mtime).
        fs::write(
            &file,
            "export function alpha() {}\nexport function gamma() {}",
        )
        .unwrap();

        let fingerprint_after = service
            .compute_reconcile_fingerprint(&service.supported_language_files("."))
            .unwrap();
        assert_ne!(
            fingerprint_before, fingerprint_after,
            "a content change must change the fingerprint"
        );

        let third = service.reconcile_index().unwrap();
        assert!(
            !third.fast_path,
            "a changed worktree must NOT take the fast path"
        );
        assert!(
            third.files_indexed >= 1,
            "the changed file must be re-indexed"
        );
        assert_eq!(third.health.status, IndexHealthStatus::Fresh);
    }

    #[test]
    fn reconcile_fast_path_is_invalidated_when_the_index_is_cleared() {
        let (service, temp_dir) = create_test_service();
        fs::write(temp_dir.path().join("a.ts"), "export function alpha() {}").unwrap();

        service.reconcile_index().unwrap();
        assert!(service.load_reconcile_checkpoint().is_some());

        // Clearing generated index data must drop the checkpoint so the next reconcile
        // rebuilds instead of trusting a stale "Fresh".
        service.symbol_store.clear_generated_index_data().unwrap();
        assert!(
            service.load_reconcile_checkpoint().is_none(),
            "clearing the index must invalidate the fast-path checkpoint"
        );

        let after_clear = service.reconcile_index().unwrap();
        assert!(
            !after_clear.fast_path,
            "the reconcile after a clear must do full work"
        );
        assert!(after_clear.files_indexed >= 1);
    }

    #[test]
    fn index_file_refreshes_when_extractor_version_changes() {
        let (service, temp_dir) = create_test_service();
        let content = "export function versionedSymbol() { return 42; }\n";
        fs::write(temp_dir.path().join("versioned.ts"), content).unwrap();

        service.index_file("versioned.ts").unwrap();
        let initial_record = service
            .symbol_store
            .indexed_file_record("versioned.ts")
            .unwrap()
            .unwrap();
        let expected_version = LanguageService::extractor_version_for_index_file("versioned.ts");
        assert_eq!(initial_record.extractor_version, expected_version);

        service
            .symbol_store
            .mark_file_indexed_with_metadata_and_extractor_version(
                "versioned.ts",
                &initial_record.file_hash,
                initial_record.symbol_count,
                initial_record.file_size,
                initial_record.line_count,
                initial_record.modified_at,
                Some(0),
            )
            .unwrap();

        let stale_record = service
            .symbol_store
            .indexed_file_record("versioned.ts")
            .unwrap()
            .unwrap();
        assert!(service
            .indexed_file_needs_refresh("versioned.ts", &stale_record, false)
            .unwrap());

        service.index_file("versioned.ts").unwrap();
        let refreshed_record = service
            .symbol_store
            .indexed_file_record("versioned.ts")
            .unwrap()
            .unwrap();
        assert_eq!(refreshed_record.extractor_version, expected_version);
    }

    #[test]
    fn reconcile_index_rebuilds_once_when_graph_integrity_is_hard_broken() {
        let (service, temp_dir) = create_test_service();

        let content = "export function repairedSymbol() { return 42; }\n";
        fs::write(temp_dir.path().join("broken.ts"), content).unwrap();
        service
            .symbol_store
            .replace_file_index(
                "broken.ts",
                &compute_hash(content),
                None,
                Some(source_line_count(content)),
                None,
                None,
                &[],
                &[],
                &[],
            )
            .unwrap();

        let report = service.reconcile_index().unwrap();
        let symbols = service.get_file_symbols_raw("broken.ts").unwrap();

        assert_eq!(report.health.status, IndexHealthStatus::Fresh);
        assert!(report.files_indexed >= 1);
        assert_eq!(report.graph_quality.indexed_files_missing_root_symbol, 0);
        assert!(symbols
            .iter()
            .any(|symbol| symbol.id == LanguageService::synthetic_file_root_id("broken.ts")));
        assert!(symbols.iter().any(|symbol| symbol.name == "repairedSymbol"));
    }

    #[test]
    fn reconcile_index_resolves_relationships_after_batch_symbol_commit() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("main.ts"),
            "import { helper } from './utils';\nexport function run() {\n  helper();\n}",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("utils.ts"),
            "export function helper() {\n  return 42;\n}",
        )
        .unwrap();

        let report = service.reconcile_index().unwrap();
        let helper = service
            .search_symbols("helper", 10)
            .unwrap()
            .into_iter()
            .find(|result| result.symbol.name == "helper")
            .map(|result| result.symbol)
            .unwrap();
        let references = service.find_references_to_symbol(&helper, 10).unwrap();

        assert_eq!(report.files_indexed, 2);
        assert_eq!(report.graph_quality.missing_source_symbols, 0);
        assert_eq!(report.graph_quality.missing_target_symbols, 0);
        assert_eq!(report.graph_quality.indexed_files_missing_root_symbol, 0);
        assert!(report.graph_quality.total_relationships > 0);
        assert!(report.graph_quality.resolved_relationships > 0);
        assert!(references.iter().any(|reference| {
            reference.source_symbol.file_path == "main.ts"
                && reference.target_symbol_id.as_deref() == Some(helper.id.as_str())
        }));
    }

    #[test]
    fn trace_symbol_graph_returns_direct_outgoing_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("trace.ts"),
            "export function leaf() {}\nexport function middle() { leaf(); }\nexport function root() { middle(); }\n",
        )
        .unwrap();
        service.index_file("trace.ts").unwrap();
        let root = service
            .search_symbols("root", 10)
            .unwrap()
            .into_iter()
            .find(|result| result.symbol.name == "root")
            .map(|result| result.symbol)
            .unwrap();

        let trace = service
            .trace_symbol_graph(
                &root,
                &[SymbolRelationshipType::Call],
                SymbolTraceDirection::Outgoing,
                1,
                10,
                10,
            )
            .unwrap();

        assert!(!trace.truncated);
        assert_eq!(trace.unresolved_edges, 0);
        assert!(trace.edges.iter().any(|edge| {
            edge.source_symbol.name == "root"
                && edge
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("middle")
                && edge.depth == 1
        }));
    }

    #[test]
    fn trace_symbol_graph_returns_bounded_multihop_edges() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("trace.ts"),
            "export function leaf() {}\nexport function middle() { leaf(); }\nexport function root() { middle(); }\n",
        )
        .unwrap();
        service.index_file("trace.ts").unwrap();
        let root = service
            .search_symbols("root", 10)
            .unwrap()
            .into_iter()
            .find(|result| result.symbol.name == "root")
            .map(|result| result.symbol)
            .unwrap();

        let trace = service
            .trace_symbol_graph(
                &root,
                &[SymbolRelationshipType::Call],
                SymbolTraceDirection::Outgoing,
                2,
                10,
                10,
            )
            .unwrap();

        assert!(trace
            .nodes
            .iter()
            .any(|node| { node.symbol.name == "middle" && node.depth == 1 }));
        assert!(trace
            .nodes
            .iter()
            .any(|node| { node.symbol.name == "leaf" && node.depth == 2 }));
        assert!(trace.edges.iter().any(|edge| {
            edge.source_symbol.name == "middle"
                && edge
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.name.as_str())
                    == Some("leaf")
                && edge.depth == 2
        }));
    }

    #[test]
    fn symbol_path_finds_confidence_weighted_multihop_route() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("path.ts"),
            "export function leaf() {}\nexport function middle() { leaf(); }\nexport function root() { middle(); }\n",
        )
        .unwrap();
        service.index_file("path.ts").unwrap();
        let symbols = service.get_file_symbols("path.ts").unwrap();
        let root = symbols.iter().find(|symbol| symbol.name == "root").unwrap();
        let leaf = symbols.iter().find(|symbol| symbol.name == "leaf").unwrap();

        let path = service
            .find_symbol_path(
                root,
                leaf,
                &[SymbolRelationshipType::Call],
                SymbolTraceDirection::Outgoing,
                4,
                40,
                10,
                0.5,
            )
            .unwrap();

        assert_eq!(path.edges.len(), 2);
        assert_eq!(path.edges[0].source_symbol.name, "root");
        assert_eq!(
            path.edges[0]
                .target_symbol
                .as_ref()
                .map(|symbol| symbol.name.as_str()),
            Some("middle")
        );
        assert_eq!(
            path.edges[1]
                .target_symbol
                .as_ref()
                .map(|symbol| symbol.name.as_str()),
            Some("leaf")
        );
        assert!(path.total_cost > 0);
    }

    #[test]
    fn symbol_path_cost_penalizes_uncertain_and_structural_edges() {
        let exact_call = LanguageService::symbol_path_edge_cost(
            SymbolRelationshipType::Call,
            1.0,
            RelationshipObservationKind::SyntaxExtracted,
        );
        let uncertain_call = LanguageService::symbol_path_edge_cost(
            SymbolRelationshipType::Call,
            0.5,
            RelationshipObservationKind::SyntaxExtracted,
        );
        let structural = LanguageService::symbol_path_edge_cost(
            SymbolRelationshipType::Contains,
            1.0,
            RelationshipObservationKind::IndexStructural,
        );

        assert!(exact_call < uncertain_call);
        assert!(uncertain_call < structural);
    }

    #[test]
    fn reconcile_index_suppresses_known_external_calls_after_resolution() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("main.ts"),
            "export function run(items: string[]) {\n  return items.map((item) => item.trim());\n}",
        )
        .unwrap();

        let report = service.reconcile_index().unwrap();

        assert!(report.graph_quality.suppressed_external_relationships >= 2);
        assert!(!report
            .graph_quality
            .top_unresolved_targets
            .iter()
            .any(|target| target.relationship_type == "call"
                && matches!(target.target_name.as_str(), "map" | "trim")));
    }

    #[test]
    fn external_named_call_is_suppressed_not_backfilled_to_unique_project_symbol() {
        // library-named call resolution deferred to M5.1 (receiver typing);
        // bare-name back-fill cannot distinguish project `parse()` from
        // `JSON.parse()`. `findUnique` is a known external/library method name, so
        // the unresolved `findUnique()` call in main.ts (which does NOT import
        // project-api.ts) is suppressed at enrichment time rather than persisted
        // as a NULL edge and mis-wired by the global-unique back-fill to the
        // project symbol that merely shares its bare name.
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("main.ts"),
            "export function run() {\n  return findUnique();\n}",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("project-api.ts"),
            "export function findUnique() {\n  return 42;\n}",
        )
        .unwrap();

        service.reconcile_index().unwrap();
        let target = service
            .search_symbols("findUnique", 10)
            .unwrap()
            .into_iter()
            .find(|result| result.symbol.file_path == "project-api.ts")
            .map(|result| result.symbol)
            .unwrap();
        let references = service.find_references_to_symbol(&target, 10).unwrap();

        assert!(
            !references.iter().any(|reference| {
                reference.source_symbol.file_path == "main.ts"
                    && reference.target_symbol_id.as_deref() == Some(target.id.as_str())
            }),
            "known external-named call must be suppressed, not back-filled to a same-named project symbol"
        );
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
    fn architecture_community_detection_separates_dense_clusters() {
        let make_edge = |source: &str, target: &str, weight: f32| ArchitectureEdge {
            source_file_path: source.to_string(),
            target_file_path: target.to_string(),
            relationship_counts: BTreeMap::from([("call".to_string(), 1)]),
            edge_count: 1,
            average_confidence: 1.0,
            weight,
        };
        let paths = ["a", "b", "c", "d", "e", "f"]
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let edges = vec![
            make_edge("a", "b", 5.0),
            make_edge("a", "c", 5.0),
            make_edge("b", "c", 5.0),
            make_edge("d", "e", 5.0),
            make_edge("d", "f", 5.0),
            make_edge("e", "f", 5.0),
            make_edge("c", "d", 0.1),
        ];

        let labels = architecture_community_labels(&paths, &edges);

        assert_eq!(labels["a"], labels["b"]);
        assert_eq!(labels["b"], labels["c"]);
        assert_eq!(labels["d"], labels["e"]);
        assert_eq!(labels["e"], labels["f"]);
        assert_ne!(labels["c"], labels["d"]);
    }

    #[test]
    fn architecture_snapshot_surfaces_hubs_communities_and_bridges() {
        let (service, temp_dir) = create_test_service();
        fs::create_dir_all(temp_dir.path().join("src/auth")).unwrap();
        fs::create_dir_all(temp_dir.path().join("src/billing")).unwrap();
        fs::write(
            temp_dir.path().join("src/auth/session.ts"),
            "export function createSession() { return 'session'; }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/auth/login.ts"),
            "import { createSession } from './session';\nexport function login() { return createSession(); }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/billing/tax.ts"),
            "export function calculateTax() { return 20; }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/billing/invoice.ts"),
            "import { calculateTax } from './tax';\nimport { createSession } from '../auth/session';\nexport function invoice() { createSession(); return calculateTax(); }\n",
        )
        .unwrap();

        service.reconcile_index().unwrap();
        let snapshot = service
            .build_architecture_snapshot(
                Some("src"),
                &[SymbolRelationshipType::Call, SymbolRelationshipType::Import],
                0.5,
                20,
                40,
                10,
            )
            .unwrap();

        assert!(snapshot.modules.len() >= 4);
        assert!(!snapshot.hubs.is_empty());
        assert!(snapshot.communities.len() >= 2);
        assert!(!snapshot.bridge_edges.is_empty());
        assert!(!snapshot.bridge_modules.is_empty());
        assert!(snapshot.edges.iter().any(|edge| {
            edge.source_file_path == "src/billing/invoice.ts"
                && edge.target_file_path == "src/auth/session.ts"
        }));
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
    fn test_get_file_symbols_skips_indexer_when_file_is_fresh() {
        let (service, temp_dir) = create_test_service();

        fs::write(
            temp_dir.path().join("fresh.ts"),
            "export function freshSymbol() { return 1; }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("marker.ts"),
            "export function markerSymbol() { return 2; }\n",
        )
        .unwrap();

        service.index_file("fresh.ts").unwrap();
        service.index_file("marker.ts").unwrap();
        assert_eq!(
            service
                .index_health_snapshot()
                .timings
                .last_file_path
                .as_deref(),
            Some("marker.ts")
        );

        let symbols = service.get_file_symbols("fresh.ts").unwrap();

        assert!(symbols.iter().any(|symbol| symbol.name == "freshSymbol"));
        assert_eq!(
            service
                .index_health_snapshot()
                .timings
                .last_file_path
                .as_deref(),
            Some("marker.ts")
        );
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
    fn test_live_sync_ignores_duplicate_and_stale_snapshots() {
        let (service, _temp_dir) = create_test_service();
        let path = "package.json";

        service.did_open(path, "{ \"name\": \"demo\" }").unwrap();
        service
            .did_change(path, 2, "{ \"name\": \"demo\", \"private\": true }")
            .unwrap();
        service
            .did_change(path, 1, "{ \"name\": \"stale\" }")
            .unwrap();

        let snapshot_key = service.snapshot_key(path);
        let snapshot = service.buffer_snapshots.get(&snapshot_key).unwrap();
        assert_eq!(snapshot.version(), Some(2));
        assert_eq!(
            snapshot.content(),
            "{ \"name\": \"demo\", \"private\": true }"
        );

        service
            .did_change(path, 3, "{ \"name\": \"demo\", \"private\": true }")
            .unwrap();

        let snapshot = service.buffer_snapshots.get(&snapshot_key).unwrap();
        assert_eq!(snapshot.version(), Some(3));
        assert_eq!(
            snapshot.content(),
            "{ \"name\": \"demo\", \"private\": true }"
        );
    }

    #[test]
    fn test_disk_snapshot_reads_refresh_from_disk() {
        let (service, temp_dir) = create_test_service();

        fs::write(temp_dir.path().join("copied.md"), "").unwrap();
        assert_eq!(service.get_file_content("copied.md").unwrap(), "");

        fs::write(
            temp_dir.path().join("copied.md"),
            "# Copied Document\n\nThe file now has content.\n",
        )
        .unwrap();

        assert_eq!(
            service.get_file_content("copied.md").unwrap(),
            "# Copied Document\n\nThe file now has content.\n"
        );
    }

    #[test]
    fn test_live_snapshot_still_overrides_disk_snapshot() {
        let (service, temp_dir) = create_test_service();

        fs::write(temp_dir.path().join("open.md"), "saved content\n").unwrap();
        service
            .did_open("open.md", "unsaved editor content\n")
            .unwrap();
        fs::write(temp_dir.path().join("open.md"), "new disk content\n").unwrap();

        assert_eq!(
            service.get_file_content("open.md").unwrap(),
            "unsaved editor content\n"
        );
    }

    #[test]
    fn unsaved_overlay_overrides_queries_without_mutating_the_persisted_index() {
        let (service, temp_dir) = create_test_service();
        let path = "src/overlay.ts";
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        fs::write(
            temp_dir.path().join(path),
            "export function savedHandler() { return 'disk'; }\n",
        )
        .unwrap();
        service.index_file(path).unwrap();

        service
            .did_open(
                path,
                "export function unsavedTarget() { return 'overlay'; }\nexport function unsavedHandler() { return unsavedTarget(); }\n",
            )
            .unwrap();

        let visible = service.get_file_symbols(path).unwrap();
        assert!(visible.iter().any(|symbol| symbol.name == "unsavedHandler"));
        let target = visible
            .iter()
            .find(|symbol| symbol.name == "unsavedTarget")
            .expect("overlay target");
        let graph = service
            .get_symbol_graph(target, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert!(graph
            .incoming
            .iter()
            .any(|reference| reference.source_symbol.name == "unsavedHandler"));
        assert!(!visible.iter().any(|symbol| symbol.name == "savedHandler"));
        let search = service.search_symbols("unsavedHandler", 10).unwrap();
        assert!(search
            .iter()
            .any(|result| result.symbol.name == "unsavedHandler"));

        let persisted = service.get_file_symbols_raw(path).unwrap();
        assert!(persisted.iter().any(|symbol| symbol.name == "savedHandler"));
        assert!(!persisted
            .iter()
            .any(|symbol| symbol.name == "unsavedHandler"));

        service.did_close(path).unwrap();
        let restored = service.get_file_symbols(path).unwrap();
        assert!(restored.iter().any(|symbol| symbol.name == "savedHandler"));
        assert!(!restored
            .iter()
            .any(|symbol| symbol.name == "unsavedHandler"));
    }

    #[test]
    fn test_non_indexed_documents_still_do_not_enter_symbol_index() {
        let (service, temp_dir) = create_test_service();

        // A genuinely unsupported document type (no language). JSON used to be the
        // example here, but it is a first-class supported language now (M1.3).
        fs::write(temp_dir.path().join("logo.png"), "not really an image").unwrap();

        let error = service
            .index_file("logo.png")
            .expect_err("unsupported documents must not be symbol-indexed");

        assert!(matches!(error, LanguageError::NotSupported(_)));
    }

    // ---- M1.4 — comment/string/heredoc-aware preprocessing ----

    fn assert_length_and_newlines_preserved(input: &str, output: &str) {
        assert_eq!(input.len(), output.len(), "byte length must be preserved");
        let in_nl: Vec<usize> = input.match_indices('\n').map(|(i, _)| i).collect();
        let out_nl: Vec<usize> = output.match_indices('\n').map(|(i, _)| i).collect();
        assert_eq!(in_nl, out_nl, "newline byte positions must be preserved");
    }

    #[test]
    fn blank_noncode_spans_blanks_multiline_block_comment() {
        let src = "package p;\n/*\nclass Foo {\n*/\nclass Bar {}\n";
        let out = blank_noncode_spans(src, &JAVA_LEX);
        assert_length_and_newlines_preserved(src, out.as_str());
        assert!(
            !out.contains("Foo"),
            "block-comment body must be blanked: {out:?}"
        );
        assert!(
            out.contains("class Bar {}"),
            "real code must survive: {out:?}"
        );
    }

    #[test]
    fn blank_noncode_spans_strips_trailing_line_comment() {
        // PHP previously minted a bogus `Baz` class from the trailing comment.
        let src = "return 1; // class Baz\n";
        let out = blank_noncode_spans(src, &PHP_LEX);
        assert_length_and_newlines_preserved(src, out.as_str());
        assert!(out.starts_with("return 1;"));
        assert!(
            !out.contains("class Baz"),
            "trailing comment must be blanked: {out:?}"
        );
    }

    #[test]
    fn blank_noncode_spans_protects_comment_marker_inside_string() {
        // The `//` lives inside a string, so it must NOT start a comment.
        let src = "var url = \"http://x // y\"; // real comment\n";
        let out = blank_noncode_spans(src, &JAVA_LEX);
        assert_length_and_newlines_preserved(src, out.as_str());
        assert!(
            out.contains("var url ="),
            "code before the string survives: {out:?}"
        );
        assert!(
            !out.contains("real comment"),
            "trailing comment is blanked: {out:?}"
        );
    }

    #[test]
    fn blank_noncode_spans_keeps_string_for_string_reading_langs() {
        // Ruby reads the import out of the string literal — it must NOT be blanked.
        let src = "require \"json\" # load\n";
        let out = blank_noncode_spans(src, &RUBY_LEX);
        assert_length_and_newlines_preserved(src, out.as_str());
        assert!(
            out.contains("\"json\""),
            "ruby string content must survive: {out:?}"
        );
        assert!(
            !out.contains("load"),
            "trailing `#` comment is still blanked: {out:?}"
        );
    }

    #[test]
    fn blank_noncode_spans_blanks_php_heredoc_body() {
        let src = "$x = <<<EOT\nclass Hidden {}\nEOT;\nclass Real {}\n";
        let out = blank_noncode_spans(src, &PHP_LEX);
        assert_length_and_newlines_preserved(src, out.as_str());
        assert!(
            !out.contains("Hidden"),
            "heredoc body must be blanked: {out:?}"
        );
        assert!(
            out.contains("class Real {}"),
            "code after EOT survives: {out:?}"
        );
    }

    #[test]
    fn blank_noncode_spans_keeps_php_attribute() {
        // `#[...]` is a PHP 8 attribute, not a `#` line comment.
        let src = "#[Route] class Controller {}\n";
        let out = blank_noncode_spans(src, &PHP_LEX);
        assert_length_and_newlines_preserved(src, out.as_str());
        assert_eq!(out, src, "attribute line must be untouched: {out:?}");
    }

    #[test]
    fn java_scanner_ignores_class_inside_block_comment() {
        let symbols = extract_java_symbols(
            "Demo.java",
            "package com.example;\n/*\nclass Foo {\n    void hidden() {}\n*/\npublic class Bar {}\n",
        );
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Bar"), "real class kept: {names:?}");
        assert!(
            !names.contains(&"Foo"),
            "commented class dropped: {names:?}"
        );
        assert!(
            !names.contains(&"hidden"),
            "commented method dropped: {names:?}"
        );
    }

    #[test]
    fn php_scanner_ignores_trailing_comment_class() {
        let symbols = extract_php_symbols(
            "Service.php",
            "<?php\nclass Bar\n{\n    public function handle()\n    {\n        return 1; // class Baz\n    }\n}\n",
        );
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Bar"), "real class kept: {names:?}");
        assert!(names.contains(&"handle"), "real method kept: {names:?}");
        assert!(
            !names.contains(&"Baz"),
            "trailing-comment class dropped: {names:?}"
        );
    }

    #[test]
    fn ruby_scanner_keeps_require_from_string() {
        // Regression guard: string blanking must stay disabled for Ruby.
        let symbols = extract_ruby_symbols("svc.rb", "require \"json\"\nclass Svc\nend\n");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"json"),
            "require target survives: {names:?}"
        );
        assert!(names.contains(&"Svc"), "class survives: {names:?}");
    }

    // =========================================================================
    // Deterministic test corpus (2026-07-18 handoff, "Deterministic test
    // corpus" + "Release gates"). Fixture files live under
    // `tests/fixtures/symbol_tracks/` and are copied VERBATIM into a fresh
    // TempDir workspace, then indexed by RELATIVE path — tests never index the
    // developer's checkout. Test names are `fixtureN_`-prefixed so the
    // release-gate mapping stays greppable.
    // =========================================================================

    /// Copy one fixture corpus (a subdirectory of
    /// `tests/fixtures/symbol_tracks/`) into the TempDir workspace root,
    /// preserving the corpus's internal relative layout.
    fn write_symbol_track_fixture(workspace_root: &Path, fixture_dir: &str) {
        let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/symbol_tracks")
            .join(fixture_dir);
        copy_fixture_tree(&source_root, workspace_root);
    }

    fn copy_fixture_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination)
            .unwrap_or_else(|error| panic!("create {}: {error}", destination.display()));
        for entry in fs::read_dir(source)
            .unwrap_or_else(|error| panic!("read fixture dir {}: {error}", source.display()))
        {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_fixture_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).unwrap_or_else(|error| {
                    panic!("copy fixture {}: {error}", entry.path().display())
                });
            }
        }
    }

    /// Find one raw symbol of a file by name (panicking with context).
    fn fixture_symbol(service: &LanguageService, file_path: &str, name: &str) -> Symbol {
        service
            .get_file_symbols_raw(file_path)
            .unwrap()
            .into_iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("no symbol named {name:?} in {file_path}"))
    }

    // ---- Fixture 1 — TSX component calls, section labels, ranking ----------

    /// Release gate: "JSX component reference recall — all fixture call sites
    /// returned". Each component is used twice in `Header` and once in
    /// `Settings` → exactly three incoming call observations, at the exact
    /// JSX-use lines (0-based).
    #[test]
    fn fixture1_tsx_components_have_exactly_three_call_observations() {
        let (service, temp_dir) = create_test_service();
        write_symbol_track_fixture(temp_dir.path(), "fixture1_tsx_components");
        service.reconcile_index().unwrap();

        let expected: [(&str, &str, [(&str, u32); 3]); 3] = [
            (
                "LanguageSwitcher",
                "components/language-switcher.tsx",
                [
                    ("components/header.tsx", 8),
                    ("components/header.tsx", 14),
                    ("components/settings.tsx", 7),
                ],
            ),
            (
                "RegionSelector",
                "components/region-selector.tsx",
                [
                    ("components/header.tsx", 9),
                    ("components/header.tsx", 15),
                    ("components/settings.tsx", 8),
                ],
            ),
            (
                "ModeToggle",
                "components/mode-toggle.tsx",
                [
                    ("components/header.tsx", 10),
                    ("components/header.tsx", 16),
                    ("components/settings.tsx", 9),
                ],
            ),
        ];

        for (component, definition_file, expected_sites) in expected {
            let symbol = fixture_symbol(&service, definition_file, component);
            let references = service
                .find_relationship_references_to_symbol(&symbol, SymbolRelationshipType::Call, 50)
                .unwrap();
            let mut observed: Vec<(String, u32)> = references
                .iter()
                .filter(|reference| {
                    reference.relationship_type == SymbolRelationshipType::Call
                })
                .map(|reference| (reference.source_symbol.file_path.clone(), reference.line))
                .collect();
            observed.sort();
            let expected_sites: Vec<(String, u32)> = expected_sites
                .iter()
                .map(|(file, line)| (file.to_string(), *line))
                .collect();
            assert_eq!(
                observed, expected_sites,
                "{component} must have exactly the three fixture call sites"
            );
            for reference in &references {
                assert!(
                    reference.source_symbol.name == "Header"
                        || reference.source_symbol.name == "Settings",
                    "{component} call source must be the enclosing component, got {:?}",
                    reference.source_symbol.name
                );
            }
        }
    }

    /// Release gate: "no native-tag false positives". Native/lowercase JSX
    /// elements (`nav`, `button`, `section`, `select`, `option`) and the
    /// lowercase namespace root `motion` (`<motion.div />`) never emit call
    /// observations.
    #[test]
    fn fixture1_native_and_lowercase_jsx_elements_emit_no_call_observations() {
        let (service, temp_dir) = create_test_service();
        write_symbol_track_fixture(temp_dir.path(), "fixture1_tsx_components");
        service.reconcile_index().unwrap();

        for native in ["nav", "button", "section", "select", "option", "motion", "div"] {
            let references = service
                .symbol_store
                .find_references_to_target(native, SymbolRelationshipType::Call, 10)
                .unwrap();
            assert!(
                references.is_empty(),
                "native/lowercase element {native:?} must not emit call observations: {:?}",
                references
                    .iter()
                    .map(|r| (r.source_symbol.file_path.clone(), r.line))
                    .collect::<Vec<_>>()
            );
        }
    }

    /// Track H — standalone JSX section comments inside the one large `Header`
    /// component are `section_label` anchors: path-narrowed AllTerms search
    /// returns them with the component as owner and exact internal lines.
    #[test]
    fn fixture1_section_label_anchors_are_path_narrowed_and_owned_by_header() {
        let (service, temp_dir) = create_test_service();
        write_symbol_track_fixture(temp_dir.path(), "fixture1_tsx_components");
        service.reconcile_index().unwrap();

        let header = fixture_symbol(&service, "components/header.tsx", "Header");
        for (query, label, line) in [
            ("mobile navigation", "Mobile navigation", 7u32),
            ("right side actions", "Right side actions", 13u32),
        ] {
            let outcome = service
                .search_semantic_anchors_mode(
                    query,
                    Some("components/header.tsx"),
                    10,
                    AnchorQueryMode::AllTerms,
                )
                .unwrap();
            assert!(!outcome.empty_query);
            let hit = outcome
                .results
                .iter()
                .find(|result| {
                    result.anchor.kind == "section_label" && result.anchor.value == label
                })
                .unwrap_or_else(|| panic!("section label {label:?} not found for {query:?}"));
            assert_eq!(hit.anchor.line, line, "section label {label:?} line");
            assert_eq!(
                hit.anchor.owner_symbol_id.as_deref(),
                Some(header.id.as_str()),
                "section label {label:?} must be owned by the Header component"
            );
        }
    }

    /// Release gates: "Short-fragment prefix match impossible when either token
    /// is under 3 characters" + "Weak-frequency distractor cannot win". The
    /// broad navigation query must NOT surface the `top_venues` translation
    /// file as a confident structural result — the distractor stays reachable
    /// through the semantic-anchor lane only.
    #[test]
    fn fixture1_broad_navigation_query_does_not_promote_translation_distractor() {
        let (service, temp_dir) = create_test_service();
        write_symbol_track_fixture(temp_dir.path(), "fixture1_tsx_components");
        service.reconcile_index().unwrap();

        // The distractor IS indexed — as semantic-anchor evidence.
        let anchors = service
            .search_semantic_anchors("top_venues", None, 10)
            .unwrap();
        assert!(
            anchors.iter().any(|result| {
                result.anchor.file_path == "locales/pt-BR/venues.json"
                    && result.anchor.value.starts_with("top_venues")
            }),
            "translation key must exist in the semantic-anchor lane"
        );

        // The handoff's regression query: `i18n` fragments (`i`, `n`) must not
        // satisfy `icons`/`navigation`, and repeated weak hits must not promote
        // the translation file structurally. An empty structural result is
        // acceptable here; the distractor winning is not.
        let results = service
            .search_symbols("mobile top navigation icons locale location", 10)
            .unwrap();
        assert!(
            !results.iter().any(|result| {
                result.symbol.file_path == "locales/pt-BR/venues.json"
                    || result.symbol.name.contains("top_venues")
            }),
            "translation distractor must not be a structural match: {:?}",
            results
                .iter()
                .map(|r| (r.symbol.file_path.clone(), r.symbol.name.clone(), r.score))
                .collect::<Vec<_>>()
        );
    }

    // ---- Fixture 2 — Rust constant usage vs. lexical shadowing --------------

    /// Release gate: "Rust constant usage — all unshadowed fixture uses
    /// returned; no shadowed false positives". Exact edge-set assertion:
    /// direct use, call-argument use, closure-body use, and the shadowing
    /// `let`'s INITIALIZER resolve; the parameter shadow, the post-`let`
    /// reference, the `for`-binding body use, and the match-arm binding use
    /// emit nothing.
    #[test]
    fn fixture2_rust_const_usage_edges_respect_lexical_shadowing() {
        let (service, temp_dir) = create_test_service();
        write_symbol_track_fixture(temp_dir.path(), "fixture2_rust_shadowing");
        service.reconcile_index().unwrap();

        let constant = fixture_symbol(&service, "consts.rs", "RETRY_LIMIT");
        let references = service
            .symbol_store
            .find_references_to_symbol_id(&constant.id, SymbolRelationshipType::Usage, 50)
            .unwrap();

        let mut observed: Vec<(String, u32)> = references
            .iter()
            .map(|reference| (reference.source_symbol.name.clone(), reference.line))
            .collect();
        observed.sort();
        assert_eq!(
            observed,
            vec![
                ("call_argument_use".to_string(), 9),
                ("closure_use".to_string(), 13),
                ("direct_use".to_string(), 5),
                ("let_shadow".to_string(), 22),
            ],
            "exactly the four lexically-valid outer-constant uses must resolve"
        );

        for reference in &references {
            assert_eq!(
                reference.resolution_strategy.as_deref(),
                Some("file_local_const"),
                "Track B edges carry the file_local_const strategy"
            );
            let confidence = reference
                .resolution_confidence
                .expect("file_local_const edges carry a confidence");
            assert!(
                (confidence - 0.9).abs() < 1e-4,
                "file_local_const confidence must be 0.9, got {confidence}"
            );
        }

        // Deliberate exclusions, by line: parameter shadow (18), post-let
        // shadowed reference (23), for-binding body use (28), match-arm
        // binding use (34).
        for excluded_line in [18u32, 23, 28, 34] {
            assert!(
                !references.iter().any(|r| r.line == excluded_line),
                "line {excluded_line} is shadowed and must not produce a usage edge"
            );
        }
        for excluded_source in ["parameter_shadow", "loop_shadow", "match_shadow"] {
            assert!(
                !references
                    .iter()
                    .any(|r| r.source_symbol.name == excluded_source),
                "{excluded_source} only touches shadowed bindings and must emit no edge"
            );
        }
    }

    // ---- Fixture 3 — Go implicit interface implementations ------------------

    fn implements_sources(service: &LanguageService, interface: &Symbol) -> Vec<SymbolReference> {
        service
            .symbol_store
            .find_references_to_symbol_id(&interface.id, SymbolRelationshipType::Implements, 50)
            .unwrap()
    }

    /// Release gate: "Go implementation recall — complete fixture method sets
    /// returned, including test fake; exclusions correct". Mined after
    /// `reconcile_index` (the pass runs inside reconcile, after backfill).
    /// Pointer-only satisfaction (`DiskStore`) must still produce the edge —
    /// its `receiver_set: pointer` tag is pinned by the store-level miner
    /// tests, which the service layer does not re-expose.
    #[test]
    fn fixture3_go_implicit_interface_implementation_set_is_complete() {
        let (service, temp_dir) = create_test_service();
        write_symbol_track_fixture(temp_dir.path(), "fixture3_go_interfaces");
        service.reconcile_index().unwrap();

        let base = fixture_symbol(&service, "cache/contract.go", "Base");
        assert_eq!(base.symbol_type, SymbolType::Interface);
        let cache = fixture_symbol(&service, "cache/contract.go", "Cache");
        assert_eq!(cache.symbol_type, SymbolType::Interface);

        let base_refs = implements_sources(&service, &base);
        let mut base_sources: Vec<String> = base_refs
            .iter()
            .map(|r| r.source_symbol.name.clone())
            .collect();
        base_sources.sort();
        assert_eq!(
            base_sources,
            vec!["DiskStore", "FakeCache", "Incomplete", "Memory"],
            "Base is satisfied by every type with Ping — value, pointer-only, \
             incomplete-for-Cache, and the test fake"
        );

        let cache_refs = implements_sources(&service, &cache);
        let mut cache_sources: Vec<String> = cache_refs
            .iter()
            .map(|r| r.source_symbol.name.clone())
            .collect();
        cache_sources.sort();
        assert_eq!(
            cache_sources,
            vec!["DiskStore", "FakeCache", "Memory"],
            "Cache (embedding Base) requires Ping AND Set — Incomplete is excluded"
        );
        assert!(
            !cache_sources.contains(&"Incomplete".to_string()),
            "Incomplete must never implement Cache"
        );

        for reference in base_refs.iter().chain(cache_refs.iter()) {
            assert_eq!(
                reference.resolution_strategy.as_deref(),
                Some("go_implicit_interface"),
                "mined edges carry the go_implicit_interface strategy"
            );
            let confidence = reference
                .resolution_confidence
                .expect("mined edges carry a confidence");
            assert!(
                (confidence - 0.75).abs() < 1e-4,
                "go_implicit_interface confidence must be 0.75, got {confidence}"
            );
        }

        // The test-only fake is retained and comes from the _test.go file.
        let fake = cache_refs
            .iter()
            .find(|r| r.source_symbol.name == "FakeCache")
            .expect("test fake must implement Cache");
        assert_eq!(fake.source_symbol.file_path, "cache/fake_test.go");
    }

    /// Go package visibility: an unexported interface method can only be
    /// satisfied from the interface's own package. The same-package
    /// implementer is the positive control proving the exclusion is
    /// package-based, not signature-based.
    #[test]
    fn fixture3_go_unexported_interface_method_is_package_scoped() {
        let (service, temp_dir) = create_test_service();
        write_symbol_track_fixture(temp_dir.path(), "fixture3_go_interfaces");
        service.reconcile_index().unwrap();

        let refresher = fixture_symbol(&service, "internala/contract.go", "refresher");
        assert_eq!(refresher.symbol_type, SymbolType::Interface);

        let refs = implements_sources(&service, &refresher);
        let mut sources: Vec<String> =
            refs.iter().map(|r| r.source_symbol.name.clone()).collect();
        sources.sort();
        assert_eq!(
            sources,
            vec!["LocalRefresher"],
            "only the same-package type may satisfy the unexported method; \
             RemoteRefresher (package internalb) must be excluded"
        );
    }

    /// Track C regression — a SINGLE-FILE re-index of a Go file replaces that
    /// file's relationships by source_file_path, which used to silently delete
    /// its types' mined `go_implicit_interface` edges until the next FULL
    /// workspace index. The single-file path must re-mine.
    #[test]
    fn fixture3_single_file_go_reindex_preserves_mined_implements_edges() {
        let (service, temp_dir) = create_test_service();
        write_symbol_track_fixture(temp_dir.path(), "fixture3_go_interfaces");
        service.reconcile_index().unwrap();

        let cache = fixture_symbol(&service, "cache/contract.go", "Cache");
        assert!(
            implements_sources(&service, &cache)
                .iter()
                .any(|reference| reference.source_symbol.name == "Memory"),
            "full workspace index must mine Memory implements Cache"
        );

        // Touch the implementer's file so the single-file path actually
        // replaces its rows (a fresh hash short-circuits before deleting).
        let memory_path = temp_dir.path().join("cache/memory.go");
        let mut content = fs::read_to_string(&memory_path).unwrap();
        content.push_str("\n// touched to force a real single-file re-index\n");
        fs::write(&memory_path, content).unwrap();
        service.index_file("cache/memory.go").unwrap();

        assert!(
            implements_sources(&service, &cache)
                .iter()
                .any(|reference| reference.source_symbol.name == "Memory"),
            "single-file re-index of cache/memory.go must re-mine the \
             Memory implements Cache edge, not drop it until the next full index"
        );
    }

    // ---- Fixture 4 — relationship-kind-aware resolution collisions ----------

    /// Track D release gate: a `call` resolves to the callable despite a
    /// same-named Markdown heading (function AND method cases), and headings
    /// are never reference targets for any relationship kind.
    #[test]
    fn fixture4_call_resolution_is_kind_aware_despite_heading_collisions() {
        let (service, temp_dir) = create_test_service();
        write_symbol_track_fixture(temp_dir.path(), "fixture4_resolution_collisions");
        service.reconcile_index().unwrap();

        // Function vs. heading: the cross-file, non-imported call resolves via
        // the kind-aware global-unique backfill — the heading neither wins nor
        // makes the callable ambiguous.
        let archive_fn = fixture_symbol(&service, "src/tasks.py", "archive_stale_sessions");
        let archive_refs = service
            .symbol_store
            .find_references_to_symbol_id(&archive_fn.id, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(
            archive_refs.len(),
            1,
            "exactly one call site resolves to the function"
        );
        assert_eq!(archive_refs[0].source_symbol.name, "run_maintenance");
        assert_eq!(archive_refs[0].source_symbol.file_path, "src/caller.py");
        assert_eq!(archive_refs[0].line, 4);
        assert_eq!(
            archive_refs[0].resolution_strategy.as_deref(),
            Some("global_unique"),
            "the kind-aware backfill resolves the call"
        );

        // Method vs. heading.
        let rotate_method = fixture_symbol(&service, "src/worker.py", "rotate_billing_keys");
        let rotate_refs = service
            .symbol_store
            .find_references_to_symbol_id(&rotate_method.id, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(rotate_refs.len(), 1);
        assert_eq!(rotate_refs[0].source_symbol.name, "run_maintenance");
        assert_eq!(rotate_refs[0].line, 5);
        assert_eq!(
            rotate_refs[0].resolution_strategy.as_deref(),
            Some("global_unique")
        );

        // Headings are never targets, for ANY relationship kind.
        for heading_name in ["archive_stale_sessions", "rotate_billing_keys"] {
            let heading = service
                .get_file_symbols_raw("docs/notes.md")
                .unwrap()
                .into_iter()
                .find(|s| s.name == heading_name && s.symbol_type == SymbolType::Heading)
                .unwrap_or_else(|| panic!("heading {heading_name:?} not indexed"));
            for kind in [
                SymbolRelationshipType::Call,
                SymbolRelationshipType::Usage,
                SymbolRelationshipType::Implements,
                SymbolRelationshipType::Extends,
                SymbolRelationshipType::UsesType,
                SymbolRelationshipType::Handles,
                SymbolRelationshipType::Contains,
            ] {
                let refs = service
                    .symbol_store
                    .find_references_to_symbol_id(&heading.id, kind, 10)
                    .unwrap();
                assert!(
                    refs.is_empty(),
                    "heading {heading_name:?} must never be a {kind:?} target"
                );
            }
        }
    }

    /// Track D: `usage` resolves only to a globally UNIQUE value-kind
    /// candidate. Two same-named constants leave the edge unresolved; import
    /// placeholders and synthetic `__file__` roots are excluded from every
    /// candidate set.
    #[test]
    fn fixture4_usage_resolution_requires_unique_value_candidate() {
        let (service, temp_dir) = create_test_service();
        write_symbol_track_fixture(temp_dir.path(), "fixture4_resolution_collisions");
        service.reconcile_index().unwrap();

        let probe = fixture_symbol(&service, "src/probe.py", "read_retention");
        // Store-layer probe edges (the extractors do not emit cross-language
        // usage edges): one ambiguous value target, one unique value target,
        // one call colliding only with an import placeholder, one call
        // colliding only with a synthetic file-root name.
        let probe_edges = vec![
            SymbolRelationship {
                source_symbol_id: probe.id.clone(),
                source_file_path: "src/probe.py".to_string(),
                target_name: "RETENTION_WINDOW".to_string(),
                target_symbol_id: None,
                relationship_type: SymbolRelationshipType::Usage,
                line: 0,
                ..Default::default()
            },
            SymbolRelationship {
                source_symbol_id: probe.id.clone(),
                source_file_path: "src/probe.py".to_string(),
                target_name: "IDLE_TIMEOUT".to_string(),
                target_symbol_id: None,
                relationship_type: SymbolRelationshipType::Usage,
                line: 1,
                ..Default::default()
            },
            SymbolRelationship {
                source_symbol_id: probe.id.clone(),
                source_file_path: "src/probe.py".to_string(),
                target_name: "widget_kit".to_string(),
                target_symbol_id: None,
                relationship_type: SymbolRelationshipType::Call,
                line: 0,
                ..Default::default()
            },
            SymbolRelationship {
                source_symbol_id: probe.id.clone(),
                source_file_path: "src/probe.py".to_string(),
                target_name: "tasks.py".to_string(),
                target_symbol_id: None,
                relationship_type: SymbolRelationshipType::Call,
                line: 1,
                ..Default::default()
            },
        ];
        service
            .symbol_store
            .replace_relationships_for_file("src/probe.py", &probe_edges)
            .unwrap();
        service
            .symbol_store
            .backfill_unresolved_relationship_targets()
            .unwrap();

        // Unique value candidate → resolved with the global-unique strategy.
        let idle = fixture_symbol(&service, "src/consts_a.rs", "IDLE_TIMEOUT");
        let idle_refs = service
            .symbol_store
            .find_references_to_symbol_id(&idle.id, SymbolRelationshipType::Usage, 10)
            .unwrap();
        assert_eq!(idle_refs.len(), 1, "unique value target must resolve");
        assert_eq!(idle_refs[0].source_symbol.name, "read_retention");
        assert_eq!(
            idle_refs[0].resolution_strategy.as_deref(),
            Some("global_unique")
        );

        // Two valid value candidates → the edge MUST stay unresolved.
        let retention_rows = service
            .symbol_store
            .find_references_to_target("RETENTION_WINDOW", SymbolRelationshipType::Usage, 10)
            .unwrap();
        assert_eq!(retention_rows.len(), 1);
        assert!(
            retention_rows[0].target_symbol_id.is_none(),
            "two same-named constants must leave the usage edge unresolved"
        );

        // Import placeholders are candidates for NO relationship kind.
        assert!(
            service
                .get_file_symbols_raw("src/caller.py")
                .unwrap()
                .iter()
                .any(|s| s.name == "widget_kit" && s.symbol_type == SymbolType::Import),
            "the import placeholder must exist for the exclusion to be meaningful"
        );
        let widget_rows = service
            .symbol_store
            .find_references_to_target("widget_kit", SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(widget_rows.len(), 1);
        assert!(
            widget_rows[0].target_symbol_id.is_none(),
            "an import placeholder must never resolve a call"
        );

        // Synthetic `__file__` roots are excluded from every candidate set.
        assert!(
            service
                .get_file_symbols_raw("src/tasks.py")
                .unwrap()
                .iter()
                .any(|s| s.name == "tasks.py" && s.qualified_name == "__file__"),
            "the synthetic file root must exist for the exclusion to be meaningful"
        );
        let root_rows = service
            .symbol_store
            .find_references_to_target("tasks.py", SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(root_rows.len(), 1);
        assert!(
            root_rows[0].target_symbol_id.is_none(),
            "a synthetic file root must never resolve a call"
        );
    }

    // ---- Fixture 6 — extractor-version invalidation -------------------------

    /// The reconcile fast-path fingerprint must incorporate every file's
    /// CURRENT extractor version (handoff: "Extractor version must participate
    /// in every shortcut fingerprint"). This pins the canonical projection —
    /// if the format changes, this test must change WITH it, and the version
    /// component must survive the change.
    #[test]
    fn fixture6_reconcile_fingerprint_includes_extractor_versions() {
        let (service, temp_dir) = create_test_service();
        fs::write(temp_dir.path().join("a.ts"), "export function alphaOne() {}\n").unwrap();
        fs::write(temp_dir.path().join("b.py"), "def beta_one():\n    return 1\n").unwrap();

        let files = service.supported_language_files(".");
        let fingerprint = service
            .compute_reconcile_fingerprint(&files)
            .expect("fingerprint computable");

        let ts_meta = file_index_metadata(&temp_dir.path().join("a.ts")).unwrap();
        let py_meta = file_index_metadata(&temp_dir.path().join("b.py")).unwrap();
        let ts_version = LanguageService::extractor_version_for_index_file("a.ts").unwrap();
        let py_version = LanguageService::extractor_version_for_index_file("b.py").unwrap();

        let canonical_current = format!(
            "store_schema\u{1f}{}\u{1f}rust_qualified_resolver\u{1f}{}\n2\na.ts\u{1f}{}\u{1f}{}\u{1f}{}\nb.py\u{1f}{}\u{1f}{}\u{1f}{}",
            SYMBOL_STORE_SCHEMA_VERSION,
            rust_project::RUST_QUALIFIED_RESOLVER_VERSION,
            ts_meta.file_size,
            ts_meta.modified_at,
            ts_version,
            py_meta.file_size,
            py_meta.modified_at,
            py_version
        );
        assert_eq!(
            fingerprint,
            compute_hash(&canonical_current),
            "fingerprint must be the hash of (path, size, mtime, extractor_version) entries"
        );

        // The SAME worktree under an older TypeScript extractor produces a
        // DIFFERENT fingerprint — an extractor upgrade invalidates the
        // no-change fast path even with byte-identical files.
        let canonical_stale = format!(
            "store_schema\u{1f}{}\u{1f}rust_qualified_resolver\u{1f}{}\n2\na.ts\u{1f}{}\u{1f}{}\u{1f}0\nb.py\u{1f}{}\u{1f}{}\u{1f}{}",
            SYMBOL_STORE_SCHEMA_VERSION,
            rust_project::RUST_QUALIFIED_RESOLVER_VERSION,
            ts_meta.file_size,
            ts_meta.modified_at,
            py_meta.file_size,
            py_meta.modified_at,
            py_version
        );
        assert_ne!(
            fingerprint,
            compute_hash(&canonical_stale),
            "changing only an extractor version must change the fingerprint"
        );
    }

    /// End-to-end extractor upgrade: an index built by an older TypeScript
    /// extractor (stale per-file rows AND a checkpoint fingerprinted by the
    /// old code) must skip the no-change fast path, structurally re-extract
    /// ONLY the affected language's files, leave the other language's rows
    /// byte-for-byte untouched, and finish Fresh with zero failures.
    #[test]
    fn fixture6_stale_extractor_version_reextracts_only_affected_language() {
        let (service, temp_dir) = create_test_service();
        fs::write(temp_dir.path().join("a.ts"), "export function alphaOne() {}\n").unwrap();
        fs::write(temp_dir.path().join("b.py"), "def beta_one():\n    return 1\n").unwrap();

        let first = service.reconcile_index().unwrap();
        assert!(!first.fast_path);
        assert_eq!(first.health.status, IndexHealthStatus::Fresh);

        // Control: the unchanged worktree takes the fast path.
        let second = service.reconcile_index().unwrap();
        assert!(second.fast_path, "control: unchanged worktree is fast-pathed");

        // Simulate "indexed by extractor version 0, reopened with the current
        // version": the per-file row AND the persisted checkpoint fingerprint
        // both carry the OLD version, exactly as an old binary left them.
        let ts_record = service
            .symbol_store
            .indexed_file_record("a.ts")
            .unwrap()
            .unwrap();
        service
            .symbol_store
            .mark_file_indexed_with_metadata_and_extractor_version(
                "a.ts",
                &ts_record.file_hash,
                ts_record.symbol_count,
                ts_record.file_size,
                ts_record.line_count,
                ts_record.modified_at,
                Some(0),
            )
            .unwrap();
        let py_before = service
            .symbol_store
            .indexed_file_record("b.py")
            .unwrap()
            .unwrap();

        let ts_meta = file_index_metadata(&temp_dir.path().join("a.ts")).unwrap();
        let py_meta = file_index_metadata(&temp_dir.path().join("b.py")).unwrap();
        let py_version = LanguageService::extractor_version_for_index_file("b.py").unwrap();
        let stale_fingerprint = compute_hash(&format!(
            "2\na.ts\u{1f}{}\u{1f}{}\u{1f}0\nb.py\u{1f}{}\u{1f}{}\u{1f}{}",
            ts_meta.file_size, ts_meta.modified_at, py_meta.file_size, py_meta.modified_at, py_version
        ));
        let raw_checkpoint = service
            .symbol_store
            .get_index_meta(RECONCILE_CHECKPOINT_KEY)
            .unwrap()
            .expect("checkpoint stored by the healthy reconcile");
        let mut checkpoint: serde_json::Value = serde_json::from_str(&raw_checkpoint).unwrap();
        checkpoint["fingerprint"] = serde_json::Value::String(stale_fingerprint);
        service
            .symbol_store
            .set_index_meta(RECONCILE_CHECKPOINT_KEY, &checkpoint.to_string())
            .unwrap();

        // The "upgraded" reconcile: no fast path, exactly one file re-indexed.
        let third = service.reconcile_index().unwrap();
        assert!(
            !third.fast_path,
            "a stale extractor version must invalidate the no-change fast path"
        );
        assert_eq!(
            third.files_indexed, 1,
            "only the affected language's file is re-extracted"
        );
        assert_eq!(third.files_removed, 0);
        assert_eq!(third.health.status, IndexHealthStatus::Fresh);
        assert_eq!(third.health.stale_files, 0, "zero failures reported");
        assert_eq!(third.health.missing_files, 0, "zero failures reported");
        assert_eq!(third.health.queued_files, 0);

        // The TS row was STRUCTURALLY re-extracted (not metadata-refreshed):
        // its extractor version is current again and its symbols survive.
        let ts_after = service
            .symbol_store
            .indexed_file_record("a.ts")
            .unwrap()
            .unwrap();
        assert_eq!(
            ts_after.extractor_version,
            LanguageService::extractor_version_for_index_file("a.ts")
        );
        assert!(service
            .get_file_symbols_raw("a.ts")
            .unwrap()
            .iter()
            .any(|s| s.name == "alphaOne"));

        // The Python row was not needlessly rebuilt.
        let py_after = service
            .symbol_store
            .indexed_file_record("b.py")
            .unwrap()
            .unwrap();
        assert_eq!(py_after.extractor_version, py_before.extractor_version);
        assert_eq!(
            py_after.indexed_at, py_before.indexed_at,
            "the unaffected language's row must be untouched"
        );
    }

    /// A stale extractor version must defeat the metadata-only refresh
    /// shortcut: unchanged bytes with drifted size/mtime normally get a
    /// cheap metadata rewrite, but a version mismatch must force structural
    /// re-extraction instead.
    #[test]
    fn fixture6_stale_extractor_version_defeats_metadata_only_refresh() {
        let (service, temp_dir) = create_test_service();
        fs::write(temp_dir.path().join("v.ts"), "export function gammaOne() {}\n").unwrap();
        service.index_file("v.ts").unwrap();

        let record = service
            .symbol_store
            .indexed_file_record("v.ts")
            .unwrap()
            .unwrap();
        let real_mtime = file_index_metadata(&temp_dir.path().join("v.ts"))
            .unwrap()
            .modified_at;
        let drifted_mtime = Some(real_mtime - 10);

        // Control: current version + drifted metadata + unchanged bytes →
        // the metadata-only shortcut refreshes the row and skips re-indexing.
        service
            .symbol_store
            .mark_file_indexed_with_metadata_and_extractor_version(
                "v.ts",
                &record.file_hash,
                record.symbol_count,
                record.file_size,
                record.line_count,
                drifted_mtime,
                record.extractor_version,
            )
            .unwrap();
        let control = service
            .symbol_store
            .indexed_file_record("v.ts")
            .unwrap()
            .unwrap();
        assert!(
            !service
                .indexed_file_needs_refresh("v.ts", &control, true)
                .unwrap(),
            "control: unchanged bytes with a current version take the metadata shortcut"
        );
        let refreshed = service
            .symbol_store
            .indexed_file_record("v.ts")
            .unwrap()
            .unwrap();
        assert_eq!(
            refreshed.modified_at,
            Some(real_mtime),
            "control: the metadata-only path rewrote the drifted mtime"
        );

        // Stale version + drifted metadata + unchanged bytes → the file MUST
        // be queued for structural re-extraction, and the row must NOT be
        // silently metadata-refreshed to the current version.
        service
            .symbol_store
            .mark_file_indexed_with_metadata_and_extractor_version(
                "v.ts",
                &record.file_hash,
                record.symbol_count,
                record.file_size,
                record.line_count,
                drifted_mtime,
                Some(0),
            )
            .unwrap();
        let stale = service
            .symbol_store
            .indexed_file_record("v.ts")
            .unwrap()
            .unwrap();
        assert!(
            service
                .indexed_file_needs_refresh("v.ts", &stale, true)
                .unwrap(),
            "a stale extractor version must force re-extraction even for unchanged bytes"
        );
        let untouched = service
            .symbol_store
            .indexed_file_record("v.ts")
            .unwrap()
            .unwrap();
        assert_eq!(
            untouched.extractor_version,
            Some(0),
            "needs_refresh must not metadata-refresh a stale-versioned row"
        );

        // The actual re-index restores the current extractor version.
        service.index_file("v.ts").unwrap();
        let reindexed = service
            .symbol_store
            .indexed_file_record("v.ts")
            .unwrap()
            .unwrap();
        assert_eq!(
            reindexed.extractor_version,
            LanguageService::extractor_version_for_index_file("v.ts")
        );
    }
}
