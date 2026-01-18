//! Tree-sitter query management
//!
//! Manages tree-sitter queries for each language to efficiently extract
//! specific patterns from code.

use std::collections::HashMap;
use tree_sitter::{Query, QueryCursor};

use super::parser::Language;

/// Query manager for tree-sitter queries per language
pub struct QueryManager {
    queries: HashMap<Language, LanguageQueries>,
}

/// Queries for a specific language
pub struct LanguageQueries {
    /// Query for finding function/method definitions
    pub functions: Option<Query>,
    /// Query for finding class/struct definitions
    pub classes: Option<Query>,
    /// Query for finding imports
    pub imports: Option<Query>,
}

impl QueryManager {
    /// Create a new query manager with queries for all supported languages
    pub fn new() -> Result<Self, String> {
        let mut queries = HashMap::new();

        // TypeScript queries
        let ts_lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        queries.insert(
            Language::TypeScript,
            LanguageQueries {
                functions: Self::create_query(&ts_lang, TYPESCRIPT_FUNCTION_QUERY).ok(),
                classes: Self::create_query(&ts_lang, TYPESCRIPT_CLASS_QUERY).ok(),
                imports: Self::create_query(&ts_lang, TYPESCRIPT_IMPORT_QUERY).ok(),
            },
        );

        // TSX uses same queries as TypeScript
        let tsx_lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TSX.into();
        queries.insert(
            Language::Tsx,
            LanguageQueries {
                functions: Self::create_query(&tsx_lang, TYPESCRIPT_FUNCTION_QUERY).ok(),
                classes: Self::create_query(&tsx_lang, TYPESCRIPT_CLASS_QUERY).ok(),
                imports: Self::create_query(&tsx_lang, TYPESCRIPT_IMPORT_QUERY).ok(),
            },
        );

        // JavaScript queries
        let js_lang: tree_sitter::Language = tree_sitter_javascript::LANGUAGE.into();
        queries.insert(
            Language::JavaScript,
            LanguageQueries {
                functions: Self::create_query(&js_lang, JAVASCRIPT_FUNCTION_QUERY).ok(),
                classes: Self::create_query(&js_lang, JAVASCRIPT_CLASS_QUERY).ok(),
                imports: Self::create_query(&js_lang, JAVASCRIPT_IMPORT_QUERY).ok(),
            },
        );

        // JSX uses same queries as JavaScript
        queries.insert(
            Language::Jsx,
            LanguageQueries {
                functions: Self::create_query(&js_lang, JAVASCRIPT_FUNCTION_QUERY).ok(),
                classes: Self::create_query(&js_lang, JAVASCRIPT_CLASS_QUERY).ok(),
                imports: Self::create_query(&js_lang, JAVASCRIPT_IMPORT_QUERY).ok(),
            },
        );

        // Python queries
        let py_lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
        queries.insert(
            Language::Python,
            LanguageQueries {
                functions: Self::create_query(&py_lang, PYTHON_FUNCTION_QUERY).ok(),
                classes: Self::create_query(&py_lang, PYTHON_CLASS_QUERY).ok(),
                imports: Self::create_query(&py_lang, PYTHON_IMPORT_QUERY).ok(),
            },
        );

        // Rust queries
        let rs_lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        queries.insert(
            Language::Rust,
            LanguageQueries {
                functions: Self::create_query(&rs_lang, RUST_FUNCTION_QUERY).ok(),
                classes: Self::create_query(&rs_lang, RUST_STRUCT_QUERY).ok(),
                imports: Self::create_query(&rs_lang, RUST_USE_QUERY).ok(),
            },
        );

        Ok(Self { queries })
    }

    fn create_query(lang: &tree_sitter::Language, query_str: &str) -> Result<Query, String> {
        Query::new(lang, query_str).map_err(|e| e.to_string())
    }

    /// Get queries for a language
    pub fn get_queries(&self, language: Language) -> Option<&LanguageQueries> {
        self.queries.get(&language)
    }

    /// Create a new query cursor for executing queries
    pub fn new_cursor() -> QueryCursor {
        QueryCursor::new()
    }
}

impl Default for QueryManager {
    fn default() -> Self {
        Self::new().expect("Failed to initialize query manager")
    }
}

// =============================================================================
// Tree-sitter Queries
// =============================================================================

// TypeScript/TSX queries
const TYPESCRIPT_FUNCTION_QUERY: &str = r#"
(function_declaration
  name: (identifier) @name) @function

(method_definition
  name: (property_identifier) @name) @method

(arrow_function) @arrow
"#;

const TYPESCRIPT_CLASS_QUERY: &str = r#"
(class_declaration
  name: (type_identifier) @name) @class

(interface_declaration
  name: (type_identifier) @name) @interface

(type_alias_declaration
  name: (type_identifier) @name) @type_alias
"#;

const TYPESCRIPT_IMPORT_QUERY: &str = r#"
(import_statement
  source: (string) @source) @import
"#;

// JavaScript queries
const JAVASCRIPT_FUNCTION_QUERY: &str = r#"
(function_declaration
  name: (identifier) @name) @function

(method_definition
  name: (property_identifier) @name) @method

(arrow_function) @arrow
"#;

const JAVASCRIPT_CLASS_QUERY: &str = r#"
(class_declaration
  name: (identifier) @name) @class
"#;

const JAVASCRIPT_IMPORT_QUERY: &str = r#"
(import_statement
  source: (string) @source) @import
"#;

// Python queries
const PYTHON_FUNCTION_QUERY: &str = r#"
(function_definition
  name: (identifier) @name) @function
"#;

const PYTHON_CLASS_QUERY: &str = r#"
(class_definition
  name: (identifier) @name) @class
"#;

const PYTHON_IMPORT_QUERY: &str = r#"
(import_statement
  name: (dotted_name) @name) @import

(import_from_statement
  module_name: (dotted_name) @module) @import_from
"#;

// Rust queries
const RUST_FUNCTION_QUERY: &str = r#"
(function_item
  name: (identifier) @name) @function
"#;

const RUST_STRUCT_QUERY: &str = r#"
(struct_item
  name: (type_identifier) @name) @struct

(enum_item
  name: (type_identifier) @name) @enum

(trait_item
  name: (type_identifier) @name) @trait

(impl_item
  type: (type_identifier) @type) @impl
"#;

const RUST_USE_QUERY: &str = r#"
(use_declaration
  argument: (use_wildcard)? @wildcard
  argument: (scoped_identifier)? @scoped
  argument: (identifier)? @ident) @use
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_manager_creation() {
        let manager = QueryManager::new().unwrap();

        // Check all languages have queries
        assert!(manager.get_queries(Language::TypeScript).is_some());
        assert!(manager.get_queries(Language::Tsx).is_some());
        assert!(manager.get_queries(Language::JavaScript).is_some());
        assert!(manager.get_queries(Language::Jsx).is_some());
        assert!(manager.get_queries(Language::Python).is_some());
        assert!(manager.get_queries(Language::Rust).is_some());
    }

    #[test]
    fn test_typescript_function_query() {
        let manager = QueryManager::new().unwrap();
        let queries = manager.get_queries(Language::TypeScript).unwrap();

        assert!(queries.functions.is_some());
    }

    #[test]
    fn test_rust_queries() {
        let manager = QueryManager::new().unwrap();
        let queries = manager.get_queries(Language::Rust).unwrap();

        assert!(queries.functions.is_some());
        assert!(queries.classes.is_some());
        assert!(queries.imports.is_some());
    }
}
