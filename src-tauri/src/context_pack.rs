use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::blade_protocol::{
    ContextFileEnrichment, ContextFileResult, ContextImpactFile, ContextImpactSummary,
    ContextImpactTargetSymbol, ContextMemoryItem, ContextPackConfidence, ContextPackError,
    ContextPackPayload, ContextProjectInfo, ContextRange, ContextRelatedFile,
    ContextSemanticAnchorSummary, ContextSymbolSummary, ContextWorkspace, ProjectDirectorySummary,
    ProjectLanguageSummary,
};
use crate::file_lang::{detect_language, is_code_file};
use crate::language_service::{IndexHealthSnapshot, IndexHealthStatus, LanguageService};
use crate::local_artifacts::LocalArtifactStore;
use crate::project_settings::get_zblade_dir;
use crate::symbol_index::SymbolStore;
use crate::tree_sitter::{Symbol, SymbolRelationshipType, SymbolType};

/// Confidence floor applied to raw symbol-search scores before they may
/// contribute to a primary file. With the strict ranking in
/// `symbol_index::search`, a raw score below this is boost-only noise and must
/// not accumulate into file relevance.
const MIN_PRIMARY_RAW_SCORE: f32 = 0.2;
/// Maximum number of lines in an ordinary suggested range.
const MAX_CONTEXT_RANGE_LINES: u32 = 160;
/// Maximum number of lines in a Markdown/documentation suggested range.
const MAX_DOC_RANGE_LINES: u32 = 120;
/// Two evidence ranges whose gap is at most this many lines are clustered.
const RANGE_CLUSTER_GAP_LINES: u32 = 8;
/// Maximum number of disjoint suggested ranges per file.
const MAX_CONTEXT_RANGES: usize = 3;

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
    build_context_pack_internal(workspace_root, active_file, open_files, request, None)
}

pub(crate) fn build_context_pack_with_service(
    workspace_root: &Path,
    active_file: Option<&str>,
    open_files: &[String],
    request: &ContextPackRequest,
    service: &LanguageService,
) -> ContextPackPayload {
    build_context_pack_internal(
        workspace_root,
        active_file,
        open_files,
        request,
        Some(service),
    )
}

fn build_context_pack_internal(
    workspace_root: &Path,
    active_file: Option<&str>,
    open_files: &[String],
    request: &ContextPackRequest,
    provided_service: Option<&LanguageService>,
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

    let owned_service;
    let service = if let Some(service) = provided_service {
        service
    } else {
        owned_service = match language_service_for_workspace(workspace_root) {
            Ok(service) => service,
            Err(error) => {
                let mut payload = error_payload("index_unavailable", &error);
                payload.queries_used = queries;
                payload.workspace = Some(workspace);
                payload.project_context = Some(build_project_context(
                    workspace_root,
                    include_project_index_min,
                    normalized_active_file.as_deref(),
                    &normalized_open_files,
                    &[],
                    None,
                ));
                payload.timing_ms = Some(started_at.elapsed().as_millis() as u64);
                return payload;
            }
        };
        &owned_service
    };

    let index_health = service.audit_index_health().ok();
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
        collect_related_docs(
            &service,
            workspace_root,
            &combined_query,
            max_results.min(6),
        )
    } else {
        Vec::new()
    };

    let memories = if include_memory {
        collect_memories(workspace_root, &combined_query, max_results.min(5))
    } else {
        Vec::new()
    };

    let enriched_files = enrich_primary_files(&service, &primary, max_results);
    let related_files = collect_related_files_from_enrichments(&enriched_files, max_results);

    let confidence = context_pack_confidence(&primary, &enriched_files, index_health.as_ref());
    let impact = build_context_impact(
        workspace_root,
        &service,
        &queries,
        request.intent.as_deref(),
        &primary,
        &enriched_files,
        index_health.as_ref(),
        max_results,
    );
    let primary_paths = primary
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let project_context = build_project_context(
        workspace_root,
        include_project_index_min,
        normalized_active_file.as_deref(),
        &normalized_open_files,
        &primary_paths,
        index_health.as_ref(),
    );

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
        impact,
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
        impact: None,
        index_health: None,
        recommended_next_step: String::new(),
        error: Some(ContextPackError {
            code: code.to_string(),
            message: message.to_string(),
        }),
        timing_ms: None,
    }
}

pub(crate) fn language_service_for_workspace(
    workspace_root: &Path,
) -> Result<LanguageService, String> {
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
    active_file: Option<&str>,
    open_files: &[String],
    candidate_paths: &[String],
    index_health: Option<&IndexHealthSnapshot>,
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
    let (source, confidence, reason) = structured_project_source(index_health);
    let source_files = collect_project_source_files(workspace_root, 5_000);
    let languages = project_language_summaries(&source_files);
    let top_directories = project_directory_summaries(&source_files);
    let likely_entry_points = likely_entry_points(&source_files);
    let mut agent_candidate_paths = open_files.to_vec();
    if let Some(active_file) = active_file {
        agent_candidate_paths.insert(0, active_file.to_string());
    }
    agent_candidate_paths.extend(candidate_paths.iter().cloned());
    let agent_instructions =
        crate::agent_instructions::load_agent_instructions(workspace_root, &agent_candidate_paths);
    // Kept empty for one wire-compatible release. Skill discovery now uses
    // list_skills/load_skill rather than repeatedly embedding the catalog.
    let local_skills = Vec::new();

    ContextProjectInfo {
        project_index_min,
        project_index_min_available,
        project_index_available,
        project_index_path: project_index_available
            .then(|| ".zblade/context/project_index.md".to_string()),
        project_index_min_requested: include_project_index_min,
        project_index_min_included,
        project_index_min_reason: Some(project_index_min_reason.to_string()),
        context_source: Some(source.to_string()),
        source: Some(source.to_string()),
        confidence: Some(confidence.to_string()),
        reason: Some(reason.to_string()),
        root_name: workspace_root
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string),
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        active_file: active_file.map(str::to_string),
        open_files: open_files.to_vec(),
        indexed_files: index_health.map(|health| health.indexed_files),
        supported_files: index_health
            .map(|health| health.supported_files)
            .or(Some(source_files.len())),
        stale_files: index_health.map(|health| health.stale_files),
        missing_files: index_health.map(|health| health.missing_files),
        orphaned_files: index_health.map(|health| health.orphaned_files),
        symbol_count: index_health.map(|health| health.symbol_count),
        languages,
        top_directories,
        likely_entry_points,
        index_health_status: index_health
            .map(|health| format!("{:?}", health.status).to_ascii_lowercase()),
        agent_instructions: agent_instructions.content,
        agent_instruction_files: agent_instructions.files,
        agent_instruction_includes: agent_instructions.includes,
        agent_instructions_truncated: agent_instructions.truncated,
        local_skills,
    }
}

fn project_index_min_reason(requested: bool, available: bool, included: bool) -> &'static str {
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

fn structured_project_source(
    index_health: Option<&IndexHealthSnapshot>,
) -> (&'static str, &'static str, &'static str) {
    match index_health {
        Some(health) => {
            let confidence = match health.status {
                IndexHealthStatus::Fresh => "high",
                IndexHealthStatus::Partial
                | IndexHealthStatus::Stale
                | IndexHealthStatus::Indexing => "medium",
                IndexHealthStatus::Unknown
                | IndexHealthStatus::Checking
                | IndexHealthStatus::Error => "low",
            };
            (
                "structured_index",
                confidence,
                "symbol_index_health_available",
            )
        }
        None => ("workspace_scan_fallback", "low", "symbol_index_unavailable"),
    }
}

fn collect_project_source_files(workspace_root: &Path, limit: usize) -> Vec<String> {
    let mut files = Vec::new();

    for entry in walkdir::WalkDir::new(workspace_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let relative = relative_path(workspace_root, entry.path());
            !should_skip_path(&relative)
        })
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = relative_path(workspace_root, entry.path());
        let relative_path = PathBuf::from(&relative);
        if !is_code_file(&relative_path) {
            continue;
        }
        files.push(relative);
        if files.len() >= limit {
            break;
        }
    }

    files.sort();
    files
}

fn project_language_summaries(files: &[String]) -> Vec<ProjectLanguageSummary> {
    let mut counts = BTreeMap::<String, usize>::new();
    for file in files {
        let language = detect_language(&PathBuf::from(file));
        if language == "text" {
            continue;
        }
        *counts
            .entry(display_language_name(&language).to_string())
            .or_insert(0) += 1;
    }

    let mut summaries = counts
        .into_iter()
        .map(|(name, files)| ProjectLanguageSummary { name, files })
        .collect::<Vec<_>>();
    summaries.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.name.cmp(&b.name)));
    summaries.truncate(8);
    summaries
}

fn project_directory_summaries(files: &[String]) -> Vec<ProjectDirectorySummary> {
    let mut counts = BTreeMap::<String, usize>::new();
    for file in files {
        let directory = project_summary_directory(file);
        *counts.entry(directory).or_insert(0) += 1;
    }

    let mut summaries = counts
        .into_iter()
        .map(|(path, files)| ProjectDirectorySummary { path, files })
        .collect::<Vec<_>>();
    summaries.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.path.cmp(&b.path)));
    summaries.truncate(10);
    summaries
}

fn likely_entry_points(files: &[String]) -> Vec<String> {
    const ENTRYPOINTS: &[&str] = &[
        "src-tauri/src/main.rs",
        "src/main.tsx",
        "src/main.ts",
        "src/main.jsx",
        "src/main.js",
        "src/index.tsx",
        "src/index.ts",
        "src/index.jsx",
        "src/index.js",
        "src/App.tsx",
        "src/lib.rs",
        "main.rs",
        "lib.rs",
        "app/page.tsx",
        "pages/index.tsx",
    ];
    let file_set = files.iter().map(String::as_str).collect::<HashSet<_>>();
    ENTRYPOINTS
        .iter()
        .filter(|path| file_set.contains(**path))
        .map(|path| (*path).to_string())
        .take(8)
        .collect()
}

fn project_summary_directory(path: &str) -> String {
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let Some(first) = parts.next() else {
        return ".".to_string();
    };
    let Some(second) = parts.next() else {
        return ".".to_string();
    };
    format!("{first}/{second}")
}

fn display_language_name(language: &str) -> &str {
    match language {
        "rust" => "Rust",
        "python" => "Python",
        "javascript" => "JavaScript",
        "typescript" => "TypeScript",
        "go" => "Go",
        "java" => "Java",
        "cpp" => "C++",
        "csharp" => "C#",
        "ruby" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "kotlin" => "Kotlin",
        "scala" => "Scala",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "sql" => "SQL",
        "bash" => "Bash",
        "yaml" => "YAML",
        "toml" => "TOML",
        "json" => "JSON",
        "html" => "HTML",
        "css" => "CSS",
        "scss" => "SCSS",
        "less" => "Less",
        "markdown" => "Markdown",
        other => other,
    }
}

/// One above-floor symbol hit buffered for per-file aggregation.
struct PrimaryHit {
    raw_score: f32,
    /// Index of the query facet (position in the normalized query list).
    facet_index: usize,
    symbol: Symbol,
    reason: String,
    why: Vec<String>,
    exact_range: ContextRange,
    padded_range: ContextRange,
}

fn collect_primary_files_for_queries(
    service: &LanguageService,
    queries: &[String],
    intent: Option<&str>,
    max_results: usize,
    active_file: Option<&str>,
    open_files: &[String],
) -> Vec<ContextFileResult> {
    let mut hits_by_path: HashMap<String, Vec<PrimaryHit>> = HashMap::new();
    let search_limit = max_results.saturating_mul(6).max(20);

    for (facet_index, query) in queries.iter().enumerate() {
        // Non-contextual retrieval on purpose: editor-state (active/open file)
        // boosts are applied exactly ONCE per file in `aggregate_primary_hits`
        // via `score_symbol_result`. Contextual retrieval would bake the same
        // boosts into every raw hit score and double-count them. This path
        // also applies visible-symbol filtering.
        let Ok(results) = service.search_symbols(query, search_limit) else {
            continue;
        };

        for result in results {
            let path = result.symbol.file_path.clone();
            if should_skip_path(&path) || is_test_path(&path) || is_doc_path(&path) {
                continue;
            }
            // Confidence floor on the RAW score, before any buffering: hits
            // below the floor may not accumulate into file relevance at all.
            if result.score < MIN_PRIMARY_RAW_SCORE {
                continue;
            }
            let reason = symbol_reason(&result.symbol, intent);
            let why = primary_file_why(query, &result.symbol, result.score);
            let exact_range = exact_range_for_symbol(&result.symbol);
            let padded_range = range_for_symbol(&result.symbol);
            hits_by_path.entry(path).or_default().push(PrimaryHit {
                raw_score: result.score,
                facet_index,
                symbol: result.symbol,
                reason,
                why,
                exact_range,
                padded_range,
            });
        }
    }

    let mut items: Vec<ContextFileResult> = hits_by_path
        .into_iter()
        .filter_map(|(path, hits)| {
            aggregate_primary_hits(path, hits, queries.len(), intent, active_file, open_files)
        })
        .collect();
    items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    items.truncate(max_results);
    items
}

/// Combine buffered per-symbol hits for one file into a single ranked result.
///
/// Hits are first deduplicated per SYMBOL: one symbol matched by several
/// query facets keeps only its best raw score, so it cannot fill multiple
/// diminishing-returns slots. Raw scores then combine with diminishing
/// returns over DISTINCT symbols (`best + 0.25*second + 0.10*third`, further
/// hits ignored) so a pile of weak hits cannot outrank one strong hit, plus
/// a small bonus for covering additional DISTINCT query facets (+0.05 each,
/// capped at +0.15; facet coverage counts every hit, including deduplicated
/// ones). Active/open/intent boosts are then applied exactly once per file
/// during quantization, never per hit.
fn aggregate_primary_hits(
    path: String,
    mut hits: Vec<PrimaryHit>,
    facet_count: usize,
    intent: Option<&str>,
    active_file: Option<&str>,
    open_files: &[String],
) -> Option<ContextFileResult> {
    // Defensive re-check of the confidence floor: never aggregate noise.
    hits.retain(|hit| hit.raw_score >= MIN_PRIMARY_RAW_SCORE);
    if hits.is_empty() {
        return None;
    }
    hits.sort_by(|a, b| {
        b.raw_score
            .partial_cmp(&a.raw_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.facet_index.cmp(&b.facet_index))
    });

    // Facet coverage counts DISTINCT facets across ALL hits (before symbol
    // dedupe): a symbol matched by two facets still covers both facets.
    let matched_facets = hits
        .iter()
        .map(|hit| hit.facet_index)
        .collect::<HashSet<_>>()
        .len();

    // Dedupe per SYMBOL, keeping the best-scored hit (the list is sorted by
    // raw score descending). One symbol matched by multiple query facets must
    // occupy only ONE diminishing-returns slot. The key includes name+range
    // alongside the store id so symbols without an assigned id still dedupe
    // by identity rather than colliding on the empty string.
    let mut seen_symbols = HashSet::new();
    hits.retain(|hit| {
        seen_symbols.insert((
            hit.symbol.id.clone(),
            hit.symbol.name.clone(),
            hit.symbol.range.start.line,
            hit.symbol.range.start.character,
            hit.symbol.range.end.line,
            hit.symbol.range.end.character,
        ))
    });

    let combined_raw = hits[0].raw_score
        + hits.get(1).map_or(0.0, |hit| hit.raw_score * 0.25)
        + hits.get(2).map_or(0.0, |hit| hit.raw_score * 0.10);
    let facet_bonus = 0.05 * matched_facets.saturating_sub(1).min(3) as f32;
    let best = &hits[0];
    // Quantize ONCE per file, applying active/open/intent boosts here only.
    let score = score_symbol_result(
        combined_raw + facet_bonus,
        &best.symbol,
        intent,
        active_file,
        open_files,
    )
    .min(100);

    let mut why = best.why.clone();
    for hit in hits.iter().skip(1) {
        merge_unique_reasons(&mut why, &hit.why, 8);
    }
    let mut summary_lines = vec![
        format!(
            "strongest evidence: {} `{}` (raw score {:.2})",
            best.symbol.symbol_type,
            symbol_display_name(&best.symbol),
            best.raw_score
        ),
        format!(
            "facet coverage: matched {} of {} query facet(s)",
            matched_facets, facet_count
        ),
    ];
    if let Some(intent) = intent {
        summary_lines.push(format!("intent `{}` influenced ranking", intent));
    }
    if active_file.is_some_and(|active| active == path) {
        summary_lines.push("active file boost applied".to_string());
    }
    if open_files.iter().any(|open| open == &path) {
        summary_lines.push("open file boost applied".to_string());
    }
    // Hit-derived lines are capped at 8 above; 13 guarantees room for all
    // five possible file-level summary lines.
    merge_unique_reasons(&mut why, &summary_lines, 13);

    Some(ContextFileResult {
        path,
        score,
        reason: best.reason.clone(),
        why,
        // The exact matched-symbol range stays FIRST so downstream clustering
        // keeps it as the highest-priority anchor.
        suggested_ranges: vec![best.exact_range, best.padded_range],
    })
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
        .filter_entry(|entry| {
            let relative = relative_path(workspace_root, entry.path());
            !should_skip_path(&relative)
        })
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = relative_path(workspace_root, path);
        if is_test_path(&rel) || is_doc_path(&rel) {
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
            why: vec![
                format!("file path matched {} query token(s)", matches),
                "symbol index did not produce a stronger source-file match".to_string(),
            ],
            suggested_ranges: vec![ContextRange {
                start_line: 1,
                // Same cap as every other ordinary suggested range.
                end_line: MAX_CONTEXT_RANGE_LINES,
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
        .map(symbol_to_context_summary)
        .collect::<Vec<_>>();
    let matched_symbols = symbols
        .iter()
        .filter(|symbol| {
            // Overlap on the EXACT symbol range (both sides 1-based inclusive)
            // so padding cannot fabricate a match.
            let symbol_range = exact_range_for_symbol(symbol);
            item.suggested_ranges
                .iter()
                .any(|range| ranges_overlap(&symbol_range, range))
        })
        .take(6)
        .map(symbol_to_context_summary)
        .collect::<Vec<_>>();

    let semantic_anchors = collect_context_semantic_anchors(service, &item.path, &symbols)?;
    let related_files = collect_context_related_files(service, &item.path, &symbols)?;
    let range_cap = range_cap_for_path(&item.path);
    // Priority order: the item's own ranges first (exact matched-symbol range
    // leads), then key-symbol context, then semantic-anchor context.
    let (suggested_ranges, ranges_clipped) = cluster_context_ranges(
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
                // anchor.line is already 1-based in the summary.
                start_line: anchor.line.saturating_sub(8).max(1),
                end_line: anchor.line.saturating_add(24),
            })),
        range_cap,
    );
    let confidence = if item.score >= 80 && !symbol_summaries.is_empty() {
        "high"
    } else if item.score >= 55 || !symbol_summaries.is_empty() || !semantic_anchors.is_empty() {
        "medium"
    } else {
        "low"
    }
    .to_string();
    let mut why = enrichment_why(
        item,
        symbol_summaries.len(),
        matched_symbols.len(),
        semantic_anchors.len(),
        related_files.len(),
        &confidence,
    );
    if ranges_clipped {
        merge_unique_reasons(&mut why, &[range_clipped_reason(range_cap)], 19);
    }
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
        matched_symbols,
        semantic_anchors,
        related_files,
        suggested_ranges,
        confidence,
        why,
        next_step,
    })
}

fn ranges_overlap(left: &ContextRange, right: &ContextRange) -> bool {
    left.start_line <= right.end_line && right.start_line <= left.end_line
}

fn enrichment_why(
    item: &ContextFileResult,
    symbol_count: usize,
    matched_symbol_count: usize,
    semantic_anchor_count: usize,
    related_file_count: usize,
    confidence: &str,
) -> Vec<String> {
    let mut why = item.why.clone();
    merge_unique_reasons(
        &mut why,
        &[
            format!("{} key symbol(s) indexed in this file", symbol_count),
            format!(
                "{} matched symbol(s) overlap suggested ranges",
                matched_symbol_count
            ),
            format!("{} semantic anchor(s) found", semantic_anchor_count),
            format!("{} related file(s) found", related_file_count),
            format!("enrichment confidence is {}", confidence),
        ],
        // Aggregated primary whys can reach 13 lines; leave room for these
        // five enrichment lines on top.
        18,
    );
    why
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
                value: crate::secrets::redact_secret_tokens(&anchor.value),
                line: anchor.line.saturating_add(1),
                preview: truncate_chars(
                    &crate::secrets::redact_secret_tokens(&anchor.preview),
                    220,
                ),
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
                    value: crate::secrets::redact_secret_tokens(&result.anchor.value),
                    line: result.anchor.line.saturating_add(1),
                    preview: truncate_chars(
                        &crate::secrets::redact_secret_tokens(&result.anchor.preview),
                        220,
                    ),
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

fn build_context_impact(
    workspace_root: &Path,
    service: &LanguageService,
    queries: &[String],
    intent: Option<&str>,
    primary: &[ContextFileResult],
    enriched_files: &[ContextFileEnrichment],
    index_health: Option<&IndexHealthSnapshot>,
    max_results: usize,
) -> Option<ContextImpactSummary> {
    let trigger_reason = impact_trigger_reason(queries, intent, primary.first())?;
    let target_symbols =
        collect_context_impact_target_symbols(service, primary, enriched_files, max_results.min(4));
    if target_symbols.is_empty() {
        return None;
    }

    let relationship_types = [
        SymbolRelationshipType::Call,
        SymbolRelationshipType::Import,
        SymbolRelationshipType::Export,
        SymbolRelationshipType::Extends,
        SymbolRelationshipType::Implements,
    ];
    let mut impacted = BTreeMap::<String, (u32, Vec<String>, Vec<ContextRange>)>::new();
    let mut target_payloads = Vec::new();
    let mut reference_count = 0usize;
    let graph_limit = max_results.clamp(4, 10);

    for symbol in &target_symbols {
        let entry = impacted
            .entry(symbol.file_path.clone())
            .or_insert_with(|| (90, Vec::new(), Vec::new()));
        merge_unique_reasons(
            &mut entry.1,
            &[format!("direct edit target `{}`", symbol.name)],
            6,
        );
        // Exact range first so clustering anchors on the edit target itself.
        entry.2.push(exact_range_for_symbol(symbol));
        entry.2.push(range_for_symbol(symbol));

        let mut incoming_count = 0usize;
        let mut outgoing_count = 0usize;

        for relationship_type in relationship_types {
            let Ok(graph) = service.get_symbol_graph(symbol, relationship_type, graph_limit) else {
                continue;
            };

            for reference in graph.incoming {
                reference_count += 1;
                incoming_count += 1;
                let relationship_name = reference.relationship_type.to_string();
                let entry = impacted
                    .entry(reference.source_symbol.file_path.clone())
                    .or_insert_with(|| (0, Vec::new(), Vec::new()));
                entry.0 = entry.0.max(if reference.target_symbol_id.is_some() {
                    78
                } else {
                    62
                });
                merge_unique_reasons(
                    &mut entry.1,
                    &[format!(
                        "incoming {} reference to `{}`",
                        relationship_name, symbol.name
                    )],
                    6,
                );
                entry.2.push(ContextRange {
                    start_line: reference.line.saturating_add(1).saturating_sub(8).max(1),
                    end_line: reference.line.saturating_add(1).saturating_add(24),
                });
            }

            for reference in graph.outgoing {
                reference_count += 1;
                outgoing_count += 1;
                let Some(related_path) = reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.clone())
                    .or_else(|| {
                        (reference.relationship_type == SymbolRelationshipType::Import)
                            .then(|| reference.target_name.clone())
                    })
                else {
                    continue;
                };
                if related_path == symbol.file_path {
                    continue;
                }
                let relationship_name = reference.relationship_type.to_string();
                let entry = impacted
                    .entry(related_path)
                    .or_insert_with(|| (0, Vec::new(), Vec::new()));
                entry.0 = entry.0.max(if reference.target_symbol_id.is_some() {
                    70
                } else {
                    56
                });
                merge_unique_reasons(
                    &mut entry.1,
                    &[format!(
                        "outgoing {} dependency from `{}`",
                        relationship_name, symbol.name
                    )],
                    6,
                );
            }
        }

        target_payloads.push(ContextImpactTargetSymbol {
            name: symbol.name.clone(),
            qualified_name: symbol.qualified_name.clone(),
            kind: symbol.symbol_type.to_string(),
            path: symbol.file_path.clone(),
            line: symbol.range.start.line.saturating_add(1),
            incoming_count,
            outgoing_count,
        });
    }

    let mut impacted_files = Vec::with_capacity(impacted.len());
    for (path, (score, mut reasons, ranges)) in impacted {
        let range_cap = range_cap_for_path(&path);
        // The disjoint-range budget lives inside the clustering fn now.
        let (suggested_ranges, ranges_clipped) =
            cluster_context_ranges(ranges.into_iter(), range_cap);
        if ranges_clipped {
            merge_unique_reasons(&mut reasons, &[range_clipped_reason(range_cap)], 7);
        }
        impacted_files.push(ContextImpactFile {
            path,
            score,
            reasons,
            suggested_ranges,
        });
    }
    impacted_files.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    impacted_files.truncate(max_results.min(8));

    let impacted_as_results = impacted_files
        .iter()
        .map(|file| ContextFileResult {
            path: file.path.clone(),
            score: file.score,
            reason: file
                .reasons
                .first()
                .cloned()
                .unwrap_or_else(|| "Potentially impacted file.".to_string()),
            why: file.reasons.clone(),
            suggested_ranges: file.suggested_ranges.clone(),
        })
        .collect::<Vec<_>>();
    let likely_tests =
        collect_related_tests(workspace_root, &impacted_as_results, max_results.min(6));
    let index_fresh =
        index_health.is_some_and(|health| matches!(health.status, IndexHealthStatus::Fresh));
    let risk = context_impact_risk(impacted_files.len(), reference_count, likely_tests.len());
    let confidence =
        context_impact_confidence(index_fresh, target_payloads.len(), impacted_files.len());

    Some(ContextImpactSummary {
        enabled: true,
        reason: trigger_reason,
        risk,
        confidence,
        target_symbols: target_payloads,
        impacted_files,
        likely_tests,
        recommended_next_steps: vec![
            "Read suggested_ranges for high-score impacted files before editing.".to_string(),
            "Run or inspect likely_tests after making the change.".to_string(),
            "Use edit_impact for deeper blast-radius analysis if risk is medium or high."
                .to_string(),
        ],
    })
}

fn impact_trigger_reason(
    queries: &[String],
    intent: Option<&str>,
    first_primary: Option<&ContextFileResult>,
) -> Option<String> {
    if matches!(intent, Some("docs" | "explain" | "question")) {
        return None;
    }
    if let Some(intent @ ("bug_fix" | "feature" | "refactor")) = intent {
        return Some(format!("intent_{}", intent));
    }
    let combined = queries.join(" ").to_ascii_lowercase();
    let edit_verbs = [
        "fix", "change", "update", "remove", "rename", "refactor", "replace", "add", "edit",
        "delete",
    ];
    if let Some(verb) = edit_verbs
        .iter()
        .find(|verb| combined.split_whitespace().any(|token| token == **verb))
    {
        return Some(format!("query_edit_verb_{}", verb));
    }
    first_primary
        .filter(|item| item.score >= 82)
        .map(|_| "high_confidence_primary_file".to_string())
}

fn collect_context_impact_target_symbols(
    service: &LanguageService,
    primary: &[ContextFileResult],
    enriched_files: &[ContextFileEnrichment],
    limit: usize,
) -> Vec<Symbol> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();

    for item in primary.iter().take(4) {
        let matched_names = enriched_files
            .iter()
            .find(|enrichment| enrichment.path == item.path)
            .map(|enrichment| {
                enrichment
                    .matched_symbols
                    .iter()
                    .map(|symbol| symbol.name.as_str())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        let Ok(symbols) = service.get_file_symbols(&item.path) else {
            continue;
        };
        for symbol in symbols {
            if !is_context_impact_target_symbol(&symbol) {
                continue;
            }
            // Exact symbol range: padding must not fabricate an edit target.
            let range = exact_range_for_symbol(&symbol);
            let range_match = item
                .suggested_ranges
                .iter()
                .any(|suggested| ranges_overlap(&range, suggested));
            let name_match = matched_names.contains(symbol.name.as_str());
            if !range_match && !name_match {
                continue;
            }
            if seen.insert(symbol.id.clone()) {
                targets.push(symbol);
            }
            if targets.len() >= limit {
                return targets;
            }
        }
    }

    targets
}

fn is_context_impact_target_symbol(symbol: &Symbol) -> bool {
    matches!(
        symbol.symbol_type,
        SymbolType::Function
            | SymbolType::Method
            | SymbolType::Class
            | SymbolType::Struct
            | SymbolType::Interface
            | SymbolType::Type
            | SymbolType::Enum
            | SymbolType::Trait
            | SymbolType::Impl
            | SymbolType::Module
    )
}

fn context_impact_risk(
    impacted_file_count: usize,
    reference_count: usize,
    test_count: usize,
) -> String {
    if impacted_file_count >= 8 || reference_count >= 16 {
        "high".to_string()
    } else if impacted_file_count >= 3 || reference_count >= 4 || test_count == 0 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

fn context_impact_confidence(
    index_fresh: bool,
    symbol_count: usize,
    impacted_file_count: usize,
) -> String {
    if index_fresh && symbol_count > 0 && impacted_file_count > 0 {
        "high".to_string()
    } else if symbol_count > 0 || impacted_file_count > 0 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
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

/// Cluster prioritized evidence ranges into at most MAX_CONTEXT_RANGES
/// disjoint ranges.
///
/// Input order is PRIORITY order: the first range is the strongest evidence
/// (callers keep the exact matched-symbol range first). Ranges whose gap is at
/// most RANGE_CLUSTER_GAP_LINES lines are unioned into one cluster; each
/// cluster is capped at `max_lines`, clipped around its highest-priority
/// member so that evidence is never clipped out. Returns the clusters sorted
/// by start line, plus a flag that is true when any line clipping happened or
/// a range had to be dropped for the disjoint-range budget.
fn cluster_context_ranges<I>(ranges: I, max_lines: u32) -> (Vec<ContextRange>, bool)
where
    I: IntoIterator<Item = ContextRange>,
{
    struct Cluster {
        start: u32,
        end: u32,
        // Highest-priority member range; clipping must keep it visible.
        anchor_start: u32,
        anchor_end: u32,
    }

    let max_lines = max_lines.max(1);
    let mut clusters: Vec<Cluster> = Vec::new();
    let mut clipped = false;

    for range in ranges {
        if range.end_line < range.start_line || range.start_line == 0 {
            continue;
        }
        // A range and a cluster belong together when they overlap or the gap
        // between them is at most RANGE_CLUSTER_GAP_LINES lines.
        let mut matched: Vec<usize> = Vec::new();
        for (index, cluster) in clusters.iter().enumerate() {
            let near = range.start_line <= cluster.end.saturating_add(RANGE_CLUSTER_GAP_LINES + 1)
                && cluster.start <= range.end_line.saturating_add(RANGE_CLUSTER_GAP_LINES + 1);
            if near {
                matched.push(index);
            }
        }

        if let Some(&first) = matched.first() {
            // Union into the highest-priority (earliest-created) near cluster,
            // keeping its anchor; a bridging range may then merge later
            // clusters into it as well.
            clusters[first].start = clusters[first].start.min(range.start_line);
            clusters[first].end = clusters[first].end.max(range.end_line);
            for &index in matched.iter().skip(1).rev() {
                let other = clusters.remove(index);
                clusters[first].start = clusters[first].start.min(other.start);
                clusters[first].end = clusters[first].end.max(other.end);
            }
        } else if clusters.len() < MAX_CONTEXT_RANGES {
            clusters.push(Cluster {
                start: range.start_line,
                end: range.end_line,
                anchor_start: range.start_line,
                anchor_end: range.end_line,
            });
        } else {
            // No room for another disjoint cluster; drop the weaker evidence
            // and say so.
            clipped = true;
        }
    }

    let mut result = Vec::with_capacity(clusters.len());
    for cluster in clusters {
        let span = cluster.end - cluster.start + 1;
        if span <= max_lines {
            result.push(ContextRange {
                start_line: cluster.start,
                end_line: cluster.end,
            });
            continue;
        }
        clipped = true;
        let anchor_span = cluster.anchor_end - cluster.anchor_start + 1;
        let start = if anchor_span >= max_lines {
            // The anchor itself exceeds the cap; keep its head.
            cluster.anchor_start
        } else {
            // Prefer the cluster start, but stay close enough to keep the
            // whole anchor range inside the capped window.
            cluster
                .start
                .max(cluster.anchor_end.saturating_sub(max_lines - 1))
        };
        result.push(ContextRange {
            start_line: start,
            end_line: start.saturating_add(max_lines - 1),
        });
    }
    result.sort_by(|a, b| {
        a.start_line
            .cmp(&b.start_line)
            .then_with(|| a.end_line.cmp(&b.end_line))
    });
    (result, clipped)
}

/// Why-string appended by callers when `cluster_context_ranges` reports
/// clipping, so the model knows the suggestion is intentionally bounded.
fn range_clipped_reason(max_lines: u32) -> String {
    format!(
        "ranges clipped to at most {} disjoint ranges of {} lines",
        MAX_CONTEXT_RANGES, max_lines
    )
}

/// Line cap for suggested ranges in `path`: Markdown/doc files get the tighter
/// documentation cap.
fn range_cap_for_path(path: &str) -> u32 {
    if is_doc_path(path) {
        MAX_DOC_RANGE_LINES
    } else {
        MAX_CONTEXT_RANGE_LINES
    }
}

fn collect_related_tests(
    workspace_root: &Path,
    primary: &[ContextFileResult],
    limit: usize,
) -> Vec<ContextFileResult> {
    if primary.is_empty() || limit == 0 {
        return Vec::new();
    }

    // Walk the workspace once (pruning skipped directories at descent time)
    // instead of one full walk per primary file.
    let mut test_paths = Vec::new();
    for entry in walkdir::WalkDir::new(workspace_root)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_entry(|entry| {
            let relative = relative_path(workspace_root, entry.path());
            !should_skip_path(&relative)
        })
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = relative_path(workspace_root, entry.path());
        if is_test_path(&rel) {
            test_paths.push(rel);
        }
    }

    let mut seen = HashSet::new();
    let mut tests = Vec::new();

    for item in primary {
        let source_path = Path::new(&item.path);
        let Some(stem) = source_path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let source_dir = source_path.parent().unwrap_or_else(|| Path::new(""));
        let stem_lower = stem.to_ascii_lowercase();

        for rel in &test_paths {
            if seen.contains(rel) {
                continue;
            }
            let rel_lower = rel.to_ascii_lowercase();
            let same_dir = Path::new(rel).parent() == Some(source_dir);
            if rel_lower.contains(&stem_lower) || same_dir {
                seen.insert(rel.clone());
                tests.push(ContextFileResult {
                    path: rel.clone(),
                    score: if rel_lower.contains(&stem_lower) {
                        78
                    } else {
                        62
                    },
                    reason: format!("Likely test near or named after {}.", item.path),
                    why: vec![
                        format!("test candidate matched source stem `{}`", stem),
                        format!("test candidate is associated with `{}`", item.path),
                    ],
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
    service: &LanguageService,
    workspace_root: &Path,
    query: &str,
    limit: usize,
) -> Vec<ContextFileResult> {
    let indexed = collect_indexed_doc_sections(service, query, limit);
    if !indexed.is_empty() {
        return indexed;
    }

    collect_related_docs_by_path(workspace_root, query, limit)
}

fn collect_indexed_doc_sections(
    service: &LanguageService,
    query: &str,
    limit: usize,
) -> Vec<ContextFileResult> {
    let Ok(results) =
        service.search_symbols_filtered(query, None, Some(vec![SymbolType::Heading]), limit * 4)
    else {
        return Vec::new();
    };
    let mut docs = Vec::new();
    let mut seen = HashSet::new();

    for result in results {
        let symbol = result.symbol;
        if !is_doc_path(&symbol.file_path) || !seen.insert(symbol.id.clone()) {
            continue;
        }
        let range = doc_section_range(service, &symbol);
        docs.push(ContextFileResult {
            path: symbol.file_path.clone(),
            score: (62.0 + result.score * 30.0).round().clamp(0.0, 92.0) as u32,
            reason: format!(
                "Indexed Markdown section `{}` matches the request.",
                symbol.name
            ),
            why: vec![
                "matched indexed Markdown heading".to_string(),
                format!("section heading `{}`", symbol.name),
                format!("symbol index score {:.2}", result.score),
            ],
            suggested_ranges: vec![range],
        });
        if docs.len() >= limit {
            break;
        }
    }

    docs.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    docs
}

fn doc_section_range(service: &LanguageService, heading: &Symbol) -> ContextRange {
    let start_line = heading.range.start.line.saturating_add(1);
    let heading_level = markdown_heading_level(heading).unwrap_or(6);
    let end_line = service
        .get_file_symbols(&heading.file_path)
        .ok()
        .and_then(|symbols| {
            symbols
                .into_iter()
                .filter(|symbol| symbol.symbol_type == SymbolType::Heading)
                .filter(|symbol| symbol.range.start.line > heading.range.start.line)
                .filter(|symbol| markdown_heading_level(symbol).unwrap_or(6) <= heading_level)
                .map(|symbol| symbol.range.start.line.saturating_add(1).saturating_sub(1))
                .min()
        })
        .unwrap_or_else(|| start_line.saturating_add(80));

    ContextRange {
        start_line,
        // `start..=start + (MAX_DOC_RANGE_LINES - 1)` is an INCLUSIVE span of
        // exactly MAX_DOC_RANGE_LINES lines; this range is emitted un-clipped
        // by the related-docs lane, so the cap must hold here.
        end_line: end_line
            .max(start_line)
            .min(start_line.saturating_add(MAX_DOC_RANGE_LINES.saturating_sub(1))),
    }
}

fn markdown_heading_level(symbol: &Symbol) -> Option<usize> {
    symbol
        .signature
        .as_deref()
        .and_then(|signature| signature.strip_prefix('h'))
        .and_then(|level| level.parse::<usize>().ok())
}

fn collect_related_docs_by_path(
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
        .filter_entry(|entry| {
            let relative = relative_path(workspace_root, entry.path());
            !should_skip_path(&relative)
        })
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = relative_path(workspace_root, entry.path());
        if !is_doc_path(&rel) {
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
            why: vec![format!(
                "documentation path matched {} query token(s)",
                matches
            )],
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

/// Per-hit rationale lines. Intent/active/open boost lines are file-level and
/// added once during aggregation, not here.
fn primary_file_why(query: &str, symbol: &Symbol, raw_score: f32) -> Vec<String> {
    vec![
        format!(
            "query `{}` matched indexed {} `{}`",
            truncate_chars(query, 80),
            symbol.symbol_type,
            symbol_display_name(symbol)
        ),
        format!("symbol search raw score {:.2}", raw_score),
    ]
}

fn symbol_display_name(symbol: &Symbol) -> &str {
    if symbol.qualified_name.is_empty() {
        symbol.name.as_str()
    } else {
        symbol.qualified_name.as_str()
    }
}

fn symbol_to_context_summary(symbol: &Symbol) -> ContextSymbolSummary {
    ContextSymbolSummary {
        name: symbol.name.clone(),
        qualified_name: symbol.qualified_name.clone(),
        kind: symbol.symbol_type.to_string(),
        line: symbol.range.start.line.saturating_add(1),
    }
}

fn merge_unique_reasons(target: &mut Vec<String>, incoming: &[String], limit: usize) {
    for reason in incoming {
        if target.len() >= limit {
            break;
        }
        if !target.iter().any(|existing| existing == reason) {
            target.push(reason.clone());
        }
    }
}

/// Exact 1-based inclusive line range of the symbol itself, with no padding.
/// `Symbol.range` lines are 0-based; `ContextRange` lines are 1-based.
fn exact_range_for_symbol(symbol: &Symbol) -> ContextRange {
    let start = symbol.range.start.line.saturating_add(1).max(1);
    let end = symbol.range.end.line.saturating_add(1).max(start);
    ContextRange {
        start_line: start,
        end_line: end,
    }
}

/// Padded context range around the symbol (up to 20 lines before, 60 after),
/// capped at MAX_CONTEXT_RANGE_LINES total so an oversized symbol cannot
/// produce an unbounded suggestion. The symbol's first line always stays
/// inside the capped range; the exact range is preserved separately by
/// `exact_range_for_symbol`.
fn range_for_symbol(symbol: &Symbol) -> ContextRange {
    let exact = exact_range_for_symbol(symbol);
    let start = exact.start_line.saturating_sub(20).max(1);
    let end = exact
        .end_line
        .saturating_add(60)
        .max(start)
        .min(start.saturating_add(MAX_CONTEXT_RANGE_LINES - 1));
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
    use crate::tree_sitter::{Position, Range};

    fn test_symbol(name: &str, start_line_0: u32, end_line_0: u32) -> Symbol {
        Symbol::new(
            name.to_string(),
            SymbolType::Function,
            "src/lib.rs".to_string(),
            Range::new(Position::new(start_line_0, 0), Position::new(end_line_0, 0)),
        )
    }

    fn test_primary_hit(
        raw_score: f32,
        facet_index: usize,
        name: &str,
        start_line_0: u32,
        end_line_0: u32,
    ) -> PrimaryHit {
        let symbol = test_symbol(name, start_line_0, end_line_0);
        PrimaryHit {
            raw_score,
            facet_index,
            reason: symbol_reason(&symbol, None),
            why: primary_file_why("test query", &symbol, raw_score),
            exact_range: exact_range_for_symbol(&symbol),
            padded_range: range_for_symbol(&symbol),
            symbol,
        }
    }

    fn cr(start_line: u32, end_line: u32) -> ContextRange {
        ContextRange {
            start_line,
            end_line,
        }
    }

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
            why: vec!["test reason".to_string()],
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
            .matched_symbols
            .iter()
            .any(|symbol| symbol.name == "buildContextPack"));
        assert!(enrichment
            .why
            .iter()
            .any(|reason| reason.contains("key symbol")));
        assert!(enrichment
            .semantic_anchors
            .iter()
            .any(|anchor| anchor.value == "/api/blade/context-pack"));
        assert!(!enrichment.suggested_ranges.is_empty());
        assert!(enrichment.suggested_ranges.len() <= MAX_CONTEXT_RANGES);
        assert!(enrichment
            .suggested_ranges
            .iter()
            .all(|range| range.end_line - range.start_line < MAX_CONTEXT_RANGE_LINES));
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
            why: Vec::new(),
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
            why: Vec::new(),
            suggested_ranges: Vec::new(),
        }];
        let enriched = vec![ContextFileEnrichment {
            path: "service.ts".to_string(),
            symbol_summaries: Vec::new(),
            matched_symbols: Vec::new(),
            semantic_anchors: Vec::new(),
            related_files: Vec::new(),
            suggested_ranges: Vec::new(),
            confidence: "medium".to_string(),
            why: Vec::new(),
            next_step: String::new(),
        }];

        assert!(matches!(
            context_pack_confidence(&primary, &enriched, None),
            ContextPackConfidence::Medium
        ));
    }

    #[test]
    fn context_pack_refactor_task_includes_bounded_impact_summary() {
        let temp_dir = tempfile::tempdir().unwrap();
        let service = test_language_service(temp_dir.path());
        std::fs::write(
            temp_dir.path().join("helper.ts"),
            "export function helperName() { return 'ok'; }",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("main.ts"),
            r#"
            import { helperName } from "./helper";
            export function runMain() { return helperName(); }
            "#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("helper.test.ts"),
            "import { helperName } from './helper'; test('helper', () => helperName());",
        )
        .unwrap();
        service.index_file("helper.ts").unwrap();
        service.index_file("main.ts").unwrap();
        service.index_file("helper.test.ts").unwrap();

        let request = ContextPackRequest {
            id: "ctx-impact".to_string(),
            query: "refactor helperName".to_string(),
            queries: Vec::new(),
            intent: Some("refactor".to_string()),
            max_results: Some(6),
            include_tests: Some(true),
            include_docs: Some(false),
            include_memory: Some(false),
            include_project_index_min: Some(false),
        };
        let open_files: Vec<String> = Vec::new();
        let payload = build_context_pack(temp_dir.path(), None, &open_files, &request);
        let impact = payload.impact.expect("refactor task should include impact");

        assert!(impact.enabled);
        assert_eq!(impact.reason, "intent_refactor");
        assert!(!impact.target_symbols.is_empty());
        assert!(impact
            .target_symbols
            .iter()
            .any(|symbol| symbol.name == "helperName"));
        assert!(impact
            .impacted_files
            .iter()
            .any(|file| file.path == "helper.ts"));
        assert!(impact.impacted_files.len() <= 6);
        assert!(impact
            .recommended_next_steps
            .iter()
            .any(|step| step.contains("edit_impact")));
    }

    #[test]
    fn context_pack_docs_intent_does_not_include_impact() {
        let temp_dir = tempfile::tempdir().unwrap();
        let service = test_language_service(temp_dir.path());
        std::fs::write(
            temp_dir.path().join("helper.ts"),
            "export function helperName() { return 'ok'; }",
        )
        .unwrap();
        service.index_file("helper.ts").unwrap();

        let request = ContextPackRequest {
            id: "ctx-docs".to_string(),
            query: "explain helperName".to_string(),
            queries: Vec::new(),
            intent: Some("docs".to_string()),
            max_results: Some(4),
            include_tests: Some(false),
            include_docs: Some(false),
            include_memory: Some(false),
            include_project_index_min: Some(false),
        };
        let open_files: Vec<String> = Vec::new();
        let payload = build_context_pack(temp_dir.path(), None, &open_files, &request);

        assert!(payload.impact.is_none());
    }

    #[test]
    fn context_pack_returns_indexed_markdown_sections_as_related_docs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let service = test_language_service(temp_dir.path());
        std::fs::create_dir_all(temp_dir.path().join("docs")).unwrap();
        std::fs::write(
            temp_dir.path().join("docs/architecture.md"),
            "# Architecture\n\nIntro\n\n## Fast Context Architecture\n\nUse symbols first.\n\n## Other Notes\n\nLater.\n",
        )
        .unwrap();
        service.index_file("docs/architecture.md").unwrap();

        let request = ContextPackRequest {
            id: "ctx-doc-sections".to_string(),
            query: "fast context architecture".to_string(),
            queries: Vec::new(),
            intent: Some("docs".to_string()),
            max_results: Some(4),
            include_tests: Some(false),
            include_docs: Some(true),
            include_memory: Some(false),
            include_project_index_min: Some(false),
        };
        let open_files: Vec<String> = Vec::new();
        let payload = build_context_pack(temp_dir.path(), None, &open_files, &request);

        let doc = payload
            .related_docs
            .iter()
            .find(|doc| doc.path == "docs/architecture.md")
            .expect("indexed markdown doc should be returned");
        assert!(doc.reason.contains("Fast Context Architecture"));
        assert_eq!(doc.suggested_ranges[0].start_line, 5);
        assert_eq!(doc.suggested_ranges[0].end_line, 8);
        assert!(doc
            .why
            .iter()
            .any(|reason| reason == "matched indexed Markdown heading"));
    }

    #[test]
    fn context_impact_confidence_drops_when_index_health_is_stale() {
        let temp_dir = tempfile::tempdir().unwrap();
        let service = test_language_service(temp_dir.path());
        std::fs::write(
            temp_dir.path().join("helper.ts"),
            "export function helperName() { return 'ok'; }",
        )
        .unwrap();
        service.index_file("helper.ts").unwrap();

        let primary = vec![ContextFileResult {
            path: "helper.ts".to_string(),
            score: 90,
            reason: "test".to_string(),
            why: Vec::new(),
            suggested_ranges: vec![ContextRange {
                start_line: 1,
                end_line: 1,
            }],
        }];
        let enriched_files = vec![enrich_context_file(&service, &primary[0]).unwrap()];
        let health = IndexHealthSnapshot {
            status: IndexHealthStatus::Stale,
            indexed_files: 1,
            supported_files: 1,
            stale_files: 1,
            missing_files: 0,
            orphaned_files: 0,
            queued_files: 1,
            active_workers: 0,
            symbol_count: 1,
            last_full_scan_ms: Some(1),
            last_incremental_update_ms: None,
            current_file: None,
            message: "stale".to_string(),
            timings: crate::language_service::IndexTimingSnapshot::default(),
            discovery: crate::language_service::IndexDiscoverySnapshot::default(),
        };
        let queries = vec!["refactor helperName".to_string()];

        let impact = build_context_impact(
            temp_dir.path(),
            &service,
            &queries,
            Some("refactor"),
            &primary,
            &enriched_files,
            Some(&health),
            4,
        )
        .expect("refactor task should include impact");

        assert_eq!(impact.confidence, "medium");
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

        let project_context = build_project_context(temp_dir.path(), true, None, &[], &[], None);

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
            Some("workspace_scan_fallback")
        );
        assert_eq!(
            project_context.source.as_deref(),
            Some("workspace_scan_fallback")
        );
        assert_eq!(project_context.confidence.as_deref(), Some("low"));
        assert_eq!(
            project_context.reason.as_deref(),
            Some("symbol_index_unavailable")
        );
        assert_eq!(
            project_context.project_index_path.as_deref(),
            Some(".zblade/context/project_index.md")
        );

        let project_context = build_project_context(temp_dir.path(), false, None, &[], &[], None);
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
        let project_context = build_project_context(temp_dir.path(), true, None, &[], &[], None);

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

        let project_context = build_project_context(temp_dir.path(), true, None, &[], &[], None);

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
    fn project_context_reports_structured_workspace_shape_from_index_health() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("src/components")).unwrap();
        std::fs::create_dir_all(temp_dir.path().join("src-tauri/src")).unwrap();
        std::fs::write(
            temp_dir.path().join("src/main.tsx"),
            "export function main() {}",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("src/components/App.tsx"),
            "export function App() {}",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("src-tauri/src/main.rs"),
            "fn main() {}",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("README.md"),
            "# ignored by source scan",
        )
        .unwrap();

        let health = IndexHealthSnapshot {
            status: IndexHealthStatus::Fresh,
            indexed_files: 3,
            supported_files: 3,
            stale_files: 0,
            missing_files: 0,
            orphaned_files: 0,
            queued_files: 0,
            active_workers: 0,
            symbol_count: 12,
            last_full_scan_ms: Some(4),
            last_incremental_update_ms: None,
            current_file: None,
            message: "ready".to_string(),
            timings: crate::language_service::IndexTimingSnapshot::default(),
            discovery: crate::language_service::IndexDiscoverySnapshot::default(),
        };
        let open_files = vec!["src/main.tsx".to_string()];

        let project_context = build_project_context(
            temp_dir.path(),
            false,
            Some("src/main.tsx"),
            &open_files,
            &[],
            Some(&health),
        );

        assert_eq!(project_context.source.as_deref(), Some("structured_index"));
        assert_eq!(project_context.confidence.as_deref(), Some("high"));
        assert_eq!(
            project_context.reason.as_deref(),
            Some("symbol_index_health_available")
        );
        assert_eq!(project_context.active_file.as_deref(), Some("src/main.tsx"));
        assert_eq!(project_context.open_files, open_files);
        assert_eq!(project_context.indexed_files, Some(3));
        assert_eq!(project_context.supported_files, Some(3));
        assert_eq!(project_context.symbol_count, Some(12));
        assert_eq!(
            project_context.index_health_status.as_deref(),
            Some("fresh")
        );
        assert!(project_context
            .languages
            .iter()
            .any(|item| item.name == "TypeScript" && item.files == 2));
        assert!(project_context
            .languages
            .iter()
            .any(|item| item.name == "Rust" && item.files == 1));
        assert!(project_context
            .top_directories
            .iter()
            .any(|item| item.path == "src/components" && item.files == 1));
        assert!(project_context
            .top_directories
            .iter()
            .any(|item| item.path == "src-tauri/src" && item.files == 1));
        assert!(project_context
            .likely_entry_points
            .contains(&"src/main.tsx".to_string()));
        assert!(project_context
            .likely_entry_points
            .contains(&"src-tauri/src/main.rs".to_string()));
    }

    #[test]
    fn project_context_degrades_to_workspace_scan_without_index_health() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        std::fs::write(temp_dir.path().join("src/lib.rs"), "pub fn run() {}").unwrap();

        let project_context = build_project_context(temp_dir.path(), false, None, &[], &[], None);

        assert_eq!(
            project_context.source.as_deref(),
            Some("workspace_scan_fallback")
        );
        assert_eq!(project_context.confidence.as_deref(), Some("low"));
        assert_eq!(
            project_context.reason.as_deref(),
            Some("symbol_index_unavailable")
        );
        assert_eq!(project_context.supported_files, Some(1));
        assert_eq!(project_context.indexed_files, None);
        assert_eq!(project_context.symbol_count, None);
        assert!(project_context
            .languages
            .iter()
            .any(|item| item.name == "Rust" && item.files == 1));
        assert!(project_context
            .likely_entry_points
            .contains(&"src/lib.rs".to_string()));
    }

    #[test]
    fn project_context_includes_agent_instructions_for_candidate_paths() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("src/ui")).unwrap();
        std::fs::write(temp_dir.path().join("AGENTS.md"), "Root workflow").unwrap();
        std::fs::write(temp_dir.path().join("src/AGENTS.md"), "Rust workflow").unwrap();
        std::fs::write(
            temp_dir.path().join("src/ui/Button.tsx"),
            "export const Button = null;\n",
        )
        .unwrap();

        let project_context = build_project_context(
            temp_dir.path(),
            false,
            None,
            &[],
            &["src/ui/Button.tsx".to_string()],
            None,
        );

        let instructions = project_context
            .agent_instructions
            .expect("agent instructions");
        assert!(instructions.contains("Root workflow"));
        assert!(instructions.contains("Rust workflow"));
        assert_eq!(
            project_context.agent_instruction_files,
            vec!["AGENTS.md", "src/AGENTS.md"]
        );
        assert!(!project_context.agent_instructions_truncated);
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

    #[test]
    fn symbol_ranges_convert_zero_based_lines_to_one_based() {
        let symbol = test_symbol("smallFn", 30, 35);
        let exact = exact_range_for_symbol(&symbol);
        assert_eq!(exact.start_line, 31);
        assert_eq!(exact.end_line, 36);

        let padded = range_for_symbol(&symbol);
        assert_eq!(padded.start_line, 11);
        assert_eq!(padded.end_line, 96);
    }

    #[test]
    fn padded_range_caps_oversized_symbols_and_keeps_symbol_start_inside() {
        let symbol = test_symbol("bigFn", 100, 500);
        let exact = exact_range_for_symbol(&symbol);
        assert_eq!(exact.start_line, 101);
        assert_eq!(exact.end_line, 501);

        let padded = range_for_symbol(&symbol);
        assert_eq!(padded.start_line, 81);
        assert_eq!(padded.end_line, 240);
        assert_eq!(
            padded.end_line - padded.start_line + 1,
            MAX_CONTEXT_RANGE_LINES
        );
        // The symbol's first line must survive the cap.
        assert!(padded.start_line <= exact.start_line && exact.start_line <= padded.end_line);
    }

    #[test]
    fn aggregation_applies_diminishing_returns_not_flat_sum() {
        let hits = vec![
            test_primary_hit(0.6, 0, "alpha", 10, 12),
            test_primary_hit(0.4, 0, "beta", 40, 42),
            test_primary_hit(0.2, 0, "gamma", 80, 82),
        ];
        let result =
            aggregate_primary_hits("src/lib.rs".to_string(), hits, 1, None, None, &[]).unwrap();
        // best + 0.25*second + 0.10*third = 0.6 + 0.1 + 0.02 = 0.72 -> 72.
        // A flat sum (1.2) would have clamped to 95.
        assert_eq!(result.score, 72);
        assert!(result
            .why
            .iter()
            .any(|line| line.contains("strongest evidence") && line.contains("alpha")));
        assert!(result
            .why
            .iter()
            .any(|line| line.contains("matched 1 of 1 query facet(s)")));

        // Hits beyond the third are ignored entirely.
        let hits = vec![
            test_primary_hit(0.6, 0, "alpha", 10, 12),
            test_primary_hit(0.4, 0, "beta", 40, 42),
            test_primary_hit(0.2, 0, "gamma", 80, 82),
            test_primary_hit(0.2, 0, "delta", 120, 122),
        ];
        let result =
            aggregate_primary_hits("src/lib.rs".to_string(), hits, 1, None, None, &[]).unwrap();
        assert_eq!(result.score, 72);
    }

    #[test]
    fn aggregation_confidence_floor_drops_boost_only_files() {
        // Every hit below the raw floor: the file must vanish entirely.
        let hits = vec![
            test_primary_hit(0.19, 0, "noise", 1, 2),
            test_primary_hit(0.05, 1, "alsoNoise", 9, 10),
        ];
        assert!(
            aggregate_primary_hits("src/lib.rs".to_string(), hits, 2, None, None, &[]).is_none()
        );

        // A sub-floor hit contributes nothing next to a real hit.
        let hits = vec![
            test_primary_hit(0.25, 0, "signal", 1, 3),
            test_primary_hit(0.19, 1, "noise", 9, 10),
        ];
        let result =
            aggregate_primary_hits("src/lib.rs".to_string(), hits, 2, None, None, &[]).unwrap();
        assert_eq!(result.score, 25);
        assert!(result
            .why
            .iter()
            .any(|line| line.contains("matched 1 of 2 query facet(s)")));
    }

    #[test]
    fn aggregation_rewards_distinct_facet_coverage_with_capped_bonus() {
        // Same raw scores as the diminishing-returns test but across three
        // DISTINCT facets: 0.72 + 2*0.05 = 0.82 -> 82.
        let hits = vec![
            test_primary_hit(0.6, 0, "alpha", 10, 12),
            test_primary_hit(0.4, 1, "beta", 40, 42),
            test_primary_hit(0.2, 2, "gamma", 80, 82),
        ];
        let result =
            aggregate_primary_hits("src/lib.rs".to_string(), hits, 3, None, None, &[]).unwrap();
        assert_eq!(result.score, 82);
        assert!(result
            .why
            .iter()
            .any(|line| line.contains("matched 3 of 3 query facet(s)")));

        // Bonus caps at +0.15: four distinct facets -> 0.72 + 0.15 = 0.87.
        let hits = vec![
            test_primary_hit(0.6, 0, "alpha", 10, 12),
            test_primary_hit(0.4, 1, "beta", 40, 42),
            test_primary_hit(0.2, 2, "gamma", 80, 82),
            test_primary_hit(0.2, 3, "delta", 120, 122),
        ];
        let result =
            aggregate_primary_hits("src/lib.rs".to_string(), hits, 4, None, None, &[]).unwrap();
        assert_eq!(result.score, 87);
    }

    #[test]
    fn aggregation_dedupes_hits_per_symbol_before_diminishing_returns() {
        // ONE symbol matched by two facets: it must fill only ONE
        // diminishing-returns slot (0.6, not 0.6 + 0.25*0.6) while still
        // covering both facets for the facet bonus: 0.6 + 0.05 = 0.65 -> 65.
        let hits = vec![
            test_primary_hit(0.6, 0, "alpha", 10, 12),
            test_primary_hit(0.6, 1, "alpha", 10, 12),
        ];
        let single_symbol =
            aggregate_primary_hits("src/lib.rs".to_string(), hits, 2, None, None, &[]).unwrap();
        assert_eq!(single_symbol.score, 65);
        assert!(single_symbol
            .why
            .iter()
            .any(|line| line.contains("matched 2 of 2 query facet(s)")));

        // TWO distinct symbols with the same raw scores across the same two
        // facets: 0.6 + 0.25*0.6 + 0.05 = 0.80 -> 80. Distinct evidence must
        // score strictly higher than one symbol counted twice.
        let hits = vec![
            test_primary_hit(0.6, 0, "alpha", 10, 12),
            test_primary_hit(0.6, 1, "beta", 40, 42),
        ];
        let distinct_symbols =
            aggregate_primary_hits("src/lib.rs".to_string(), hits, 2, None, None, &[]).unwrap();
        assert_eq!(distinct_symbols.score, 80);
        assert!(single_symbol.score < distinct_symbols.score);
    }

    #[test]
    fn aggregation_better_second_hit_replaces_reason_and_ranges() {
        // Masking-bug regression: the old flat `+4` was applied before the
        // score comparison, so a stronger later hit could fail to replace the
        // reason and ranges. Buffered aggregation must pick the best raw hit.
        let hits = vec![
            test_primary_hit(0.5, 0, "weakSymbol", 10, 12),
            test_primary_hit(0.9, 1, "strongSymbol", 200, 204),
        ];
        let result =
            aggregate_primary_hits("src/lib.rs".to_string(), hits, 2, None, None, &[]).unwrap();
        assert!(result.reason.contains("strongSymbol"));
        // Exact matched-symbol range first, padded range second.
        assert_eq!(result.suggested_ranges[0].start_line, 201);
        assert_eq!(result.suggested_ranges[0].end_line, 205);
        assert!(result.suggested_ranges[1].start_line <= 201);
        assert!(result
            .why
            .iter()
            .any(|line| line.contains("strongest evidence") && line.contains("strongSymbol")));
        // 0.9 + 0.25*0.5 + 0.05 facet bonus = 1.075 -> clamped quantization 95.
        assert_eq!(result.score, 95);
    }

    #[test]
    fn aggregation_applies_editor_boosts_once_per_file() {
        let hits = vec![
            test_primary_hit(0.5, 0, "alpha", 10, 12),
            test_primary_hit(0.5, 0, "beta", 40, 42),
        ];
        let result = aggregate_primary_hits(
            "src/lib.rs".to_string(),
            hits,
            1,
            None,
            Some("src/lib.rs"),
            &[],
        )
        .unwrap();
        // 0.5 + 0.125 = 0.625 -> 63, plus ONE active-file boost of 8 -> 71.
        assert_eq!(result.score, 71);
        assert_eq!(
            result
                .why
                .iter()
                .filter(|line| line.as_str() == "active file boost applied")
                .count(),
            1
        );
    }

    #[test]
    fn primary_lane_retrieval_is_boost_free_so_editor_boosts_apply_exactly_once() {
        // Regression: the primary lane used contextual retrieval, which bakes
        // active/open-file boosts into every RAW hit score, and then the
        // aggregator applied the same boosts once per file — double-counting.
        // With non-contextual retrieval, two identical symbols in different
        // directories must differ by EXACTLY the once-per-file +8 active-file
        // boost.
        let temp_dir = tempfile::tempdir().unwrap();
        let service = test_language_service(temp_dir.path());
        std::fs::create_dir_all(temp_dir.path().join("src/alpha")).unwrap();
        std::fs::create_dir_all(temp_dir.path().join("src/beta")).unwrap();
        std::fs::write(
            temp_dir.path().join("src/alpha/engine.rs"),
            "pub const WARP_FACTOR: u32 = 9;\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("src/beta/engine.rs"),
            "pub const WARP_FACTOR: u32 = 9;\n",
        )
        .unwrap();
        service.index_file("src/alpha/engine.rs").unwrap();
        service.index_file("src/beta/engine.rs").unwrap();

        let queries = vec!["factor warp".to_string()];
        let items = collect_primary_files_for_queries(
            &service,
            &queries,
            None,
            5,
            Some("src/alpha/engine.rs"),
            &[],
        );
        let active = items
            .iter()
            .find(|item| item.path == "src/alpha/engine.rs")
            .expect("active file ranked");
        let other = items
            .iter()
            .find(|item| item.path == "src/beta/engine.rs")
            .expect("non-active file ranked");
        // Identical raw evidence: the only difference is the single
        // aggregation-time active-file boost. Contextual retrieval would have
        // inflated the active file's raw score too (+0.35/+0.12), widening
        // the gap well beyond 8.
        assert_eq!(active.score, other.score + 8);
        assert!(active.score < 100, "delta must not be masked by the cap");
    }

    #[test]
    fn clustering_unions_nearby_ranges_and_respects_the_gap() {
        let (ranges, clipped) =
            cluster_context_ranges(vec![cr(10, 20), cr(25, 30)], MAX_CONTEXT_RANGE_LINES);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 10);
        assert_eq!(ranges[0].end_line, 30);
        assert!(!clipped);

        // Gap of exactly RANGE_CLUSTER_GAP_LINES lines still unions...
        let (ranges, _) =
            cluster_context_ranges(vec![cr(10, 20), cr(29, 35)], MAX_CONTEXT_RANGE_LINES);
        assert_eq!(ranges.len(), 1);
        // ...one more line of gap stays disjoint.
        let (ranges, clipped) =
            cluster_context_ranges(vec![cr(10, 20), cr(30, 35)], MAX_CONTEXT_RANGE_LINES);
        assert_eq!(ranges.len(), 2);
        assert!(!clipped);
    }

    #[test]
    fn clustering_sorts_disjoint_ranges_by_start_line() {
        let (ranges, clipped) =
            cluster_context_ranges(vec![cr(100, 110), cr(10, 20)], MAX_CONTEXT_RANGE_LINES);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start_line, 10);
        assert_eq!(ranges[1].start_line, 100);
        assert!(!clipped);
    }

    #[test]
    fn clustering_caps_at_three_disjoint_ranges_and_flags_dropping() {
        let (ranges, clipped) = cluster_context_ranges(
            vec![cr(1, 5), cr(100, 105), cr(200, 205), cr(300, 305)],
            MAX_CONTEXT_RANGE_LINES,
        );
        assert_eq!(ranges.len(), MAX_CONTEXT_RANGES);
        // The three highest-priority ranges survive.
        assert_eq!(ranges[0].start_line, 1);
        assert_eq!(ranges[1].start_line, 100);
        assert_eq!(ranges[2].start_line, 200);
        assert!(clipped);
    }

    #[test]
    fn clustering_clips_to_line_caps_and_flags_it() {
        let (ranges, clipped) = cluster_context_ranges(vec![cr(1, 400)], MAX_CONTEXT_RANGE_LINES);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 1);
        assert_eq!(ranges[0].end_line, 160);
        assert!(clipped);

        // Markdown cap is tighter.
        let (ranges, clipped) = cluster_context_ranges(vec![cr(1, 400)], MAX_DOC_RANGE_LINES);
        assert_eq!(ranges[0].end_line, 120);
        assert!(clipped);
    }

    #[test]
    fn clustering_keeps_highest_priority_evidence_inside_clipped_range() {
        // The exact matched-symbol range comes first (priority order); a huge
        // padded range would otherwise clip it out from the top.
        let (ranges, clipped) =
            cluster_context_ranges(vec![cr(300, 305), cr(1, 400)], MAX_CONTEXT_RANGE_LINES);
        assert_eq!(ranges.len(), 1);
        assert!(clipped);
        assert_eq!(ranges[0].end_line - ranges[0].start_line + 1, 160);
        // The anchor evidence lines 300..305 survive the clipping.
        assert!(ranges[0].start_line <= 300);
        assert!(ranges[0].end_line >= 305);
    }

    #[test]
    fn clustering_merges_clusters_bridged_by_a_later_range() {
        let (ranges, clipped) = cluster_context_ranges(
            vec![cr(1, 10), cr(30, 40), cr(12, 28)],
            MAX_CONTEXT_RANGE_LINES,
        );
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 1);
        assert_eq!(ranges[0].end_line, 40);
        assert!(!clipped);
    }

    #[test]
    fn clustering_skips_invalid_ranges_without_flagging() {
        let (ranges, clipped) =
            cluster_context_ranges(vec![cr(20, 10), cr(0, 5)], MAX_CONTEXT_RANGE_LINES);
        assert!(ranges.is_empty());
        assert!(!clipped);
    }

    #[test]
    fn fallback_path_matches_use_capped_default_range() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        std::fs::write(
            temp_dir.path().join("src/context_helper.rs"),
            "pub fn context_helper() {}\n",
        )
        .unwrap();

        let queries = vec!["context helper".to_string()];
        let items = collect_fallback_path_matches_for_queries(temp_dir.path(), &queries, 4);
        let item = items
            .iter()
            .find(|item| item.path == "src/context_helper.rs")
            .expect("fallback path match");
        assert_eq!(item.suggested_ranges[0].start_line, 1);
        assert_eq!(item.suggested_ranges[0].end_line, MAX_CONTEXT_RANGE_LINES);
    }

    #[test]
    fn doc_section_range_caps_markdown_and_stops_at_peer_or_parent_heading() {
        let temp_dir = tempfile::tempdir().unwrap();
        let service = test_language_service(temp_dir.path());
        std::fs::create_dir_all(temp_dir.path().join("docs")).unwrap();

        // Peer heading is 151 lines away, so the Markdown cap wins.
        let mut content = String::from("## Alpha\n");
        for index in 0..150 {
            content.push_str(&format!("alpha body line {}\n", index));
        }
        content.push_str("## Beta\nbeta body\n");
        std::fs::write(temp_dir.path().join("docs/long.md"), content).unwrap();
        service.index_file("docs/long.md").unwrap();

        let symbols = service.get_file_symbols("docs/long.md").unwrap();
        let alpha = symbols
            .iter()
            .find(|symbol| symbol.symbol_type == SymbolType::Heading && symbol.name == "Alpha")
            .expect("Alpha heading indexed");
        let range = doc_section_range(&service, alpha);
        assert_eq!(range.start_line, 1);
        // Inclusive span: lines 1..=120 is exactly MAX_DOC_RANGE_LINES lines.
        assert_eq!(range.end_line, 120);
        assert_eq!(
            range.end_line - range.start_line + 1,
            MAX_DOC_RANGE_LINES,
            "doc section span must never exceed the doc range cap"
        );

        // A parent heading stops a subsection before the cap matters.
        std::fs::write(
            temp_dir.path().join("docs/short.md"),
            "### Sub\nsub body\nmore\n## Parent\nparent body\n",
        )
        .unwrap();
        service.index_file("docs/short.md").unwrap();
        let symbols = service.get_file_symbols("docs/short.md").unwrap();
        let sub = symbols
            .iter()
            .find(|symbol| symbol.symbol_type == SymbolType::Heading && symbol.name == "Sub")
            .expect("Sub heading indexed");
        let range = doc_section_range(&service, sub);
        assert_eq!(range.start_line, 1);
        assert_eq!(range.end_line, 3);
    }
}
