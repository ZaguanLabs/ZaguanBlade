//! Symbol extraction from parsed AST trees
//!
//! Extracts semantic symbols (functions, classes, methods, etc.) from
//! tree-sitter AST trees for indexing and context assembly.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Tree};

use super::parser::Language;

/// Bounds-checked node text extraction.
///
/// tree-sitter can — during parse-error recovery or certain grammar states —
/// return a node whose byte range lies OUTSIDE the source buffer. `Node::utf8_text`
/// then panics on the out-of-range slice (`&source[start..end]`), BEFORE the
/// ubiquitous `.ok()` can see it. With `panic = "abort"` in the release profile a
/// worker panic aborts the whole process, so one pathological file crashed the
/// entire Firefox index (`range start index 153 out of range for slice of length
/// 30`, on web-platform-tests). Validate the range first and return `Err(())` —
/// which the existing `.ok()` turns into `None` — instead of panicking. Same
/// happy-path behaviour, no crash.
trait SafeNodeText {
    fn safe_text<'a>(&self, source: &'a str) -> Result<&'a str, ()>;
}

impl SafeNodeText for Node<'_> {
    fn safe_text<'a>(&self, source: &'a str) -> Result<&'a str, ()> {
        let bytes = source.as_bytes();
        if self.start_byte() <= self.end_byte() && self.end_byte() <= bytes.len() {
            self.utf8_text(bytes).map_err(|_| ())
        } else {
            Err(())
        }
    }
}

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
    /// A declarative infrastructure resource (e.g. a Kubernetes manifest doc
    /// with both `apiVersion` and `kind`), named `"<kind>/<metadata.name>"`.
    Resource,
    /// An HTTP route registration (M4.4), named with the canonical
    /// `"<METHOD> <canonical-path>"` (e.g. `"POST /api/orders/{}"`). The path is
    /// canonicalized so the same route unifies across frameworks (every param
    /// syntax collapses to `{}`). A `Handles` edge links the Route to its handler
    /// function/method symbol.
    Route,
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
            SymbolType::Resource => "resource",
            SymbolType::Route => "route",
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
                ..Default::default()
            });
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImportBinding {
    imported_name: String,
    module_path: String,
}

/// Collect unambiguous named ES-module imports keyed by the local identifier
/// used at call sites. Ambiguous duplicate local bindings are dropped instead
/// of guessing which module introduced the name.
fn js_ts_import_bindings(
    root: &Node<'_>,
    source: &str,
    language: Language,
) -> HashMap<String, ImportBinding> {
    if !matches!(
        language,
        Language::TypeScript
            | Language::Tsx
            | Language::Astro
            | Language::JavaScript
            | Language::Jsx
    ) {
        return HashMap::new();
    }

    fn visit(
        node: Node<'_>,
        source: &str,
        bindings: &mut HashMap<String, ImportBinding>,
        ambiguous: &mut HashSet<String>,
    ) {
        if node.kind() == "import_statement" {
            if let Ok(statement) = node.safe_text(source) {
                for (local_name, binding) in parse_js_ts_named_imports(statement) {
                    if ambiguous.contains(&local_name) {
                        continue;
                    }
                    if bindings
                        .get(&local_name)
                        .is_some_and(|existing| existing != &binding)
                    {
                        bindings.remove(&local_name);
                        ambiguous.insert(local_name);
                    } else {
                        bindings.insert(local_name, binding);
                    }
                }
            }
            return;
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            visit(child, source, bindings, ambiguous);
        }
    }

    let mut bindings = HashMap::new();
    let mut ambiguous = HashSet::new();
    visit(*root, source, &mut bindings, &mut ambiguous);
    bindings
}

fn parse_js_ts_named_imports(statement: &str) -> Vec<(String, ImportBinding)> {
    let Some(open_quote) = statement.rfind(['\'', '"']) else {
        return Vec::new();
    };
    let quote = statement.as_bytes()[open_quote] as char;
    let Some(relative_close) = statement[..open_quote].rfind(quote) else {
        return Vec::new();
    };
    let module_path = statement[relative_close + 1..open_quote].trim();
    if module_path.is_empty() {
        return Vec::new();
    }

    let clause = statement[..relative_close]
        .trim_end()
        .strip_suffix("from")
        .map(str::trim_end)
        .unwrap_or_default();
    let Some(open_brace) = clause.find('{') else {
        return Vec::new();
    };
    let Some(close_brace) = clause[open_brace + 1..].find('}') else {
        return Vec::new();
    };
    let named = &clause[open_brace + 1..open_brace + 1 + close_brace];

    named
        .split(',')
        .filter_map(|entry| {
            let entry = entry.trim().strip_prefix("type ").unwrap_or(entry.trim());
            if entry.is_empty() {
                return None;
            }
            let mut words = entry.split_whitespace();
            let imported_name = words.next()?.trim();
            let next = words.next();
            let local_name = match next {
                None => imported_name,
                Some("as") => words.next()?.trim(),
                Some(_) => return None,
            };
            if words.next().is_some() || imported_name.is_empty() || local_name.is_empty() {
                return None;
            }
            Some((
                local_name.to_owned(),
                ImportBinding {
                    imported_name: imported_name.to_owned(),
                    module_path: module_path.to_owned(),
                },
            ))
        })
        .collect()
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
            "resource" => Ok(SymbolType::Resource),
            "route" => Ok(SymbolType::Route),
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// An HTTP Route node HANDLES a handler function/method (M4.4). The SOURCE is
    /// the synthetic `Route` symbol (`"<METHOD> <canonical-path>"`); the TARGET is
    /// the handler function/method symbol that the route maps to (`target_name` is
    /// the handler's name and `target_symbol_id` carries its resolved id). Stored
    /// as the TEXT value `"handles"`. Routes with no identifiable handler (e.g. an
    /// inline anonymous Express callback) emit the Route node WITHOUT this edge
    /// (the low-confidence anchor fallback).
    Handles,
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
            SymbolRelationshipType::Handles => "handles",
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
            "handles" => Ok(SymbolRelationshipType::Handles),
            _ => Err(format!("Unknown relationship type: {}", s)),
        }
    }
}

/// `skip_serializing_if` predicate for a `bool` field that is omitted when false
/// (serde needs a `fn(&bool) -> bool`; `std::ops::Not::not` takes `bool` by value).
fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolRelationship {
    pub source_symbol_id: String,
    pub source_file_path: String,
    pub target_name: String,
    pub target_symbol_id: Option<String>,
    pub relationship_type: SymbolRelationshipType,
    pub line: u32,
    /// M5.1 receiver-type dispatch. For a method/attribute Call edge whose
    /// receiver evaluated to a known `TypeRep::Named`, this carries that base
    /// type NAME (e.g. `self.run()` inside class `A` → `Some("A")`; `x.run()`
    /// where `x = B()` → `Some("B")`). Bare calls and `TypeRep::Unknown`
    /// receivers leave it `None` so downstream resolution is unchanged. Persisted
    /// to the `metadata_json` column as `{"recv_type":"<name>"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recv_type: Option<String>,
    /// M5.1b provenance gate. `true` ONLY when `recv_type` was derived from a
    /// `self`/`this` receiver — i.e. it is the EXACT enclosing-class qualified
    /// name, guaranteed to be a real project class defined in THIS file, never a
    /// simple name inferred from a typed param / constructor / annotation (which
    /// may shadow an imported library type of the same simple name). Only
    /// self-typed Call edges are eligible for the GLOBAL receiver-type mining
    /// (`SymbolStore::mine_receiver_type_relationship_targets`); param/constructor
    /// recv_types stay usable for M5.1's in-candidate-set disambiguation but are
    /// NOT globally mined. Persisted to `metadata_json` as `"recv_self":true`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub recv_self: bool,
    /// Module specifier that introduced the local call target name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_path: Option<String>,
    /// Exported name requested from `import_path` before any local alias.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_name: Option<String>,
    /// M5.1 audit tag. Set to `Some("receiver_type")` ONLY when the per-file
    /// resolver disambiguated an otherwise-ambiguous candidate set by `recv_type`.
    /// `None` everywhere else (matching today's per-file resolutions, which carry
    /// no strategy until the global-unique back-fill tags them in the store).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_strategy: Option<String>,
    /// M5.1 confidence for a `receiver_type` resolution (above `global_unique`'s
    /// `0.5`). `None` unless `resolution_strategy` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Track C Go receiver kind, set ONLY on the receiver-type→method
    /// `Contains` edge: `Some("pointer")` for `func (m *Memory) …`,
    /// `Some("value")` for `func (m Memory) …`, `None` on every other edge.
    /// Persisted to the `metadata_json` column as `{"receiver":"<kind>"}`
    /// (store-side serialization, following the M5.1 `recv_type` idiom) so the
    /// store's Go implicit-interface miner can apply pointer/value method-set
    /// semantics instead of assuming them away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_kind: Option<String>,
    /// Qualified Rust call observation: absolute byte offset of the call
    /// expression in the source file. Essential for exact call-site identity —
    /// two calls on the same line (`A::new(); B::new();`) share source ID,
    /// target name, relationship kind, and line, so byte offset is the only
    /// discriminator. Persisted to `metadata_json` as `{"byte_offset":<u32>}`.
    /// `None` for legacy relationships that predate qualified-call extraction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_offset: Option<u32>,
    /// Qualified Rust call observation: the normalized qualifier segments
    /// before the terminal callable name, in source order. For
    /// `crate::store::SymbolStore::new()` this is `["crate", "store",
    /// "SymbolStore"]`. For `Self::open()` this is `["Self"]`. For a bare
    /// `new()` call this is `None`. Retains `crate`, `self`, `super`, and
    /// `Self` keywords. Generic arguments and turbofish are stripped from
    /// lookup identity. Persisted to `metadata_json` as
    /// `{"qualifier":["seg1","seg2",...]}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualifier_segments: Option<Vec<String>>,
    /// Qualified Rust call observation: the syntactic form of the call.
    /// `"bare"` for `new()`, `"associated"` for `Type::method()`,
    /// `"self_path"` for `Self::method()`, `"crate_path"` for
    /// `crate::…::method()`, `"module_path"` for `self::…` or `super::…`,
    /// `"ufcs"` for `<Type as Trait>::method()`, `"receiver"` for
    /// `value.method()`. `None` for non-call relationships or legacy edges.
    /// Persisted to `metadata_json` as `{"call_form":"<form>"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_form: Option<String>,
    /// Qualified Rust call observation: a stable category explaining why a
    /// qualified call could not be resolved. One of: `"missing_project_context"`,
    /// `"unresolved_owner"`, `"unresolved_method"`, `"ambiguous"`,
    /// `"unsupported"`, `"ambiguous_import"`, `"glob_only_visibility"`,
    /// `"self_without_owner"`, `"external_crate_not_indexed"`.
    /// `None` for resolved or legacy relationships. Persisted to
    /// `metadata_json` as `{"unresolved_reason":"<reason>"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved_reason: Option<String>,
}

/// Call form identifiers for qualified Rust call observations.
pub mod call_form {
    pub const BARE: &str = "bare";
    pub const ASSOCIATED: &str = "associated";
    pub const SELF_PATH: &str = "self_path";
    pub const CRATE_PATH: &str = "crate_path";
    pub const MODULE_PATH: &str = "module_path";
    pub const UFCS: &str = "ufcs";
    pub const RECEIVER: &str = "receiver";
}

/// Unresolved reason categories for qualified Rust call observations.
pub mod unresolved_reason {
    pub const MISSING_PROJECT_CONTEXT: &str = "missing_project_context";
    pub const UNRESOLVED_OWNER: &str = "unresolved_owner";
    pub const UNRESOLVED_METHOD: &str = "unresolved_method";
    pub const AMBIGUOUS: &str = "ambiguous";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const AMBIGUOUS_IMPORT: &str = "ambiguous_import";
    pub const GLOB_ONLY_VISIBILITY: &str = "glob_only_visibility";
    pub const SELF_WITHOUT_OWNER: &str = "self_without_owner";
    pub const EXTERNAL_CRATE_NOT_INDEXED: &str = "external_crate_not_indexed";
}

impl Default for SymbolRelationship {
    fn default() -> Self {
        SymbolRelationship {
            source_symbol_id: String::new(),
            source_file_path: String::new(),
            target_name: String::new(),
            target_symbol_id: None,
            relationship_type: SymbolRelationshipType::Call,
            line: 0,
            recv_type: None,
            recv_self: false,
            import_path: None,
            imported_name: None,
            resolution_strategy: None,
            confidence: None,
            receiver_kind: None,
            byte_offset: None,
            qualifier_segments: None,
            call_form: None,
            unresolved_reason: None,
        }
    }
}

/// M5.1 minimal type representation for receiver-type dispatch. Deliberately
/// tiny: the dispatch path only ever reads a base `Named` qualified-name, and an
/// `Unknown` receiver falls straight through to today's resolver (strict
/// superset). `Ref`/`Generic`/trait-solving are deferred (§13 parking lot).
#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeRep {
    /// A concrete named type (class/struct/interface/…) — its base NAME as
    /// written (`Foo`), used to match a method candidate's parent-class name.
    Named(String),
    /// The receiver's type could not be evaluated cheaply → no narrowing.
    Unknown,
}

/// M5.1 one scope's local-variable typing, with the conservatism the adversarial
/// review demands. A name maps to a `TypeRep`; `poisoned` names are stuck on
/// `Unknown` for the rest of the scope (a conflicting re-assignment makes the type
/// ambiguous and no later write may revive a concrete type).
///
/// Guiding principle: when a variable's type is not CONFIDENTLY known it MUST be
/// `Unknown` (which falls through to today's safe resolver) — never a guess. Every
/// mutator below errs toward `Unknown`.
#[derive(Default)]
struct VarFrame {
    types: HashMap<String, TypeRep>,
    /// Names made permanently `Unknown` by a conflicting re-assignment (FIX 2).
    poisoned: HashSet<String>,
}

impl VarFrame {
    /// Record a binding of `name` to `rep`, overwriting any prior type (FIX 1: a
    /// rebind must never leave the stale type). Two guards keep it conservative:
    /// - a poisoned name is immutable (`Unknown` wins forever);
    /// - re-binding an existing CONCRETE type to a DIFFERENT concrete type is a
    ///   flow-insensitive conflict (we have no CFG) → poison to `Unknown` (FIX 2).
    fn bind(&mut self, name: &str, rep: TypeRep) {
        if self.poisoned.contains(name) {
            return;
        }
        if let (Some(TypeRep::Named(old)), TypeRep::Named(new)) = (self.types.get(name), &rep) {
            if old != new {
                self.poison(name);
                return;
            }
        }
        self.types.insert(name.to_string(), rep);
    }

    /// Force `name` to a sticky `Unknown` for the rest of the scope (FIX 2).
    fn poison(&mut self, name: &str) {
        self.types.insert(name.to_string(), TypeRep::Unknown);
        self.poisoned.insert(name.to_string());
    }

    /// Insert an `Unknown` shadow marker so `lookup_var_type` STOPS here instead of
    /// leaking an outer same-named type (FIX 1/FIX 3: untyped params, loop/with/
    /// except/comprehension/lambda targets). Goes through `bind`, so it also clears
    /// a stale concrete type rather than leaving it.
    fn shadow(&mut self, name: &str) {
        self.bind(name, TypeRep::Unknown);
    }
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
    /// M5.1: whether this scope is a class-like TYPE (class/struct/interface/
    /// trait/enum/impl). Used to resolve `self`/`this` to the nearest enclosing
    /// type's `child_qn` (the qualified name handed to its members).
    is_type: bool,
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
    /// M5.1 per-scope local-variable type frames, pushed/popped in lockstep with
    /// `scope` during the RELATIONSHIP walk only (the symbol walk never touches
    /// it). Typed params and constructor-bound locals (`x = Foo()` / `new Foo()` /
    /// `Foo::new()` / `Foo{}`) are inserted into the top frame; a receiver
    /// identifier is resolved by scanning top→bottom. See `VarFrame` for the
    /// conservative (Unknown-on-doubt) write semantics.
    var_types: Vec<VarFrame>,
    /// M5.1 class-gating set (FIX 4): the simple names of every class-like symbol
    /// in this file. A Python `x = Foo()` is constructor-typed as `Foo` ONLY when
    /// `Foo` is in this set (a real class, not a factory function); Rust
    /// `let x = Foo::new()` likewise only when `Foo` is a known struct/enum. Built
    /// once up-front from the full symbol list; empty during the symbol walk.
    class_names: HashSet<String>,
    /// Track B per-scope constant-shadow frames, pushed/popped in lockstep with
    /// `scope` during the RELATIONSHIP walk only (like `var_types`). Each entry
    /// is a `(name, activation_byte)` pair recorded when a lexical binding
    /// (let / match arm / closure / loop / parameter) re-binds the NAME of a
    /// file-local Rust constant. A constant reference is suppressed only when it
    /// STARTS at/after the activation byte, so a `let X = X.len();` initializer
    /// — evaluated with the OUTER binding — still resolves to the constant while
    /// later uses in the shadowed scope emit nothing. Frames stay empty unless a
    /// binding actually collides with a constant name.
    const_shadows: Vec<Vec<(String, usize)>>,
}

/// Relationship-walk sink, threaded alongside `WalkState` when the unified DFS
/// runs in relationship mode.
struct RelState<'a> {
    relationships: Vec<SymbolRelationship>,
    /// De-dup key shared across the call, import, and structural concerns
    /// (matching the original single shared `seen` set).
    seen: HashSet<(String, String, SymbolRelationshipType, u32)>,
    /// Qualified Rust call dedup: includes byte_offset to keep same-line
    /// calls distinct (`A::new(); B::new();` share source, target name,
    /// relationship kind, and line — only byte offset differs).
    call_seen: HashSet<(String, String, u32, u32)>,
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
    /// Single-file / scope-agnostic heuristic — no dataflow. Borrowed from the
    /// shared per-file `ExtractionFacts` (route detection consumes the same map).
    const_map: &'a HashMap<String, String>,
    /// Track B: this file's ROOT-LEVEL Rust constants (`const_item` →
    /// `SymbolType::Constant` with `parent_id: None`), name → symbol id.
    /// Mod-nested / fn-local / impl-associated consts are excluded — they are
    /// not file-wide bare names. A name bound by MORE THAN ONE eligible
    /// constant symbol is ambiguous and dropped up-front (false negative over a
    /// wrong edge). Built only for `Language::Rust`; empty for every other
    /// language, so the per-identifier usage concern rejects in O(1).
    const_targets: HashMap<&'a str, &'a str>,
    /// Named ES-module bindings keyed by the identifier used in this file.
    import_bindings: HashMap<String, ImportBinding>,
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
            var_types: Vec::new(),
            class_names: HashSet::new(),
            const_shadows: Vec::new(),
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
            .safe_text(source)
            .ok()
            .map(compute_content_hash)
            .unwrap_or_default();
        symbol.qualified_name = qualified_name.clone();
        symbol.id = stable_symbol_id(&self.file_path, &qualified_name, symbol.symbol_type);

        let id: Arc<str> = Arc::from(symbol.id.as_str());
        let range = symbol.range;
        let is_type = is_class_like_symbol(symbol.symbol_type);
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
            is_type,
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
                .and_then(|type_node| type_node.safe_text(source).ok())
                .and_then(normalize_reference_name)
            {
                let id: Arc<str> = symbols
                    .iter()
                    .find(|symbol| {
                        symbol.name == type_name && is_rust_type_symbol(symbol.symbol_type)
                    })
                    .map(|symbol| Arc::from(symbol.id.as_str()))
                    .unwrap_or_else(|| self_id.clone());
                let owner_qualified_name = node
                    .child_by_field_name("trait")
                    .and_then(|trait_node| trait_node.safe_text(source).ok())
                    .and_then(normalize_reference_name)
                    .map(|trait_name| {
                        format!(
                            "{} as {}",
                            type_name.strip_prefix("r#").unwrap_or(&type_name),
                            trait_name.strip_prefix("r#").unwrap_or(&trait_name)
                        )
                    })
                    .unwrap_or_else(|| {
                        type_name
                            .strip_prefix("r#")
                            .unwrap_or(&type_name)
                            .to_string()
                    });
                return ChildCtx {
                    id,
                    qn: Arc::from(owner_qualified_name),
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
        if pushed {
            // M5.1: keep a per-scope local-variable type frame in lockstep with
            // the scope stack (relationship walk only). Typed params for THIS
            // node (if it is a function) land in the frame just pushed.
            state.var_types.push(VarFrame::default());
            // Track B: constant-shadow frame, same lifecycle.
            state.const_shadows.push(Vec::new());
        }
        // M5.1: record typed params / constructor-bound locals into the current
        // (innermost) frame BEFORE any receiver in this subtree is evaluated.
        // Pre-order + source order means `x = B()` is recorded before a later
        // `x.run()` reads it.
        self.process_var_typing(&node, source, language, state);
        // Track B: record lexical shadows of file-local Rust constant names for
        // THIS binding node BEFORE its subtree's identifiers are visited (the
        // activation byte, not walk order, decides which of them are affected).
        self.process_const_shadow(&node, source, language, state, rel);

        // Call / macro-call concern: attribute to the innermost enclosing symbol.
        self.process_call_relationship(&node, source, language, state, rel);
        // Track B: ordinary value references to file-local Rust constants emit
        // resolved `usage` edges. Like calls, these live anywhere in a body, so
        // this fires on EVERY node and attributes to the innermost enclosing
        // symbol.
        self.process_const_usage_relationship(&node, source, language, state, rel);
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
            state.var_types.pop();
            state.const_shadows.pop();
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
        let Some(source_id) =
            resolve_enclosing_scope(&state.scope, &range).map(|s| s.id.to_string())
        else {
            return;
        };
        let line = node.start_position().row as u32;

        // Qualified Rust call observation: extract qualifier segments, call
        // form, and byte offset for precise same-line dedup. Non-Rust paths
        // and Rust macro invocations fall through to the original shared
        // `seen` set.
        let qualified = if language == Language::Rust {
            extract_rust_qualified_call_from_node(node, source)
        } else {
            None
        };
        let target_name = qualified
            .as_ref()
            .map(|call| call.terminal.clone())
            .unwrap_or(target_name);

        let (byte_offset, qualifier_segments, call_form) = match &qualified {
            Some(q) => {
                let offset = q.byte_offset;
                let dedup_key = (
                    source_id.clone(),
                    q.terminal.clone(),
                    line,
                    offset,
                );
                if !rel.call_seen.insert(dedup_key) {
                    return;
                }
                (
                    Some(offset),
                    if q.qualifier.is_empty() {
                        None
                    } else {
                        Some(q.qualifier.clone())
                    },
                    Some(q.call_form.clone()),
                )
            }
            None => (None, None, None),
        };

        let key = (
            source_id.clone(),
            target_name.clone(),
            SymbolRelationshipType::Call,
            line,
        );
        // For qualified Rust calls, the `call_seen` set already handles
        // dedup. For everything else, use the shared `seen` set.
        let is_new = if qualified.is_some() {
            true
        } else {
            rel.seen.insert(key)
        };
        if is_new {
            // M5.1: evaluate the receiver's type (self/this → enclosing class;
            // constructor-bound / typed local → its type). `None` for bare calls
            // and unknown receivers → downstream resolution unchanged. M5.1b: the
            // bool records `self`/`this` provenance (the type IS the exact
            // enclosing-class qn) so the GLOBAL miner can restrict to those.
            let (recv_type, recv_self) =
                match self.eval_call_receiver_type(node, source, language, state) {
                    Some((name, is_self)) => (Some(name), is_self),
                    None => (None, false),
                };
            let import_binding = rel.import_bindings.get(&target_name);
            rel.relationships.push(SymbolRelationship {
                source_symbol_id: source_id,
                source_file_path: self.file_path.clone(),
                target_name,
                target_symbol_id: None,
                relationship_type: SymbolRelationshipType::Call,
                line,
                recv_type,
                recv_self,
                import_path: import_binding.map(|binding| binding.module_path.clone()),
                imported_name: import_binding.map(|binding| binding.imported_name.clone()),
                byte_offset,
                qualifier_segments,
                call_form,
                unresolved_reason: None,
                ..Default::default()
            });
        }
    }

    /// Track B: record lexical shadows of FILE-LOCAL Rust constant names so
    /// `process_const_usage_relationship` can suppress references a `let` /
    /// `match` / closure / loop / parameter binding re-bound. Only names that
    /// actually collide with a known constant are recorded (frames stay empty in
    /// the common case). Each shadow carries an ACTIVATION byte:
    /// - `let` / `if let` / `while let` activate at the END of the whole
    ///   declaration/condition node, so the initializer — evaluated with the
    ///   OUTER binding — still resolves to the constant (the
    ///   `let WORKFLOW_TEXT = WORKFLOW_TEXT.len();` case);
    /// - patterns (fn params, closure params, `for` bindings) activate at the
    ///   END of the pattern node, which conservatively also suppresses a use
    ///   inside the `for`-iterator expression (false negative by design);
    /// - `match` arms activate at the START of the pattern node, because the
    ///   grammar's `match_arm` pattern field SPANS the `if` guard — start-byte
    ///   activation suppresses guard and body references of an arm-bound
    ///   colliding name alike (false negative by design).
    /// Shadows land in the innermost frame and persist to the end of the
    /// enclosing SYMBOL scope (blocks/arms push no frame) — again a deliberate
    /// false-negative bias: a shadow can only suppress edges, never invent one.
    fn process_const_shadow(
        &self,
        node: &Node,
        source: &str,
        language: Language,
        state: &mut WalkState,
        rel: &RelState,
    ) {
        if language != Language::Rust || rel.const_targets.is_empty() {
            return;
        }
        match node.kind() {
            // Whole-statement activation: the value subtree still sees the
            // outer constant.
            "let_declaration" | "let_condition" => {
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    record_const_shadows(&pattern, node.end_byte(), source, state, rel);
                }
            }
            // Pattern-START activation: tree-sitter-rust's `match_arm` pattern
            // node SPANS the `if` guard, so end-byte activation would let guard
            // identifiers escape suppression and resolve to the shadowed
            // constant (a false positive). Activating at the pattern's start
            // byte suppresses guard AND body references of an arm-bound
            // colliding name — a deliberate false negative, the handoff bias.
            "match_arm" => {
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    record_const_shadows(&pattern, pattern.start_byte(), source, state, rel);
                }
            }
            // Pattern-end activation: the loop body comes after (this also
            // conservatively suppresses a colliding use inside the iterator
            // expression — false negative by design, see the doc above).
            "for_expression" => {
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    record_const_shadows(&pattern, pattern.end_byte(), source, state, rel);
                }
            }
            // Parameter patterns only — walking each `parameter`'s `pattern`
            // field (not the whole list) keeps type-position identifiers (e.g.
            // `[u8; LEN]` array lengths) out of the shadow set. Bare closure
            // params (`|x|`) are their own pattern node.
            "function_item" | "closure_expression" => {
                let Some(params) = node.child_by_field_name("parameters") else {
                    return;
                };
                let mut cursor = params.walk();
                for param in params.named_children(&mut cursor) {
                    let pattern = if param.kind() == "parameter" {
                        param.child_by_field_name("pattern")
                    } else {
                        Some(param)
                    };
                    if let Some(pattern) = pattern {
                        record_const_shadows(&pattern, pattern.end_byte(), source, state, rel);
                    }
                }
            }
            _ => {}
        }
    }

    /// Track B: emit a resolved `Usage` edge for an ORDINARY value reference to
    /// a FILE-LOCAL Rust constant. Precision-first: only a plain `identifier`
    /// whose PARENT position guarantees a value expression counts
    /// (`rust_const_expression_position`) — patterns, declarations, field
    /// names, non-terminal `scoped_identifier` path segments, macro
    /// `token_tree`s, and attribute contents all fall outside the whitelist and
    /// emit nothing (false negative over false positive). Lexical shadows
    /// registered by `process_const_shadow` suppress references at/after their
    /// activation byte. The target is THIS file's constant symbol, known
    /// exactly, so the edge resolves immediately with strategy
    /// `file_local_const` and confidence 0.9 (below `receiver_type`'s certainty
    /// of a typed dispatch, above `global_unique`'s 0.5).
    fn process_const_usage_relationship(
        &self,
        node: &Node,
        source: &str,
        language: Language,
        state: &WalkState,
        rel: &mut RelState,
    ) {
        // Cheap early rejects, hot-path order: language gate, empty target set,
        // node kind, uppercase first char (Rust consts are SCREAMING_SNAKE_CASE),
        // then the map lookup.
        if language != Language::Rust || rel.const_targets.is_empty() {
            return;
        }
        if node.kind() != "identifier" {
            return;
        }
        let Ok(name) = node.safe_text(source) else {
            return;
        };
        if !name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            return;
        }
        let Some(&target_id) = rel.const_targets.get(name) else {
            return;
        };
        if !rust_const_expression_position(node) {
            return;
        }
        // A lexical binding of the same name that activated BEFORE this use
        // wins: emit nothing (the shadowing rule).
        let use_start = node.start_byte();
        if state
            .const_shadows
            .iter()
            .any(|frame| frame.iter().any(|(n, act)| *act <= use_start && n == name))
        {
            return;
        }
        let range = Range::from_node(node);
        // Module-level uses outside any symbol are dropped (matching the
        // env-access concern).
        let Some(source_id) =
            resolve_enclosing_scope(&state.scope, &range).map(|s| s.id.to_string())
        else {
            return;
        };
        // Never a self-edge (a constant's own subtree naming itself).
        if source_id == target_id {
            return;
        }
        let line = node.start_position().row as u32;
        let key = (
            source_id.clone(),
            name.to_string(),
            SymbolRelationshipType::Usage,
            line,
        );
        if rel.seen.insert(key) {
            rel.relationships.push(SymbolRelationship {
                source_symbol_id: source_id,
                source_file_path: self.file_path.clone(),
                target_name: name.to_string(),
                target_symbol_id: Some(target_id.to_string()),
                relationship_type: SymbolRelationshipType::Usage,
                line,
                resolution_strategy: Some("file_local_const".to_string()),
                confidence: Some(0.9),
                ..Default::default()
            });
        }
    }

    /// M5.1: evaluate the TYPE of a call's receiver, if the call is a method/
    /// attribute access whose receiver is `self`/`this`, or a local variable whose
    /// type is known. Returns `(base type NAME, is_self)` where `is_self` is `true`
    /// ONLY for a `self`/`this` receiver (M5.1b provenance — then the NAME is the
    /// exact enclosing-class qualified name). `None` for `TypeRep::Unknown` / bare
    /// call → the resolver falls through unchanged.
    fn eval_call_receiver_type(
        &self,
        node: &Node,
        source: &str,
        language: Language,
        state: &WalkState,
    ) -> Option<(String, bool)> {
        let receiver = call_receiver_node(node, language)?;
        match eval_receiver_type_rep(&receiver, source, language, state) {
            // A `self`/`this` receiver yields the enclosing-class qn; record that
            // provenance. (A `this` rebound by a plain function already evaluated to
            // `Unknown` above and never reaches here, so it is never self-tagged.)
            TypeRep::Named(name) => Some((name, receiver_is_self(&receiver, source))),
            TypeRep::Unknown => None,
        }
    }

    /// M5.1: record typed parameters and constructor-bound locals for `node` into
    /// the current (innermost) variable-type frame. Dispatched per language; a
    /// no-op when there is no enclosing frame (module-level) or the node is not a
    /// binding/function. See the per-language `*_record_var_types` helpers.
    fn process_var_typing(
        &self,
        node: &Node,
        source: &str,
        language: Language,
        state: &mut WalkState,
    ) {
        // Disjoint field borrows: the frame is mutated, the class-gating set is
        // only read (FIX 4).
        let WalkState {
            var_types,
            class_names,
            ..
        } = state;
        let Some(frame) = var_types.last_mut() else {
            return;
        };
        match language {
            Language::Python => python_record_var_types(node, source, frame, class_names),
            Language::TypeScript
            | Language::Tsx
            | Language::Astro
            | Language::JavaScript
            | Language::Jsx => ts_record_var_types(node, language, source, frame, class_names),
            Language::Rust => rust_record_var_types(node, source, frame, class_names),
            _ => {}
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
            Some(sym) if matches!(sym.symbol_type, SymbolType::Function | SymbolType::Method) => {
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
            Language::Cpp => self.cpp_node_to_symbol(node, source, language),
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
            let text = node.safe_text(source).ok()?;
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
            let text = node.safe_text(source).ok()?;
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

    fn rust_node_to_symbol(&self, node: &Node, source: &str, language: Language) -> Option<Symbol> {
        let kind = node.kind();
        let kind_id = node.kind_id();
        let bits = lang_bitsets(language, &node.language());
        let range = Range::from_node(node);

        if bits.import.contains(kind_id) {
            let text = node.safe_text(source).ok()?;
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
            return Some(Symbol::new(
                name,
                symbol_type,
                self.file_path.clone(),
                range,
            ));
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
                    let implemented_type = type_node.safe_text(source).ok()?;
                    let name = if let Some(trait_node) = node.child_by_field_name("trait") {
                        let implemented_trait = trait_node.safe_text(source).ok()?;
                        format!("{} for {}", implemented_trait, implemented_type)
                    } else {
                        implemented_type.to_string()
                    };
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

    fn go_node_to_symbol(&self, node: &Node, source: &str, language: Language) -> Option<Symbol> {
        let kind = node.kind();
        let kind_id = node.kind_id();
        let bits = lang_bitsets(language, &node.language());
        let range = Range::from_node(node);

        if bits.import.contains(kind_id) {
            let text = node.safe_text(source).ok()?;
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
            // Track C: an interface METHOD SPEC (`Ping(context.Context) error`
            // inside an `interface_type`) is a Method symbol. The enclosing
            // `type_spec` already pushed the interface's scope, so
            // `parent_id`/qualified name nest under the interface symbol
            // automatically, and `extract_signature`'s Go arm picks up the
            // spec's `parameters`/`result` fields for the "(params) result"
            // signature text — the store-side method-set miner's contract.
            //
            // GATE: only a spec belonging to a NAMED interface declaration
            // (nearest enclosing `interface_type` whose parent is a `type_spec`)
            // mints a symbol. Anonymous interface literals — function params
            // (`func Wait(s interface{ Done() })`), type assertions, aliases —
            // would otherwise mint false Method symbols parented to whatever
            // scope encloses them; they produce NO symbols (false negative by
            // design).
            "method_elem" => {
                let mut ancestor = node.parent();
                let mut named_interface = false;
                while let Some(a) = ancestor {
                    if a.kind() == "interface_type" {
                        named_interface = a.parent().is_some_and(|p| p.kind() == "type_spec");
                        break;
                    }
                    ancestor = a.parent();
                }
                if !named_interface {
                    return None;
                }
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

    /// C/C++ (M5.3): definitions-only extraction from the real `tree-sitter-cpp`
    /// grammar (replaces the old regex line-scanner). Names are dug out of the
    /// nested `declarator` chains the C grammar uses. NO relationships are emitted
    /// (N8 — the whole relationship walk is gated off for C/C++ at the public
    /// entry). Aggregates (struct/union/class/enum) and namespaces only produce a
    /// symbol when they carry a body, so usages and forward declarations are
    /// skipped; bare declarations produce a symbol only when they are function
    /// prototypes (plain variables/globals are intentionally dropped as noise).
    fn cpp_node_to_symbol(&self, node: &Node, source: &str, _language: Language) -> Option<Symbol> {
        let range = Range::from_node(node);
        let mk = |name: String, ty: SymbolType| {
            Some(Symbol::new(name, ty, self.file_path.clone(), range))
        };
        match node.kind() {
            // `#define X` / `#define X(...)` → macro, modeled as a Constant.
            "preproc_def" | "preproc_function_def" => mk(
                self.get_child_text(node, "name", source)?,
                SymbolType::Constant,
            ),
            // `namespace ns { … }` → Namespace; members nest via the scope stack.
            "namespace_definition" => mk(
                self.get_child_text(node, "name", source)?,
                SymbolType::Namespace,
            ),
            // struct / union WITH a body → Struct (the body gate skips usages such
            // as `struct Foo x;` and forward declarations `struct Foo;`).
            "struct_specifier" | "union_specifier" => {
                node.child_by_field_name("body")?;
                mk(
                    self.get_child_text(node, "name", source)?,
                    SymbolType::Struct,
                )
            }
            // class WITH a body → Class.
            "class_specifier" => {
                node.child_by_field_name("body")?;
                mk(
                    self.get_child_text(node, "name", source)?,
                    SymbolType::Class,
                )
            }
            // enum WITH a body → Enum; its `enumerator`s are handled below and nest
            // under it via the scope stack.
            "enum_specifier" => {
                node.child_by_field_name("body")?;
                mk(self.get_child_text(node, "name", source)?, SymbolType::Enum)
            }
            "enumerator" => mk(
                self.get_child_text(node, "name", source)?,
                SymbolType::EnumMember,
            ),
            // typedef → Type, EXCEPT when it merely names an aggregate that already
            // produces its own symbol (named struct/union/enum/class WITH a body):
            // defer to that aggregate so the ubiquitous `typedef struct Foo {…} Foo;`
            // idiom yields ONE clean `Foo` (no duplicate Type, no double nesting).
            "type_definition" => {
                if let Some(t) = node.child_by_field_name("type") {
                    if matches!(
                        t.kind(),
                        "struct_specifier"
                            | "union_specifier"
                            | "enum_specifier"
                            | "class_specifier"
                    ) && t.child_by_field_name("body").is_some()
                        && t.child_by_field_name("name").is_some()
                    {
                        return None;
                    }
                }
                let decl = node.child_by_field_name("declarator")?;
                let name =
                    cpp_clean_declarator_name(&cpp_innermost_declarator_name(&decl)?, source)?;
                mk(name, SymbolType::Type)
            }
            // function definition (has a body) → Function (free) or Method (member).
            "function_definition" => {
                let decl = node.child_by_field_name("declarator")?;
                let name_node = cpp_innermost_declarator_name(&decl)?;
                let name = cpp_clean_declarator_name(&name_node, source)?;
                mk(name, cpp_callable_symbol_type(&name_node, node))
            }
            // A bare declaration is a symbol only when it is a function PROTOTYPE
            // (its declarator subtree reaches a `function_declarator`). Variable /
            // global / usage declarations are intentionally skipped.
            "declaration" => {
                let decl = node.child_by_field_name("declarator")?;
                if !cpp_declarator_is_function(&decl) {
                    return None;
                }
                let name_node = cpp_innermost_declarator_name(&decl)?;
                let name = cpp_clean_declarator_name(&name_node, source)?;
                mk(name, cpp_callable_symbol_type(&name_node, node))
            }
            // Inside a struct/class body: a `field_declaration` whose declarator is a
            // function is a Method (e.g. `void run();`), otherwise a Property (data
            // member). The name-node kind is `field_identifier` either way, so the
            // declarator shape — not the name kind — decides here.
            "field_declaration" => {
                let decl = node.child_by_field_name("declarator")?;
                let name =
                    cpp_clean_declarator_name(&cpp_innermost_declarator_name(&decl)?, source)?;
                let ty = if cpp_declarator_is_function(&decl) {
                    SymbolType::Method
                } else {
                    SymbolType::Property
                };
                mk(name, ty)
            }
            _ => None,
        }
    }

    fn get_child_text(&self, node: &Node, field_name: &str, source: &str) -> Option<String> {
        node.child_by_field_name(field_name)
            .and_then(|n| n.safe_text(source).ok())
            .map(|s| s.to_string())
    }

    fn extract_js_ts_binding_name(&self, node: &Node, source: &str) -> Option<String> {
        match node.kind() {
            "identifier" | "type_identifier" => {
                node.safe_text(source).ok().map(|value| value.to_string())
            }
            _ => None,
        }
    }

    fn extract_js_ts_property_name(&self, node: &Node, source: &str) -> Option<String> {
        let text = node.safe_text(source).ok()?.trim();
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
            .safe_text(source)
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
            // M5.3: C/C++ takes NO docstring. The generic "previous comment" grab
            // would attach license headers (every kernel file opens with a GPL
            // block), `} // namespace X` closing comments, and section banners as
            // docstrings — pure noise at the kernel's scale. Proper `/** … */` /
            // `///` doc-comment extraction is deferred (§13).
            Language::Cpp => None,
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
        prev.safe_text(source).ok().map(|s| {
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
            let Ok(text) = prev.safe_text(source) else {
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
        let text = string_node.safe_text(source).ok()?;
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
                        let params_text = params.safe_text(source).ok()?;
                        // Try to get return type
                        let return_type = sig_node
                            .child_by_field_name("return_type")
                            .and_then(|n| n.safe_text(source).ok())
                            .map(|s| format!(" {}", s))
                            .unwrap_or_default();
                        return Some(format!("{}{}", params_text, return_type));
                    }
                }
            }
            Language::Python => {
                if let Some(params) = node.child_by_field_name("parameters") {
                    let params_text = params.safe_text(source).ok()?;
                    let return_type = node
                        .child_by_field_name("return_type")
                        .and_then(|n| n.safe_text(source).ok())
                        .map(|s| format!(" -> {}", s))
                        .unwrap_or_default();
                    return Some(format!("{}{}", params_text, return_type));
                }
            }
            Language::Rust => {
                if let Some(params) = node.child_by_field_name("parameters") {
                    let params_text = params.safe_text(source).ok()?;
                    let return_type = node
                        .child_by_field_name("return_type")
                        .and_then(|n| n.safe_text(source).ok())
                        .map(|s| format!(" {}", s))
                        .unwrap_or_default();
                    return Some(format!("{}{}", params_text, return_type));
                }
            }
            Language::Go => {
                if let Some(params) = node.child_by_field_name("parameters") {
                    let params_text = params.safe_text(source).ok()?;
                    let return_type = node
                        .child_by_field_name("result")
                        .and_then(|n| n.safe_text(source).ok())
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

/// Per-file facts shared between the symbol and relationship passes so each is
/// computed at most once per file: detected routes (M4.4) and the module-level
/// constant map (M4.2), which BOTH route-path resolution and env-var KEY
/// resolution consume. Gating preserves each language's work profile exactly:
/// constants are collected only when routes or relationships can use them
/// (C/C++ skip both walks entirely; Rust/Go collect constants only for the
/// relationship pass, as before), and routes only for
/// `supports_route_detection` languages.
pub(crate) struct ExtractionFacts {
    routes: Vec<DetectedRoute>,
    constants: HashMap<String, String>,
}

pub(crate) fn collect_extraction_facts(
    tree: &Tree,
    source: &str,
    language: Language,
) -> ExtractionFacts {
    let needs_routes = supports_route_detection(language);
    let needs_constants = needs_routes || language.capability().extracts.relationships;
    let mut constants: HashMap<String, String> = HashMap::new();
    if needs_constants {
        collect_module_constants(&tree.root_node(), language, source, &mut constants);
    }
    let mut routes = Vec::new();
    if needs_routes {
        walk_routes(&tree.root_node(), source, language, &constants, &mut routes);
    }
    ExtractionFacts { routes, constants }
}

/// Convenience function to extract symbols from source code. Collects the
/// per-file facts itself; the production indexing path collects them ONCE and
/// shares them with the relationship pass via `extract_symbols_with_facts`.
pub fn extract_symbols(
    tree: &Tree,
    source: &str,
    language: Language,
    file_path: &str,
) -> Vec<Symbol> {
    let facts = collect_extraction_facts(tree, source, language);
    extract_symbols_with_facts(tree, source, language, file_path, &facts)
}

/// Symbol extraction over pre-collected per-file facts, so a single
/// routes-and-constants pass can be shared with
/// `extract_symbol_relationships_with_facts`.
pub(crate) fn extract_symbols_with_facts(
    tree: &Tree,
    source: &str,
    language: Language,
    file_path: &str,
    facts: &ExtractionFacts,
) -> Vec<Symbol> {
    let extractor = SymbolExtractor::new(file_path.to_string());
    let mut symbols = extractor.extract(tree, source, language);
    // M4.4: append a `Route` symbol per detected HTTP route registration (Python
    // decorators, NestJS `@Controller`/`@Get`, Express `app`/`router.METHOD`).
    // Each is named with its canonical `"<METHOD> <path>"`; the matching `Handles`
    // edge to the handler is emitted later by the relationship pass.
    append_route_symbols(&facts.routes, file_path, &mut symbols);
    // Source-order-independent impl ownership: re-parent methods whose parent
    // is an `Impl` symbol to the actual type symbol when the type was declared
    // after its impl. The single-pass extractor cannot see forward declarations,
    // so this post-pass closes the gap (feature contract: source-order-
    // independent impl ownership).
    reparent_rust_impl_methods(file_path, &mut symbols);
    symbols
}

/// Re-parent Rust impl methods to their actual type symbol when the type was
/// declared after the impl block.
///
/// During the single pre-order extraction pass, `child_ctx` scans
/// `symbols-so-far` for the implemented type. When the `struct`/`enum`/`trait`
/// appears *after* the `impl`, the type is not yet in `symbols`, so methods
/// are incorrectly parented to the `Impl` block itself. This post-pass:
///
/// 1. builds a name → type-symbol map from all extracted symbols;
/// 2. finds `Impl` symbols whose implemented type now matches a real type;
/// 3. re-parents the children of those `Impl` symbols to the type symbol,
///    updating `parent_id`, `qualified_name`, and `id`.
///
/// `impl Trait for Type` methods are parented to `Type` (the implementing
/// type), consistent with the extractor's `child_ctx` behavior when the type
/// is found.
fn reparent_rust_impl_methods(file_path: &str, symbols: &mut Vec<Symbol>) {
    // Build a map of type name → type symbol ID.
    let mut type_ids: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for sym in symbols.iter() {
        if is_rust_type_symbol(sym.symbol_type) {
            type_ids.entry(sym.name.as_str()).or_insert(sym.id.as_str());
        }
    }
    if type_ids.is_empty() {
        return;
    }

    // Find Impl symbols whose implemented type matches a real type.
    // Collect (impl_id, type_id, type_name) triples.
    let mut reparents: Vec<(String, String, String)> = Vec::new();
    for sym in symbols.iter() {
        if sym.symbol_type != SymbolType::Impl {
            continue;
        }
        // `impl Foo` → `Foo`; `impl Trait for Foo` → (`Foo`, `Trait`).
        let (type_name, trait_name) = match sym.name.strip_prefix("impl ") {
            Some(rest) => {
                if let Some(pos) = rest.find(" for ") {
                    (rest[pos + 5..].trim(), Some(rest[..pos].trim()))
                } else {
                    (rest.trim(), None)
                }
            }
            None => continue,
        };
        if let Some(&type_id) = type_ids.get(type_name) {
            // Only reparent if the impl's children are currently parented to
            // the impl block (i.e. the type wasn't found during extraction).
            let normalized_type_name = type_name.strip_prefix("r#").unwrap_or(type_name);
            let owner_qualified_name = trait_name
                .map(|trait_name| {
                    format!(
                        "{normalized_type_name} as {}",
                        trait_name.strip_prefix("r#").unwrap_or(trait_name)
                    )
                })
                .unwrap_or_else(|| normalized_type_name.to_string());
            reparents.push((
                sym.id.clone(),
                type_id.to_string(),
                owner_qualified_name,
            ));
        }
    }
    if reparents.is_empty() {
        return;
    }

    // Re-parent children of each impl block to the actual type symbol.
    for sym in symbols.iter_mut() {
        let Some(ref parent_id) = sym.parent_id else {
            continue;
        };
        // Find a matching reparent entry.
        let Some((_, new_parent_id, owner_qualified_name)) =
            reparents.iter().find(|(impl_id, _, _)| impl_id == parent_id)
        else {
            continue;
        };
        // Update parent_id, qualified_name, and id.
        let new_parent_id = new_parent_id.clone();
        let new_qn = format!("{}::{}", owner_qualified_name, sym.name);
        let new_id = stable_symbol_id(file_path, &new_qn, sym.symbol_type);
        sym.parent_id = Some(new_parent_id);
        sym.qualified_name = new_qn;
        sym.id = new_id;
    }
}

/// Detect HTTP route registrations and append one `Route` symbol per route
/// (M4.4). The route's canonical name (`"<METHOD> <canon-path>"`) is its own
/// `qualified_name`, and its id is derived with `stable_symbol_id` so the
/// relationship pass can recover it for the `Handles` edge. Duplicate
/// `<METHOD> <path>` routes in one file collapse to a single node.
fn append_route_symbols(routes: &[DetectedRoute], file_path: &str, symbols: &mut Vec<Symbol>) {
    let mut seen: HashSet<String> = HashSet::new();
    for route in routes {
        let qn = route.symbol_name();
        if !seen.insert(qn.clone()) {
            continue;
        }
        let mut symbol = Symbol::new(
            qn.clone(),
            SymbolType::Route,
            file_path.to_string(),
            route.reg_range,
        );
        symbol.qualified_name = qn.clone();
        symbol.byte_offset = route.reg_byte_offset;
        symbol.byte_length = route.reg_byte_length;
        // The Route node is synthetic (no own source span text to hash); hash its
        // canonical identity so the value is stable and span-independent.
        symbol.content_hash = compute_content_hash(&qn);
        symbol.id = stable_symbol_id(file_path, &qn, SymbolType::Route);
        symbols.push(symbol);
    }
}

/// Follow the C/C++ `declarator` nesting (pointer / reference / array / function /
/// init / parenthesized declarators) down to the innermost *name* node — an
/// `identifier`, `field_identifier`, `qualified_identifier`, `type_identifier`,
/// `destructor_name`, `operator_name` or `operator_cast`. Returns `None` for an
/// abstract declarator that names nothing. (M5.3)
fn cpp_innermost_declarator_name<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    match node.kind() {
        "identifier"
        | "field_identifier"
        | "qualified_identifier"
        | "type_identifier"
        | "destructor_name"
        | "operator_name"
        | "operator_cast" => Some(*node),
        "function_declarator"
        | "pointer_declarator"
        | "reference_declarator"
        | "array_declarator"
        | "init_declarator" => {
            let child = node.child_by_field_name("declarator")?;
            cpp_innermost_declarator_name(&child)
        }
        "parenthesized_declarator" => {
            let mut cursor = node.walk();
            let found = node
                .named_children(&mut cursor)
                .find_map(|child| cpp_innermost_declarator_name(&child));
            found
        }
        _ => None,
    }
}

/// Whether a declarator subtree declares a function — it reaches a
/// `function_declarator` through pointer / reference / array / init wrappers. A
/// function POINTER (`(*fp)(int)`, i.e. via a `parenthesized_declarator`) is
/// deliberately NOT a function here: it is a variable or typedef, not a callable
/// definition. (M5.3)
fn cpp_declarator_is_function(node: &Node) -> bool {
    match node.kind() {
        "function_declarator" => true,
        "pointer_declarator" | "reference_declarator" | "array_declarator" | "init_declarator" => {
            node.child_by_field_name("declarator")
                .map(|child| cpp_declarator_is_function(&child))
                .unwrap_or(false)
        }
        _ => false,
    }
}

/// A callable's symbol type. It is a Method when either its innermost name-node
/// kind marks a member (`field_identifier`, `qualified_identifier`,
/// `destructor_name`, `operator_name`, `operator_cast`) OR the callable sits
/// directly in a class/struct body (`field_declaration_list`) — the latter
/// catches constructors, whose name is a plain `identifier` matching the type.
/// Otherwise it is a free Function. (M5.3)
/// Text of a declarator name node, defensively reduced to its last
/// whitespace-delimited token. The C grammar, lacking a preprocessor, sometimes
/// folds a macro modifier into the declarator (`bool __cold foo(...)` parses the
/// name as `__cold foo`); keeping only the trailing token recovers the real name
/// — `foo`. A no-op for ordinary single-token identifiers and `a::b::c` qualified
/// names (no embedded whitespace). Returns `None` if nothing usable remains. (M5.3)
fn cpp_clean_declarator_name(name_node: &Node, source: &str) -> Option<String> {
    let text = name_node.safe_text(source).ok()?;
    let name = text.split_whitespace().next_back()?;
    (!name.is_empty()).then(|| name.to_string())
}

fn cpp_callable_symbol_type(name_node: &Node, callable_node: &Node) -> SymbolType {
    let is_member = matches!(
        name_node.kind(),
        "field_identifier"
            | "qualified_identifier"
            | "destructor_name"
            | "operator_name"
            | "operator_cast"
    ) || callable_node
        .parent()
        .is_some_and(|p| p.kind() == "field_declaration_list");
    if is_member {
        SymbolType::Method
    } else {
        SymbolType::Function
    }
}

/// Convenience relationship extraction. Collects the per-file facts itself;
/// the production indexing path collects them ONCE and shares them with the
/// symbol pass via `extract_symbol_relationships_with_facts`.
pub fn extract_symbol_relationships(
    tree: &Tree,
    source: &str,
    language: Language,
    file_path: &str,
    symbols: &[Symbol],
) -> Vec<SymbolRelationship> {
    let facts = collect_extraction_facts(tree, source, language);
    extract_symbol_relationships_with_facts(tree, source, language, file_path, symbols, &facts)
}

/// Relationship extraction over pre-collected per-file facts (shared with
/// `extract_symbols_with_facts` by the production indexing path).
pub(crate) fn extract_symbol_relationships_with_facts(
    tree: &Tree,
    source: &str,
    language: Language,
    file_path: &str,
    symbols: &[Symbol],
    facts: &ExtractionFacts,
) -> Vec<SymbolRelationship> {
    // M5.3 / N8: a language whose capability declares no relationship extraction
    // (graduated C/C++ is definitions-only until source-attribution fixtures
    // exist) skips the ENTIRE edge walk — calls, uses_type, reads_env, structural,
    // imports and routes. One capability-driven gate keeps every concern off for
    // such languages instead of auditing each concern for safety. No-op for the
    // five full grammars (all declare `relationships: true`).
    if !language.capability().extracts.relationships {
        return Vec::new();
    }

    let extractor = SymbolExtractor::new(file_path.to_string());
    // M5.1 (FIX 4): the simple names of every class-like symbol in this file, so
    // `x = Foo()` is only constructor-typed when `Foo` is a real class/struct —
    // never a factory function. Built once before the relationship walk.
    let class_names: HashSet<String> = symbols
        .iter()
        .filter(|s| is_class_like_symbol(s.symbol_type))
        .map(|s| s.name.clone())
        .collect();
    let mut state = WalkState {
        scope: Vec::new(),
        symbols: Vec::new(),
        var_types: Vec::new(),
        class_names,
        const_shadows: Vec::new(),
    };
    // Track B: file-local Rust constant targets (name → symbol id), built once
    // from the full symbol set so a constant declared AFTER its use still
    // resolves. Only ROOT-level constants (`parent_id: None`) are eligible: a
    // const nested in `mod x { }` is not addressable as a bare identifier from
    // scopes outside that module, and fn-local / impl-associated consts are
    // likewise not file-wide bare names — including any of them fabricates
    // edges (false negative over a wrong edge, so the strictest rule wins).
    // A name bound by more than one eligible constant symbol in this file is
    // ambiguous and dropped. Only Rust pays for the map; every other language
    // keeps it empty (O(1) concern reject).
    let mut const_targets: HashMap<&str, &str> = HashMap::new();
    if language == Language::Rust {
        let mut ambiguous: HashSet<&str> = HashSet::new();
        for symbol in symbols {
            if symbol.symbol_type != SymbolType::Constant
                || symbol.parent_id.is_some()
                || ambiguous.contains(symbol.name.as_str())
            {
                continue;
            }
            if let Some(prev) = const_targets.insert(symbol.name.as_str(), symbol.id.as_str()) {
                if prev != symbol.id.as_str() {
                    const_targets.remove(symbol.name.as_str());
                    ambiguous.insert(symbol.name.as_str());
                }
            }
        }
    }
    // The per-file module-level constant map (M4.2): `NAME = "literal"` bindings
    // used to resolve bare-identifier env-var KEYs. Precomputed in the shared
    // facts (not during the walk) so a const declared *after* its use still
    // resolves — and so route detection reuses the identical map.
    let import_bindings = js_ts_import_bindings(&tree.root_node(), source, language);
    let mut rel = RelState {
        relationships: Vec::new(),
        seen: HashSet::new(),
        call_seen: HashSet::new(),
        all_symbols: symbols,
        const_map: &facts.constants,
        const_targets,
        import_bindings,
    };

    // Unified relationship walk: call/macro + structural edges off one DFS,
    // attributed to the enclosing symbol via the scope stack.
    extractor.walk_relationships(tree.root_node(), source, language, &mut state, &mut rel);

    // Import edges are derived from the symbol list, not the tree.
    extract_import_relationships(file_path, symbols, &mut rel.relationships, &mut rel.seen);

    // M4.4: `Handles` edges (Route → handler). The production extraction path
    // shares its route facts with symbol extraction; compatibility callers that
    // request relationships separately still detect them once here. Routes whose
    // handler cannot be identified emit no edge (the Route node alone is the
    // anchor fallback).
    emit_route_handles(
        &facts.routes,
        file_path,
        symbols,
        &mut rel.relationships,
        &mut rel.seen,
    );

    rel.relationships
}

/// Emit one `Handles` edge per detected route whose handler resolves to a
/// function/method symbol in `all_symbols` (M4.4). DIRECTION: source = the
/// synthetic `Route` symbol, target = the handler (so "what handles `POST
/// /api/orders`" is the Route node's outgoing `Handles` edge). The handler is
/// matched either by its definition position (Python-decorated function, NestJS
/// method) or by name (an Express identifier callback). A route with no resolvable
/// handler contributes no edge.
fn emit_route_handles(
    routes: &[DetectedRoute],
    file_path: &str,
    all_symbols: &[Symbol],
    relationships: &mut Vec<SymbolRelationship>,
    seen: &mut HashSet<(String, String, SymbolRelationshipType, u32)>,
) {
    for route in routes {
        let qn = route.symbol_name();
        let route_id = stable_symbol_id(file_path, &qn, SymbolType::Route);
        let Some(handler) = resolve_route_handler(&route.handler, all_symbols) else {
            continue;
        };
        let key = (
            route_id.clone(),
            handler.name.clone(),
            SymbolRelationshipType::Handles,
            route.reg_line,
        );
        if seen.insert(key) {
            relationships.push(SymbolRelationship {
                source_symbol_id: route_id,
                source_file_path: file_path.to_string(),
                target_name: handler.name.clone(),
                target_symbol_id: Some(handler.id.clone()),
                relationship_type: SymbolRelationshipType::Handles,
                line: route.reg_line,
                ..Default::default()
            });
        }
    }
}

/// Resolve a route's `HandlerRef` to the concrete handler symbol, restricted to
/// function/method symbols. Position matching is exact (a symbol's range start is
/// its defining node's start), so it cannot collide with the Route node itself.
fn resolve_route_handler<'a>(
    handler: &HandlerRef,
    all_symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    let is_callable = |symbol: &Symbol| {
        matches!(
            symbol.symbol_type,
            SymbolType::Function | SymbolType::Method
        )
    };
    match handler {
        HandlerRef::ByPosition(pos) => all_symbols
            .iter()
            .find(|symbol| symbol.range.start == *pos && is_callable(symbol)),
        HandlerRef::ByName(name) => all_symbols
            .iter()
            .find(|symbol| symbol.name == *name && is_callable(symbol)),
        HandlerRef::None => None,
    }
}

// ---- M4.4 route detection + canonicalization -------------------------------

/// One detected HTTP route registration: its method + canonical path, the span of
/// the registration site (decorator / call / method-decorator) for the synthetic
/// `Route` symbol, and how to find its handler symbol.
pub(crate) struct DetectedRoute {
    method: String,
    canon_path: String,
    reg_range: Range,
    reg_byte_offset: usize,
    reg_byte_length: usize,
    reg_line: u32,
    handler: HandlerRef,
}

impl DetectedRoute {
    /// The canonical Route symbol name / qualified_name: `"<METHOD> <canon-path>"`.
    fn symbol_name(&self) -> String {
        format!("{} {}", self.method, self.canon_path)
    }
}

/// How to locate a route's handler symbol in the file's symbol set.
enum HandlerRef {
    /// The handler is the function/method whose definition node STARTS here
    /// (Python-decorated function, NestJS controller method).
    ByPosition(Position),
    /// The handler is a named function/method referenced by an Express callback
    /// identifier (`router.post("/x", handler)`).
    ByName(String),
    /// No identifiable handler (inline anonymous callback / unresolved) — the
    /// Route node is emitted alone (low-confidence anchor fallback).
    None,
}

/// Canonicalize a raw route path so the SAME route unifies across frameworks:
/// every parameter syntax collapses to a single `{}` token. Recognized param
/// forms — Express `:name`, FastAPI/OpenAPI `{name}`, Flask `<id>` / `<int:id>` /
/// `<path:p>`, and template `${…}` — all become `{}`; literal segments are
/// preserved. A trailing slash is stripped (except the root `/`).
///
/// `/users/:id`, `/users/{id}`, and `/users/<int:id>` all canonicalize to
/// `/users/{}`.
fn route_canon_path(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return "/".to_string();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    // `true` at the very start and immediately after a `/` — the only position an
    // Express `:name` param may begin (so a stray `:` elsewhere is left literal).
    let mut at_segment_start = true;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            // Template `${…}` — collapse, skipping the brace-balanced interior.
            b'$' if i + 1 < bytes.len() && bytes[i + 1] == b'{' => {
                out.push_str("{}");
                i += 2;
                let mut depth = 1u32;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                at_segment_start = false;
            }
            // FastAPI / OpenAPI `{name}` — collapse, skipping to the matching `}`.
            b'{' => {
                out.push_str("{}");
                i += 1;
                let mut depth = 1u32;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                at_segment_start = false;
            }
            // Flask `<id>` / `<int:id>` / `<path:p>` — collapse to the matching `>`.
            b'<' => {
                out.push_str("{}");
                i += 1;
                while i < bytes.len() && bytes[i] != b'>' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1; // consume `>`
                }
                at_segment_start = false;
            }
            // Express `:name` — only at a segment start; consume to the next `/`.
            b':' if at_segment_start => {
                out.push_str("{}");
                i += 1;
                while i < bytes.len() && bytes[i] != b'/' {
                    i += 1;
                }
                at_segment_start = false;
            }
            _ => {
                out.push(c as char);
                at_segment_start = c == b'/';
                i += 1;
            }
        }
    }
    // Strip a trailing slash (except the bare root `/`).
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

/// The only languages with route-extraction rules in `walk_routes`. Everything
/// else (Rust, Go, C/C++, …) skips route detection entirely — no constants map,
/// no tree walk — because it could never emit a route.
const fn supports_route_detection(language: Language) -> bool {
    matches!(
        language,
        Language::Python
            | Language::TypeScript
            | Language::Tsx
            | Language::Astro
            | Language::JavaScript
            | Language::Jsx
    )
}

/// Detect every HTTP route registration in the file (M4.4). Framework coverage:
/// Python decorators (`@app.route`/`@app.get`/`@router.post`), NestJS
/// `@Controller` + `@Get`/`@Post`/… method decorators, and Express/JS
/// `app`/`router.METHOD(...)` calls. Paths passed as bare identifiers are
/// resolved through the M4.2 module-level constant map. Recursive dispatch
/// driven by `collect_extraction_facts`: visit each node and run the
/// per-framework route extractors that apply to `language`.
fn walk_routes(
    node: &Node,
    source: &str,
    language: Language,
    const_map: &HashMap<String, String>,
    out: &mut Vec<DetectedRoute>,
) {
    match language {
        Language::Python if node.kind() == "decorated_definition" => {
            python_routes_from_decorated(node, source, const_map, out);
        }
        Language::Python => {}
        Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx => {
            if matches!(node.kind(), "class_declaration" | "class") {
                nest_routes_from_class(node, source, const_map, out);
            }
            if node.kind() == "call_expression" {
                express_route_from_call(node, source, const_map, out);
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_routes(&child, source, language, const_map, out);
    }
}

/// Map a verb word to its canonical upper-case HTTP method, or `None` when the
/// word is not a recognized verb. Case-insensitive (so Python `get` / NestJS
/// `Get` / Express `GET` all resolve).
fn http_method_word(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "get" => Some("GET"),
        "post" => Some("POST"),
        "put" => Some("PUT"),
        "delete" => Some("DELETE"),
        "patch" => Some("PATCH"),
        "head" => Some("HEAD"),
        "options" => Some("OPTIONS"),
        _ => None,
    }
}

/// Python route decorators (`decorated_definition`): a function decorated with
/// `@app.route(...)`/`@app.get(...)`/`@router.post(...)` (FastAPI/Flask). The
/// decorated FUNCTION is the handler; the METHOD comes from the verb in the
/// decorator name (`get` → `GET`) or, for `route`, the `methods=[…]` keyword
/// (defaulting to `GET`). Only a function (not a decorated class) yields routes,
/// and only a decorator whose call resolves to a verb/`route` does — so ordinary
/// decorators (`@login_required`) are ignored.
fn python_routes_from_decorated(
    node: &Node,
    source: &str,
    const_map: &HashMap<String, String>,
    out: &mut Vec<DetectedRoute>,
) {
    let Some(definition) = node.child_by_field_name("definition") else {
        return;
    };
    if definition.kind() != "function_definition" {
        return;
    }
    let start = definition.start_position();
    let handler_pos = Position::new(start.row as u32, start.column as u32);

    let mut cursor = node.walk();
    for decorator in node.named_children(&mut cursor) {
        if decorator.kind() != "decorator" {
            continue;
        }
        let Some(call) = first_child_of_kind(&decorator, "call") else {
            continue;
        };
        let Some(func) = call.child_by_field_name("function") else {
            continue;
        };
        let method_token = match func.kind() {
            "attribute" => func
                .child_by_field_name("attribute")
                .and_then(|n| node_text(&n, source)),
            "identifier" => node_text(&func, source),
            _ => None,
        };
        let Some(method_token) = method_token else {
            continue;
        };
        let Some(args) = call.child_by_field_name("arguments") else {
            continue;
        };
        // The METHODS this decorator registers.
        let methods: Vec<String> = if let Some(verb) = http_method_word(method_token) {
            vec![verb.to_string()]
        } else if method_token == "route" {
            parse_methods_kwarg(&args, source).unwrap_or_else(|| vec!["GET".to_string()])
        } else {
            continue;
        };
        let Some(path) =
            first_positional_arg(&args).and_then(|arg| literal_or_const(&arg, source, const_map))
        else {
            continue;
        };
        let canon = route_canon_path(&path);
        for method in methods {
            out.push(DetectedRoute {
                method,
                canon_path: canon.clone(),
                reg_range: Range::from_node(&decorator),
                reg_byte_offset: decorator.start_byte(),
                reg_byte_length: decorator.end_byte().saturating_sub(decorator.start_byte()),
                reg_line: decorator.start_position().row as u32,
                handler: HandlerRef::ByPosition(handler_pos),
            });
        }
    }
}

/// The `methods=[…]` keyword argument of a Flask `@app.route(...)` call as
/// upper-cased method names, or `None` when no such keyword (caller defaults to
/// `GET`).
fn parse_methods_kwarg(args: &Node, source: &str) -> Option<Vec<String>> {
    let mut cursor = args.walk();
    for child in args.named_children(&mut cursor) {
        if child.kind() != "keyword_argument" {
            continue;
        }
        let Some(name) = child.child_by_field_name("name") else {
            continue;
        };
        if node_text(&name, source) != Some("methods") {
            continue;
        }
        let Some(value) = child.child_by_field_name("value") else {
            continue;
        };
        if value.kind() != "list" {
            continue;
        }
        let mut methods = Vec::new();
        let mut vc = value.walk();
        for item in value.named_children(&mut vc) {
            if let Some(v) = string_literal_value(&item, source) {
                methods.push(v.to_ascii_uppercase());
            }
        }
        if !methods.is_empty() {
            return Some(methods);
        }
    }
    None
}

/// The first POSITIONAL argument node of an argument list (skips Python
/// `keyword_argument`s).
fn first_positional_arg<'t>(args: &Node<'t>) -> Option<Node<'t>> {
    let mut cursor = args.walk();
    let found = args
        .named_children(&mut cursor)
        .find(|c| c.kind() != "keyword_argument");
    found
}

/// NestJS controller routes: a class carrying `@Controller("base")` whose methods
/// carry `@Get`/`@Post`/`@Put`/`@Delete`/`@Patch`/`@Head`/`@Options`/`@All`. The
/// route is `<METHOD> <base + sub>`; the decorated METHOD is the handler.
/// `@Controller` is REQUIRED (a plain class with a stray `@Get` is not promoted).
fn nest_routes_from_class(
    class_node: &Node,
    source: &str,
    const_map: &HashMap<String, String>,
    out: &mut Vec<DetectedRoute>,
) {
    let decorators = collect_class_decorators(class_node);
    let mut base_path: Option<String> = None;
    for decorator in &decorators {
        let Some(call) = first_child_of_kind(decorator, "call_expression") else {
            continue;
        };
        let Some(func) = call.child_by_field_name("function") else {
            continue;
        };
        if func.kind() != "identifier" || node_text(&func, source) != Some("Controller") {
            continue;
        }
        let base = call
            .child_by_field_name("arguments")
            .and_then(|args| first_positional_arg(&args))
            .and_then(|arg| literal_or_const(&arg, source, const_map))
            .unwrap_or_default();
        base_path = Some(base);
        break;
    }
    // Require @Controller — conservative gate against false routes.
    let Some(base) = base_path else {
        return;
    };

    let Some(body) = class_node.child_by_field_name("body") else {
        return;
    };
    let mut pending: Vec<Node> = Vec::new();
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        match child.kind() {
            "decorator" => pending.push(child),
            "method_definition" => {
                nest_routes_for_method(&child, &pending, &base, source, const_map, out);
                pending.clear();
            }
            _ => pending.clear(),
        }
    }
}

/// Emit a route for each HTTP-method decorator preceding a NestJS controller
/// method. The handler is the method itself (matched later by definition start).
fn nest_routes_for_method(
    method_node: &Node,
    decorators: &[Node],
    base: &str,
    source: &str,
    const_map: &HashMap<String, String>,
    out: &mut Vec<DetectedRoute>,
) {
    let start = method_node.start_position();
    let handler_pos = Position::new(start.row as u32, start.column as u32);
    for decorator in decorators {
        let Some(call) = first_child_of_kind(decorator, "call_expression") else {
            continue;
        };
        let Some(func) = call.child_by_field_name("function") else {
            continue;
        };
        if func.kind() != "identifier" {
            continue;
        }
        let Some(name) = node_text(&func, source) else {
            continue;
        };
        // Nest decorators are PascalCase; match exactly so a lowercase call cannot.
        let method = match name {
            "Get" | "Post" | "Put" | "Delete" | "Patch" | "Head" | "Options" => {
                http_method_word(name).unwrap_or("GET")
            }
            "All" => "ALL",
            _ => continue,
        };
        let sub = call
            .child_by_field_name("arguments")
            .and_then(|args| first_positional_arg(&args))
            .and_then(|arg| literal_or_const(&arg, source, const_map))
            .unwrap_or_default();
        let canon = route_canon_path(&join_route_paths(base, &sub));
        out.push(DetectedRoute {
            method: method.to_string(),
            canon_path: canon,
            reg_range: Range::from_node(decorator),
            reg_byte_offset: decorator.start_byte(),
            reg_byte_length: decorator.end_byte().saturating_sub(decorator.start_byte()),
            reg_line: decorator.start_position().row as u32,
            handler: HandlerRef::ByPosition(handler_pos),
        });
    }
}

/// Gather the `decorator` nodes attached to a class: the class node's own leading
/// decorators (non-exported `@Controller class …`) plus, when the class is the
/// declaration of an `export_statement`, that statement's decorators
/// (`@Controller export class …`).
fn collect_class_decorators<'t>(class_node: &Node<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut cursor = class_node.walk();
    for child in class_node.children(&mut cursor) {
        if child.kind() == "decorator" {
            out.push(child);
        }
    }
    if let Some(parent) = class_node.parent() {
        if parent.kind() == "export_statement" {
            let mut pc = parent.walk();
            for child in parent.children(&mut pc) {
                if child.kind() == "decorator" {
                    out.push(child);
                }
            }
        }
    }
    out
}

/// Join a NestJS controller base path and a method sub-path into a single raw
/// path, dropping empty segments (`"users"` + `":id"` → `/users/:id`; `""` + `""`
/// → `/`). Canonicalization happens afterwards.
fn join_route_paths(base: &str, sub: &str) -> String {
    let mut segs: Vec<&str> = Vec::new();
    for part in [base, sub] {
        for seg in part.split('/') {
            if !seg.is_empty() {
                segs.push(seg);
            }
        }
    }
    if segs.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segs.join("/"))
    }
}

/// Express/JS route call: `app.get("/x", handler)` / `router.post("/x", h)` (also
/// `put`/`delete`/`patch`/`head`/`options`/`all`/`use`). To avoid false routes
/// from generic `.get()` calls (`map.get(k)`, `obj.get()`), the receiver MUST be
/// an app/router-like identifier AND the first argument MUST be a string path
/// (starting with `/`). The handler is the LAST argument when it is a bare
/// identifier; an inline anonymous callback yields a Route with no handler.
fn express_route_from_call(
    call: &Node,
    source: &str,
    const_map: &HashMap<String, String>,
    out: &mut Vec<DetectedRoute>,
) {
    let Some(func) = call.child_by_field_name("function") else {
        return;
    };
    if func.kind() != "member_expression" {
        return;
    }
    let Some(object) = func.child_by_field_name("object") else {
        return;
    };
    if object.kind() != "identifier" {
        return;
    }
    let Some(receiver) = node_text(&object, source) else {
        return;
    };
    if !is_app_router_like(receiver) {
        return;
    }
    let Some(property) = func.child_by_field_name("property") else {
        return;
    };
    let Some(prop) = node_text(&property, source) else {
        return;
    };
    let Some(method) = express_method_word(prop) else {
        return;
    };
    let Some(args) = call.child_by_field_name("arguments") else {
        return;
    };
    let arg_nodes: Vec<Node> = {
        let mut cursor = args.walk();
        args.named_children(&mut cursor).collect()
    };
    let Some(path_node) = arg_nodes.first() else {
        return;
    };
    let Some(path) = literal_or_const(path_node, source, const_map) else {
        return;
    };
    // A real route path starts with `/`; this is the decisive gate that rejects
    // `map.get(key)` / `obj.get()` and other generic accessor calls.
    if !path.starts_with('/') {
        return;
    }
    let canon = route_canon_path(&path);

    // Handler = the LAST argument when it names a function; inline callbacks and
    // path-only calls leave the route handler-less (anchor fallback).
    let handler = match arg_nodes.last() {
        Some(last) if arg_nodes.len() >= 2 && last.kind() == "identifier" => {
            HandlerRef::ByName(node_text(last, source).unwrap_or_default().to_string())
        }
        _ => HandlerRef::None,
    };

    out.push(DetectedRoute {
        method: method.to_string(),
        canon_path: canon,
        reg_range: Range::from_node(call),
        reg_byte_offset: call.start_byte(),
        reg_byte_length: call.end_byte().saturating_sub(call.start_byte()),
        reg_line: call.start_position().row as u32,
        handler,
    });
}

/// Express method words (HTTP verbs plus `all` / `use`) → canonical method label.
fn express_method_word(prop: &str) -> Option<String> {
    if let Some(verb) = http_method_word(prop) {
        return Some(verb.to_string());
    }
    match prop.to_ascii_lowercase().as_str() {
        "all" => Some("ALL".to_string()),
        "use" => Some("USE".to_string()),
        _ => None,
    }
}

/// True when an identifier names an Express app/router-like object. Conservative
/// allow-list (`app`/`router`/`api`/`server`/`route`/`routes` and the common
/// `*Router`/`*App`/`*Routes` suffixes) so generic receivers (`map`, `cache`,
/// `obj`) never register routes.
fn is_app_router_like(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "app" | "router" | "api" | "server" | "route" | "routes"
    ) || lower.ends_with("router")
        || lower.ends_with("app")
        || lower.ends_with("routes")
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
    let Ok(text) = node.safe_text(source) else {
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
    let Ok(text) = node.safe_text(source) else {
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

    let Ok(text) = node.safe_text(source) else {
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
            let Some((receiver_type, is_pointer)) = go_receiver_type_name(&receiver, source)
            else {
                return;
            };
            let Some(method_name) = node
                .child_by_field_name("name")
                .and_then(|name| name.safe_text(source).ok())
            else {
                return;
            };
            let Some(type_symbol) = find_symbol_by_name(symbols, &receiver_type) else {
                return;
            };
            let line = node.start_position().row as u32;
            let key = (
                type_symbol.id.clone(),
                method_name.to_string(),
                SymbolRelationshipType::Contains,
                line,
            );
            if seen.insert(key) {
                relationships.push(SymbolRelationship {
                    source_symbol_id: type_symbol.id.clone(),
                    source_file_path: file_path.to_string(),
                    target_name: method_name.to_string(),
                    target_symbol_id: None,
                    relationship_type: SymbolRelationshipType::Contains,
                    line,
                    // Track C: pointer/value receiver kind, persisted to
                    // `metadata_json` as `{"receiver":"…"}` so the store-side
                    // implicit-interface miner can honor Go method-set
                    // semantics (a pointer-receiver method is absent from the
                    // value type's method set).
                    receiver_kind: Some(
                        if is_pointer { "pointer" } else { "value" }.to_string(),
                    ),
                    ..Default::default()
                });
            }
        }
        // Struct embedding: `type Server struct { Base }` → Server extends Base.
        // Track C adds the interface analogue: `type Cache interface { Base }`
        // → Cache extends Base (embedded interfaces contribute inherited
        // method requirements in the store-side miner).
        "type_spec" => {
            let Some(type_node) = node.child_by_field_name("type") else {
                return;
            };
            let Some(type_name) = node
                .child_by_field_name("name")
                .and_then(|name| name.safe_text(source).ok())
            else {
                return;
            };
            let embedded_names = match type_node.kind() {
                "struct_type" => go_embedded_type_names(&type_node, source),
                "interface_type" => go_embedded_interface_names(&type_node, source),
                _ => return,
            };
            if embedded_names.is_empty() {
                return;
            }
            let Some(type_symbol) = find_symbol_by_name(symbols, type_name) else {
                return;
            };
            let line = node.start_position().row as u32;
            for embedded in embedded_names {
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

/// Resolve the base type name of a Go method receiver (`(s *Server)` →
/// `Server`), plus whether it is a POINTER (`*Server`) or VALUE (`Server`)
/// receiver — the distinction Go method sets depend on (Track C).
fn go_receiver_type_name(receiver: &Node, source: &str) -> Option<(String, bool)> {
    let mut type_node = None;
    let mut cursor = receiver.walk();
    for param in receiver.named_children(&mut cursor) {
        if let Some(ty) = param.child_by_field_name("type") {
            type_node = Some(ty);
            break;
        }
    }
    let mut ty = type_node?;
    let mut is_pointer = false;
    while ty.kind() == "pointer_type" {
        is_pointer = true;
        ty = last_named_child(&ty)?;
    }
    // Generic receiver (`(p Pair[K, V])` / `(p *Pair[K, V])`): reduce the
    // `generic_type` to its base `type` field so the name matches the declared
    // type symbol (`Pair`). `normalize_reference_name` also strips a `[...]`
    // suffix as a backstop for text-level forms.
    if ty.kind() == "generic_type" {
        ty = ty.child_by_field_name("type")?;
    }
    ty.safe_text(source)
        .ok()
        .and_then(normalize_reference_name)
        .map(|name| (name, is_pointer))
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
        if let Some(name) = ty.safe_text(source).ok().and_then(normalize_reference_name) {
            names.push(name);
        }
    }
    names
}

/// Track C: collect the names of interfaces EMBEDDED in a Go `interface_type`
/// (`type Cache interface { Base; Set(string, []byte) error }` → `["Base"]`).
/// Only a `type_elem` holding EXACTLY ONE named type (`type_identifier` /
/// `qualified_type`, or a `generic_type` instantiation of one — `Getter[int]`
/// embeds `Getter`) is an embedding; union / negated generic-constraint
/// elements (`~int | string`) and `method_elem` specs emit nothing (false
/// negative over a fabricated edge). A qualified embedding (`io.Reader`)
/// reduces to its terminal name, matching the struct-embedding arm.
fn go_embedded_interface_names(interface_type: &Node, source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = interface_type.walk();
    for elem in interface_type.named_children(&mut cursor) {
        if elem.kind() != "type_elem" || elem.named_child_count() != 1 {
            continue;
        }
        let Some(mut ty) = elem.named_child(0) else {
            continue;
        };
        // A generic instantiation (`Getter[int]`) embeds its BASE interface:
        // reduce `generic_type` to its `type` field before the name gate.
        if ty.kind() == "generic_type" {
            let Some(base) = ty.child_by_field_name("type") else {
                continue;
            };
            ty = base;
        }
        if !matches!(ty.kind(), "type_identifier" | "qualified_type") {
            continue;
        }
        if let Some(name) = ty.safe_text(source).ok().and_then(normalize_reference_name) {
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
            ..Default::default()
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
    // Go generics: `Pair[K, V]` reduces to its base name `Pair` (also drops
    // slice/array prefixes like `[]byte` to None — never a named target).
    let value = value.split('[').next().unwrap_or(value).trim();
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

// ---- Track B file-local Rust constant usage ---------------------------------

/// Record every plain `identifier` in a binding-pattern subtree that collides
/// with a file-local constant name as a shadow in the INNERMOST frame, carrying
/// `activation_byte`. Descending the whole pattern subtree over-collects (a
/// unit-struct or const pattern also lands here), but a shadow only ever
/// SUPPRESSES edges, never fabricates one.
fn record_const_shadows(
    pattern: &Node,
    activation_byte: usize,
    source: &str,
    state: &mut WalkState,
    rel: &RelState,
) {
    let Some(frame) = state.const_shadows.last_mut() else {
        return;
    };
    collect_pattern_const_shadows(pattern, activation_byte, source, &rel.const_targets, frame);
}

/// Recursive worker for `record_const_shadows`: only names that pass the cheap
/// uppercase pre-filter AND exist in the constant-target map are recorded, so
/// frames stay empty for ordinary snake_case bindings.
fn collect_pattern_const_shadows(
    node: &Node,
    activation_byte: usize,
    source: &str,
    const_targets: &HashMap<&str, &str>,
    frame: &mut Vec<(String, usize)>,
) {
    if node.kind() == "identifier" {
        if let Ok(name) = node.safe_text(source) {
            if name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && const_targets.contains_key(name)
            {
                frame.push((name.to_string(), activation_byte));
            }
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_pattern_const_shadows(&child, activation_byte, source, const_targets, frame);
    }
}

/// Track B: whether an `identifier` sits in a position its PARENT guarantees is
/// a value expression. Conservative whitelist — anything unlisted (patterns,
/// the `const_item` declaration name and its direct-identifier value,
/// `scoped_identifier` path segments, type positions, macro `token_tree`s,
/// attribute contents, `shorthand_field_initializer` keys, …) is NOT counted as
/// a value use (false negative by design). Field checks pin the identifier to
/// the value-carrying field where the parent also holds patterns / conditions /
/// names.
fn rust_const_expression_position(node: &Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_field = |field: &str| {
        parent
            .child_by_field_name(field)
            .is_some_and(|child| child.id() == node.id())
    };
    match parent.kind() {
        // Positions where EVERY identifier child is a value expression. A
        // `block`'s direct identifier child is its tail expression (statements
        // are wrapped in `expression_statement`).
        "arguments"
        | "binary_expression"
        | "unary_expression"
        | "parenthesized_expression"
        | "reference_expression"
        | "index_expression"
        | "return_expression"
        | "array_expression"
        | "tuple_expression"
        | "range_expression"
        | "await_expression"
        | "expression_statement"
        | "block" => true,
        // Value-carrying fields on parents that also hold patterns/names. (A
        // `match_arm`'s value is an expression; its own pattern's shadow only
        // activates at the pattern's END byte, so an unshadowed arm value still
        // qualifies while a same-arm binding suppresses it.)
        "field_expression" => is_field("value"),
        "let_declaration" | "let_condition" | "match_expression" | "for_expression"
        | "match_arm" => is_field("value"),
        "if_expression" | "while_expression" => is_field("condition"),
        "assignment_expression" | "compound_assignment_expr" => is_field("right"),
        "field_initializer" => is_field("value"),
        "closure_expression" => is_field("body"),
        _ => false,
    }
}

// ---- M5.1 receiver-type dispatch (extraction side) --------------------------

/// True for class-like symbol kinds whose scope `self`/`this` resolves to, and
/// whose qualified name a method candidate's parent must match (M5.1).
fn is_class_like_symbol(symbol_type: SymbolType) -> bool {
    matches!(
        symbol_type,
        SymbolType::Class
            | SymbolType::Struct
            | SymbolType::Interface
            | SymbolType::Trait
            | SymbolType::Enum
            | SymbolType::Impl
    )
}

/// The receiver (object) node of a method/attribute call, if any. `obj.m(...)`
/// yields the `obj` node; a bare `m(...)` / macro invocation yields `None`.
fn call_receiver_node<'tree>(call_node: &Node<'tree>, _language: Language) -> Option<Node<'tree>> {
    let callee = call_node.child_by_field_name("function")?;
    match callee.kind() {
        // Python `obj.method(...)`
        "attribute" => callee.child_by_field_name("object"),
        // TS/JS `obj.method(...)`
        "member_expression" | "member_access_expression" => callee.child_by_field_name("object"),
        // Rust `value.method(...)`
        "field_expression" => callee.child_by_field_name("value"),
        _ => None,
    }
}

/// M5.1b: whether a call's receiver node is a literal `self`/`this` (the only
/// provenance the GLOBAL miner trusts — its type is the EXACT enclosing class).
/// Determined the same way `eval_receiver_type_rep` does: by the receiver TEXT
/// (Python `self` is an `identifier`; Rust `self` / TS `this` are dedicated kinds).
fn receiver_is_self(receiver: &Node, source: &str) -> bool {
    matches!(
        receiver.kind(),
        "identifier" | "self" | "this" | "shorthand_property_identifier"
    ) && matches!(receiver.safe_text(source), Ok("self") | Ok("this"))
}

/// Evaluate a receiver node to a `TypeRep`: `self`/`this` → the nearest enclosing
/// class qualified name; a bare identifier → its recorded local-variable type;
/// anything else → `Unknown` (the resolver then behaves exactly as today).
fn eval_receiver_type_rep(
    receiver: &Node,
    source: &str,
    language: Language,
    state: &WalkState,
) -> TypeRep {
    let text = match receiver.kind() {
        "identifier" | "self" | "this" | "shorthand_property_identifier" => {
            receiver.safe_text(source).ok()
        }
        _ => None,
    };
    let Some(text) = text else {
        return TypeRep::Unknown;
    };
    if text == "self" || text == "this" {
        // Minor fix: a `this` whose nearest enclosing function is a nested NON-arrow
        // `function` is rebound at runtime — it is NOT the class instance. Type it
        // `Unknown` rather than guessing the class.
        if text == "this" && is_ts_family(language) && this_rebound_by_plain_function(receiver) {
            return TypeRep::Unknown;
        }
        return nearest_type_qn(&state.scope)
            .map(TypeRep::Named)
            .unwrap_or(TypeRep::Unknown);
    }
    lookup_var_type(&state.var_types, text)
}

/// Whether `language` is a TS/JS-family grammar (shared `new`/`this`/param shapes).
fn is_ts_family(language: Language) -> bool {
    matches!(
        language,
        Language::TypeScript
            | Language::Tsx
            | Language::Astro
            | Language::JavaScript
            | Language::Jsx
    )
}

/// Minor fix (TS/JS): true when a nested NON-arrow `function` sits between `this`
/// and its enclosing class. Such a `function` rebinds `this` at call time, so the
/// receiver is not the class instance. Arrow functions are transparent to `this`
/// (they capture the lexical `this`), so we walk through them; a `method_definition`
/// or the class boundary means `this` IS the class.
fn this_rebound_by_plain_function(receiver: &Node) -> bool {
    let mut cur = receiver.parent();
    while let Some(n) = cur {
        match n.kind() {
            // Transparent to `this` — keep looking outward.
            "arrow_function" => {}
            // A plain function rebinds `this`.
            "function_declaration"
            | "function_expression"
            | "generator_function_declaration"
            | "generator_function"
            | "function" => return true,
            // The method/accessor/constructor whose `this` IS the class instance.
            "method_definition" => return false,
            // Reached the class / file with no intervening plain function.
            "class_declaration" | "class" | "class_body" | "program" | "module" => {
                return false;
            }
            _ => {}
        }
        cur = n.parent();
    }
    false
}

/// Qualified name of the nearest enclosing class-like scope (`self`/`this`).
fn nearest_type_qn(scope: &[Scope]) -> Option<String> {
    scope
        .iter()
        .rev()
        .find(|s| s.is_type)
        .map(|s| s.child_qn.to_string())
}

/// Resolve a variable name through the scope-stacked type frames (inner→outer).
/// Stops at the FIRST frame that mentions the name — including an `Unknown` shadow
/// marker (FIX 3) — so an inner binding hides an outer same-named type.
fn lookup_var_type(var_types: &[VarFrame], name: &str) -> TypeRep {
    for frame in var_types.iter().rev() {
        if let Some(t) = frame.types.get(name) {
            return t.clone();
        }
    }
    TypeRep::Unknown
}

/// Base type NAME of a type-annotation node (`: Foo` / `Foo` / `pkg.Foo`), via the
/// shared `collect_type_names` + `clean_type_name` normalizers. The first clean
/// identifier leaf wins (`Optional[Foo]` keeps `Foo` only when unwrapped — here we
/// take the outermost concrete name).
fn type_node_base_name(type_node: &Node, language: Language, source: &str) -> Option<String> {
    let mut raw = Vec::new();
    collect_type_names(type_node, language, source, &mut raw);
    raw.into_iter().find_map(|r| clean_type_name(&r))
}

/// Base type NAME bound by a constructor-style initializer, per language:
/// Python `Foo(...)` / `pkg.Foo(...)`; TS/JS `new Foo(...)`; Rust `Foo::new(...)`
/// or `Foo { .. }`. `None` if the initializer is not a recognized constructor.
///
/// FIX 4 (class-gating): `Foo(...)` is syntactically identical to a FACTORY call
/// (`def Foo(): return Bar()`), and a Rust `Foo::method(...)` assoc fn may return a
/// type other than `Foo`. So the CALL forms are trusted ONLY when `Foo` is a known
/// class/struct name (`class_names`). The unambiguous forms — TS `new Foo()` and a
/// Rust `Foo { .. }` struct literal — need no gating.
fn constructor_type_name(
    value_node: &Node,
    language: Language,
    source: &str,
    class_names: &HashSet<String>,
) -> Option<String> {
    match language {
        Language::Python => {
            if value_node.kind() != "call" {
                return None;
            }
            let func = value_node.child_by_field_name("function")?;
            let name = node_clean_type_name(&func, source)?;
            // Only a KNOWN class — a factory function `Widget()` is NOT a ctor.
            class_names.contains(&name).then_some(name)
        }
        Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx => {
            if value_node.kind() != "new_expression" {
                return None;
            }
            // `new Foo()` is unambiguously a construction — no gating needed.
            let ctor = value_node.child_by_field_name("constructor")?;
            node_clean_type_name(&ctor, source)
        }
        Language::Rust => match value_node.kind() {
            "call_expression" => {
                let func = value_node.child_by_field_name("function")?;
                // `Foo::new(...)` → the path before the final `::`. Assoc fns may
                // return another type (`Foo::from`, `Regex::new` → `Result`), so
                // trust the path only when `Foo` is a known struct/enum.
                if func.kind() == "scoped_identifier" {
                    let path = func.child_by_field_name("path")?;
                    let name = node_clean_type_name(&path, source)?;
                    return class_names.contains(&name).then_some(name);
                }
                None
            }
            // `Foo { .. }` literal is unambiguously a construction — no gating.
            "struct_expression" => {
                let name = value_node.child_by_field_name("name")?;
                node_clean_type_name(&name, source)
            }
            _ => None,
        },
        _ => None,
    }
}

/// The cleaned base type name of a node's source text (`pkg.Foo`/`a::B` → `Foo`/`B`).
fn node_clean_type_name(node: &Node, source: &str) -> Option<String> {
    node.safe_text(source).ok().and_then(clean_type_name)
}

/// Record params + constructor-bound locals (and Unknown-clearing rebinds) for a
/// Python `node` into the current frame. See FIX 1–4 in the module notes.
fn python_record_var_types(
    node: &Node,
    source: &str,
    frame: &mut VarFrame,
    class_names: &HashSet<String>,
) {
    match node.kind() {
        // A function (or lambda) introduces its parameters into the inner frame.
        "function_definition" | "lambda" => record_params(node, Language::Python, source, frame),
        "assignment" => {
            let Some(left) = node.child_by_field_name("left") else {
                return;
            };
            // Tuple/list unpack target (`a, b = ...`) → each name is rebound to an
            // Unknown we cannot type (FIX 1); never leave a stale type.
            if left.kind() != "identifier" {
                bind_pattern_unknown(&left, source, frame);
                return;
            }
            let Ok(var_name) = left.safe_text(source) else {
                return;
            };
            // Annotated `x: Foo = ...` / `x: Foo`.
            if let Some(type_node) = node.child_by_field_name("type") {
                if let Some(t) = type_node_base_name(&type_node, Language::Python, source) {
                    frame.bind(var_name, TypeRep::Named(t));
                    return;
                }
            }
            // Constructor `x = Foo(...)` (gated to a known class, FIX 4), else an
            // unrecognized RHS → Unknown, which still OVERWRITES any stale type
            // (FIX 1) and is what `bind` does.
            let rep = node
                .child_by_field_name("right")
                .and_then(|right| {
                    constructor_type_name(&right, Language::Python, source, class_names)
                })
                .map(TypeRep::Named)
                .unwrap_or(TypeRep::Unknown);
            frame.bind(var_name, rep);
        }
        // Loop / comprehension targets rebind their names to a value we cannot type
        // (FIX 1): `for x in ...` / `[.. for x in ..]`.
        "for_statement" | "for_in_clause" => {
            if let Some(target) = node.child_by_field_name("left") {
                bind_pattern_unknown(&target, source, frame);
            }
        }
        // `with ... as y` and `except E as y` (both `as_pattern`) bind `y` to an
        // untypable value (FIX 1).
        "as_pattern" => {
            if let Some(alias) = node.child_by_field_name("alias") {
                bind_pattern_unknown(&alias, source, frame);
            }
        }
        _ => {}
    }
}

/// Record params + constructor-bound locals (and Unknown-clearing rebinds) for a
/// TS/JS `node`. `new Foo()` is unambiguous so its branch ignores `class_names`.
fn ts_record_var_types(
    node: &Node,
    language: Language,
    source: &str,
    frame: &mut VarFrame,
    class_names: &HashSet<String>,
) {
    match node.kind() {
        "function_declaration"
        | "generator_function_declaration"
        | "function_expression"
        | "arrow_function"
        | "method_definition" => record_params(node, language, source, frame),
        "variable_declarator" => {
            let Some(name) = node.child_by_field_name("name") else {
                return;
            };
            if name.kind() != "identifier" {
                return;
            }
            let Ok(var_name) = name.safe_text(source) else {
                return;
            };
            // Typed `const x: Foo = ...`.
            if let Some(type_node) = node.child_by_field_name("type") {
                if let Some(t) = type_node_base_name(&type_node, language, source) {
                    frame.bind(var_name, TypeRep::Named(t));
                    return;
                }
            }
            // `const x = new Foo()`, else unrecognized RHS → Unknown (FIX 1).
            let rep = node
                .child_by_field_name("value")
                .and_then(|value| constructor_type_name(&value, language, source, class_names))
                .map(TypeRep::Named)
                .unwrap_or(TypeRep::Unknown);
            frame.bind(var_name, rep);
        }
        // Re-assignment without `let`/`const`: `x = new Foo()` retypes, anything
        // else clears the stale type (FIX 1).
        "assignment_expression" => {
            let Some(left) = node.child_by_field_name("left") else {
                return;
            };
            if left.kind() != "identifier" {
                return;
            }
            let Ok(var_name) = left.safe_text(source) else {
                return;
            };
            let rep = node
                .child_by_field_name("right")
                .and_then(|right| constructor_type_name(&right, language, source, class_names))
                .map(TypeRep::Named)
                .unwrap_or(TypeRep::Unknown);
            frame.bind(var_name, rep);
        }
        _ => {}
    }
}

/// Record params + constructor-bound locals (and Unknown-clearing rebinds) for a
/// Rust `node`. `Foo::new()` is gated to a known struct/enum (FIX 4).
fn rust_record_var_types(
    node: &Node,
    source: &str,
    frame: &mut VarFrame,
    class_names: &HashSet<String>,
) {
    match node.kind() {
        "function_item" => record_params(node, Language::Rust, source, frame),
        "let_declaration" => {
            let Some(pattern) = node.child_by_field_name("pattern") else {
                return;
            };
            if pattern.kind() != "identifier" {
                return;
            }
            let Ok(var_name) = pattern.safe_text(source) else {
                return;
            };
            // Typed `let x: Foo = ...`.
            if let Some(type_node) = node.child_by_field_name("type") {
                if let Some(t) = type_node_base_name(&type_node, Language::Rust, source) {
                    frame.bind(var_name, TypeRep::Named(t));
                    return;
                }
            }
            // `let x = Foo::new()` / `let x = Foo { .. }`, else unrecognized RHS →
            // Unknown (FIX 1).
            let rep = node
                .child_by_field_name("value")
                .and_then(|value| {
                    constructor_type_name(&value, Language::Rust, source, class_names)
                })
                .map(TypeRep::Named)
                .unwrap_or(TypeRep::Unknown);
            frame.bind(var_name, rep);
        }
        // `x = Foo::new()` re-assignment retypes; anything else clears stale (FIX 1).
        "assignment_expression" => {
            let Some(left) = node.child_by_field_name("left") else {
                return;
            };
            if left.kind() != "identifier" {
                return;
            }
            let Ok(var_name) = left.safe_text(source) else {
                return;
            };
            let rep = node
                .child_by_field_name("right")
                .and_then(|right| {
                    constructor_type_name(&right, Language::Rust, source, class_names)
                })
                .map(TypeRep::Named)
                .unwrap_or(TypeRep::Unknown);
            frame.bind(var_name, rep);
        }
        _ => {}
    }
}

/// Insert an `Unknown` shadow (FIX 1/FIX 3) for every plain identifier introduced
/// by a binding pattern, descending only through pattern/tuple/list containers —
/// NOT through `attribute`/`subscript` targets (`self.x`, `d[k]`), which rebind a
/// FIELD, not a local name.
fn bind_pattern_unknown(node: &Node, source: &str, frame: &mut VarFrame) {
    match node.kind() {
        "identifier" => {
            if let Ok(name) = node.safe_text(source) {
                frame.shadow(name);
            }
        }
        "pattern_list"
        | "tuple_pattern"
        | "list_pattern"
        | "tuple"
        | "list"
        | "as_pattern_target"
        | "parenthesized_expression" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                bind_pattern_unknown(&child, source, frame);
            }
        }
        _ => {}
    }
}

/// Record an entry in the inner frame for EACH parameter of a function/lambda node
/// (FIX 3): a TYPED param → its base type (`Named`); an UNTYPED param → an `Unknown`
/// SHADOW so a lookup stops at this frame and an outer same-named type cannot leak
/// in. `self`/`this` is handled separately (`nearest_type_qn`); recording it here
/// as `Unknown` is harmless because that path runs before `lookup_var_type`.
fn record_params(func_node: &Node, language: Language, source: &str, frame: &mut VarFrame) {
    let Some(params) = func_node.child_by_field_name("parameters") else {
        return;
    };
    let mut cursor = params.walk();
    for param in params.named_children(&mut cursor) {
        let (name_node, type_node) = param_name_and_type(&param, language);
        let Some(name_node) = name_node else {
            continue;
        };
        if name_node.kind() != "identifier" {
            continue;
        }
        let Ok(var_name) = name_node.safe_text(source) else {
            continue;
        };
        let rep = type_node
            .and_then(|tn| type_node_base_name(&tn, language, source))
            .map(TypeRep::Named)
            .unwrap_or(TypeRep::Unknown);
        frame.bind(var_name, rep);
    }
}

/// The `(name_node, type_node)` of one parameter node, per language. A `None` type
/// means an untyped param (recorded as an `Unknown` shadow by `record_params`).
fn param_name_and_type<'t>(
    param: &Node<'t>,
    language: Language,
) -> (Option<Node<'t>>, Option<Node<'t>>) {
    match language {
        Language::Python => match param.kind() {
            // Bare untyped param: the param node IS the identifier.
            "identifier" => (Some(*param), None),
            "typed_parameter" => (param.named_child(0), param.child_by_field_name("type")),
            "default_parameter" => (param.child_by_field_name("name"), None),
            "typed_default_parameter" => (
                param.child_by_field_name("name"),
                param.child_by_field_name("type"),
            ),
            "list_splat_pattern" | "dictionary_splat_pattern" => (param.named_child(0), None),
            _ => (None, None),
        },
        Language::TypeScript
        | Language::Tsx
        | Language::Astro
        | Language::JavaScript
        | Language::Jsx => match param.kind() {
            "required_parameter" | "optional_parameter" => (
                param.child_by_field_name("pattern"),
                param.child_by_field_name("type"),
            ),
            _ => (None, None),
        },
        Language::Rust => match param.kind() {
            "parameter" => (
                param.child_by_field_name("pattern"),
                param.child_by_field_name("type"),
            ),
            _ => (None, None),
        },
        _ => (None, None),
    }
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
    let s =
        s.trim_start_matches(|c: char| matches!(c, 'r' | 'R' | 'b' | 'B' | 'f' | 'F' | 'u' | 'U'));
    for quote in ["\"\"\"", "'''", "\"", "'"] {
        if let Some(inner) = s
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
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
    jsx_element_kinds: &'static [&'static str],
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
    jsx_element_kinds: &[],
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
    // Track A: PascalCase JSX elements are call observations. The names are
    // absent from the plain-TypeScript grammar, so its compiled bitset is
    // empty and .ts files are unaffected.
    jsx_element_kinds: &["jsx_opening_element", "jsx_self_closing_element"],
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
    jsx_element_kinds: &[],
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
    jsx_element_kinds: &[],
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
    jsx_element_kinds: &[],
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

/// All `LangSpec` concerns compiled to `KindSet`s for one grammar.
struct LangBitsets {
    function: KindSet,
    method: KindSet,
    class: KindSet,
    field: KindSet,
    enum_variant: KindSet,
    call: KindSet,
    macro_call: KindSet,
    jsx_element: KindSet,
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
            jsx_element: KindSet::build(grammar, spec.jsx_element_kinds),
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
            } else if bits.jsx_element.contains(kind_id) {
                // Track A: treat PascalCase JSX component elements as call
                // observations.  Native/lowercase tags (`nav`, `button`) and
                // lowercase namespace roots (`motion.div`) are excluded.
                // Grammars without JSX (plain TypeScript) compile an empty set.
                let name_node = node.child_by_field_name("name")?;
                extract_jsx_component_name(&name_node, source)
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
        "identifier" | "property_identifier" | "field_identifier" | "type_identifier" => {
            node.safe_text(source).ok().map(|s| s.to_string())
        }
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

/// Qualified Rust call observation: the terminal callable name and the
/// normalized qualifier segments before it, plus the call form.
///
/// For `SymbolStore::new()` → terminal `new`, qualifier `["SymbolStore"]`,
/// form `associated`. For `Self::open()` → terminal `open`, qualifier
/// `["Self"]`, form `self_path`. For `crate::store::Store::new()` →
/// terminal `new`, qualifier `["crate", "store", "Store"]`, form
/// `crate_path`. For `value.method()` → terminal `method`, no qualifier,
/// form `receiver` (kept in the receiver-method lane, not reinterpreted as
/// an associated path). For a bare `new()` → terminal `new`, no qualifier,
/// form `bare`.
///
/// Generic arguments and turbofish are stripped from the qualifier segments:
/// `Type::<T>::new()` → terminal `new`, qualifier `["Type"]`, form
/// `associated`. UFCS `<Type as Trait>::make()` → terminal `make`, qualifier
/// `["Type", "Trait"]`, form `ufcs` (the `as` keyword is dropped, and the
/// type and trait are both retained in order).
///
/// Returns `None` when the node is not a recognized Rust call target.
pub(crate) struct QualifiedRustCall {
    pub terminal: String,
    pub qualifier: Vec<String>,
    pub call_form: String,
    /// Byte offset of the call target (the `function` field start byte),
    /// not the entire call expression. Used for same-line dedup.
    pub byte_offset: u32,
}

/// Extract a qualified Rust call observation from a `call_expression` or
/// `macro_invocation` node. Returns `None` if the node is not a Rust call or
/// the function/macro child cannot be parsed.
///
/// This is the entry point from `process_call_relationship`: it resolves the
/// `function` field of a `call_expression` (or `macro` field of a
/// `macro_invocation`) and delegates to `extract_qualified_rust_call`.
fn extract_rust_qualified_call_from_node(node: &Node, source: &str) -> Option<QualifiedRustCall> {
    if node.is_error() || node.is_missing() || node.has_error() {
        return None;
    }
    match node.kind() {
        "call_expression" => {
            let callee = node.child_by_field_name("function")?;
            if callee.is_error() || callee.is_missing() || callee.has_error() {
                return None;
            }
            extract_qualified_rust_call(&callee, source)
        }
        "macro_invocation" => {
            // Preserve the existing terminal-name macro observation without
            // treating a qualified macro path as a resolvable Rust call path.
            // Macro expansion and qualified macro resolution are explicitly
            // outside the advertised Rust call-resolution subset.
            let macro_node = node.child_by_field_name("macro")?;
            let mut call = extract_qualified_rust_call(&macro_node, source)?;
            call.qualifier.clear();
            call.call_form = call_form::BARE.to_string();
            Some(call)
        }
        _ => None,
    }
}

/// Extract a qualified Rust call observation from the `function` child of a
/// `call_expression` node (or the `macro` child of a `macro_invocation`).
///
/// The terminal name is always the last segment. The qualifier is the ordered
/// list of segments before it. Keywords `crate`, `self`, `super`, and `Self`
/// are retained. Raw identifiers (`r#name`) are normalized to `name`.
fn extract_qualified_rust_call(node: &Node, source: &str) -> Option<QualifiedRustCall> {
    let byte_offset = node.start_byte() as u32;
    match node.kind() {
        // Bare call: `new()` — identifier is the function itself.
        "identifier" | "field_identifier" | "raw_identifier" => {
            let name = normalize_rust_identifier(&node.safe_text(source).ok()?)?;
            Some(QualifiedRustCall {
                terminal: name,
                qualifier: Vec::new(),
                call_form: call_form::BARE.to_string(),
                byte_offset,
            })
        }

        // Associated path: `Type::method()`, `Self::method()`,
        // `crate::a::b::Type::method()`, `self::a::Type::method()`,
        // `super::a::Type::method()`.
        // tree-sitter-rust represents these as `scoped_identifier` with
        // a `path` field (the left side) and a `name` field (the terminal).
        "scoped_identifier" | "qualified_identifier" => {
            let terminal = normalize_rust_identifier(
                &node.child_by_field_name("name")?.safe_text(source).ok()?,
            )?;
            let path = node.child_by_field_name("path")?;
            let mut segments = Vec::new();
            let form = if path.kind() == "bracketed_type" {
                let qualified = path
                    .named_child(0)
                    .filter(|child| child.kind() == "qualified_type")?;
                let concrete = qualified.child_by_field_name("type")?;
                let trait_alias = qualified.child_by_field_name("alias")?;
                collect_scoped_path_segments(&concrete, source, &mut segments);
                collect_scoped_path_segments(&trait_alias, source, &mut segments);
                if segments.len() < 2 {
                    return None;
                }
                call_form::UFCS.to_string()
            } else {
                collect_scoped_path_segments(&path, source, &mut segments);
                classify_rust_call_form(&segments)
            };
            Some(QualifiedRustCall {
                terminal,
                qualifier: segments,
                call_form: form,
                byte_offset,
            })
        }

        // Receiver call: `value.method()` — kept in the receiver lane.
        // We extract the terminal method name but set form to `receiver` and
        // produce no qualifier (the receiver is handled by recv_type).
        "field_expression" => {
            let field = node.child_by_field_name("field")?;
            let terminal = normalize_rust_identifier(&field.safe_text(source).ok()?)?;
            Some(QualifiedRustCall {
                terminal,
                qualifier: Vec::new(),
                call_form: call_form::RECEIVER.to_string(),
                byte_offset,
            })
        }

        // UFCS: `<Type as Trait>::make()` — tree-sitter-rust uses
        // `generic_type` or `qualified_type` with a `as`-style path.
        // The call_expression's function field may be a `scoped_identifier`
        // whose path is a `bracketed_type` or similar. We try to extract the
        // type and trait from the angle-bracketed form.
        _ => {
            // Fallback: try extracting from scoped_identifier pattern in case
            // the grammar wraps it differently.
            None
        }
    }
}

/// Classify the call form from the qualifier segments.
fn classify_rust_call_form(segments: &[String]) -> String {
    if segments.is_empty() {
        return call_form::BARE.to_string();
    }
    match segments[0].as_str() {
        "Self" => call_form::SELF_PATH.to_string(),
        "crate" => call_form::CRATE_PATH.to_string(),
        "self" | "super" => call_form::MODULE_PATH.to_string(),
        _ => call_form::ASSOCIATED.to_string(),
    }
}

/// Extract a leading `crate`/`self`/`super`/`Self` keyword from a path
/// segment when tree-sitter-rust does not emit it as a named child.
/// Returns the keyword (`crate`, `self`, `super`, or `Self`) if the text
/// starts with it followed by `::`, otherwise `None`.
fn extract_path_keyword_prefix(text: &str) -> Option<&'static str> {
    for keyword in &["crate", "self", "super", "Self"] {
        let prefix = format!("{}::", keyword);
        if text.starts_with(&prefix) {
            return Some(keyword);
        }
    }
    None
}

/// Collect scoped identifier path segments recursively, in source order.
/// For `crate::store::Store`, this produces `["crate", "store", "Store"]`.
/// Generic arguments (`<T>`) and turbofish (`::<T>`) are stripped.
fn collect_scoped_path_segments(node: &Node, source: &str, out: &mut Vec<String>) {
    match node.kind() {
        "identifier" | "field_identifier" | "type_identifier" | "raw_identifier" => {
            if let Ok(text) = node.safe_text(source) {
                if let Some(name) = normalize_rust_identifier(&text) {
                    out.push(name);
                }
            }
        }
        // tree-sitter-rust emits `crate`, `self`, and `super` as keyword
        // nodes (not `identifier`) when they appear as the root of a
        // `scoped_identifier` path. `Self` is a `type_identifier`.
        "crate" => out.push("crate".to_string()),
        "self" => out.push("self".to_string()),
        "super" => out.push("super".to_string()),
        "scoped_identifier" | "scoped_type_identifier" | "qualified_identifier" => {
            if let Some(path) = node.child_by_field_name("path") {
                collect_scoped_path_segments(&path, source, out);
            } else if let Ok(text) = node.safe_text(source) {
                // tree-sitter-rust 0.24.x parses `crate::Config` as a
                // `scoped_identifier` with only a `name` field — the `crate`/
                // `self`/`super` keyword is an anonymous token with no named
                // child. Recover it from the source text prefix.
                if let Some(keyword) = extract_path_keyword_prefix(&text) {
                    out.push(keyword.to_string());
                }
            }
            if let Some(name) = node.child_by_field_name("name") {
                if let Ok(text) = name.safe_text(source) {
                    if let Some(name) = normalize_rust_identifier(&text) {
                        out.push(name);
                    }
                }
            }
        }
        // `self` as a standalone identifier in paths.
        "self_parameter" => {
            out.push("self".to_string());
        }
        // Skip generic_type / type_arguments / turbofish — they carry no
        // lookup-identity segments.
        "generic_type" => {
            if let Some(ty) = node.child_by_field_name("type") {
                collect_scoped_path_segments(&ty, source, out);
            }
        }
        _ => {
            // Best-effort: collect named children that are identifiers.
            let count = node.named_child_count();
            for i in 0..count {
                if let Some(child) = node.named_child(i as u32) {
                    if matches!(
                        child.kind(),
                        "identifier" | "field_identifier" | "type_identifier" | "raw_identifier"
                    ) {
                        if let Ok(text) = child.safe_text(source) {
                            if let Some(name) = normalize_rust_identifier(&text) {
                                out.push(name);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Normalize a Rust identifier: strip raw-identifier prefix (`r#name` → `name`).
/// Returns `None` for empty strings.
fn normalize_rust_identifier(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("r#") {
        if rest.is_empty() {
            return None;
        }
        Some(rest.to_string())
    } else {
        Some(trimmed.to_string())
    }
}

/// Track A: extract a JSX component call target from a `jsx_opening_element`
/// or `jsx_self_closing_element` name node.
///
/// Only PascalCase component names are treated as calls: `<LanguageSwitcher />`,
/// `<RegionSelector></RegionSelector>`, `<Dialog.Trigger />`.
///
/// Native HTML tags (`nav`, `button`) and lowercase namespace roots
/// (`motion.div`) are rejected because their root does not begin with an
/// uppercase letter.
///
/// For member expressions like `<Dialog.Trigger />`, the terminal component
/// name (`Trigger`) is used conservatively as the call target.
fn extract_jsx_component_name(node: &Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "property_identifier" | "type_identifier" => {
            let text = node.safe_text(source).ok()?;
            // Only PascalCase identifiers are component calls.
            if text.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                Some(text.to_string())
            } else {
                None
            }
        }
        "member_expression" | "qualified_identifier" => {
            // `<Dialog.Trigger />` — check the ROOT (object) is PascalCase.
            // If the root is lowercase (e.g. `motion` in `motion.div`), reject
            // the whole expression.  If the root is uppercase, use the terminal
            // (property) name as the call target.
            let object = node.child_by_field_name("object")?;
            let root_text = object.safe_text(source).ok()?;
            if !root_text
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
            {
                return None;
            }
            // Use the terminal property name as the call target.
            let property = node
                .child_by_field_name("property")
                .or_else(|| last_named_child(node))?;
            extract_jsx_component_name(&property, source)
        }
        _ => None,
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
                property.safe_text(source).ok().map(|s| s.to_string())
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
    let text = node.safe_text(source).ok()?;
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
    node.safe_text(source).ok()
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
            return child.safe_text(source).ok().map(|s| s.to_string());
        }
    }
    // Fallback (e.g. an empty string with no content child): strip one matching
    // surrounding quote pair.
    let text = node.safe_text(source).ok()?.trim();
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
        if let Ok(text) = node.safe_text(source) {
            out.insert(text.to_string());
        }
        return;
    }
    if let Some(name) = first_child_of_kind(node, "type_identifier") {
        if let Ok(text) = name.safe_text(source) {
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
            if let Ok(text) = child.safe_text(source) {
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
                        if let Ok(text) = child.safe_text(source) {
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
                if let Ok(text) = node.safe_text(source) {
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
                if let Ok(text) = node.safe_text(source) {
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
    let value = value
        .trim_matches(|c| matches!(c, '[' | ']' | '?' | '(' | ')'))
        .trim();

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
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "f32",
    "f64",
    "bool",
    "char",
    "str",
    "String",
    "Self",
    "Vec",
    "Option",
    "Result",
    "Box",
    "Rc",
    "Arc",
    "Weak",
    "Cell",
    "RefCell",
    "Mutex",
    "RwLock",
    "Cow",
    "Pin",
    "HashMap",
    "HashSet",
    "BTreeMap",
    "BTreeSet",
    "VecDeque",
    "BinaryHeap",
    "LinkedList",
    "Fn",
    "FnMut",
    "FnOnce",
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
            let line_span = frame.range.end.line.saturating_sub(frame.range.start.line);
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
    // M6 CL1 — version-stable across toolchains (was SipHash `DefaultHasher`).
    crate::stable_hash::stable_hash_hex(content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter::TreeSitterParser;

    #[test]
    fn route_detection_is_limited_to_framework_languages() {
        for language in [
            Language::Python,
            Language::TypeScript,
            Language::Tsx,
            Language::Astro,
            Language::JavaScript,
            Language::Jsx,
        ] {
            assert!(supports_route_detection(language));
        }

        for language in [Language::Rust, Language::Go, Language::Cpp] {
            assert!(!supports_route_detection(language));
        }
    }

    #[test]
    fn shared_extraction_facts_preserve_separate_extraction_results() {
        // Each fixture routes a path through a module constant AND reads an env
        // var through a module constant, so BOTH consumers of the shared
        // constants map (route detection, reads_env resolution) are exercised.
        let fixtures = [
            (
                Language::TypeScript,
                "routes.ts",
                r#"const ROOT = "/users";
const KEY = "API_TOKEN";
function showUser() { return process.env[KEY]; }
router.get(ROOT, showUser);"#,
            ),
            (
                Language::Python,
                "routes.py",
                r#"import os

ROOT = "/users/{id}"
KEY = "API_TOKEN"

@app.get(ROOT)
def show_user():
    return os.environ.get(KEY)
"#,
            ),
        ];

        for (language, path, code) in fixtures {
            let mut parser = TreeSitterParser::new().unwrap();
            let tree = parser.parse(code, language).unwrap();
            let separate_symbols = extract_symbols(&tree, code, language, path);
            let separate_relationships =
                extract_symbol_relationships(&tree, code, language, path, &separate_symbols);

            let facts = collect_extraction_facts(&tree, code, language);
            let shared_symbols = extract_symbols_with_facts(&tree, code, language, path, &facts);
            let shared_relationships = extract_symbol_relationships_with_facts(
                &tree,
                code,
                language,
                path,
                &shared_symbols,
                &facts,
            );

            assert!(
                shared_symbols.iter().any(|s| s.symbol_type == SymbolType::Route),
                "{path}: fixture must actually detect a route"
            );
            assert!(
                shared_relationships.iter().any(|r| {
                    r.relationship_type == SymbolRelationshipType::ReadsEnv
                        && r.target_name == "API_TOKEN"
                }),
                "{path}: fixture must resolve the env key through the constant map"
            );
            assert_eq!(shared_symbols, separate_symbols);
            assert_eq!(shared_relationships, separate_relationships);
        }
    }

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
    fn test_typescript_calls_preserve_named_import_provenance() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
import { createStore as useStore, UserCard, type User } from "@/shared/ui";

export function Page() {
    const state = useStore();
    return <UserCard user={state as User} />;
}
"#;
        let tree = parser.parse(code, Language::Tsx).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Tsx, "src/app/page.tsx");
        let relationships = extract_symbol_relationships(
            &tree,
            code,
            Language::Tsx,
            "src/app/page.tsx",
            &symbols,
        );

        let use_store = relationships
            .iter()
            .find(|relationship| {
                relationship.relationship_type == SymbolRelationshipType::Call
                    && relationship.target_name == "useStore"
            })
            .expect("aliased imported call");
        assert_eq!(use_store.import_path.as_deref(), Some("@/shared/ui"));
        assert_eq!(use_store.imported_name.as_deref(), Some("createStore"));

        let user_card = relationships
            .iter()
            .find(|relationship| {
                relationship.relationship_type == SymbolRelationshipType::Call
                    && relationship.target_name == "UserCard"
            })
            .expect("imported JSX component call");
        assert_eq!(user_card.import_path.as_deref(), Some("@/shared/ui"));
        assert_eq!(user_card.imported_name.as_deref(), Some("UserCard"));
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
        let py =
            "def getenv(name):\n    return name\ndef load():\n    return getenv(\"NOT_ENV\")\n";
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
        let rust_mod =
            "const KEY: &str = \"REAL_KEY\";\nfn a() {\n    let _ = std::env::var(KEY);\n}\n";
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
        let go =
            "package main\nfunc Gen[T any, K comparable](a T, c Config) K { var z K; return z }";
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
        assert!(
            !targets.contains(&"Optional".to_string()),
            "got {targets:?}"
        );
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

    // ---- M4.4 route detection -------------------------------------------

    /// The sorted `Route` symbol qualified-names extracted from `code`.
    fn route_nodes(code: &str, language: Language, path: &str) -> Vec<String> {
        let mut parser = TreeSitterParser::new().unwrap();
        let tree = parser.parse(code, language).unwrap();
        let symbols = extract_symbols(&tree, code, language, path);
        let mut names: Vec<String> = symbols
            .iter()
            .filter(|s| s.symbol_type == SymbolType::Route)
            .map(|s| s.qualified_name.clone())
            .collect();
        names.sort();
        names
    }

    /// The sorted `(route_qualified_name, handler_name)` `Handles` edges from
    /// `code`, with the Route source id resolved back to its qualified name.
    fn handles_edges(code: &str, language: Language, path: &str) -> Vec<(String, String)> {
        let mut parser = TreeSitterParser::new().unwrap();
        let tree = parser.parse(code, language).unwrap();
        let symbols = extract_symbols(&tree, code, language, path);
        let relationships = extract_symbol_relationships(&tree, code, language, path, &symbols);
        let id_to_qn: HashMap<&str, &str> = symbols
            .iter()
            .map(|s| (s.id.as_str(), s.qualified_name.as_str()))
            .collect();
        let mut edges: Vec<(String, String)> = relationships
            .iter()
            .filter(|r| r.relationship_type == SymbolRelationshipType::Handles)
            .map(|r| {
                let route = id_to_qn
                    .get(r.source_symbol_id.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| r.source_symbol_id.clone());
                (route, r.target_name.clone())
            })
            .collect();
        edges.sort();
        edges
    }

    #[test]
    fn test_route_canon_path_unifies_param_syntax() {
        // Express `:id`, FastAPI/OpenAPI `{id}`, and Flask `<int:id>` all unify.
        assert_eq!(route_canon_path("/users/:id"), "/users/{}");
        assert_eq!(route_canon_path("/users/{id}"), "/users/{}");
        assert_eq!(route_canon_path("/users/<int:id>"), "/users/{}");
        assert_eq!(route_canon_path("/users/<path:p>"), "/users/{}");
        assert_eq!(route_canon_path("/users/${id}"), "/users/{}");
        // Multiple params + literal-segment preservation.
        assert_eq!(route_canon_path("/a/:x/b/{y}"), "/a/{}/b/{}");
        assert_eq!(route_canon_path("/api/orders/:id"), "/api/orders/{}");
        // Trailing slash stripped, except the bare root.
        assert_eq!(route_canon_path("/users/"), "/users");
        assert_eq!(route_canon_path("/"), "/");
        assert_eq!(route_canon_path(""), "/");
        // A stray non-segment-start `:` is left literal (not a param).
        assert_eq!(route_canon_path("/a:b"), "/a:b");
    }

    #[test]
    fn test_route_flask_emits_route_and_handles() {
        // `@app.route(..., methods=["GET","POST"])` yields one Route per method,
        // both handled by the decorated function.
        let code = "import flask\napp = flask.Flask(__name__)\n\n@app.route(\"/users/<int:id>\", methods=[\"GET\", \"POST\"])\ndef get_user(id):\n    return id\n";
        assert_eq!(
            route_nodes(code, Language::Python, "app.py"),
            vec!["GET /users/{}".to_string(), "POST /users/{}".to_string()]
        );
        assert_eq!(
            handles_edges(code, Language::Python, "app.py"),
            vec![
                ("GET /users/{}".to_string(), "get_user".to_string()),
                ("POST /users/{}".to_string(), "get_user".to_string()),
            ]
        );
    }

    #[test]
    fn test_route_fastapi_verb_decorator_emits_route() {
        // FastAPI/Flask `@router.post("/items")` — method from the decorator verb.
        let code = "@router.post(\"/items\")\ndef create_item():\n    pass\n";
        assert_eq!(
            route_nodes(code, Language::Python, "api.py"),
            vec!["POST /items".to_string()]
        );
        assert_eq!(
            handles_edges(code, Language::Python, "api.py"),
            vec![("POST /items".to_string(), "create_item".to_string())]
        );
    }

    #[test]
    fn test_route_nest_emits_route_and_handles() {
        // `@Controller("users")` base + `@Get(":id")` method → `GET /users/{}`.
        let code = "@Controller(\"users\")\nexport class UsersController {\n    @Get(\":id\")\n    findOne(id: string) {\n        return id;\n    }\n\n    @Post()\n    create() {}\n}\n";
        assert_eq!(
            route_nodes(code, Language::TypeScript, "users.controller.ts"),
            vec!["GET /users/{}".to_string(), "POST /users".to_string()]
        );
        assert_eq!(
            handles_edges(code, Language::TypeScript, "users.controller.ts"),
            vec![
                ("GET /users/{}".to_string(), "findOne".to_string()),
                ("POST /users".to_string(), "create".to_string()),
            ]
        );
    }

    #[test]
    fn test_route_express_emits_route_and_handles() {
        // `router.post("/api/orders/:id", createOrder)` → Route + Handles to the
        // named handler.
        let code = "const router = express.Router();\nfunction createOrder(req, res) { res.end(); }\nrouter.post(\"/api/orders/:id\", createOrder);\n";
        assert_eq!(
            route_nodes(code, Language::JavaScript, "routes.js"),
            vec!["POST /api/orders/{}".to_string()]
        );
        assert_eq!(
            handles_edges(code, Language::JavaScript, "routes.js"),
            vec![("POST /api/orders/{}".to_string(), "createOrder".to_string())]
        );
    }

    #[test]
    fn test_route_express_inline_handler_is_anchor_only() {
        // An inline anonymous callback has no named symbol → Route node, no edge.
        let code = "app.get(\"/health\", (req, res) => res.send(\"ok\"));\n";
        assert_eq!(
            route_nodes(code, Language::JavaScript, "app.js"),
            vec!["GET /health".to_string()]
        );
        assert!(
            handles_edges(code, Language::JavaScript, "app.js").is_empty(),
            "inline handler must not produce a handles edge"
        );
    }

    #[test]
    fn test_route_generic_map_get_emits_no_route() {
        // Generic `.get()` calls on non-app/router receivers must NOT become
        // routes — false routes are worse than missed ones.
        let code = "const m = new Map();\nfunction f(key) {\n    const a = m.get(key);\n    const b = obj.get();\n    const c = cache.get(\"/looks/like/path\");\n    return a;\n}\n";
        assert!(
            route_nodes(code, Language::JavaScript, "m.js").is_empty(),
            "no routes expected from generic .get()"
        );
        assert!(
            handles_edges(code, Language::JavaScript, "m.js").is_empty(),
            "no handles expected from generic .get()"
        );
    }

    #[test]
    fn test_route_nest_requires_controller() {
        // A plain class with a stray `@Get` but NO `@Controller` is not promoted.
        let code = "export class NotAController {\n    @Get(\":id\")\n    findOne(id: string) {\n        return id;\n    }\n}\n";
        assert!(
            route_nodes(code, Language::TypeScript, "x.ts").is_empty(),
            "no @Controller → no routes"
        );
    }

    // ---- Track A: JSX component reference extraction -------------------------

    /// Helper: collect all Call relationships from a TSX source.
    fn tsx_call_targets(code: &str) -> Vec<(String, u32)> {
        let mut parser = TreeSitterParser::new().unwrap();
        let tree = parser.parse(code, Language::Tsx).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Tsx, "test.tsx");
        let rels =
            extract_symbol_relationships(&tree, code, Language::Tsx, "test.tsx", &symbols);
        rels.into_iter()
            .filter(|r| r.relationship_type == SymbolRelationshipType::Call)
            .map(|r| (r.target_name, r.line))
            .collect()
    }

    #[test]
    fn test_jsx_pascalcase_elements_emit_calls() {
        let code = r#"
function Toolbar() {
  return (
    <nav>
      <LanguageSwitcher />
      <RegionSelector></RegionSelector>
      <Dialog.Trigger />
    </nav>
  );
}
"#;
        let calls = tsx_call_targets(code);

        // PascalCase components emit calls.
        assert!(
            calls.iter().any(|(name, _)| name == "LanguageSwitcher"),
            "LanguageSwitcher should emit a call, got: {calls:?}"
        );
        assert!(
            calls.iter().any(|(name, _)| name == "RegionSelector"),
            "RegionSelector should emit a call, got: {calls:?}"
        );
        // Member expression: use terminal component name.
        assert!(
            calls.iter().any(|(name, _)| name == "Trigger"),
            "Dialog.Trigger should emit a call to Trigger, got: {calls:?}"
        );
    }

    #[test]
    fn test_jsx_native_and_lowercase_elements_emit_no_calls() {
        let code = r#"
function Toolbar() {
  return (
    <nav>
      <button>Menu</button>
      <motion.div />
    </nav>
  );
}
"#;
        let calls = tsx_call_targets(code);

        // Native HTML tags must NOT emit calls.
        assert!(
            !calls.iter().any(|(name, _)| name == "nav"),
            "native <nav> must not emit a call, got: {calls:?}"
        );
        assert!(
            !calls.iter().any(|(name, _)| name == "button"),
            "native <button> must not emit a call, got: {calls:?}"
        );
        // Lowercase namespace root must NOT emit calls.
        assert!(
            !calls.iter().any(|(name, _)| name == "div" || name == "motion"),
            "lowercase <motion.div> must not emit a call, got: {calls:?}"
        );
    }

    // ---- Track B: Rust file-local constant usage -----------------------------

    /// Helper: extract a Rust source's symbols and its `usage` relationships.
    fn rust_const_usage_edges(code: &str) -> (Vec<Symbol>, Vec<SymbolRelationship>) {
        let mut parser = TreeSitterParser::new().unwrap();
        let tree = parser.parse(code, Language::Rust).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Rust, "consts.rs");
        let relationships =
            extract_symbol_relationships(&tree, code, Language::Rust, "consts.rs", &symbols);
        let usages = relationships
            .into_iter()
            .filter(|r| r.relationship_type == SymbolRelationshipType::Usage)
            .collect();
        (symbols, usages)
    }

    /// The `(source_qualified_name, target_name, line)` view of usage edges.
    fn usage_triples(
        symbols: &[Symbol],
        usages: &[SymbolRelationship],
    ) -> Vec<(String, String, u32)> {
        let id_to_qn: HashMap<&str, &str> = symbols
            .iter()
            .map(|s| (s.id.as_str(), s.qualified_name.as_str()))
            .collect();
        let mut triples: Vec<(String, String, u32)> = usages
            .iter()
            .map(|r| {
                let source = id_to_qn
                    .get(r.source_symbol_id.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| r.source_symbol_id.clone());
                (source, r.target_name.clone(), r.line)
            })
            .collect();
        triples.sort();
        triples
    }

    #[test]
    fn test_rust_const_usage_and_let_shadowing() {
        // The handoff fixture: a plain consumer resolves; the shadowing `let`'s
        // INITIALIZER still refers to the outer constant; later references in
        // the shadowed scope emit nothing.
        let code = r#"
const WORKFLOW_TEXT: &str = "workflow";

fn server_info() -> &'static str {
    WORKFLOW_TEXT
}

fn shadowing() {
    let WORKFLOW_TEXT = WORKFLOW_TEXT.len();
    consume(WORKFLOW_TEXT);
}
"#;
        let (symbols, usages) = rust_const_usage_edges(code);
        assert_eq!(
            usage_triples(&symbols, &usages),
            vec![
                ("server_info".to_string(), "WORKFLOW_TEXT".to_string(), 4),
                ("shadowing".to_string(), "WORKFLOW_TEXT".to_string(), 8),
            ],
            "expected exactly the tail-expression use and the let-initializer use"
        );
        // Every edge resolves immediately to THIS file's constant symbol.
        let const_id = symbols
            .iter()
            .find(|s| s.name == "WORKFLOW_TEXT" && s.symbol_type == SymbolType::Constant)
            .map(|s| s.id.clone())
            .expect("constant symbol not found");
        for edge in &usages {
            assert_eq!(edge.target_symbol_id.as_deref(), Some(const_id.as_str()));
            assert_eq!(edge.resolution_strategy.as_deref(), Some("file_local_const"));
            assert_eq!(edge.confidence, Some(0.9));
        }
    }

    #[test]
    fn test_rust_const_usage_excludes_binding_shadows() {
        // Parameters, closure params, loop bindings, and match bindings all
        // shadow the constant's name: NO usage edges anywhere in this file.
        let code = r#"
const LIMIT: usize = 8;

fn param_shadow(LIMIT: usize) -> usize {
    LIMIT + 1
}

fn closure_shadow() -> usize {
    let apply = |LIMIT: usize| LIMIT * 2;
    apply(3)
}

fn loop_shadow(values: [usize; 3]) -> usize {
    let mut total = 0;
    for LIMIT in values {
        total += LIMIT;
    }
    total
}

fn match_shadow(input: Option<usize>) -> usize {
    match input {
        Some(LIMIT) => LIMIT,
        None => 0,
    }
}
"#;
        let (symbols, usages) = rust_const_usage_edges(code);
        assert!(
            usages.is_empty(),
            "shadowed bindings must emit no constant-usage edges, got: {:?}",
            usage_triples(&symbols, &usages)
        );
    }

    #[test]
    fn test_rust_const_usage_expression_positions() {
        // Whitelisted expression positions (argument, binary, condition,
        // return) each emit a per-use edge; the declarations themselves and a
        // macro token-tree reference emit nothing.
        let code = r#"
const MAX_RETRIES: u32 = 3;
const GREETING: &str = "hi";

fn caller() -> u32 {
    log(GREETING);
    let doubled = MAX_RETRIES * 2;
    if doubled > MAX_RETRIES {
        return MAX_RETRIES;
    }
    doubled
}

fn macro_user() {
    println!("{}", MAX_RETRIES);
}
"#;
        let (symbols, usages) = rust_const_usage_edges(code);
        assert_eq!(
            usage_triples(&symbols, &usages),
            vec![
                ("caller".to_string(), "GREETING".to_string(), 5),
                ("caller".to_string(), "MAX_RETRIES".to_string(), 6),
                ("caller".to_string(), "MAX_RETRIES".to_string(), 7),
                ("caller".to_string(), "MAX_RETRIES".to_string(), 8),
            ],
            "macro token trees and declaration sites must not emit usage edges"
        );
    }

    #[test]
    fn test_rust_const_usage_ambiguous_and_path_references_drop() {
        // The same constant name in two modules is ambiguous (map drops it),
        // and a `scoped_identifier` path reference is never a whitelisted
        // position — a truthful miss on both counts.
        let code = r#"
mod alpha {
    pub const TIMEOUT: u32 = 1;
}

mod beta {
    pub const TIMEOUT: u32 = 2;
}

fn read_timeout() -> u32 {
    let direct = TIMEOUT;
    alpha::TIMEOUT + direct
}
"#;
        let (symbols, usages) = rust_const_usage_edges(code);
        assert!(
            usages.is_empty(),
            "ambiguous names and path segments must emit nothing, got: {:?}",
            usage_triples(&symbols, &usages)
        );
    }

    #[test]
    fn test_rust_const_usage_match_guard_shadow_suppressed() {
        // `match_arm`'s pattern node SPANS the `if` guard, so shadow activation
        // at the pattern's START byte must suppress the guard reference AND the
        // arm value alike (deliberate false negative). An unshadowed use in
        // another function is the positive control.
        let code = r#"
const LIMIT: usize = 8;

fn check(input: Option<usize>) -> usize {
    match input {
        Some(LIMIT) if LIMIT > 2 => LIMIT,
        _ => 0,
    }
}

fn ok() -> usize {
    LIMIT
}
"#;
        let (symbols, usages) = rust_const_usage_edges(code);
        assert_eq!(
            usage_triples(&symbols, &usages),
            vec![("ok".to_string(), "LIMIT".to_string(), 11)],
            "guard and arm-value references of an arm-bound colliding name must emit nothing"
        );
    }

    #[test]
    fn test_rust_mod_nested_const_is_not_a_file_wide_target() {
        // A const nested in `mod x { }` is NOT lexically visible file-wide by
        // bare name: an outer function's bare reference must emit NO edge (the
        // false-positive case), and a match guard colliding with the nested
        // const's name likewise emits nothing.
        let code = r#"
mod inner {
    pub const RETRY_MAX: u32 = 3;
    pub const CODE: u32 = 1;
}

fn outer() -> u32 {
    RETRY_MAX
}

fn pick(v: Option<u32>) -> u32 {
    match v {
        Some(CODE) if CODE > 0 => CODE,
        _ => 0,
    }
}
"#;
        let (symbols, usages) = rust_const_usage_edges(code);
        assert!(
            usages.is_empty(),
            "mod-nested constants must never be usage targets, got: {:?}",
            usage_triples(&symbols, &usages)
        );
    }

    // ---- Qualified Rust call extraction tests ---------------------------

    /// Helper: extract a Rust source's symbols and relationships.
    fn rust_extract(code: &str) -> (Vec<Symbol>, Vec<SymbolRelationship>) {
        let mut parser = TreeSitterParser::new().unwrap();
        let tree = parser.parse(code, Language::Rust).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Rust, "lib.rs");
        let relationships =
            extract_symbol_relationships(&tree, code, Language::Rust, "lib.rs", &symbols);
        (symbols, relationships)
    }

    /// Helper: find a call relationship by terminal name.
    fn rust_call_rel<'a>(
        rels: &'a [SymbolRelationship],
        terminal: &str,
    ) -> &'a SymbolRelationship {
        rels.iter()
            .find(|r| {
                r.relationship_type == SymbolRelationshipType::Call && r.target_name == terminal
            })
            .unwrap_or_else(|| panic!("no Call relationship with terminal `{terminal}`"))
    }

    #[test]
    fn test_rust_bare_call_form_and_no_qualifier() {
        let code = "fn foo() {}\nfn caller() { foo() }\n";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "foo");
        assert_eq!(rel.call_form.as_deref(), Some(call_form::BARE));
        assert!(rel.qualifier_segments.is_none());
        assert!(rel.byte_offset.is_some(), "bare call must have byte_offset");
    }

    #[test]
    fn test_rust_associated_call_form_and_qualifier() {
        let code = "\
struct Store;
impl Store { fn new() -> Store { Store } }
fn caller() { Store::new() }
";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "new");
        assert_eq!(rel.call_form.as_deref(), Some(call_form::ASSOCIATED));
        assert_eq!(
            rel.qualifier_segments.as_deref(),
            Some(&["Store".to_string()][..]),
        );
        assert!(rel.byte_offset.is_some());
    }

    #[test]
    fn test_rust_self_path_call_form() {
        let code = "\
struct Store;
impl Store {
    fn new() -> Store { Store }
    fn open(&self) { Self::new() }
}
";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "new");
        assert_eq!(rel.call_form.as_deref(), Some(call_form::SELF_PATH));
        assert_eq!(
            rel.qualifier_segments.as_deref(),
            Some(&["Self".to_string()][..]),
        );
    }

    #[test]
    fn test_rust_crate_path_call_form() {
        let code = "\
pub struct Config;
impl Config { pub fn load() -> Config { Config } }
fn init() { crate::Config::load() }
";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "load");
        assert_eq!(rel.call_form.as_deref(), Some(call_form::CRATE_PATH));
        assert_eq!(
            rel.qualifier_segments.as_deref(),
            Some(&["crate".to_string(), "Config".to_string()][..]),
        );
    }

    #[test]
    fn test_rust_module_path_self_form() {
        let code = "\
pub struct Store;
impl Store { pub fn open() -> Store { Store } }
fn use_store() { self::Store::open() }
";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "open");
        assert_eq!(rel.call_form.as_deref(), Some(call_form::MODULE_PATH));
        assert_eq!(
            rel.qualifier_segments.as_deref(),
            Some(&["self".to_string(), "Store".to_string()][..]),
        );
    }

    #[test]
    fn test_rust_module_path_super_form() {
        let code = "\
pub struct Store;
impl Store { pub fn open() -> Store { Store } }
mod child {
    use super::Store;
    fn use_store() { super::Store::open() }
}
";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "open");
        assert_eq!(rel.call_form.as_deref(), Some(call_form::MODULE_PATH));
        assert_eq!(
            rel.qualifier_segments.as_deref(),
            Some(&["super".to_string(), "Store".to_string()][..]),
        );
    }

    #[test]
    fn test_rust_receiver_call_form_no_qualifier() {
        let code = "\
struct Store;
impl Store { fn get(&self) -> i32 { 0 } }
fn use_store(s: &Store) { s.get() }
";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "get");
        assert_eq!(rel.call_form.as_deref(), Some(call_form::RECEIVER));
        assert!(rel.qualifier_segments.is_none());
    }

    #[test]
    fn test_rust_turbofish_stripped_from_qualifier() {
        let code = "\
struct Store<T>(T);
impl<T> Store<T> {
    fn new() -> Store<T> { Store(Default::default()) }
}
fn caller() { Store::<u32>::new() }
";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "new");
        // turbofish `::<u32>` must be stripped — qualifier is just `["Store"]`.
        assert_eq!(rel.call_form.as_deref(), Some(call_form::ASSOCIATED));
        let qualifier = rel.qualifier_segments.as_ref().expect("qualifier");
        assert!(
            !qualifier.iter().any(|s| s.contains("u32") || s.contains("<")),
            "turbofish must be stripped from qualifier, got {qualifier:?}"
        );
    }

    #[test]
    fn test_rust_same_line_calls_have_distinct_byte_offsets() {
        let code = "\
struct A; struct B;
impl A { fn new() -> A { A } }
impl B { fn new() -> B { B } }
fn make() { A::new(); B::new() }
";
        let (_symbols, rels) = rust_extract(code);
        let new_calls: Vec<_> = rels
            .iter()
            .filter(|r| {
                r.relationship_type == SymbolRelationshipType::Call && r.target_name == "new"
            })
            .collect();
        assert_eq!(new_calls.len(), 2, "two same-line calls must both be extracted");
        let offsets: Vec<_> = new_calls.iter().filter_map(|r| r.byte_offset).collect();
        assert_eq!(offsets.len(), 2, "both calls must have byte offsets");
        assert_ne!(offsets[0], offsets[1], "same-line calls must have distinct byte offsets");
    }

    #[test]
    fn test_rust_raw_identifier_normalized() {
        let code = "\
struct r#Type;
impl r#Type { fn new() -> r#Type { r#Type } }
fn caller() { r#Type::new() }
";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "new");
        let qualifier = rel.qualifier_segments.as_ref().expect("qualifier");
        assert!(
            !qualifier.iter().any(|s| s.contains("r#")),
            "raw identifier prefix must be normalized, got {qualifier:?}"
        );
        assert!(qualifier.iter().any(|s| s == "Type"), "normalized name must be present");
    }

    #[test]
    fn test_rust_ufcs_call_preserves_type_and_trait() {
        let code = "\
struct Store;
trait Maker { fn make() -> Self; }
impl Maker for Store { fn make() -> Self { Store } }
fn caller() { <Store as Maker>::make(); }
";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "make");
        assert_eq!(rel.call_form.as_deref(), Some(call_form::UFCS));
        assert_eq!(
            rel.qualifier_segments.as_deref(),
            Some(&["Store".to_string(), "Maker".to_string()][..])
        );
    }

    #[test]
    fn test_rust_trait_impl_method_identity_does_not_collapse_inherent_method() {
        let code = "\
struct Store;
trait Maker { fn make() -> Self; }
impl Store { fn make() -> Self { Store } }
impl Maker for Store { fn make() -> Self { Store } }
";
        let (symbols, _) = rust_extract(code);
        let methods = symbols
            .iter()
            .filter(|symbol| symbol.symbol_type == SymbolType::Method && symbol.name == "make")
            .collect::<Vec<_>>();
        assert_eq!(methods.len(), 2, "both impl methods survive");
        assert_eq!(
            methods
                .iter()
                .map(|symbol| symbol.id.as_str())
                .collect::<HashSet<_>>()
                .len(),
            2,
            "trait-impl and inherent method stable IDs must remain distinct"
        );
        assert!(methods
            .iter()
            .any(|symbol| symbol.qualified_name == "Store::make"));
        assert!(methods
            .iter()
            .any(|symbol| symbol.qualified_name == "Store as Maker::make"));
    }

    #[test]
    fn test_qualified_rust_macro_stays_in_bare_lane() {
        let code = "fn caller() { tracing::info!(\"hello\"); }\n";
        let (_symbols, rels) = rust_extract(code);
        let rel = rust_call_rel(&rels, "info");
        assert_eq!(rel.call_form.as_deref(), Some(call_form::BARE));
        assert!(rel.qualifier_segments.is_none());
    }

    #[test]
    fn test_rust_parse_recovery_does_not_manufacture_qualified_call() {
        let code = "fn caller() { Store::::new(); }\n";
        let (_symbols, rels) = rust_extract(code);
        assert!(
            !rels.iter().any(|relationship| {
                relationship.relationship_type == SymbolRelationshipType::Call
                    && relationship
                        .qualifier_segments
                        .as_ref()
                        .is_some_and(|segments| !segments.is_empty())
            }),
            "malformed recovered call paths must not become qualified observations"
        );
    }

    // ---- Track C: Go interface method specs, embedding, receiver kinds ------

    /// Helper: extract a Go source's symbols and relationships.
    fn go_extract(code: &str) -> (Vec<Symbol>, Vec<SymbolRelationship>) {
        let mut parser = TreeSitterParser::new().unwrap();
        let tree = parser.parse(code, Language::Go).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Go, "cache.go");
        let relationships =
            extract_symbol_relationships(&tree, code, Language::Go, "cache.go", &symbols);
        (symbols, relationships)
    }

    #[test]
    fn test_go_interface_method_specs_are_child_symbols_with_signatures() {
        let code = r#"
package cache

import "context"

type Base interface {
	Ping(context.Context) error
}

type Cache interface {
	Base
	Set(string, []byte) error
}

type Memory struct{}

func (Memory) Ping(ctx context.Context) error { return nil }

func (m *Memory) Set(k string, v []byte) error { return nil }
"#;
        let (symbols, _) = go_extract(code);

        fn find_qn<'a>(symbols: &'a [Symbol], qn: &str) -> &'a Symbol {
            symbols
                .iter()
                .find(|s| s.qualified_name == qn)
                .unwrap_or_else(|| panic!("symbol {qn} not found"))
        }

        let base = find_qn(&symbols, "Base");
        assert_eq!(base.symbol_type, SymbolType::Interface);
        let cache = find_qn(&symbols, "Cache");
        assert_eq!(cache.symbol_type, SymbolType::Interface);

        // Interface method SPECS are Method symbols parented to the interface,
        // carrying "(params) result" signature text — the store-side miner's
        // extractor contract.
        let ping_spec = find_qn(&symbols, "Base.Ping");
        assert_eq!(ping_spec.symbol_type, SymbolType::Method);
        assert_eq!(ping_spec.parent_id.as_deref(), Some(base.id.as_str()));
        assert_eq!(ping_spec.signature.as_deref(), Some("(context.Context) error"));

        let set_spec = find_qn(&symbols, "Cache.Set");
        assert_eq!(set_spec.symbol_type, SymbolType::Method);
        assert_eq!(set_spec.parent_id.as_deref(), Some(cache.id.as_str()));
        assert_eq!(set_spec.signature.as_deref(), Some("(string, []byte) error"));

        // Concrete methods keep their signatures too (receiver excluded).
        let ping_impl = find_qn(&symbols, "Ping");
        assert_eq!(ping_impl.symbol_type, SymbolType::Method);
        assert_eq!(
            ping_impl.signature.as_deref(),
            Some("(ctx context.Context) error")
        );
        let set_impl = find_qn(&symbols, "Set");
        assert_eq!(
            set_impl.signature.as_deref(),
            Some("(k string, v []byte) error")
        );
    }

    #[test]
    fn test_go_interface_embedding_emits_extends() {
        let code = r#"
package cache

import "io"

type Base interface {
	Ping() error
}

type Cache interface {
	Base
	Set(string, []byte) error
}

type MyCloser interface {
	io.Closer
}

type Number interface {
	~int | ~int64
}
"#;
        let (symbols, relationships) = go_extract(code);
        let id_of = |name: &str| -> String {
            symbols
                .iter()
                .find(|s| s.name == name && s.symbol_type == SymbolType::Interface)
                .map(|s| s.id.clone())
                .unwrap_or_else(|| panic!("interface {name} not found"))
        };
        let extends: Vec<(String, String)> = relationships
            .iter()
            .filter(|r| r.relationship_type == SymbolRelationshipType::Extends)
            .map(|r| (r.source_symbol_id.clone(), r.target_name.clone()))
            .collect();

        // `Cache` embeds `Base`; a qualified embedding reduces to its terminal
        // name; a generic-constraint union is NOT an embedding.
        assert!(extends.contains(&(id_of("Cache"), "Base".to_string())));
        assert!(extends.contains(&(id_of("MyCloser"), "Closer".to_string())));
        assert!(
            !extends.iter().any(|(source, _)| *source == id_of("Number")),
            "constraint unions must not emit extends edges, got: {extends:?}"
        );
        // Method specs never become embedding targets.
        assert!(
            !extends.iter().any(|(_, target)| target == "Set" || target == "Ping"),
            "method specs must not emit extends edges, got: {extends:?}"
        );
    }

    #[test]
    fn test_go_receiver_kind_metadata_on_contains() {
        let code = r#"
package cache

type Memory struct{}

func (Memory) Ping() error { return nil }

func (m *Memory) Set(k string, v []byte) error { return nil }
"#;
        let (symbols, relationships) = go_extract(code);
        let memory_id = symbols
            .iter()
            .find(|s| s.name == "Memory" && s.symbol_type == SymbolType::Struct)
            .map(|s| s.id.clone())
            .expect("Memory struct not found");
        let contains_kind = |method: &str| -> Option<String> {
            relationships
                .iter()
                .find(|r| {
                    r.relationship_type == SymbolRelationshipType::Contains
                        && r.source_symbol_id == memory_id
                        && r.target_name == method
                })
                .unwrap_or_else(|| panic!("contains edge for {method} not found"))
                .receiver_kind
                .clone()
        };

        assert_eq!(contains_kind("Ping").as_deref(), Some("value"));
        assert_eq!(contains_kind("Set").as_deref(), Some("pointer"));
        // No other edge kind carries a receiver kind.
        assert!(relationships
            .iter()
            .filter(|r| r.relationship_type != SymbolRelationshipType::Contains)
            .all(|r| r.receiver_kind.is_none()));
    }

    #[test]
    fn test_go_generic_receiver_reduces_to_base_type() {
        // Generic receivers (`Pair[K, V]` / `*Pair[K, V]`) must reduce to the
        // base type name so Contains + receiver_kind still emit.
        let code = r#"
package cache

type Pair[K comparable, V any] struct {
	key K
	val V
}

func (p Pair[K, V]) Key() K { return p.key }

func (p *Pair[K, V]) SetVal(v V) { p.val = v }
"#;
        let (symbols, relationships) = go_extract(code);
        let pair_id = symbols
            .iter()
            .find(|s| s.name == "Pair" && s.symbol_type == SymbolType::Struct)
            .map(|s| s.id.clone())
            .expect("Pair struct not found");
        let contains_kind = |method: &str| -> Option<String> {
            relationships
                .iter()
                .find(|r| {
                    r.relationship_type == SymbolRelationshipType::Contains
                        && r.source_symbol_id == pair_id
                        && r.target_name == method
                })
                .unwrap_or_else(|| panic!("contains edge for {method} not found"))
                .receiver_kind
                .clone()
        };
        assert_eq!(contains_kind("Key").as_deref(), Some("value"));
        assert_eq!(contains_kind("SetVal").as_deref(), Some("pointer"));
    }

    #[test]
    fn test_go_generic_embedded_interface_emits_extends() {
        // A generic instantiation embedded in an interface (`Getter[T]`)
        // extends its BASE interface (`Getter`).
        let code = r#"
package cache

type Getter[T any] interface {
	Get() T
}

type Store[T any] interface {
	Getter[T]
	Put(T)
}
"#;
        let (symbols, relationships) = go_extract(code);
        let id_of = |name: &str| -> String {
            symbols
                .iter()
                .find(|s| s.name == name && s.symbol_type == SymbolType::Interface)
                .map(|s| s.id.clone())
                .unwrap_or_else(|| panic!("interface {name} not found"))
        };
        let extends: Vec<(String, String)> = relationships
            .iter()
            .filter(|r| r.relationship_type == SymbolRelationshipType::Extends)
            .map(|r| (r.source_symbol_id.clone(), r.target_name.clone()))
            .collect();
        assert!(
            extends.contains(&(id_of("Store"), "Getter".to_string())),
            "generic embedding must extend its base interface, got: {extends:?}"
        );
    }

    #[test]
    fn test_go_anonymous_interface_literal_mints_no_method_symbols() {
        // `method_elem` only mints a Method symbol under a NAMED interface
        // declaration. Anonymous interface literals in function params and
        // type assertions produce NO symbols at all.
        let code = r#"
package cache

func Wait(s interface{ Done() }) {}

func Cast(v any) bool {
	_, ok := v.(interface{ Close() error })
	return ok
}
"#;
        let (symbols, _) = go_extract(code);
        assert!(
            !symbols.iter().any(|s| s.name == "Done" || s.name == "Close"),
            "anonymous interface literals must mint no Method symbols, got: {:?}",
            symbols
                .iter()
                .map(|s| s.qualified_name.as_str())
                .collect::<Vec<_>>()
        );
        // The enclosing functions themselves are unaffected.
        assert!(symbols
            .iter()
            .any(|s| s.name == "Wait" && s.symbol_type == SymbolType::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Cast" && s.symbol_type == SymbolType::Function));
    }
}
