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
    Astro,
    JavaScript,
    Jsx,
    Python,
    Rust,
    Go,
    Markdown,
    Css,
    Scss,
    Sass,
    Less,
    Html,
    Vue,
    Svelte,
    Json,
    Yaml,
    Toml,
    Php,
    Java,
    CSharp,
    Kotlin,
    Ruby,
    Cpp,
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportLevel {
    Full,
    Partial,
    AnchorOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeSitterGrammar {
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Rust,
    Go,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserKind {
    TreeSitter(TreeSitterGrammar),
    Projection { target: Language },
    Scanner,
    MarkdownHeadings,
    AnchorOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractionCapabilities {
    pub definitions: bool,
    pub imports: bool,
    pub relationships: bool,
    pub semantic_anchors: bool,
    pub markdown_headings: bool,
}

impl ExtractionCapabilities {
    pub const fn code(definitions: bool, imports: bool, relationships: bool) -> Self {
        Self {
            definitions,
            imports,
            relationships,
            semantic_anchors: true,
            markdown_headings: false,
        }
    }

    pub const fn markdown() -> Self {
        Self {
            definitions: false,
            imports: false,
            relationships: false,
            semantic_anchors: true,
            markdown_headings: true,
        }
    }

    pub const fn scanner(definitions: bool) -> Self {
        Self {
            definitions,
            imports: false,
            relationships: false,
            semantic_anchors: true,
            markdown_headings: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageCapability {
    pub language: Language,
    pub display_name: &'static str,
    pub extensions: &'static [&'static str],
    pub parser: ParserKind,
    pub support: SupportLevel,
    pub extractor_version: u32,
    pub extracts: ExtractionCapabilities,
}

const LANGUAGE_CAPABILITIES: &[LanguageCapability] = &[
    LanguageCapability {
        language: Language::TypeScript,
        display_name: "TypeScript",
        extensions: &["ts"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::TypeScript),
        support: SupportLevel::Full,
        extractor_version: 1,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Tsx,
        display_name: "TSX",
        extensions: &["tsx"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::Tsx),
        support: SupportLevel::Full,
        extractor_version: 1,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Astro,
        display_name: "Astro",
        extensions: &["astro"],
        parser: ParserKind::Projection {
            target: Language::Tsx,
        },
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::JavaScript,
        display_name: "JavaScript",
        extensions: &["js", "mjs", "cjs"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::JavaScript),
        support: SupportLevel::Full,
        extractor_version: 1,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Jsx,
        display_name: "JSX",
        extensions: &["jsx"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::JavaScript),
        support: SupportLevel::Full,
        extractor_version: 1,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Python,
        display_name: "Python",
        extensions: &["py"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::Python),
        support: SupportLevel::Full,
        extractor_version: 1,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Rust,
        display_name: "Rust",
        extensions: &["rs"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::Rust),
        support: SupportLevel::Full,
        extractor_version: 1,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Go,
        display_name: "Go",
        extensions: &["go"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::Go),
        support: SupportLevel::Full,
        extractor_version: 1,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Markdown,
        display_name: "Markdown",
        extensions: &["md", "markdown"],
        parser: ParserKind::MarkdownHeadings,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::markdown(),
    },
    LanguageCapability {
        language: Language::Css,
        display_name: "CSS",
        extensions: &["css"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Scss,
        display_name: "SCSS",
        extensions: &["scss"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Sass,
        display_name: "Sass",
        extensions: &["sass"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Less,
        display_name: "Less",
        extensions: &["less"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Html,
        display_name: "HTML",
        extensions: &["html", "htm"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Vue,
        display_name: "Vue",
        extensions: &["vue"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 2,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Svelte,
        display_name: "Svelte",
        extensions: &["svelte"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 2,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Json,
        display_name: "JSON",
        extensions: &["json", "jsonc"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 3,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Yaml,
        display_name: "YAML",
        extensions: &["yaml", "yml"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 4,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Toml,
        display_name: "TOML",
        extensions: &["toml"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Php,
        display_name: "PHP",
        extensions: &["php"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Java,
        display_name: "Java",
        extensions: &["java"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::CSharp,
        display_name: "C#",
        extensions: &["cs"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Kotlin,
        display_name: "Kotlin",
        extensions: &["kt", "kts"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Ruby,
        display_name: "Ruby",
        extensions: &["rb", "rake"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Cpp,
        display_name: "C/C++",
        extensions: &["c", "h", "cc", "cxx", "cpp", "hpp", "hxx"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Shell,
        display_name: "Shell",
        extensions: &["sh", "bash", "zsh", "fish"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
];

impl Language {
    /// Detect language from file path extension
    pub fn from_path(path: &str) -> Option<Self> {
        let ext = path.rsplit('.').next()?;
        Self::from_extension(ext)
    }

    pub fn from_extension(extension: &str) -> Option<Self> {
        let extension = extension.trim_start_matches('.').to_lowercase();
        LANGUAGE_CAPABILITIES
            .iter()
            .find(|capability| capability.extensions.contains(&extension.as_str()))
            .map(|capability| capability.language)
    }

    pub fn capability(self) -> &'static LanguageCapability {
        LANGUAGE_CAPABILITIES
            .iter()
            .find(|capability| capability.language == self)
            .expect("every Language variant must have a capability entry")
    }

    pub fn capability_for_path(path: &str) -> Option<&'static LanguageCapability> {
        let ext = path.rsplit('.').next()?;
        Self::from_extension(ext).map(Self::capability)
    }

    pub fn all_capabilities() -> &'static [LanguageCapability] {
        LANGUAGE_CAPABILITIES
    }

    pub fn is_stylesheet_scanner(self) -> bool {
        matches!(
            self,
            Language::Css | Language::Scss | Language::Sass | Language::Less
        )
    }

    pub fn is_markup_scanner(self) -> bool {
        matches!(self, Language::Html | Language::Vue | Language::Svelte)
    }

    pub fn is_config_scanner(self) -> bool {
        matches!(self, Language::Json | Language::Yaml | Language::Toml)
    }

    pub fn is_php_scanner(self) -> bool {
        matches!(self, Language::Php)
    }

    pub fn is_java_scanner(self) -> bool {
        matches!(self, Language::Java)
    }

    pub fn is_csharp_scanner(self) -> bool {
        matches!(self, Language::CSharp)
    }

    pub fn is_kotlin_scanner(self) -> bool {
        matches!(self, Language::Kotlin)
    }

    pub fn is_cpp_scanner(self) -> bool {
        matches!(self, Language::Cpp)
    }

    pub fn is_shell_scanner(self) -> bool {
        matches!(self, Language::Shell)
    }

    pub fn is_ruby_scanner(self) -> bool {
        matches!(self, Language::Ruby)
    }

    /// Get display name for the language
    pub fn display_name(&self) -> &'static str {
        self.capability().display_name
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

        for capability in Language::all_capabilities() {
            let ParserKind::TreeSitter(grammar) = capability.parser else {
                continue;
            };
            parsers.insert(
                capability.language,
                Self::new_parser(grammar).map_err(|e| {
                    TreeSitterError::LanguageInitFailed(format!(
                        "{}: {}",
                        capability.display_name, e
                    ))
                })?,
            );
        }

        Ok(Self { parsers })
    }

    fn new_parser(grammar: TreeSitterGrammar) -> Result<Parser, tree_sitter::LanguageError> {
        let mut parser = Parser::new();
        match grammar {
            TreeSitterGrammar::TypeScript => {
                parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?
            }
            TreeSitterGrammar::Tsx => {
                parser.set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())?
            }
            TreeSitterGrammar::JavaScript => {
                parser.set_language(&tree_sitter_javascript::LANGUAGE.into())?
            }
            TreeSitterGrammar::Python => {
                parser.set_language(&tree_sitter_python::LANGUAGE.into())?
            }
            TreeSitterGrammar::Rust => parser.set_language(&tree_sitter_rust::LANGUAGE.into())?,
            TreeSitterGrammar::Go => parser.set_language(&tree_sitter_go::LANGUAGE.into())?,
        }
        Ok(parser)
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
        assert_eq!(Language::from_path("page.astro"), Some(Language::Astro));
        assert_eq!(Language::from_path("script.js"), Some(Language::JavaScript));
        assert_eq!(Language::from_path("component.jsx"), Some(Language::Jsx));
        assert_eq!(Language::from_path("main.py"), Some(Language::Python));
        assert_eq!(Language::from_path("lib.rs"), Some(Language::Rust));
        assert_eq!(Language::from_path("main.go"), Some(Language::Go));
        assert_eq!(Language::from_path("README.md"), Some(Language::Markdown));
        assert_eq!(
            Language::from_path("docs/plan.markdown"),
            Some(Language::Markdown)
        );
        assert_eq!(Language::from_path("src/index.css"), Some(Language::Css));
        assert_eq!(
            Language::from_path("src/Button.module.css"),
            Some(Language::Css)
        );
        assert_eq!(Language::from_path("src/index.scss"), Some(Language::Scss));
        assert_eq!(
            Language::from_path("src/Button.module.scss"),
            Some(Language::Scss)
        );
        assert_eq!(Language::from_path("src/index.sass"), Some(Language::Sass));
        assert_eq!(Language::from_path("src/index.less"), Some(Language::Less));
        assert_eq!(
            Language::from_path("public/index.html"),
            Some(Language::Html)
        );
        assert_eq!(
            Language::from_path("public/index.htm"),
            Some(Language::Html)
        );
        assert_eq!(Language::from_path("src/App.vue"), Some(Language::Vue));
        assert_eq!(
            Language::from_path("src/App.svelte"),
            Some(Language::Svelte)
        );
        assert_eq!(Language::from_path("data.json"), Some(Language::Json));
        assert_eq!(Language::from_path("tsconfig.jsonc"), Some(Language::Json));
        assert_eq!(Language::from_path("config.yaml"), Some(Language::Yaml));
        assert_eq!(Language::from_path("config.yml"), Some(Language::Yaml));
        assert_eq!(Language::from_path("Cargo.toml"), Some(Language::Toml));
        assert_eq!(Language::from_path("pyproject.toml"), Some(Language::Toml));
        assert_eq!(Language::from_path("app.php"), Some(Language::Php));
        assert_eq!(Language::from_path("Main.java"), Some(Language::Java));
        assert_eq!(
            Language::from_path("UserService.cs"),
            Some(Language::CSharp)
        );
        assert_eq!(
            Language::from_path("UserService.kt"),
            Some(Language::Kotlin)
        );
        assert_eq!(
            Language::from_path("build.gradle.kts"),
            Some(Language::Kotlin)
        );
        assert_eq!(Language::from_path("user_service.rb"), Some(Language::Ruby));
        assert_eq!(
            Language::from_path("tasks/deploy.rake"),
            Some(Language::Ruby)
        );
        assert_eq!(Language::from_path("main.c"), Some(Language::Cpp));
        assert_eq!(Language::from_path("main.cpp"), Some(Language::Cpp));
        assert_eq!(Language::from_path("include/user.hpp"), Some(Language::Cpp));
        assert_eq!(
            Language::from_path("scripts/deploy.sh"),
            Some(Language::Shell)
        );
        assert_eq!(
            Language::from_path("scripts/deploy.bash"),
            Some(Language::Shell)
        );
    }

    #[test]
    fn test_language_capabilities_are_the_detection_source() {
        for capability in Language::all_capabilities() {
            assert_eq!(capability.language.display_name(), capability.display_name);
            for extension in capability.extensions {
                assert_eq!(
                    Language::from_extension(extension),
                    Some(capability.language),
                    "extension {extension} should resolve through the capability registry"
                );
                assert_eq!(
                    Language::capability_for_path(&format!("file.{extension}")),
                    Some(capability)
                );
            }
        }
    }

    #[test]
    fn test_non_tree_sitter_capability_strategies() {
        let astro = Language::Astro.capability();
        assert_eq!(
            astro.parser,
            ParserKind::Projection {
                target: Language::Tsx
            }
        );
        assert_eq!(astro.support, SupportLevel::Partial);

        let markdown = Language::Markdown.capability();
        assert_eq!(markdown.parser, ParserKind::MarkdownHeadings);
        assert_eq!(markdown.support, SupportLevel::Partial);
        assert!(markdown.extracts.markdown_headings);

        let css = Language::Css.capability();
        assert_eq!(css.parser, ParserKind::Scanner);
        assert_eq!(css.support, SupportLevel::Partial);
        assert!(css.extracts.definitions);

        for language in [Language::Scss, Language::Sass, Language::Less] {
            let capability = language.capability();
            assert_eq!(capability.parser, ParserKind::Scanner);
            assert_eq!(capability.support, SupportLevel::Partial);
            assert!(capability.extracts.definitions);
            assert!(language.is_stylesheet_scanner());
        }

        let html = Language::Html.capability();
        assert_eq!(html.parser, ParserKind::Scanner);
        assert_eq!(html.support, SupportLevel::Partial);
        assert!(html.extracts.definitions);
        assert!(Language::Html.is_markup_scanner());

        for language in [Language::Vue, Language::Svelte] {
            let capability = language.capability();
            assert_eq!(capability.parser, ParserKind::Scanner);
            assert_eq!(capability.support, SupportLevel::Partial);
            assert!(capability.extracts.definitions);
            assert!(language.is_markup_scanner());
        }

        for language in [Language::Json, Language::Yaml, Language::Toml] {
            let capability = language.capability();
            assert_eq!(capability.parser, ParserKind::Scanner);
            assert_eq!(capability.support, SupportLevel::Partial);
            assert!(capability.extracts.definitions);
            assert!(language.is_config_scanner());
        }
    }

    #[test]
    fn test_parser_manager_initializes_only_tree_sitter_backed_languages() {
        let parser = TreeSitterParser::new().unwrap();
        for capability in Language::all_capabilities() {
            let expected = matches!(capability.parser, ParserKind::TreeSitter(_));
            assert_eq!(
                parser.supports_language(capability.language),
                expected,
                "{} parser availability should match capability strategy",
                capability.display_name
            );
        }
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
    fn test_parse_go() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "package main\n\nfunc greet(name string) string { return \"hi \" + name }";
        let tree = parser.parse(code, Language::Go).unwrap();

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
