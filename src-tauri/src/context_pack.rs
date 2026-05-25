use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::blade_protocol::{
    ContextFileResult, ContextMemoryItem, ContextPackConfidence, ContextPackError,
    ContextPackPayload, ContextRange,
};
use crate::language_service::LanguageService;
use crate::local_artifacts::LocalArtifactStore;
use crate::project_settings::get_zblade_dir;
use crate::symbol_index::SymbolStore;
use crate::tree_sitter::Symbol;

#[derive(Debug, Clone)]
pub struct ContextPackRequest {
    pub id: String,
    pub query: String,
    pub intent: Option<String>,
    pub max_results: Option<usize>,
    pub include_tests: Option<bool>,
    pub include_docs: Option<bool>,
    pub include_memory: Option<bool>,
}

pub fn build_context_pack(
    workspace_root: &Path,
    active_file: Option<&str>,
    open_files: &[String],
    request: &ContextPackRequest,
) -> ContextPackPayload {
    let query = request.query.trim();
    if query.is_empty() {
        return error_payload("unsupported", "context_pack_request requires a non-empty query");
    }

    if !workspace_root.exists() || !workspace_root.is_dir() {
        return error_payload("workspace_unavailable", "No usable workspace root is available");
    }

    let max_results = request.max_results.unwrap_or(8).clamp(1, 20);
    let include_tests = request.include_tests.unwrap_or(true);
    let include_docs = request.include_docs.unwrap_or(true);
    let include_memory = request.include_memory.unwrap_or(true);

    let service = match language_service_for_workspace(workspace_root) {
        Ok(service) => service,
        Err(error) => return error_payload("index_unavailable", &error),
    };

    let mut primary = collect_primary_files(
        &service,
        query,
        request.intent.as_deref(),
        max_results,
        active_file,
        open_files,
    );

    if primary.is_empty() {
        primary = collect_fallback_path_matches(workspace_root, query, max_results);
    }

    let related_tests = if include_tests {
        collect_related_tests(workspace_root, &primary, max_results.min(6))
    } else {
        Vec::new()
    };

    let related_docs = if include_docs {
        collect_related_docs(workspace_root, query, max_results.min(6))
    } else {
        Vec::new()
    };

    let memories = if include_memory {
        collect_memories(workspace_root, query, max_results.min(5))
    } else {
        Vec::new()
    };

    let confidence = if primary.len() >= 3 && primary.first().is_some_and(|item| item.score >= 75) {
        ContextPackConfidence::High
    } else if !primary.is_empty() {
        ContextPackConfidence::Medium
    } else {
        ContextPackConfidence::Low
    };

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
            format!("Read {} and {} first, then inspect the related tests if behavior is unclear.", first.path, second.path)
        } else {
            format!("Read {} first, then broaden with symbol or grep search if needed.", first.path)
        }
    } else if let Some(doc) = related_docs.first() {
        format!("Read {} first, then use normal code search to find the implementation path.", doc.path)
    } else {
        "Fall back to normal tool-based workspace exploration.".to_string()
    };

    let hypothesized_flow = build_hypothesized_flow(&primary);

    ContextPackPayload {
        summary,
        confidence,
        primary_files: primary,
        related_tests,
        related_docs,
        memories,
        hypothesized_flow,
        recommended_next_step,
        error: None,
    }
}

pub fn error_payload(code: &str, message: &str) -> ContextPackPayload {
    ContextPackPayload {
        summary: String::new(),
        confidence: ContextPackConfidence::Low,
        primary_files: Vec::new(),
        related_tests: Vec::new(),
        related_docs: Vec::new(),
        memories: Vec::new(),
        hypothesized_flow: Vec::new(),
        recommended_next_step: String::new(),
        error: Some(ContextPackError {
            code: code.to_string(),
            message: message.to_string(),
        }),
    }
}

fn language_service_for_workspace(workspace_root: &Path) -> Result<LanguageService, String> {
    let db_path = get_zblade_dir(workspace_root).join("index").join("symbols.db");
    let symbol_store = std::sync::Arc::new(
        SymbolStore::new(&db_path).map_err(|error| format!("Failed to open symbol index: {}", error))?,
    );
    LanguageService::new(workspace_root.to_path_buf(), symbol_store)
        .map_err(|error| format!("Failed to initialize language service: {}", error))
}

fn collect_primary_files(
    service: &LanguageService,
    query: &str,
    intent: Option<&str>,
    max_results: usize,
    active_file: Option<&str>,
    open_files: &[String],
) -> Vec<ContextFileResult> {
    let mut by_path: HashMap<String, ContextFileResult> = HashMap::new();
    let search_limit = max_results.saturating_mul(6).max(20);

    let Ok(results) = service.search_symbols_contextual(query, search_limit, active_file, open_files)
    else {
        return Vec::new();
    };

    for result in results {
        let path = result.symbol.file_path.clone();
        if should_skip_path(&path) || is_test_path(&path) || is_doc_path(&path) {
            continue;
        }
        let score = score_symbol_result(result.score, &result.symbol, intent, active_file, open_files);
        let reason = symbol_reason(&result.symbol, intent);
        let suggested_ranges = vec![range_for_symbol(&result.symbol)];

        by_path
            .entry(path.clone())
            .and_modify(|existing| {
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

    let mut items: Vec<ContextFileResult> = by_path.into_values().collect();
    items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    items.truncate(max_results);
    items
}

fn collect_fallback_path_matches(
    workspace_root: &Path,
    query: &str,
    max_results: usize,
) -> Vec<ContextFileResult> {
    let tokens = query_tokens(query);
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
                    score: if rel_lower.contains(&stem_lower) { 78 } else { 62 },
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

fn collect_related_docs(workspace_root: &Path, query: &str, limit: usize) -> Vec<ContextFileResult> {
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
        Some("bug_fix") => format!("Matched {} `{}` while looking for likely bug-fix entry points.", kind, name),
        Some("feature") => format!("Matched {} `{}` as a likely implementation entry point.", kind, name),
        _ => format!("Matched indexed {} `{}`.", kind, name),
    }
}

fn range_for_symbol(symbol: &Symbol) -> ContextRange {
    let start = symbol.range.start.line.saturating_add(1).saturating_sub(20).max(1);
    let end = symbol.range.end.line.saturating_add(1).saturating_add(60).max(start);
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
            intent: None,
            max_results: None,
            include_tests: None,
            include_docs: None,
            include_memory: None,
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
}
