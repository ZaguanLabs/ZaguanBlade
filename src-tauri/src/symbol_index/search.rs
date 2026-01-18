//! Symbol search functionality
//!
//! Provides structured queries and result types for searching
//! the symbol index.

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
    /// Filter by symbol types
    pub symbol_types: Option<Vec<SymbolType>>,
    /// Maximum results to return
    pub limit: Option<usize>,
    /// Include symbols from subdirectories
    pub recursive: bool,
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

    /// Add type filter
    pub fn with_types(mut self, types: Vec<SymbolType>) -> Self {
        self.symbol_types = Some(types);
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
        let results = filter_by_type(symbols, query.symbol_types.as_deref())
            .into_iter()
            .take(limit)
            .map(SearchResult::new)
            .collect();
        return Ok(results);
    }

    // Search by text
    if let Some(ref text) = query.text {
        let symbols = store.search_by_name_like(text, limit * 2)?;
        let mut results: Vec<SearchResult> = symbols
            .into_iter()
            .map(|s| {
                let score = calculate_relevance(&s.name, text);
                SearchResult::with_score(s, score)
            })
            .collect();

        // Filter by type if specified
        if let Some(ref types) = query.symbol_types {
            results.retain(|r| types.contains(&r.symbol.symbol_type));
        }

        // Filter by file if specified
        if let Some(ref file_path) = query.file_path {
            if query.recursive {
                results.retain(|r| r.symbol.file_path.starts_with(file_path));
            } else {
                results.retain(|r| &r.symbol.file_path == file_path);
            }
        }

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
        all_results.truncate(limit);
        return Ok(all_results);
    }

    Ok(vec![])
}

/// Filter symbols by type
fn filter_by_type(symbols: Vec<Symbol>, types: Option<&[SymbolType]>) -> Vec<Symbol> {
    match types {
        Some(types) if !types.is_empty() => symbols
            .into_iter()
            .filter(|s| types.contains(&s.symbol_type))
            .collect(),
        _ => symbols,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter::{Position, Range};

    fn create_test_symbol(name: &str, symbol_type: SymbolType) -> Symbol {
        Symbol {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            symbol_type,
            file_path: "test.ts".to_string(),
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
            parent_id: None,
            docstring: None,
            signature: None,
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
}
