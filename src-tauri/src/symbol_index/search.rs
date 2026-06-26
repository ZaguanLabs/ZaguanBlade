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

    /// Create a query for symbols in a specific file
    pub fn in_file(file_path: &str) -> Self {
        Self {
            file_path: Some(file_path.to_string()),
            limit: Some(100),
            recursive: false,
            ..Default::default()
        }
    }

    /// Create a query for symbols of a specific type
    pub fn of_type(symbol_type: SymbolType) -> Self {
        Self {
            symbol_types: Some(vec![symbol_type]),
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
}

impl SearchResult {
    /// Create a search result with default score
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            score: 1.0,
            highlights: vec![],
        }
    }

    /// Create with a specific score
    pub fn with_score(symbol: Symbol, score: f32) -> Self {
        Self {
            symbol,
            score,
            highlights: vec![],
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
        let symbols = search_symbol_candidates(store, text, search_candidate_limit(query, limit))?;
        let mut results: Vec<SearchResult> = symbols
            .into_iter()
            .map(|s| {
                let score = calculate_symbol_relevance(&s, text);
                SearchResult::with_score(s, score)
            })
            .collect();

        apply_result_filters(&mut results, query);

        apply_contextual_boosts(&mut results, query);

        // Sort by score and limit
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
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

fn symbol_matches_query_filters(symbol: &Symbol, query: &SearchQuery) -> bool {
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
    candidate_limit: usize,
) -> Result<Vec<Symbol>, SymbolStoreError> {
    let mut symbols = Vec::new();
    let mut seen = HashSet::new();

    for symbol in store.search_by_name(text, candidate_limit)? {
        if seen.insert(symbol.id.clone()) {
            symbols.push(symbol);
        }
    }

    if symbols.len() < candidate_limit {
        for symbol in store.search_by_name_like(text, candidate_limit)? {
            if seen.insert(symbol.id.clone()) {
                symbols.push(symbol);
            }
            if symbols.len() >= candidate_limit {
                break;
            }
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

fn calculate_symbol_relevance(symbol: &Symbol, query: &str) -> f32 {
    let name_score = calculate_relevance(&symbol.name, query);
    let qualified_score = calculate_relevance(&symbol.qualified_name, query) * 0.92;
    let signature_score = symbol
        .signature
        .as_deref()
        .map(|signature| calculate_relevance(signature, query) * 0.62)
        .unwrap_or(0.0);
    let docstring_score = symbol
        .docstring
        .as_deref()
        .map(|docstring| calculate_relevance(docstring, query) * 0.48)
        .unwrap_or(0.0);
    name_score
        .max(qualified_score)
        .max(signature_score)
        .max(docstring_score)
}

/// Calculate relevance score between query and symbol name
fn calculate_relevance(name: &str, query: &str) -> f32 {
    let name_lower = name.to_lowercase();
    let query_lower = query.to_lowercase();

    // Exact match
    if name_lower == query_lower {
        return 1.0;
    }

    // Prefix match
    if name_lower.starts_with(&query_lower) {
        return 0.9;
    }

    // Contains match
    if name_lower.contains(&query_lower) {
        // Score based on position (earlier is better)
        let pos = name_lower.find(&query_lower).unwrap_or(0) as f32;
        let len = name_lower.len() as f32;
        return 0.7 - (pos / len) * 0.3;
    }

    let token_score = calculate_token_relevance(name, query);
    if token_score > 0.0 {
        return token_score;
    }

    // Fuzzy match using character overlap
    let query_chars: std::collections::HashSet<char> = query_lower.chars().collect();
    let name_chars: std::collections::HashSet<char> = name_lower.chars().collect();
    let intersection = query_chars.intersection(&name_chars).count() as f32;
    let union = query_chars.union(&name_chars).count() as f32;

    if union > 0.0 {
        0.5 * (intersection / union)
    } else {
        0.0
    }
}

fn calculate_token_relevance(name: &str, query: &str) -> f32 {
    let name_tokens = text_tokens(name);
    let query_tokens = text_tokens(query);
    if name_tokens.is_empty() || query_tokens.is_empty() {
        return 0.0;
    }

    let mut used_name_tokens = vec![false; name_tokens.len()];
    let mut score = 0.0f32;
    for query_token in &query_tokens {
        if let Some(index) = name_tokens
            .iter()
            .enumerate()
            .position(|(index, name_token)| !used_name_tokens[index] && name_token == query_token)
        {
            used_name_tokens[index] = true;
            score += 1.0;
            continue;
        }

        if let Some(index) = name_tokens
            .iter()
            .enumerate()
            .position(|(index, name_token)| {
                !used_name_tokens[index]
                    && (name_token.starts_with(query_token) || query_token.starts_with(name_token))
            })
        {
            used_name_tokens[index] = true;
            score += 0.82;
        }
    }

    let coverage = score / query_tokens.len() as f32;
    if coverage >= 1.0 {
        0.86
    } else if coverage >= 0.5 {
        0.65 * coverage
    } else {
        0.0
    }
}

fn text_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut previous_was_lower_or_digit = false;
    let mut previous_was_upper = false;

    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if !ch.is_ascii_alphanumeric() {
            push_token(&mut tokens, &mut current);
            previous_was_lower_or_digit = false;
            previous_was_upper = false;
            continue;
        }

        if ch.is_ascii_uppercase() && !current.is_empty() {
            if previous_was_lower_or_digit
                || (previous_was_upper
                    && chars
                        .peek()
                        .is_some_and(|next| next.is_ascii_lowercase()))
            {
                push_token(&mut tokens, &mut current);
            }
        }
        current.push(ch.to_ascii_lowercase());
        previous_was_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        previous_was_upper = ch.is_ascii_uppercase();
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
}
