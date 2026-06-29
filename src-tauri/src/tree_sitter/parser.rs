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
    Dockerfile,
    Sql,
    BuildScript,
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
    Cpp,
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
        extractor_version: 2,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Tsx,
        display_name: "TSX",
        extensions: &["tsx"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::Tsx),
        support: SupportLevel::Full,
        extractor_version: 2,
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
        extractor_version: 2,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::JavaScript,
        display_name: "JavaScript",
        extensions: &["js", "mjs", "cjs"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::JavaScript),
        support: SupportLevel::Full,
        extractor_version: 2,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Jsx,
        display_name: "JSX",
        extensions: &["jsx"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::JavaScript),
        support: SupportLevel::Full,
        extractor_version: 2,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Python,
        display_name: "Python",
        extensions: &["py"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::Python),
        support: SupportLevel::Full,
        extractor_version: 2,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Rust,
        display_name: "Rust",
        extensions: &["rs"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::Rust),
        support: SupportLevel::Full,
        extractor_version: 2,
        extracts: ExtractionCapabilities::code(true, true, true),
    },
    LanguageCapability {
        language: Language::Go,
        display_name: "Go",
        extensions: &["go"],
        parser: ParserKind::TreeSitter(TreeSitterGrammar::Go),
        support: SupportLevel::Full,
        extractor_version: 2,
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
        // M5.3: graduated from the regex line-scanner to the real `tree-sitter-cpp`
        // grammar. DEFINITIONS-ONLY (N8): no call graph / relationships until
        // source-attribution fixtures exist — so `relationships: false`, which the
        // N8 gate in `extract_symbol_relationships` reads to skip the whole edge
        // walk. Stays `Partial` (Full is reserved for defs+imports+relationships).
        parser: ParserKind::TreeSitter(TreeSitterGrammar::Cpp),
        support: SupportLevel::Partial,
        extractor_version: 2,
        extracts: ExtractionCapabilities::code(true, false, false),
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
    LanguageCapability {
        language: Language::Dockerfile,
        display_name: "Dockerfile",
        extensions: &["dockerfile"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::Sql,
        display_name: "SQL",
        extensions: &["sql", "psql"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
    LanguageCapability {
        language: Language::BuildScript,
        display_name: "Make/CMake",
        extensions: &["mk", "make", "cmake"],
        parser: ParserKind::Scanner,
        support: SupportLevel::Partial,
        extractor_version: 1,
        extracts: ExtractionCapabilities::scanner(true),
    },
];

/// Exact (case-sensitive) basenames mapped to the best-fitting existing
/// `Language`. This is the finite, committed set from milestone M1.3 — no
/// open-ended "etc."; later additions go through the plan's promotion list.
///
/// Each target is an existing language whose downstream scanner never errors on
/// foreign content (config scanners decline non-matching input and semantic
/// anchors are extracted regardless), so every entry detects-and-produces at
/// least anchors. See `M1.3` notes in the report for the per-entry rationale:
///   - `.env` / `requirements*.txt` → Toml: line `key[=...]` extraction yields
///     real env-var / dependency-name symbols.
///   - `go.mod` / `Kconfig` → Toml: no `=`-bearing lines, so anchors only (no
///     false-positive symbols).
///   - `go.sum` → Json: its `module ver hash=` lines would mis-parse under the
///     Toml/Yaml key scanners; serde_json simply declines → anchors only.
///   - `kustomization.yaml` → Yaml: it is YAML; full config-key extraction.
///   - Bazel `BUILD` / `BUILD.bazel` / `WORKSPACE` → Python: Starlark is a
///     Python dialect that tree-sitter-python parses safely.
const FILENAME_TABLE: &[(&str, Language)] = &[
    ("go.mod", Language::Toml),
    ("go.sum", Language::Json),
    ("requirements.txt", Language::Toml),
    ("requirements-dev.txt", Language::Toml),
    (".env", Language::Toml),
    ("kustomization.yaml", Language::Yaml),
    ("BUILD", Language::Python),
    ("BUILD.bazel", Language::Python),
    ("WORKSPACE", Language::Python),
    ("Kconfig", Language::Toml),
];

/// Compound (multi-dot) extension suffixes. Scanned first-dot→last-dot so the
/// most specific suffix wins, and consulted *before* the plain last-extension
/// fallback. `.blade.php` (Laravel Blade) has no dedicated grammar, so it maps
/// to the closest existing variant (PHP), matching today's effective behaviour
/// but now as an explicit, stable rule.
const COMPOUND_EXT_TABLE: &[(&str, Language)] = &[("blade.php", Language::Php)];

impl Language {
    /// Detect language from a file path (filename table → compound extension →
    /// plain last-extension). Pure path-based; for content-aware detection of
    /// extensionless files use [`Language::detect_with_head`].
    pub fn from_path(path: &str) -> Option<Self> {
        let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);

        // 1. Exact filename table (case-sensitive: Bazel `BUILD`/`WORKSPACE`,
        //    `Kconfig`, `.env`, …).
        if let Some(language) = FILENAME_TABLE
            .iter()
            .find(|(name, _)| *name == file_name)
            .map(|(_, language)| *language)
        {
            return Some(language);
        }

        let file_name_lower = file_name.to_lowercase();
        if file_name_lower == "dockerfile" || file_name_lower.starts_with("dockerfile.") {
            return Some(Language::Dockerfile);
        }
        if matches!(file_name_lower.as_str(), "makefile" | "gnumakefile")
            || file_name_lower == "cmakelists.txt"
        {
            return Some(Language::BuildScript);
        }

        // 2. Compound extension table (first-dot→last-dot) before the plain
        //    last-extension fallback, so `.blade.php` wins over `.php`.
        if let Some(language) = Self::from_compound_extension(&file_name_lower) {
            return Some(language);
        }

        // 3. Plain last-extension fallback.
        let ext = path.rsplit('.').next()?;
        Self::from_extension(ext)
    }

    /// Walk each `.`-delimited boundary from the first dot toward the last,
    /// returning the first matching compound suffix (longest/most-specific
    /// first) from [`COMPOUND_EXT_TABLE`].
    fn from_compound_extension(file_name_lower: &str) -> Option<Self> {
        let mut search = file_name_lower;
        while let Some(dot) = search.find('.') {
            let suffix = &search[dot + 1..];
            if let Some(language) = COMPOUND_EXT_TABLE
                .iter()
                .find(|(ext, _)| *ext == suffix)
                .map(|(_, language)| *language)
            {
                return Some(language);
            }
            search = suffix;
        }
        None
    }

    /// Detect language using the path first; if the path can't classify the
    /// file (e.g. an extensionless script), fall back to a shebang sniff of the
    /// file's leading bytes. Pure-path [`Language::from_path`] remains the
    /// primary path and is tried first.
    pub fn detect_with_head(path: &str, head: &[u8]) -> Option<Self> {
        if let Some(language) = Self::from_path(path) {
            return Some(language);
        }
        let first_line_end = head.iter().position(|&b| b == b'\n').unwrap_or(head.len());
        let first_line = String::from_utf8_lossy(&head[..first_line_end]);
        Self::detect_by_shebang(&first_line)
    }

    /// Map a `#!` shebang line to a `Language`, taking the interpreter basename
    /// (or the token after `env`). Only interpreters with an existing
    /// `Language` are mapped.
    pub fn detect_by_shebang(head: &str) -> Option<Self> {
        let first_line = head.lines().next()?;
        let rest = first_line.strip_prefix("#!")?;

        let mut tokens = rest.split_whitespace();
        let first = tokens.next()?;
        let mut interpreter = first.rsplit(['/', '\\']).next().unwrap_or(first);

        // `#!/usr/bin/env [-S] [VAR=val ...] python3 -u` → real interpreter is
        // the first token after `env` that is neither a flag nor an assignment.
        if interpreter == "env" {
            interpreter = tokens
                .find(|token| !token.starts_with('-') && !token.contains('='))?;
            interpreter = interpreter.rsplit(['/', '\\']).next().unwrap_or(interpreter);
        }

        Self::interpreter_to_language(interpreter)
    }

    /// Map an interpreter basename (e.g. `python3.11`, `bash`, `node`) to an
    /// existing `Language`.
    fn interpreter_to_language(interpreter: &str) -> Option<Self> {
        let lower = interpreter.to_ascii_lowercase();
        if lower.starts_with("python") {
            return Some(Language::Python);
        }
        if lower.starts_with("php") {
            return Some(Language::Php);
        }
        match lower.as_str() {
            "bash" | "sh" | "zsh" | "fish" | "dash" | "ksh" | "ash" => Some(Language::Shell),
            "node" | "nodejs" => Some(Language::JavaScript),
            "ruby" | "jruby" => Some(Language::Ruby),
            _ => None,
        }
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
        Self::from_path(path).map(Self::capability)
    }

    pub fn all_capabilities() -> &'static [LanguageCapability] {
        LANGUAGE_CAPABILITIES
    }

    pub fn all_extensions() -> impl Iterator<Item = &'static str> {
        LANGUAGE_CAPABILITIES
            .iter()
            .flat_map(|capability| capability.extensions.iter().copied())
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

    pub fn is_dockerfile_scanner(self) -> bool {
        matches!(self, Language::Dockerfile)
    }

    pub fn is_sql_scanner(self) -> bool {
        matches!(self, Language::Sql)
    }

    pub fn is_build_script_scanner(self) -> bool {
        matches!(self, Language::BuildScript)
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
            TreeSitterGrammar::Cpp => parser.set_language(&tree_sitter_cpp::LANGUAGE.into())?,
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
        assert_eq!(
            Language::from_path("Dockerfile"),
            Some(Language::Dockerfile)
        );
        assert_eq!(
            Language::from_path("docker/Dockerfile"),
            Some(Language::Dockerfile)
        );
        assert_eq!(
            Language::from_path("Dockerfile.prod"),
            Some(Language::Dockerfile)
        );
        assert_eq!(Language::from_path("schema.sql"), Some(Language::Sql));
        assert_eq!(Language::from_path("migration.psql"), Some(Language::Sql));
        assert_eq!(Language::from_path("Makefile"), Some(Language::BuildScript));
        assert_eq!(
            Language::from_path("GNUmakefile"),
            Some(Language::BuildScript)
        );
        assert_eq!(
            Language::from_path("CMakeLists.txt"),
            Some(Language::BuildScript)
        );
        assert_eq!(Language::from_path("rules.mk"), Some(Language::BuildScript));
        assert_eq!(
            Language::from_path("cmake/helpers.cmake"),
            Some(Language::BuildScript)
        );
    }

    #[test]
    fn file_type_detection_filename_table_resolves_invisible_files() {
        // Each committed FILENAME_TABLE entry must resolve to its mapped
        // existing language (and therefore become a supported, indexable file).
        let cases = [
            ("go.mod", Language::Toml, SupportLevel::Partial),
            ("path/to/go.mod", Language::Toml, SupportLevel::Partial),
            ("go.sum", Language::Json, SupportLevel::Partial),
            ("requirements.txt", Language::Toml, SupportLevel::Partial),
            ("requirements-dev.txt", Language::Toml, SupportLevel::Partial),
            (".env", Language::Toml, SupportLevel::Partial),
            ("config/.env", Language::Toml, SupportLevel::Partial),
            ("kustomization.yaml", Language::Yaml, SupportLevel::Partial),
            ("BUILD", Language::Python, SupportLevel::Full),
            ("src/BUILD", Language::Python, SupportLevel::Full),
            ("BUILD.bazel", Language::Python, SupportLevel::Full),
            ("WORKSPACE", Language::Python, SupportLevel::Full),
            ("Kconfig", Language::Toml, SupportLevel::Partial),
        ];
        for (path, expected, support) in cases {
            assert_eq!(
                Language::from_path(path),
                Some(expected),
                "filename {path} should resolve to {expected:?}"
            );
            assert_eq!(
                Language::capability_for_path(path).map(|capability| capability.support),
                Some(support),
                "filename {path} should report support level {support:?}"
            );
        }
    }

    #[test]
    fn file_type_detection_filename_table_is_case_sensitive_for_bazel_names() {
        // Bazel/Kconfig names are intentionally case-sensitive so a lowercase
        // `build` or `workspace` does not get misclassified as Starlark.
        assert_eq!(Language::from_path("BUILD"), Some(Language::Python));
        assert_eq!(Language::from_path("build"), None);
        assert_eq!(Language::from_path("workspace"), None);
        assert_eq!(Language::from_path("kconfig"), None);
    }

    #[test]
    fn file_type_detection_compound_extension_wins_over_last_extension() {
        assert_eq!(
            Language::from_path("resources/views/welcome.blade.php"),
            Some(Language::Php)
        );
        assert_eq!(Language::from_path("welcome.blade.php"), Some(Language::Php));
        // A plain `.php` still resolves through the last-extension fallback.
        assert_eq!(Language::from_path("app.php"), Some(Language::Php));
        // Unrelated compound extensions still fall through to the last ext.
        assert_eq!(
            Language::from_path("src/Button.module.css"),
            Some(Language::Css)
        );
    }

    #[test]
    fn file_type_detection_shebang_resolves_extensionless_scripts() {
        let cases = [
            ("#!/usr/bin/env python3\nprint('hi')\n", Language::Python),
            ("#!/usr/bin/python\n", Language::Python),
            ("#!/usr/bin/env python3.11 -u\n", Language::Python),
            ("#!/bin/bash\necho hi\n", Language::Shell),
            ("#!/bin/sh\n", Language::Shell),
            ("#!/usr/bin/env zsh\n", Language::Shell),
            ("#!/usr/bin/env node\n", Language::JavaScript),
            ("#!/usr/bin/env -S node --flag\n", Language::JavaScript),
            ("#!/usr/bin/ruby\n", Language::Ruby),
            ("#!/usr/bin/env php\n", Language::Php),
        ];
        for (head, expected) in cases {
            assert_eq!(
                Language::detect_by_shebang(head),
                Some(expected),
                "shebang {head:?} should resolve to {expected:?}"
            );
            // detect_with_head: path gives nothing (extensionless), head wins.
            assert_eq!(
                Language::detect_with_head("scripts/run", head.as_bytes()),
                Some(expected),
                "detect_with_head for {head:?} should resolve to {expected:?}"
            );
        }
    }

    #[test]
    fn file_type_detection_shebang_ignored_when_path_is_classifiable() {
        // A real extension/filename must win over the shebang sniff.
        assert_eq!(
            Language::detect_with_head("main.rs", b"#!/usr/bin/env python3\n"),
            Some(Language::Rust)
        );
        // No shebang and an unknown extensionless name → still unknown.
        assert_eq!(Language::detect_with_head("LICENSE", b"MIT License\n"), None);
        assert_eq!(Language::detect_by_shebang("not a shebang"), None);
        assert_eq!(Language::detect_by_shebang("#!/usr/bin/env weirdlang"), None);
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

        let all_extensions = Language::all_extensions().collect::<Vec<_>>();
        assert!(all_extensions.contains(&"ts"));
        assert!(all_extensions.contains(&"css"));
        assert!(all_extensions.contains(&"php"));
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
