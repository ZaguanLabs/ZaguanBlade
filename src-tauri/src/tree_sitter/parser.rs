//! Tree-sitter parser management
//!
//! Manages parsers for multiple programming languages with support for
//! incremental parsing for fast updates on file changes.

use std::collections::HashMap;
use tree_sitter::{Parser, Tree};

/// Supported programming languages for parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    TypeScript,
    Tsx,
    JavaScript,
    Jsx,
    Python,
    Rust,
}

impl Language {
    /// Detect language from file path extension
    pub fn from_path(path: &str) -> Option<Self> {
        let ext = path.rsplit('.').next()?;
        match ext.to_lowercase().as_str() {
            "ts" => Some(Language::TypeScript),
            "tsx" => Some(Language::Tsx),
            "js" => Some(Language::JavaScript),
            "jsx" => Some(Language::Jsx),
            "mjs" | "cjs" => Some(Language::JavaScript),
            "py" => Some(Language::Python),
            "rs" => Some(Language::Rust),
            _ => None,
        }
    }

    /// Get display name for the language
    pub fn display_name(&self) -> &'static str {
        match self {
            Language::TypeScript => "TypeScript",
            Language::Tsx => "TSX",
            Language::JavaScript => "JavaScript",
            Language::Jsx => "JSX",
            Language::Python => "Python",
            Language::Rust => "Rust",
        }
    }
}

/// Error type for tree-sitter operations
#[derive(Debug)]
pub enum TreeSitterError {
    UnsupportedLanguage,
    ParseFailed,
    LanguageInitFailed(String),
}

impl std::fmt::Display for TreeSitterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeSitterError::UnsupportedLanguage => write!(f, "Unsupported language"),
            TreeSitterError::ParseFailed => write!(f, "Failed to parse code"),
            TreeSitterError::LanguageInitFailed(msg) => {
                write!(f, "Failed to initialize language: {}", msg)
            }
        }
    }
}

impl std::error::Error for TreeSitterError {}

/// Tree-sitter parser manager
///
/// Manages parsers for multiple languages and provides parsing functionality
/// with support for incremental updates.
pub struct TreeSitterParser {
    parsers: HashMap<Language, Parser>,
}

impl TreeSitterParser {
    /// Create a new parser manager with all supported languages initialized
    pub fn new() -> Result<Self, TreeSitterError> {
        let mut parsers = HashMap::new();

        // Initialize TypeScript parser
        let mut ts_parser = Parser::new();
        ts_parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .map_err(|e| TreeSitterError::LanguageInitFailed(e.to_string()))?;
        parsers.insert(Language::TypeScript, ts_parser);

        // Initialize TSX parser
        let mut tsx_parser = Parser::new();
        tsx_parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
            .map_err(|e| TreeSitterError::LanguageInitFailed(e.to_string()))?;
        parsers.insert(Language::Tsx, tsx_parser);

        // Initialize JavaScript parser
        let mut js_parser = Parser::new();
        js_parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .map_err(|e| TreeSitterError::LanguageInitFailed(e.to_string()))?;
        parsers.insert(Language::JavaScript, js_parser);

        // JSX uses the same grammar as JavaScript in tree-sitter-javascript
        let mut jsx_parser = Parser::new();
        jsx_parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .map_err(|e| TreeSitterError::LanguageInitFailed(e.to_string()))?;
        parsers.insert(Language::Jsx, jsx_parser);

        // Initialize Python parser
        let mut py_parser = Parser::new();
        py_parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .map_err(|e| TreeSitterError::LanguageInitFailed(e.to_string()))?;
        parsers.insert(Language::Python, py_parser);

        // Initialize Rust parser
        let mut rs_parser = Parser::new();
        rs_parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|e| TreeSitterError::LanguageInitFailed(e.to_string()))?;
        parsers.insert(Language::Rust, rs_parser);

        Ok(Self { parsers })
    }

    /// Parse source code for the given language
    ///
    /// Returns the AST tree on success.
    pub fn parse(&mut self, code: &str, language: Language) -> Result<Tree, TreeSitterError> {
        let parser = self
            .parsers
            .get_mut(&language)
            .ok_or(TreeSitterError::UnsupportedLanguage)?;

        parser.parse(code, None).ok_or(TreeSitterError::ParseFailed)
    }

    /// Parse source code with an existing tree for incremental updates
    ///
    /// This is significantly faster for small edits as tree-sitter can reuse
    /// unchanged portions of the old tree.
    pub fn parse_incremental(
        &mut self,
        code: &str,
        old_tree: &Tree,
        language: Language,
    ) -> Result<Tree, TreeSitterError> {
        let parser = self
            .parsers
            .get_mut(&language)
            .ok_or(TreeSitterError::UnsupportedLanguage)?;

        parser
            .parse(code, Some(old_tree))
            .ok_or(TreeSitterError::ParseFailed)
    }

    /// Check if a language is supported
    pub fn supports_language(&self, language: Language) -> bool {
        self.parsers.contains_key(&language)
    }

    /// Get list of supported languages
    pub fn supported_languages(&self) -> Vec<Language> {
        self.parsers.keys().copied().collect()
    }
}

impl Default for TreeSitterParser {
    fn default() -> Self {
        Self::new().expect("Failed to initialize tree-sitter parsers")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection() {
        assert_eq!(Language::from_path("main.ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_path("app.tsx"), Some(Language::Tsx));
        assert_eq!(Language::from_path("script.js"), Some(Language::JavaScript));
        assert_eq!(Language::from_path("component.jsx"), Some(Language::Jsx));
        assert_eq!(Language::from_path("main.py"), Some(Language::Python));
        assert_eq!(Language::from_path("lib.rs"), Some(Language::Rust));
        assert_eq!(Language::from_path("data.json"), None);
    }

    #[test]
    fn test_parse_typescript() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "function hello(): string { return 'world'; }";
        let tree = parser.parse(code, Language::TypeScript).unwrap();

        assert!(!tree.root_node().has_error());
        assert_eq!(tree.root_node().kind(), "program");
    }

    #[test]
    fn test_parse_javascript() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "const greet = (name) => `Hello, ${name}!`;";
        let tree = parser.parse(code, Language::JavaScript).unwrap();

        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn test_parse_python() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "def greet(name: str) -> str:\n    return f'Hello, {name}!'";
        let tree = parser.parse(code, Language::Python).unwrap();

        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn test_parse_rust() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "fn greet(name: &str) -> String { format!(\"Hello, {}!\", name) }";
        let tree = parser.parse(code, Language::Rust).unwrap();

        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn test_incremental_parse() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code1 = "function hello() { return 'world'; }";
        let tree1 = parser.parse(code1, Language::TypeScript).unwrap();

        // Simulate an edit (replace 'world' with 'universe')
        let code2 = "function hello() { return 'universe'; }";
        let tree2 = parser
            .parse_incremental(code2, &tree1, Language::TypeScript)
            .unwrap();

        assert!(!tree2.root_node().has_error());
    }
}
