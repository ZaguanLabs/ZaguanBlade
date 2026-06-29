//! M0.3 — Golden-snapshot fixture harness.
//!
//! Snapshots the FULL extracted `(symbols, relationships)` for a set of fixture
//! files and FAILS on any diff. This is a regression net that captures the
//! *current* behavior of the public tree-sitter extraction API
//! (`zblade_lib::tree_sitter::{extract_symbols, extract_symbol_relationships}`),
//! including current bugs and gaps. It deliberately does NOT fix or change any
//! extraction behavior.
//!
//! Each fixture is read, its `Language` is detected from the path, it is parsed
//! with `TreeSitterParser`, and the extracted symbols/relationships are
//! projected onto a stable, deterministically-ordered view and serialized to
//! pretty JSON. The serialization intentionally omits volatile fields (the UUID
//! `id` and the SipHash `content_hash`) so the golden does not drift across
//! Rust toolchains.
//!
//! Behavior:
//! - `UPDATE_GOLDEN=1 cargo test --test golden_extraction` (re)writes the
//!   golden files from the current extraction.
//! - Plain `cargo test --test golden_extraction` reads each golden and asserts
//!   the freshly-serialized output matches it.
//!
//! NOTE: The "scanner offender" fixtures (java/php/scss/sql/dockerfile/json/
//! yaml) are NOT backed by a tree-sitter grammar, so `TreeSitterParser::parse`
//! returns `UnsupportedLanguage`. For these, the harness instead drives the
//! non-tree-sitter line/regex SCANNER path via the
//! `zblade_lib::language_service::extract_scanner_symbols` entry point (exposed
//! by M1.4) and records `parse_status: "scanner"`. This makes the scanner
//! false-positives those fixtures encode *live* in the golden so M1.4's
//! comment/string/heredoc-aware preprocessing is gated by them. Scanner symbols
//! never carry relationships, so the relationship list is always empty here.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use zblade_lib::language_service::extract_scanner_symbols;
use zblade_lib::tree_sitter::{
    extract_symbol_relationships, extract_symbols, Language, TreeSitterParser,
};

/// Stable projection of a `Symbol` for snapshotting. Field order here is the
/// serialized order (serde emits struct fields in declaration order).
#[derive(Serialize)]
struct SymbolView {
    name: String,
    kind: String,
    qualified_name: String,
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
    parent_qualified_name: Option<String>,
    docstring: Option<String>,
    signature: Option<String>,
}

/// Stable projection of a `SymbolRelationship` for snapshotting. `recv_type`
/// (M5.1) is the receiver type captured for a method/attribute Call edge; it is
/// `skip_serializing_if` None so existing goldens (no receiver typing) stay
/// byte-identical and only receiver-typed calls surface it. `recv_self` (M5.1b)
/// records `self`/`this` provenance — `true` only when `recv_type` is the EXACT
/// enclosing-class qn (the sole signal the GLOBAL miner consumes); it is omitted
/// when false so param/constructor-typed calls stay byte-identical.
#[derive(Serialize)]
struct RelationshipView {
    relationship_type: String,
    source_qualified_name: String,
    target_name: String,
    line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    recv_type: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    recv_self: bool,
}

/// `skip_serializing_if` predicate: omit a `bool` field when false.
fn is_false(value: &bool) -> bool {
    !*value
}

/// Full deterministic snapshot for one fixture case.
#[derive(Serialize)]
struct GoldenSnapshot {
    case: String,
    language: Option<String>,
    parse_status: String,
    symbol_count: usize,
    relationship_count: usize,
    symbols: Vec<SymbolView>,
    relationships: Vec<RelationshipView>,
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden")
}

/// Build the deterministic JSON snapshot string for a fixture case.
fn snapshot_for_case(case: &str, fixture_file: &str) -> String {
    let fixture_path = golden_dir().join(fixture_file);
    let source = fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", fixture_path.display()));

    // Use the bare fixture file name as the (stable) extraction path so language
    // detection works and no absolute path leaks into the snapshot.
    let language = Language::from_path(fixture_file);

    let (parse_status, symbols, relationships) = match language {
        None => ("no_language".to_string(), Vec::new(), Vec::new()),
        Some(lang) => {
            let mut parser = TreeSitterParser::new().expect("tree-sitter parser init failed");
            match parser.parse(&source, lang) {
                Ok(tree) => {
                    let symbols = extract_symbols(&tree, &source, lang, fixture_file);
                    let relationships =
                        extract_symbol_relationships(&tree, &source, lang, fixture_file, &symbols);
                    ("ok".to_string(), symbols, relationships)
                }
                // Scanner-only languages have no tree-sitter grammar; drive the
                // line/regex scanner path directly. Scanners emit no relationships.
                Err(_) => {
                    let symbols = extract_scanner_symbols(fixture_file, &source, lang);
                    ("scanner".to_string(), symbols, Vec::new())
                }
            }
        }
    };

    // Resolve opaque ids to qualified names for readable, stable diffs.
    let id_to_qn: HashMap<&str, &str> = symbols
        .iter()
        .map(|s| (s.id.as_str(), s.qualified_name.as_str()))
        .collect();
    let resolve = |id: &str| -> String {
        id_to_qn
            .get(id)
            .map(|qn| qn.to_string())
            .unwrap_or_else(|| id.to_string())
    };

    let mut symbol_views: Vec<SymbolView> = symbols
        .iter()
        .map(|s| SymbolView {
            name: s.name.clone(),
            kind: s.symbol_type.to_string(),
            qualified_name: s.qualified_name.clone(),
            start_line: s.range.start.line,
            start_col: s.range.start.character,
            end_line: s.range.end.line,
            end_col: s.range.end.character,
            parent_qualified_name: s.parent_id.as_deref().map(&resolve),
            docstring: s.docstring.clone(),
            signature: s.signature.clone(),
        })
        .collect();

    // Sort by (start line, start col, name, kind) — deterministic ordering.
    symbol_views.sort_by(|a, b| {
        (a.start_line, a.start_col, &a.name, &a.kind).cmp(&(
            b.start_line,
            b.start_col,
            &b.name,
            &b.kind,
        ))
    });

    let mut relationship_views: Vec<RelationshipView> = relationships
        .iter()
        .map(|r| RelationshipView {
            relationship_type: r.relationship_type.to_string(),
            source_qualified_name: resolve(&r.source_symbol_id),
            target_name: r.target_name.clone(),
            line: r.line,
            recv_type: r.recv_type.clone(),
            recv_self: r.recv_self,
        })
        .collect();

    // Sort by (source line, type, target name) — deterministic ordering.
    relationship_views.sort_by(|a, b| {
        (a.line, &a.relationship_type, &a.target_name).cmp(&(
            b.line,
            &b.relationship_type,
            &b.target_name,
        ))
    });

    let snapshot = GoldenSnapshot {
        case: case.to_string(),
        language: language.map(|l| format!("{l:?}")),
        parse_status,
        symbol_count: symbol_views.len(),
        relationship_count: relationship_views.len(),
        symbols: symbol_views,
        relationships: relationship_views,
    };

    let mut json = serde_json::to_string_pretty(&snapshot).expect("serialize snapshot");
    json.push('\n');
    json
}

/// Either regenerate the golden (`UPDATE_GOLDEN=1`) or assert against it.
fn check_case(case: &str, fixture_file: &str) {
    let actual = snapshot_for_case(case, fixture_file);
    let golden_path = golden_dir().join(format!("{case}.golden.json"));

    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        fs::write(&golden_path, &actual)
            .unwrap_or_else(|e| panic!("failed to write golden {}: {e}", golden_path.display()));
        return;
    }

    let expected = fs::read_to_string(&golden_path).unwrap_or_else(|e| {
        panic!(
            "missing golden for case '{case}' at {} ({e}). \
             Run `UPDATE_GOLDEN=1 cargo test --test golden_extraction` to generate it.",
            golden_path.display()
        )
    });

    assert_eq!(
        actual, expected,
        "golden extraction mismatch for case '{case}'. \
         If this change is intended, regenerate with \
         `UPDATE_GOLDEN=1 cargo test --test golden_extraction` and review the diff."
    );
}

// ---- Full-grammar cases (real tree-sitter) ----

#[test]
fn golden_rust_basic() {
    check_case("rust-basic", "rust-basic.rs");
}

#[test]
fn golden_typescript_basic() {
    check_case("typescript-basic", "typescript-basic.ts");
}

#[test]
fn golden_javascript_basic() {
    check_case("javascript-basic", "javascript-basic.js");
}

#[test]
fn golden_python_basic() {
    check_case("python-basic", "python-basic.py");
}

#[test]
fn golden_go_basic() {
    check_case("go-basic", "go-basic.go");
}

// M5.3 — C/C++ graduated to the real `tree-sitter-cpp` grammar (definitions-only,
// N8). Exercises macros, typedefs (incl. function-pointer + the named-struct-typedef
// idiom that must collapse to one symbol), struct/union/enum + members, function
// definitions vs prototypes, the skipped variable/usage declarations, namespaces,
// a class with ctor/dtor/inline+declared methods/fields, an enum class, and an
// out-of-line qualified method definition. Relationship list MUST be empty (N8).
#[test]
fn golden_cpp_basic() {
    check_case("cpp-basic", "cpp-basic.cpp");
}

// ---- M5.1 receiver-type dispatch case ----
// Two classes `A` and `B` each define `run`. Inside `A.go`, `self.run()` captures
// recv_type `A` and the constructor-bound `x = B(); x.run()` captures recv_type
// `B` — the inputs that let the resolver disambiguate the otherwise-ambiguous
// `run` call to the CORRECT class method (proven end-to-end in the lib tests).
#[test]
fn golden_python_receiver_type() {
    check_case("python-receiver-type", "python-receiver-type.py");
}

// ---- M5.1b GLOBAL receiver-type mining — extraction inputs ----
// The golden harness snapshots EXTRACTION only (recv_type + edges), not target
// resolution; the actual cross-file/registry mining is proven end-to-end in the
// lib tests (`receiver_global_*` / `null_mining_*`). This fixture documents the
// extraction signals the mine pass consumes: `Derived(Base)` yields an `extends`
// edge Derived→Base, and `self.base_method()` inside `Derived.go` captures
// recv_type `Derived` WITH `recv_self: true` provenance — the ONLY provenance the
// global miner trusts (the recv_type is the EXACT enclosing-class qn, never a
// simple name that could shadow an imported library type). The method is defined
// only on the SUPERTYPE `Base`. If either input regresses (the extends edge, the
// recv_type, or the self provenance) the global mining win silently disappears —
// this golden gates that.
#[test]
fn golden_python_receiver_inherit() {
    check_case("python-receiver-inherit", "python-receiver-inherit.py");
}

// ---- Scanner-offender cases (capture current behavior; see module note) ----

#[test]
fn golden_java_block_comment() {
    check_case("java-block-comment", "java-block-comment.java");
}

#[test]
fn golden_php_trailing_comment() {
    check_case("php-trailing-comment", "php-trailing-comment.php");
}

#[test]
fn golden_scss_brace_in_string() {
    check_case("scss-brace-in-string", "scss-brace-in-string.scss");
}

#[test]
fn golden_sql_multiline_create() {
    check_case("sql-multiline-create", "sql-multiline-create.sql");
}

#[test]
fn golden_dockerfile_continuation() {
    check_case("dockerfile-continuation", "dockerfile-continuation.dockerfile");
}

#[test]
fn golden_json_duplicate_key() {
    check_case("json-duplicate-key", "json-duplicate-key.json");
}

#[test]
fn golden_yaml_duplicate_key() {
    check_case("yaml-duplicate-key", "yaml-duplicate-key.yaml");
}

// ---- M4.3 — K8s Resource nodes + Kustomize IMPORTS ----

#[test]
fn golden_k8s_deployment() {
    check_case("k8s-deployment", "k8s-deployment.yaml");
}

#[test]
fn golden_kustomization() {
    check_case("kustomization", "kustomization.yaml");
}

// Regression: a TOP-LEVEL SEQUENCE root (which `marked-yaml`'s mapping-root
// loader rejects) must still yield config keys via the serde fallback — BUG 1.
#[test]
fn golden_yaml_toplevel_sequence() {
    check_case("yaml-toplevel-sequence", "yaml-toplevel-sequence.yaml");
}

// Regression: a MIXED multi-document file must emit the manifest doc's Resource
// AND the non-manifest doc's flat config keys (server/server.port/…) — BUG 2.
#[test]
fn golden_yaml_mixed_multidoc() {
    check_case("yaml-mixed-multidoc", "yaml-mixed-multidoc.yaml");
}

// ---- M4.4 — Route nodes + canonicalization + HANDLES ----

// Flask: `@app.route("/users/<int:id>", methods=["GET","POST"])` → two Route
// nodes (`GET`/`POST /users/{}`) each with a `handles` edge to `get_user`.
#[test]
fn golden_flask_route() {
    check_case("flask-route", "flask-route.py");
}

// Express: `router.post("/api/orders/:id", handler)` → one Route node
// (`POST /api/orders/{}`) + a `handles` edge to the named `handler` function.
#[test]
fn golden_express_route() {
    check_case("express-route", "express-route.js");
}

// NestJS: `@Controller("users")` + `@Get(":id")` → Route node `GET /users/{}` +
// a `handles` edge to the `findOne` controller method.
#[test]
fn golden_nest_controller() {
    check_case("nest-controller", "nest-controller.ts");
}
