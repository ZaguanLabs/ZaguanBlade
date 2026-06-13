use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use crate::blade_protocol::{
    ContextFileEnrichment, ContextFileResult, ContextMemoryItem, ContextPackConfidence,
    ContextPackError, ContextPackPayload, ContextProjectInfo, ContextRange, ContextRelatedFile,
    ContextSemanticAnchorSummary, ContextSymbolSummary, ContextWorkspace,
};
use crate::language_service::LanguageService;
use crate::local_artifacts::LocalArtifactStore;
use crate::project_settings::get_zblade_dir;
use crate::symbol_index::SymbolStore;
use crate::tree_sitter::{Symbol, SymbolRelationshipType, SymbolType};

#[derive(Debug, Clone)]
pub struct ContextPackRequest {
    pub id: String,
    pub query: String,
    pub queries: Vec<String>,
    pub intent: Option<String>,
    pub max_results: Option<usize>,
    pub include_tests: Option<bool>,
    pub include_docs: Option<bool>,
    pub include_memory: Option<bool>,
    pub include_project_index_min: Option<bool>,
}

pub fn build_context_pack(
    workspace_root: &Path,
    active_file: Option<&str>,
    open_files: &[String],
    request: &ContextPackRequest,
) -> ContextPackPayload {
    let started_at = Instant::now();
    let queries = normalized_queries(&request.query, &request.queries);
    if queries.is_empty() {
        return error_payload("unsupported", "fast_context requires a non-empty query");
    }

    if !workspace_root.exists() || !workspace_root.is_dir() {
        return error_payload(
            "workspace_unavailable",
            "No usable workspace root is available",
        );
    }

    let max_results = request.max_results.unwrap_or(8).clamp(1, 20);
    let include_tests = request.include_tests.unwrap_or(true);
    let include_docs = request.include_docs.unwrap_or(true);
    let include_memory = request.include_memory.unwrap_or(true);
    let include_project_index_min = request.include_project_index_min.unwrap_or(false);
    let normalized_active_file =
        active_file.and_then(|path| normalize_workspace_path(workspace_root, path));
    let normalized_open_files = normalize_workspace_paths(workspace_root, open_files);
    let workspace = build_workspace_payload(
        workspace_root,
        normalized_active_file.clone(),
        normalized_open_files.clone(),
    );
    let project_context = build_project_context(workspace_root, include_project_index_min);

    let service = match language_service_for_workspace(workspace_root) {
        Ok(service) => service,
        Err(error) => {
            let mut payload = error_payload("index_unavailable", &error);
            payload.queries_used = queries;
            payload.workspace = Some(workspace);
            payload.project_context = Some(project_context);
            payload.timing_ms = Some(started_at.elapsed().as_millis() as u64);
            return payload;
        }
    };

    let mut primary = collect_primary_files_for_queries(
        &service,
        &queries,
        request.intent.as_deref(),
        max_results,
        normalized_active_file.as_deref(),
        &normalized_open_files,
    );

    if primary.is_empty() {
        primary = collect_fallback_path_matches_for_queries(workspace_root, &queries, max_results);
    }

    let related_tests = if include_tests {
        collect_related_tests(workspace_root, &primary, max_results.min(6))
    } else {
        Vec::new()
    };

    let combined_query = queries.join(" ");
    let related_docs = if include_docs {
        collect_related_docs(workspace_root, &combined_query, max_results.min(6))
    } else {
        Vec::new()
    };

    let memories = if include_memory {
        collect_memories(workspace_root, &combined_query, max_results.min(5))
    } else {
        Vec::new()
    };

    let index_health = service.audit_index_health().ok();
    let enriched_files = enrich_primary_files(&service, &primary, max_results);
    let related_files = collect_related_files_from_enrichments(&enriched_files, max_results);

    let confidence = context_pack_confidence(&primary, &enriched_files, index_health.as_ref());

    let summary = if let Some(first) = primary.first() {
        format!(
            "Found {} likely relevant source file{} for the request; start with {}.",
            primary.len(),
            if primary.len() == 1 { "" } else { "s" },
            first.path
        )
    } else if !related_docs.is_empty() || !memories.is_empty() {
        "No strong source-file match was found, but related docs or local memories may help orient the investigation.".to_string()
    } else {
        "No strong context-pack matches were found in the local index.".to_string()
    };

    let recommended_next_step = if let Some(first) = primary.first() {
        if let Some(second) = primary.get(1) {
            format!(
                "Read {} and {} first, then inspect the related tests if behavior is unclear.",
                first.path, second.path
            )
        } else {
            format!(
                "Read {} first, then broaden with symbol or grep search if needed.",
                first.path
            )
        }
    } else if let Some(doc) = related_docs.first() {
        format!(
            "Read {} first, then use normal code search to find the implementation path.",
            doc.path
        )
    } else {
        "Fall back to normal tool-based workspace exploration.".to_string()
    };

    let hypothesized_flow = build_hypothesized_flow(&primary);

    ContextPackPayload {
        queries_used: queries,
        workspace: Some(workspace),
        project_context: Some(project_context),
        summary,
        confidence,
        primary_files: primary,
        related_tests,
        related_docs,
        memories,
        hypothesized_flow,
        enriched_files,
        related_files,
        index_health,
        recommended_next_step,
        error: None,
        timing_ms: Some(started_at.elapsed().as_millis() as u64),
    }
}

pub fn error_payload(code: &str, message: &str) -> ContextPackPayload {
    ContextPackPayload {
        queries_used: Vec::new(),
        workspace: None,
        project_context: None,
        summary: String::new(),
        confidence: ContextPackConfidence::Low,
        primary_files: Vec::new(),
        related_tests: Vec::new(),
        related_docs: Vec::new(),
        memories: Vec::new(),
        hypothesized_flow: Vec::new(),
        enriched_files: Vec::new(),
        related_files: Vec::new(),
        index_health: None,
        recommended_next_step: String::new(),
        error: Some(ContextPackError {
            code: code.to_string(),
            message: message.to_string(),
        }),
        timing_ms: None,
    }
}

fn language_service_for_workspace(workspace_root: &Path) -> Result<LanguageService, String> {
    let db_path = get_zblade_dir(workspace_root)
        .join("index")
        .join("symbols.db");
    let symbol_store = std::sync::Arc::new(
        SymbolStore::new(&db_path)
            .map_err(|error| format!("Failed to open symbol index: {}", error))?,
    );
    LanguageService::new(workspace_root.to_path_buf(), symbol_store)
        .map_err(|error| format!("Failed to initialize language service: {}", error))
}

fn normalized_queries(query: &str, queries: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();

    for value in std::iter::once(query).chain(queries.iter().map(String::as_str)) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            normalized.push(trimmed.to_string());
        }
        if normalized.len() >= 20 {
            break;
        }
    }

    normalized
}

fn normalize_workspace_path(workspace_root: &Path, path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        path.strip_prefix(workspace_root)
            .ok()
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .or_else(|| Some(trimmed.replace('\\', "/")))
    } else {
        Some(trimmed.trim_start_matches("./").replace('\\', "/"))
    }
}

fn normalize_workspace_paths(workspace_root: &Path, paths: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();

    for path in paths {
        let Some(relative) = normalize_workspace_path(workspace_root, path) else {
            continue;
        };
        if seen.insert(relative.clone()) {
            normalized.push(relative);
        }
    }

    normalized
}

fn build_workspace_payload(
    workspace_root: &Path,
    active_file: Option<String>,
    open_files: Vec<String>,
) -> ContextWorkspace {
    ContextWorkspace {
        root: workspace_root.to_string_lossy().to_string(),
        active_file,
        open_files,
    }
}

fn build_project_context(
    workspace_root: &Path,
    include_project_index_min: bool,
) -> ContextProjectInfo {
    let context_dir = get_zblade_dir(workspace_root).join("context");
    let project_index_min_path = context_dir.join("project_index_min.md");
    let project_index_path = context_dir.join("project_index.md");
    let project_index_min_available = project_index_min_path.is_file();
    let project_index_available = project_index_path.is_file();
    let project_index_min = if include_project_index_min && project_index_min_available {
        std::fs::read_to_string(&project_index_min_path)
            .ok()
            .map(|content| truncate_chars(content.trim(), 16_000))
            .filter(|content| !content.is_empty())
    } else {
        None
    };
    let project_index_min_included = project_index_min.is_some();
    let project_index_min_reason = project_index_min_reason(
        include_project_index_min,
        project_index_min_available,
        project_index_min_included,
    );

    ContextProjectInfo {
        project_index_min,
        project_index_min_available,
        project_index_available,
        project_index_path: project_index_available
            .then(|| ".zblade/context/project_index.md".to_string()),
        project_index_min_requested: include_project_index_min,
        project_index_min_included,
        project_index_min_reason: Some(project_index_min_reason.to_string()),
        context_source: Some("legacy_project_index_metadata".to_string()),
    }
}

fn project_index_min_reason(
    requested: bool,
    available: bool,
    included: bool,
) -> &'static str {
    if included {
        "included"
    } else if !requested {
        "not_requested"
    } else if !available {
        "missing"
    } else {
        "empty_or_unreadable"
    }
}

fn collect_primary_files_for_queries(
    service: &LanguageService,
    queries: &[String],
    intent: Option<&str>,
    max_results: usize,
    active_file: Option<&str>,
    open_files: &[String],
) -> Vec<ContextFileResult> {
    let mut by_path: HashMap<String, ContextFileResult> = HashMap::new();
    let search_limit = max_results.saturating_mul(6).max(20);

    for query in queries {
        let Ok(results) =
            service.search_symbols_contextual(query, search_limit, active_file, open_files)
        else {
            continue;
        };

        for result in results {
            let path = result.symbol.file_path.clone();
            if should_skip_path(&path) || is_test_path(&path) || is_doc_path(&path) {
                continue;
            }
            let score = score_symbol_result(
                result.score,
                &result.symbol,
                intent,
                active_file,
                open_files,
            );
            let reason = symbol_reason(&result.symbol, intent);
            let suggested_ranges = vec![range_for_symbol(&result.symbol)];

            by_path
                .entry(path.clone())
                .and_modify(|existing| {
                    existing.score = existing.score.saturating_add(4).min(100);
                    if score > existing.score {
                        existing.score = score;
                        existing.reason = reason.clone();
                        existing.suggested_ranges = suggested_ranges.clone();
                    }
                })
                .or_insert(ContextFileResult {
                    path,
                    score,
                    reason,
                    suggested_ranges,
                });
        }
    }

    let mut items: Vec<ContextFileResult> = by_path.into_values().collect();
    items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    items.truncate(max_results);
    items
}

fn collect_fallback_path_matches_for_queries(
    workspace_root: &Path,
    queries: &[String],
    max_results: usize,
) -> Vec<ContextFileResult> {
    let tokens = queries
        .iter()
        .flat_map(|query| query_tokens(query))
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut items = Vec::new();
    for entry in walkdir::WalkDir::new(workspace_root)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = relative_path(workspace_root, path);
        if should_skip_path(&rel) || is_test_path(&rel) || is_doc_path(&rel) {
            continue;
        }
        let lower = rel.to_ascii_lowercase();
        let matches = tokens.iter().filter(|token| lower.contains(*token)).count();
        if matches == 0 {
            continue;
        }
        items.push(ContextFileResult {
            path: rel,
            score: (45 + matches as u32 * 8).min(70),
            reason: "File path matches terms from the request.".to_string(),
            suggested_ranges: vec![ContextRange {
                start_line: 1,
                end_line: 160,
            }],
        });
    }

    items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    items.truncate(max_results);
    items
}

fn enrich_primary_files(
    service: &LanguageService,
    primary: &[ContextFileResult],
    max_results: usize,
) -> Vec<ContextFileEnrichment> {
    primary
        .iter()
        .take(max_results)
        .filter_map(|item| enrich_context_file(service, item).ok())
        .collect()
}

fn enrich_context_file(
    service: &LanguageService,
    item: &ContextFileResult,
) -> Result<ContextFileEnrichment, String> {
    let symbols = service
        .get_file_symbols(&item.path)
        .map_err(|error| error.to_string())?;
    let symbol_summaries = symbols
        .iter()
        .filter(|symbol| is_context_key_symbol(symbol))
        .take(12)
        .map(|symbol| ContextSymbolSummary {
            name: symbol.name.clone(),
            qualified_name: symbol.qualified_name.clone(),
            kind: symbol.symbol_type.to_string(),
            line: symbol.range.start.line.saturating_add(1),
        })
        .collect::<Vec<_>>();

    let semantic_anchors = collect_context_semantic_anchors(service, &item.path, &symbols)?;
    let related_files = collect_context_related_files(service, &item.path, &symbols)?;
    let suggested_ranges = merge_context_ranges(
        item.suggested_ranges
            .iter()
            .copied()
            .chain(
                symbols
                    .iter()
                    .filter(|symbol| is_context_key_symbol(symbol))
                    .take(4)
                    .map(range_for_symbol),
            )
            .chain(semantic_anchors.iter().take(3).map(|anchor| ContextRange {
                start_line: anchor.line.saturating_add(1).saturating_sub(8).max(1),
                end_line: anchor.line.saturating_add(1).saturating_add(24),
            })),
    );
    let confidence = if item.score >= 80 && !symbol_summaries.is_empty() {
        "high"
    } else if item.score >= 55 || !symbol_summaries.is_empty() || !semantic_anchors.is_empty() {
        "medium"
    } else {
        "low"
    }
    .to_string();
    let next_step = if let Some(range) = suggested_ranges.first() {
        format!(
            "Read {} lines {}-{} first, then follow related_files if the edit surface is unclear.",
            item.path, range.start_line, range.end_line
        )
    } else {
        format!(
            "Read {} first, then broaden with symbol_references if needed.",
            item.path
        )
    };

    Ok(ContextFileEnrichment {
        path: item.path.clone(),
        symbol_summaries,
        semantic_anchors,
        related_files,
        suggested_ranges,
        confidence,
        next_step,
    })
}

fn collect_context_semantic_anchors(
    service: &LanguageService,
    file_path: &str,
    symbols: &[Symbol],
) -> Result<Vec<ContextSemanticAnchorSummary>, String> {
    let mut anchors = Vec::new();
    let mut seen = HashSet::new();

    for anchor in service
        .get_file_semantic_anchors(file_path, 8)
        .map_err(|error| error.to_string())?
    {
        let key = (anchor.kind.clone(), anchor.value.clone(), anchor.line);
        if seen.insert(key) {
            anchors.push(ContextSemanticAnchorSummary {
                kind: anchor.kind,
                value: anchor.value,
                line: anchor.line.saturating_add(1),
                preview: truncate_chars(&anchor.preview, 220),
                confidence: anchor.confidence,
            });
        }
    }

    for symbol in symbols
        .iter()
        .filter(|symbol| is_context_key_symbol(symbol))
        .take(8)
    {
        for result in service
            .search_semantic_anchors(&symbol.name, Some(file_path), 4)
            .map_err(|error| error.to_string())?
        {
            let key = (
                result.anchor.kind.clone(),
                result.anchor.value.clone(),
                result.anchor.line,
            );
            if seen.insert(key) {
                anchors.push(ContextSemanticAnchorSummary {
                    kind: result.anchor.kind,
                    value: result.anchor.value,
                    line: result.anchor.line.saturating_add(1),
                    preview: truncate_chars(&result.anchor.preview, 220),
                    confidence: result.anchor.confidence,
                });
            }
            if anchors.len() >= 8 {
                return Ok(anchors);
            }
        }
    }

    anchors.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.line.cmp(&b.line))
    });
    anchors.truncate(8);
    Ok(anchors)
}

fn collect_context_related_files(
    service: &LanguageService,
    file_path: &str,
    symbols: &[Symbol],
) -> Result<Vec<ContextRelatedFile>, String> {
    let mut related = Vec::new();
    let mut seen = HashSet::new();

    for imported in service
        .get_file_relationship_targets(file_path, SymbolRelationshipType::Import, 8)
        .map_err(|error| error.to_string())?
    {
        if imported != file_path && seen.insert(imported.clone()) {
            related.push(ContextRelatedFile {
                path: imported.clone(),
                relationship: "import".to_string(),
                reason: format!("Imported by {}.", file_path),
                score: 82,
            });
        }
    }

    for symbol in symbols
        .iter()
        .filter(|symbol| is_context_key_symbol(symbol))
        .take(6)
    {
        for reference in service
            .find_references_to_symbol(symbol, 6)
            .map_err(|error| error.to_string())?
        {
            let related_path = reference.source_symbol.file_path;
            if related_path == file_path || !seen.insert(related_path.clone()) {
                continue;
            }
            related.push(ContextRelatedFile {
                path: related_path.clone(),
                relationship: reference.relationship_type.to_string(),
                reason: format!(
                    "{} references {}.",
                    related_path,
                    if symbol.qualified_name.is_empty() {
                        symbol.name.as_str()
                    } else {
                        symbol.qualified_name.as_str()
                    }
                ),
                score: if reference.target_symbol_id.is_some() {
                    78
                } else {
                    62
                },
            });
            if related.len() >= 12 {
                return Ok(related);
            }
        }

        for related_symbol in service
            .get_related_symbols(symbol, 8)
            .map_err(|error| error.to_string())?
        {
            if !matches!(
                related_symbol.relationship.as_str(),
                "module_importer" | "sibling_export_consumer" | "same_module_export"
            ) {
                continue;
            }
            let related_path = related_symbol.symbol.file_path;
            if related_path == file_path || !seen.insert(related_path.clone()) {
                continue;
            }
            related.push(ContextRelatedFile {
                path: related_path.clone(),
                relationship: related_symbol.relationship,
                reason: related_symbol.reason,
                score: related_symbol.score.min(72),
            });
            if related.len() >= 12 {
                return Ok(related);
            }
        }
    }

    Ok(related)
}

fn collect_related_files_from_enrichments(
    enriched_files: &[ContextFileEnrichment],
    limit: usize,
) -> Vec<ContextRelatedFile> {
    let mut seen = HashSet::new();
    let mut related = enriched_files
        .iter()
        .flat_map(|file| file.related_files.iter().cloned())
        .filter(|file| seen.insert(file.path.clone()))
        .collect::<Vec<_>>();
    related.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    related.truncate(limit.saturating_mul(2).max(limit));
    related
}

fn context_pack_confidence(
    primary: &[ContextFileResult],
    enriched_files: &[ContextFileEnrichment],
    index_health: Option<&crate::language_service::IndexHealthSnapshot>,
) -> ContextPackConfidence {
    let fresh_or_unknown = index_health
        .map(|health| {
            matches!(
                health.status,
                crate::language_service::IndexHealthStatus::Fresh
            )
        })
        .unwrap_or(true);
    let enriched_count = enriched_files
        .iter()
        .filter(|file| file.confidence == "high" || file.confidence == "medium")
        .count();

    if fresh_or_unknown
        && primary.len() >= 3
        && primary.first().is_some_and(|item| item.score >= 75)
        && enriched_count >= 2
    {
        ContextPackConfidence::High
    } else if !primary.is_empty() || enriched_count > 0 {
        ContextPackConfidence::Medium
    } else {
        ContextPackConfidence::Low
    }
}

fn is_context_key_symbol(symbol: &Symbol) -> bool {
    !symbol.name.is_empty()
        && !matches!(symbol.qualified_name.as_str(), "__file__")
        && matches!(
            symbol.symbol_type,
            SymbolType::Function
                | SymbolType::Method
                | SymbolType::Class
                | SymbolType::Struct
                | SymbolType::Interface
                | SymbolType::Type
                | SymbolType::Enum
                | SymbolType::Trait
                | SymbolType::Module
        )
}

fn merge_context_ranges<I>(ranges: I) -> Vec<ContextRange>
where
    I: IntoIterator<Item = ContextRange>,
{
    let mut merged = Vec::<ContextRange>::new();
    for range in ranges {
        if range.end_line < range.start_line {
            continue;
        }
        if merged.iter().any(|existing| {
            existing.start_line <= range.end_line && range.start_line <= existing.end_line
        }) {
            continue;
        }
        merged.push(range);
        if merged.len() >= 6 {
            break;
        }
    }
    merged
}

fn collect_related_tests(
    workspace_root: &Path,
    primary: &[ContextFileResult],
    limit: usize,
) -> Vec<ContextFileResult> {
    let mut seen = HashSet::new();
    let mut tests = Vec::new();

    for item in primary {
        let source_path = Path::new(&item.path);
        let Some(stem) = source_path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let source_dir = source_path.parent().unwrap_or_else(|| Path::new(""));
        let stem_lower = stem.to_ascii_lowercase();

        for entry in walkdir::WalkDir::new(workspace_root)
            .follow_links(false)
            .max_depth(8)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = relative_path(workspace_root, entry.path());
            if should_skip_path(&rel) || !is_test_path(&rel) || !seen.insert(rel.clone()) {
                continue;
            }
            let rel_lower = rel.to_ascii_lowercase();
            let same_dir = Path::new(&rel).parent() == Some(source_dir);
            if rel_lower.contains(&stem_lower) || same_dir {
                tests.push(ContextFileResult {
                    path: rel,
                    score: if rel_lower.contains(&stem_lower) {
                        78
                    } else {
                        62
                    },
                    reason: format!("Likely test near or named after {}.", item.path),
                    suggested_ranges: Vec::new(),
                });
                if tests.len() >= limit {
                    return tests;
                }
            }
        }
    }

    tests
}

fn collect_related_docs(
    workspace_root: &Path,
    query: &str,
    limit: usize,
) -> Vec<ContextFileResult> {
    let tokens = query_tokens(query);
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut docs = Vec::new();
    for entry in walkdir::WalkDir::new(workspace_root)
        .follow_links(false)
        .max_depth(6)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = relative_path(workspace_root, entry.path());
        if should_skip_path(&rel) || !is_doc_path(&rel) {
            continue;
        }
        let lower = rel.to_ascii_lowercase();
        let matches = tokens.iter().filter(|token| lower.contains(*token)).count();
        if matches == 0 {
            continue;
        }
        docs.push(ContextFileResult {
            path: rel,
            score: (50 + matches as u32 * 6).min(72),
            reason: "Documentation path matches terms from the request.".to_string(),
            suggested_ranges: Vec::new(),
        });
    }

    docs.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    docs.truncate(limit);
    docs
}

fn collect_memories(workspace_root: &Path, query: &str, limit: usize) -> Vec<ContextMemoryItem> {
    let store = LocalArtifactStore::new(workspace_root);
    let Ok(moments) = store.search_moments(query, limit as i32) else {
        return Vec::new();
    };

    moments
        .into_iter()
        .map(|moment| ContextMemoryItem {
            summary: truncate_chars(&moment.content, 320),
            source: "local_project_memory".to_string(),
            score: (moment.relevance_score * 100.0).round().clamp(0.0, 100.0) as u32,
        })
        .collect()
}

fn score_symbol_result(
    score: f32,
    symbol: &Symbol,
    intent: Option<&str>,
    active_file: Option<&str>,
    open_files: &[String],
) -> u32 {
    let mut value = (score * 100.0).round().clamp(0.0, 95.0) as u32;
    if active_file.is_some_and(|path| path == symbol.file_path) {
        value = value.saturating_add(8);
    }
    if open_files.iter().any(|path| path == &symbol.file_path) {
        value = value.saturating_add(4);
    }
    if intent == Some("bug_fix") && !is_test_path(&symbol.file_path) {
        value = value.saturating_add(3);
    }
    value.min(100)
}

fn symbol_reason(symbol: &Symbol, intent: Option<&str>) -> String {
    let kind = symbol.symbol_type.to_string();
    let name = if symbol.qualified_name.is_empty() {
        symbol.name.as_str()
    } else {
        symbol.qualified_name.as_str()
    };
    match intent {
        Some("bug_fix") => format!(
            "Matched {} `{}` while looking for likely bug-fix entry points.",
            kind, name
        ),
        Some("feature") => format!(
            "Matched {} `{}` as a likely implementation entry point.",
            kind, name
        ),
        _ => format!("Matched indexed {} `{}`.", kind, name),
    }
}

fn range_for_symbol(symbol: &Symbol) -> ContextRange {
    let start = symbol
        .range
        .start
        .line
        .saturating_add(1)
        .saturating_sub(20)
        .max(1);
    let end = symbol
        .range
        .end
        .line
        .saturating_add(1)
        .saturating_add(60)
        .max(start);
    ContextRange {
        start_line: start,
        end_line: end,
    }
}

fn build_hypothesized_flow(primary: &[ContextFileResult]) -> Vec<String> {
    primary
        .iter()
        .take(5)
        .map(|item| format!("Inspect {}", item.path))
        .collect()
}

fn query_tokens(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .map(str::trim)
        .filter(|token| token.len() >= 4)
        .map(|token| token.to_ascii_lowercase())
        .take(12)
        .collect()
}

fn relative_path(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn should_skip_path(path: &str) -> bool {
    path.split('/').any(|part| {
        matches!(
            part,
            ".git" | ".zblade" | "node_modules" | "target" | "dist" | "build" | ".next"
        )
    })
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || lower.starts_with("test/")
        || lower.starts_with("tests/")
        || lower.starts_with("__tests__/")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.ends_with("_test.rs")
}

fn is_doc_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with("docs/")
        || lower.contains("/docs/")
        || lower.ends_with(".md")
        || lower.ends_with(".mdx")
        || lower.ends_with(".rst")
        || lower.ends_with(".adoc")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut output: String = value.chars().take(max_chars).collect();
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_returns_error_payload() {
        let request = ContextPackRequest {
            id: "ctx-1".to_string(),
            query: "   ".to_string(),
            queries: Vec::new(),
            intent: None,
            max_results: None,
            include_tests: None,
            include_docs: None,
            include_memory: None,
            include_project_index_min: None,
        };
        let open_files: Vec<String> = Vec::new();
        let payload = build_context_pack(Path::new("."), None, &open_files, &request);
        assert_eq!(payload.error.unwrap().code, "unsupported");
    }

    #[test]
    fn skips_internal_and_generated_paths() {
        assert!(should_skip_path(".zblade/index/symbols.db"));
        assert!(should_skip_path("node_modules/react/index.js"));
        assert!(!should_skip_path("src/main.rs"));
    }

    #[test]
    fn classifies_tests_and_docs() {
        assert!(is_test_path("src/foo.test.ts"));
        assert!(is_test_path("tests/foo.rs"));
        assert!(is_doc_path("docs/guide.md"));
        assert!(!is_doc_path("src/lib.rs"));
    }

    #[test]
    fn normalizes_queries_and_paths() {
        let extra_queries = vec![
            " fast context ".to_string(),
            "context_pack_request".to_string(),
            "".to_string(),
        ];
        let queries = normalized_queries("Fast Context", &extra_queries);
        assert_eq!(queries, vec!["Fast Context", "context_pack_request"]);

        let workspace_root = Path::new("/workspace/project");
        assert_eq!(
            normalize_workspace_path(workspace_root, "/workspace/project/src/main.rs"),
            Some("src/main.rs".to_string())
        );
        assert_eq!(
            normalize_workspace_paths(
                workspace_root,
                &[
                    "/workspace/project/src/main.rs".to_string(),
                    "src/main.rs".to_string(),
                    "./src/lib.rs".to_string(),
                ],
            ),
            vec!["src/main.rs".to_string(), "src/lib.rs".to_string()]
        );
    }

    fn test_language_service(root: &Path) -> LanguageService {
        let db_path = root.join(".zblade/index/symbols.db");
        let store = std::sync::Arc::new(SymbolStore::new(&db_path).unwrap());
        LanguageService::new(root.to_path_buf(), store).unwrap()
    }

    #[test]
    fn enriches_context_file_with_symbols_anchors_and_ranges() {
        let temp_dir = tempfile::tempdir().unwrap();
        let service = test_language_service(temp_dir.path());
        std::fs::write(
            temp_dir.path().join("service.ts"),
            r#"
            export const route = "/api/blade/context-pack";
            export function buildContextPack() {
                return route;
            }
            "#,
        )
        .unwrap();
        service.index_file("service.ts").unwrap();

        let item = ContextFileResult {
            path: "service.ts".to_string(),
            score: 88,
            reason: "test".to_string(),
            suggested_ranges: vec![ContextRange {
                start_line: 1,
                end_line: 20,
            }],
        };
        let enrichment = enrich_context_file(&service, &item).unwrap();

        assert_eq!(enrichment.path, "service.ts");
        assert_eq!(enrichment.confidence, "high");
        assert!(enrichment
            .symbol_summaries
            .iter()
            .any(|symbol| symbol.name == "buildContextPack"));
        assert!(enrichment
            .semantic_anchors
            .iter()
            .any(|anchor| anchor.value == "/api/blade/context-pack"));
        assert!(!enrichment.suggested_ranges.is_empty());
    }

    #[test]
    fn enrichment_collects_related_import_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let service = test_language_service(temp_dir.path());
        std::fs::write(
            temp_dir.path().join("utils.ts"),
            "export function helperName() { return 'ok'; }",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("main.ts"),
            r#"
            import { helperName } from "./utils";
            export function runMain() { return helperName(); }
            "#,
        )
        .unwrap();
        service.index_file("utils.ts").unwrap();
        service.index_file("main.ts").unwrap();

        let item = ContextFileResult {
            path: "main.ts".to_string(),
            score: 82,
            reason: "test".to_string(),
            suggested_ranges: Vec::new(),
        };
        let enrichment = enrich_context_file(&service, &item).unwrap();

        assert!(enrichment
            .related_files
            .iter()
            .any(|file| file.path == "utils.ts" && file.relationship == "import"));
    }

    #[test]
    fn context_pack_confidence_uses_enrichment_and_index_health() {
        let primary = vec![ContextFileResult {
            path: "service.ts".to_string(),
            score: 90,
            reason: "test".to_string(),
            suggested_ranges: Vec::new(),
        }];
        let enriched = vec![ContextFileEnrichment {
            path: "service.ts".to_string(),
            symbol_summaries: Vec::new(),
            semantic_anchors: Vec::new(),
            related_files: Vec::new(),
            suggested_ranges: Vec::new(),
            confidence: "medium".to_string(),
            next_step: String::new(),
        }];

        assert!(matches!(
            context_pack_confidence(&primary, &enriched, None),
            ContextPackConfidence::Medium
        ));
    }

    #[test]
    fn builds_project_context_from_zblade_context_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let context_dir = temp_dir.path().join(".zblade/context");
        std::fs::create_dir_all(&context_dir).unwrap();
        std::fs::write(
            context_dir.join("project_index_min.md"),
            "minimal project context",
        )
        .unwrap();
        std::fs::write(context_dir.join("project_index.md"), "full project context").unwrap();

        let project_context = build_project_context(temp_dir.path(), true);

        assert_eq!(
            project_context.project_index_min.as_deref(),
            Some("minimal project context")
        );
        assert!(project_context.project_index_min_available);
        assert!(project_context.project_index_available);
        assert!(project_context.project_index_min_requested);
        assert!(project_context.project_index_min_included);
        assert_eq!(
            project_context.project_index_min_reason.as_deref(),
            Some("included")
        );
        assert_eq!(
            project_context.context_source.as_deref(),
            Some("legacy_project_index_metadata")
        );
        assert_eq!(
            project_context.project_index_path.as_deref(),
            Some(".zblade/context/project_index.md")
        );

        let project_context = build_project_context(temp_dir.path(), false);
        assert!(project_context.project_index_min.is_none());
        assert!(project_context.project_index_min_available);
        assert!(!project_context.project_index_min_requested);
        assert!(!project_context.project_index_min_included);
        assert_eq!(
            project_context.project_index_min_reason.as_deref(),
            Some("not_requested")
        );
    }

    #[test]
    fn project_context_reports_missing_project_index_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let project_context = build_project_context(temp_dir.path(), true);

        assert!(project_context.project_index_min.is_none());
        assert!(!project_context.project_index_min_available);
        assert!(!project_context.project_index_available);
        assert!(project_context.project_index_path.is_none());
        assert!(project_context.project_index_min_requested);
        assert!(!project_context.project_index_min_included);
        assert_eq!(
            project_context.project_index_min_reason.as_deref(),
            Some("missing")
        );
    }

    #[test]
    fn project_context_reports_empty_project_index_min_as_unincluded() {
        let temp_dir = tempfile::tempdir().unwrap();
        let context_dir = temp_dir.path().join(".zblade/context");
        std::fs::create_dir_all(&context_dir).unwrap();
        std::fs::write(context_dir.join("project_index_min.md"), "   \n").unwrap();

        let project_context = build_project_context(temp_dir.path(), true);

        assert!(project_context.project_index_min.is_none());
        assert!(project_context.project_index_min_available);
        assert!(project_context.project_index_min_requested);
        assert!(!project_context.project_index_min_included);
        assert_eq!(
            project_context.project_index_min_reason.as_deref(),
            Some("empty_or_unreadable")
        );
    }

    #[test]
    fn context_pack_skips_project_index_min_by_default_when_present() {
        let temp_dir = tempfile::tempdir().unwrap();
        let context_dir = temp_dir.path().join(".zblade/context");
        std::fs::create_dir_all(&context_dir).unwrap();
        std::fs::write(
            context_dir.join("project_index_min.md"),
            "legacy context snapshot",
        )
        .unwrap();

        let request = ContextPackRequest {
            id: "ctx-default".to_string(),
            query: "context".to_string(),
            queries: Vec::new(),
            intent: None,
            max_results: None,
            include_tests: Some(false),
            include_docs: Some(false),
            include_memory: Some(false),
            include_project_index_min: None,
        };
        let open_files: Vec<String> = Vec::new();
        let payload = build_context_pack(temp_dir.path(), None, &open_files, &request);
        let project_context = payload.project_context.unwrap();

        assert!(project_context.project_index_min.is_none());
        assert!(project_context.project_index_min_available);
        assert!(!project_context.project_index_min_requested);
        assert!(!project_context.project_index_min_included);
        assert_eq!(
            project_context.project_index_min_reason.as_deref(),
            Some("not_requested")
        );
    }

    #[test]
    fn context_pack_includes_project_index_min_when_explicitly_requested() {
        let temp_dir = tempfile::tempdir().unwrap();
        let context_dir = temp_dir.path().join(".zblade/context");
        std::fs::create_dir_all(&context_dir).unwrap();
        std::fs::write(
            context_dir.join("project_index_min.md"),
            "legacy context snapshot",
        )
        .unwrap();

        let request = ContextPackRequest {
            id: "ctx-explicit".to_string(),
            query: "context".to_string(),
            queries: Vec::new(),
            intent: None,
            max_results: None,
            include_tests: Some(false),
            include_docs: Some(false),
            include_memory: Some(false),
            include_project_index_min: Some(true),
        };
        let open_files: Vec<String> = Vec::new();
        let payload = build_context_pack(temp_dir.path(), None, &open_files, &request);
        let project_context = payload.project_context.unwrap();

        assert_eq!(
            project_context.project_index_min.as_deref(),
            Some("legacy context snapshot")
        );
        assert!(project_context.project_index_min_available);
        assert!(project_context.project_index_min_requested);
        assert!(project_context.project_index_min_included);
        assert_eq!(
            project_context.project_index_min_reason.as_deref(),
            Some("included")
        );
    }
}
