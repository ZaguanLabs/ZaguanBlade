//! Symbol extraction from parsed AST trees
//!
//! Extracts semantic symbols (functions, classes, methods, etc.) from
//! tree-sitter AST trees for indexing and context assembly.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Tree};

use super::parser::Language;

/// Types of symbols we extract from code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolType {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Type,
    Enum,
    EnumMember,
    Constant,
    Variable,
    Property,
    Module,
    Namespace,
    Import,
    Export,
    Trait,
    Impl,
    Heading,
    CssSelector,
    CssCustomProperty,
    CssKeyframes,
    CssAtRule,
    CssLayer,
    CssFontFace,
}

impl std::fmt::Display for SymbolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SymbolType::Function => "function",
            SymbolType::Method => "method",
            SymbolType::Class => "class",
            SymbolType::Struct => "struct",
            SymbolType::Interface => "interface",
            SymbolType::Type => "type",
            SymbolType::Enum => "enum",
            SymbolType::EnumMember => "enum_member",
            SymbolType::Constant => "constant",
            SymbolType::Variable => "variable",
            SymbolType::Property => "property",
            SymbolType::Module => "module",
            SymbolType::Namespace => "namespace",
            SymbolType::Import => "import",
            SymbolType::Export => "export",
            SymbolType::Trait => "trait",
            SymbolType::Impl => "impl",
            SymbolType::Heading => "heading",
            SymbolType::CssSelector => "css_selector",
            SymbolType::CssCustomProperty => "css_custom_property",
            SymbolType::CssKeyframes => "css_keyframes",
            SymbolType::CssAtRule => "css_at_rule",
            SymbolType::CssLayer => "css_layer",
            SymbolType::CssFontFace => "css_font_face",
        };
        write!(f, "{}", s)
    }
}

fn extract_import_relationships(
    file_path: &str,
    symbols: &[Symbol],
    relationships: &mut Vec<SymbolRelationship>,
    seen: &mut HashSet<(String, String, SymbolRelationshipType, u32)>,
) {
    for symbol in symbols {
        if symbol.symbol_type != SymbolType::Import || symbol.name.is_empty() {
            continue;
        }

        let key = (
            symbol.id.clone(),
            symbol.name.clone(),
            SymbolRelationshipType::Import,
            symbol.range.start.line,
        );

        if seen.insert(key) {
            relationships.push(SymbolRelationship {
                source_symbol_id: symbol.id.clone(),
                source_file_path: file_path.to_string(),
                target_name: symbol.name.clone(),
                target_symbol_id: None,
                relationship_type: SymbolRelationshipType::Import,
                line: symbol.range.start.line,
            });
        }
    }
}

impl std::str::FromStr for SymbolType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "function" => Ok(SymbolType::Function),
            "method" => Ok(SymbolType::Method),
            "class" => Ok(SymbolType::Class),
            "struct" => Ok(SymbolType::Struct),
            "interface" => Ok(SymbolType::Interface),
            "type" => Ok(SymbolType::Type),
            "enum" => Ok(SymbolType::Enum),
            "enum_member" => Ok(SymbolType::EnumMember),
            "constant" => Ok(SymbolType::Constant),
            "variable" => Ok(SymbolType::Variable),
            "property" => Ok(SymbolType::Property),
            "module" => Ok(SymbolType::Module),
            "namespace" => Ok(SymbolType::Namespace),
            "import" => Ok(SymbolType::Import),
            "export" => Ok(SymbolType::Export),
            "trait" => Ok(SymbolType::Trait),
            "impl" => Ok(SymbolType::Impl),
            "heading" => Ok(SymbolType::Heading),
            "css_selector" => Ok(SymbolType::CssSelector),
            "css_custom_property" => Ok(SymbolType::CssCustomProperty),
            "css_keyframes" => Ok(SymbolType::CssKeyframes),
            "css_at_rule" => Ok(SymbolType::CssAtRule),
            "css_layer" => Ok(SymbolType::CssLayer),
            "css_font_face" => Ok(SymbolType::CssFontFace),
            _ => Err(format!("Unknown symbol type: {}", s)),
        }
    }
}

/// Position in source code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// Range in source code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn from_node(node: &Node) -> Self {
        let start = node.start_position();
        let end = node.end_position();
        Self {
            start: Position::new(start.row as u32, start.column as u32),
            end: Position::new(end.row as u32, end.column as u32),
        }
    }
}

/// A symbol extracted from source code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Unique identifier (UUID v4)
    pub id: String,
    /// Symbol name (e.g., function name, class name)
    pub name: String,
    /// Qualified symbol name
    pub qualified_name: String,
    /// Type of symbol
    pub symbol_type: SymbolType,
    /// File path where symbol is defined
    pub file_path: String,
    /// Range in source code
    pub range: Range,
    /// Byte offset in the containing file
    pub byte_offset: usize,
    /// Byte length in the containing file
    pub byte_length: usize,
    /// Parent symbol ID (for methods inside classes, etc.)
    pub parent_id: Option<String>,
    /// Documentation string if present
    pub docstring: Option<String>,
    /// Type signature (for functions: parameters and return type)
    pub signature: Option<String>,
    /// Content hash for the symbol span
    pub content_hash: String,
}

impl Symbol {
    pub fn new(name: String, symbol_type: SymbolType, file_path: String, range: Range) -> Self {
        Self {
            id: String::new(),
            name,
            qualified_name: String::new(),
            symbol_type,
            file_path,
            range,
            byte_offset: 0,
            byte_length: 0,
            parent_id: None,
            docstring: None,
            signature: None,
            content_hash: String::new(),
        }
    }

    pub fn with_parent(mut self, parent_id: String) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    pub fn with_docstring(mut self, docstring: String) -> Self {
        self.docstring = Some(docstring);
        self
    }

    pub fn with_signature(mut self, signature: String) -> Self {
        self.signature = Some(signature);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolRelationshipType {
    Call,
    Import,
    Export,
    Extends,
    Implements,
    Contains,
    Usage,
    /// A function/method references a TYPE in its signature (parameter or return
    /// type). Source is the enclosing function/method symbol; `target_name` is the
    /// base type name (M4.1). Stored as the TEXT value `"uses_type"`.
    UsesType,
    /// A function/method reads an ENVIRONMENT-VARIABLE KEY (e.g. `std::env::var`,
    /// `os.environ[...]`, `process.env.X`, `os.Getenv`). Source is the enclosing
    /// function/method symbol; `target_name` is the KEY string (NOT a symbol, so
    /// `target_symbol_id` stays NULL) (M4.2). Stored as the TEXT value
    /// `"reads_env"`.
    ReadsEnv,
}

impl std::fmt::Display for SymbolRelationshipType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SymbolRelationshipType::Call => "call",
            SymbolRelationshipType::Import => "import",
            SymbolRelationshipType::Export => "export",
            SymbolRelationshipType::Extends => "extends",
            SymbolRelationshipType::Implements => "implements",
            SymbolRelationshipType::Contains => "contains",
            SymbolRelationshipType::Usage => "usage",
            SymbolRelationshipType::UsesType => "uses_type",
            SymbolRelationshipType::ReadsEnv => "reads_env",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for SymbolRelationshipType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "call" => Ok(SymbolRelationshipType::Call),
            "import" => Ok(SymbolRelationshipType::Import),
            "export" => Ok(SymbolRelationshipType::Export),
            "extends" => Ok(SymbolRelationshipType::Extends),
            "implements" => Ok(SymbolRelationshipType::Implements),
            "contains" => Ok(SymbolRelationshipType::Contains),
            "usage" | "uses" => Ok(SymbolRelationshipType::Usage),
            "uses_type" => Ok(SymbolRelationshipType::UsesType),
            "reads_env" => Ok(SymbolRelationshipType::ReadsEnv),
            _ => Err(format!("Unknown relationship type: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRelationship {
    pub source_symbol_id: String,
    pub source_file_path: String,
    pub target_name: String,
    pub target_symbol_id: Option<String>,
    pub relationship_type: SymbolRelationshipType,
    pub line: u32,
}

/// Symbol extractor for extracting symbols from AST trees
pub struct SymbolExtractor {
    file_path: String,
}

/// One frame of the unified-walk scope stack: the enclosing symbol's identity.
///
/// A single frame carries BOTH concerns the old code threaded separately:
/// - what `find_enclosing_symbol` needed — this symbol's own `id` + `range`
///   (used to attribute relationships to their innermost enclosing symbol);
/// - what the old `parent_context` threading needed — the context handed to
///   this symbol's children (`child_id` / `child_qn` / `child_sep`), so a child's
///   `parent_id` and `qualified_name` reproduce exactly what was composed before.
///
/// `Arc<str>` lets a frame's id/qn be cloned out cheaply for relationship
/// sources and child composition, replacing the per-child `String` clones the
/// recursive `parent_context.clone()` used to pay.
struct Scope {
    /// This symbol's own id (relationship source / parent linkage of children).
    id: Arc<str>,
    /// This symbol's own range (enclosing-symbol containment + tie-break key).
    range: Range,
    /// Parent-id handed to this scope's children.
    child_id: Arc<str>,
    /// Qualified-name prefix handed to this scope's children.
    child_qn: Arc<str>,
    /// Separator joining `child_qn` to a child's name.
    /// `.` for most nesting; `::` for Rust `impl`/method paths (`Type::method`).
    child_sep: &'static str,
}

/// The child-naming context a symbol hands down to its descendants. The default
/// nests under the symbol itself with a `.`; a Rust `impl` block remaps it to the
/// implemented type with `::` (so methods read `Type::method`).
struct ChildCtx {
    id: Arc<str>,
    qn: Arc<str>,
    sep: &'static str,
}

/// Mutable state threaded through the single unified DFS.
struct WalkState {
    /// Stack of enclosing symbol scopes (outermost at index 0, innermost on top).
    /// Pushed on entering a symbol-creating node, popped on leaving it.
    scope: Vec<Scope>,
    /// Symbols produced so far, in pre-order. This IS the output of the symbol
    /// walk; it is also consulted live for the Rust-`impl` child-context lookup,
    /// which must see only symbols-so-far (matching the original pass).
    symbols: Vec<Symbol>,
}

/// Relationship-walk sink, threaded alongside `WalkState` when the unified DFS
/// runs in relationship mode.
struct RelState<'a> {
    relationships: Vec<SymbolRelationship>,
    /// De-dup key shared across the call, import, and structural concerns
    /// (matching the original single shared `seen` set).
    seen: HashSet<(String, String, SymbolRelationshipType, u32)>,
    /// The full, already-computed symbol set for the file. Needed for the
    /// forward-reference structural lookups (Rust `impl` / Go receiver +
    /// embedding via `find_symbol_by_name`), which may target a type declared
    /// later in the file. Call/structural-TS/Python attribution instead uses the
    /// live scope stack in `WalkState` (the innermost enclosing symbol).
    all_symbols: &'a [Symbol],
    /// Module-level `NAME = "string literal"` bindings, precomputed once for the
    /// whole file (M4.2 constant propagation). When an env accessor's KEY arg is a
    /// BARE IDENTIFIER rather than a string literal, its name is looked up here to
    /// recover the literal (e.g. `const KEY = "DATABASE_URL"; env::var(KEY)`).
    /// Single-file / scope-agnostic heuristic — no dataflow.
    const_map: HashMap<String, String>,
}

impl SymbolExtractor {
    pub fn new(file_path: String) -> Self {
        Self { file_path }
    }

    /// Extract all symbols from a tree via the single unified DFS (symbol mode).
    pub fn extract(&self, tree: &Tree, source: &str, language: Language) -> Vec<Symbol> {
        let mut state = WalkState {
            scope: Vec::new(),
            symbols: Vec::new(),
        };
        self.walk_symbols(tree.root_node(), source, language, &mut state);
        state.symbols
    }

    /// One pre-order DFS that produces symbols, threading the scope stack.
    /// (The relationship concerns are layered onto this same shape in
    /// `extract_symbol_relationships`'s walk; this one carries only symbols.)
    fn walk_symbols(&self, node: Node, source: &str, language: Language, state: &mut WalkState) {
        let pushed = self.enter_symbol_scope(&node, source, language, state);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_symbols(child, source, language, state);
        }

        if pushed {
            state.scope.pop();
        }
    }

    /// If `node` creates a symbol, finalize it (qualified_name / parent_id from
    /// the scope-stack top, plus docstring / signature / spans / hash), append it
    /// to `state.symbols`, and push its `Scope`. Returns whether a scope was
    /// pushed (so the caller knows to pop on the way out).
    ///
    /// This reproduces the old `parent_context` threading exactly: a child's
    /// `parent_id`/`qualified_name` are composed from the innermost scope's
    /// child-context, and the Rust-`impl` remap to the implemented type is
    /// preserved (members read `Type::method`, parented to the type symbol).
    fn enter_symbol_scope(
        &self,
        node: &Node,
        source: &str,
        language: Language,
        state: &mut WalkState,
    ) -> bool {
        let Some(mut symbol) = self.node_to_symbol(node, source, language) else {
            return false;
        };

        // Compose qualified_name + parent_id from the enclosing scope's
        // child-context (the top of the stack), or treat as a root symbol.
        let (parent_id, qualified_name) = match state.scope.last() {
            Some(parent) => (
                Some(parent.child_id.to_string()),
                format!("{}{}{}", parent.child_qn, parent.child_sep, symbol.name),
            ),
            None => (None, symbol.name.clone()),
        };
        symbol.parent_id = parent_id;

        // Try to extract docstring
        if let Some(doc) = self.extract_docstring(node, source, language) {
            symbol.docstring = Some(doc);
        }

        // Try to extract signature
        if let Some(sig) = self.extract_signature(node, source, language) {
            symbol.signature = Some(sig);
        }

        symbol.byte_offset = node.start_byte();
        symbol.byte_length = node.end_byte().saturating_sub(node.start_byte());
        symbol.content_hash = node
            .utf8_text(source.as_bytes())
            .ok()
            .map(compute_content_hash)
            .unwrap_or_default();
        symbol.qualified_name = qualified_name.clone();
        symbol.id = stable_symbol_id(&self.file_path, &qualified_name, symbol.symbol_type);

        let id: Arc<str> = Arc::from(symbol.id.as_str());
        let range = symbol.range;
        state.symbols.push(symbol);

        // Compose the context handed to this node's children. For a Rust `impl`
        // block the members inside bind to the *implemented type* (`Point::new`),
        // not to the synthetic `impl Point` node.
        let child = self.child_ctx(node, source, language, &id, &qualified_name, &state.symbols);
        state.scope.push(Scope {
            id,
            range,
            child_id: child.id,
            child_qn: child.qn,
            child_sep: child.sep,
        });
        true
    }

    /// Build the child-naming context a symbol node hands to its children.
    ///
    /// The default simply nests under the current symbol with a `.` separator.
    /// Rust `impl` blocks are special-cased: their members bind to the
    /// implemented type symbol with a `::` separator so methods read as
    /// `Type::method` and parent to the struct/enum/trait being implemented.
    /// The implemented-type lookup scans `symbols` (symbols-so-far) exactly as
    /// before, so a type declared *after* its `impl` still falls back to self.
    fn child_ctx(
        &self,
        node: &Node,
        source: &str,
        language: Language,
        self_id: &Arc<str>,
        self_qualified_name: &str,
        symbols: &[Symbol],
    ) -> ChildCtx {
        if language == Language::Rust && node.kind() == "impl_item" {
            if let Some(type_name) = node
                .child_by_field_name("type")
                .and_then(|type_node| type_node.utf8_text(source.as_bytes()).ok())
                .and_then(normalize_reference_name)
            {
                let id: Arc<str> = symbols
                    .iter()
                    .find(|symbol| {
                        symbol.name == type_name && is_rust_type_symbol(symbol.symbol_type)
                    })
                    .map(|symbol| Arc::from(symbol.id.as_str()))
                    .unwrap_or_else(|| self_id.clone());
                return ChildCtx {
                    id,
                    qn: Arc::from(type_name.as_str()),
                    sep: "::",
                };
            }
        }

        ChildCtx {
            id: self_id.clone(),
            qn: Arc::from(self_qualified_name),
            sep: ".",
        }
    }

    /// One pre-order DFS that produces relationships. It rebuilds the SAME scope
    /// stack as the symbol walk (via `enter_symbol_scope`, so the re-derived
    /// symbol ids are byte-identical), then runs the relationship concerns per
    /// node off that stack. The re-derived `state.symbols` is scratch here; the
    /// returned edges live in `rel`.
    fn walk_relationships(
        &self,
        node: Node,
        source: &str,
        language: Language,
        state: &mut WalkState,
        rel: &mut RelState,
    ) {
        let pushed = self.enter_symbol_scope(&node, source, language, state);

        // Call / macro-call concern: attribute to the innermost enclosing symbol.
        self.process_call_relationship(&node, source, language, state, rel);
        // `reads_env` concern (M4.2): body-level env accessors (`std::env::var`,
        // `os.environ[...]`, `process.env.X`, `os.Getenv`, …). Like calls, these
        // live anywhere in a body, so this fires on EVERY node (not gated on
        // `pushed`) and attributes to the innermost enclosing symbol.
        self.process_env_access_relationship(&node, source, language, state, rel);
        // Structural concern: extends / implements / contains. Runs AFTER the
        // scope push so a class node resolves to its own symbol (reproducing the
        // old `find_enclosing_symbol(class_range)` self-match).
        self.process_structural_relationship(node, source, language, state, rel);
        // `uses_type` concern (M4.1): when this node just created a
        // function/method symbol, emit an edge to each TYPE named in its
        // signature (parameter + return types). Gated on `pushed` so we only fire
        // for the symbol-creating node, and on symbol_type inside the handler.
        if pushed {
            self.process_uses_type_relationship(&node, source, language, state, rel);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_relationships(child, source, language, state, rel);
        }

        if pushed {
            state.scope.pop();
        }
    }

    /// Dispatch the structural-relationship concern for this node. TS/Python
    /// attribute to the innermost enclosing symbol via the live scope stack
    /// (the class itself once its scope is pushed); Rust `impl` / Go use the full
    /// `all_symbols` set via `find_symbol_by_name` (the implemented/receiver type
    /// may be declared later in the file — a true forward reference).
    fn process_structural_relationship(
        &self,
        node: Node,
        source: &str,
        language: Language,
        state: &WalkState,
        rel: &mut RelState,
    ) {
        match language {
            Language::TypeScript
            | Language::Tsx
            | Language::Astro
            | Language::JavaScript
            | Language::Jsx => extract_typescript_structural_relationships(
                node,
                source,
                &self.file_path,
                &state.scope,
                &mut rel.relationships,
                &mut rel.seen,
            ),
            Language::Python => extract_python_structural_relationships(
                node,
                source,
                &self.file_path,
                &state.scope,
                &mut rel.relationships,
                &mut rel.seen,
            ),
            Language::Rust => extract_rust_structural_relationships(
                node,
                source,
                &self.file_path,
                rel.all_symbols,
                &mut rel.relationships,
                &mut rel.seen,
            ),
            Language::Go => extract_go_structural_relationships(
                node,
                source,
                &self.file_path,
                rel.all_symbols,
                &mut rel.relationships,
                &mut rel.seen,
            ),
            Language::Markdown
            | Language::Css
            | Language::Scss
            | Language::Sass
            | Language::Less
            | Language::Html
            | Language::Vue
            | Language::Svelte
            | Language::Json
            | Language::Yaml
            | Language::Toml
            | Language::Php
            | Language::Java
            | Language::CSharp
            | Language::Kotlin
            | Language::Ruby
            | Language::Cpp
            | Language::Shell
            | Language::Dockerfile
            | Language::Sql
            | Language::BuildScript => {}
        }
    }

    /// Emit a `Call` edge for a call/macro-invocation node, sourced from the
    /// innermost enclosing symbol on the scope stack. Reproduces the old
    /// `extract_relationships_from_node` + `find_enclosing_symbol` pairing.
    fn process_call_relationship(
        &self,
        node: &Node,
        source: &str,
        language: Language,
        state: &WalkState,
        rel: &mut RelState,
    ) {
        let Some(target_name) = extract_relationship_target_name(node, source, language) else {
            return;
        };
        let range = Range::from_node(node);
        let Some(source_id) = resolve_enclosing_scope(&state.scope, &range).map(|s| s.id.to_string())
        else {
            return;
        };
        let line = node.start_position().row as u32;
        let key = (
            source_id.clone(),
            target_name.clone(),
            SymbolRelationshipType::Call,
            line,
        );
        if rel.seen.insert(key) {
            rel.relationships.push(SymbolRelationship {
                source_symbol_id: source_id,
                source_file_path: self.file_path.clone(),
                target_name,
                target_symbol_id: None,
                relationship_type: SymbolRelationshipType::Call,
                line,
            });
        }
    }

    /// Emit a `ReadsEnv` edge (M4.2) when `node` is an environment-variable
    /// accessor (`std::env::var("KEY")`, `os.environ["KEY"]`, `os.getenv("KEY")`,
    /// `process.env.KEY` / `process.env["KEY"]`, `os.Getenv("KEY")`, …). The KEY is
    /// the first string-literal argument / subscript / member-property name, with
    /// bare-identifier args resolved through the module-level constant map. The
    /// source is the innermost enclosing symbol (normally the function/method whose
    /// body holds the access); module-level accesses with no enclosing symbol are
    /// skipped. `target_symbol_id` stays NULL (the target is a KEY string, not a
    /// symbol). The edge `line` is the enclosing symbol's definition line so the
    /// shared `seen` set dedups on `(source, KEY)` (one edge per function per KEY).
    fn process_env_access_relationship(
        &self,
        node: &Node,
        source: &str,
        language: Language,
        state: &WalkState,
        rel: &mut RelState,
    ) {
        let Some(key) = extract_env_key(node, source, language, &rel.const_map) else {
            return;
        };
        if !is_env_var_name(&key) {
            return;
        }
        // Attribute to the innermost enclosing symbol (the function/method whose
        // body holds the access). Use ITS definition line as the edge line so the
        // dedup key `(source, KEY, ReadsEnv, line)` collapses to `(source, KEY)`.
        let range = Range::from_node(node);
        let Some(scope) = resolve_enclosing_scope(&state.scope, &range) else {
            return;
        };
        let source_id = scope.id.to_string();
        let line = scope.range.start.line;
        push_relationship(
            &mut rel.relationships,
            &mut rel.seen,
            &source_id,
            &self.file_path,
            key,
            SymbolRelationshipType::ReadsEnv,
            line,
        );
    }

    /// Emit `UsesType` edges (M4.1) for a node that just created a
    /// function/method symbol. The source is that symbol; the targets are the
    /// non-builtin base type names referenced in the signature's parameter types
    /// and return type. Body casts/generics are deferred (§13) — signatures only.
    ///
    /// The edge `line` is the function symbol's definition line, so the shared
    /// `seen` set collapses a type used in both a parameter and the return into a
    /// single edge (dedup on `(source, type)`).
    fn process_uses_type_relationship(
        &self,
        node: &Node,
        source: &str,
        language: Language,
        state: &WalkState,
        rel: &mut RelState,
    ) {
        // The symbol just appended by `enter_symbol_scope` for THIS node.
        let (source_id, line) = match state.symbols.last() {
            Some(sym)
                if matches!(
                    sym.symbol_type,
                    SymbolType::Function | SymbolType::Method
                ) =>
            {
                (sym.id.clone(), sym.range.start.line)
            }
            _ => return,
        };

        let Some(sig_node) = self.uses_type_signature_node(node, language) else {
            return;
        };

        // The type parameters THIS function declares (`fn f<T, K>` / `func F[T any]`
        // / `def f[T]`) are NOT referenced types — they are introduced by the
        // signature itself. Gather their names so they can be filtered out of the
        // emitted targets below (M4.1 noise fix). Scoped to THIS signature.
        let mut declared_params: HashSet<String> = HashSet::new();
        collect_declared_type_params(&sig_node, language, source, &mut declared_params);

        // Collect raw type-name leaves from each parameter's `type` field. Going
        // type-field-by-field (rather than scanning the whole parameter list)
        // avoids picking up parameter *names* — which in Python share the
        // `identifier` node-kind with type names.
        let mut raw: Vec<String> = Vec::new();
        if let Some(params) = sig_node.child_by_field_name("parameters") {
            let mut cursor = params.walk();
            for param in params.named_children(&mut cursor) {
                if let Some(type_node) = param.child_by_field_name("type") {
                    collect_type_names(&type_node, language, source, &mut raw);
                }
            }
        }

        // Return type: Go names it `result`; the others `return_type`.
        let return_field = if language == Language::Go {
            "result"
        } else {
            "return_type"
        };
        if let Some(return_node) = sig_node.child_by_field_name(return_field) {
            collect_type_names(&return_node, language, source, &mut raw);
        }

        for name in raw {
            let Some(clean) = clean_type_name(&name) else {
                continue;
            };
            // Drop the function's OWN generic type parameters (`<T, K, V>`): they
            // are declarations, not references.
            if declared_params.contains(&clean) {
                continue;
            }
            if is_builtin_type(language, &clean) {
                continue;
            }
            // `push_relationship` dedups on (source, target, UsesType, line); since
            // line is constant per function, this is dedup on (source, type).
            push_relationship(
                &mut rel.relationships,
                &mut rel.seen,
                &source_id,
                &self.file_path,
                clean,
                SymbolRelationshipType::UsesType,
                line,
            );
        }
    }

    /// The node whose `parameters` / return-type fields carry the signature for a
    /// just-created function/method symbol. For most languages that is the symbol
    /// node itself; for a TS/JS `const f = (...) => ...` the params live on the
    /// arrow/function *value*, so reuse `js_ts_signature_node`.
    fn uses_type_signature_node<'tree>(
        &self,
        node: &Node<'tree>,
        language: Language,
    ) -> Option<Node<'tree>> {
        match language {
            Language::TypeScript
            | Language::Tsx
            | Language::Astro
            | Language::JavaScript
            | Language::Jsx => self.js_ts_signature_node(node),
            _ => Some(*node),
        }
    }

    fn node_to_symbol(&self, node: &Node, source: &str, language: Language) -> Option<Symbol> {
        match language {
            Language::TypeScript | Language::Tsx | Language::Astro => {
                self.typescript_node_to_symbol(node, source, language)
            }
            Language::JavaScript | Language::Jsx => {
                self.javascript_node_to_symbol(node, source, language)
            }
            Language::Python => self.python_node_to_symbol(node, source, language),
            Language::Rust => self.rust_node_to_symbol(node, source, language),
            Language::Go => self.go_node_to_symbol(node, source, language),
            Language::Markdown
            | Language::Css
            | Language::Scss
            | Language::Sass
            | Language::Less
            | Language::Html
            | Language::Vue
            | Language::Svelte
            | Language::Json
            | Language::Yaml
            | Language::Toml
            | Language::Php
            | Language::Java
            | Language::CSharp
            | Language::Kotlin
            | Language::Ruby
            | Language::Cpp
            | Language::Shell
            | Language::Dockerfile
            | Language::Sql
            | Language::BuildScript => None,
        }
    }

    fn typescript_node_to_symbol(
        &self,
        node: &Node,
        source: &str,
        language: Language,
    ) -> Option<Symbol> {
        let kind = node.kind();
        let kind_id = node.kind_id();
        let bits = lang_bitsets(language, &node.language());
        let range = Range::from_node(node);

        if bits.import.contains(kind_id) {
            let text = node.utf8_text(source.as_bytes()).ok()?;
            let name = self.extract_quoted_text(text)?;
            return Some(Symbol::new(
                name,
                SymbolType::Import,
                self.file_path.clone(),
                range,
            ));
        }
        if bits.function.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            return Some(Symbol::new(
                name,
                SymbolType::Function,
                self.file_path.clone(),
                range,
            ));
        }
        if bits.method.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            return Some(Symbol::new(
                name,
                SymbolType::Method,
                self.file_path.clone(),
                range,
            ));
        }
        if bits.class.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            return Some(Symbol::new(
                name,
                SymbolType::Class,
                self.file_path.clone(),
                range,
            ));
        }
        if bits.enum_variant.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            return Some(Symbol::new(
                name,
                SymbolType::EnumMember,
                self.file_path.clone(),
                range,
            ));
        }
        if bits.field.contains(kind_id) {
            let name_node = node.child_by_field_name("name")?;
            let name = self.extract_js_ts_property_name(&name_node, source)?;
            let symbol_type = node
                .child_by_field_name("value")
                .as_ref()
                .filter(|value| self.is_js_ts_function_value(value, source))
                .map(|_| SymbolType::Method)
                .unwrap_or(SymbolType::Property);
            return Some(Symbol::new(
                name,
                symbol_type,
                self.file_path.clone(),
                range,
            ));
        }

        // Kinds with bespoke extraction (no simple kind→concern classification)
        // keep their inline logic: interface/type/enum declarations, the
        // variable-declarator function/const/variable split, object-literal
        // method `pair`s, and namespaces.
        match kind {
            "interface_declaration" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Interface,
                    self.file_path.clone(),
                    range,
                ))
            }
            "type_alias_declaration" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Type,
                    self.file_path.clone(),
                    range,
                ))
            }
            "enum_declaration" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Enum,
                    self.file_path.clone(),
                    range,
                ))
            }
            "variable_declarator" => {
                // Skip function-body locals; only module/class-scope declarators
                // are meaningful symbols (cuts index/token noise).
                if !self.is_emittable_js_ts_declarator(node) {
                    return None;
                }
                let name_node = node.child_by_field_name("name")?;
                let name = self.extract_js_ts_binding_name(&name_node, source)?;
                let value = node.child_by_field_name("value");
                let symbol_type = if value
                    .as_ref()
                    .is_some_and(|value| self.is_js_ts_function_value(value, source))
                {
                    SymbolType::Function
                } else if self.is_js_ts_const_declarator(node, source) {
                    SymbolType::Constant
                } else {
                    SymbolType::Variable
                };
                Some(Symbol::new(
                    name,
                    symbol_type,
                    self.file_path.clone(),
                    range,
                ))
            }
            "pair" => {
                let value = node.child_by_field_name("value")?;
                if !self.is_js_ts_function_value(&value, source) {
                    return None;
                }
                let key = node.child_by_field_name("key")?;
                let name = self.extract_js_ts_property_name(&key, source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Method,
                    self.file_path.clone(),
                    range,
                ))
            }
            "namespace_declaration" | "internal_module" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Namespace,
                    self.file_path.clone(),
                    range,
                ))
            }
            _ => None,
        }
    }

    fn javascript_node_to_symbol(
        &self,
        node: &Node,
        source: &str,
        language: Language,
    ) -> Option<Symbol> {
        // JavaScript uses similar structure to TypeScript
        self.typescript_node_to_symbol(node, source, language)
    }

    fn python_node_to_symbol(
        &self,
        node: &Node,
        source: &str,
        language: Language,
    ) -> Option<Symbol> {
        let kind = node.kind();
        let kind_id = node.kind_id();
        let bits = lang_bitsets(language, &node.language());
        let range = Range::from_node(node);

        if bits.import.contains(kind_id) {
            // `import X` and `from Y import Z` are both imports but parse their
            // target name differently — classification is table-driven, the name
            // extraction stays per-kind.
            let text = node.utf8_text(source.as_bytes()).ok()?;
            let name = if kind == "import_from_statement" {
                self.extract_python_from_import_target(text)?
            } else {
                self.extract_python_import_target(text)?
            };
            return Some(Symbol::new(
                name,
                SymbolType::Import,
                self.file_path.clone(),
                range,
            ));
        }
        if bits.function.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            // Check if it's a method (inside a class)
            let is_method = node
                .parent()
                .and_then(|p| p.parent())
                .map(|gp| gp.kind() == "class_definition")
                .unwrap_or(false);
            return Some(Symbol::new(
                name,
                if is_method {
                    SymbolType::Method
                } else {
                    SymbolType::Function
                },
                self.file_path.clone(),
                range,
            ));
        }
        if bits.class.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            return Some(Symbol::new(
                name,
                SymbolType::Class,
                self.file_path.clone(),
                range,
            ));
        }
        None
    }

    fn rust_node_to_symbol(
        &self,
        node: &Node,
        source: &str,
        language: Language,
    ) -> Option<Symbol> {
        let kind = node.kind();
        let kind_id = node.kind_id();
        let bits = lang_bitsets(language, &node.language());
        let range = Range::from_node(node);

        if bits.import.contains(kind_id) {
            let text = node.utf8_text(source.as_bytes()).ok()?;
            let name = self.extract_rust_use_target(text)?;
            return Some(Symbol::new(
                name,
                SymbolType::Import,
                self.file_path.clone(),
                range,
            ));
        }
        if bits.function.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            // A `function_item` directly inside an `impl` block is a method.
            let symbol_type = if is_rust_impl_method(node) {
                SymbolType::Method
            } else {
                SymbolType::Function
            };
            return Some(Symbol::new(name, symbol_type, self.file_path.clone(), range));
        }
        if bits.field.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            return Some(Symbol::new(
                name,
                SymbolType::Property,
                self.file_path.clone(),
                range,
            ));
        }
        if bits.enum_variant.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            return Some(Symbol::new(
                name,
                SymbolType::EnumMember,
                self.file_path.clone(),
                range,
            ));
        }

        // Type-level declarations carry no shared LangSpec concern (no
        // struct/enum/trait/impl/type/const/module fields) — kept inline.
        match kind {
            "struct_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Struct,
                    self.file_path.clone(),
                    range,
                ))
            }
            "enum_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Enum,
                    self.file_path.clone(),
                    range,
                ))
            }
            "trait_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Trait,
                    self.file_path.clone(),
                    range,
                ))
            }
            "impl_item" => {
                // Get the type being implemented
                if let Some(type_node) = node.child_by_field_name("type") {
                    let name = type_node.utf8_text(source.as_bytes()).ok()?;
                    Some(Symbol::new(
                        format!("impl {}", name),
                        SymbolType::Impl,
                        self.file_path.clone(),
                        range,
                    ))
                } else {
                    None
                }
            }
            "type_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Type,
                    self.file_path.clone(),
                    range,
                ))
            }
            "const_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Constant,
                    self.file_path.clone(),
                    range,
                ))
            }
            "mod_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Module,
                    self.file_path.clone(),
                    range,
                ))
            }
            _ => None,
        }
    }

    fn go_node_to_symbol(
        &self,
        node: &Node,
        source: &str,
        language: Language,
    ) -> Option<Symbol> {
        let kind = node.kind();
        let kind_id = node.kind_id();
        let bits = lang_bitsets(language, &node.language());
        let range = Range::from_node(node);

        if bits.import.contains(kind_id) {
            let text = node.utf8_text(source.as_bytes()).ok()?;
            let name = self.extract_quoted_text(text)?;
            return Some(Symbol::new(
                name,
                SymbolType::Import,
                self.file_path.clone(),
                range,
            ));
        }
        if bits.function.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            return Some(Symbol::new(
                name,
                SymbolType::Function,
                self.file_path.clone(),
                range,
            ));
        }
        if bits.method.contains(kind_id) {
            let name = self.get_child_text(node, "name", source)?;
            return Some(Symbol::new(
                name,
                SymbolType::Method,
                self.file_path.clone(),
                range,
            ));
        }

        // `type_spec`/`const_spec`/`var_spec` need bespoke sub-classification
        // (struct vs interface vs alias) — no shared concern — kept inline.
        match kind {
            "type_spec" => {
                let name = self.get_child_text(node, "name", source)?;
                let symbol_type = match node.child_by_field_name("type")?.kind() {
                    "struct_type" => SymbolType::Struct,
                    "interface_type" => SymbolType::Interface,
                    _ => SymbolType::Type,
                };
                Some(Symbol::new(
                    name,
                    symbol_type,
                    self.file_path.clone(),
                    range,
                ))
            }
            "const_spec" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Constant,
                    self.file_path.clone(),
                    range,
                ))
            }
            "var_spec" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Variable,
                    self.file_path.clone(),
                    range,
                ))
            }
            _ => None,
        }
    }

    fn get_child_text(&self, node: &Node, field_name: &str, source: &str) -> Option<String> {
        node.child_by_field_name(field_name)
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string())
    }

    fn extract_js_ts_binding_name(&self, node: &Node, source: &str) -> Option<String> {
        match node.kind() {
            "identifier" | "type_identifier" => node
                .utf8_text(source.as_bytes())
                .ok()
                .map(|value| value.to_string()),
            _ => None,
        }
    }

    fn extract_js_ts_property_name(&self, node: &Node, source: &str) -> Option<String> {
        let text = node.utf8_text(source.as_bytes()).ok()?.trim();
        let normalized = text
            .trim_start_matches('#')
            .trim_matches('"')
            .trim_matches('\'')
            .trim_matches('`');
        if normalized.is_empty() || normalized.starts_with('[') {
            None
        } else {
            Some(normalized.to_string())
        }
    }

    fn is_js_ts_const_declarator(&self, node: &Node, source: &str) -> bool {
        let Some(parent) = node.parent() else {
            return false;
        };
        if parent.kind() != "lexical_declaration" {
            return false;
        }
        parent
            .utf8_text(source.as_bytes())
            .ok()
            .is_some_and(|text| text.trim_start().starts_with("const"))
    }

    fn is_js_ts_function_value(&self, node: &Node, source: &str) -> bool {
        match node.kind() {
            "arrow_function" | "function" | "function_expression" | "generator_function" => true,
            "parenthesized_expression"
            | "as_expression"
            | "satisfies_expression"
            | "non_null_expression"
            | "type_assertion" => last_named_child(node)
                .as_ref()
                .is_some_and(|child| self.is_js_ts_function_value(child, source)),
            "call_expression" => {
                let is_component_wrapper = node
                    .child_by_field_name("function")
                    .and_then(|callee| extract_callable_name(&callee, source))
                    .is_some_and(|name| matches!(name.as_str(), "memo" | "forwardRef" | "lazy"));
                is_component_wrapper && self.has_js_ts_function_descendant(node, source)
            }
            _ => false,
        }
    }

    /// Return the node carrying the `parameters` field for a signature. For a
    /// `variable_declarator` bound to an arrow/function value, that is the value
    /// node; otherwise it is the node itself (when it has parameters).
    fn js_ts_signature_node<'tree>(&self, node: &Node<'tree>) -> Option<Node<'tree>> {
        if node.child_by_field_name("parameters").is_some() {
            return Some(*node);
        }
        if node.kind() == "variable_declarator" {
            let value = node.child_by_field_name("value")?;
            if matches!(
                value.kind(),
                "arrow_function" | "function" | "function_expression" | "generator_function"
            ) {
                return Some(value);
            }
        }
        None
    }

    /// Only emit a `variable_declarator` symbol when its enclosing scope is
    /// module/class level. Declarators inside a function body are locals (noise).
    fn is_emittable_js_ts_declarator(&self, node: &Node) -> bool {
        let mut current = node.parent();
        while let Some(ancestor) = current {
            match ancestor.kind() {
                // A statement block means we are inside a function/arrow/loop body.
                "statement_block" | "function_body" => return false,
                "program" | "module" => return true,
                _ => {}
            }
            current = ancestor.parent();
        }
        true
    }

    fn has_js_ts_function_descendant(&self, node: &Node, source: &str) -> bool {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if self.is_js_ts_function_value(&child, source)
                || self.has_js_ts_function_descendant(&child, source)
            {
                return true;
            }
        }
        false
    }

    fn extract_quoted_text(&self, text: &str) -> Option<String> {
        let start = text.find(['"', '\''])?;
        let quote = text[start..].chars().next()?;
        let rest = &text[start + quote.len_utf8()..];
        let end = rest.find(quote)?;
        Some(rest[..end].to_string())
    }

    fn extract_python_import_target(&self, text: &str) -> Option<String> {
        let trimmed = text.trim();
        let imports = trimmed.strip_prefix("import ")?.trim();
        imports
            .split(',')
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    fn extract_python_from_import_target(&self, text: &str) -> Option<String> {
        let trimmed = text.trim();
        let after_from = trimmed.strip_prefix("from ")?;
        let end = after_from.find(" import")?;
        let target = after_from[..end].trim();
        if target.is_empty() {
            None
        } else {
            Some(target.to_string())
        }
    }

    fn extract_rust_use_target(&self, text: &str) -> Option<String> {
        let trimmed = text.trim();
        let target = trimmed.strip_prefix("use ")?.trim_end_matches(';').trim();
        if target.is_empty() {
            None
        } else {
            Some(target.to_string())
        }
    }

    fn extract_docstring(&self, node: &Node, source: &str, language: Language) -> Option<String> {
        match language {
            Language::Rust => self.extract_rust_docstring(node, source),
            Language::Python => self
                .extract_python_docstring(node, source)
                .or_else(|| self.extract_prev_comment_docstring(node, source)),
            Language::Go => self.extract_go_docstring(node, source),
            _ => self.extract_prev_comment_docstring(node, source),
        }
    }

    /// Capture the single comment node immediately preceding `node` (the
    /// original, language-agnostic behavior).
    fn extract_prev_comment_docstring(&self, node: &Node, source: &str) -> Option<String> {
        let prev = node.prev_sibling()?;
        let kind = prev.kind();
        if kind != "comment" && kind != "block_comment" && kind != "line_comment" {
            return None;
        }
        prev.utf8_text(source.as_bytes()).ok().map(|s| {
            // Clean up comment markers
            let s = s.trim();
            let s = s.strip_prefix("///").unwrap_or(s);
            let s = s.strip_prefix("//").unwrap_or(s);
            let s = s.strip_prefix("/*").unwrap_or(s);
            let s = s.strip_suffix("*/").unwrap_or(s);
            let s = s.strip_prefix('#').unwrap_or(s);
            let s = s.strip_prefix("\"\"\"").unwrap_or(s);
            let s = s.strip_suffix("\"\"\"").unwrap_or(s);
            s.trim().to_string()
        })
    }

    /// Join consecutive `///` / `//!` doc-comment lines directly above `node`
    /// into one docstring. Falls back to a single preceding comment otherwise.
    fn extract_rust_docstring(&self, node: &Node, source: &str) -> Option<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut expected_row = node.start_position().row;
        let mut cursor = node.prev_sibling();

        while let Some(prev) = cursor {
            if prev.kind() != "line_comment" {
                break;
            }
            let Ok(text) = prev.utf8_text(source.as_bytes()) else {
                break;
            };
            let trimmed = text.trim();
            if !(trimmed.starts_with("///") || trimmed.starts_with("//!")) {
                break;
            }
            // Only attach contiguous doc-comment lines (no blank-line gap).
            if prev.end_position().row + 1 != expected_row {
                break;
            }
            lines.push(clean_rust_doc_line(trimmed));
            expected_row = prev.start_position().row;
            cursor = prev.prev_sibling();
        }

        if lines.is_empty() {
            return self.extract_prev_comment_docstring(node, source);
        }
        lines.reverse();
        Some(lines.join("\n"))
    }

    /// Capture a Python docstring: the first `expression_statement → string`
    /// child of a function/class body.
    fn extract_python_docstring(&self, node: &Node, source: &str) -> Option<String> {
        if node.kind() != "function_definition" && node.kind() != "class_definition" {
            return None;
        }
        let body = node.child_by_field_name("body")?;
        let first = body.named_child(0)?;
        if first.kind() != "expression_statement" {
            return None;
        }
        let string_node = first.named_child(0)?;
        if string_node.kind() != "string" {
            return None;
        }
        let text = string_node.utf8_text(source.as_bytes()).ok()?;
        Some(decode_python_string(text))
    }

    /// Go doc comments precede the declaration; for type/const/var specs the
    /// comment sits before the wrapping `*_declaration`, not the spec itself.
    fn extract_go_docstring(&self, node: &Node, source: &str) -> Option<String> {
        if let Some(doc) = self.extract_prev_comment_docstring(node, source) {
            return Some(doc);
        }
        if matches!(node.kind(), "type_spec" | "const_spec" | "var_spec") {
            if let Some(parent) = node.parent() {
                // Only borrow the wrapper's comment for the first spec in a group.
                if parent.named_child(0).map(|c| c.id()) == Some(node.id()) {
                    return self.extract_prev_comment_docstring(&parent, source);
                }
            }
        }
        None
    }

    fn extract_signature(&self, node: &Node, source: &str, language: Language) -> Option<String> {
        match language {
            Language::TypeScript
            | Language::Tsx
            | Language::Astro
            | Language::JavaScript
            | Language::Jsx => {
                // For functions, extract parameters. For `const f = (...) => ...`
                // the parameters live on the arrow/function value, not on the
                // `variable_declarator` itself.
                if let Some(sig_node) = self.js_ts_signature_node(node) {
                    if let Some(params) = sig_node.child_by_field_name("parameters") {
                        let params_text = params.utf8_text(source.as_bytes()).ok()?;
                        // Try to get return type
                        let return_type = sig_node
                            .child_by_field_name("return_type")
                            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                            .map(|s| format!(" {}", s))
                            .unwrap_or_default();
                        return Some(format!("{}{}", params_text, return_type));
                    }
                }
            }
            Language::Python => {
                if let Some(params) = node.child_by_field_name("parameters") {
                    let params_text = params.utf8_text(source.as_bytes()).ok()?;
                    let return_type = node
                        .child_by_field_name("return_type")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| format!(" -> {}", s))
                        .unwrap_or_default();
                    return Some(format!("{}{}", params_text, return_type));
                }
            }
            Language::Rust => {
                if let Some(params) = node.child_by_field_name("parameters") {
                    let params_text = params.utf8_text(source.as_bytes()).ok()?;
                    let return_type = node
                        .child_by_field_name("return_type")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| format!(" {}", s))
                        .unwrap_or_default();
                    return Some(format!("{}{}", params_text, return_type));
                }
            }
            Language::Go => {
                if let Some(params) = node.child_by_field_name("parameters") {
                    let params_text = params.utf8_text(source.as_bytes()).ok()?;
                    let return_type = node
                        .child_by_field_name("result")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| format!(" {}", s))
                        .unwrap_or_default();
                    return Some(format!("{}{}", params_text, return_type));
                }
            }
            Language::Markdown
            | Language::Css
            | Language::Scss
            | Language::Sass
            | Language::Less
            | Language::Html
            | Language::Vue
            | Language::Svelte
            | Language::Json
            | Language::Yaml
            | Language::Toml
            | Language::Php
            | Language::Java
            | Language::CSharp
            | Language::Kotlin
            | Language::Ruby
            | Language::Cpp
            | Language::Shell
            | Language::Dockerfile
            | Language::Sql
            | Language::BuildScript => {}
        }
        None
    }
}

/// Convenience function to extract symbols from source code
pub fn extract_symbols(
    tree: &Tree,
    source: &str,
    language: Language,
    file_path: &str,
) -> Vec<Symbol> {
    let extractor = SymbolExtractor::new(file_path.to_string());
    extractor.extract(tree, source, language)
}

pub fn extract_symbol_relationships(
    tree: &Tree,
    source: &str,
    language: Language,
    file_path: &str,
    symbols: &[Symbol],
) -> Vec<SymbolRelationship> {
    let extractor = SymbolExtractor::new(file_path.to_string());
    let mut state = WalkState {
        scope: Vec::new(),
        symbols: Vec::new(),
    };
    // Precompute the per-file module-level constant map (M4.2): `NAME = "literal"`
    // bindings used to resolve bare-identifier env-var KEYs. Built up-front (not
    // during the walk) so a const declared *after* its use still resolves.
    let mut const_map: HashMap<String, String> = HashMap::new();
    collect_module_constants(&tree.root_node(), language, source, &mut const_map);

    let mut rel = RelState {
        relationships: Vec::new(),
        seen: HashSet::new(),
        all_symbols: symbols,
        const_map,
    };

    // Unified relationship walk: call/macro + structural edges off one DFS,
    // attributed to the enclosing symbol via the scope stack.
    extractor.walk_relationships(tree.root_node(), source, language, &mut state, &mut rel);

    // Import edges are derived from the symbol list, not the tree.
    extract_import_relationships(file_path, symbols, &mut rel.relationships, &mut rel.seen);

    rel.relationships
}

fn extract_typescript_structural_relationships(
    node: Node,
    source: &str,
    file_path: &str,
    scope: &[Scope],
    relationships: &mut Vec<SymbolRelationship>,
    seen: &mut HashSet<(String, String, SymbolRelationshipType, u32)>,
) {
    let kind = node.kind();
    if kind != "class_declaration" && kind != "class" && kind != "interface_declaration" {
        return;
    }

    let Some(source_id) =
        resolve_enclosing_scope(scope, &Range::from_node(&node)).map(|frame| frame.id.to_string())
    else {
        return;
    };
    let Ok(text) = node.utf8_text(source.as_bytes()) else {
        return;
    };
    let header = text.split('{').next().unwrap_or(text);
    let line = node.start_position().row as u32;

    if kind == "class_declaration" || kind == "class" {
        if let Some(tail) = extract_text_after_keyword(header, "extends") {
            let base_segment = tail.split(" implements ").next().unwrap_or(tail).trim();
            if let Some(target_name) = normalize_reference_name(base_segment) {
                push_relationship(
                    relationships,
                    seen,
                    &source_id,
                    file_path,
                    target_name,
                    SymbolRelationshipType::Extends,
                    line,
                );
            }
        }

        if let Some(tail) = extract_text_after_keyword(header, "implements") {
            for target_name in split_reference_list(tail) {
                push_relationship(
                    relationships,
                    seen,
                    &source_id,
                    file_path,
                    target_name,
                    SymbolRelationshipType::Implements,
                    line,
                );
            }
        }
    }

    if kind == "interface_declaration" {
        if let Some(tail) = extract_text_after_keyword(header, "extends") {
            for target_name in split_reference_list(tail) {
                push_relationship(
                    relationships,
                    seen,
                    &source_id,
                    file_path,
                    target_name,
                    SymbolRelationshipType::Extends,
                    line,
                );
            }
        }
    }
}

fn extract_python_structural_relationships(
    node: Node,
    source: &str,
    file_path: &str,
    scope: &[Scope],
    relationships: &mut Vec<SymbolRelationship>,
    seen: &mut HashSet<(String, String, SymbolRelationshipType, u32)>,
) {
    if node.kind() != "class_definition" {
        return;
    }

    let Some(source_id) =
        resolve_enclosing_scope(scope, &Range::from_node(&node)).map(|frame| frame.id.to_string())
    else {
        return;
    };
    let Ok(text) = node.utf8_text(source.as_bytes()) else {
        return;
    };
    let header = text.lines().next().unwrap_or(text).trim();
    let Some(start_idx) = header.find('(') else {
        return;
    };
    let Some(end_idx) = header[start_idx + 1..].find(')') else {
        return;
    };
    let bases = &header[start_idx + 1..start_idx + 1 + end_idx];
    let line = node.start_position().row as u32;

    for target_name in split_reference_list(bases) {
        push_relationship(
            relationships,
            seen,
            &source_id,
            file_path,
            target_name,
            SymbolRelationshipType::Extends,
            line,
        );
    }
}

fn extract_rust_structural_relationships(
    node: Node,
    source: &str,
    file_path: &str,
    symbols: &[Symbol],
    relationships: &mut Vec<SymbolRelationship>,
    seen: &mut HashSet<(String, String, SymbolRelationshipType, u32)>,
) {
    if node.kind() != "impl_item" {
        return;
    }

    let Ok(text) = node.utf8_text(source.as_bytes()) else {
        return;
    };
    let header = text.split('{').next().unwrap_or(text).trim();
    let Some(after_impl) = header.strip_prefix("impl") else {
        return;
    };
    let after_impl = strip_leading_generic_block(after_impl.trim());
    let Some((trait_part, type_part)) = after_impl.split_once(" for ") else {
        return;
    };
    let Some(type_name) = normalize_reference_name(type_part) else {
        return;
    };
    let Some(trait_name) = normalize_reference_name(trait_part) else {
        return;
    };
    let Some(source_symbol) = find_symbol_by_name(symbols, &type_name) else {
        return;
    };

    push_relationship(
        relationships,
        seen,
        &source_symbol.id,
        file_path,
        trait_name,
        SymbolRelationshipType::Implements,
        node.start_position().row as u32,
    );
}

fn extract_go_structural_relationships(
    node: Node,
    source: &str,
    file_path: &str,
    symbols: &[Symbol],
    relationships: &mut Vec<SymbolRelationship>,
    seen: &mut HashSet<(String, String, SymbolRelationshipType, u32)>,
) {
    match node.kind() {
        // Receiver→method binding: the method's parent is its receiver type.
        "method_declaration" => {
            let Some(receiver) = node.child_by_field_name("receiver") else {
                return;
            };
            let Some(receiver_type) = go_receiver_type_name(&receiver, source) else {
                return;
            };
            let Some(method_name) = node
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(source.as_bytes()).ok())
            else {
                return;
            };
            let Some(type_symbol) = find_symbol_by_name(symbols, &receiver_type) else {
                return;
            };
            push_relationship(
                relationships,
                seen,
                &type_symbol.id,
                file_path,
                method_name.to_string(),
                SymbolRelationshipType::Contains,
                node.start_position().row as u32,
            );
        }
        // Struct embedding: `type Server struct { Base }` → Server extends Base.
        "type_spec" => {
            let Some(type_node) = node.child_by_field_name("type") else {
                return;
            };
            if type_node.kind() != "struct_type" {
                return;
            }
            let Some(type_name) = node
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(source.as_bytes()).ok())
            else {
                return;
            };
            let Some(type_symbol) = find_symbol_by_name(symbols, type_name) else {
                return;
            };
            let line = node.start_position().row as u32;
            for embedded in go_embedded_type_names(&type_node, source) {
                push_relationship(
                    relationships,
                    seen,
                    &type_symbol.id,
                    file_path,
                    embedded,
                    SymbolRelationshipType::Extends,
                    line,
                );
            }
        }
        _ => {}
    }
}

/// Resolve the base type name of a Go method receiver (`(s *Server)` → `Server`).
fn go_receiver_type_name(receiver: &Node, source: &str) -> Option<String> {
    let mut type_node = None;
    let mut cursor = receiver.walk();
    for param in receiver.named_children(&mut cursor) {
        if let Some(ty) = param.child_by_field_name("type") {
            type_node = Some(ty);
            break;
        }
    }
    let mut ty = type_node?;
    while ty.kind() == "pointer_type" {
        ty = last_named_child(&ty)?;
    }
    ty.utf8_text(source.as_bytes())
        .ok()
        .and_then(normalize_reference_name)
}

/// Collect the names of anonymously-embedded fields in a Go struct type.
fn go_embedded_type_names(struct_type: &Node, source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let Some(field_list) = (0..struct_type.named_child_count())
        .filter_map(|i| struct_type.named_child(i as u32))
        .find(|child| child.kind() == "field_declaration_list")
    else {
        return names;
    };

    let mut cursor = field_list.walk();
    for field in field_list.named_children(&mut cursor) {
        if field.kind() != "field_declaration" {
            continue;
        }
        // Named fields have a `name` field; embedded fields carry only a type.
        if field.child_by_field_name("name").is_some() {
            continue;
        }
        let Some(mut ty) = field.child_by_field_name("type") else {
            continue;
        };
        while ty.kind() == "pointer_type" {
            let Some(inner) = last_named_child(&ty) else {
                break;
            };
            ty = inner;
        }
        if let Some(name) = ty
            .utf8_text(source.as_bytes())
            .ok()
            .and_then(normalize_reference_name)
        {
            names.push(name);
        }
    }
    names
}

fn push_relationship(
    relationships: &mut Vec<SymbolRelationship>,
    seen: &mut HashSet<(String, String, SymbolRelationshipType, u32)>,
    source_id: &str,
    file_path: &str,
    target_name: String,
    relationship_type: SymbolRelationshipType,
    line: u32,
) {
    if target_name.is_empty() {
        return;
    }

    let key = (
        source_id.to_string(),
        target_name.clone(),
        relationship_type,
        line,
    );

    if seen.insert(key) {
        relationships.push(SymbolRelationship {
            source_symbol_id: source_id.to_string(),
            source_file_path: file_path.to_string(),
            target_name,
            target_symbol_id: None,
            relationship_type,
            line,
        });
    }
}

fn extract_text_after_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let needle = format!(" {} ", keyword);
    let idx = text.find(&needle)?;
    Some(text[idx + needle.len()..].trim())
}

fn split_reference_list(segment: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut angle_depth = 0u32;
    let mut paren_depth = 0u32;
    let mut bracket_depth = 0u32;

    for ch in segment.chars() {
        match ch {
            '<' => {
                angle_depth = angle_depth.saturating_add(1);
                current.push(ch);
            }
            '>' => {
                angle_depth = angle_depth.saturating_sub(1);
                current.push(ch);
            }
            '(' => {
                paren_depth = paren_depth.saturating_add(1);
                current.push(ch);
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                current.push(ch);
            }
            '[' => {
                bracket_depth = bracket_depth.saturating_add(1);
                current.push(ch);
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if angle_depth == 0 && paren_depth == 0 && bracket_depth == 0 => {
                if let Some(value) = normalize_reference_name(&current) {
                    items.push(value);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if let Some(value) = normalize_reference_name(&current) {
        items.push(value);
    }

    items
}

fn normalize_reference_name(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    if value.is_empty() {
        return None;
    }

    if let Some(prefix) = value.strip_prefix("dyn ") {
        value = prefix.trim();
    }
    if let Some(prefix) = value.strip_prefix('&') {
        value = prefix.trim();
    }
    if let Some(prefix) = value.strip_prefix("mut ") {
        value = prefix.trim();
    }

    value = value
        .trim_end_matches('{')
        .trim_end_matches(':')
        .trim_end_matches(';')
        .trim();

    let value = value.split('<').next().unwrap_or(value).trim();
    let value = value.split('(').next().unwrap_or(value).trim();
    let value = value.split_whitespace().next().unwrap_or(value).trim();
    let value = value.rsplit("::").next().unwrap_or(value).trim();
    let value = value.rsplit('.').next().unwrap_or(value).trim();

    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn strip_leading_generic_block(text: &str) -> &str {
    let trimmed = text.trim();
    if !trimmed.starts_with('<') {
        return trimmed;
    }

    let mut depth = 0u32;
    for (idx, ch) in trimmed.char_indices() {
        match ch {
            '<' => depth = depth.saturating_add(1),
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return trimmed[idx + ch.len_utf8()..].trim();
                }
            }
            _ => {}
        }
    }

    trimmed
}

fn find_symbol_by_name<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
    symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.symbol_type != SymbolType::Import)
}

/// Symbol kinds a Rust `impl` block can be implemented for.
fn is_rust_type_symbol(symbol_type: SymbolType) -> bool {
    matches!(
        symbol_type,
        SymbolType::Struct | SymbolType::Enum | SymbolType::Trait | SymbolType::Type
    )
}

/// True when a Rust `function_item` is declared directly inside an `impl` block
/// (`impl_item → declaration_list → function_item`).
fn is_rust_impl_method(node: &Node) -> bool {
    node.parent()
        .filter(|parent| parent.kind() == "declaration_list")
        .and_then(|parent| parent.parent())
        .map(|grandparent| grandparent.kind() == "impl_item")
        .unwrap_or(false)
}

/// Strip the leading `///` / `//` marker from a single Rust doc-comment line.
fn clean_rust_doc_line(text: &str) -> String {
    let s = text.trim();
    let s = s.strip_prefix("///").unwrap_or(s);
    let s = s.strip_prefix("//").unwrap_or(s);
    s.trim().to_string()
}

/// Decode a Python string literal (the docstring text) by stripping any string
/// prefix and the surrounding quote markers.
fn decode_python_string(text: &str) -> String {
    let s = text.trim();
    let s = s.trim_start_matches(|c: char| matches!(c, 'r' | 'R' | 'b' | 'B' | 'f' | 'F' | 'u' | 'U'));
    for quote in ["\"\"\"", "'''", "\"", "'"] {
        if let Some(inner) = s.strip_prefix(quote).and_then(|rest| rest.strip_suffix(quote)) {
            return inner.trim().to_string();
        }
    }
    s.trim().to_string()
}

/// Per-language node-kind classification table (M3.1 / 3.1g).
///
/// Each field lists the tree-sitter node-kind strings that one extraction
/// *concern* matches for a `Language`. This `&'static [&str]` set is the source
/// of truth; it is compiled once into O(1) `kind_id` bitsets (`LangBitsets`,
/// 3.1b). Only the kind→concern *classification* lives here — per-kind
/// name/signature extraction and the M1.2 special cases (Rust impl-method
/// retyping, Python method detection, the JS/TS field Property-vs-Method split,
/// `Type::method` parenting, Go receiver/embedding) stay in the extractors.
struct LangSpec {
    function_kinds: &'static [&'static str],
    method_kinds: &'static [&'static str],
    class_kinds: &'static [&'static str],
    field_kinds: &'static [&'static str],
    enum_variant_kinds: &'static [&'static str],
    call_kinds: &'static [&'static str],
    macro_call_kinds: &'static [&'static str],
    import_kinds: &'static [&'static str],
    decorator_kinds: &'static [&'static str],
}

/// Languages with no full-grammar extractor (scanners / anchors only).
static EMPTY_SPEC: LangSpec = LangSpec {
    function_kinds: &[],
    method_kinds: &[],
    class_kinds: &[],
    field_kinds: &[],
    enum_variant_kinds: &[],
    call_kinds: &[],
    macro_call_kinds: &[],
    import_kinds: &[],
    decorator_kinds: &[],
};

/// TypeScript family — TS / TSX / Astro / JavaScript / JSX all funnel through
/// `typescript_node_to_symbol`, so they share these rows (each still compiles to
/// its own grammar's bitset). `interface`/`type`/`enum` declarations,
/// `variable_declarator`, object-literal `pair`s and namespaces are NOT concerns
/// (bespoke extraction) and stay inline.
static TS_SPEC: LangSpec = LangSpec {
    function_kinds: &[
        "function_declaration",
        "generator_function_declaration",
        "function_signature",
        "function",
        "generator_function",
    ],
    method_kinds: &[
        "method_definition",
        "method_signature",
        "abstract_method_signature",
    ],
    class_kinds: &["class_declaration", "class"],
    field_kinds: &[
        "public_field_definition",
        "field_definition",
        "property_signature",
    ],
    enum_variant_kinds: &["enum_assignment"],
    call_kinds: &["call_expression"],
    macro_call_kinds: &[],
    import_kinds: &["import_statement"],
    decorator_kinds: &[],
};

/// Python. `function_definition` is reclassified to a method inline when nested
/// in a class; `class_definition` is the only class kind.
static PYTHON_SPEC: LangSpec = LangSpec {
    function_kinds: &["function_definition"],
    method_kinds: &[],
    class_kinds: &["class_definition"],
    field_kinds: &[],
    enum_variant_kinds: &[],
    call_kinds: &["call"],
    macro_call_kinds: &[],
    import_kinds: &["import_statement", "import_from_statement"],
    decorator_kinds: &[],
};

/// Rust. `function_item` is the only function kind and is retyped to a method
/// inline when directly inside an `impl`. Struct/enum/trait/impl/type/const/mod
/// declarations are not concerns (no matching `LangSpec` field) and stay inline.
static RUST_SPEC: LangSpec = LangSpec {
    function_kinds: &["function_item"],
    method_kinds: &[],
    class_kinds: &[],
    field_kinds: &["field_declaration"],
    enum_variant_kinds: &["enum_variant"],
    call_kinds: &["call_expression"],
    macro_call_kinds: &["macro_invocation"],
    import_kinds: &["use_declaration"],
    decorator_kinds: &[],
};

/// Go. `type_spec` (struct/interface/alias), `const_spec` and `var_spec` are
/// bespoke and stay inline.
static GO_SPEC: LangSpec = LangSpec {
    function_kinds: &["function_declaration"],
    method_kinds: &["method_declaration"],
    class_kinds: &[],
    field_kinds: &[],
    enum_variant_kinds: &[],
    call_kinds: &["call_expression"],
    macro_call_kinds: &[],
    import_kinds: &["import_spec"],
    decorator_kinds: &[],
};

/// The `LangSpec` for a language. The TS variants share `TS_SPEC`; languages
/// without a full grammar get the empty spec.
fn lang_spec(language: Language) -> &'static LangSpec {
    match language {
        Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx => &TS_SPEC,
        Language::Python => &PYTHON_SPEC,
        Language::Rust => &RUST_SPEC,
        Language::Go => &GO_SPEC,
        _ => &EMPTY_SPEC,
    }
}

/// O(1) `kind_id` membership test compiled from a `&[&str]` of node-kind names
/// (3.1b). Backed by a `Vec<bool>` sized to the grammar's `node_kind_count()`
/// (the fastest dep-free form). Both the named and anonymous symbol id of each
/// name are set, so anonymous-token nodes match their string form exactly as the
/// old `node.kind() == "..."` tests did.
struct KindSet {
    bits: Vec<bool>,
}

impl KindSet {
    fn build(grammar: &tree_sitter::Language, names: &[&str]) -> Self {
        let count = grammar.node_kind_count();
        let mut bits = vec![false; count];
        for &name in names {
            // Try both the named and the anonymous form; a kind may exist as
            // either (or neither, in which case `id_for_node_kind` returns 0).
            for named in [true, false] {
                let id = grammar.id_for_node_kind(name, named);
                // 0 is the "no such kind" sentinel (and the end token) — skip.
                if id != 0 && (id as usize) < count {
                    bits[id as usize] = true;
                }
            }
        }
        Self { bits }
    }

    #[inline]
    fn contains(&self, kind_id: u16) -> bool {
        self.bits.get(kind_id as usize).copied().unwrap_or(false)
    }
}

/// All nine `LangSpec` concerns compiled to `KindSet`s for one grammar.
struct LangBitsets {
    function: KindSet,
    method: KindSet,
    class: KindSet,
    field: KindSet,
    enum_variant: KindSet,
    call: KindSet,
    macro_call: KindSet,
    import: KindSet,
    /// Kept for the committed `LangSpec` field set; no language emits decorator
    /// edges yet (that is Phase 4), so this compiled set is intentionally unread.
    #[allow(dead_code)]
    decorator: KindSet,
}

impl LangBitsets {
    fn build(grammar: &tree_sitter::Language, spec: &LangSpec) -> Self {
        Self {
            function: KindSet::build(grammar, spec.function_kinds),
            method: KindSet::build(grammar, spec.method_kinds),
            class: KindSet::build(grammar, spec.class_kinds),
            field: KindSet::build(grammar, spec.field_kinds),
            enum_variant: KindSet::build(grammar, spec.enum_variant_kinds),
            call: KindSet::build(grammar, spec.call_kinds),
            macro_call: KindSet::build(grammar, spec.macro_call_kinds),
            import: KindSet::build(grammar, spec.import_kinds),
            decorator: KindSet::build(grammar, spec.decorator_kinds),
        }
    }
}

/// Compiled-bitset cache: one `LangBitsets` per `Language`, built lazily from the
/// node's own grammar (`node.language()`) so the TS/JS variants — which share
/// `LangSpec` rows but use *different* grammars (distinct `kind_id`s) — each get
/// a correctly-keyed set. Languages with no rows share one inert (all-false)
/// cell. `#[allow(unused)]` on `decorator` keeps the committed field set whole
/// (no language populates it yet).
fn lang_bitsets(language: Language, grammar: &tree_sitter::Language) -> &'static LangBitsets {
    use std::sync::OnceLock;
    static TS: OnceLock<LangBitsets> = OnceLock::new();
    static TSX: OnceLock<LangBitsets> = OnceLock::new();
    static ASTRO: OnceLock<LangBitsets> = OnceLock::new();
    static JS: OnceLock<LangBitsets> = OnceLock::new();
    static JSX: OnceLock<LangBitsets> = OnceLock::new();
    static PY: OnceLock<LangBitsets> = OnceLock::new();
    static RS: OnceLock<LangBitsets> = OnceLock::new();
    static GO: OnceLock<LangBitsets> = OnceLock::new();
    static OTHER: OnceLock<LangBitsets> = OnceLock::new();

    let cell = match language {
        Language::TypeScript => &TS,
        Language::Tsx => &TSX,
        Language::Astro => &ASTRO,
        Language::JavaScript => &JS,
        Language::Jsx => &JSX,
        Language::Python => &PY,
        Language::Rust => &RS,
        Language::Go => &GO,
        _ => &OTHER,
    };
    cell.get_or_init(|| LangBitsets::build(grammar, lang_spec(language)))
}

fn extract_relationship_target_name(
    node: &Node,
    source: &str,
    language: Language,
) -> Option<String> {
    match language {
        Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx
        | Language::Python
        | Language::Rust
        | Language::Go => {
            let bits = lang_bitsets(language, &node.language());
            let kind_id = node.kind_id();
            if bits.call.contains(kind_id) {
                let callee = node.child_by_field_name("function")?;
                extract_callable_name(&callee, source)
            } else if bits.macro_call.contains(kind_id) {
                // `println!(...)`, `anyhow!(...)`, `tracing::info!(...)` etc. emit
                // a Call edge from the macro path child (Rust only today).
                let macro_node = node.child_by_field_name("macro")?;
                extract_callable_name(&macro_node, source)
            } else {
                None
            }
        }
        Language::Markdown
        | Language::Css
        | Language::Scss
        | Language::Sass
        | Language::Less
        | Language::Html
        | Language::Vue
        | Language::Svelte
        | Language::Json
        | Language::Yaml
        | Language::Toml
        | Language::Php
        | Language::Java
        | Language::CSharp
        | Language::Kotlin
        | Language::Ruby
        | Language::Cpp
        | Language::Shell
        | Language::Dockerfile
        | Language::Sql
        | Language::BuildScript => None,
    }
}

fn extract_callable_name(node: &Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "property_identifier" | "field_identifier" | "type_identifier" => node
            .utf8_text(source.as_bytes())
            .ok()
            .map(|s| s.to_string()),
        "member_expression"
        | "member_access_expression"
        | "field_expression"
        | "selector_expression" => {
            if let Some(child) = node
                .child_by_field_name("property")
                .or_else(|| node.child_by_field_name("field"))
                .or_else(|| last_named_child(node))
            {
                extract_callable_name(&child, source)
            } else {
                None
            }
        }
        "attribute" => {
            if let Some(child) = node
                .child_by_field_name("attribute")
                .or_else(|| last_named_child(node))
            {
                extract_callable_name(&child, source)
            } else {
                None
            }
        }
        "scoped_identifier" | "qualified_identifier" => {
            last_named_child(node).and_then(|child| extract_callable_name(&child, source))
        }
        _ => None,
    }
}

fn last_named_child<'tree>(node: &Node<'tree>) -> Option<Node<'tree>> {
    let count = node.named_child_count();
    if count == 0 {
        None
    } else {
        node.named_child((count - 1) as u32)
    }
}

// ---- M4.2 `reads_env` env-accessor extraction -------------------------------

/// Extract the environment-variable KEY that `node` reads, if `node` is an env
/// accessor for `language` (M4.2). Returns the raw KEY (the caller validates it
/// via `is_env_var_name`). Three node shapes are recognized:
/// - a CALL whose callee path matches a known accessor (`std::env::var` /
///   `env::var_os` / `os.getenv` / `os.environ.get` / `os.Getenv` /
///   `os.LookupEnv`) → its first string/identifier argument;
/// - a SUBSCRIPT on a known container (`os.environ["KEY"]`, `environ["KEY"]`,
///   `process.env["KEY"]`) → the index string/identifier;
/// - a MEMBER access on `process.env` (`process.env.KEY`) → the property name.
///
/// A bare-identifier argument/index is resolved through `const_map` (constant
/// propagation); a member property name is already a name and is taken literally.
fn extract_env_key(
    node: &Node,
    source: &str,
    language: Language,
    const_map: &HashMap<String, String>,
) -> Option<String> {
    match language {
        Language::Python => match node.kind() {
            "subscript" => {
                let container = node.child_by_field_name("value")?;
                if !env_container_matches(&container, source, language) {
                    return None;
                }
                let index = node.child_by_field_name("subscript")?;
                literal_or_const(&index, source, const_map)
            }
            "call" => {
                let callee = node.child_by_field_name("function")?;
                if !env_callee_matches(&callee, source, language) {
                    return None;
                }
                first_string_arg(node, source, const_map)
            }
            _ => None,
        },
        Language::Rust | Language::Go => {
            if node.kind() != "call_expression" {
                return None;
            }
            let callee = node.child_by_field_name("function")?;
            if !env_callee_matches(&callee, source, language) {
                return None;
            }
            first_string_arg(node, source, const_map)
        }
        Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx => match node.kind() {
            "member_expression" => {
                let object = node.child_by_field_name("object")?;
                if !env_container_matches(&object, source, language) {
                    return None;
                }
                let property = node.child_by_field_name("property")?;
                property
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string())
            }
            "subscript_expression" => {
                let object = node.child_by_field_name("object")?;
                if !env_container_matches(&object, source, language) {
                    return None;
                }
                let index = node.child_by_field_name("index")?;
                literal_or_const(&index, source, const_map)
            }
            _ => None,
        },
        _ => None,
    }
}

/// The dotted/scoped identifier path of `node` as segments, or `None` when `node`
/// is not a pure path expression. `::` is normalized to `.`; any whitespace or
/// bracket/paren/quote/operator char means it is not a simple path (reject), which
/// keeps callee/receiver matching robust against complex expressions.
fn node_path_text(node: &Node, source: &str) -> Option<Vec<String>> {
    let text = node.utf8_text(source.as_bytes()).ok()?;
    if text.is_empty() {
        return None;
    }
    let norm = text.replace("::", ".");
    if norm
        .chars()
        .any(|c| c.is_whitespace() || "()[]{}<>\"'`,?&*!".contains(c))
    {
        return None;
    }
    Some(norm.split('.').map(|s| s.to_string()).collect())
}

/// The UTF-8 text of `node` in `source`, if valid.
fn node_text<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    node.utf8_text(source.as_bytes()).ok()
}

/// True when `node` is a bare `identifier` whose text equals `name` exactly.
fn ident_is(node: &Node, source: &str, name: &str) -> bool {
    node.kind() == "identifier" && node_text(node, source) == Some(name)
}

/// True when `callee`'s node STRUCTURE is a known env-reading FUNCTION for
/// `language`, anchored on the REAL qualifier by inspecting the AST shape rather
/// than a flattened path suffix (M4.2 BUG-1 fix). This rejects field/method
/// receivers that merely END in the right name (`self.env.var(...)`,
/// `obj.os.environ.get(...)`, a user-defined bare `getenv(...)`).
fn env_callee_matches(callee: &Node, source: &str, language: Language) -> bool {
    match language {
        Language::Rust => rust_env_callee_matches(callee, source),
        Language::Go => go_env_callee_matches(callee, source),
        Language::Python => py_env_callee_matches(callee, source),
        _ => false,
    }
}

/// Rust env callee: a `scoped_identifier` PATH (never a `field_expression`)
/// ending in `env::var` / `env::var_os`, where the segment immediately before
/// `var`/`var_os` is exactly the module `env` (so `env::var` and `std::env::var`
/// match; `self.env.var(...)` / `foo.env.var(...)` — `field_expression` callees —
/// are rejected because their kind is not `scoped_identifier`).
fn rust_env_callee_matches(callee: &Node, source: &str) -> bool {
    if callee.kind() != "scoped_identifier" {
        return false;
    }
    let Some(name) = callee.child_by_field_name("name") else {
        return false;
    };
    let name_txt = node_text(&name, source);
    if name_txt != Some("var") && name_txt != Some("var_os") {
        return false;
    }
    let Some(path) = callee.child_by_field_name("path") else {
        return false;
    };
    match path.kind() {
        // `env::var` — the whole qualifier is the single identifier `env`.
        "identifier" => node_text(&path, source) == Some("env"),
        // `std::env::var` (or `a::b::env::var`) — the LAST path segment is `env`.
        "scoped_identifier" => {
            path.child_by_field_name("name")
                .and_then(|n| node_text(&n, source))
                == Some("env")
        }
        _ => false,
    }
}

/// Go env callee: a `selector_expression` `os.Getenv` / `os.LookupEnv` whose
/// `operand` is the bare identifier `os` (rejects other receivers / nested
/// selectors such as `foo.os.Getenv(...)`).
fn go_env_callee_matches(callee: &Node, source: &str) -> bool {
    if callee.kind() != "selector_expression" {
        return false;
    }
    let Some(field) = callee.child_by_field_name("field") else {
        return false;
    };
    let field_txt = node_text(&field, source);
    if field_txt != Some("Getenv") && field_txt != Some("LookupEnv") {
        return false;
    }
    callee
        .child_by_field_name("operand")
        .is_some_and(|operand| ident_is(&operand, source, "os"))
}

/// Python env callee: an `attribute` `os.getenv` (object = identifier `os`) or
/// `os.environ.get` (object = the `os.environ` attribute). A bare `getenv(...)`
/// (identifier callee), `self.getenv(...)`, or `obj.os.getenv(...)` is rejected.
fn py_env_callee_matches(callee: &Node, source: &str) -> bool {
    if callee.kind() != "attribute" {
        return false;
    }
    let Some(attr) = callee.child_by_field_name("attribute") else {
        return false;
    };
    let Some(object) = callee.child_by_field_name("object") else {
        return false;
    };
    match node_text(&attr, source) {
        Some("getenv") => ident_is(&object, source, "os"),
        Some("get") => py_is_os_environ(&object, source),
        _ => false,
    }
}

/// True when `node`'s STRUCTURE is a known env-mapping CONTAINER for `language`,
/// anchored on the real qualifier (M4.2 BUG-1 fix): Python `os.environ`, TS/JS
/// `process.env`.
fn env_container_matches(node: &Node, source: &str, language: Language) -> bool {
    match language {
        Language::Python => py_is_os_environ(node, source),
        Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx => ts_is_process_env(node, source),
        _ => false,
    }
}

/// True when `node` is the Python `os.environ` `attribute` whose object is the
/// bare identifier `os` (rejects bare `environ`, `self.environ`, `obj.os.environ`).
fn py_is_os_environ(node: &Node, source: &str) -> bool {
    if node.kind() != "attribute" {
        return false;
    }
    let Some(attr) = node.child_by_field_name("attribute") else {
        return false;
    };
    if node_text(&attr, source) != Some("environ") {
        return false;
    }
    node.child_by_field_name("object")
        .is_some_and(|object| ident_is(&object, source, "os"))
}

/// True when `node` is the TS/JS `process.env` `member_expression` whose object
/// is the bare identifier `process` (rejects `foo.process.env`, where the
/// `.env` member's object is a nested member access, not the bare `process`).
fn ts_is_process_env(node: &Node, source: &str) -> bool {
    if node.kind() != "member_expression" {
        return false;
    }
    let Some(property) = node.child_by_field_name("property") else {
        return false;
    };
    if node_text(&property, source) != Some("env") {
        return false;
    }
    node.child_by_field_name("object")
        .is_some_and(|object| ident_is(&object, source, "process"))
}

/// The KEY carried by the first argument of a call node (`arguments` field): a
/// string literal's value, or — via `const_map` — a bare identifier's bound
/// literal. `None` if there is no argument or it is neither.
fn first_string_arg(
    call_node: &Node,
    source: &str,
    const_map: &HashMap<String, String>,
) -> Option<String> {
    let args = call_node.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let first = args.named_children(&mut cursor).next()?;
    literal_or_const(&first, source, const_map)
}

/// The string value of `node` when it is a string literal, otherwise — when it is
/// a bare/scoped identifier — its module-level constant binding (keyed on the last
/// path segment). `None` for anything else. This is the constant-propagation hook
/// for env KEYs.
fn literal_or_const(
    node: &Node,
    source: &str,
    const_map: &HashMap<String, String>,
) -> Option<String> {
    if let Some(value) = string_literal_value(node, source) {
        return Some(value);
    }
    let segs = node_path_text(node, source)?;
    let name = segs.last()?;
    const_map.get(name).cloned()
}

/// The literal text of a string node (quotes / string prefixes stripped), across
/// the per-language string node kinds. `None` for any non-string node.
fn string_literal_value(node: &Node, source: &str) -> Option<String> {
    let is_string = matches!(
        node.kind(),
        "string" | "string_literal" | "interpreted_string_literal" | "raw_string_literal"
    );
    if !is_string {
        return None;
    }
    // Prefer the inner content node (excludes quotes / `r"`/`b"` prefixes).
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(
            child.kind(),
            "string_content" | "string_fragment" | "interpreted_string_literal_content"
        ) {
            return child
                .utf8_text(source.as_bytes())
                .ok()
                .map(|s| s.to_string());
        }
    }
    // Fallback (e.g. an empty string with no content child): strip one matching
    // surrounding quote pair.
    let text = node.utf8_text(source.as_bytes()).ok()?.trim();
    let bytes = text.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'' || first == b'`') && last == first {
            return Some(text[1..text.len() - 1].to_string());
        }
    }
    Some(text.to_string())
}

/// True when `name` is a PLAUSIBLE environment-variable KEY (M4.2, relaxed now
/// that accessors are structurally anchored — only genuine env reads reach here).
/// Accepts a non-empty run of ASCII letters (EITHER case), digits, and
/// underscores containing at least one letter — so conventional upper-case keys
/// (`DATABASE_URL`, `PORT`) AND real lower-case keys (`http_proxy`, `no_proxy`,
/// `npm_config_cache`) both qualify, while pure-digit/underscore noise (`123`,
/// `__`) and any key carrying other characters (`A-B`, dynamic/computed names)
/// are rejected. For `process.env.X` the property `X` is always a real env key.
fn is_env_var_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut has_letter = false;
    for c in name.chars() {
        if c.is_ascii_alphabetic() {
            has_letter = true;
        } else if c.is_ascii_digit() || c == '_' {
            // allowed, but not letter-bearing on its own
        } else {
            return false;
        }
    }
    has_letter
}

/// Build the per-file MODULE-LEVEL constant map (M4.2 constant propagation,
/// BUG-2 fix): `NAME = "string literal"` bindings declared at FILE / MODULE scope
/// ONLY — never inside a function/method/class body. Used to resolve
/// bare-identifier env KEYs (`const KEY = "DATABASE_URL"; env::var(KEY)`).
///
/// Scope is determined structurally during the walk: the first time the DFS
/// descends through a function/method/class node (`introduces_local_scope`) the
/// `in_local_scope` flag latches, gating out every binding nested below it — so a
/// function-local `const` can never substitute across unrelated functions.
///
/// Ambiguity: if the SAME module-level name maps to DIFFERENT literals it is
/// dropped from the map and never re-added (no substitution); identical
/// re-bindings of the same literal are harmless and kept.
fn collect_module_constants(
    root: &Node,
    language: Language,
    source: &str,
    out: &mut HashMap<String, String>,
) {
    let mut ambiguous: HashSet<String> = HashSet::new();
    walk_module_constants(root, language, source, false, out, &mut ambiguous);
}

/// Recursive worker for `collect_module_constants`. `in_local_scope` is true once
/// the walk has descended into a function/method/class body.
fn walk_module_constants(
    node: &Node,
    language: Language,
    source: &str,
    in_local_scope: bool,
    out: &mut HashMap<String, String>,
    ambiguous: &mut HashSet<String>,
) {
    if !in_local_scope {
        if let Some((name, value)) = constant_binding(node, language, source) {
            if !ambiguous.contains(&name) {
                match out.get(&name) {
                    // Same module-level name bound to a DIFFERENT literal → drop.
                    Some(existing) if *existing != value => {
                        out.remove(&name);
                        ambiguous.insert(name);
                    }
                    // First binding, or an identical re-binding (kept).
                    _ => {
                        out.insert(name, value);
                    }
                }
            }
        }
    }
    // Descendants of a function/method/class body are NOT module-level.
    let child_local = in_local_scope || introduces_local_scope(node, language);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_module_constants(&child, language, source, child_local, out, ambiguous);
    }
}

/// True when `node` opens a non-module (function / method / class) scope, so any
/// `NAME = "literal"` binding nested inside it is local and must NOT enter the
/// module constant map. (Unknown kind strings simply never match a real grammar
/// node, so over-listing is harmless.)
fn introduces_local_scope(node: &Node, language: Language) -> bool {
    match language {
        Language::Rust => matches!(node.kind(), "function_item" | "closure_expression"),
        Language::Python => matches!(node.kind(), "function_definition" | "class_definition"),
        Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx => matches!(
            node.kind(),
            "function_declaration"
                | "function_expression"
                | "generator_function_declaration"
                | "arrow_function"
                | "method_definition"
                | "class_declaration"
                | "class"
        ),
        Language::Go => matches!(
            node.kind(),
            "function_declaration" | "method_declaration" | "func_literal"
        ),
        _ => false,
    }
}

/// If `node` is a simple `NAME = "string literal"` binding for `language`, return
/// `(NAME, literal)`. Per-language binding sites:
/// - Rust: `const_item` / `static_item` (`name` + string `value`).
/// - Python: `assignment` (identifier `left` + string `right`).
/// - TS/JS: `variable_declarator` (identifier `name` + string `value`).
/// - Go: `const_spec` / `var_spec` (identifier `name` + string in the `value`
///   `expression_list`).
fn constant_binding(node: &Node, language: Language, source: &str) -> Option<(String, String)> {
    let pair = match language {
        Language::Rust => {
            if matches!(node.kind(), "const_item" | "static_item") {
                node.child_by_field_name("name")
                    .zip(node.child_by_field_name("value"))
            } else {
                None
            }
        }
        Language::Python => {
            if node.kind() == "assignment" {
                node.child_by_field_name("left")
                    .zip(node.child_by_field_name("right"))
            } else {
                None
            }
        }
        Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx => {
            if node.kind() == "variable_declarator" {
                node.child_by_field_name("name")
                    .zip(node.child_by_field_name("value"))
            } else {
                None
            }
        }
        Language::Go => {
            if matches!(node.kind(), "const_spec" | "var_spec") {
                node.child_by_field_name("name")
                    .zip(node.child_by_field_name("value"))
            } else {
                None
            }
        }
        _ => None,
    };
    let (name_node, mut value_node) = pair?;
    // Only a single, plain identifier name (skip tuple/array/object patterns).
    if name_node.kind() != "identifier" {
        return None;
    }
    // Go wraps the binding value in an `expression_list`; unwrap its first entry.
    if value_node.kind() == "expression_list" {
        value_node = value_node.named_child(0)?;
    }
    let value = string_literal_value(&value_node, source)?;
    let name = node_text(&name_node, source)?;
    Some((name.to_string(), value))
}

/// The first named child of `node` whose kind is `kind`, if any.
fn first_child_of_kind<'tree>(node: &Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).find(|c| c.kind() == kind);
    found
}

/// Insert the NAME of one declared type parameter into `out`. The name is the
/// node itself when it is already a `type_identifier`, otherwise the first
/// `type_identifier` DIRECT child (e.g. Rust/TS `type_parameter` → its leading
/// `type_identifier`; any trailing bound like `: Clone` / `extends X` is nested
/// deeper and is intentionally not treated as the name).
fn insert_type_param_name(node: &Node, source: &str, out: &mut HashSet<String>) {
    if node.kind() == "type_identifier" {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            out.insert(text.to_string());
        }
        return;
    }
    if let Some(name) = first_child_of_kind(node, "type_identifier") {
        if let Ok(text) = name.utf8_text(source.as_bytes()) {
            out.insert(text.to_string());
        }
    }
}

/// Insert every direct `identifier` child of `node` into `out` (Go/Python name
/// position uses `identifier`, not `type_identifier`).
fn insert_identifier_names(node: &Node, source: &str, out: &mut HashSet<String>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "identifier" {
            if let Ok(text) = child.utf8_text(source.as_bytes()) {
                out.insert(text.to_string());
            }
        }
    }
}

/// Collect the set of type-parameter names that THIS function/method DECLARES,
/// from the structural declaration site on its signature node (M4.1 noise fix).
///
/// A function's own generic parameters (`fn f<T, K, V>` / `function f<T, U>` /
/// `func F[T any, K comparable]` / PEP-695 `def f[T, K]`) are NOT referenced
/// types — they are introduced by the signature itself — so the caller filters
/// them out of the emitted `uses_type` targets. Per-language declaration sites:
/// - Rust / TS family: a `type_parameters` child; each entry's name is its
///   leading `type_identifier` (bounds are nested and ignored).
/// - Go: a `type_parameter_list` child; each `type_parameter_declaration` names
///   one-or-more params via `identifier` leaves (the constraint is a sibling).
/// - Python: a PEP-695 `type_parameter` child; each `type` wraps an `identifier`.
///
/// Old-style Python `TypeVar`s assigned elsewhere are invisible from the
/// signature and are a known minor limitation (not handled here).
fn collect_declared_type_params(
    sig_node: &Node,
    language: Language,
    source: &str,
    out: &mut HashSet<String>,
) {
    match language {
        Language::Rust
        | Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx => {
            if let Some(tp) = first_child_of_kind(sig_node, "type_parameters") {
                let mut cursor = tp.walk();
                for child in tp.named_children(&mut cursor) {
                    insert_type_param_name(&child, source, out);
                }
            }
        }
        Language::Go => {
            if let Some(tpl) = first_child_of_kind(sig_node, "type_parameter_list") {
                let mut cursor = tpl.walk();
                for decl in tpl.named_children(&mut cursor) {
                    insert_identifier_names(&decl, source, out);
                }
            }
        }
        Language::Python => {
            // The list node kind is `type_parameter` (singular) holding `type`
            // entries that each wrap the name `identifier`.
            if let Some(tp) = first_child_of_kind(sig_node, "type_parameter") {
                let mut cursor = tp.walk();
                for child in tp.named_children(&mut cursor) {
                    if child.kind() == "identifier" {
                        if let Ok(text) = child.utf8_text(source.as_bytes()) {
                            out.insert(text.to_string());
                        }
                    } else {
                        insert_identifier_names(&child, source, out);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Collect the type-name leaves referenced inside a type subtree (M4.1).
///
/// Recurses through generics, references, slices, pointers, maps, etc., so a
/// container like `Result<Widget>` / `map[string]Widget` / `List[Widget]` yields
/// its INNER named types (the container itself is dropped later by
/// `is_builtin_type`). This is "recurse into generic args" done structurally via
/// tree-sitter, which also avoids lifetime / keyword / module-prefix noise that a
/// purely textual parse would hit.
///
/// Per-language leaf kinds:
/// - Rust / TS family / Go: type names are `type_identifier` leaves. Rust/TS
///   primitives are distinct node kinds (`primitive_type` / `predefined_type`)
///   and never appear here; Go predeclared types ARE `type_identifier` and are
///   dropped by name in `is_builtin_type`.
/// - Python: type positions are expressions, so type names are `identifier`
///   leaves (filtered by name). An `attribute` (e.g. `typing.Optional`) is
///   reduced to its rightmost segment so the module prefix is not emitted.
fn collect_type_names(node: &Node, language: Language, source: &str, out: &mut Vec<String>) {
    match language {
        Language::Python => match node.kind() {
            "identifier" => {
                if let Ok(text) = node.utf8_text(source.as_bytes()) {
                    out.push(text.to_string());
                }
            }
            // `a.b.C` → keep only `C` (drop the module/object path).
            "attribute" => {
                if let Some(attr) = node.child_by_field_name("attribute") {
                    collect_type_names(&attr, language, source, out);
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    collect_type_names(&child, language, source, out);
                }
            }
        },
        _ => {
            if node.kind() == "type_identifier" {
                if let Ok(text) = node.utf8_text(source.as_bytes()) {
                    out.push(text.to_string());
                }
                return;
            }
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_type_names(&child, language, source, out);
            }
        }
    }
}

/// Normalize one collected type leaf into a bare base type name (M4.1).
///
/// Strips wrapper sigils/keywords (`&`, `*`, `mut`, `dyn`, `impl`, `const`),
/// unwraps any residual generic (`Foo<Bar>` → `Foo`), drops slice/array/option
/// punctuation (`[]`, `?`), and takes the last path segment (`a::B`/`a.B` → `B`).
/// `collect_type_names` already yields atomic leaves, so this is mostly a
/// validating normalizer; it returns `None` for anything that is not a plain
/// identifier (e.g. a stray lifetime or punctuation token).
fn clean_type_name(raw: &str) -> Option<String> {
    let mut value = raw.trim();

    // Peel leading reference/pointer sigils and type keywords, repeatedly.
    loop {
        let before = value;
        value = value.trim_start_matches(['&', '*']).trim_start();
        for keyword in ["mut ", "dyn ", "impl ", "const "] {
            if let Some(rest) = value.strip_prefix(keyword) {
                value = rest.trim_start();
            }
        }
        if value == before {
            break;
        }
    }

    // Drop any generic argument list, slice/array brackets, and option marker.
    let value = value.split('<').next().unwrap_or(value);
    let value = value.trim_matches(|c| matches!(c, '[' | ']' | '?' | '(' | ')')).trim();

    // Keep the final path segment of a qualified name.
    let value = value.rsplit("::").next().unwrap_or(value);
    let value = value.rsplit('.').next().unwrap_or(value).trim();

    // Accept only a plain identifier (defensive against lifetimes / tokens).
    let mut chars = value.chars();
    let first_ok = chars
        .next()
        .map(|c| c.is_alphabetic() || c == '_')
        .unwrap_or(false);
    if !first_ok || !value.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(value.to_string())
}

/// True when `name` is a language primitive / ubiquitous std container whose
/// `uses_type` edge would be pure noise (M4.1). Container generics (e.g. Rust
/// `Vec`/`Result`, TS `Promise`/`Array`, Python `Optional`/`List`) are dropped
/// *here* while their inner type was already kept by `collect_type_names`. JS/JSX
/// have no type annotations and unsupported languages emit nothing, so they treat
/// every name as builtin (skip).
fn is_builtin_type(language: Language, name: &str) -> bool {
    match language {
        Language::Rust => RUST_BUILTIN_TYPES.contains(&name),
        Language::TypeScript | Language::Tsx | Language::Astro => TS_BUILTIN_TYPES.contains(&name),
        Language::Python => PYTHON_BUILTIN_TYPES.contains(&name),
        Language::Go => GO_BUILTIN_TYPES.contains(&name),
        _ => true,
    }
}

/// Rust primitives + `String`/`Self` + the ubiquitous std smart-pointer /
/// collection generics + the closure traits. The container names are dropped (low
/// signal); their inner type was already collected, so `Result<Widget>` still
/// yields `Widget` and `&dyn Fn(In) -> Out` still yields `In`/`Out` while the
/// `Fn`/`FnMut`/`FnOnce` trait itself is noise.
static RUST_BUILTIN_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32",
    "f64", "bool", "char", "str", "String", "Self", "Vec", "Option", "Result", "Box", "Rc", "Arc",
    "Weak", "Cell", "RefCell", "Mutex", "RwLock", "Cow", "Pin", "HashMap", "HashSet", "BTreeMap",
    "BTreeSet", "VecDeque", "BinaryHeap", "LinkedList", "Fn", "FnMut", "FnOnce",
];

/// TypeScript primitive/utility types + global container generics (their inner
/// type was already collected).
static TS_BUILTIN_TYPES: &[&str] = &[
    "string",
    "number",
    "boolean",
    "void",
    "any",
    "unknown",
    "never",
    "null",
    "undefined",
    "object",
    "symbol",
    "bigint",
    "this",
    "Promise",
    "Array",
    "ReadonlyArray",
    "Map",
    "ReadonlyMap",
    "Set",
    "ReadonlySet",
    "WeakMap",
    "WeakSet",
    "Record",
    "Partial",
    "Required",
    "Readonly",
    "Pick",
    "Omit",
    "Exclude",
    "Extract",
    "NonNullable",
];

/// Python builtins + `typing` container generics (inner type already collected).
static PYTHON_BUILTIN_TYPES: &[&str] = &[
    "int",
    "str",
    "bool",
    "float",
    "complex",
    "bytes",
    "bytearray",
    "memoryview",
    "None",
    "NoneType",
    "object",
    "type",
    "list",
    "dict",
    "set",
    "frozenset",
    "tuple",
    "range",
    "Any",
    "List",
    "Dict",
    "Set",
    "FrozenSet",
    "Tuple",
    "Optional",
    "Union",
    "Sequence",
    "Mapping",
    "MutableMapping",
    "Iterable",
    "Iterator",
    "Callable",
    "Awaitable",
    "Coroutine",
    "Type",
    "ClassVar",
    "Final",
    "Annotated",
    "Literal",
];

/// Go predeclared types. Go has no container *type names* (`map`/`[]`/`chan` are
/// distinct node kinds), so only the predeclared identifiers need dropping.
static GO_BUILTIN_TYPES: &[&str] = &[
    "bool",
    "string",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    "byte",
    "rune",
    "float32",
    "float64",
    "complex64",
    "complex128",
    "error",
    "any",
    "comparable",
];

/// Scope-stack analogue of `find_enclosing_symbol`: the smallest-range scope
/// containing `range`, ties broken by earliest-pushed (outermost). Because the
/// stack holds exactly the symbol-creating ancestors in pre-order — the same set
/// `find_enclosing_symbol` would match (range containment ⇔ ancestry in a tree),
/// in the same relative order it scans `symbols` — this reproduces its result
/// (including the equal-range tie-break) at O(depth) instead of O(symbols).
fn resolve_enclosing_scope<'a>(scope: &'a [Scope], range: &Range) -> Option<&'a Scope> {
    scope
        .iter()
        .filter(|frame| {
            starts_before_or_at(frame.range.start, range.start)
                && starts_before_or_at(range.end, frame.range.end)
        })
        .min_by_key(|frame| {
            let line_span = frame
                .range
                .end
                .line
                .saturating_sub(frame.range.start.line);
            let char_span = frame
                .range
                .end
                .character
                .saturating_sub(frame.range.start.character);
            (line_span, char_span)
        })
}

fn starts_before_or_at(left: Position, right: Position) -> bool {
    left.line < right.line || (left.line == right.line && left.character <= right.character)
}

pub(crate) fn stable_symbol_id(
    file_path: &str,
    qualified_name: &str,
    symbol_type: SymbolType,
) -> String {
    format!("{}::{}#{}", file_path, qualified_name, symbol_type)
}

fn compute_content_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter::TreeSitterParser;

    #[test]
    fn test_extract_typescript_function() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "function authenticate(token: string): boolean { return true; }";
        let tree = parser.parse(code, Language::TypeScript).unwrap();

        let symbols = extract_symbols(&tree, code, Language::TypeScript, "test.ts");

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "authenticate");
        assert_eq!(symbols[0].symbol_type, SymbolType::Function);
    }

    #[test]
    fn test_extract_typescript_class() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
class UserService {
    private users: User[] = [];

    getUser(id: string): User | undefined {
        return this.users.find(u => u.id === id);
    }
}
"#;
        let tree = parser.parse(code, Language::TypeScript).unwrap();
        let symbols = extract_symbols(&tree, code, Language::TypeScript, "service.ts");

        // Should have class and method
        assert!(symbols
            .iter()
            .any(|s| s.name == "UserService" && s.symbol_type == SymbolType::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "getUser" && s.symbol_type == SymbolType::Method));
    }

    #[test]
    fn test_extract_typescript_import() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "import { helper } from './utils';\nfunction main() { helper(); }";
        let tree = parser.parse(code, Language::TypeScript).unwrap();
        let symbols = extract_symbols(&tree, code, Language::TypeScript, "main.ts");

        assert!(symbols
            .iter()
            .any(|s| s.name == "./utils" && s.symbol_type == SymbolType::Import));
    }

    #[test]
    fn test_extract_typescript_variable_function_symbols() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
export const authenticateUser = async (token: string): Promise<boolean> => true;
const normalizeToken = function (token: string): string { return token.trim(); };
let retryCount = 0;
"#;
        let tree = parser.parse(code, Language::TypeScript).unwrap();
        let symbols = extract_symbols(&tree, code, Language::TypeScript, "auth.ts");

        assert!(symbols
            .iter()
            .any(|s| s.name == "authenticateUser" && s.symbol_type == SymbolType::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "normalizeToken" && s.symbol_type == SymbolType::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "retryCount" && s.symbol_type == SymbolType::Variable));
    }

    #[test]
    fn test_extract_tsx_react_component_symbols() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
import React, { memo, forwardRef } from "react";

export const UserCard = memo(function UserCard({ user }: Props) {
    return <section>{user.name}</section>;
});

const SearchInput = forwardRef<HTMLInputElement, Props>((props, ref) => {
    return <input ref={ref} />;
});
"#;
        let tree = parser.parse(code, Language::Tsx).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Tsx, "UserCard.tsx");

        assert!(symbols
            .iter()
            .any(|s| s.name == "UserCard" && s.symbol_type == SymbolType::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "SearchInput" && s.symbol_type == SymbolType::Function));
    }

    #[test]
    fn test_extract_javascript_object_function_symbols() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
const handlers = {
    authenticate(req) {
        return true;
    },
    refresh: async (token) => token,
};
"#;
        let tree = parser.parse(code, Language::JavaScript).unwrap();
        let symbols = extract_symbols(&tree, code, Language::JavaScript, "handlers.js");

        assert!(symbols
            .iter()
            .any(|s| s.name == "authenticate" && s.symbol_type == SymbolType::Method));
        assert!(symbols
            .iter()
            .any(|s| s.name == "refresh" && s.symbol_type == SymbolType::Method));
    }

    #[test]
    fn test_extract_typescript_call_relationships() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
function helperName(): string {
    return "helper";
}

function greetUser(): string {
    return helperName();
}
"#;
        let tree = parser.parse(code, Language::TypeScript).unwrap();
        let symbols = extract_symbols(&tree, code, Language::TypeScript, "main.ts");
        let relationships =
            extract_symbol_relationships(&tree, code, Language::TypeScript, "main.ts", &symbols);

        assert!(relationships.iter().any(|relationship| {
            relationship.target_name == "helperName"
                && relationship.relationship_type == SymbolRelationshipType::Call
        }));
    }

    #[test]
    fn test_extract_typescript_import_relationships() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
import { helper } from "./utils";

function run(): string {
    return helper();
}
"#;
        let tree = parser.parse(code, Language::TypeScript).unwrap();
        let symbols = extract_symbols(&tree, code, Language::TypeScript, "main.ts");
        let relationships =
            extract_symbol_relationships(&tree, code, Language::TypeScript, "main.ts", &symbols);

        assert!(relationships.iter().any(|relationship| {
            relationship.target_name == "./utils"
                && relationship.relationship_type == SymbolRelationshipType::Import
        }));
    }

    #[test]
    fn test_extract_typescript_structural_relationships() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
interface Service extends BaseService, Auditable {}

class UserService extends CoreService implements Service, Disposable {
    run() {}
}
"#;
        let tree = parser.parse(code, Language::TypeScript).unwrap();
        let symbols = extract_symbols(&tree, code, Language::TypeScript, "service.ts");
        let relationships =
            extract_symbol_relationships(&tree, code, Language::TypeScript, "service.ts", &symbols);

        assert!(relationships.iter().any(|relationship| {
            relationship.source_symbol_id == "service.ts::Service#interface"
                && relationship.target_name == "BaseService"
                && relationship.relationship_type == SymbolRelationshipType::Extends
        }));
        assert!(relationships.iter().any(|relationship| {
            relationship.source_symbol_id == "service.ts::UserService#class"
                && relationship.target_name == "CoreService"
                && relationship.relationship_type == SymbolRelationshipType::Extends
        }));
        assert!(relationships.iter().any(|relationship| {
            relationship.source_symbol_id == "service.ts::UserService#class"
                && relationship.target_name == "Service"
                && relationship.relationship_type == SymbolRelationshipType::Implements
        }));
    }

    #[test]
    fn test_extract_python_class_extends_relationships() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
class Service(BaseService, Auditable):
    pass
"#;
        let tree = parser.parse(code, Language::Python).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Python, "service.py");
        let relationships =
            extract_symbol_relationships(&tree, code, Language::Python, "service.py", &symbols);

        assert!(relationships.iter().any(|relationship| {
            relationship.source_symbol_id == "service.py::Service#class"
                && relationship.target_name == "BaseService"
                && relationship.relationship_type == SymbolRelationshipType::Extends
        }));
        assert!(relationships.iter().any(|relationship| {
            relationship.source_symbol_id == "service.py::Service#class"
                && relationship.target_name == "Auditable"
                && relationship.relationship_type == SymbolRelationshipType::Extends
        }));
    }

    #[test]
    fn test_extract_rust_impl_relationships() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
trait Renderable {}

struct Button;

impl Renderable for Button {}
"#;
        let tree = parser.parse(code, Language::Rust).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Rust, "lib.rs");
        let relationships =
            extract_symbol_relationships(&tree, code, Language::Rust, "lib.rs", &symbols);

        assert!(relationships.iter().any(|relationship| {
            relationship.source_symbol_id == "lib.rs::Button#struct"
                && relationship.target_name == "Renderable"
                && relationship.relationship_type == SymbolRelationshipType::Implements
        }));
    }

    #[test]
    fn test_extract_rust_function() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "fn greet(name: &str) -> String { format!(\"Hello, {}!\", name) }";
        let tree = parser.parse(code, Language::Rust).unwrap();

        let symbols = extract_symbols(&tree, code, Language::Rust, "lib.rs");

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "greet");
        assert_eq!(symbols[0].symbol_type, SymbolType::Function);
    }

    #[test]
    fn test_extract_python_class() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
class Calculator:
    def add(self, a: int, b: int) -> int:
        return a + b

    def subtract(self, a: int, b: int) -> int:
        return a - b
"#;
        let tree = parser.parse(code, Language::Python).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Python, "calc.py");

        assert!(symbols
            .iter()
            .any(|s| s.name == "Calculator" && s.symbol_type == SymbolType::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "add" && s.symbol_type == SymbolType::Method));
        assert!(symbols
            .iter()
            .any(|s| s.name == "subtract" && s.symbol_type == SymbolType::Method));
    }

    #[test]
    fn test_extract_go_symbols() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
package main

import "fmt"

type Server struct{}
type Store interface{ Save() }

const Version = "v1"
var ready = true

func Run() {
    fmt.Println("ok")
}

func (s *Server) Handle() error {
    return nil
}
"#;
        let tree = parser.parse(code, Language::Go).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Go, "main.go");

        assert!(symbols
            .iter()
            .any(|s| s.name == "fmt" && s.symbol_type == SymbolType::Import));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Server" && s.symbol_type == SymbolType::Struct));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Store" && s.symbol_type == SymbolType::Interface));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Version" && s.symbol_type == SymbolType::Constant));
        assert!(symbols
            .iter()
            .any(|s| s.name == "ready" && s.symbol_type == SymbolType::Variable));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Run" && s.symbol_type == SymbolType::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Handle" && s.symbol_type == SymbolType::Method));
    }

    /// Collect the `uses_type` target names attributed to a given source
    /// qualified-name from a freshly-extracted file.
    fn uses_type_targets(
        code: &str,
        language: Language,
        path: &str,
        source_qn: &str,
    ) -> Vec<String> {
        let mut parser = TreeSitterParser::new().unwrap();
        let tree = parser.parse(code, language).unwrap();
        let symbols = extract_symbols(&tree, code, language, path);
        let relationships = extract_symbol_relationships(&tree, code, language, path, &symbols);
        let id_for = |qn: &str| -> Option<String> {
            symbols
                .iter()
                .find(|s| s.qualified_name == qn)
                .map(|s| s.id.clone())
        };
        let source_id = id_for(source_qn).expect("source symbol not found");
        let mut targets: Vec<String> = relationships
            .iter()
            .filter(|r| {
                r.relationship_type == SymbolRelationshipType::UsesType
                    && r.source_symbol_id == source_id
            })
            .map(|r| r.target_name.clone())
            .collect();
        targets.sort();
        targets
    }

    /// Collect the `reads_env` target KEYs attributed to a given source
    /// qualified-name from a freshly-extracted file (M4.2).
    fn reads_env_targets(
        code: &str,
        language: Language,
        path: &str,
        source_qn: &str,
    ) -> Vec<String> {
        let mut parser = TreeSitterParser::new().unwrap();
        let tree = parser.parse(code, language).unwrap();
        let symbols = extract_symbols(&tree, code, language, path);
        let relationships = extract_symbol_relationships(&tree, code, language, path, &symbols);
        let source_id = symbols
            .iter()
            .find(|s| s.qualified_name == source_qn)
            .map(|s| s.id.clone())
            .expect("source symbol not found");
        let mut targets: Vec<String> = relationships
            .iter()
            .filter(|r| {
                r.relationship_type == SymbolRelationshipType::ReadsEnv
                    && r.source_symbol_id == source_id
            })
            .map(|r| r.target_name.clone())
            .collect();
        targets.sort();
        targets
    }

    #[test]
    fn test_reads_env_literal_keys_all_languages() {
        // Rust: `std::env::var` / `env::var_os`, both string-literal KEYs.
        let rust = "fn load() {\n    let _ = std::env::var(\"API_KEY\");\n    let _ = env::var_os(\"HOME_DIR\");\n}\n";
        assert_eq!(
            reads_env_targets(rust, Language::Rust, "lib.rs", "load"),
            vec!["API_KEY".to_string(), "HOME_DIR".to_string()]
        );

        // Python: `os.environ[...]` subscript, `os.environ.get`, `os.getenv`.
        let py = "import os\ndef load():\n    a = os.environ[\"DATABASE_URL\"]\n    b = os.environ.get(\"REDIS_URL\")\n    c = os.getenv(\"PORT\")\n    return (a, b, c)\n";
        assert_eq!(
            reads_env_targets(py, Language::Python, "m.py", "load"),
            vec![
                "DATABASE_URL".to_string(),
                "PORT".to_string(),
                "REDIS_URL".to_string()
            ]
        );

        // TS/JS: member access (KEY is the property name) and subscript string.
        let ts = "function load(): void {\n    const a = process.env.NODE_ENV;\n    const b = process.env[\"PORT\"];\n}\n";
        assert_eq!(
            reads_env_targets(ts, Language::TypeScript, "m.ts", "load"),
            vec!["NODE_ENV".to_string(), "PORT".to_string()]
        );

        // Go: `os.Getenv` / `os.LookupEnv`.
        let go = "package main\nfunc load() {\n    _ = os.Getenv(\"HOME\")\n    _, _ = os.LookupEnv(\"PATH\")\n}\n";
        assert_eq!(
            reads_env_targets(go, Language::Go, "m.go", "load"),
            vec!["HOME".to_string(), "PATH".to_string()]
        );
    }

    #[test]
    fn test_reads_env_constant_propagation() {
        // A bare-identifier KEY resolves to its module-level string literal.
        let rust =
            "const KEY: &str = \"DATABASE_URL\";\nfn load() {\n    let _ = std::env::var(KEY);\n}\n";
        assert_eq!(
            reads_env_targets(rust, Language::Rust, "lib.rs", "load"),
            vec!["DATABASE_URL".to_string()]
        );

        // Same for Python: module-level `NAME = "literal"` feeds `os.getenv(NAME)`.
        let py = "import os\nKEY = \"REDIS_URL\"\ndef load():\n    return os.getenv(KEY)\n";
        assert_eq!(
            reads_env_targets(py, Language::Python, "m.py", "load"),
            vec!["REDIS_URL".to_string()]
        );
    }

    #[test]
    fn test_reads_env_unanchored_negative() {
        // BUG-1 regression: receivers that merely END in the right names must NOT
        // emit a `reads_env` edge — accessors are anchored on the REAL qualifier.

        // TS: `foo.process.env.PORT` is NOT the global `process.env` (the `.env`
        // member's object is a nested member access, not the bare `process`).
        let ts = "function load(foo: any): string {\n    return foo.process.env.PORT;\n}\n";
        assert!(
            reads_env_targets(ts, Language::TypeScript, "m.ts", "load").is_empty(),
            "foo.process.env.PORT must not emit reads_env"
        );

        // Rust: `self.env.var(\"X\")` is a FIELD access (`field_expression` callee),
        // not the `std::env::var` PATH.
        let rust = "struct S;\nimpl S {\n    fn load(&self) {\n        let _ = self.env.var(\"NOT_ENV\");\n    }\n}\n";
        assert!(
            reads_env_targets(rust, Language::Rust, "lib.rs", "S::load").is_empty(),
            "self.env.var(...) must not emit reads_env"
        );

        // Python: a user-defined `getenv` shadow called bare is NOT `os.getenv`.
        let py = "def getenv(name):\n    return name\ndef load():\n    return getenv(\"NOT_ENV\")\n";
        assert!(
            reads_env_targets(py, Language::Python, "m.py", "load").is_empty(),
            "user-defined getenv(...) must not emit reads_env"
        );

        // Python: `self.environ[...]` is not the `os.environ` mapping.
        let py2 = "def load(self):\n    return self.environ[\"NOT_ENV\"]\n";
        assert!(
            reads_env_targets(py2, Language::Python, "m.py", "load").is_empty(),
            "self.environ[...] must not emit reads_env"
        );

        // Go: a non-`os` receiver `cfg.Getenv(...)` must not emit.
        let go = "package main\nfunc load() string {\n    return cfg.Getenv(\"NOT_ENV\")\n}\n";
        assert!(
            reads_env_targets(go, Language::Go, "m.go", "load").is_empty(),
            "cfg.Getenv(...) must not emit reads_env"
        );
    }

    #[test]
    fn test_reads_env_anchored_positive_still_fires() {
        // Sanity guard for the BUG-1 fix: the genuine anchored forms keep firing.
        let ts = "function load(): string {\n    return process.env.PORT;\n}\n";
        assert_eq!(
            reads_env_targets(ts, Language::TypeScript, "m.ts", "load"),
            vec!["PORT".to_string()]
        );
        // Relaxed key rule: a real lower-case env property now fires.
        let ts_lower = "function load(): string {\n    return process.env.http_proxy;\n}\n";
        assert_eq!(
            reads_env_targets(ts_lower, Language::TypeScript, "m.ts", "load"),
            vec!["http_proxy".to_string()]
        );
    }

    #[test]
    fn test_reads_env_rejects_unresolved_and_garbage() {
        // An unresolved bare identifier (no module-level binding) yields nothing.
        let rust = "fn load() {\n    let _ = std::env::var(missing);\n}\n";
        assert!(reads_env_targets(rust, Language::Rust, "lib.rs", "load").is_empty());

        // A non-key index (pure digits) yields nothing even on a real accessor.
        let ts = "function load(): void {\n    const a = process.env[\"123\"];\n}\n";
        assert!(reads_env_targets(ts, Language::TypeScript, "m.ts", "load").is_empty());
    }

    #[test]
    fn test_reads_env_module_vs_local_const_scope() {
        // BUG-2 regression: two functions each declare a function-LOCAL `const`
        // with the SAME name but DIFFERENT literals. A local const must NOT be
        // collected, so neither `var(KEY)` resolves — no edge, and crucially no
        // WRONG cross-function substitution.
        let rust_local = "fn a() {\n    const KEY: &str = \"AAA\";\n    let _ = std::env::var(KEY);\n}\nfn b() {\n    const KEY: &str = \"BBB\";\n    let _ = std::env::var(KEY);\n}\n";
        assert!(
            reads_env_targets(rust_local, Language::Rust, "lib.rs", "a").is_empty(),
            "function-local const must not substitute (a)"
        );
        assert!(
            reads_env_targets(rust_local, Language::Rust, "lib.rs", "b").is_empty(),
            "function-local const must not substitute (b)"
        );

        // A MODULE-level const DOES substitute correctly.
        let rust_mod = "const KEY: &str = \"REAL_KEY\";\nfn a() {\n    let _ = std::env::var(KEY);\n}\n";
        assert_eq!(
            reads_env_targets(rust_mod, Language::Rust, "lib.rs", "a"),
            vec!["REAL_KEY".to_string()]
        );

        // Two CONFLICTING module-level consts of the same name → ambiguous → drop.
        let rust_ambig = "const KEY: &str = \"AAA\";\nconst KEY: &str = \"BBB\";\nfn a() {\n    let _ = std::env::var(KEY);\n}\n";
        assert!(
            reads_env_targets(rust_ambig, Language::Rust, "lib.rs", "a").is_empty(),
            "ambiguous module const must be dropped"
        );

        // Python: a function-local `KEY = \"X\"` must not leak to another function.
        let py_local = "import os\ndef a():\n    KEY = \"AAA\"\n    return os.getenv(KEY)\ndef b():\n    KEY = \"BBB\"\n    return os.getenv(KEY)\n";
        assert!(reads_env_targets(py_local, Language::Python, "m.py", "a").is_empty());
        assert!(reads_env_targets(py_local, Language::Python, "m.py", "b").is_empty());

        // Python: a module-level `NAME = \"literal\"` resolves correctly.
        let py_mod = "import os\nKEY = \"REAL_KEY\"\ndef a():\n    return os.getenv(KEY)\n";
        assert_eq!(
            reads_env_targets(py_mod, Language::Python, "m.py", "a"),
            vec!["REAL_KEY".to_string()]
        );
    }

    #[test]
    fn test_is_env_var_name_rule() {
        // Relaxed (M4.2): upper-case AND real lower-case keys are accepted.
        for ok in [
            "DATABASE_URL",
            "PORT",
            "NODE_ENV",
            "PORT2",
            "_PRIVATE",
            "A1",
            "http_proxy",
            "no_proxy",
            "npm_config_cache",
            "nodeEnv",
            "a",
        ] {
            assert!(is_env_var_name(ok), "{ok} should be accepted");
        }
        // Rejected: empty, pure digits, pure underscores, and stray characters.
        for bad in ["", "123", "__", "A-B", "a.b", "FOO BAR"] {
            assert!(!is_env_var_name(bad), "{bad} should be rejected");
        }
    }

    #[test]
    fn test_uses_type_rust_signature_edges() {
        // `Config` (param) and `Widget` (inner of `Result<Widget>`) are emitted;
        // the `bool` primitive and the `Result` container are NOT.
        let code = r#"
struct Config;
struct Widget;
fn f(c: Config, flag: bool) -> Result<Widget> {
    todo!()
}
"#;
        let targets = uses_type_targets(code, Language::Rust, "lib.rs", "f");
        assert!(targets.contains(&"Config".to_string()), "got {targets:?}");
        assert!(targets.contains(&"Widget".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"bool".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"Result".to_string()), "got {targets:?}");
    }

    #[test]
    fn test_uses_type_rust_nested_generics_and_refs() {
        // `&mut Vec<Box<dyn Trait>>` keeps the inner `Trait`; `HashMap<String,i32>`
        // is all-builtin so it yields nothing.
        let code = r#"
trait Trait {}
struct Item;
fn g(items: &mut Vec<Box<dyn Trait>>, m: std::collections::HashMap<String, i32>) -> Option<Item> {
    todo!()
}
"#;
        let targets = uses_type_targets(code, Language::Rust, "lib.rs", "g");
        assert_eq!(targets, vec!["Item".to_string(), "Trait".to_string()]);
    }

    #[test]
    fn test_uses_type_rust_excludes_generic_params_and_fn_traits() {
        // A function's OWN declared generic params (`<T, K, V>`) are declarations,
        // not references, so they must NOT be emitted. A real param type (`Config`)
        // IS emitted. The closure trait `Fn` is low-signal noise and is dropped,
        // while the closure's argument/return types (`In`/`Out`) are still kept.
        let code = r#"
struct Config;
struct In;
struct Out;
fn generic<T, K, V>(a: T, m: K, cfg: Config, f: &dyn Fn(In) -> Out) -> V {
    todo!()
}
"#;
        let targets = uses_type_targets(code, Language::Rust, "lib.rs", "generic");
        assert!(targets.contains(&"Config".to_string()), "got {targets:?}");
        assert!(targets.contains(&"In".to_string()), "got {targets:?}");
        assert!(targets.contains(&"Out".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"T".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"K".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"V".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"Fn".to_string()), "got {targets:?}");
    }

    #[test]
    fn test_uses_type_excludes_generic_params_ts_go_python() {
        // TypeScript `<T, U>` declared params excluded; real `Config` kept.
        let ts = "function gen<T, U>(a: T, b: Config): U { return b as any; }";
        let ts_targets = uses_type_targets(ts, Language::TypeScript, "m.ts", "gen");
        assert_eq!(ts_targets, vec!["Config".to_string()], "ts: {ts_targets:?}");

        // Go `[T any, K comparable]` declared params excluded; real `Config` kept.
        let go = "package main\nfunc Gen[T any, K comparable](a T, c Config) K { var z K; return z }";
        let go_targets = uses_type_targets(go, Language::Go, "m.go", "Gen");
        assert_eq!(go_targets, vec!["Config".to_string()], "go: {go_targets:?}");

        // Python PEP-695 `def gen[T, K]` declared params excluded; real `Config` kept.
        let py = "def gen[T, K](a: T, c: Config) -> K:\n    return a";
        let py_targets = uses_type_targets(py, Language::Python, "m.py", "gen");
        assert_eq!(py_targets, vec!["Config".to_string()], "py: {py_targets:?}");
    }

    #[test]
    fn test_uses_type_typescript_signature_edges() {
        // Arrow-const function: params/return live on the arrow value. `Config`
        // and `Widget` (inner of `Promise<Widget>`) emitted; `boolean` not.
        let code = r#"
const build = (c: Config, flag: boolean): Promise<Widget> => {
    return load(c);
};
"#;
        let targets = uses_type_targets(code, Language::TypeScript, "m.ts", "build");
        assert!(targets.contains(&"Config".to_string()), "got {targets:?}");
        assert!(targets.contains(&"Widget".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"boolean".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"Promise".to_string()), "got {targets:?}");
    }

    #[test]
    fn test_uses_type_python_signature_edges() {
        // `int`/`str` builtins dropped; `Config` param and `Widget` (inner of
        // `typing.Optional[Widget]`) kept; the `typing` module prefix is not a
        // target.
        let code = r#"
def f(c: Config, n: int, w: typing.Optional[Widget]) -> str:
    return str(n)
"#;
        let targets = uses_type_targets(code, Language::Python, "m.py", "f");
        assert!(targets.contains(&"Config".to_string()), "got {targets:?}");
        assert!(targets.contains(&"Widget".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"int".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"str".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"typing".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"Optional".to_string()), "got {targets:?}");
    }

    #[test]
    fn test_uses_type_go_signature_edges() {
        // `string`/`error` predeclared dropped; `Widget` (map value) and `Server`
        // (pointer return) kept.
        let code = r#"
package main
func F(name string, m map[string]Widget) (*Server, error) {
    return nil, nil
}
"#;
        let targets = uses_type_targets(code, Language::Go, "m.go", "F");
        assert!(targets.contains(&"Widget".to_string()), "got {targets:?}");
        assert!(targets.contains(&"Server".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"string".to_string()), "got {targets:?}");
        assert!(!targets.contains(&"error".to_string()), "got {targets:?}");
    }

    #[test]
    fn test_extract_go_relationships() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
package main

import "fmt"

func Run() {
    fmt.Println("ok")
    helper()
}

func helper() {}
"#;
        let tree = parser.parse(code, Language::Go).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Go, "main.go");
        let relationships =
            extract_symbol_relationships(&tree, code, Language::Go, "main.go", &symbols);

        assert!(relationships.iter().any(|relationship| {
            relationship.target_name == "fmt"
                && relationship.relationship_type == SymbolRelationshipType::Import
        }));
        assert!(relationships.iter().any(|relationship| {
            relationship.target_name == "Println"
                && relationship.relationship_type == SymbolRelationshipType::Call
        }));
        assert!(relationships.iter().any(|relationship| {
            relationship.target_name == "helper"
                && relationship.relationship_type == SymbolRelationshipType::Call
        }));
    }
}
