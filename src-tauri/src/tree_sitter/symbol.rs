//! Symbol extraction from parsed AST trees
//!
//! Extracts semantic symbols (functions, classes, methods, etc.) from
//! tree-sitter AST trees for indexing and context assembly.

use std::collections::HashSet;
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
        // Structural concern: extends / implements / contains. Runs AFTER the
        // scope push so a class node resolves to its own symbol (reproducing the
        // old `find_enclosing_symbol(class_range)` self-match).
        self.process_structural_relationship(node, source, language, state, rel);

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

    fn node_to_symbol(&self, node: &Node, source: &str, language: Language) -> Option<Symbol> {
        match language {
            Language::TypeScript | Language::Tsx | Language::Astro => {
                self.typescript_node_to_symbol(node, source)
            }
            Language::JavaScript | Language::Jsx => self.javascript_node_to_symbol(node, source),
            Language::Python => self.python_node_to_symbol(node, source),
            Language::Rust => self.rust_node_to_symbol(node, source),
            Language::Go => self.go_node_to_symbol(node, source),
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

    fn typescript_node_to_symbol(&self, node: &Node, source: &str) -> Option<Symbol> {
        let kind = node.kind();
        let range = Range::from_node(node);

        match kind {
            "import_statement" => {
                let text = node.utf8_text(source.as_bytes()).ok()?;
                let name = self.extract_quoted_text(text)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Import,
                    self.file_path.clone(),
                    range,
                ))
            }
            "function_declaration" | "generator_function_declaration" | "function_signature" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Function,
                    self.file_path.clone(),
                    range,
                ))
            }
            "function" | "generator_function" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Function,
                    self.file_path.clone(),
                    range,
                ))
            }
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Method,
                    self.file_path.clone(),
                    range,
                ))
            }
            "class_declaration" | "class" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Class,
                    self.file_path.clone(),
                    range,
                ))
            }
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
            "enum_assignment" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::EnumMember,
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
            "public_field_definition" | "field_definition" | "property_signature" => {
                let name_node = node.child_by_field_name("name")?;
                let name = self.extract_js_ts_property_name(&name_node, source)?;
                let symbol_type = node
                    .child_by_field_name("value")
                    .as_ref()
                    .filter(|value| self.is_js_ts_function_value(value, source))
                    .map(|_| SymbolType::Method)
                    .unwrap_or(SymbolType::Property);
                Some(Symbol::new(
                    name,
                    symbol_type,
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

    fn javascript_node_to_symbol(&self, node: &Node, source: &str) -> Option<Symbol> {
        // JavaScript uses similar structure to TypeScript
        self.typescript_node_to_symbol(node, source)
    }

    fn python_node_to_symbol(&self, node: &Node, source: &str) -> Option<Symbol> {
        let kind = node.kind();
        let range = Range::from_node(node);

        match kind {
            "import_statement" => {
                let text = node.utf8_text(source.as_bytes()).ok()?;
                let name = self.extract_python_import_target(text)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Import,
                    self.file_path.clone(),
                    range,
                ))
            }
            "import_from_statement" => {
                let text = node.utf8_text(source.as_bytes()).ok()?;
                let name = self.extract_python_from_import_target(text)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Import,
                    self.file_path.clone(),
                    range,
                ))
            }
            "function_definition" => {
                let name = self.get_child_text(node, "name", source)?;
                // Check if it's a method (inside a class)
                let is_method = node
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|gp| gp.kind() == "class_definition")
                    .unwrap_or(false);
                Some(Symbol::new(
                    name,
                    if is_method {
                        SymbolType::Method
                    } else {
                        SymbolType::Function
                    },
                    self.file_path.clone(),
                    range,
                ))
            }
            "class_definition" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Class,
                    self.file_path.clone(),
                    range,
                ))
            }
            _ => None,
        }
    }

    fn rust_node_to_symbol(&self, node: &Node, source: &str) -> Option<Symbol> {
        let kind = node.kind();
        let range = Range::from_node(node);

        match kind {
            "use_declaration" => {
                let text = node.utf8_text(source.as_bytes()).ok()?;
                let name = self.extract_rust_use_target(text)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Import,
                    self.file_path.clone(),
                    range,
                ))
            }
            "function_item" => {
                let name = self.get_child_text(node, "name", source)?;
                // A `function_item` directly inside an `impl` block is a method.
                let symbol_type = if is_rust_impl_method(node) {
                    SymbolType::Method
                } else {
                    SymbolType::Function
                };
                Some(Symbol::new(name, symbol_type, self.file_path.clone(), range))
            }
            "field_declaration" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Property,
                    self.file_path.clone(),
                    range,
                ))
            }
            "enum_variant" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::EnumMember,
                    self.file_path.clone(),
                    range,
                ))
            }
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

    fn go_node_to_symbol(&self, node: &Node, source: &str) -> Option<Symbol> {
        let kind = node.kind();
        let range = Range::from_node(node);

        match kind {
            "import_spec" => {
                let text = node.utf8_text(source.as_bytes()).ok()?;
                let name = self.extract_quoted_text(text)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Import,
                    self.file_path.clone(),
                    range,
                ))
            }
            "function_declaration" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Function,
                    self.file_path.clone(),
                    range,
                ))
            }
            "method_declaration" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Method,
                    self.file_path.clone(),
                    range,
                ))
            }
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
    let mut rel = RelState {
        relationships: Vec::new(),
        seen: HashSet::new(),
        all_symbols: symbols,
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
        | Language::Jsx => {
            if node.kind() != "call_expression" {
                return None;
            }
            let callee = node.child_by_field_name("function")?;
            extract_callable_name(&callee, source)
        }
        Language::Python => {
            if node.kind() != "call" {
                return None;
            }
            let callee = node.child_by_field_name("function")?;
            extract_callable_name(&callee, source)
        }
        Language::Rust => match node.kind() {
            "call_expression" => {
                let callee = node.child_by_field_name("function")?;
                extract_callable_name(&callee, source)
            }
            // `println!(...)`, `anyhow!(...)`, `tracing::info!(...)` etc. emit a
            // Call edge from the macro path child.
            "macro_invocation" => {
                let macro_node = node.child_by_field_name("macro")?;
                extract_callable_name(&macro_node, source)
            }
            _ => None,
        },
        Language::Go => {
            if node.kind() != "call_expression" {
                return None;
            }
            let callee = node.child_by_field_name("function")?;
            extract_callable_name(&callee, source)
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
