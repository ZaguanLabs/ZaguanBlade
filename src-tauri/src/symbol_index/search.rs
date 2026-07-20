//! Symbol search functionality
//!
//! Provides structured queries and result types for searching
//! the symbol index.

use std::collections::HashSet;

use super::store::{SymbolStore, SymbolStoreError};
use crate::tree_sitter::{Symbol, SymbolType};
use serde::{Deserialize, Serialize};

/// Structured search query
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    /// Text query (name, fuzzy match)
    pub text: Option<String>,
    /// Filter by file path
    pub file_path: Option<String>,
    /// Filter by file path glob or substring
    pub file_pattern: Option<String>,
    /// Filter by symbol name glob or substring
    pub name_pattern: Option<String>,
    /// Filter by qualified-name glob or substring
    pub qualified_name_pattern: Option<String>,
    /// Filter by symbol types
    pub symbol_types: Option<Vec<SymbolType>>,
    /// Maximum results to return
    pub limit: Option<usize>,
    /// Include symbols from subdirectories
    pub recursive: bool,
    pub active_file: Option<String>,
    pub preferred_files: Vec<String>,
    pub preferred_directories: Vec<String>,
    /// Include deterministic score components in returned results.
    pub explain: bool,
}

impl SearchQuery {
    /// Create a simple text search query
    pub fn text(query: &str) -> Self {
        Self {
            text: Some(query.to_string()),
            limit: Some(50),
            recursive: true,
            ..Default::default()
        }
    }

    /// Add a limit to the query
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Add file filter
    pub fn with_file(mut self, file_path: &str) -> Self {
        self.file_path = Some(file_path.to_string());
        self
    }

    /// Add file-pattern filter
    pub fn with_file_pattern(mut self, pattern: &str) -> Self {
        self.file_pattern = Some(pattern.to_string());
        self
    }

    /// Add symbol-name-pattern filter
    pub fn with_name_pattern(mut self, pattern: &str) -> Self {
        self.name_pattern = Some(pattern.to_string());
        self
    }

    /// Add qualified-name-pattern filter
    pub fn with_qualified_name_pattern(mut self, pattern: &str) -> Self {
        self.qualified_name_pattern = Some(pattern.to_string());
        self
    }

    /// Add type filter
    pub fn with_types(mut self, types: Vec<SymbolType>) -> Self {
        self.symbol_types = Some(types);
        self
    }

    pub fn with_active_file(mut self, file_path: &str) -> Self {
        self.active_file = Some(file_path.to_string());
        self
    }

    pub fn with_preferred_files(mut self, file_paths: Vec<String>) -> Self {
        self.preferred_files = file_paths;
        self
    }

    pub fn with_preferred_directories(mut self, directories: Vec<String>) -> Self {
        self.preferred_directories = directories;
        self
    }

    pub fn with_explain(mut self, explain: bool) -> Self {
        self.explain = explain;
        self
    }
}

/// Confidence tier derived from weighted cross-field query coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchConfidence {
    High,
    Medium,
    Low,
}

/// Explainable components of a deterministic symbol score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub matched_tokens: Vec<String>,
    pub missing_tokens: Vec<String>,
    pub matched_fields: Vec<String>,
    pub token_coverage: f32,
    pub phrase_match: bool,
    pub exact_identifier: bool,
    pub confidence: SearchConfidence,
}

/// Search result with relevance score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The matched symbol
    pub symbol: Symbol,
    /// Relevance score (0.0 to 1.0)
    pub score: f32,
    /// Matched portions of the name (for highlighting)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<(usize, usize)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_breakdown: Option<ScoreBreakdown>,
}

impl SearchResult {
    /// Create a search result with default score
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            score: 1.0,
            highlights: vec![],
            score_breakdown: None,
        }
    }

    /// Create with a specific score
    pub fn with_score(symbol: Symbol, score: f32) -> Self {
        Self {
            symbol,
            score,
            highlights: vec![],
            score_breakdown: None,
        }
    }

    fn with_breakdown(symbol: Symbol, score: f32, score_breakdown: ScoreBreakdown) -> Self {
        Self {
            symbol,
            score,
            highlights: Vec::new(),
            score_breakdown: Some(score_breakdown),
        }
    }
}

/// Execute a search query against the symbol store
pub fn execute_search(
    store: &SymbolStore,
    query: &SearchQuery,
) -> Result<Vec<SearchResult>, SymbolStoreError> {
    let limit = query.limit.unwrap_or(50);

    // Simple case: get symbols in a specific file
    if query.text.is_none() && query.file_path.is_some() {
        let symbols = store.get_symbols_in_file(query.file_path.as_ref().unwrap())?;
        let results = filter_symbols(symbols, query)
            .into_iter()
            .take(limit)
            .map(SearchResult::new)
            .collect();
        return Ok(results);
    }

    // Search by text
    if let Some(ref text) = query.text {
        let query_model = QueryModel::parse(text);
        if query_model.is_empty() {
            return Ok(Vec::new());
        }
        let symbols = search_symbol_candidates(
            store,
            text,
            &query_model,
            search_candidate_limit(query, limit),
        )?;
        let mut results: Vec<SearchResult> = symbols
            .into_iter()
            .filter_map(|symbol| {
                let (score, breakdown) = score_symbol(&symbol, &query_model)?;
                Some(if query.explain {
                    SearchResult::with_breakdown(symbol, score, breakdown)
                } else {
                    SearchResult::with_score(symbol, score)
                })
            })
            .collect();

        apply_result_filters(&mut results, query);

        apply_contextual_boosts(&mut results, query);

        // Sort by score and limit
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.symbol.qualified_name.cmp(&b.symbol.qualified_name))
                .then_with(|| a.symbol.file_path.cmp(&b.symbol.file_path))
                .then_with(|| a.symbol.id.cmp(&b.symbol.id))
        });
        results.truncate(limit);

        return Ok(results);
    }

    // Filter by type only
    if let Some(ref types) = query.symbol_types {
        let mut all_results = Vec::new();
        for sym_type in types {
            let symbols = store.get_symbols_by_type(*sym_type, limit)?;
            all_results.extend(symbols.into_iter().map(SearchResult::new));
        }
        apply_result_filters(&mut all_results, query);
        all_results.truncate(limit);
        return Ok(all_results);
    }

    Ok(vec![])
}

fn apply_result_filters(results: &mut Vec<SearchResult>, query: &SearchQuery) {
    results.retain(|result| symbol_matches_query_filters(&result.symbol, query));
}

fn filter_symbols(symbols: Vec<Symbol>, query: &SearchQuery) -> Vec<Symbol> {
    symbols
        .into_iter()
        .filter(|symbol| symbol_matches_query_filters(symbol, query))
        .collect()
}

pub(crate) fn symbol_matches_query_filters(symbol: &Symbol, query: &SearchQuery) -> bool {
    if !query
        .symbol_types
        .as_deref()
        .map(|types| types.is_empty() || types.contains(&symbol.symbol_type))
        .unwrap_or(true)
    {
        return false;
    }

    if let Some(ref file_path) = query.file_path {
        let matches_file = if query.recursive {
            symbol.file_path.starts_with(file_path)
        } else {
            &symbol.file_path == file_path
        };
        if !matches_file {
            return false;
        }
    }

    if !query
        .file_pattern
        .as_deref()
        .map(|pattern| path_or_text_matches_pattern(&symbol.file_path, pattern))
        .unwrap_or(true)
    {
        return false;
    }

    if !query
        .name_pattern
        .as_deref()
        .map(|pattern| path_or_text_matches_pattern(&symbol.name, pattern))
        .unwrap_or(true)
    {
        return false;
    }

    query
        .qualified_name_pattern
        .as_deref()
        .map(|pattern| path_or_text_matches_pattern(&symbol.qualified_name, pattern))
        .unwrap_or(true)
}

fn path_or_text_matches_pattern(value: &str, pattern: &str) -> bool {
    pattern
        .split(',')
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .any(|pattern| single_pattern_matches(value, pattern))
}

fn single_pattern_matches(value: &str, pattern: &str) -> bool {
    let pattern = pattern.trim_start_matches('/');
    if pattern.contains(['*', '?', '[', ']']) {
        return glob::Pattern::new(pattern)
            .map(|compiled| {
                compiled.matches_with(
                    value,
                    glob::MatchOptions {
                        case_sensitive: false,
                        require_literal_separator: false,
                        require_literal_leading_dot: false,
                    },
                )
            })
            .unwrap_or(false);
    }

    value.to_lowercase().contains(&pattern.to_lowercase())
}

fn search_symbol_candidates(
    store: &SymbolStore,
    text: &str,
    query: &QueryModel,
    candidate_limit: usize,
) -> Result<Vec<Symbol>, SymbolStoreError> {
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();

    // Whole-query identifier/signature/docstring lane. Always union it with
    // FTS: a saturated FTS window must not prevent stronger LIKE candidates
    // from reaching the scorer.
    for symbol in store.search_by_name_like(text, candidate_limit)? {
        if seen.insert(symbol.id.clone()) {
            symbols.push(symbol);
        }
    }

    // Balanced identifier lanes keep one common query term from filling the
    // entire candidate window before a concise entry point is considered.
    if query.len() > 1 {
        let per_term_limit = candidate_limit.div_ceil(query.len()).max(8);
        for term in query.tokens() {
            for symbol in store.search_by_name_like(term, per_term_limit)? {
                if seen.insert(symbol.id.clone()) {
                    symbols.push(symbol);
                }
            }
        }
    }

    for symbol in store.search_by_name(text, candidate_limit)? {
        if seen.insert(symbol.id.clone()) {
            symbols.push(symbol);
        }
    }

    Ok(symbols)
}

fn search_candidate_limit(query: &SearchQuery, result_limit: usize) -> usize {
    let base = result_limit.saturating_mul(4).max(result_limit).max(20);
    if query_has_post_candidate_filters(query) {
        result_limit
            .saturating_mul(24)
            .max(base)
            .max(100)
            .min(1_000)
    } else {
        base
    }
}

fn query_has_post_candidate_filters(query: &SearchQuery) -> bool {
    query.file_path.is_some()
        || query.file_pattern.is_some()
        || query.name_pattern.is_some()
        || query.qualified_name_pattern.is_some()
        || query
            .symbol_types
            .as_deref()
            .is_some_and(|types| !types.is_empty())
}

fn apply_contextual_boosts(results: &mut [SearchResult], query: &SearchQuery) {
    if query.active_file.is_none()
        && query.preferred_files.is_empty()
        && query.preferred_directories.is_empty()
    {
        return;
    }

    let active_directory = query.active_file.as_deref().and_then(parent_directory);

    for result in results.iter_mut() {
        if query
            .active_file
            .as_ref()
            .is_some_and(|active| active == &result.symbol.file_path)
        {
            result.score += 0.35;
        } else if query
            .preferred_files
            .iter()
            .any(|path| path == &result.symbol.file_path)
        {
            result.score += 0.15;
        }

        if active_directory.is_some_and(|dir| same_directory(dir, &result.symbol.file_path)) {
            result.score += 0.12;
        } else if query
            .preferred_directories
            .iter()
            .any(|dir| same_directory(dir, &result.symbol.file_path))
        {
            result.score += 0.07;
        }

        if result
            .symbol
            .qualified_name
            .eq_ignore_ascii_case(result.symbol.name.as_str())
        {
            result.score += 0.02;
        }
    }
}

pub fn collect_preferred_directories(
    active_file: Option<&str>,
    preferred_files: &[String],
) -> Vec<String> {
    let mut directories = Vec::new();
    let mut seen = HashSet::new();

    if let Some(dir) = active_file.and_then(parent_directory) {
        if seen.insert(dir.to_string()) {
            directories.push(dir.to_string());
        }
    }

    for path in preferred_files {
        if let Some(dir) = parent_directory(path) {
            if seen.insert(dir.to_string()) {
                directories.push(dir.to_string());
            }
        }
    }

    directories
}

fn parent_directory(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(dir, _)| dir)
}

fn same_directory(directory: &str, file_path: &str) -> bool {
    parent_directory(file_path).is_some_and(|candidate| candidate == directory)
}

/// Parsed query shared by balanced candidate retrieval and final ranking.
#[derive(Debug, Clone)]
struct QueryModel {
    normalized_phrase: String,
    terms: Vec<QueryTerm>,
    total_weight: f32,
}

#[derive(Debug, Clone)]
struct QueryTerm {
    text: String,
    weight: f32,
}

impl QueryModel {
    fn parse(query: &str) -> Self {
        let mut seen = HashSet::new();
        let terms = text_tokens(query)
            .into_iter()
            .filter(|term| term.chars().count() >= 2)
            .filter(|term| seen.insert(term.clone()))
            .map(|text| QueryTerm {
                weight: query_token_weight(&text),
                text,
            })
            .collect::<Vec<_>>();
        let total_weight = terms.iter().map(|term| term.weight).sum();
        let normalized_phrase = terms
            .iter()
            .map(|term| term.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Self {
            normalized_phrase,
            terms,
            total_weight,
        }
    }

    fn tokens(&self) -> impl Iterator<Item = &str> {
        self.terms.iter().map(|term| term.text.as_str())
    }

    fn len(&self) -> usize {
        self.terms.len()
    }

    fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

struct SearchField {
    name: &'static str,
    weight: f32,
    normalized: String,
    tokens: Vec<String>,
}

impl SearchField {
    fn new(name: &'static str, weight: f32, text: &str) -> Self {
        let tokens = text_tokens(text);
        let normalized = tokens.join(" ");
        Self {
            name,
            weight,
            normalized,
            tokens,
        }
    }
}

struct QueryEvaluation {
    matched_tokens: Vec<String>,
    missing_tokens: Vec<String>,
    matched_fields: Vec<String>,
    coverage: f32,
    field_quality: f32,
}

fn score_symbol(symbol: &Symbol, query: &QueryModel) -> Option<(f32, ScoreBreakdown)> {
    if query.is_empty() {
        return None;
    }
    let fields = [
        SearchField::new("name", 1.0, &symbol.name),
        SearchField::new("qualified_name", 0.88, &symbol.qualified_name),
        SearchField::new(
            "signature",
            0.62,
            symbol.signature.as_deref().unwrap_or_default(),
        ),
        SearchField::new(
            "docstring",
            0.45,
            symbol.docstring.as_deref().unwrap_or_default(),
        ),
        SearchField::new("path", 0.30, &symbol.file_path),
    ];
    let exact_identifier = fields[0].normalized == query.normalized_phrase
        || fields[1].normalized == query.normalized_phrase;
    let phrase_match = fields
        .iter()
        .any(|field| field.normalized.contains(&query.normalized_phrase));
    let evaluation = evaluate_query(query, &fields);
    let exploratory_name_match = query.len() >= 4
        && is_entry_point_kind(symbol.symbol_type)
        && has_distinctive_exact_name_match(query, &fields[..2]);
    let partial_short_only = query.len() >= 2
        && !evaluation
            .matched_tokens
            .iter()
            .any(|token| token.chars().count() >= 3)
        && !evaluation.missing_tokens.is_empty();
    if evaluation.matched_tokens.is_empty()
        || partial_short_only
        || (query.len() >= 2 && evaluation.coverage + f32::EPSILON < 0.5 && !exploratory_name_match)
    {
        return None;
    }

    let ordered = fields
        .iter()
        .any(|field| contains_ordered_tokens(&field.tokens, query));
    let confidence = if exact_identifier || phrase_match || evaluation.coverage >= 0.75 {
        SearchConfidence::High
    } else if evaluation.coverage >= 0.5 {
        SearchConfidence::Medium
    } else {
        SearchConfidence::Low
    };
    let raw_score = evaluation.coverage * 58.0
        + evaluation.field_quality * 30.0
        + if exact_identifier { 42.0 } else { 0.0 }
        + if phrase_match { 24.0 } else { 0.0 }
        + if ordered && query.len() > 1 { 6.0 } else { 0.0 }
        + if exploratory_name_match { 8.0 } else { 0.0 };
    let path_only_multiplier =
        if evaluation.matched_fields.len() == 1 && evaluation.matched_fields[0] == "path" {
            0.55
        } else {
            1.0
        };
    // Keep Blade's established normalized score contract while preserving
    // Scout's relative weighting and deterministic breakdown.
    let score = (raw_score / 160.0
        * symbol_type_relevance_multiplier(symbol.symbol_type)
        * path_only_multiplier)
        .min(1.0);
    Some((
        score,
        ScoreBreakdown {
            matched_tokens: evaluation.matched_tokens,
            missing_tokens: evaluation.missing_tokens,
            matched_fields: evaluation.matched_fields,
            token_coverage: evaluation.coverage,
            phrase_match,
            exact_identifier,
            confidence,
        },
    ))
}

/// Recompute the shared lexical score explanation for a returned symbol.
/// Contextual active-file boosts remain represented by the result's final
/// `score`, while this breakdown explains deterministic query matching.
pub fn explain_symbol_score(symbol: &Symbol, query: &str) -> Option<ScoreBreakdown> {
    let query = QueryModel::parse(query);
    score_symbol(symbol, &query).map(|(_, breakdown)| breakdown)
}

pub(crate) fn score_symbol_query(
    symbol: &Symbol,
    query: &str,
) -> Option<(f32, ScoreBreakdown)> {
    score_symbol(symbol, &QueryModel::parse(query))
}

fn evaluate_query(query: &QueryModel, fields: &[SearchField]) -> QueryEvaluation {
    let mut matched_tokens = Vec::new();
    let mut missing_tokens = Vec::new();
    let mut matched_fields = Vec::new();
    let mut matched_weight = 0.0;
    let mut weighted_quality = 0.0;

    for term in &query.terms {
        let best = fields
            .iter()
            .filter_map(|field| {
                token_match_quality(&term.text, &field.tokens)
                    .map(|quality| (field, quality * field.weight))
            })
            .max_by(|left, right| left.1.total_cmp(&right.1));
        if let Some((field, quality)) = best {
            matched_weight += term.weight;
            weighted_quality += term.weight * quality;
            matched_tokens.push(term.text.clone());
            if !matched_fields.iter().any(|name| name == field.name) {
                matched_fields.push(field.name.to_string());
            }
        } else {
            missing_tokens.push(term.text.clone());
        }
    }
    let coverage = if query.total_weight > 0.0 {
        matched_weight / query.total_weight
    } else {
        0.0
    };
    let field_quality = if query.total_weight > 0.0 {
        weighted_quality / query.total_weight
    } else {
        0.0
    };
    QueryEvaluation {
        matched_tokens,
        missing_tokens,
        matched_fields,
        coverage,
        field_quality,
    }
}

fn token_match_quality(query: &str, candidate: &[String]) -> Option<f32> {
    if candidate.iter().any(|token| token == query) {
        return Some(1.0);
    }
    if query.chars().count() >= 3
        && candidate.iter().any(|token| {
            token.chars().count() >= 3 && (token.starts_with(query) || query.starts_with(token))
        })
    {
        return Some(0.78);
    }
    if query.chars().count() >= 4 && candidate.iter().any(|token| token.contains(query)) {
        return Some(0.62);
    }
    None
}

fn contains_ordered_tokens(candidate: &[String], query: &QueryModel) -> bool {
    let mut next = 0;
    for token in candidate {
        if query.terms.get(next).is_some_and(|term| {
            token_match_quality(&term.text, std::slice::from_ref(token)).is_some()
        }) {
            next += 1;
            if next == query.terms.len() {
                return true;
            }
        }
    }
    false
}

fn has_distinctive_exact_name_match(query: &QueryModel, name_fields: &[SearchField]) -> bool {
    query.terms.iter().any(|term| {
        term.weight >= 1.0
            && name_fields
                .iter()
                .any(|field| field.tokens.iter().any(|token| token == &term.text))
    })
}

const fn is_entry_point_kind(kind: SymbolType) -> bool {
    matches!(
        kind,
        SymbolType::Function
            | SymbolType::Method
            | SymbolType::Class
            | SymbolType::Struct
            | SymbolType::Interface
            | SymbolType::Trait
    )
}

fn symbol_type_relevance_multiplier(symbol_type: SymbolType) -> f32 {
    match symbol_type {
        SymbolType::Function | SymbolType::Method => 1.08,
        SymbolType::Class
        | SymbolType::Struct
        | SymbolType::Interface
        | SymbolType::Type
        | SymbolType::Enum
        | SymbolType::Trait => 1.06,
        SymbolType::CssSelector
        | SymbolType::CssCustomProperty
        | SymbolType::CssKeyframes
        | SymbolType::CssAtRule
        | SymbolType::CssLayer
        | SymbolType::CssFontFace => 1.04,
        SymbolType::Import | SymbolType::Export | SymbolType::Module | SymbolType::Heading => 0.92,
        _ => 1.0,
    }
}

/// Calculate relevance score between query and symbol name
#[cfg(test)]
fn calculate_relevance(name: &str, query: &str) -> f32 {
    let name_lower = name.to_lowercase();
    let query_lower = query.to_lowercase();

    // Exact match
    if name_lower == query_lower {
        return 1.0;
    }

    // Prefix match — require at least 3 characters to avoid short-fragment noise
    if query_lower.len() >= 3 && name_lower.starts_with(&query_lower) {
        return 0.9;
    }

    // Contains match — require at least 3 characters to avoid short-fragment noise
    if query_lower.len() >= 3 && name_lower.contains(&query_lower) {
        // Score based on position (earlier is better)
        let pos = name_lower.find(&query_lower).unwrap_or(0) as f32;
        let len = name_lower.len() as f32;
        return 0.7 - (pos / len) * 0.3;
    }

    let token_score = calculate_token_relevance(name, query);
    if token_score > 0.0 {
        return token_score;
    }

    // Fuzzy match is intentionally not attempted via character-set overlap.
    // Short-fragment prefix matching and Jaccard-style char overlap can promote
    // unrelated results (e.g. token `i` satisfying `icons`) and manufacture
    // relevance from noise. Require token-level coverage or admit a truthful miss.
    0.0
}

#[cfg(test)]
fn calculate_token_relevance(name: &str, query: &str) -> f32 {
    let name_tokens = text_tokens(name);
    let query_tokens = text_tokens(query);
    if name_tokens.is_empty() || query_tokens.is_empty() {
        return 0.0;
    }

    // Weight query tokens by inverse generic-ness so that generic terms
    // (get, set, new, data, etc.) contribute less to coverage than
    // domain-specific terms.  This prevents a file with repeated weak terms
    // from winning solely through hit frequency.
    let weights: Vec<f32> = query_tokens.iter().map(|t| query_token_weight(t)).collect();
    let total_weight: f32 = weights.iter().sum();
    if total_weight <= 0.0 {
        return 0.0;
    }

    let mut used_name_tokens = vec![false; name_tokens.len()];
    let mut earned_weight = 0.0f32;
    let mut specific_match = false;
    for (query_token, &w) in query_tokens.iter().zip(&weights) {
        // Exact token match
        if let Some(index) = name_tokens
            .iter()
            .enumerate()
            .position(|(index, name_token)| !used_name_tokens[index] && name_token == query_token)
        {
            used_name_tokens[index] = true;
            earned_weight += w;
            specific_match |= query_token.len() >= 3;
            continue;
        }

        // Prefix match — require at least 3 chars on both sides
        if let Some(index) = name_tokens
            .iter()
            .enumerate()
            .position(|(index, name_token)| {
                !used_name_tokens[index]
                    && name_token.len() >= 3
                    && query_token.len() >= 3
                    && (name_token.starts_with(query_token) || query_token.starts_with(name_token))
            })
        {
            used_name_tokens[index] = true;
            earned_weight += w * 0.82;
            specific_match = true;
        }
    }

    let coverage = earned_weight / total_weight;

    // In a multi-token query, short-token (<3 char) matches shrink the
    // coverage DENOMINATOR as well as the numerator, so one weak exact
    // match like `to` == `to` alone would reach 50% coverage and admit
    // the candidate.  Short-token matches therefore only count when at
    // least one specific (>=3 char) token also matched — unless EVERY
    // query token matched exactly (coverage >= 1.0), in which case the
    // match is truthful even when all tokens are short (`io db` vs
    // `io_db`).  A single-token query is exempt for the same reason: an
    // exact short identifier (`db`, `io`) is a truthful match.
    if query_tokens.len() >= 2 && !specific_match && coverage < 1.0 {
        return 0.0;
    }

    if coverage >= 1.0 {
        0.86
    } else if coverage >= 0.5 {
        0.65 * coverage
    } else {
        0.0
    }
}

/// Common low-signal identifiers that appear across many symbols.
/// They receive reduced weight in query-token coverage so that a file
/// or symbol with repeated generic terms cannot win solely through
/// frequency.
const GENERIC_TERMS: &[&str] = &[
    "get", "set", "new", "data", "val", "value", "item", "items", "list", "map", "push", "insert",
    "create", "update", "delete", "remove", "add", "find", "check", "run", "init", "start", "stop",
    "load", "save", "read", "write", "print", "show", "hide", "open", "close", "make", "build",
    "parse", "format", "handle", "process", "call", "send", "recv", "clone", "copy", "move",
    "from", "into", "with", "self", "this", "test", "mock", "stub", "true", "false", "null",
    "none", "type", "kind", "name", "id", "key", "src", "dst", "msg", "err", "ctx", "req", "res",
    "len", "num", "cnt", "idx", "buf", "tmp", "obj", "arr", "str", "int", "bool", "result", "ret",
    "out",
];

/// Weight a query token by inverse generic-ness.
/// Generic terms get 0.3 weight; all other tokens get 1.0.
fn query_token_weight(token: &str) -> f32 {
    if token.len() < 3 {
        // Very short tokens are inherently low-signal.
        0.15
    } else if GENERIC_TERMS.contains(&token) {
        0.3
    } else {
        1.0
    }
}

fn text_tokens(text: &str) -> Vec<String> {
    let characters = text.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut current = String::new();

    for (index, character) in characters.iter().copied().enumerate() {
        if !character.is_alphanumeric() {
            push_token(&mut tokens, &mut current);
            continue;
        }
        let previous = index.checked_sub(1).and_then(|index| characters.get(index));
        let next = characters.get(index + 1);
        let camel_boundary = !current.is_empty()
            && character.is_uppercase()
            && (previous.is_some_and(|value| value.is_lowercase())
                || (previous.is_some_and(|value| value.is_uppercase())
                    && next.is_some_and(|value| value.is_lowercase())));
        let digit_boundary = !current.is_empty()
            && previous.is_some_and(|value| value.is_numeric() != character.is_numeric());
        if camel_boundary || digit_boundary {
            push_token(&mut tokens, &mut current);
        }
        current.extend(character.to_lowercase());
    }

    push_token(&mut tokens, &mut current);
    tokens
}

fn push_token(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(std::mem::take(current));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_index::store::SymbolStore;
    use crate::tree_sitter::{Position, Range};

    fn create_test_symbol(name: &str, symbol_type: SymbolType) -> Symbol {
        create_test_symbol_in_file(name, symbol_type, "test.ts")
    }

    fn create_test_symbol_in_file(name: &str, symbol_type: SymbolType, file_path: &str) -> Symbol {
        Symbol {
            id: format!("{}::{}#{}", file_path, name, symbol_type),
            name: name.to_string(),
            qualified_name: format!("{}::{}", file_path, name),
            symbol_type,
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
            docstring: None,
            signature: None,
            content_hash: "hash".to_string(),
        }
    }

    #[test]
    fn test_relevance_exact_match() {
        assert_eq!(calculate_relevance("authenticate", "authenticate"), 1.0);
    }

    #[test]
    fn test_relevance_prefix_match() {
        let score = calculate_relevance("authenticate", "auth");
        assert!(score > 0.8 && score <= 0.9);
    }

    #[test]
    fn test_relevance_contains_match() {
        let score = calculate_relevance("doAuthenticate", "auth");
        assert!(score > 0.4 && score < 0.7);
    }

    #[test]
    fn test_relevance_matches_tokenized_phrases() {
        assert!(calculate_relevance("UserService", "user service") > 0.8);
        assert!(calculate_relevance("normalize_user_id", "normalize user id") > 0.8);
        assert!(calculate_relevance(".button-primary", "button primary") > 0.8);
        assert!(calculate_relevance("XMLParser", "xml parser") > 0.8);
        assert_eq!(text_tokens("XMLParser"), vec!["xml", "parser"]);
    }

    #[test]
    fn test_relevance_short_fragment_prefix_does_not_match() {
        // A single-char or 2-char query must not satisfy a longer token via prefix.
        // Regression for `i` satisfying `icons`.
        assert_eq!(calculate_relevance("icons", "i"), 0.0);
        assert_eq!(calculate_relevance("navigation", "na"), 0.0);
        // 3-char prefix is still accepted.
        assert!(calculate_relevance("icons", "ico") > 0.0);
    }

    #[test]
    fn test_relevance_short_fragment_contains_does_not_match() {
        assert_eq!(calculate_relevance("doIcons", "i"), 0.0);
        assert_eq!(calculate_relevance("doNavigation", "na"), 0.0);
    }

    #[test]
    fn test_relevance_char_overlap_does_not_manufacture_match() {
        // Character-set overlap must not be an admission path.
        // `icons` and `navigation` share many characters but have no token overlap.
        assert_eq!(calculate_relevance("navigation", "icons"), 0.0);
        // `abc` and `cab` share all characters but are not the same token.
        assert_eq!(calculate_relevance("cab", "abc"), 0.0);
    }

    #[test]
    fn test_relevance_token_prefix_requires_min_length() {
        // Token-level prefix matching must require >= 3 chars on both sides.
        // `i18n` tokenizes to short fragments; `i` must not satisfy `icons`.
        assert_eq!(calculate_token_relevance("icons", "i"), 0.0);
        assert_eq!(calculate_token_relevance("icons", "ic"), 0.0);
        // 3-char token prefix is accepted.
        assert!(calculate_token_relevance("icons", "ico") > 0.0);
    }

    #[test]
    fn test_search_query_builder() {
        let query = SearchQuery::text("auth")
            .with_limit(10)
            .with_types(vec![SymbolType::Function]);

        assert_eq!(query.text, Some("auth".to_string()));
        assert_eq!(query.limit, Some(10));
        assert!(query.symbol_types.is_some());
    }

    #[test]
    fn test_search_result_creation() {
        let symbol = create_test_symbol("test", SymbolType::Function);
        let result = SearchResult::with_score(symbol.clone(), 0.85);

        assert_eq!(result.symbol.name, "test");
        assert_eq!(result.score, 0.85);
    }

    #[test]
    fn test_execute_search_filters_by_file_name_and_qualified_patterns() {
        let store = SymbolStore::in_memory().unwrap();
        let symbols = vec![
            create_test_symbol_in_file("buttonPrimary", SymbolType::CssSelector, "src/button.css"),
            create_test_symbol_in_file("buttonPrimary", SymbolType::Function, "src/button.tsx"),
            create_test_symbol_in_file(
                "buttonSecondary",
                SymbolType::CssSelector,
                "src/legacy.css",
            ),
            create_test_symbol_in_file("cardPrimary", SymbolType::CssSelector, "src/card.css"),
        ];
        store.upsert_symbols(&symbols).unwrap();

        let css_results = execute_search(
            &store,
            &SearchQuery::text("button")
                .with_file_pattern("src/*.css")
                .with_name_pattern("button*")
                .with_qualified_name_pattern("*button.css::*")
                .with_limit(10),
        )
        .unwrap();

        assert_eq!(css_results.len(), 1);
        assert_eq!(css_results[0].symbol.file_path, "src/button.css");
        assert_eq!(css_results[0].symbol.name, "buttonPrimary");

        let comma_results = execute_search(
            &store,
            &SearchQuery::text("button")
                .with_file_pattern("src/button.tsx,src/legacy.css")
                .with_limit(10),
        )
        .unwrap();

        assert_eq!(comma_results.len(), 2);
        assert!(comma_results
            .iter()
            .any(|result| result.symbol.file_path == "src/button.tsx"));
        assert!(comma_results
            .iter()
            .any(|result| result.symbol.file_path == "src/legacy.css"));
    }

    #[test]
    fn test_execute_search_overfetches_when_filters_are_present() {
        let store = SymbolStore::in_memory().unwrap();
        let mut symbols = (0..40)
            .map(|index| {
                create_test_symbol_in_file(
                    &format!("button{:02}", index),
                    SymbolType::Function,
                    "src/noisy.ts",
                )
            })
            .collect::<Vec<_>>();
        symbols.push(create_test_symbol_in_file(
            "buttonTarget",
            SymbolType::CssSelector,
            "src/target.css",
        ));
        store.upsert_symbols(&symbols).unwrap();

        let results = execute_search(
            &store,
            &SearchQuery::text("button")
                .with_file_pattern("src/target.css")
                .with_limit(1),
        )
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "buttonTarget");
        assert_eq!(results[0].symbol.file_path, "src/target.css");
    }

    #[test]
    fn test_execute_search_uses_signature_metadata_candidates() {
        let store = SymbolStore::in_memory().unwrap();
        let mut symbol =
            create_test_symbol_in_file("findUser", SymbolType::Function, "src/user_service.ts");
        symbol.signature = Some("function findUser(id: string): UserDto".to_string());
        store.upsert_symbols(&[symbol]).unwrap();

        let results = execute_search(&store, &SearchQuery::text("UserDto").with_limit(10)).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol.name, "findUser");
        assert!(results[0].score > 0.0);
    }

    #[test]
    fn test_execute_search_ranks_tokenized_name_matches() {
        let store = SymbolStore::in_memory().unwrap();
        let symbols = vec![
            create_test_symbol_in_file("UserService", SymbolType::Class, "src/user_service.ts"),
            create_test_symbol_in_file("UserSettings", SymbolType::Class, "src/user_settings.ts"),
        ];
        store.upsert_symbols(&symbols).unwrap();

        let results =
            execute_search(&store, &SearchQuery::text("user service").with_limit(10)).unwrap();

        assert_eq!(results[0].symbol.name, "UserService");
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_execute_search_ranks_acronym_token_matches() {
        let store = SymbolStore::in_memory().unwrap();
        let symbols = vec![
            create_test_symbol_in_file("XMLHttpRequest", SymbolType::Class, "src/http.ts"),
            create_test_symbol_in_file("XMLParser", SymbolType::Class, "src/parser.ts"),
        ];
        store.upsert_symbols(&symbols).unwrap();

        let results =
            execute_search(&store, &SearchQuery::text("xml parser").with_limit(10)).unwrap();

        assert_eq!(results[0].symbol.name, "XMLParser");
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_execute_search_applies_structural_type_boosts() {
        let store = SymbolStore::in_memory().unwrap();
        let symbols = vec![
            create_test_symbol_in_file("runTask", SymbolType::Import, "src/imports.ts"),
            create_test_symbol_in_file("runTask", SymbolType::Function, "src/tasks.ts"),
        ];
        store.upsert_symbols(&symbols).unwrap();

        let results = execute_search(&store, &SearchQuery::text("runTask").with_limit(10)).unwrap();

        assert_eq!(results[0].symbol.symbol_type, SymbolType::Function);
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_execute_search_merges_fts_and_like_candidates() {
        let store = SymbolStore::in_memory().unwrap();
        let symbols = vec![
            create_test_symbol("--accent-ai", SymbolType::CssCustomProperty),
            create_test_symbol(".chat-message", SymbolType::CssSelector),
            create_test_symbol("GitCommitMessage", SymbolType::Function),
        ];
        store.upsert_symbols(&symbols).unwrap();

        let accent_results =
            execute_search(&store, &SearchQuery::text("--accent").with_limit(10)).unwrap();
        assert!(accent_results
            .iter()
            .any(|result| result.symbol.name == "--accent-ai"));

        let git_results = execute_search(
            &store,
            &SearchQuery::text("GitCommitMessage").with_limit(10),
        )
        .unwrap();
        assert_eq!(git_results[0].symbol.name, "GitCommitMessage");
    }

    #[test]
    fn query_model_splits_camel_acronym_digits_and_unicode_terms() {
        let model = QueryModel::parse("HTTPServer symbol_search planSummary i18n Zaguán");
        assert_eq!(
            model.tokens().collect::<Vec<_>>(),
            [
                "http", "server", "symbol", "search", "plan", "summary", "18", "zaguán"
            ]
        );
    }

    #[test]
    fn cross_field_coverage_admits_a_symbol_and_explains_the_match() {
        let mut symbol = create_test_symbol_in_file(
            "findUser",
            SymbolType::Function,
            "src/services/accounts.ts",
        );
        symbol.signature = Some("(id: UserId) -> AccountDto".to_string());
        let query = QueryModel::parse("user account dto");

        let (_, breakdown) = score_symbol(&symbol, &query).expect("cross-field match");
        assert_eq!(breakdown.matched_tokens, ["user", "account", "dto"]);
        assert!(breakdown
            .matched_fields
            .iter()
            .any(|field| field == "name"));
        assert!(breakdown
            .matched_fields
            .iter()
            .any(|field| field == "signature"));
        assert!(breakdown.token_coverage >= 0.99);
    }

    #[test]
    fn long_queries_admit_declared_entry_points_but_not_path_only_properties() {
        let query = QueryModel::parse("mobile top navigation icons locale location");
        let entry_point = create_test_symbol_in_file(
            "MobileHeader",
            SymbolType::Function,
            "src/components/header.tsx",
        );
        let (_, breakdown) = score_symbol(&entry_point, &query).expect("named entry point");
        assert_eq!(breakdown.matched_tokens, ["mobile"]);
        assert_eq!(breakdown.confidence, SearchConfidence::Low);

        let incidental = create_test_symbol_in_file(
            "top_venues",
            SymbolType::Property,
            "src/lib/i18n/locales/da/venues.json",
        );
        assert!(score_symbol(&incidental, &query).is_none());
    }

    #[test]
    fn explain_mode_returns_score_breakdown_only_when_requested() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[create_test_symbol("HTTPServer", SymbolType::Struct)])
            .unwrap();

        let explained = execute_search(
            &store,
            &SearchQuery::text("HTTPServer")
                .with_limit(5)
                .with_explain(true),
        )
        .unwrap();
        assert!(explained[0].score_breakdown.is_some());
        assert!(explained[0]
            .score_breakdown
            .as_ref()
            .is_some_and(|breakdown| breakdown.exact_identifier));

        let compact =
            execute_search(&store, &SearchQuery::text("HTTPServer").with_limit(5)).unwrap();
        assert!(compact[0].score_breakdown.is_none());
    }

    // ---- Track G: weighted coverage, generic-term penalty, score floor --------

    #[test]
    fn test_generic_term_weight_reduces_coverage_impact() {
        // A query where the only match is a generic term should score lower
        // than a query where the only match is a specific term.
        // `getData` tokens: ["get", "data"] — both generic.
        // Query "get" (generic): exact match on `get`, weight 0.3, coverage 1.0 → 0.86.
        // Query "fetch" (specific): no match → 0.0.
        // Query "data" (generic): exact match on `data`, weight 0.3, coverage 1.0 → 0.86.
        // Now compare: `getIcon` with `get` (generic) vs `getIcon` with `icon` (specific).
        // `get` exact match: weight 0.3, coverage = 0.3/0.3 = 1.0 → 0.86.
        // `icon` prefix match on `icon`: weight 1.0, earned 0.82, coverage = 0.82/1.0 = 0.82 → 0.533.
        // The generic exact match scores higher than the specific prefix match.
        // That is expected: exact is exact. The weighting matters when there
        // are multiple tokens.
        let score_generic_only = calculate_token_relevance("getIcon", "get");
        let score_specific_only = calculate_token_relevance("getIcon", "icons");
        // Both should be non-zero.
        assert!(score_generic_only > 0.0);
        assert!(score_specific_only > 0.0);

        // Now verify that in a multi-token query, adding a generic term
        // that also matches does not dramatically increase the score
        // compared to a specific-only match.
        // `getIcon` vs `get icons`: `get` exact (0.3), `icons` prefix on `icon` (0.82).
        // earned = 0.3 + 0.82 = 1.12, total = 0.3 + 1.0 = 1.3, coverage = 0.862 → 0.56.
        // `getIcon` vs `icons`: `icons` prefix on `icon` (0.82), coverage = 0.82 → 0.533.
        // The generic-diluted score (0.56) is only slightly higher than the
        // specific-only score (0.533) because the generic term's weight is
        // only 0.3. Without weighting, both would be `get`(1.0) + `icons`(0.82)
        // = 1.82 / 2.0 = 0.91 → 0.59, which is a bigger boost from the generic term.
        let score_with_generic = calculate_token_relevance("getIcon", "get icons");
        let score_without_generic = calculate_token_relevance("getIcon", "icons");
        // The generic-diluted score should be only modestly higher.
        assert!(
            score_with_generic > score_without_generic,
            "adding a matching generic term should still increase coverage"
        );
        // But the increase should be small (less than 0.1) because the generic
        // term is weighted low.
        assert!(
            score_with_generic - score_without_generic < 0.1,
            "generic term should not dramatically boost score: diff = {}",
            score_with_generic - score_without_generic
        );
    }

    #[test]
    fn test_execute_search_rejects_zero_score_candidates() {
        let store = SymbolStore::in_memory().unwrap();
        let symbols = vec![
            create_test_symbol("LanguageSwitcher", SymbolType::Function),
            create_test_symbol("completely_unrelated_symbol", SymbolType::Function),
        ];
        store.upsert_symbols(&symbols).unwrap();

        // Search for `language` — should match LanguageSwitcher but not the
        // unrelated symbol.  The unrelated symbol may appear in broad
        // candidate retrieval but must be filtered out by the score floor.
        let results =
            execute_search(&store, &SearchQuery::text("language").with_limit(10)).unwrap();
        assert!(
            results.iter().all(|r| r.score > 0.0),
            "all returned results must have non-zero score"
        );
        assert!(
            results.iter().any(|r| r.symbol.name == "LanguageSwitcher"),
            "LanguageSwitcher should be in results"
        );
        assert!(
            !results
                .iter()
                .any(|r| r.symbol.name == "completely_unrelated_symbol"),
            "unrelated symbol must be filtered out by score floor"
        );
    }

    #[test]
    fn test_multi_token_query_requires_a_specific_token_match() {
        // `to` exactly matches a token of `switchTo`, but in a multi-token
        // query a short-token match alone must not reach the coverage
        // threshold (the short weights shrink the denominator too).
        assert_eq!(calculate_token_relevance("switchTo", "na to"), 0.0);
        assert_eq!(calculate_token_relevance("switchTo", "to na"), 0.0);
        // With a specific token matching, the short token counts again.
        assert!(calculate_token_relevance("switchTo", "switch to") > 0.0);
        // Single-token short queries stay a truthful exact match.
        assert!(calculate_token_relevance("db_pool", "db") > 0.0);
        // Full weighted coverage via exact matches is a truthful match even
        // when every query token is short: `io db` covers `io_db` entirely.
        assert_eq!(calculate_token_relevance("io_db", "io db"), 0.86);
        assert_eq!(calculate_token_relevance("io_db", "db io"), 0.86);
        // Partial short-only coverage stays rejected (`to` covers only half
        // of `na to` by weight).
        assert_eq!(calculate_token_relevance("switchTo", "na to"), 0.0);
    }

    #[test]
    fn test_contextual_boosts_cannot_resurrect_zero_score_candidates() {
        // A candidate with zero lexical relevance must be rejected even when
        // it lives in the active file: boosts re-rank admitted results, they
        // are not an admission path.
        let store = SymbolStore::in_memory().unwrap();
        let symbols = vec![
            create_test_symbol_in_file("LanguageSwitcher", SymbolType::Function, "other.ts"),
            create_test_symbol_in_file(
                "completely_unrelated_symbol",
                SymbolType::Function,
                "active.ts",
            ),
        ];
        store.upsert_symbols(&symbols).unwrap();

        let results = execute_search(
            &store,
            &SearchQuery::text("language")
                .with_limit(10)
                .with_active_file("active.ts"),
        )
        .unwrap();
        assert!(
            !results
                .iter()
                .any(|r| r.symbol.name == "completely_unrelated_symbol"),
            "active-file boost must not admit a zero-lexical-score candidate"
        );
        assert!(
            results.iter().any(|r| r.symbol.name == "LanguageSwitcher"),
            "LanguageSwitcher should still be returned"
        );
    }

    #[test]
    fn test_distractor_top_venues_does_not_match_navigation_query() {
        // Regression query from the handoff:
        //   query: "mobile top navigation icons locale location"
        //   distractor: top_venues in a locales JSON path
        // Tokenisation may split `i18n` into short fragments, but neither
        // `i` nor `n` may satisfy `icons` or `navigation`.
        let name = "top_venues";
        let query = "mobile top navigation icons locale location";
        let score = calculate_relevance(name, query);
        assert_eq!(
            score, 0.0,
            "top_venues must not match the navigation/icons query via char overlap or short fragments"
        );
        // Also verify token-level: `top` matches but the rest don't.
        let token_score = calculate_token_relevance(name, query);
        // `top` is one of 5 query tokens.  Even if it matches, coverage is
        // 1/5 = 0.2 which is below the 0.5 threshold, so score is 0.0.
        assert_eq!(
            token_score, 0.0,
            "token relevance for top_venues vs the full query should be 0 (coverage < 0.5)"
        );
    }
}
