//! Symbol storage using SQLite
//!
//! Persistent storage for extracted code symbols with efficient
//! indexing and retrieval.

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use crate::tree_sitter::{
    Language, Symbol, SymbolRelationship, SymbolRelationshipType, SymbolType,
};

const GENERATED_INDEX_CLEAR_STATEMENTS: &[&str] = &[
    "DELETE FROM symbol_relationships",
    "DELETE FROM semantic_anchors",
    "DELETE FROM symbols",
    "DELETE FROM indexed_files",
];

const GENERATED_INDEX_TABLES: &[&str] = &[
    "symbol_relationships",
    "semantic_anchors",
    "symbols",
    "indexed_files",
];

/// Track D — the symbol kinds each relationship kind may legally resolve to.
///
/// Global-unique name resolution must consider what KIND of target an edge can
/// reference: a `call` may land on a callable, never on a Markdown heading with
/// the same text, and a heading must never make a callable "ambiguous" either.
/// The allowlist is rendered into a temp join table by
/// `build_unique_relationship_targets_sql`, so uniqueness is judged per
/// `(name, kind-class)` instead of per bare name.
///
/// Per-kind classes (a kind absent from a row's list is never a candidate):
///   * `call`       → function, method, class, struct, enum, type — callables
///     plus constructable types (constructor calls in class/struct languages).
///   * `implements` → interface, trait, class — implementable contracts.
///   * `extends`    → class, interface, struct, trait, type — inheritable /
///     embeddable bases.
///   * `uses_type`  → class, struct, interface, type, enum, trait — nominal
///     types a signature/annotation can reference.
///   * `usage`      → constant, variable, property, enum_member, function,
///     css_custom_property — value references (a function used as a value, not
///     called). `css_custom_property` is REQUIRED: `var(--token)` usage edges
///     across stylesheet files resolved under the old kind-blind back-fill,
///     and omitting the kind here would permanently orphan them.
///   * `contains`   → function, method, property, variable, constant,
///     enum_member — members a container binds.
///   * `handles`    → function, method — handler callables.
///
/// Relationship kinds with NO row (`reads_env`, `export`) are EXCLUDED from
/// the back-fill entirely, exactly like `import` already is — their targets
/// are not global symbol names. `heading` appears in NO allowlist: a Markdown
/// heading must never resolve a call/usage (explicit release gate). `import`
/// SYMBOLS are likewise in no allowlist, preserving the old candidate filter.
const RELATIONSHIP_TARGET_KIND_ALLOWLIST: &[(&str, &[&str])] = &[
    (
        "call",
        &["function", "method", "class", "struct", "enum", "type"],
    ),
    ("implements", &["interface", "trait", "class"]),
    (
        "extends",
        &["class", "interface", "struct", "trait", "type"],
    ),
    (
        "uses_type",
        &["class", "struct", "interface", "type", "enum", "trait"],
    ),
    (
        "usage",
        &[
            "constant",
            "variable",
            "property",
            "enum_member",
            "function",
            "css_custom_property",
        ],
    ),
    (
        "contains",
        &[
            "function",
            "method",
            "property",
            "variable",
            "constant",
            "enum_member",
        ],
    ),
    ("handles", &["function", "method"]),
];

/// Render `RELATIONSHIP_TARGET_KIND_ALLOWLIST` as SQL `VALUES` rows for the
/// temp allowlist table. Every entry is a compile-time literal from the const
/// above — nothing user-controlled is interpolated.
fn relationship_kind_allowlist_values_sql() -> String {
    let mut rows = Vec::new();
    for (relationship_type, symbol_types) in RELATIONSHIP_TARGET_KIND_ALLOWLIST {
        for symbol_type in *symbol_types {
            rows.push(format!("('{relationship_type}','{symbol_type}')"));
        }
    }
    rows.join(",")
}

/// M2.4 set-based back-fill, target-driven: resolve each DISTINCT observed
/// `(target_name, relationship_type)` pair once — not once per relationship,
/// and never for symbols nothing requests. The old form evaluated a correlated
/// candidate `SELECT` plus a correlated `COUNT(*)` guard PER unresolved
/// relationship row, so thousands of `calls → get` edges repeated the
/// identical `get` lookup thousands of times (on Scout's Firefox corpus:
/// complete resolution 110.77s → 44.23s from this change alone). Four steps
/// inside one transaction:
///
/// 1. `relationship_kind_allowlist` — the static kind-class table rendered
///    from `RELATIONSHIP_TARGET_KIND_ALLOWLIST`.
/// 2. `wanted_relationship_names` — the distinct `(name, relationship_type)`
///    pairs unresolved rows request.
/// 3. `unique_relationship_targets` — those pairs joined against `symbols`
///    THROUGH the allowlist, keeping ONLY pairs with exactly one candidate of
///    a legal kind.
/// 4. One `UPDATE` from that compact unique-indexed lookup table.
///
/// CORRECTNESS GUARD: a target is written ONLY when the name matches EXACTLY
/// ONE candidate symbol of a kind the relationship may legally reference
/// (`HAVING COUNT(*) = 1` per `(name, relationship_type)`; the allowlist PK
/// guarantees each symbol joins at most once per pair, so the row count IS the
/// candidate count). Candidates exclude the synthetic `__file__` root, and
/// `import` placeholders are excluded because `import` is in no kind class;
/// the old filter's `name != ''` is implied by the equality join against
/// wanted names, which are never empty. An ambiguous pair is simply absent
/// from the lookup table and stays NULL. `import` relationships are skipped,
/// kinds without an allowlist row (`reads_env`, `export`) drop out of the
/// allowlist join, and already-resolved (non-NULL) rows are never overwritten.
/// `MIN(s.id)` is the single candidate the guard admits. `confidence = 0.5`:
/// a name+kind heuristic (no scope analysis) — the uniqueness guard is what
/// makes it safe, not the score.
fn build_unique_relationship_targets_sql() -> String {
    format!(
        r#"
DROP TABLE IF EXISTS temp.relationship_kind_allowlist;
CREATE TEMP TABLE relationship_kind_allowlist (
    relationship_type TEXT NOT NULL,
    symbol_type TEXT NOT NULL,
    PRIMARY KEY (relationship_type, symbol_type)
) WITHOUT ROWID;
INSERT INTO relationship_kind_allowlist (relationship_type, symbol_type)
VALUES {allowlist_rows};

DROP TABLE IF EXISTS temp.wanted_relationship_names;
CREATE TEMP TABLE wanted_relationship_names AS
SELECT DISTINCT target_name AS name, relationship_type AS relationship_type
FROM symbol_relationships
WHERE target_symbol_id IS NULL
  AND relationship_type != 'import'
  AND target_name != ''
  AND NOT (
        relationship_type = 'call'
        AND (
            lower(source_file_path) GLOB '*.ts'
            OR lower(source_file_path) GLOB '*.tsx'
            OR lower(source_file_path) GLOB '*.astro'
            OR lower(source_file_path) GLOB '*.js'
            OR lower(source_file_path) GLOB '*.jsx'
            OR lower(source_file_path) GLOB '*.mjs'
            OR lower(source_file_path) GLOB '*.cjs'
        )
      );
CREATE UNIQUE INDEX wanted_relationship_names_idx
    ON wanted_relationship_names(name, relationship_type);

DROP TABLE IF EXISTS temp.unique_relationship_targets;
CREATE TEMP TABLE unique_relationship_targets AS
SELECT wanted.name AS name,
       wanted.relationship_type AS relationship_type,
       MIN(s.id) AS id
FROM wanted_relationship_names AS wanted
JOIN relationship_kind_allowlist AS allowed
  ON allowed.relationship_type = wanted.relationship_type
JOIN symbols AS s
  ON s.name = wanted.name AND s.symbol_type = allowed.symbol_type
WHERE s.qualified_name != '__file__'
GROUP BY wanted.name, wanted.relationship_type
HAVING COUNT(*) = 1;
CREATE UNIQUE INDEX unique_relationship_targets_idx
    ON unique_relationship_targets(name, relationship_type);
"#,
        allowlist_rows = relationship_kind_allowlist_values_sql()
    )
}

const BACKFILL_GLOBAL_UNIQUE_SQL: &str = r#"
UPDATE symbol_relationships
SET target_symbol_id = (
        SELECT candidate.id
        FROM unique_relationship_targets AS candidate
        WHERE candidate.name = symbol_relationships.target_name
          AND candidate.relationship_type = symbol_relationships.relationship_type
    ),
    resolution_strategy = 'global_unique',
    confidence = 0.5
WHERE target_symbol_id IS NULL
  AND relationship_type != 'import'
  AND target_name != ''
  AND EXISTS (
        SELECT 1
        FROM unique_relationship_targets AS candidate
        WHERE candidate.name = symbol_relationships.target_name
          AND candidate.relationship_type = symbol_relationships.relationship_type
      )
"#;

const DROP_UNIQUE_RELATIONSHIP_TARGETS_SQL: &str = r#"
DROP TABLE relationship_kind_allowlist;
DROP TABLE wanted_relationship_names;
DROP TABLE unique_relationship_targets;
"#;

/// Resolve named ES-module call bindings from their source import provenance.
/// A relationship is updated only when exactly one top-level callable/value
/// with the exported name exists in the imported module path.
fn resolve_import_provenance(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<usize, SymbolStoreError> {
    struct PendingBinding {
        rowid: i64,
        source_path: String,
        import_path: String,
        imported_name: String,
    }

    struct Candidate {
        id: String,
        name: String,
        file_path: String,
    }

    let pending = {
        let mut statement = transaction.prepare(
            r#"
            SELECT rowid, source_file_path, import_path, imported_name
            FROM symbol_relationships
            WHERE target_symbol_id IS NULL
              AND relationship_type = 'call'
              AND import_path IS NOT NULL
              AND imported_name IS NOT NULL
            "#,
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(PendingBinding {
                    rowid: row.get(0)?,
                    source_path: row.get(1)?,
                    import_path: row.get(2)?,
                    imported_name: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    if pending.is_empty() {
        return Ok(0);
    }

    let wanted = pending
        .iter()
        .map(|binding| binding.imported_name.as_str())
        .collect::<HashSet<_>>();
    let candidates = {
        let mut statement = transaction.prepare(
            r#"
            SELECT id, name, file_path
            FROM symbols
            WHERE symbol_type IN ('function','method','class','struct','type','enum','constant')
              AND parent_id IS NULL
            "#,
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(Candidate {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    file_path: row.get(2)?,
                })
            })?
            .filter_map(|row| match row {
                Ok(candidate) if wanted.contains(candidate.name.as_str()) => Some(Ok(candidate)),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        rows
    };

    let mut resolved = 0usize;
    for binding in pending {
        let matches = candidates
            .iter()
            .filter(|candidate| {
                candidate.name == binding.imported_name
                    && candidate_path_matches_import(
                        &binding.source_path,
                        &binding.import_path,
                        &candidate.file_path,
                    )
            })
            .collect::<Vec<_>>();
        if let [candidate] = matches.as_slice() {
            resolved = resolved.saturating_add(transaction.execute(
                r#"
                UPDATE symbol_relationships
                SET target_symbol_id = ?1,
                    resolution_strategy = 'import_binding',
                    confidence = 0.9
                WHERE rowid = ?2 AND target_symbol_id IS NULL
                "#,
                params![candidate.id, binding.rowid],
            )?);
        }
    }

    Ok(resolved)
}

fn candidate_path_matches_import(
    source_path: &str,
    import_path: &str,
    candidate_path: &str,
) -> bool {
    let candidate_stem = module_stem(candidate_path);
    let import_path = import_path.trim();
    if import_path.is_empty() {
        return false;
    }

    let target = if import_path.starts_with('.') {
        let source_parent = Path::new(source_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        normalize_lexical(&source_parent.join(import_path))
            .to_string_lossy()
            .replace('\\', "/")
    } else if let Some(path) = import_path
        .strip_prefix("@/")
        .or_else(|| import_path.strip_prefix("~/"))
        .or_else(|| import_path.strip_prefix('/'))
    {
        path.trim_matches('/').to_owned()
    } else {
        return false;
    };

    candidate_stem == target
        || candidate_stem == format!("{target}/index")
        || candidate_stem.ends_with(&format!("/{target}"))
        || candidate_stem.ends_with(&format!("/{target}/index"))
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    normalized
}

fn module_stem(path: &str) -> String {
    let mut stem = PathBuf::from(path);
    stem.set_extension("");
    stem.to_string_lossy().replace('\\', "/")
}

/// Target-driven global anchor resolution — the same shape as the relationship
/// back-fill above, with the anchor rules preserved: lookups match `name` OR
/// `qualified_name`, and a target is written only when exactly one candidate
/// symbol matches (`HAVING COUNT(*) = 1`; the OR is a row-level predicate over
/// a table keyed by `id`, so the row count IS the distinct-symbol count).
const BUILD_UNIQUE_ANCHOR_TARGETS_SQL: &str = r#"
DROP TABLE IF EXISTS temp.wanted_anchor_lookups;
CREATE TEMP TABLE wanted_anchor_lookups AS
SELECT DISTINCT target_name AS lookup
FROM semantic_anchors
WHERE target_symbol_id IS NULL
  AND target_file_path IS NULL
  AND target_name IS NOT NULL
  AND target_name != '';
CREATE UNIQUE INDEX wanted_anchor_lookups_idx ON wanted_anchor_lookups(lookup);

DROP TABLE IF EXISTS temp.unique_anchor_targets;
CREATE TEMP TABLE unique_anchor_targets AS
SELECT wanted.lookup AS lookup, MIN(symbol.id) AS id
FROM wanted_anchor_lookups AS wanted
JOIN symbols AS symbol
  ON symbol.name = wanted.lookup OR symbol.qualified_name = wanted.lookup
WHERE symbol.symbol_type != 'import'
  AND symbol.qualified_name != '__file__'
GROUP BY wanted.lookup
HAVING COUNT(*) = 1;
CREATE UNIQUE INDEX unique_anchor_targets_idx ON unique_anchor_targets(lookup);
"#;

const GLOBAL_SEMANTIC_TARGET_BACKFILL_SQL: &str = r#"
UPDATE semantic_anchors
SET target_symbol_id = (
    SELECT candidate.id
    FROM unique_anchor_targets AS candidate
    WHERE candidate.lookup = semantic_anchors.target_name
)
WHERE target_file_path IS NULL
  AND target_symbol_id IS NULL
  AND target_name IS NOT NULL
  AND target_name != ''
  AND EXISTS (
    SELECT 1
    FROM unique_anchor_targets AS candidate
    WHERE candidate.lookup = semantic_anchors.target_name
  )
"#;

const DROP_UNIQUE_ANCHOR_TARGETS_SQL: &str = r#"
DROP TABLE wanted_anchor_lookups;
DROP TABLE unique_anchor_targets;
"#;

/// Track H — cap on normalized anchor-query terms. Each term contributes a
/// bounded handful of unindexable `%…%` LIKE predicates (value + preview per
/// case variant), so the cap bounds the per-row predicate work of the
/// full-table scan; extra terms are dropped.
const ANCHOR_QUERY_MAX_TERMS: usize = 8;

/// Track H — bounded overfetch factor for the anchor-candidate SQL. The SQL
/// stage is broad OR-retrieval only (mode semantics are verified in Rust), so
/// every mode overfetches by this factor before Rust filters and truncates.
const ANCHOR_CANDIDATE_OVERFETCH: usize = 4;

/// Track H — render one query term as a `%…%` LIKE CANDIDATE pattern.
///
/// This pattern is retrieval-only — Rust re-verifies every match with literal
/// `str::contains` — so it must be a SUPERSET of the true matches and never
/// treat query text as wildcards:
///   * `\`, `%`, `_` are escaped with `\` (paired with `ESCAPE '\'` in SQL) so
///     `foo_bar` stops wildcard-matching `foo-bar`;
///   * every non-ASCII char degrades to `%`: SQLite's LIKE case-folds ASCII
///     ONLY, so `é` in a pattern would MISS a stored `É` — the wildcard keeps
///     the row retrievable and Rust's Unicode-lowercased `contains` decides.
fn anchor_like_candidate_pattern(term: &str) -> String {
    let mut pattern = String::with_capacity(term.len() + 4);
    pattern.push('%');
    // Tracked explicitly: `ends_with('%')` would be fooled by an ESCAPED `\%`.
    let mut ends_with_wildcard = true;
    for ch in term.chars() {
        match ch {
            '\\' | '%' | '_' => {
                pattern.push('\\');
                pattern.push(ch);
                ends_with_wildcard = false;
            }
            ch if ch.is_ascii() => {
                pattern.push(ch);
                ends_with_wildcard = false;
            }
            _ => {
                if !ends_with_wildcard {
                    pattern.push('%');
                    ends_with_wildcard = true;
                }
            }
        }
    }
    if !ends_with_wildcard {
        pattern.push('%');
    }
    pattern
}

/// Run the whole-index anchor back-fill (build unique-target lookup, update,
/// drop) in one transaction. Returns the number of anchors newly resolved.
fn backfill_global_anchor_targets(conn: &mut Connection) -> Result<usize, SymbolStoreError> {
    let tx = conn.transaction()?;
    tx.execute_batch(BUILD_UNIQUE_ANCHOR_TARGETS_SQL)?;
    let resolved = tx.execute(GLOBAL_SEMANTIC_TARGET_BACKFILL_SQL, [])?;
    tx.execute_batch(DROP_UNIQUE_ANCHOR_TARGETS_SQL)?;
    tx.commit()?;
    Ok(resolved)
}

/// Per-file anchor resolution keeps the correlated form deliberately: a single
/// file's few unresolved anchors don't repay the temp-table setup that pays
/// off for the whole-index passes above.
const GLOBAL_SEMANTIC_TARGET_BACKFILL_FOR_FILE_SQL: &str = r#"
UPDATE semantic_anchors AS anchor
SET target_symbol_id = (
    SELECT symbol.id
    FROM symbols AS symbol
    WHERE (symbol.name = anchor.target_name OR symbol.qualified_name = anchor.target_name)
      AND symbol.symbol_type != 'import'
      AND symbol.qualified_name != '__file__'
    LIMIT 1
)
WHERE anchor.file_path = ?1
  AND anchor.target_file_path IS NULL
  AND anchor.target_symbol_id IS NULL
  AND anchor.target_name IS NOT NULL
  AND anchor.target_name != ''
  AND (
    SELECT COUNT(*)
    FROM symbols AS candidate
    WHERE (candidate.name = anchor.target_name OR candidate.qualified_name = anchor.target_name)
      AND candidate.symbol_type != 'import'
      AND candidate.qualified_name != '__file__'
  ) = 1
"#;

/// M5.1: serialize a relationship's receiver type into the `metadata_json`
/// column (M2.3) as `{"recv_type":"<name>"}`. `None` (bare / unknown receiver) →
/// SQL NULL, so non-receiver edges are byte-for-byte what today stores.
///
/// M5.1b: a `self`/`this`-derived recv_type additionally carries
/// `"recv_self":true` — the provenance the GLOBAL miner requires (a param /
/// constructor / annotation recv_type omits the key and stays byte-identical to
/// what M5.1 stored, so it is never globally mined).
fn relationship_metadata_json(relationship: &SymbolRelationship) -> Option<String> {
    let mut map = serde_json::Map::new();
    if let Some(recv) = relationship.recv_type.as_ref() {
        map.insert("recv_type".into(), serde_json::json!(recv));
        if relationship.recv_self {
            map.insert("recv_self".into(), serde_json::json!(true));
        }
    }
    // Track C: Go `contains` edges carry the method's receiver kind so the
    // implicit-interface miner can distinguish pointer/value method sets.
    if let Some(kind) = relationship.receiver_kind.as_ref() {
        map.insert("receiver".into(), serde_json::json!(kind));
    }
    if map.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(map).to_string())
    }
}

/// M5.1b: the `(recv_type, recv_self)` carried in a relationship's `metadata_json`
/// (`{"recv_type":"<name>","recv_self":<bool>}`), or `None` for malformed /
/// receiver-less metadata. `recv_self` defaults to `false` (a legacy / param /
/// constructor recv_type with no provenance key — NOT eligible for global mining).
fn recv_meta_from_metadata(metadata_json: &str) -> Option<(String, bool)> {
    let value: serde_json::Value = serde_json::from_str(metadata_json).ok()?;
    let recv_type = value.get("recv_type")?.as_str()?.to_string();
    let recv_self = value
        .get("recv_self")
        .and_then(|flag| flag.as_bool())
        .unwrap_or(false);
    Some((recv_type, recv_self))
}

/// M5.1b: class-like symbol kinds whose methods participate in receiver-type
/// dispatch and whose names seed the simple-name → classQn map. Mirrors the
/// per-file resolver's notion of a "type that owns methods" (a free function or
/// a `Type`/`impl` symbol is deliberately excluded — it owns no dispatchable
/// method). Kept as a single source of truth for the SQL `IN (...)` filters.
const RECEIVER_CLASS_KINDS: &str = "'class','struct','interface','enum','trait'";

/// M5.1b GLOBAL receiver-type registry, built ONCE per index over ALL committed
/// symbols + every RESOLVED cross-file inheritance edge. Precision-first: every
/// lookup that is not EXACTLY-ONE yields no resolution, so the caller leaves the
/// edge NULL (unchanged). This is the structure the §13 follow-up adds on top of
/// the M5.1 per-file `ReceiverTypeIndex` to mine the still-NULL call edges.
///
/// PRECISION GATES (the M5.1b reviewer blocker): the registry is keyed entirely
/// by EXACT class qualified names — there is no simple-name → class map, so a
/// receiver type can only enter via an exact qn. The miner only ever feeds it a
/// `self`/`this`-derived recv_type (which IS the enclosing class's exact qn), and
/// the supertype links are built ONLY from inheritance edges whose target
/// RESOLVED to a real indexed class. A library/builtin type whose simple name
/// merely collides with a project class therefore never reaches a project method.
#[derive(Default)]
struct GlobalReceiverRegistry {
    /// `(classQn, methodSimpleName)` → the method symbol ids defined ON that
    /// exact class. `len() > 1` for one key means the method is ambiguous on that
    /// class (duplicate class qn across files, overloads, …) → NULL.
    methods: HashMap<(String, String), Vec<String>>,
    /// classQn → its DIRECT supertype classQns, from RESOLVED `Extends`/`Implements`
    /// edges (cross-file) ONLY. The transitive closure is walked level-by-level at
    /// resolve time (cycle-guarded, depth-capped).
    supertypes: HashMap<String, HashSet<String>>,
}

impl GlobalReceiverRegistry {
    /// Resolve a still-NULL `(recv_type_qn, method)` call edge to EXACTLY ONE
    /// method symbol id, or `None` on ANY ambiguity (precision is the prime
    /// directive). `recv_type_qn` MUST be a guaranteed-project class qualified
    /// name — the miner only passes a `self`/`this`-derived recv_type, which is
    /// the EXACT enclosing-class qn (never a simple name that could shadow an
    /// imported library type). The walk starts from that exact qn; no simple-name
    /// lookup is performed, so a library/builtin receiver can never enter here.
    ///
    /// Walk the supertype chain in derivation order (BFS by level) and take the
    /// MOST-DERIVED level that defines `method` (a method on the class itself wins
    /// over an inherited one — deterministic). Resolve ONLY when that winning
    /// level yields EXACTLY ONE symbol; 0 (not a project method on the chain) or
    /// `> 1` (diamond / parallel supertypes) → NULL.
    fn resolve(&self, recv_type_qn: &str, method: &str) -> Option<String> {
        // The receiver type is the EXACT enclosing-class qn (self/this provenance);
        // start the supertype walk directly from it — no simple-name mapping.
        let start = recv_type_qn.to_string();

        // Most-derived defining level on the supertype chain.
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(start.clone());
        let mut level: Vec<String> = vec![start];
        let mut steps = 0usize;
        while !level.is_empty() {
            // Method ids defined at THIS derivation level (deduped across the
            // possibly-multiple classes sharing the level).
            let mut ids: Vec<String> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for class_qn in &level {
                if let Some(method_ids) = self.methods.get(&(class_qn.clone(), method.to_string()))
                {
                    for id in method_ids {
                        if seen.insert(id.clone()) {
                            ids.push(id.clone());
                        }
                    }
                }
            }
            if !ids.is_empty() {
                // The most-derived class(es) that define the method. EXACTLY ONE
                // → resolve; otherwise the dispatch target is genuinely ambiguous.
                return if ids.len() == 1 {
                    Some(ids.remove(0))
                } else {
                    None
                };
            }
            // Descend to the next (less-derived) level; cycle-guarded.
            let mut next: Vec<String> = Vec::new();
            for class_qn in &level {
                if let Some(supers) = self.supertypes.get(class_qn) {
                    for super_qn in supers {
                        if visited.insert(super_qn.clone()) {
                            next.push(super_qn.clone());
                        }
                    }
                }
            }
            level = next;
            steps += 1;
            if steps > 256 {
                break;
            }
        }
        None
    }
}

/// M5.1b: build the GLOBAL receiver-type registry from the committed DB. Three
/// reads — class-like symbols (id → classQn), their methods (the dispatch table),
/// and the RESOLVED inheritance edges (cross-file supertype links). An
/// inheritance edge whose target did NOT resolve to a real indexed class is
/// dropped, so the closure only ever omits a link — it never invents a wrong
/// supertype by coincidental simple name (a library base has no resolved target →
/// its methods are never pulled in).
fn build_global_receiver_registry(
    conn: &Connection,
) -> Result<GlobalReceiverRegistry, SymbolStoreError> {
    let mut registry = GlobalReceiverRegistry::default();

    // class-like symbols → id → classQn (used to map BOTH the subclass and the
    // RESOLVED inheritance target of every Extends/Implements edge to a real class).
    let mut class_id_to_qn: HashMap<String, String> = HashMap::new();
    {
        let sql = format!(
            r#"
            SELECT id, qualified_name
            FROM symbols
            WHERE symbol_type IN ({RECEIVER_CLASS_KINDS})
              AND qualified_name != ''
              AND qualified_name != '__file__'
            "#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, qualified_name) = row?;
            class_id_to_qn.insert(id, qualified_name);
        }
    }

    // (a) method symbols whose PARENT is a class-like symbol → dispatch table.
    {
        let sql = format!(
            r#"
            SELECT m.id, m.name, p.qualified_name
            FROM symbols m
            JOIN symbols p ON m.parent_id = p.id
            WHERE m.symbol_type = 'method'
              AND p.symbol_type IN ({RECEIVER_CLASS_KINDS})
              AND m.name != ''
              AND p.qualified_name != ''
            "#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, method_name, class_qn) = row?;
            registry
                .methods
                .entry((class_qn, method_name))
                .or_default()
                .push(id);
        }
    }

    // Cross-file inheritance — INHERITANCE GATE (M5.1b precision blocker): follow
    // an Extends/Implements edge ONLY when its `target_symbol_id` RESOLVED to a
    // real indexed class. A link is NEVER added by coincidental simple name, so an
    // `extends LibBase` whose simple name merely collides with a project class
    // (unresolved target) is dropped and that project class's methods are never
    // pulled into the subtype's dispatch.
    {
        let mut stmt = conn.prepare(
            r#"
            SELECT source_symbol_id, target_symbol_id
            FROM symbol_relationships
            WHERE relationship_type IN ('extends','implements')
              AND target_symbol_id IS NOT NULL
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (source_id, target_id) = row?;
            // The subclass must itself be a known project class.
            let Some(sub_qn) = class_id_to_qn.get(&source_id).cloned() else {
                continue;
            };
            // The RESOLVED target must itself be a known project class (not, e.g.,
            // a function the name happened to bind to). A target outside the class
            // set is dropped — never a wrong supertype link.
            let Some(super_qn) = class_id_to_qn.get(&target_id).cloned() else {
                continue;
            };
            if super_qn == sub_qn {
                continue;
            }
            registry
                .supertypes
                .entry(sub_qn)
                .or_default()
                .insert(super_qn);
        }
    }

    Ok(registry)
}

/// Track C — the package of a Go file is its parent directory path
/// (`pkg/cache/mem.go` → `pkg/cache`; a root-level file → `""`).
fn go_package_dir(file_path: &str) -> &str {
    file_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// Go keywords that can START an unnamed type followed by whitespace
/// (`chan int`, `func(int) error`, …). A leading token in this set is never a
/// parameter NAME, so `chan int` normalizes as a type, not as `name type`.
const GO_TYPE_KEYWORDS: &[&str] = &["chan", "func", "map", "struct", "interface"];

/// Collapse internal whitespace runs to single spaces (`map[string]  int` and
/// `map[string] int` canonicalize identically).
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` starting at `(`: return `(inner, rest_after_close)` for the first
/// balanced group, or `None` when the text is not a well-formed group
/// (conservative — the caller falls back to raw text).
fn split_balanced_group(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    // Only the matching `)` may close the opening group.
                    if *byte != b')' {
                        return None;
                    }
                    return Some((&text[1..index], &text[index + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Split on commas at bracket depth 0 (`string, map[int, x]y` → 2 items).
fn split_top_level_commas(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, byte) in text.bytes().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                items.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    items.push(text[start..].trim());
    items.retain(|item| !item.is_empty());
    items
}

/// A bare Go identifier (letter/underscore start, alphanumeric/underscore
/// body) — the only shape a parameter NAME can take.
fn is_plain_go_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_alphanumeric() || ch == '_')
}

/// When `item` is definitively `name Type` (leading plain identifier that is
/// not a type keyword, followed by top-level whitespace and a type), return
/// the type text; otherwise `None` (the item is an unnamed type, or too
/// ambiguous to strip — conservative).
fn strip_go_param_name(item: &str) -> Option<&str> {
    let mut depth = 0usize;
    for (index, ch) in item.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ch if ch.is_whitespace() && depth == 0 => {
                let head = &item[..index];
                let tail = item[index..].trim_start();
                if tail.is_empty() {
                    return None;
                }
                if is_plain_go_identifier(head) && !GO_TYPE_KEYWORDS.contains(&head) {
                    return Some(tail);
                }
                return None;
            }
            _ => {}
        }
    }
    None
}

/// Canonicalize a Go parameter list: strip parameter NAMES, keep parameter
/// TYPES, collapse whitespace. Go lists are either all-named or all-unnamed,
/// so if no item is definitively `name Type` every item is kept as a raw type;
/// grouped names (`a, b string`) inherit the type of the next typed item to
/// their right. Anything ambiguous keeps its raw (whitespace-collapsed) text —
/// both sides of a comparison normalize identically, so a conservative raw
/// keep can only produce a false negative, never a wrong match.
fn canonical_go_param_list(raw: &str) -> String {
    let items = split_top_level_commas(raw);
    if items.is_empty() {
        return String::new();
    }
    let list_is_named = items.iter().any(|item| strip_go_param_name(item).is_some());
    if !list_is_named {
        return items
            .iter()
            .map(|item| collapse_whitespace(item))
            .collect::<Vec<_>>()
            .join(",");
    }
    // Named list: walk right-to-left so grouped bare names pick up the type of
    // the nearest typed item after them.
    let mut types_reversed: Vec<String> = Vec::with_capacity(items.len());
    let mut inherited: Option<String> = None;
    for item in items.iter().rev() {
        if let Some(type_text) = strip_go_param_name(item) {
            let canonical = collapse_whitespace(type_text);
            inherited = Some(canonical.clone());
            types_reversed.push(canonical);
        } else if is_plain_go_identifier(item) {
            match &inherited {
                Some(type_text) => types_reversed.push(type_text.clone()),
                // Nothing to inherit: ambiguous — keep the raw text.
                None => types_reversed.push(collapse_whitespace(item)),
            }
        } else {
            // A complex typeless item inside a named list is not legal Go;
            // keep it raw and stop inheriting through it.
            inherited = None;
            types_reversed.push(collapse_whitespace(item));
        }
    }
    types_reversed.reverse();
    types_reversed.join(",")
}

/// Canonicalize a Go result list. `func() (error)` and `func() error` are the
/// same signature, so a parenthesized group canonicalizes to the bare comma
/// list with result NAMES stripped like parameters.
fn canonical_go_result_list(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some((inner, rest)) = split_balanced_group(trimmed) {
        if rest.trim().is_empty() {
            return canonical_go_param_list(inner);
        }
    }
    collapse_whitespace(trimmed)
}

/// Track C — receiver-independent canonical Go method signature. Parameter
/// NAMES and whitespace are stripped; parameter and result TYPES are kept
/// (grouped params like `(a, b string)` become two `string` params). The
/// output embeds the method name, so the canonical string alone IS the
/// method-set identity `(name, signature)`. Tolerates a leading receiver
/// group (`(m Memory) Ping(…) error`) by skipping it; anything it cannot
/// parse confidently keeps its raw whitespace-collapsed text — deterministic
/// on both comparison sides, so ambiguity yields a false negative at worst.
fn canonical_go_method_signature(name: &str, signature: &str) -> String {
    let trimmed = signature.trim();
    if trimmed.is_empty() {
        return format!("{name}()");
    }
    let Some(open) = trimmed.find('(') else {
        return format!("{name} {}", collapse_whitespace(trimmed));
    };
    let Some((first_inner, after_first)) = split_balanced_group(&trimmed[open..]) else {
        return format!("{name} {}", collapse_whitespace(trimmed));
    };
    // Receiver-independence: `(recv T) Name(params) results` — when the text
    // after the first group is `<identifier>(…` and the identifier is not a
    // type keyword, the first group was a receiver; skip it and the name.
    let after_first_trimmed = after_first.trim_start();
    let (params_raw, results_raw) = match after_first_trimmed.find('(') {
        Some(paren) => {
            let head = after_first_trimmed[..paren].trim_end();
            if !head.is_empty() && is_plain_go_identifier(head) && !GO_TYPE_KEYWORDS.contains(&head)
            {
                match split_balanced_group(&after_first_trimmed[paren..]) {
                    Some((params, results)) => (params, results),
                    None => return format!("{name} {}", collapse_whitespace(trimmed)),
                }
            } else {
                (first_inner, after_first)
            }
        }
        None => (first_inner, after_first),
    };
    let params = canonical_go_param_list(params_raw);
    let results = canonical_go_result_list(results_raw);
    if results.is_empty() {
        format!("{name}({params})")
    } else {
        format!("{name}({params}) {results}")
    }
}

/// Go universe/builtin type names — the only BARE identifiers whose meaning is
/// identical in every package. Any other bare identifier (`Config`) resolves
/// against its OWN package's declarations, so two textually equal canonical
/// signatures in different packages may denote different types.
const GO_UNIVERSE_TYPES: &[&str] = &[
    "any",
    "bool",
    "byte",
    "comparable",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
];

/// `rest` starting just AFTER an opening `[`: return `(inner, after_close)`
/// for the matching `]`, or `None` when unbalanced / closed by a different
/// bracket (conservative).
fn split_go_bracket_group(rest: &str) -> Option<(&str, &str)> {
    let mut depth = 1usize;
    for (index, byte) in rest.bytes().enumerate() {
        match byte {
            b'[' | b'(' | b'{' => depth += 1,
            b']' | b')' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    if byte != b']' {
                        return None;
                    }
                    return Some((&rest[..index], &rest[index + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Track C / cross-package guard — is a canonical Go TYPE text
/// PACKAGE-PORTABLE, i.e. guaranteed to denote the same type no matter which
/// package's file spelled it? Portable shapes:
///   * universe/builtin bare identifiers (`string`, `error`, …);
///   * QUALIFIED names (`context.Context`) — spelled against an import, not
///     the local package scope (import aliasing is ignored, matching the
///     handoff's accepted risk level);
///   * empty `interface{}` / `struct{}` literals;
///   * composites over portable types: pointers, variadics, slices,
///     literal-length arrays, maps, chans, and `func` types with unnamed
///     portable params/results.
/// Anything else — notably a bare NON-universe identifier like `Config` — is
/// non-portable: it resolves per-package, so textual equality across packages
/// proves nothing. Unparseable text is non-portable (false negative over a
/// compiler-false edge).
fn is_package_portable_go_type(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("...") {
        return is_package_portable_go_type(rest);
    }
    if let Some(rest) = trimmed.strip_prefix('*') {
        return is_package_portable_go_type(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("<-chan") {
        return is_package_portable_go_type(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("chan<-") {
        return is_package_portable_go_type(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("chan ") {
        return is_package_portable_go_type(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("map[") {
        let Some((key, value)) = split_go_bracket_group(rest) else {
            return false;
        };
        return is_package_portable_go_type(key) && is_package_portable_go_type(value);
    }
    if let Some(rest) = trimmed.strip_prefix('[') {
        let Some((length, element)) = split_go_bracket_group(rest) else {
            return false;
        };
        let length = length.trim();
        // Slice (`[]T`) or literal-length array (`[4]T`). A CONST-named length
        // (`[N]T`) resolves per-package — non-portable.
        if !(length.is_empty() || length.bytes().all(|byte| byte.is_ascii_digit())) {
            return false;
        }
        return is_package_portable_go_type(element);
    }
    if let Some(rest) = trimmed.strip_prefix("func") {
        let rest = rest.trim_start();
        let Some((params, results)) = split_balanced_group(rest) else {
            return false;
        };
        if !split_top_level_commas(params)
            .iter()
            .all(|param| is_package_portable_go_type(param))
        {
            return false;
        }
        let results = canonical_go_result_list(results);
        return results.is_empty()
            || split_top_level_commas(&results)
                .iter()
                .all(|result| is_package_portable_go_type(result));
    }
    let squashed: String = trimmed.chars().filter(|ch| !ch.is_whitespace()).collect();
    if squashed == "interface{}" || squashed == "struct{}" {
        return true;
    }
    if trimmed.contains('.')
        && trimmed
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.')
    {
        return true;
    }
    is_plain_go_identifier(trimmed) && GO_UNIVERSE_TYPES.contains(&trimmed)
}

/// Track C / cross-package guard — is a CANONICAL method signature
/// (`Name(params) results` from `canonical_go_method_signature`) built purely
/// from package-portable types? Only then does canonical-string equality
/// between a method in one package and an interface spec in ANOTHER package
/// imply the compiler would agree. The raw-fallback canonical shapes are not
/// analyzable and are treated as non-portable.
fn canonical_go_signature_is_package_portable(canonical: &str) -> bool {
    let Some(open) = canonical.find('(') else {
        return false;
    };
    if !is_plain_go_identifier(&canonical[..open]) {
        return false;
    }
    let Some((params, results)) = split_balanced_group(&canonical[open..]) else {
        return false;
    };
    if !split_top_level_commas(params)
        .iter()
        .all(|param| is_package_portable_go_type(param))
    {
        return false;
    }
    let results = results.trim();
    results.is_empty()
        || split_top_level_commas(results)
            .iter()
            .all(|result| is_package_portable_go_type(result))
}

/// Track C — pointer/value receiver classification of a Go `contains` edge
/// (`{"receiver":"pointer"}` / `{"receiver":"value"}` in `metadata_json`).
/// Returns `true` only for an explicit pointer tag. Absent or malformed
/// metadata and unrecognized values are treated as VALUE receivers
/// (conservative-INCLUSIVE by the Track C contract: a value-receiver method
/// belongs to BOTH method sets, so an untagged edge widens the value set for
/// recall; pointer-only satisfaction still carries its explicit
/// `receiver_set` marker whenever the extractor tags the edge).
fn go_contains_receiver_is_pointer(metadata_json: Option<&str>) -> bool {
    metadata_json
        .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
        .and_then(|value| {
            value
                .get("receiver")
                .and_then(|receiver| receiver.as_str())
                .map(str::to_string)
        })
        .is_some_and(|receiver| receiver == "pointer")
}

/// Track C — one required method of a Go interface: simple name, canonical
/// signature, and the package dir of the interface that DECLARED the spec.
/// An unexported requirement (lowercase first char) can only be satisfied by
/// a type in that exact package — Go's package-visibility rule; keying by the
/// DECLARING interface (not the interface being checked) means an unexported
/// method inherited from an embedded foreign-package interface stays
/// unsatisfiable outside its home package (prefer the false negative).
#[derive(Clone, PartialEq, Eq, Hash)]
struct GoInterfaceRequirement {
    method_name: String,
    canonical_signature: String,
    declaring_package: String,
    /// Derived once from `canonical_signature` via
    /// `canonical_go_signature_is_package_portable`: whether every type in the
    /// signature means the same thing in EVERY package. A non-portable
    /// requirement is only satisfiable by a type in `declaring_package` —
    /// `pkga.Applier`'s `Apply(Config) error` must never match `pkgb`'s
    /// textually identical method over a DIFFERENT `Config`.
    package_portable: bool,
}

/// Track C — a Go interface or concrete named type participating in implicit
/// implementation mining.
struct GoNamedSymbol {
    name: String,
    file_path: String,
    line: u32,
    package_dir: String,
}

/// SQLite-backed symbol store
pub struct SymbolStore {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationshipObservationKind {
    SyntaxExtracted,
    IndexStructural,
}

impl RelationshipObservationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SyntaxExtracted => "syntax_extracted",
            Self::IndexStructural => "index_structural",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SymbolReference {
    pub source_symbol: Symbol,
    pub relationship_type: SymbolRelationshipType,
    pub target_name: String,
    pub target_symbol_id: Option<String>,
    pub target_symbol: Option<Symbol>,
    pub line: u32,
    pub observation_kind: RelationshipObservationKind,
    pub resolution_strategy: Option<String>,
    pub resolution_confidence: Option<f32>,
    pub receiver_type: Option<String>,
    pub receiver_is_self: bool,
    pub import_path: Option<String>,
    pub imported_name: Option<String>,
}

struct StoredRelationshipReference {
    source_symbol: Symbol,
    relationship_type: String,
    target_name: String,
    target_symbol_id: Option<String>,
    line: u32,
    resolution_strategy: Option<String>,
    resolution_confidence: Option<f32>,
    metadata_json: Option<String>,
    import_path: Option<String>,
    imported_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IndexedFileRecord {
    pub file_path: String,
    pub file_hash: String,
    pub indexed_at: i64,
    pub symbol_count: usize,
    pub file_size: Option<u64>,
    pub line_count: Option<usize>,
    pub modified_at: Option<i64>,
    pub extractor_version: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct FileIndexRecord {
    pub file_path: String,
    pub file_hash: String,
    pub file_size: Option<u64>,
    pub line_count: Option<usize>,
    pub modified_at: Option<i64>,
    pub extractor_version: Option<u32>,
    pub symbols: Vec<Symbol>,
    pub anchors: Vec<SemanticAnchor>,
    pub relationships: Vec<SymbolRelationship>,
}

#[derive(Debug, Clone)]
pub struct FileRelationshipRecord {
    pub file_path: String,
    pub relationships: Vec<SymbolRelationship>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleRelationshipAggregate {
    pub source_file_path: String,
    pub target_file_path: String,
    pub relationship_type: SymbolRelationshipType,
    pub edge_count: usize,
    pub average_confidence: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationshipIntegrityStats {
    pub total_relationships: usize,
    pub resolved_relationships: usize,
    pub unresolved_symbol_relationships: usize,
    pub missing_source_symbols: usize,
    pub missing_target_symbols: usize,
    pub by_type: Vec<RelationshipTypeStats>,
    pub top_unresolved_targets: Vec<UnresolvedRelationshipTarget>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationshipTypeStats {
    pub relationship_type: String,
    pub total_relationships: usize,
    pub resolved_relationships: usize,
    pub unresolved_symbol_relationships: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnresolvedRelationshipTarget {
    pub relationship_type: String,
    pub target_name: String,
    pub count: usize,
    pub example_source_file: String,
}

/// Per-language × per-kind coverage matrix with explicit `0` rows back-filled.
///
/// Read-only diagnostic answering "are we indexing the right data per file
/// type." Serialize-friendly so it can later feed the schema tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoverageHistogram {
    pub per_language: Vec<LanguageCoverage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LanguageCoverage {
    pub language: String,
    /// Count per `SymbolType` variant; absent variants are present as `0`.
    pub symbol_counts: Vec<(String, u64)>,
    /// Count per `SymbolRelationshipType` variant; absent variants are `0`.
    pub relationship_counts: Vec<(String, u64)>,
    /// Fraction (0.0..=1.0) of Function+Method symbols with a non-empty signature.
    pub function_method_signature_pct: f64,
}

/// Every `SymbolType` variant, so the histogram can back-fill an explicit `0`
/// row for each kind that never appeared in a given language.
const ALL_SYMBOL_TYPES: &[SymbolType] = &[
    SymbolType::Function,
    SymbolType::Method,
    SymbolType::Class,
    SymbolType::Struct,
    SymbolType::Interface,
    SymbolType::Type,
    SymbolType::Enum,
    SymbolType::EnumMember,
    SymbolType::Constant,
    SymbolType::Variable,
    SymbolType::Property,
    SymbolType::Module,
    SymbolType::Namespace,
    SymbolType::Import,
    SymbolType::Export,
    SymbolType::Trait,
    SymbolType::Impl,
    SymbolType::Heading,
    SymbolType::CssSelector,
    SymbolType::CssCustomProperty,
    SymbolType::CssKeyframes,
    SymbolType::CssAtRule,
    SymbolType::CssLayer,
    SymbolType::CssFontFace,
    SymbolType::Resource,
    SymbolType::Route,
];

/// Every `SymbolRelationshipType` variant, for the same zero back-fill.
const ALL_RELATIONSHIP_TYPES: &[SymbolRelationshipType] = &[
    SymbolRelationshipType::Call,
    SymbolRelationshipType::Import,
    SymbolRelationshipType::Export,
    SymbolRelationshipType::Extends,
    SymbolRelationshipType::Implements,
    SymbolRelationshipType::Contains,
    SymbolRelationshipType::Usage,
    SymbolRelationshipType::UsesType,
    SymbolRelationshipType::ReadsEnv,
    SymbolRelationshipType::Handles,
];

// NOTE: language derived from file path until M2.3 adds a real `language` column; re-base then.
fn coverage_language_label(file_path: &str) -> String {
    Language::from_path(file_path)
        .map(|language| language.display_name().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// Drift guard: adding a variant to either enum without extending the arrays
// above is a compile error, keeping the zero back-fill exhaustive.
#[allow(dead_code)]
fn coverage_variant_drift_guard(
    symbol_type: SymbolType,
    relationship_type: SymbolRelationshipType,
) {
    match symbol_type {
        SymbolType::Function
        | SymbolType::Method
        | SymbolType::Class
        | SymbolType::Struct
        | SymbolType::Interface
        | SymbolType::Type
        | SymbolType::Enum
        | SymbolType::EnumMember
        | SymbolType::Constant
        | SymbolType::Variable
        | SymbolType::Property
        | SymbolType::Module
        | SymbolType::Namespace
        | SymbolType::Import
        | SymbolType::Export
        | SymbolType::Trait
        | SymbolType::Impl
        | SymbolType::Heading
        | SymbolType::CssSelector
        | SymbolType::CssCustomProperty
        | SymbolType::CssKeyframes
        | SymbolType::CssAtRule
        | SymbolType::CssLayer
        | SymbolType::CssFontFace
        | SymbolType::Resource
        | SymbolType::Route => {}
    }
    match relationship_type {
        SymbolRelationshipType::Call
        | SymbolRelationshipType::Import
        | SymbolRelationshipType::Export
        | SymbolRelationshipType::Extends
        | SymbolRelationshipType::Implements
        | SymbolRelationshipType::Contains
        | SymbolRelationshipType::Usage
        | SymbolRelationshipType::UsesType
        | SymbolRelationshipType::ReadsEnv
        | SymbolRelationshipType::Handles => {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticAnchor {
    pub id: String,
    pub file_path: String,
    pub kind: String,
    pub value: String,
    pub line: u32,
    pub character: u32,
    pub preview: String,
    pub confidence: f32,
    #[serde(default)]
    pub owner_symbol_id: Option<String>,
    #[serde(default)]
    pub target_file_path: Option<String>,
    #[serde(default)]
    pub target_name: Option<String>,
    #[serde(default)]
    pub target_symbol_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticAnchorResult {
    pub anchor: SemanticAnchor,
    pub score: f32,
    /// Track H — how many normalized query terms this row matched. `Some` only
    /// for the term-based modes (`AllTerms`/`AnyTerms`); `None` in `Phrase`
    /// mode, where term accounting does not apply.
    #[serde(default)]
    pub matched_terms: Option<usize>,
    /// Track H — the total normalized term count the query produced (the
    /// denominator of `matched_terms`). `None` in `Phrase` mode.
    #[serde(default)]
    pub total_terms: Option<usize>,
}

/// Track H — how a semantic-anchor query is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorQueryMode {
    /// Exact contiguous substring of value/preview — today's historical
    /// behavior and the delegate for `search_semantic_anchors`.
    Phrase,
    /// Every normalized whitespace-split term must match value OR preview.
    AllTerms,
    /// Any term may match; rows carry matched/total coverage and are ranked by
    /// coverage-weighted score.
    AnyTerms,
}

impl AnchorQueryMode {
    /// Parse the wire name used by the tool layer. Unknown strings are `None`
    /// so the caller can reject them explicitly instead of silently defaulting.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "phrase" => Some(Self::Phrase),
            "all_terms" => Some(Self::AllTerms),
            "any_terms" => Some(Self::AnyTerms),
            _ => None,
        }
    }
}

/// Track H — the full result of a mode-aware anchor search. `empty_query`
/// distinguishes "the query normalized to nothing" from a legitimate no-match:
/// an empty phrase search must never look identical to zero matches.
#[derive(Debug, Clone)]
pub struct SemanticAnchorSearchOutcome {
    pub results: Vec<SemanticAnchorResult>,
    pub empty_query: bool,
    pub mode: AnchorQueryMode,
    pub total_terms: usize,
}

fn query_scoped_count(
    conn: &Connection,
    unscoped_sql: &str,
    scoped_sql: &str,
    scoped: Option<(&str, &str)>,
) -> Result<i64, SymbolStoreError> {
    match scoped {
        Some((scope, like)) => conn.query_row(scoped_sql, params![scope, like], |row| row.get(0)),
        None => conn.query_row(unscoped_sql, [], |row| row.get(0)),
    }
    .map_err(SymbolStoreError::from)
}

/// M5.4 — bulk-write performance PRAGMAs for the symbol-index connection.
///
/// The symbol index is a DERIVED, fully rebuildable cache, so we trade the
/// SQLite defaults' maximal durability for write throughput. On a cold index the
/// DB write is 60–95% of wall time (278k INSERT-OR-REPLACE on a TEXT PK, each
/// maintaining every secondary index), and the defaults make it worst-case:
///   * `journal_mode = WAL` — faster than the default DELETE journal (no
///     per-transaction journal create/delete) and lets UI reads run concurrently
///     with the indexer's writes instead of blocking on them.
///   * `synchronous = NORMAL` — safe under WAL (no corruption on power loss; at
///     worst the last in-flight commit is lost, and the index just re-reconciles
///     it), without the fsync-per-commit of FULL.
///   * `cache_size = -65536` — a 64 MiB page cache (a BOUNDED amount of RAM,
///     unlike `mmap_size`) keeps the index B-trees hot through the insert storm
///     instead of thrashing the 2 MiB default cache to disk.
///   * `temp_store = MEMORY` — build transient indexes/sorts in RAM.
///
/// Deliberately NOT set: `mmap_size` — mapping the DB into the process address
/// space inflates RSS, the memory bloat we already fought, and buys little for a
/// write-bound workload.
fn configure_index_pragmas(conn: &Connection) -> Result<(), SymbolStoreError> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;\n\
         PRAGMA synchronous = NORMAL;\n\
         PRAGMA temp_store = MEMORY;\n\
         PRAGMA cache_size = -65536;\n\
         PRAGMA wal_autocheckpoint = 262144;",
    )?;
    Ok(())
}
// M5.17 — `wal_autocheckpoint = 262144` (1 GiB, up from SQLite's default 1000
// pages / 4 MiB). Live profiling of a Firefox cold index showed the single
// commit thread pinned in `sqlite3WalDefaultHook → wal_checkpoint → fsync`: the
// default fired a synchronous checkpoint+fsync on essentially every batch commit
// (~1000 over a multi-GB index), serializing the committer on disk I/O while all
// 31 extraction workers sat idle on backpressure (9% iowait, ~1 core of 32 busy).
// Raising the threshold means only a handful of checkpoints fire mid-index; the
// commits in between are cheap WAL appends, so the committer keeps up with
// extraction and the cores stay busy. The final `checkpoint()` (M5.13,
// TRUNCATE) still consolidates the WAL at the end. Interactive edits produce
// tiny WAL growth, so they never approach the threshold — no downside there.

impl SymbolStore {
    /// Create a new symbol store at the given path
    pub fn new(db_path: &Path) -> Result<Self, SymbolStoreError> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
        configure_index_pragmas(&conn)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.create_schema()?;
        Ok(store)
    }

    /// Create an in-memory symbol store (for testing)
    pub fn in_memory() -> Result<Self, SymbolStoreError> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.create_schema()?;
        Ok(store)
    }

    /// M5.13 — collapse the write-ahead log back into the main database file and
    /// truncate the `-wal` sidecar to zero bytes.
    ///
    /// With WAL + a single long-lived connection under a heavy cold index,
    /// SQLite's passive autocheckpoint falls far behind (a 469k-file Firefox
    /// index left a 560 MB `-wal`), stranding committed data outside the main
    /// file and slowing the next open (which must replay the whole WAL). Call
    /// this once indexing settles. A busy checkpoint (some other reader active)
    /// is NON-FATAL — the checkpoint simply does less and the WAL stays until the
    /// next attempt; it never blocks or fails the index.
    pub fn checkpoint(&self) -> Result<(), SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// Create database schema
    fn create_schema(&self) -> Result<(), SymbolStoreError> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            r#"
            -- Main symbols table
            CREATE TABLE IF NOT EXISTS symbols (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                qualified_name TEXT NOT NULL DEFAULT '',
                symbol_type TEXT NOT NULL,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                start_char INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_char INTEGER NOT NULL,
                byte_offset INTEGER NOT NULL DEFAULT 0,
                byte_length INTEGER NOT NULL DEFAULT 0,
                parent_id TEXT,
                docstring TEXT,
                signature TEXT,
                content_hash TEXT NOT NULL DEFAULT '',
                indexed_at INTEGER NOT NULL
            );

            -- Full-text search using FTS5
            CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
                name,
                qualified_name,
                signature,
                docstring,
                content=symbols,
                content_rowid=rowid,
                tokenize='unicode61'
            );

            -- Triggers to keep FTS in sync
            CREATE TRIGGER IF NOT EXISTS symbols_ai AFTER INSERT ON symbols BEGIN
                INSERT INTO symbols_fts(rowid, name, qualified_name, signature, docstring)
                VALUES (new.rowid, new.name, new.qualified_name, new.signature, new.docstring);
            END;

            CREATE TRIGGER IF NOT EXISTS symbols_ad AFTER DELETE ON symbols BEGIN
                INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, signature, docstring)
                VALUES ('delete', old.rowid, old.name, old.qualified_name, old.signature, old.docstring);
            END;

            CREATE TRIGGER IF NOT EXISTS symbols_au AFTER UPDATE ON symbols BEGIN
                INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, signature, docstring)
                VALUES ('delete', old.rowid, old.name, old.qualified_name, old.signature, old.docstring);
                INSERT INTO symbols_fts(rowid, name, qualified_name, signature, docstring)
                VALUES (new.rowid, new.name, new.qualified_name, new.signature, new.docstring);
            END;

            -- File metadata for tracking indexing status
            CREATE TABLE IF NOT EXISTS indexed_files (
                file_path TEXT PRIMARY KEY,
                file_hash TEXT,
                indexed_at INTEGER NOT NULL,
                symbol_count INTEGER NOT NULL,
                file_size INTEGER,
                line_count INTEGER,
                modified_at INTEGER
            );

            CREATE TABLE IF NOT EXISTS symbol_relationships (
                source_symbol_id TEXT NOT NULL,
                source_file_path TEXT NOT NULL,
                target_name TEXT NOT NULL,
                target_symbol_id TEXT,
                relationship_type TEXT NOT NULL,
                line INTEGER NOT NULL,
                PRIMARY KEY (source_symbol_id, target_name, relationship_type, line)
            );


            CREATE TABLE IF NOT EXISTS semantic_anchors (
                id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                kind TEXT NOT NULL,
                value TEXT NOT NULL,
                line INTEGER NOT NULL,
                character INTEGER NOT NULL,
                preview TEXT NOT NULL,
                confidence REAL NOT NULL,
                owner_symbol_id TEXT,
                target_file_path TEXT,
                target_name TEXT,
                target_symbol_id TEXT,
                indexed_at INTEGER NOT NULL
            );

            -- M6.1 — key/value cache for the reconcile no-change fast path. Holds the
            -- fingerprint + last fully-healthy health/graph snapshot as JSON. Cleared
            -- whenever generated index data is cleared (see clear_generated_index_data).
            CREATE TABLE IF NOT EXISTS index_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )?;

        ensure_column(
            &conn,
            "symbols",
            "qualified_name",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            &conn,
            "symbols",
            "byte_offset",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            &conn,
            "symbols",
            "byte_length",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(&conn, "symbols", "content_hash", "TEXT NOT NULL DEFAULT ''")?;
        ensure_column(&conn, "symbol_relationships", "target_symbol_id", "TEXT")?;
        ensure_column(&conn, "indexed_files", "file_size", "INTEGER")?;
        ensure_column(&conn, "indexed_files", "line_count", "INTEGER")?;
        ensure_column(&conn, "indexed_files", "modified_at", "INTEGER")?;
        ensure_column(&conn, "indexed_files", "extractor_version", "INTEGER")?;

        conn.execute_batch(
            r#"
            -- Indexes for common queries
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_qualified_name ON symbols(qualified_name);
            CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
            CREATE INDEX IF NOT EXISTS idx_symbols_type ON symbols(symbol_type);
            CREATE INDEX IF NOT EXISTS idx_symbols_parent ON symbols(parent_id);
            CREATE INDEX IF NOT EXISTS idx_symbols_indexed ON symbols(indexed_at);
            -- No source_symbol_id index: the composite PRIMARY KEY's autoindex
            -- (sqlite_autoindex_symbol_relationships_1) already serves those
            -- lookups via its leftmost prefix. Migration v3 drops the old
            -- duplicate from pre-existing databases.
            CREATE INDEX IF NOT EXISTS idx_symbol_relationships_file ON symbol_relationships(source_file_path);
            CREATE INDEX IF NOT EXISTS idx_symbol_relationships_target ON symbol_relationships(target_name);
            CREATE INDEX IF NOT EXISTS idx_symbol_relationships_target_symbol_id ON symbol_relationships(target_symbol_id);
            CREATE INDEX IF NOT EXISTS idx_semantic_anchors_value ON semantic_anchors(value);
            CREATE INDEX IF NOT EXISTS idx_semantic_anchors_file ON semantic_anchors(file_path);
            CREATE INDEX IF NOT EXISTS idx_semantic_anchors_kind ON semantic_anchors(kind);
            "#,
        )?;

        // Apply ordered, versioned migrations AFTER the base schema is ensured.
        // This framework owns NEW schema going forward (M2.3); the legacy
        // `ensure_column` probes above continue to own the CURRENT columns.
        run_migrations(&conn)?;

        Ok(())
    }

    /// Insert or update symbols for a file
    pub fn upsert_symbols(&self, symbols: &[Symbol]) -> Result<usize, SymbolStoreError> {
        if symbols.is_empty() {
            return Ok(0);
        }

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        for symbol in symbols {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO symbols 
                (id, name, qualified_name, symbol_type, file_path, start_line, start_char, end_line, end_char,
                 byte_offset, byte_length, parent_id, docstring, signature, content_hash, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
                params![
                    &symbol.id,
                    &symbol.name,
                    &symbol.qualified_name,
                    symbol.symbol_type.to_string(),
                    &symbol.file_path,
                    symbol.range.start.line,
                    symbol.range.start.character,
                    symbol.range.end.line,
                    symbol.range.end.character,
                    symbol.byte_offset as i64,
                    symbol.byte_length as i64,
                    symbol.parent_id.as_deref(),
                    symbol.docstring.as_deref(),
                    symbol.signature.as_deref(),
                    &symbol.content_hash,
                    now,
                ],
            )?;
        }

        tx.commit()?;
        Ok(symbols.len())
    }

    pub fn replace_semantic_anchors_for_file(
        &self,
        file_path: &str,
        anchors: &[SemanticAnchor],
    ) -> Result<usize, SymbolStoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        tx.execute(
            "DELETE FROM semantic_anchors WHERE file_path = ?1",
            params![file_path],
        )?;

        for anchor in anchors {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO semantic_anchors
                (id, file_path, kind, value, line, character, preview, confidence,
                 owner_symbol_id, target_file_path, target_name, target_symbol_id, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
                params![
                    &anchor.id,
                    &anchor.file_path,
                    &anchor.kind,
                    &anchor.value,
                    anchor.line,
                    anchor.character,
                    &anchor.preview,
                    anchor.confidence,
                    anchor.owner_symbol_id.as_deref(),
                    anchor.target_file_path.as_deref(),
                    anchor.target_name.as_deref(),
                    anchor.target_symbol_id.as_deref(),
                    now,
                ],
            )?;
        }

        tx.commit()?;
        Ok(anchors.len())
    }

    /// Historical single-phrase anchor search — delegates to
    /// `search_semantic_anchors_mode` in `Phrase` mode so the ~20 existing
    /// call sites keep their exact contiguous-substring semantics and shape.
    pub fn search_semantic_anchors(
        &self,
        query: &str,
        file_path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SemanticAnchorResult>, SymbolStoreError> {
        Ok(self
            .search_semantic_anchors_mode(query, file_path, limit, AnchorQueryMode::Phrase)?
            .results)
    }

    /// Track H — mode-aware semantic-anchor search over `value`/`preview`.
    ///
    /// Two-stage design (review findings G+H): SQL is BROAD CANDIDATE
    /// RETRIEVAL ONLY — OR-joined per-term LIKE patterns (original-case and
    /// lowercased variants, metacharacters escaped via `ESCAPE '\'`, non-ASCII
    /// degraded to wildcards — see `anchor_like_candidate_pattern`) with a
    /// bounded overfetch. ALL mode semantics live in Rust over
    /// Unicode-lowercased text, because SQL LIKE alone gets both edges wrong:
    /// `_`/`%` in a query act as wildcards (AllTerms then OVERCLAIMS
    /// `matched_terms`), and LIKE case-folds ASCII only (accented queries
    /// false-empty against differently-cased values).
    ///
    /// * `Phrase`: the whole trimmed phrase must be a contiguous
    ///   case-insensitive substring of value or preview.
    /// * `AllTerms`: every normalized whitespace-split term must match —
    ///   verified per term in Rust; `matched_terms` never exceeds what Rust
    ///   verified.
    /// * `AnyTerms`: at least one term matches; rows carry matched/total
    ///   coverage and the final score is the phrase score weighted by
    ///   `matched/total`.
    ///
    /// Results in every mode are scored and sorted in Rust (score descending,
    /// deterministic location tie-break) and truncated to `limit`. An empty
    /// trimmed query returns `empty_query = true` with no results — never
    /// silently identical to a legitimate no-match.
    ///
    /// PERFORMANCE CAVEAT: every predicate is an unindexable `%…%` LIKE, so
    /// each search is a full scan of `semantic_anchors`. Acceptable because
    /// the table is small relative to `symbols` and the predicate count is
    /// bounded: at most `ANCHOR_QUERY_MAX_TERMS` terms, at most two pattern
    /// variants each (two LIKEs per variant).
    pub fn search_semantic_anchors_mode(
        &self,
        query: &str,
        file_path: Option<&str>,
        limit: usize,
        mode: AnchorQueryMode,
    ) -> Result<SemanticAnchorSearchOutcome, SymbolStoreError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(SemanticAnchorSearchOutcome {
                results: Vec::new(),
                empty_query: true,
                mode,
                total_terms: 0,
            });
        }

        // Normalize ONCE: whitespace-split terms kept as (original, Unicode-
        // lowercase) pairs, deduped by the lowercase form in order, capped so
        // the LIKE predicate count stays sane. Phrase mode reports a single
        // "term" (the phrase itself).
        let term_pairs: Vec<(String, String)> = {
            let mut seen = HashSet::new();
            trimmed
                .split_whitespace()
                .map(|raw| (raw.to_string(), raw.to_lowercase()))
                .filter(|(_, lower)| seen.insert(lower.clone()))
                .take(ANCHOR_QUERY_MAX_TERMS)
                .collect()
        };
        let terms: Vec<&str> = term_pairs.iter().map(|(_, lower)| lower.as_str()).collect();
        let total_terms = match mode {
            AnchorQueryMode::Phrase => 1,
            AnchorQueryMode::AllTerms | AnchorQueryMode::AnyTerms => terms.len(),
        };
        if limit == 0 {
            return Ok(SemanticAnchorSearchOutcome {
                results: Vec::new(),
                empty_query: false,
                mode,
                total_terms,
            });
        }

        // Build the WHERE clause: optional file scope + OR-joined candidate
        // patterns for EVERY mode (Phrase containment implies each of its
        // terms is contained, so per-term OR retrieval is a superset there
        // too). Both case variants are included when they render differently.
        let mut where_clauses = Vec::new();
        let mut values = Vec::new();
        if let Some(file_path) = file_path {
            where_clauses.push("file_path = ?".to_string());
            values.push(Value::Text(file_path.to_string()));
        }
        let mut like_clauses: Vec<&str> = Vec::new();
        for (original, lower) in &term_pairs {
            let mut patterns = vec![anchor_like_candidate_pattern(original)];
            let lower_pattern = anchor_like_candidate_pattern(lower);
            if lower_pattern != patterns[0] {
                patterns.push(lower_pattern);
            }
            for pattern in patterns {
                like_clauses.push(r#"(value LIKE ? ESCAPE '\' OR preview LIKE ? ESCAPE '\')"#);
                values.push(Value::Text(pattern.clone()));
                values.push(Value::Text(pattern));
            }
        }
        where_clauses.push(format!("({})", like_clauses.join(" OR ")));

        // The SQL stage no longer enforces mode semantics, so EVERY mode
        // overfetches a bounded factor beyond the limit; the ORDER BY is only
        // a bias toward likely-good candidates within that budget.
        let fetch_limit = limit.saturating_mul(ANCHOR_CANDIDATE_OVERFETCH);
        let sql = format!(
            r#"
            SELECT id, file_path, kind, value, line, character, preview, confidence,
                   owner_symbol_id, target_file_path, target_name, target_symbol_id
            FROM semantic_anchors
            WHERE {where_clause}
            ORDER BY CASE
                WHEN lower(value) = ? THEN 0
                WHEN lower(value) LIKE ? THEN 1
                ELSE 2
            END, confidence DESC, file_path, line, character
            LIMIT ?
            "#,
            where_clause = where_clauses.join(" AND ")
        );
        let trimmed_lower = trimmed.to_lowercase();
        values.push(Value::Text(trimmed_lower.clone()));
        values.push(Value::Text(format!("{}%", trimmed_lower)));
        values.push(Value::Integer(fetch_limit as i64));

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&sql)?;
        let candidates = stmt
            .query_map(params_from_iter(values), |row| row_to_semantic_anchor(row))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);

        // Mode semantics, verified in Rust over Unicode-lowercased text: the
        // SQL candidates are a superset, and `matched_terms` reports ONLY what
        // `str::contains` confirmed.
        let mut results: Vec<SemanticAnchorResult> = Vec::new();
        for anchor in candidates {
            let value_lower = anchor.value.to_lowercase();
            let preview_lower = anchor.preview.to_lowercase();
            let matched = terms
                .iter()
                .filter(|term| value_lower.contains(*term) || preview_lower.contains(*term))
                .count();
            let (score, matched_terms, total) = match mode {
                AnchorQueryMode::Phrase => {
                    if !(value_lower.contains(&trimmed_lower)
                        || preview_lower.contains(&trimmed_lower))
                    {
                        continue;
                    }
                    (semantic_anchor_score(&anchor, trimmed), None, None)
                }
                AnchorQueryMode::AllTerms => {
                    if matched < terms.len() {
                        continue;
                    }
                    (
                        semantic_anchor_score(&anchor, trimmed),
                        Some(matched),
                        Some(terms.len()),
                    )
                }
                AnchorQueryMode::AnyTerms => {
                    if matched == 0 {
                        continue;
                    }
                    let coverage = matched as f32 / terms.len().max(1) as f32;
                    (
                        semantic_anchor_score(&anchor, trimmed) * coverage,
                        Some(matched),
                        Some(terms.len()),
                    )
                }
            };
            results.push(SemanticAnchorResult {
                anchor,
                score,
                matched_terms,
                total_terms: total,
            });
        }

        // Score decides in every mode; deterministic location tie-break.
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.anchor.file_path.cmp(&b.anchor.file_path))
                .then_with(|| a.anchor.line.cmp(&b.anchor.line))
                .then_with(|| a.anchor.character.cmp(&b.anchor.character))
        });
        results.truncate(limit);

        Ok(SemanticAnchorSearchOutcome {
            results,
            empty_query: false,
            mode,
            total_terms,
        })
    }

    pub fn get_semantic_anchors_in_file(
        &self,
        file_path: &str,
        limit: usize,
    ) -> Result<Vec<SemanticAnchor>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, file_path, kind, value, line, character, preview, confidence,
                   owner_symbol_id, target_file_path, target_name, target_symbol_id
            FROM semantic_anchors
            WHERE file_path = ?1
            ORDER BY line, character, value
            LIMIT ?2
            "#,
        )?;
        let anchors = stmt
            .query_map(params![file_path, limit as i64], |row| {
                row_to_semantic_anchor(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(anchors)
    }

    pub fn replace_relationships_for_file(
        &self,
        file_path: &str,
        relationships: &[SymbolRelationship],
    ) -> Result<usize, SymbolStoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        tx.execute(
            "DELETE FROM symbol_relationships WHERE source_file_path = ?1",
            params![file_path],
        )?;

        for relationship in relationships {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO symbol_relationships
                (source_symbol_id, source_file_path, target_name, target_symbol_id, relationship_type, line, import_path, imported_name, resolution_strategy, confidence, metadata_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
                params![
                    &relationship.source_symbol_id,
                    &relationship.source_file_path,
                    &relationship.target_name,
                    relationship.target_symbol_id.as_deref(),
                    relationship.relationship_type.to_string(),
                    relationship.line,
                    relationship.import_path.as_deref(),
                    relationship.imported_name.as_deref(),
                    relationship.resolution_strategy.as_deref(),
                    relationship.confidence,
                    relationship_metadata_json(relationship),
                ],
            )?;
        }

        tx.commit()?;
        Ok(relationships.len())
    }

    pub fn replace_file_index(
        &self,
        file_path: &str,
        file_hash: &str,
        file_size: Option<u64>,
        line_count: Option<usize>,
        modified_at: Option<i64>,
        extractor_version: Option<u32>,
        symbols: &[Symbol],
        anchors: &[SemanticAnchor],
        relationships: &[SymbolRelationship],
    ) -> Result<(), SymbolStoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        tx.execute(
            "DELETE FROM symbol_relationships WHERE source_file_path = ?1",
            params![file_path],
        )?;
        tx.execute(
            "UPDATE semantic_anchors SET target_symbol_id = NULL WHERE target_file_path = ?1",
            params![file_path],
        )?;
        tx.execute(
            "DELETE FROM semantic_anchors WHERE file_path = ?1",
            params![file_path],
        )?;
        tx.execute(
            "DELETE FROM symbols WHERE file_path = ?1",
            params![file_path],
        )?;

        for symbol in symbols {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO symbols
                (id, name, qualified_name, symbol_type, file_path, start_line, start_char, end_line, end_char,
                 byte_offset, byte_length, parent_id, docstring, signature, content_hash, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
                params![
                    &symbol.id,
                    &symbol.name,
                    &symbol.qualified_name,
                    symbol.symbol_type.to_string(),
                    &symbol.file_path,
                    symbol.range.start.line,
                    symbol.range.start.character,
                    symbol.range.end.line,
                    symbol.range.end.character,
                    symbol.byte_offset as i64,
                    symbol.byte_length as i64,
                    symbol.parent_id.as_deref(),
                    symbol.docstring.as_deref(),
                    symbol.signature.as_deref(),
                    &symbol.content_hash,
                    now,
                ],
            )?;
        }

        for anchor in anchors {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO semantic_anchors
                (id, file_path, kind, value, line, character, preview, confidence,
                 owner_symbol_id, target_file_path, target_name, target_symbol_id, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
                params![
                    &anchor.id,
                    &anchor.file_path,
                    &anchor.kind,
                    &anchor.value,
                    anchor.line,
                    anchor.character,
                    &anchor.preview,
                    anchor.confidence,
                    anchor.owner_symbol_id.as_deref(),
                    anchor.target_file_path.as_deref(),
                    anchor.target_name.as_deref(),
                    anchor.target_symbol_id.as_deref(),
                    now,
                ],
            )?;
        }

        for relationship in relationships {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO symbol_relationships
                (source_symbol_id, source_file_path, target_name, target_symbol_id, relationship_type, line, import_path, imported_name, resolution_strategy, confidence, metadata_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
                params![
                    &relationship.source_symbol_id,
                    &relationship.source_file_path,
                    &relationship.target_name,
                    relationship.target_symbol_id.as_deref(),
                    relationship.relationship_type.to_string(),
                    relationship.line,
                    relationship.import_path.as_deref(),
                    relationship.imported_name.as_deref(),
                    relationship.resolution_strategy.as_deref(),
                    relationship.confidence,
                    relationship_metadata_json(relationship),
                ],
            )?;
        }

        tx.execute(
            r#"
            INSERT OR REPLACE INTO indexed_files
            (file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                file_path,
                file_hash,
                now,
                symbols.len() as i64,
                file_size.map(|size| size as i64),
                line_count.map(|count| count as i64),
                modified_at,
                extractor_version.map(i64::from)
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn replace_file_indexes(&self, files: &[FileIndexRecord]) -> Result<(), SymbolStoreError> {
        if files.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        {
            let mut delete_relationships =
                tx.prepare_cached("DELETE FROM symbol_relationships WHERE source_file_path = ?1")?;
            let mut delete_anchors =
                tx.prepare_cached("DELETE FROM semantic_anchors WHERE file_path = ?1")?;
            let mut delete_symbols =
                tx.prepare_cached("DELETE FROM symbols WHERE file_path = ?1")?;
            let mut clear_incoming_anchor_targets = tx.prepare_cached(
                "UPDATE semantic_anchors SET target_symbol_id = NULL WHERE target_file_path = ?1",
            )?;

            for file in files {
                delete_relationships.execute(params![&file.file_path])?;
                clear_incoming_anchor_targets.execute(params![&file.file_path])?;
                delete_anchors.execute(params![&file.file_path])?;
                delete_symbols.execute(params![&file.file_path])?;
            }
        }

        {
            let mut insert_symbol = tx.prepare_cached(
                r#"
                INSERT OR REPLACE INTO symbols
                (id, name, qualified_name, symbol_type, file_path, start_line, start_char, end_line, end_char,
                 byte_offset, byte_length, parent_id, docstring, signature, content_hash, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
            )?;

            for file in files {
                for symbol in &file.symbols {
                    insert_symbol.execute(params![
                        &symbol.id,
                        &symbol.name,
                        &symbol.qualified_name,
                        symbol.symbol_type.to_string(),
                        &symbol.file_path,
                        symbol.range.start.line,
                        symbol.range.start.character,
                        symbol.range.end.line,
                        symbol.range.end.character,
                        symbol.byte_offset as i64,
                        symbol.byte_length as i64,
                        symbol.parent_id.as_deref(),
                        symbol.docstring.as_deref(),
                        symbol.signature.as_deref(),
                        &symbol.content_hash,
                        now,
                    ])?;
                }
            }
        }

        {
            let mut insert_anchor = tx.prepare_cached(
                r#"
                INSERT OR REPLACE INTO semantic_anchors
                (id, file_path, kind, value, line, character, preview, confidence,
                 owner_symbol_id, target_file_path, target_name, target_symbol_id, indexed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
            )?;

            for file in files {
                for anchor in &file.anchors {
                    insert_anchor.execute(params![
                        &anchor.id,
                        &anchor.file_path,
                        &anchor.kind,
                        &anchor.value,
                        anchor.line,
                        anchor.character,
                        &anchor.preview,
                        anchor.confidence,
                        anchor.owner_symbol_id.as_deref(),
                        anchor.target_file_path.as_deref(),
                        anchor.target_name.as_deref(),
                        anchor.target_symbol_id.as_deref(),
                        now,
                    ])?;
                }
            }
        }

        {
            let mut insert_relationship = tx.prepare_cached(
                r#"
                INSERT OR REPLACE INTO symbol_relationships
                (source_symbol_id, source_file_path, target_name, target_symbol_id, relationship_type, line, import_path, imported_name, resolution_strategy, confidence, metadata_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
            )?;

            for file in files {
                for relationship in &file.relationships {
                    insert_relationship.execute(params![
                        &relationship.source_symbol_id,
                        &relationship.source_file_path,
                        &relationship.target_name,
                        relationship.target_symbol_id.as_deref(),
                        relationship.relationship_type.to_string(),
                        relationship.line,
                        relationship.import_path.as_deref(),
                        relationship.imported_name.as_deref(),
                        relationship.resolution_strategy.as_deref(),
                        relationship.confidence,
                        relationship_metadata_json(relationship),
                    ])?;
                }
            }
        }

        {
            let mut insert_indexed_file = tx.prepare_cached(
                r#"
                INSERT OR REPLACE INTO indexed_files
                (file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
            )?;

            for file in files {
                insert_indexed_file.execute(params![
                    &file.file_path,
                    &file.file_hash,
                    now,
                    file.symbols.len() as i64,
                    file.file_size.map(|size| size as i64),
                    file.line_count.map(|count| count as i64),
                    file.modified_at,
                    file.extractor_version.map(i64::from)
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn replace_relationships_for_files(
        &self,
        files: &[FileRelationshipRecord],
    ) -> Result<(), SymbolStoreError> {
        if files.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        {
            let mut delete_relationships =
                tx.prepare_cached("DELETE FROM symbol_relationships WHERE source_file_path = ?1")?;
            for file in files {
                delete_relationships.execute(params![&file.file_path])?;
            }
        }

        {
            let mut insert_relationship = tx.prepare_cached(
                r#"
                INSERT OR REPLACE INTO symbol_relationships
                (source_symbol_id, source_file_path, target_name, target_symbol_id, relationship_type, line, import_path, imported_name, resolution_strategy, confidence, metadata_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
            )?;

            for file in files {
                for relationship in &file.relationships {
                    insert_relationship.execute(params![
                        &relationship.source_symbol_id,
                        &relationship.source_file_path,
                        &relationship.target_name,
                        relationship.target_symbol_id.as_deref(),
                        relationship.relationship_type.to_string(),
                        relationship.line,
                        relationship.import_path.as_deref(),
                        relationship.imported_name.as_deref(),
                        relationship.resolution_strategy.as_deref(),
                        relationship.confidence,
                        relationship_metadata_json(relationship),
                    ])?;
                }
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// M2.4 — set-based relationship back-fill, kind-aware since Track D.
    ///
    /// Resolves `symbol_relationships.target_symbol_id` for rows that are still
    /// NULL by matching `target_name` against the GLOBAL symbol set, replacing
    /// the old per-reference `search_symbols_contextual` round-trips (the
    /// cold-index bottleneck: 81–97% of wall time). Target-driven: distinct
    /// requested `(name, relationship_type)` pairs are resolved once through a
    /// temp lookup table instead of re-evaluating correlated subqueries per
    /// relationship row (see `build_unique_relationship_targets_sql` for the
    /// guard rationale).
    ///
    /// CORRECTNESS GUARD: a target is written ONLY when the name matches
    /// EXACTLY ONE candidate symbol of a kind the relationship may legally
    /// reference (`RELATIONSHIP_TARGET_KIND_ALLOWLIST`). An ambiguous pair
    /// (`> 1` legal match) is left NULL — a wrong `target_symbol_id` silently
    /// corrupts the knowledge graph (symbol_trace / edit_impact). Already-
    /// resolved (non-NULL) rows are never overwritten, `import` edges are left
    /// to the dedicated import canonicalization, and kinds with no allowlist
    /// row (`reads_env`, `export`) are never back-filled. Candidate symbols
    /// exclude `import` placeholders, `heading` symbols (in no kind class),
    /// and the synthetic `__file__` root.
    ///
    /// Idempotent: re-running only ever turns NULL into a unique match. Returns
    /// the number of rows newly resolved.
    pub fn backfill_unresolved_relationship_targets(&self) -> Result<usize, SymbolStoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let import_resolved = resolve_import_provenance(&tx)?;
        tx.execute_batch(&build_unique_relationship_targets_sql())?;
        let resolved = tx.execute(BACKFILL_GLOBAL_UNIQUE_SQL, [])?;
        tx.execute_batch(DROP_UNIQUE_RELATIONSHIP_TARGETS_SQL)?;
        tx.commit()?;
        Ok(import_resolved.saturating_add(resolved))
    }

    /// Resolve deterministic semantic-anchor targets after their candidate
    /// symbols have been committed. Exact file-scoped references may also match
    /// normalized Markdown heading slugs; unscoped code references resolve only
    /// when one exact global name/qualified-name candidate exists.
    pub fn resolve_semantic_anchor_targets(
        &self,
        source_file_path: Option<&str>,
    ) -> Result<usize, SymbolStoreError> {
        let mut conn = self.conn.lock().unwrap();
        let global_resolved = match source_file_path {
            Some(file_path) => conn.execute(
                GLOBAL_SEMANTIC_TARGET_BACKFILL_FOR_FILE_SQL,
                params![file_path],
            )?,
            None => backfill_global_anchor_targets(&mut conn)?,
        };
        conn.execute(
            r#"
            UPDATE semantic_anchors
            SET target_file_path = (
                SELECT file_path FROM symbols WHERE id = semantic_anchors.target_symbol_id
            )
            WHERE target_symbol_id IS NOT NULL AND target_file_path IS NULL
            "#,
            [],
        )?;
        let pending = {
            let (sql, values) = match source_file_path {
                Some(file_path) => (
                    r#"
                    SELECT id, target_file_path, target_name
                    FROM semantic_anchors
                    WHERE (file_path = ?1 OR target_file_path = ?1)
                      AND target_symbol_id IS NULL
                      AND target_file_path IS NOT NULL
                      AND target_name IS NOT NULL
                      AND target_name != ''
                    "#,
                    vec![Value::Text(file_path.to_string())],
                ),
                None => (
                    r#"
                    SELECT id, target_file_path, target_name
                    FROM semantic_anchors
                    WHERE target_symbol_id IS NULL
                      AND target_file_path IS NOT NULL
                      AND target_name IS NOT NULL
                      AND target_name != ''
                    "#,
                    Vec::new(),
                ),
            };
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params_from_iter(values), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut file_candidates = HashMap::<String, Vec<SemanticTargetCandidate>>::new();
        let mut resolved = Vec::<(String, String)>::new();
        for (anchor_id, target_file_path, target_name) in pending {
            let candidate_ids = if let Some(target_file_path) = target_file_path {
                if !file_candidates.contains_key(&target_file_path) {
                    let mut stmt = conn.prepare(
                        r#"
                        SELECT id, name, qualified_name, symbol_type
                        FROM symbols
                        WHERE file_path = ?1
                          AND symbol_type != 'import'
                        ORDER BY start_line, start_char
                        "#,
                    )?;
                    let candidates = stmt
                        .query_map(params![&target_file_path], |row| {
                            Ok(SemanticTargetCandidate {
                                id: row.get(0)?,
                                name: row.get(1)?,
                                qualified_name: row.get(2)?,
                                symbol_type: row.get(3)?,
                            })
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    file_candidates.insert(target_file_path.clone(), candidates);
                }
                file_candidates
                    .get(&target_file_path)
                    .into_iter()
                    .flatten()
                    .filter(|candidate| candidate.matches(&target_name))
                    .map(|candidate| candidate.id.clone())
                    .collect::<Vec<_>>()
            } else {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT id, name, qualified_name, symbol_type
                    FROM symbols
                    WHERE (name = ?1 OR qualified_name = ?1)
                      AND symbol_type != 'import'
                      AND qualified_name != '__file__'
                    ORDER BY file_path, start_line, start_char
                    LIMIT 3
                    "#,
                )?;
                let rows = stmt.query_map(params![&target_name], |row| {
                    Ok(SemanticTargetCandidate {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        qualified_name: row.get(2)?,
                        symbol_type: row.get(3)?,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|candidate| candidate.id)
                    .collect::<Vec<_>>()
            };
            if candidate_ids.len() == 1 {
                resolved.push((anchor_id, candidate_ids[0].clone()));
            }
        }

        if resolved.is_empty() {
            return Ok(global_resolved);
        }
        let tx = conn.transaction()?;
        let mut count = 0usize;
        {
            let mut update = tx.prepare_cached(
                r#"
                UPDATE semantic_anchors
                SET target_symbol_id = ?1
                WHERE id = ?2 AND target_symbol_id IS NULL
                "#,
            )?;
            for (anchor_id, target_symbol_id) in resolved {
                count += update.execute(params![target_symbol_id, anchor_id])?;
            }
        }
        tx.commit()?;
        Ok(global_resolved + count)
    }

    /// M5.1b — GLOBAL receiver-type mining of the still-NULL call edges.
    ///
    /// Runs AFTER the per-file resolver (`resolve_relationship_targets`) and the
    /// M2.4 global-unique back-fill, over the call edges that are STILL
    /// unresolved yet carry a confident `recv_type` (persisted in
    /// `metadata_json`). Unlike M5.1's strict-superset slice this DELIBERATELY
    /// crosses the candidate-set line — it can resolve to a method that was never
    /// a same-file/imported candidate — so PRECISION is the prime directive:
    /// every step is exactly-one and any ambiguity leaves the edge NULL.
    ///
    /// PROVENANCE GATE (M5.1b reviewer blocker): ONLY edges whose recv_type came
    /// from a `self`/`this` receiver (`"recv_self":true` in `metadata_json`) are
    /// mined. Such a recv_type is the EXACT enclosing-class qualified name —
    /// guaranteed to be a project class defined in this file. A param /
    /// constructor / annotation recv_type is only a simple name that may shadow an
    /// imported library type of the same name (`from pathlib import Path` +
    /// project `class Path`), so it is DEFERRED here (it still serves M5.1's
    /// in-candidate-set disambiguation).
    ///
    /// Builds the GLOBAL cross-file registry once, then for each self-typed
    /// candidate edge looks up `(enclosingClassQn + its RESOLVED supertype chain,
    /// methodName)`, taking the MOST-DERIVED class that defines the method and
    /// resolving ONLY when that level yields EXACTLY ONE symbol.
    /// On success: `target_symbol_id` set, `resolution_strategy =
    /// 'receiver_type_global'`, `confidence = 0.7` (below M5.1 `receiver_type`'s
    /// 0.8 since it is less constrained). Only NULL call edges are ever touched;
    /// non-call edges, recv_type-less edges, non-self-typed edges, already-resolved
    /// edges, and every ambiguous lookup are left exactly as they were. Idempotent:
    /// a re-run only ever turns a NULL edge into a single confident match. Returns
    /// the number of edges newly resolved.
    pub fn mine_receiver_type_relationship_targets(&self) -> Result<usize, SymbolStoreError> {
        let mut conn = self.conn.lock().unwrap();
        let registry = build_global_receiver_registry(&conn)?;

        // The still-NULL, recv_type-carrying call edges, keyed by stable rowid.
        let candidates: Vec<(i64, String, String)> = {
            let mut stmt = conn.prepare(
                r#"
                SELECT rowid, target_name, metadata_json
                FROM symbol_relationships
                WHERE target_symbol_id IS NULL
                  AND relationship_type = 'call'
                  AND metadata_json IS NOT NULL
                  AND target_name != ''
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        // Resolve in memory first; only EXACTLY-ONE outcomes are kept.
        let mut resolved: Vec<(i64, String)> = Vec::new();
        for (rowid, method_name, metadata_json) in candidates {
            let Some((recv_type, recv_self)) = recv_meta_from_metadata(&metadata_json) else {
                continue;
            };
            // PROVENANCE GATE: only a `self`/`this`-derived recv_type (the exact
            // enclosing-class qn) is globally mined. Param/constructor recv_types
            // are deferred — they could shadow an imported library type.
            if !recv_self {
                continue;
            }
            if let Some(target_id) = registry.resolve(&recv_type, &method_name) {
                resolved.push((rowid, target_id));
            }
        }

        if resolved.is_empty() {
            return Ok(0);
        }

        // Write back under one transaction. The `target_symbol_id IS NULL` guard
        // makes the write a strict NULL→value transition (never an overwrite).
        let tx = conn.transaction()?;
        let mut count = 0usize;
        {
            let mut update = tx.prepare_cached(
                r#"
                UPDATE symbol_relationships
                SET target_symbol_id = ?1,
                    resolution_strategy = 'receiver_type_global',
                    confidence = 0.7
                WHERE rowid = ?2 AND target_symbol_id IS NULL
                "#,
            )?;
            for (rowid, target_id) in &resolved {
                count += update.execute(params![target_id, rowid])?;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// Track C — mine Go implicit interface implementations after the
    /// workspace has been indexed.
    ///
    /// Go interfaces are satisfied implicitly, so no explicit `implements`
    /// syntax can feed the graph. This derives `implements` edges by comparing
    /// canonical, receiver-independent method signatures
    /// (`canonical_go_method_signature`): a type `T` implements interface `I`
    /// when every required `(name, canonical signature)` of `I` — its own
    /// method specs plus requirements inherited through RESOLVED
    /// embedded-interface `extends` edges (memoized closure, cycle + depth-256
    /// guarded, modeled on `GlobalReceiverRegistry::resolve`) — is present in
    /// `T`'s method set.
    ///
    /// Method sets follow Go's pointer/value rule: the VALUE set holds
    /// value-receiver methods, the POINTER set holds all methods. When the
    /// VALUE set satisfies, a plain edge is emitted; when only the POINTER set
    /// satisfies, the edge carries `{"receiver_set":"pointer"}` in
    /// `metadata_json`. A `contains` edge with NO receiver metadata is treated
    /// as a value receiver (conservative-inclusive per the Track C contract —
    /// see `go_contains_receiver_is_pointer`).
    ///
    /// Precision rules:
    ///   * an unexported requirement (lowercase first char) only counts when
    ///     the type's package dir equals the DECLARING interface's package dir;
    ///   * a CROSS-PACKAGE requirement only counts when its canonical
    ///     signature is PACKAGE-PORTABLE (universe/builtin, qualified, or
    ///     composites thereof — `canonical_go_signature_is_package_portable`):
    ///     canonical strings are compared as text, and a bare `Config` in two
    ///     packages is two different types with identical text;
    ///   * two same-NAMED interfaces satisfied by one type collide on the
    ///     `(source, target_name, relationship_type, line)` PK — the schema
    ///     cannot represent both edges, so the whole colliding group is
    ///     skipped rather than silently clobbering one;
    ///   * interfaces with ZERO method specs are skipped entirely — the empty
    ///     interface matches every type, and a full `T implements interface{}`
    ///     cross-product is pure graph noise;
    ///   * an interface with an UNRESOLVED (or non-Go-interface) embed target
    ///     is skipped entirely: its true requirement set includes methods we
    ///     cannot see (an embedded `io.Reader`, a name-collision), so emitting
    ///     from the partial set would be a confident false positive — the
    ///     incompleteness propagates to every interface that embeds it;
    ///   * a `contains` edge whose method symbol cannot be located unambiguously
    ///     (resolved id, then unique `(file, name, line)`, then unique
    ///     `(file, name)`) is dropped, never guessed.
    ///
    /// All reads are batched (no per-type/per-interface SQL); the model is
    /// built in memory before the write transaction, under the same conn lock
    /// (`mine_receiver_type_relationship_targets` is the template). Idempotent
    /// recompute: one transaction DELETEs every prior
    /// `resolution_strategy = 'go_implicit_interface'` projection, then inserts
    /// the freshly derived set. Emitted edges: `source_symbol_id` = the type,
    /// `target_name`/`target_symbol_id` = the interface, `line` = the type
    /// declaration line, confidence 0.75 (below explicit-syntax languages).
    /// Returns the number of rows actually inserted (PK-colliding groups are
    /// excluded from both the writes and the count).
    pub fn mine_go_interface_implementations(&self) -> Result<usize, SymbolStoreError> {
        let mut conn = self.conn.lock().unwrap();

        // (1) Go interfaces, sorted by id for deterministic edge order.
        let mut interfaces: BTreeMap<String, GoNamedSymbol> = BTreeMap::new();
        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, name, file_path, start_line
                FROM symbols
                WHERE symbol_type = 'interface' AND file_path LIKE '%.go'
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? as u32,
                ))
            })?;
            for row in rows {
                let (id, name, file_path, line) = row?;
                let package_dir = go_package_dir(&file_path).to_string();
                interfaces.insert(
                    id,
                    GoNamedSymbol {
                        name,
                        file_path,
                        line,
                        package_dir,
                    },
                );
            }
        }
        if interfaces.is_empty() {
            // Still clear stale projections: a re-index may have removed the
            // last Go interface, and the derived set must follow it.
            let tx = conn.transaction()?;
            tx.execute(
                "DELETE FROM symbol_relationships WHERE resolution_strategy = 'go_implicit_interface'",
                [],
            )?;
            tx.commit()?;
            return Ok(0);
        }

        // (2) Interface METHOD SPECS: method children of Go interfaces, with
        // signature text `(params) result` per the Track C extractor contract.
        // Package-portability is computed ONCE per spec here (not per
        // type×interface pair in the satisfaction loop).
        let mut specs: HashMap<String, Vec<(String, String, bool)>> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                r#"
                SELECT m.parent_id, m.name, m.signature
                FROM symbols m
                JOIN symbols i ON m.parent_id = i.id
                WHERE m.symbol_type = 'method'
                  AND i.symbol_type = 'interface'
                  AND i.file_path LIKE '%.go'
                  AND m.name != ''
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?;
            for row in rows {
                let (interface_id, method_name, signature) = row?;
                let canonical =
                    canonical_go_method_signature(&method_name, signature.as_deref().unwrap_or(""));
                let package_portable = canonical_go_signature_is_package_portable(&canonical);
                specs.entry(interface_id).or_default().push((
                    method_name,
                    canonical,
                    package_portable,
                ));
            }
        }

        // (3) Embedded-interface links: `extends` edges whose SOURCE is a Go
        // interface. A resolved target that is itself a Go interface becomes an
        // embed link; a NULL or non-Go-interface target marks the source
        // INCOMPLETE — its true requirement set includes methods we cannot see
        // (e.g. an embedded `io.Reader`), so emitting `implements` from the
        // partial set would be a false positive. Incompleteness propagates
        // through the closure walk and such interfaces are skipped entirely.
        let mut embeds: HashMap<String, Vec<String>> = HashMap::new();
        let mut incomplete_interfaces: HashSet<String> = HashSet::new();
        {
            let mut stmt = conn.prepare(
                r#"
                SELECT source_symbol_id, target_symbol_id
                FROM symbol_relationships
                WHERE relationship_type = 'extends'
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?;
            for row in rows {
                let (source_id, target_id) = row?;
                if !interfaces.contains_key(&source_id) {
                    continue;
                }
                match target_id {
                    Some(target_id)
                        if target_id != source_id && interfaces.contains_key(&target_id) =>
                    {
                        embeds.entry(source_id).or_default().push(target_id);
                    }
                    Some(target_id) if target_id == source_id => {}
                    // Unresolved or non-Go-interface embed target: the
                    // requirement set is unknowable — never guess.
                    _ => {
                        incomplete_interfaces.insert(source_id);
                    }
                }
            }
        }

        // (4) Concrete Go named types (struct + named type), sorted by id.
        let mut types: BTreeMap<String, GoNamedSymbol> = BTreeMap::new();
        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, name, file_path, start_line
                FROM symbols
                WHERE symbol_type IN ('struct', 'type') AND file_path LIKE '%.go'
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? as u32,
                ))
            })?;
            for row in rows {
                let (id, name, file_path, line) = row?;
                let package_dir = go_package_dir(&file_path).to_string();
                types.insert(
                    id,
                    GoNamedSymbol {
                        name,
                        file_path,
                        line,
                        package_dir,
                    },
                );
            }
        }

        // (5) Concrete Go method symbols (interface specs excluded via their
        // interface parent), with the lookup maps the contains-edge resolution
        // fallback chain needs: id, then (file, name, line), then (file, name).
        let mut method_canonicals: HashMap<String, String> = HashMap::new();
        let mut methods_by_location: HashMap<(String, String, u32), Vec<String>> = HashMap::new();
        let mut methods_by_file_name: HashMap<(String, String), Vec<String>> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, name, file_path, start_line, signature
                FROM symbols
                WHERE symbol_type = 'method'
                  AND file_path LIKE '%.go'
                  AND (parent_id IS NULL OR parent_id NOT IN (
                        SELECT id FROM symbols WHERE symbol_type = 'interface'))
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? as u32,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?;
            for row in rows {
                let (id, name, file_path, line, signature) = row?;
                let canonical =
                    canonical_go_method_signature(&name, signature.as_deref().unwrap_or(""));
                methods_by_location
                    .entry((file_path.clone(), name.clone(), line))
                    .or_default()
                    .push(id.clone());
                methods_by_file_name
                    .entry((file_path, name))
                    .or_default()
                    .push(id.clone());
                method_canonicals.insert(id, canonical);
            }
        }

        // (6) Per-type method sets from Go `contains` edges. VALUE set =
        // value-receiver methods; POINTER set = all methods. Canonical strings
        // embed the method name, so the set element IS the (name, sig) identity.
        let mut value_sets: HashMap<String, HashSet<String>> = HashMap::new();
        let mut pointer_sets: HashMap<String, HashSet<String>> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                r#"
                SELECT source_symbol_id, source_file_path, target_name,
                       target_symbol_id, line, metadata_json
                FROM symbol_relationships
                WHERE relationship_type = 'contains' AND source_file_path LIKE '%.go'
                "#,
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)? as u32,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?;
            for row in rows {
                let (type_id, source_file, method_name, target_id, line, metadata_json) = row?;
                if !types.contains_key(&type_id) {
                    continue;
                }
                // Resolution fallback chain: resolved target id, then unique
                // (file, name, line), then unique (file, name); else drop.
                let method_id = target_id
                    .filter(|id| method_canonicals.contains_key(id))
                    .or_else(|| {
                        methods_by_location
                            .get(&(source_file.clone(), method_name.clone(), line))
                            .filter(|ids| ids.len() == 1)
                            .map(|ids| ids[0].clone())
                    })
                    .or_else(|| {
                        methods_by_file_name
                            .get(&(source_file.clone(), method_name.clone()))
                            .filter(|ids| ids.len() == 1)
                            .map(|ids| ids[0].clone())
                    });
                let Some(method_id) = method_id else {
                    continue;
                };
                let Some(canonical) = method_canonicals.get(&method_id) else {
                    continue;
                };
                pointer_sets
                    .entry(type_id.clone())
                    .or_default()
                    .insert(canonical.clone());
                if !go_contains_receiver_is_pointer(metadata_json.as_deref()) {
                    value_sets
                        .entry(type_id)
                        .or_default()
                        .insert(canonical.clone());
                }
            }
        }

        // (7) Interface requirement closures: own specs plus requirements
        // inherited through embeds. Memoized (a cached entry is always a
        // COMPLETE closure with its completeness flag, so reusing it mid-walk
        // is safe), cycle-guarded via the visited set, depth-capped at 256
        // levels like `GlobalReceiverRegistry::resolve`. The closure is
        // COMPLETE only when every reachable node is complete and the depth
        // cap never tripped — anything else means unknowable requirements and
        // the interface is skipped at emission (false negative over guessing).
        let mut requirement_memo: HashMap<String, (Vec<GoInterfaceRequirement>, bool)> =
            HashMap::new();
        for interface_id in interfaces.keys() {
            if requirement_memo.contains_key(interface_id) {
                continue;
            }
            let mut requirements: HashSet<GoInterfaceRequirement> = HashSet::new();
            let mut complete = true;
            let mut visited: HashSet<&str> = HashSet::new();
            visited.insert(interface_id);
            let mut level: Vec<&str> = vec![interface_id];
            let mut steps = 0usize;
            while !level.is_empty() {
                let mut next: Vec<&str> = Vec::new();
                for node in &level {
                    if *node != interface_id.as_str() {
                        if let Some((cached, cached_complete)) = requirement_memo.get(*node) {
                            requirements.extend(cached.iter().cloned());
                            complete &= *cached_complete;
                            continue;
                        }
                    }
                    if incomplete_interfaces.contains(*node) {
                        complete = false;
                    }
                    if let Some(own_specs) = specs.get(*node) {
                        // `interfaces` holds every node the embed filter admitted.
                        if let Some(declaring) = interfaces.get(*node) {
                            for (method_name, canonical, package_portable) in own_specs {
                                requirements.insert(GoInterfaceRequirement {
                                    method_name: method_name.clone(),
                                    canonical_signature: canonical.clone(),
                                    declaring_package: declaring.package_dir.clone(),
                                    package_portable: *package_portable,
                                });
                            }
                        }
                    }
                    if let Some(children) = embeds.get(*node) {
                        for child in children {
                            if visited.insert(child) {
                                next.push(child);
                            }
                        }
                    }
                }
                level = next;
                steps += 1;
                if steps > 256 {
                    // Depth cap tripped: the walk may have missed requirements.
                    complete = false;
                    break;
                }
            }
            let mut closure: Vec<GoInterfaceRequirement> = requirements.into_iter().collect();
            // Deterministic order so memo reuse and diagnostics are stable.
            closure.sort_by(|a, b| {
                a.method_name
                    .cmp(&b.method_name)
                    .then_with(|| a.canonical_signature.cmp(&b.canonical_signature))
                    .then_with(|| a.declaring_package.cmp(&b.declaring_package))
            });
            requirement_memo.insert(interface_id.clone(), (closure, complete));
        }

        // (8) Satisfaction. One requirement is met when its canonical signature
        // is in the method set AND — for an unexported name — the type lives in
        // the requirement's declaring package AND — across packages — the
        // signature is package-portable. Canonical strings are compared as
        // TEXT: a bare `Config` in `pkga` and a bare `Config` in `pkgb` are
        // different types with identical text, so cross-package equality is
        // only trusted when every type in the signature is universe/builtin,
        // qualified, or a composite of those (skip = false negative; emitting
        // would be a compiler-false edge).
        let requirement_satisfied = |requirement: &GoInterfaceRequirement,
                                     method_set: &HashSet<String>,
                                     type_package: &str| {
            if !method_set.contains(&requirement.canonical_signature) {
                return false;
            }
            let same_package = type_package == requirement.declaring_package;
            let exported = requirement
                .method_name
                .chars()
                .next()
                .is_some_and(char::is_uppercase);
            if !exported && !same_package {
                return false;
            }
            same_package || requirement.package_portable
        };

        let empty_set: HashSet<String> = HashSet::new();
        // (type_id, type_file, interface_name, interface_id, line, metadata).
        let mut derived: Vec<(String, String, String, String, u32, Option<&'static str>)> =
            Vec::new();
        for (type_id, type_record) in &types {
            let Some(pointer_set) = pointer_sets.get(type_id) else {
                // No methods at all: cannot satisfy any non-empty interface.
                continue;
            };
            let value_set = value_sets.get(type_id).unwrap_or(&empty_set);
            for (interface_id, interface_record) in &interfaces {
                let (requirements, complete) = &requirement_memo[interface_id];
                if !complete {
                    // The requirement set is unknowable (unresolved embed /
                    // depth cap) — never emit from a partial set.
                    continue;
                }
                if requirements.is_empty() {
                    // Empty interface (`interface{}` and spec-less fixtures)
                    // matches EVERY type — skipped entirely as pure noise.
                    continue;
                }
                let value_satisfies = requirements.iter().all(|requirement| {
                    requirement_satisfied(requirement, value_set, &type_record.package_dir)
                });
                let metadata = if value_satisfies {
                    None
                } else {
                    let pointer_satisfies = requirements.iter().all(|requirement| {
                        requirement_satisfied(requirement, pointer_set, &type_record.package_dir)
                    });
                    if !pointer_satisfies {
                        continue;
                    }
                    Some(r#"{"receiver_set":"pointer"}"#)
                };
                derived.push((
                    type_id.clone(),
                    type_record.file_path.clone(),
                    interface_record.name.clone(),
                    interface_id.clone(),
                    type_record.line,
                    metadata,
                ));
            }
        }

        // (9a) PK-collision guard. The `symbol_relationships` PK is
        // `(source_symbol_id, target_name, relationship_type, line)` — it has
        // no room for the TARGET ID, so two same-NAMED interfaces (in
        // different packages) satisfied by one type map to the SAME row and
        // `INSERT OR REPLACE` would silently keep whichever came last while
        // the return count claimed both. The schema cannot represent both
        // edges, and clobbering one is a confident wrong answer, so when a PK
        // key maps to more than one distinct target interface id the WHOLE
        // group is skipped (conservative false negative, per the handoff
        // invariant).
        let mut pk_target_ids: HashMap<(&str, &str, u32), HashSet<&str>> = HashMap::new();
        for (type_id, _, interface_name, interface_id, line, _) in &derived {
            pk_target_ids
                .entry((type_id.as_str(), interface_name.as_str(), *line))
                .or_default()
                .insert(interface_id.as_str());
        }

        // (9b) Idempotent recompute inside ONE transaction: drop every prior
        // projection, insert the fresh (collision-free) set. `count` is the
        // number of rows actually inserted, never the pre-dedup derived size.
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM symbol_relationships WHERE resolution_strategy = 'go_implicit_interface'",
            [],
        )?;
        let mut count = 0usize;
        {
            let mut insert = tx.prepare_cached(
                r#"
                INSERT OR REPLACE INTO symbol_relationships
                (source_symbol_id, source_file_path, target_name, target_symbol_id,
                 relationship_type, line, resolution_strategy, confidence, metadata_json)
                VALUES (?1, ?2, ?3, ?4, 'implements', ?5, 'go_implicit_interface', 0.75, ?6)
                "#,
            )?;
            for (type_id, type_file, interface_name, interface_id, line, metadata) in &derived {
                let colliding = pk_target_ids
                    .get(&(type_id.as_str(), interface_name.as_str(), *line))
                    .is_some_and(|targets| targets.len() > 1);
                if colliding {
                    continue;
                }
                count += insert.execute(params![
                    type_id,
                    type_file,
                    interface_name,
                    interface_id,
                    line,
                    metadata.as_deref(),
                ])?;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    pub fn get_relationship_targets(
        &self,
        source_symbol_id: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<String>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT target_name
            FROM symbol_relationships
            WHERE source_symbol_id = ?1 AND relationship_type = ?2
            ORDER BY line, target_name
            LIMIT ?3
            "#,
        )?;

        let targets = stmt
            .query_map(
                params![
                    source_symbol_id,
                    relationship_type.to_string(),
                    limit as i64
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(targets)
    }

    pub fn get_file_relationship_targets(
        &self,
        source_file_path: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<String>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT target_name
            FROM symbol_relationships
            WHERE source_file_path = ?1 AND relationship_type = ?2
            ORDER BY target_name
            LIMIT ?3
            "#,
        )?;

        let targets = stmt
            .query_map(
                params![
                    source_file_path,
                    relationship_type.to_string(),
                    limit as i64
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(targets)
    }

    pub fn module_relationship_aggregates(
        &self,
        scope_path: Option<&str>,
        relationship_types: &[SymbolRelationshipType],
        min_confidence: f32,
        limit: usize,
    ) -> Result<Vec<ModuleRelationshipAggregate>, SymbolStoreError> {
        if relationship_types.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut seen_types = HashSet::new();
        let relationship_types = relationship_types
            .iter()
            .copied()
            .filter(|relationship_type| seen_types.insert(*relationship_type))
            .collect::<Vec<_>>();
        let type_placeholders = std::iter::repeat("?")
            .take(relationship_types.len())
            .collect::<Vec<_>>()
            .join(",");
        let mut sql = format!(
            r#"
            SELECT relationship.source_file_path,
                   target.file_path,
                   relationship.relationship_type,
                   COUNT(*) AS edge_count,
                   AVG(COALESCE(
                       relationship.confidence,
                       CASE
                           WHEN relationship.relationship_type IN ('import', 'export') THEN 0.75
                           ELSE 0.5
                       END
                   )) AS average_confidence
            FROM symbol_relationships AS relationship
            JOIN symbols AS target ON target.id = relationship.target_symbol_id
            WHERE relationship.relationship_type IN ({type_placeholders})
              AND relationship.source_file_path != target.file_path
              AND COALESCE(
                    relationship.confidence,
                    CASE
                        WHEN relationship.relationship_type IN ('import', 'export') THEN 0.75
                        ELSE 0.5
                    END
                  ) >= ?
            "#,
        );
        let mut values = relationship_types
            .iter()
            .map(ToString::to_string)
            .map(Value::Text)
            .collect::<Vec<_>>();
        values.push(Value::Real(min_confidence.clamp(0.0, 1.0) as f64));
        if let Some(scope_path) = scope_path {
            sql.push_str(
                r#"
                AND (relationship.source_file_path = ? OR relationship.source_file_path LIKE ?)
                AND (target.file_path = ? OR target.file_path LIKE ?)
                "#,
            );
            let scope = scope_path.trim_end_matches('/');
            let like = format!("{scope}/%");
            values.extend([
                Value::Text(scope.to_string()),
                Value::Text(like.clone()),
                Value::Text(scope.to_string()),
                Value::Text(like),
            ]);
        }
        sql.push_str(
            r#"
            GROUP BY relationship.source_file_path, target.file_path,
                     relationship.relationship_type
            ORDER BY edge_count DESC, average_confidence DESC,
                     relationship.source_file_path, target.file_path,
                     relationship.relationship_type
            LIMIT ?
            "#,
        );
        values.push(Value::Integer(limit as i64));

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(values), |row| {
                let relationship_type = row
                    .get::<_, String>(2)?
                    .parse::<SymbolRelationshipType>()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
                        )
                    })?;
                Ok(ModuleRelationshipAggregate {
                    source_file_path: row.get(0)?,
                    target_file_path: row.get(1)?,
                    relationship_type,
                    edge_count: row.get::<_, i64>(3)? as usize,
                    average_confidence: row.get::<_, f64>(4)? as f32,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_references_to_target(
        &self,
        target_name: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<SymbolReference>, SymbolStoreError> {
        let rows = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                r#"
                SELECT s.id, s.name, s.qualified_name, s.symbol_type, s.file_path, s.start_line, s.start_char,
                       s.end_line, s.end_char, s.byte_offset, s.byte_length, s.parent_id, s.docstring, s.signature,
                       s.content_hash, r.relationship_type, r.target_name, r.target_symbol_id, r.line,
                       r.resolution_strategy, r.confidence, r.metadata_json,
                       r.import_path, r.imported_name
                FROM symbol_relationships r
                JOIN symbols s ON s.id = r.source_symbol_id
                WHERE r.target_name = ?1 AND r.relationship_type = ?2
                ORDER BY s.file_path, r.line, s.start_line, s.start_char
                LIMIT ?3
                "#,
            )?;

            let rows = stmt
                .query_map(
                    params![target_name, relationship_type.to_string(), limit as i64],
                    |row| {
                        Ok(StoredRelationshipReference {
                            source_symbol: row_to_symbol(row)?,
                            relationship_type: row.get(15)?,
                            target_name: row.get(16)?,
                            target_symbol_id: row.get(17)?,
                            line: row.get::<_, i64>(18)? as u32,
                            resolution_strategy: row.get(19)?,
                            resolution_confidence: row.get(20)?,
                            metadata_json: row.get(21)?,
                            import_path: row.get(22)?,
                            imported_name: row.get(23)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;

            rows
        };

        self.hydrate_symbol_references(rows)
    }

    pub fn find_references_to_symbol_id(
        &self,
        target_symbol_id: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<SymbolReference>, SymbolStoreError> {
        let rows = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                r#"
                SELECT s.id, s.name, s.qualified_name, s.symbol_type, s.file_path, s.start_line, s.start_char,
                       s.end_line, s.end_char, s.byte_offset, s.byte_length, s.parent_id, s.docstring, s.signature,
                       s.content_hash, r.relationship_type, r.target_name, r.target_symbol_id, r.line,
                       r.resolution_strategy, r.confidence, r.metadata_json,
                       r.import_path, r.imported_name
                FROM symbol_relationships r
                JOIN symbols s ON s.id = r.source_symbol_id
                WHERE r.target_symbol_id = ?1 AND r.relationship_type = ?2
                ORDER BY s.file_path, r.line, s.start_line, s.start_char
                LIMIT ?3
                "#,
            )?;

            let rows = stmt
                .query_map(
                    params![
                        target_symbol_id,
                        relationship_type.to_string(),
                        limit as i64
                    ],
                    |row| {
                        Ok(StoredRelationshipReference {
                            source_symbol: row_to_symbol(row)?,
                            relationship_type: row.get(15)?,
                            target_name: row.get(16)?,
                            target_symbol_id: row.get(17)?,
                            line: row.get::<_, i64>(18)? as u32,
                            resolution_strategy: row.get(19)?,
                            resolution_confidence: row.get(20)?,
                            metadata_json: row.get(21)?,
                            import_path: row.get(22)?,
                            imported_name: row.get(23)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;

            rows
        };

        self.hydrate_symbol_references(rows)
    }

    pub fn get_relationship_edges_from_source(
        &self,
        source_symbol_id: &str,
        relationship_type: SymbolRelationshipType,
        limit: usize,
    ) -> Result<Vec<SymbolReference>, SymbolStoreError> {
        let Some(source_symbol) = self.get_symbol(source_symbol_id)? else {
            return Ok(Vec::new());
        };

        let rows = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                r#"
                SELECT relationship_type, target_name, target_symbol_id, line,
                       resolution_strategy, confidence, metadata_json,
                       import_path, imported_name
                FROM symbol_relationships
                WHERE source_symbol_id = ?1 AND relationship_type = ?2
                ORDER BY line, target_name
                LIMIT ?3
                "#,
            )?;

            let rows = stmt
                .query_map(
                    params![
                        source_symbol_id,
                        relationship_type.to_string(),
                        limit as i64
                    ],
                    |row| {
                        Ok(StoredRelationshipReference {
                            source_symbol: source_symbol.clone(),
                            relationship_type: row.get(0)?,
                            target_name: row.get(1)?,
                            target_symbol_id: row.get(2)?,
                            line: row.get::<_, i64>(3)? as u32,
                            resolution_strategy: row.get(4)?,
                            resolution_confidence: row.get(5)?,
                            metadata_json: row.get(6)?,
                            import_path: row.get(7)?,
                            imported_name: row.get(8)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;

            rows
        };

        self.hydrate_symbol_references(rows)
    }

    /// Get a symbol by ID
    pub fn get_symbol(&self, id: &str) -> Result<Option<Symbol>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols WHERE id = ?1
            "#,
        )?;

        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_symbol(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get all symbols in a file
    pub fn get_symbols_in_file(&self, file_path: &str) -> Result<Vec<Symbol>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols WHERE file_path = ?1
            ORDER BY start_line, start_char
            "#,
        )?;

        let symbols = stmt
            .query_map(params![file_path], |row| row_to_symbol(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Search symbols by name (with fuzzy matching)
    pub fn search_by_name(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Symbol>, SymbolStoreError> {
        let fts_query = symbol_search_fts_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().unwrap();

        // Use FTS5 for searching
        let mut stmt = conn.prepare(
            r#"
            SELECT s.id, s.name, s.qualified_name, s.symbol_type, s.file_path, s.start_line, s.start_char,
                   s.end_line, s.end_char, s.byte_offset, s.byte_length, s.parent_id, s.docstring, s.signature, s.content_hash
            FROM symbols s
            JOIN symbols_fts fts ON s.rowid = fts.rowid
            WHERE symbols_fts MATCH ?1
            ORDER BY bm25(symbols_fts, 8.0, 5.0, 2.0, 1.0), length(s.name), s.name
            LIMIT ?2
            "#,
        )?;

        let symbols = stmt
            .query_map(params![fts_query, limit as i64], |row| row_to_symbol(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Search symbols with LIKE pattern (fallback for simple queries)
    pub fn search_by_name_like(
        &self,
        pattern: &str,
        limit: usize,
    ) -> Result<Vec<Symbol>, SymbolStoreError> {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().unwrap();
        let mut symbols = Vec::new();
        let mut seen = HashSet::new();

        append_symbol_query_results(
            &conn,
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols
            WHERE name = ?1 OR qualified_name = ?1
            ORDER BY length(name), name
            LIMIT ?2
            "#,
            vec![
                Value::Text(trimmed.to_string()),
                Value::Integer(limit as i64),
            ],
            &mut symbols,
            &mut seen,
            limit,
        )?;
        if !symbols.is_empty() {
            return Ok(symbols);
        }

        let prefix_pattern = format!("{}%", trimmed);
        append_symbol_query_results(
            &conn,
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols
            WHERE name LIKE ?1 OR qualified_name LIKE ?1
            ORDER BY CASE
                WHEN name LIKE ?2 THEN 0
                WHEN qualified_name LIKE ?2 THEN 1
                ELSE 2
            END, length(name), name
            LIMIT ?3
            "#,
            vec![
                Value::Text(prefix_pattern.clone()),
                Value::Text(prefix_pattern),
                Value::Integer(limit as i64),
            ],
            &mut symbols,
            &mut seen,
            limit,
        )?;
        if symbols.len() >= limit {
            return Ok(symbols);
        }

        let mut search_patterns = Vec::new();
        search_patterns.push(format!("%{}%", trimmed));
        for term in symbol_search_terms(trimmed) {
            let candidate = format!("%{}%", term);
            if !search_patterns
                .iter()
                .any(|existing| existing == &candidate)
            {
                search_patterns.push(candidate);
            }
        }

        let where_clause = std::iter::repeat(
            "(name LIKE ? OR qualified_name LIKE ? OR signature LIKE ? OR docstring LIKE ?)",
        )
        .take(search_patterns.len())
        .collect::<Vec<_>>()
        .join(" OR ");
        let sql = format!(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols 
            WHERE {}
            ORDER BY CASE
                WHEN lower(qualified_name) = lower(?) THEN 0
                WHEN lower(name) = lower(?) THEN 1
                WHEN lower(qualified_name) LIKE lower(?) THEN 2
                WHEN lower(name) LIKE lower(?) THEN 3
                WHEN lower(signature) LIKE lower(?) THEN 4
                WHEN lower(docstring) LIKE lower(?) THEN 5
                ELSE 6
            END, length(name), name
            LIMIT ?
            "#,
            where_clause
        );

        let mut values = Vec::with_capacity(search_patterns.len() * 4 + 7);
        for search_pattern in search_patterns {
            values.push(Value::Text(search_pattern.clone()));
            values.push(Value::Text(search_pattern.clone()));
            values.push(Value::Text(search_pattern.clone()));
            values.push(Value::Text(search_pattern));
        }
        values.push(Value::Text(trimmed.to_string()));
        values.push(Value::Text(trimmed.to_string()));
        values.push(Value::Text(format!("{}%", trimmed)));
        values.push(Value::Text(format!("{}%", trimmed)));
        values.push(Value::Text(format!("%{}%", trimmed)));
        values.push(Value::Text(format!("%{}%", trimmed)));
        values.push(Value::Integer(limit as i64));

        append_symbol_query_results(&conn, &sql, values, &mut symbols, &mut seen, limit)?;
        Ok(symbols)
    }

    /// Get symbol at a specific position in a file
    pub fn get_symbol_at(
        &self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<Symbol>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols 
            WHERE file_path = ?1
              AND start_line <= ?2 AND end_line >= ?2
              AND (start_line < ?2 OR start_char <= ?3)
              AND (end_line > ?2 OR end_char >= ?3)
            ORDER BY (end_line - start_line), (end_char - start_char)
            LIMIT 1
            "#,
        )?;

        let mut rows = stmt.query(params![file_path, line, character])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_symbol(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get symbols by type
    pub fn get_symbols_by_type(
        &self,
        symbol_type: SymbolType,
        limit: usize,
    ) -> Result<Vec<Symbol>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                   end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
            FROM symbols 
            WHERE symbol_type = ?1
            ORDER BY name
            LIMIT ?2
            "#,
        )?;

        let symbols = stmt
            .query_map(params![symbol_type.to_string(), limit as i64], |row| {
                row_to_symbol(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Delete all symbols for a file
    pub fn delete_file_symbols(&self, file_path: &str) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM symbol_relationships WHERE source_file_path = ?1",
            params![file_path],
        )?;
        conn.execute(
            "UPDATE semantic_anchors SET target_symbol_id = NULL WHERE target_file_path = ?1",
            params![file_path],
        )?;
        conn.execute(
            "DELETE FROM semantic_anchors WHERE file_path = ?1",
            params![file_path],
        )?;
        let count = conn.execute(
            "DELETE FROM symbols WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(count)
    }

    /// Delete indexing metadata for a file
    pub fn delete_indexed_file(&self, file_path: &str) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let count = conn.execute(
            "DELETE FROM indexed_files WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(count)
    }

    /// Delete generated index data only.
    ///
    /// This must not include user-authored metadata tables. If user-authored
    /// data is ever stored in this database, keep it outside
    /// `GENERATED_INDEX_CLEAR_STATEMENTS` or explicitly preserve and restore it
    /// around destructive rebuilds.
    pub fn clear_generated_index_data(&self) -> Result<(), SymbolStoreError> {
        debug_assert_eq!(
            GENERATED_INDEX_CLEAR_STATEMENTS.len(),
            GENERATED_INDEX_TABLES.len()
        );
        let conn = self.conn.lock().unwrap();
        for statement in GENERATED_INDEX_CLEAR_STATEMENTS {
            conn.execute(statement, [])?;
        }
        // M6.1 — the reconcile fast-path checkpoint summarizes the index we just
        // cleared; drop it so the next reopen re-verifies from scratch instead of
        // trusting a stale "Fresh" snapshot.
        conn.execute("DELETE FROM index_meta", [])?;
        Ok(())
    }

    /// Delete generated index data.
    pub fn clear(&self) -> Result<(), SymbolStoreError> {
        self.clear_generated_index_data()
    }

    /// Get total symbol count
    pub fn count(&self) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn list_indexed_files(
        &self,
        limit: usize,
    ) -> Result<Vec<IndexedFileRecord>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version
            FROM indexed_files
            ORDER BY symbol_count DESC, file_path ASC
            LIMIT ?1
            "#,
        )?;

        let records = stmt
            .query_map(params![limit as i64], |row| {
                indexed_file_record_from_row(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    pub fn indexed_file_record(
        &self,
        file_path: &str,
    ) -> Result<Option<IndexedFileRecord>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let record = conn
            .query_row(
                r#"
                SELECT file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version
                FROM indexed_files
                WHERE file_path = ?1
                "#,
                params![file_path],
                indexed_file_record_from_row,
            )
            .optional()?;

        Ok(record)
    }

    pub fn indexed_file_records_for_paths(
        &self,
        file_paths: &[String],
    ) -> Result<HashMap<String, IndexedFileRecord>, SymbolStoreError> {
        let unique_paths = file_paths.iter().cloned().collect::<BTreeSet<_>>();
        if unique_paths.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.conn.lock().unwrap();
        let mut records = HashMap::with_capacity(unique_paths.len());
        let unique_paths = unique_paths.into_iter().collect::<Vec<_>>();
        for chunk in unique_paths.chunks(900) {
            let placeholders = std::iter::repeat("?")
                .take(chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let mut stmt = conn.prepare(&format!(
                r#"
                SELECT file_path, file_hash, indexed_at, symbol_count, file_size,
                       line_count, modified_at, extractor_version
                FROM indexed_files
                WHERE file_path IN ({placeholders})
                "#,
            ))?;
            let rows = stmt
                .query_map(params_from_iter(chunk.iter()), indexed_file_record_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            for record in rows {
                records.insert(record.file_path.clone(), record);
            }
        }
        Ok(records)
    }

    pub fn list_all_indexed_files(&self) -> Result<Vec<IndexedFileRecord>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version
            FROM indexed_files
            ORDER BY file_path ASC
            "#,
        )?;

        let records = stmt
            .query_map([], |row| indexed_file_record_from_row(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    /// Get count of indexed files
    pub fn file_count(&self) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(DISTINCT file_path) FROM symbols", [], |row| {
                row.get(0)
            })?;
        Ok(count as usize)
    }

    /// M6.1 — number of rows in `indexed_files`. A cheap COUNT the reconcile
    /// fast path uses to detect out-of-band index mutations that leave disk
    /// mtimes untouched.
    pub fn indexed_file_count(&self) -> Result<usize, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM indexed_files", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// M6.1 — read a value from the `index_meta` key/value cache.
    pub fn get_index_meta(&self, key: &str) -> Result<Option<String>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row(
                "SELECT value FROM index_meta WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(value)
    }

    /// M6.1 — upsert a value into the `index_meta` key/value cache.
    pub fn set_index_meta(&self, key: &str, value: &str) -> Result<(), SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO index_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// M6.1 — delete a value from the `index_meta` key/value cache.
    pub fn delete_index_meta(&self, key: &str) -> Result<(), SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM index_meta WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// Record file as indexed
    pub fn mark_file_indexed(
        &self,
        file_path: &str,
        file_hash: &str,
        symbol_count: usize,
    ) -> Result<(), SymbolStoreError> {
        self.mark_file_indexed_with_metadata(file_path, file_hash, symbol_count, None, None, None)
    }

    pub fn mark_file_indexed_with_metadata(
        &self,
        file_path: &str,
        file_hash: &str,
        symbol_count: usize,
        file_size: Option<u64>,
        line_count: Option<usize>,
        modified_at: Option<i64>,
    ) -> Result<(), SymbolStoreError> {
        self.mark_file_indexed_with_metadata_and_extractor_version(
            file_path,
            file_hash,
            symbol_count,
            file_size,
            line_count,
            modified_at,
            None,
        )
    }

    pub fn mark_file_indexed_with_metadata_and_extractor_version(
        &self,
        file_path: &str,
        file_hash: &str,
        symbol_count: usize,
        file_size: Option<u64>,
        line_count: Option<usize>,
        modified_at: Option<i64>,
        extractor_version: Option<u32>,
    ) -> Result<(), SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            r#"
            INSERT OR REPLACE INTO indexed_files
            (file_path, file_hash, indexed_at, symbol_count, file_size, line_count, modified_at, extractor_version)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                file_path,
                file_hash,
                now,
                symbol_count as i64,
                file_size.map(|size| size as i64),
                line_count.map(|count| count as i64),
                modified_at,
                extractor_version.map(i64::from)
            ],
        )?;
        Ok(())
    }

    /// Check if file needs reindexing
    pub fn needs_reindex(
        &self,
        file_path: &str,
        file_hash: &str,
    ) -> Result<bool, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let result: Option<String> = conn
            .query_row(
                "SELECT file_hash FROM indexed_files WHERE file_path = ?1",
                params![file_path],
                |row| row.get(0),
            )
            .ok();

        match result {
            Some(stored_hash) => Ok(stored_hash != file_hash),
            None => Ok(true), // Not indexed yet
        }
    }

    pub fn needs_reindex_for_metadata(
        &self,
        file_path: &str,
        file_size: u64,
        modified_at: i64,
    ) -> Result<Option<bool>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let result: Option<(Option<i64>, Option<i64>)> = conn
            .query_row(
                "SELECT file_size, modified_at FROM indexed_files WHERE file_path = ?1",
                params![file_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        match result {
            Some((Some(stored_size), Some(stored_modified_at))) => Ok(Some(
                stored_size != file_size as i64 || stored_modified_at != modified_at,
            )),
            Some(_) => Ok(None),
            None => Ok(Some(true)),
        }
    }

    pub fn relationship_integrity_stats(
        &self,
    ) -> Result<RelationshipIntegrityStats, SymbolStoreError> {
        self.relationship_integrity_stats_for_scope(None)
    }

    pub fn relationship_integrity_stats_for_scope(
        &self,
        scope_path: Option<&str>,
    ) -> Result<RelationshipIntegrityStats, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        let scope_like = scope_path.map(|path| format!("{path}/%"));
        let scoped = scope_path.zip(scope_like.as_deref());

        let total_relationships = query_scoped_count(
            &conn,
            "SELECT COUNT(*) FROM symbol_relationships",
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships
            WHERE source_file_path = ?1 OR source_file_path LIKE ?2
            "#,
            scoped,
        )?;
        let resolved_relationships = query_scoped_count(
            &conn,
            "SELECT COUNT(*) FROM symbol_relationships WHERE target_symbol_id IS NOT NULL",
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships
            WHERE target_symbol_id IS NOT NULL
              AND (source_file_path = ?1 OR source_file_path LIKE ?2)
            "#,
            scoped,
        )?;
        let unresolved_symbol_relationships = query_scoped_count(
            &conn,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships
            WHERE target_symbol_id IS NULL AND relationship_type <> 'import'
            "#,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships
            WHERE target_symbol_id IS NULL
              AND relationship_type <> 'import'
              AND (source_file_path = ?1 OR source_file_path LIKE ?2)
            "#,
            scoped,
        )?;
        let missing_source_symbols = query_scoped_count(
            &conn,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships r
            LEFT JOIN symbols s ON s.id = r.source_symbol_id
            WHERE s.id IS NULL
            "#,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships r
            LEFT JOIN symbols s ON s.id = r.source_symbol_id
            WHERE s.id IS NULL
              AND (r.source_file_path = ?1 OR r.source_file_path LIKE ?2)
            "#,
            scoped,
        )?;
        let missing_target_symbols = query_scoped_count(
            &conn,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships r
            LEFT JOIN symbols s ON s.id = r.target_symbol_id
            WHERE r.target_symbol_id IS NOT NULL AND s.id IS NULL
            "#,
            r#"
            SELECT COUNT(*)
            FROM symbol_relationships r
            LEFT JOIN symbols s ON s.id = r.target_symbol_id
            WHERE r.target_symbol_id IS NOT NULL
              AND s.id IS NULL
              AND (r.source_file_path = ?1 OR r.source_file_path LIKE ?2)
            "#,
            scoped,
        )?;
        let by_type = {
            match scoped {
                Some((scope, like)) => {
                    let mut stmt = conn.prepare(
                        r#"
                        SELECT relationship_type,
                               COUNT(*),
                               SUM(CASE WHEN target_symbol_id IS NOT NULL THEN 1 ELSE 0 END),
                               SUM(CASE WHEN target_symbol_id IS NULL AND relationship_type <> 'import' THEN 1 ELSE 0 END)
                        FROM symbol_relationships
                        WHERE source_file_path = ?1 OR source_file_path LIKE ?2
                        GROUP BY relationship_type
                        ORDER BY COUNT(*) DESC, relationship_type ASC
                        "#,
                    )?;
                    let rows = stmt.query_map(params![scope, like], |row| {
                        Ok(RelationshipTypeStats {
                            relationship_type: row.get(0)?,
                            total_relationships: row.get::<_, i64>(1)? as usize,
                            resolved_relationships: row.get::<_, i64>(2)? as usize,
                            unresolved_symbol_relationships: row.get::<_, i64>(3)? as usize,
                        })
                    })?;
                    rows.collect::<Result<Vec<_>, _>>()?
                }
                None => {
                    let mut stmt = conn.prepare(
                        r#"
                        SELECT relationship_type,
                               COUNT(*),
                               SUM(CASE WHEN target_symbol_id IS NOT NULL THEN 1 ELSE 0 END),
                               SUM(CASE WHEN target_symbol_id IS NULL AND relationship_type <> 'import' THEN 1 ELSE 0 END)
                        FROM symbol_relationships
                        GROUP BY relationship_type
                        ORDER BY COUNT(*) DESC, relationship_type ASC
                        "#,
                    )?;
                    let rows = stmt.query_map([], |row| {
                        Ok(RelationshipTypeStats {
                            relationship_type: row.get(0)?,
                            total_relationships: row.get::<_, i64>(1)? as usize,
                            resolved_relationships: row.get::<_, i64>(2)? as usize,
                            unresolved_symbol_relationships: row.get::<_, i64>(3)? as usize,
                        })
                    })?;
                    rows.collect::<Result<Vec<_>, _>>()?
                }
            }
        };
        let top_unresolved_targets = {
            match scoped {
                Some((scope, like)) => {
                    let mut stmt = conn.prepare(
                        r#"
                        SELECT relationship_type, target_name, COUNT(*), MIN(source_file_path)
                        FROM symbol_relationships
                        WHERE target_symbol_id IS NULL
                          AND relationship_type <> 'import'
                          AND (source_file_path = ?1 OR source_file_path LIKE ?2)
                        GROUP BY relationship_type, target_name
                        ORDER BY COUNT(*) DESC, relationship_type ASC, target_name ASC
                        LIMIT 12
                        "#,
                    )?;
                    let rows = stmt.query_map(params![scope, like], |row| {
                        Ok(UnresolvedRelationshipTarget {
                            relationship_type: row.get(0)?,
                            target_name: row.get(1)?,
                            count: row.get::<_, i64>(2)? as usize,
                            example_source_file: row.get(3)?,
                        })
                    })?;
                    rows.collect::<Result<Vec<_>, _>>()?
                }
                None => {
                    let mut stmt = conn.prepare(
                        r#"
                        SELECT relationship_type, target_name, COUNT(*), MIN(source_file_path)
                        FROM symbol_relationships
                        WHERE target_symbol_id IS NULL AND relationship_type <> 'import'
                        GROUP BY relationship_type, target_name
                        ORDER BY COUNT(*) DESC, relationship_type ASC, target_name ASC
                        LIMIT 12
                        "#,
                    )?;
                    let rows = stmt.query_map([], |row| {
                        Ok(UnresolvedRelationshipTarget {
                            relationship_type: row.get(0)?,
                            target_name: row.get(1)?,
                            count: row.get::<_, i64>(2)? as usize,
                            example_source_file: row.get(3)?,
                        })
                    })?;
                    rows.collect::<Result<Vec<_>, _>>()?
                }
            }
        };

        Ok(RelationshipIntegrityStats {
            total_relationships: total_relationships as usize,
            resolved_relationships: resolved_relationships as usize,
            unresolved_symbol_relationships: unresolved_symbol_relationships as usize,
            missing_source_symbols: missing_source_symbols as usize,
            missing_target_symbols: missing_target_symbols as usize,
            by_type,
            top_unresolved_targets,
        })
    }

    /// Per-language × per-kind matrix of symbol and relationship counts, with an
    /// explicit `0` row back-filled for every `SymbolType` / `SymbolRelationshipType`
    /// that did not appear for a language, plus the per-language "% of
    /// Function/Method symbols carrying a non-null signature".
    ///
    /// Read-only diagnostic; does not change any extraction behavior.
    //
    // NOTE: language derived from file path until M2.3 adds a real `language` column; re-base then.
    pub fn coverage_histogram(&self) -> Result<CoverageHistogram, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();

        // language -> (symbol_type string -> count)
        let mut symbol_counts: BTreeMap<String, HashMap<String, u64>> = BTreeMap::new();
        // language -> (function+method total, function+method with non-empty signature)
        let mut signature_totals: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        // language -> (relationship_type string -> count)
        let mut relationship_counts: BTreeMap<String, HashMap<String, u64>> = BTreeMap::new();

        let function_key = SymbolType::Function.to_string();
        let method_key = SymbolType::Method.to_string();

        {
            let mut stmt = conn.prepare("SELECT file_path, symbol_type, signature FROM symbols")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let file_path: String = row.get(0)?;
                let symbol_type: String = row.get(1)?;
                let signature: Option<String> = row.get(2)?;
                let language = coverage_language_label(&file_path);

                *symbol_counts
                    .entry(language.clone())
                    .or_default()
                    .entry(symbol_type.clone())
                    .or_insert(0) += 1;

                if symbol_type == function_key || symbol_type == method_key {
                    let entry = signature_totals.entry(language).or_insert((0, 0));
                    entry.0 += 1;
                    if signature.is_some_and(|sig| !sig.trim().is_empty()) {
                        entry.1 += 1;
                    }
                }
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT source_file_path, relationship_type FROM symbol_relationships")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let file_path: String = row.get(0)?;
                let relationship_type: String = row.get(1)?;
                let language = coverage_language_label(&file_path);

                *relationship_counts
                    .entry(language)
                    .or_default()
                    .entry(relationship_type)
                    .or_insert(0) += 1;
            }
        }

        // Union of every language that produced a symbol or a relationship.
        let mut languages: BTreeSet<String> = BTreeSet::new();
        languages.extend(symbol_counts.keys().cloned());
        languages.extend(relationship_counts.keys().cloned());

        let per_language = languages
            .into_iter()
            .map(|language| {
                let observed_symbols = symbol_counts.get(&language);
                // Back-fill an explicit 0 for every SymbolType variant that is absent.
                let symbol_counts = ALL_SYMBOL_TYPES
                    .iter()
                    .map(|kind| {
                        let key = kind.to_string();
                        let count = observed_symbols
                            .and_then(|counts| counts.get(&key))
                            .copied()
                            .unwrap_or(0);
                        (key, count)
                    })
                    .collect();

                let observed_relationships = relationship_counts.get(&language);
                // Back-fill an explicit 0 for every relationship variant that is absent.
                let relationship_counts = ALL_RELATIONSHIP_TYPES
                    .iter()
                    .map(|relationship_type| {
                        let key = relationship_type.to_string();
                        let count = observed_relationships
                            .and_then(|counts| counts.get(&key))
                            .copied()
                            .unwrap_or(0);
                        (key, count)
                    })
                    .collect();

                let function_method_signature_pct = match signature_totals.get(&language) {
                    Some((total, with_signature)) if *total > 0 => {
                        *with_signature as f64 / *total as f64
                    }
                    _ => 0.0,
                };

                LanguageCoverage {
                    language,
                    symbol_counts,
                    relationship_counts,
                    function_method_signature_pct,
                }
            })
            .collect();

        Ok(CoverageHistogram { per_language })
    }

    pub fn symbol_type_counts(&self) -> Result<Vec<(String, usize)>, SymbolStoreError> {
        self.symbol_type_counts_for_scope(None)
    }

    pub fn symbol_type_counts_for_scope(
        &self,
        scope_path: Option<&str>,
    ) -> Result<Vec<(String, usize)>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        match scope_path {
            Some(scope) => {
                let like = format!("{scope}/%");
                let mut stmt = conn.prepare(
                    r#"
                    SELECT symbol_type, COUNT(*)
                    FROM symbols
                    WHERE file_path = ?1 OR file_path LIKE ?2
                    GROUP BY symbol_type
                    ORDER BY COUNT(*) DESC, symbol_type ASC
                    "#,
                )?;
                let rows = stmt
                    .query_map(params![scope, like], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
            None => {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT symbol_type, COUNT(*)
                    FROM symbols
                    GROUP BY symbol_type
                    ORDER BY COUNT(*) DESC, symbol_type ASC
                    "#,
                )?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
        }
    }

    pub fn semantic_anchor_kind_counts(&self) -> Result<Vec<(String, usize)>, SymbolStoreError> {
        self.semantic_anchor_kind_counts_for_scope(None)
    }

    pub fn semantic_anchor_kind_counts_for_scope(
        &self,
        scope_path: Option<&str>,
    ) -> Result<Vec<(String, usize)>, SymbolStoreError> {
        let conn = self.conn.lock().unwrap();
        match scope_path {
            Some(scope) => {
                let like = format!("{scope}/%");
                let mut stmt = conn.prepare(
                    r#"
                    SELECT kind, COUNT(*)
                    FROM semantic_anchors
                    WHERE file_path = ?1 OR file_path LIKE ?2
                    GROUP BY kind
                    ORDER BY COUNT(*) DESC, kind ASC
                    "#,
                )?;
                let rows = stmt
                    .query_map(params![scope, like], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
            None => {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT kind, COUNT(*)
                    FROM semantic_anchors
                    GROUP BY kind
                    ORDER BY COUNT(*) DESC, kind ASC
                    "#,
                )?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            }
        }
    }

    fn hydrate_symbol_references(
        &self,
        rows: Vec<StoredRelationshipReference>,
    ) -> Result<Vec<SymbolReference>, SymbolStoreError> {
        // M2.4 — hydrate target symbols with ONE batched lookup keyed on the
        // distinct target ids, instead of N+1 `get_symbol` round-trips (one per
        // edge). The map below is the JOIN, resolved in a single query.
        let target_symbols = self.get_symbols_by_ids(
            rows.iter()
                .filter_map(|row| row.target_symbol_id.as_deref()),
        )?;

        let mut references = Vec::with_capacity(rows.len());

        for row in rows {
            let relationship_type = row
                .relationship_type
                .parse::<SymbolRelationshipType>()
                .map_err(|error| {
                    SymbolStoreError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        error,
                    ))
                })?;
            let target_symbol = row
                .target_symbol_id
                .as_deref()
                .and_then(|id| target_symbols.get(id).cloned());
            let (receiver_type, receiver_is_self) = row
                .metadata_json
                .as_deref()
                .and_then(recv_meta_from_metadata)
                .map(|(receiver_type, receiver_is_self)| (Some(receiver_type), receiver_is_self))
                .unwrap_or((None, false));

            references.push(SymbolReference {
                source_symbol: row.source_symbol,
                relationship_type,
                target_name: row.target_name,
                target_symbol_id: row.target_symbol_id,
                target_symbol,
                line: row.line,
                observation_kind: RelationshipObservationKind::SyntaxExtracted,
                resolution_strategy: row.resolution_strategy,
                resolution_confidence: row.resolution_confidence,
                receiver_type,
                receiver_is_self,
                import_path: row.import_path,
                imported_name: row.imported_name,
            });
        }

        Ok(references)
    }

    pub fn get_semantic_context_for_symbol_ids(
        &self,
        symbol_ids: &[String],
        limit: usize,
    ) -> Result<Vec<SemanticAnchor>, SymbolStoreError> {
        if symbol_ids.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let unique_ids = symbol_ids.iter().cloned().collect::<BTreeSet<_>>();
        let conn = self.conn.lock().unwrap();
        let mut anchors = Vec::new();
        let unique_ids = unique_ids.into_iter().collect::<Vec<_>>();
        for chunk in unique_ids.chunks(900) {
            if anchors.len() >= limit {
                break;
            }
            let placeholders = std::iter::repeat("?")
                .take(chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                r#"
                SELECT id, file_path, kind, value, line, character, preview, confidence,
                       owner_symbol_id, target_file_path, target_name, target_symbol_id
                FROM semantic_anchors
                WHERE owner_symbol_id IN ({placeholders})
                  AND kind IN ('rationale', 'documentation_link', 'design_reference',
                               'design_symbol_reference')
                ORDER BY confidence DESC, file_path, line, character
                LIMIT ?
                "#,
            );
            let mut values = chunk.iter().cloned().map(Value::Text).collect::<Vec<_>>();
            values.push(Value::Integer(limit.saturating_sub(anchors.len()) as i64));
            let mut stmt = conn.prepare(&sql)?;
            anchors.extend(
                stmt.query_map(params_from_iter(values), row_to_semantic_anchor)?
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        anchors.truncate(limit);
        Ok(anchors)
    }

    /// Fetch the given symbol ids in a single query, returning an `id -> Symbol`
    /// map. Ids are de-duplicated and chunked under SQLite's bound-parameter
    /// ceiling so one logical lookup replaces N `get_symbol` calls.
    fn get_symbols_by_ids<'a, I>(&self, ids: I) -> Result<HashMap<String, Symbol>, SymbolStoreError>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let unique_ids: Vec<String> = ids
            .into_iter()
            .collect::<BTreeSet<&str>>()
            .into_iter()
            .map(str::to_string)
            .collect();
        if unique_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let conn = self.conn.lock().unwrap();
        let mut symbols = HashMap::with_capacity(unique_ids.len());
        // Stay well under SQLite's default 999 bound-parameter limit per query.
        for chunk in unique_ids.chunks(900) {
            let placeholders = std::iter::repeat("?")
                .take(chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let mut stmt = conn.prepare(&format!(
                r#"
                SELECT id, name, qualified_name, symbol_type, file_path, start_line, start_char,
                       end_line, end_char, byte_offset, byte_length, parent_id, docstring, signature, content_hash
                FROM symbols WHERE id IN ({placeholders})
                "#,
            ))?;
            let chunk_symbols = stmt
                .query_map(params_from_iter(chunk.iter()), row_to_symbol)?
                .collect::<Result<Vec<_>, _>>()?;
            for symbol in chunk_symbols {
                symbols.insert(symbol.id.clone(), symbol);
            }
        }

        Ok(symbols)
    }
}

fn indexed_file_record_from_row(row: &rusqlite::Row) -> rusqlite::Result<IndexedFileRecord> {
    Ok(IndexedFileRecord {
        file_path: row.get::<_, String>(0)?,
        file_hash: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        indexed_at: row.get::<_, i64>(2)?,
        symbol_count: row.get::<_, i64>(3)? as usize,
        file_size: row
            .get::<_, Option<i64>>(4)?
            .and_then(|size| u64::try_from(size).ok()),
        line_count: row
            .get::<_, Option<i64>>(5)?
            .and_then(|count| usize::try_from(count).ok()),
        modified_at: row.get::<_, Option<i64>>(6)?,
        extractor_version: row
            .get::<_, Option<i64>>(7)?
            .and_then(|version| u32::try_from(version).ok()),
    })
}

/// Convert a database row to a Symbol
fn row_to_semantic_anchor(row: &rusqlite::Row) -> rusqlite::Result<SemanticAnchor> {
    Ok(SemanticAnchor {
        id: row.get(0)?,
        file_path: row.get(1)?,
        kind: row.get(2)?,
        value: row.get(3)?,
        line: row.get::<_, i64>(4)? as u32,
        character: row.get::<_, i64>(5)? as u32,
        preview: row.get(6)?,
        confidence: row.get::<_, f64>(7)? as f32,
        owner_symbol_id: row.get(8)?,
        target_file_path: row.get(9)?,
        target_name: row.get(10)?,
        target_symbol_id: row.get(11)?,
    })
}

fn semantic_anchor_score(anchor: &SemanticAnchor, query: &str) -> f32 {
    let value = anchor.value.to_lowercase();
    let query = query.to_lowercase();
    if value == query {
        1.0
    } else if value.starts_with(&query) {
        0.9 * anchor.confidence
    } else if value.contains(&query) {
        0.75 * anchor.confidence
    } else {
        0.45 * anchor.confidence
    }
}

struct SemanticTargetCandidate {
    id: String,
    name: String,
    qualified_name: String,
    symbol_type: String,
}

impl SemanticTargetCandidate {
    fn matches(&self, target_name: &str) -> bool {
        self.name.eq_ignore_ascii_case(target_name)
            || self.qualified_name.eq_ignore_ascii_case(target_name)
            || self.symbol_type == "heading"
                && semantic_heading_slug(&self.name).eq_ignore_ascii_case(target_name)
    }
}

fn semantic_heading_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '_' {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(character);
        } else if character.is_whitespace() || character == '-' {
            pending_dash = true;
        }
    }
    slug
}

fn row_to_symbol(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
    use crate::tree_sitter::{Position, Range};

    let symbol_type_str: String = row.get(3)?;
    let symbol_type = symbol_type_str
        .parse::<SymbolType>()
        .unwrap_or(SymbolType::Function);

    Ok(Symbol {
        id: row.get(0)?,
        name: row.get(1)?,
        qualified_name: row.get(2)?,
        symbol_type,
        file_path: row.get(4)?,
        range: Range {
            start: Position {
                line: row.get::<_, i32>(5)? as u32,
                character: row.get::<_, i32>(6)? as u32,
            },
            end: Position {
                line: row.get::<_, i32>(7)? as u32,
                character: row.get::<_, i32>(8)? as u32,
            },
        },
        byte_offset: row.get::<_, i64>(9)? as usize,
        byte_length: row.get::<_, i64>(10)? as usize,
        parent_id: row.get(11)?,
        docstring: row.get(12)?,
        signature: row.get(13)?,
        content_hash: row.get(14)?,
    })
}

fn append_symbol_query_results(
    conn: &Connection,
    sql: &str,
    values: Vec<Value>,
    symbols: &mut Vec<Symbol>,
    seen: &mut HashSet<String>,
    limit: usize,
) -> Result<(), SymbolStoreError> {
    if symbols.len() >= limit {
        return Ok(());
    }

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params_from_iter(values), |row| row_to_symbol(row))?
        .collect::<Result<Vec<_>, _>>()?;

    for symbol in rows {
        if seen.insert(symbol.id.clone()) {
            symbols.push(symbol);
            if symbols.len() >= limit {
                break;
            }
        }
    }

    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table_name: &str,
    column_name: &str,
    definition: &str,
) -> Result<(), SymbolStoreError> {
    let pragma = format!("PRAGMA table_info({})", table_name);
    let mut stmt = conn.prepare(&pragma)?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;

    if !columns.iter().any(|existing| existing == column_name) {
        let alter = format!(
            "ALTER TABLE {} ADD COLUMN {} {}",
            table_name, column_name, definition
        );
        conn.execute(&alter, [])?;
    }

    Ok(())
}

/// Latest schema version this binary knows how to produce.
///
/// Equal to `MIGRATIONS.len()`. A freshly-migrated database ends with
/// `PRAGMA user_version` set to this value.
// Currently only asserted by the migration tests; the next column-adding
// milestone (M2.4) will read it from production code.
#[cfg_attr(not(test), allow(dead_code))]
const LATEST_SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// Ordered, **append-only** schema migrations keyed on `PRAGMA user_version`.
///
/// `MIGRATIONS[n]` migrates the database from `user_version = n` to
/// `user_version = n + 1` (so `MIGRATIONS[0]` takes v0 → v1). NEVER edit,
/// reorder, or remove a shipped step — only append new ones. This framework
/// owns NEW schema going forward; the legacy `ensure_column` probes in
/// `create_schema` keep owning the CURRENT columns (converting them is riskier
/// than letting the two coexist), so the two intentionally overlap with no
/// conflict.
///
/// N7 — no column without an in-phase consumer: v1 adds the three relationship
/// provenance columns consumed by set-based resolution; v2 adds the four
/// semantic-owner/target columns consumed by rationale/document extraction and
/// graph-context queries. Wrong or ambiguous targets stay NULL and auditable.
///
/// DEFERRED per N7 (do NOT add here): the other columns from the plan's M2.3
/// list — `language`, `is_lexical`, `return_type`, `visibility`, `modifiers` on
/// `symbols`, and the `routes` table — are deferred to their consuming
/// milestones (M0.2 re-base / M4.x). They land as new appended migration steps
/// when (and only when) their consumer ships.
const MIGRATIONS: &[fn(&Connection) -> Result<(), SymbolStoreError>] = &[
    migration_v1_relationship_resolution_columns,
    migration_v2_semantic_anchor_links,
    migration_v3_drop_redundant_relationship_source_index,
    migration_v4_import_binding_provenance,
    migration_v5_expand_symbol_fts,
];

/// Apply every pending migration step, advancing `PRAGMA user_version`.
///
/// Reads the current `user_version`, then runs each not-yet-applied step from
/// `MIGRATIONS` inside its own transaction, bumping `user_version` in the same
/// transaction so the version can never run ahead of the schema. Idempotent:
/// once `user_version == LATEST_SCHEMA_VERSION` this is a no-op, so running it
/// repeatedly (e.g. on every `SymbolStore::new`) is safe.
fn run_migrations(conn: &Connection) -> Result<(), SymbolStoreError> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    while (version as usize) < MIGRATIONS.len() {
        let step = MIGRATIONS[version as usize];
        // `unchecked_transaction` works on `&Connection`; on any early return
        // (the `?` below) the guard drops and rolls the step back, so a failed
        // migration never leaves `user_version` ahead of the real schema.
        let tx = conn.unchecked_transaction()?;
        step(&tx)?;
        let next = version + 1;
        // `user_version` is an integer we fully control and cannot be bound as a
        // parameter, so format it directly; it is transactional with the DDL above.
        tx.execute_batch(&format!("PRAGMA user_version = {next}"))?;
        tx.commit()?;
        version = next;
    }

    Ok(())
}

/// Migration v1: add the relationship-resolution audit columns M2.4 consumes.
///
/// `ensure_column` pragma-checks `table_info` before issuing `ALTER TABLE … ADD
/// COLUMN`, which makes each add idempotent: re-running this migration, or
/// running it on a DB where a prior ad-hoc `ensure_column` already added one of
/// these columns, is a safe no-op (no "duplicate column name" error).
fn migration_v1_relationship_resolution_columns(conn: &Connection) -> Result<(), SymbolStoreError> {
    ensure_column(conn, "symbol_relationships", "resolution_strategy", "TEXT")?;
    ensure_column(conn, "symbol_relationships", "confidence", "REAL")?;
    ensure_column(conn, "symbol_relationships", "metadata_json", "TEXT")?;
    Ok(())
}

/// Migration v2: attach semantic evidence to its owning symbol and retain
/// deterministic document/symbol link targets without fabricating graph edges.
fn migration_v2_semantic_anchor_links(conn: &Connection) -> Result<(), SymbolStoreError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS semantic_anchors (
            id TEXT PRIMARY KEY,
            file_path TEXT NOT NULL,
            kind TEXT NOT NULL,
            value TEXT NOT NULL,
            line INTEGER NOT NULL,
            character INTEGER NOT NULL,
            preview TEXT NOT NULL,
            confidence REAL NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        "#,
    )?;
    ensure_column(conn, "semantic_anchors", "owner_symbol_id", "TEXT")?;
    ensure_column(conn, "semantic_anchors", "target_file_path", "TEXT")?;
    ensure_column(conn, "semantic_anchors", "target_name", "TEXT")?;
    ensure_column(conn, "semantic_anchors", "target_symbol_id", "TEXT")?;
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_semantic_anchors_owner ON semantic_anchors(owner_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_semantic_anchors_target_file ON semantic_anchors(target_file_path);
        CREATE INDEX IF NOT EXISTS idx_semantic_anchors_target_symbol ON semantic_anchors(target_symbol_id);
        "#,
    )?;
    Ok(())
}

/// Migration v3: drop the redundant `symbol_relationships(source_symbol_id)`
/// index. The table's composite PRIMARY KEY `(source_symbol_id, target_name,
/// relationship_type, line)` materializes
/// `sqlite_autoindex_symbol_relationships_1`, whose leftmost prefix already
/// serves every `source_symbol_id` lookup — the explicit index only duplicated
/// it (write amplification on every relationship insert plus dead pages).
/// `IF EXISTS` because a fresh database never creates it (removed from the
/// base schema in the same change). Freed pages are recycled by SQLite, not
/// returned to the filesystem — deliberately no `VACUUM` here, as it rewrites
/// the entire file and this runs on every open of a possibly-huge index.
fn migration_v3_drop_redundant_relationship_source_index(
    conn: &Connection,
) -> Result<(), SymbolStoreError> {
    conn.execute_batch("DROP INDEX IF EXISTS idx_symbol_relationships_source")?;
    Ok(())
}

/// Migration v4: retain the ES-module specifier and exported name that
/// introduced a local JS/TS call target. These columns make aliased import
/// resolution provenance-based instead of a repository-wide name guess.
fn migration_v4_import_binding_provenance(conn: &Connection) -> Result<(), SymbolStoreError> {
    ensure_column(conn, "symbol_relationships", "import_path", "TEXT")?;
    ensure_column(conn, "symbol_relationships", "imported_name", "TEXT")?;
    Ok(())
}

/// Migration v5: index qualified names and signatures alongside names and
/// docstrings. Rebuilding the disposable FTS projection keeps the authoritative
/// `symbols` table and stable IDs unchanged.
fn migration_v5_expand_symbol_fts(conn: &Connection) -> Result<(), SymbolStoreError> {
    conn.execute_batch(
        r#"
        DROP TRIGGER IF EXISTS symbols_ai;
        DROP TRIGGER IF EXISTS symbols_ad;
        DROP TRIGGER IF EXISTS symbols_au;
        DROP TABLE IF EXISTS symbols_fts;

        CREATE VIRTUAL TABLE symbols_fts USING fts5(
            name,
            qualified_name,
            signature,
            docstring,
            content=symbols,
            content_rowid=rowid,
            tokenize='unicode61'
        );

        CREATE TRIGGER symbols_ai AFTER INSERT ON symbols BEGIN
            INSERT INTO symbols_fts(rowid, name, qualified_name, signature, docstring)
            VALUES (new.rowid, new.name, new.qualified_name, new.signature, new.docstring);
        END;

        CREATE TRIGGER symbols_ad AFTER DELETE ON symbols BEGIN
            INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, signature, docstring)
            VALUES ('delete', old.rowid, old.name, old.qualified_name, old.signature, old.docstring);
        END;

        CREATE TRIGGER symbols_au AFTER UPDATE ON symbols BEGIN
            INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, signature, docstring)
            VALUES ('delete', old.rowid, old.name, old.qualified_name, old.signature, old.docstring);
            INSERT INTO symbols_fts(rowid, name, qualified_name, signature, docstring)
            VALUES (new.rowid, new.name, new.qualified_name, new.signature, new.docstring);
        END;

        INSERT INTO symbols_fts(symbols_fts) VALUES ('rebuild');
        "#,
    )?;
    Ok(())
}

fn symbol_search_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .map(str::trim)
        .filter(|term| term.len() >= 2)
        .map(ToString::to_string)
        .collect()
}

fn symbol_search_fts_query(query: &str) -> String {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for term in symbol_search_terms(query) {
        let term = term.to_lowercase();
        if seen.insert(term.clone()) {
            terms.push(format!("{}*", term));
        }
    }
    terms.join(" OR ")
}

/// Error type for symbol store operations
#[derive(Debug)]
pub enum SymbolStoreError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for SymbolStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolStoreError::Sqlite(e) => write!(f, "SQLite error: {}", e),
            SymbolStoreError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for SymbolStoreError {}

impl From<rusqlite::Error> for SymbolStoreError {
    fn from(err: rusqlite::Error) -> Self {
        SymbolStoreError::Sqlite(err)
    }
}

impl From<std::io::Error> for SymbolStoreError {
    fn from(err: std::io::Error) -> Self {
        SymbolStoreError::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter::{Position, Range};

    fn create_test_symbol(name: &str, file_path: &str) -> Symbol {
        Symbol {
            id: format!("{}::{}#function", file_path, name),
            name: name.to_string(),
            qualified_name: name.to_string(),
            symbol_type: SymbolType::Function,
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
            docstring: Some("Test function".to_string()),
            signature: Some("(param: string): void".to_string()),
            content_hash: "hash".to_string(),
        }
    }

    /// The pre-target-driven correlated statements, kept VERBATIM as the
    /// reference semantics for `target_driven_backfill_matches_legacy_correlated_backfill`.
    /// NOTE: the legacy form is kind-UNAWARE. The fixture below is deliberately
    /// kind-compatible (every resolvable edge is a `call` and every candidate a
    /// `function`), so the legacy oracle still pins the uniqueness/exclusion
    /// semantics; the Track D kind-aware divergences are covered by the
    /// dedicated `kind_aware_backfill_*` tests.
    const LEGACY_BACKFILL_GLOBAL_UNIQUE_SQL: &str = r#"
UPDATE symbol_relationships
SET target_symbol_id = (
        SELECT s.id
        FROM symbols s
        WHERE s.name = symbol_relationships.target_name
          AND s.name != ''
          AND s.symbol_type != 'import'
          AND s.qualified_name != '__file__'
    ),
    resolution_strategy = 'global_unique',
    confidence = 0.5
WHERE target_symbol_id IS NULL
  AND relationship_type != 'import'
  AND symbol_relationships.target_name != ''
  AND (
        SELECT COUNT(*)
        FROM symbols s2
        WHERE s2.name = symbol_relationships.target_name
          AND s2.name != ''
          AND s2.symbol_type != 'import'
          AND s2.qualified_name != '__file__'
      ) = 1
"#;

    const LEGACY_GLOBAL_SEMANTIC_TARGET_BACKFILL_SQL: &str = r#"
UPDATE semantic_anchors AS anchor
SET target_symbol_id = (
    SELECT symbol.id
    FROM symbols AS symbol
    WHERE (symbol.name = anchor.target_name OR symbol.qualified_name = anchor.target_name)
      AND symbol.symbol_type != 'import'
      AND symbol.qualified_name != '__file__'
    LIMIT 1
)
WHERE anchor.target_file_path IS NULL
  AND anchor.target_symbol_id IS NULL
  AND anchor.target_name IS NOT NULL
  AND anchor.target_name != ''
  AND (
    SELECT COUNT(*)
    FROM symbols AS candidate
    WHERE (candidate.name = anchor.target_name OR candidate.qualified_name = anchor.target_name)
      AND candidate.symbol_type != 'import'
      AND candidate.qualified_name != '__file__'
  ) = 1
"#;

    /// Every discriminating case the global back-fills must handle: a unique
    /// name, an ambiguous name, import-only and `__file__`-only candidates, an
    /// `import`-type edge, an already-resolved edge, an empty target name, a
    /// qualified-name anchor match, and a name that is unique for relationships
    /// (name-only match) but ambiguous for anchors (name OR qualified-name).
    fn seed_backfill_fixture(store: &SymbolStore) {
        let conn = store.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            INSERT INTO symbols (id, name, qualified_name, symbol_type, file_path,
                                 start_line, start_char, end_line, end_char, indexed_at)
            VALUES
                ('s_unique',     'unique_fn',   'mod.unique_fn',   'function', 'a.py', 0, 0, 0, 0, 0),
                ('s_dup1',       'dup_fn',      'mod1.dup_fn',     'function', 'a.py', 0, 0, 0, 0, 0),
                ('s_dup2',       'dup_fn',      'mod2.dup_fn',     'function', 'b.py', 0, 0, 0, 0, 0),
                ('s_import',     'imported_fn', 'mod.imported_fn', 'import',   'a.py', 0, 0, 0, 0, 0),
                ('s_root',       'root_fn',     '__file__',        'function', 'a.py', 0, 0, 0, 0, 0),
                ('s_qn',         'qn_fn',       'pkg.qn_fn',       'function', 'c.py', 0, 0, 0, 0, 0),
                ('s_cross_name', 'crossy',      'mod.crossy',      'function', 'd.py', 0, 0, 0, 0, 0),
                ('s_cross_qn',   'other_fn',    'crossy',          'function', 'e.py', 0, 0, 0, 0, 0);

            INSERT INTO symbol_relationships
                (source_symbol_id, source_file_path, target_name, target_symbol_id, relationship_type, line)
            VALUES
                ('r_src', 'x.py', 'unique_fn',   NULL,     'call',   1),
                ('r_src', 'x.py', 'dup_fn',      NULL,     'call',   2),
                ('r_src', 'x.py', 'imported_fn', NULL,     'call',   3),
                ('r_src', 'x.py', 'root_fn',     NULL,     'call',   4),
                ('r_src', 'x.py', 'unique_fn',   NULL,     'import', 5),
                ('r_src', 'x.py', 'unique_fn',   'preset', 'use',    6),
                ('r_src', 'x.py', 'crossy',      NULL,     'call',   7),
                ('r_src', 'x.py', '',            NULL,     'call',   8);

            INSERT INTO semantic_anchors
                (id, file_path, kind, value, line, character, preview, confidence,
                 target_file_path, target_name, target_symbol_id, indexed_at)
            VALUES
                ('a1', 'doc.md', 'ref', 'v', 1, 0, 'p', 1.0, NULL,        'unique_fn',   NULL, 0),
                ('a2', 'doc.md', 'ref', 'v', 2, 0, 'p', 1.0, NULL,        'pkg.qn_fn',   NULL, 0),
                ('a3', 'doc.md', 'ref', 'v', 3, 0, 'p', 1.0, NULL,        'dup_fn',      NULL, 0),
                ('a4', 'doc.md', 'ref', 'v', 4, 0, 'p', 1.0, NULL,        'crossy',      NULL, 0),
                ('a5', 'doc.md', 'ref', 'v', 5, 0, 'p', 1.0, 'target.py', 'unique_fn',   NULL, 0),
                ('a6', 'doc.md', 'ref', 'v', 6, 0, 'p', 1.0, NULL,        NULL,          NULL, 0),
                ('a7', 'doc.md', 'ref', 'v', 7, 0, 'p', 1.0, NULL,        'imported_fn', NULL, 0);
            "#,
        )
        .unwrap();
    }

    type RelationshipResolutionRow = (String, i64, Option<String>, Option<String>, Option<f64>);

    fn dump_relationship_resolutions(store: &SymbolStore) -> Vec<RelationshipResolutionRow> {
        let conn = store.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT target_name, line, target_symbol_id, resolution_strategy, confidence
                 FROM symbol_relationships ORDER BY target_name, line",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap();
        rows.collect::<Result<Vec<_>, _>>().unwrap()
    }

    fn dump_anchor_resolutions(store: &SymbolStore) -> Vec<(String, Option<String>)> {
        let conn = store.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, target_symbol_id FROM semantic_anchors ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap();
        rows.collect::<Result<Vec<_>, _>>().unwrap()
    }

    /// The target-driven back-fills must be row-for-row identical to the legacy
    /// correlated statements, and each fixture case must land where the
    /// precision rules say (so both implementations being wrong together
    /// cannot pass).
    #[test]
    fn target_driven_backfill_matches_legacy_correlated_backfill() {
        let legacy = SymbolStore::in_memory().unwrap();
        let current = SymbolStore::in_memory().unwrap();
        seed_backfill_fixture(&legacy);
        seed_backfill_fixture(&current);

        let (legacy_relationships, legacy_anchors) = {
            let conn = legacy.conn.lock().unwrap();
            (
                conn.execute(LEGACY_BACKFILL_GLOBAL_UNIQUE_SQL, []).unwrap(),
                conn.execute(LEGACY_GLOBAL_SEMANTIC_TARGET_BACKFILL_SQL, [])
                    .unwrap(),
            )
        };

        let current_relationships = current.backfill_unresolved_relationship_targets().unwrap();
        let current_anchors = {
            let mut conn = current.conn.lock().unwrap();
            backfill_global_anchor_targets(&mut conn).unwrap()
        };

        assert_eq!(current_relationships, legacy_relationships);
        assert_eq!(current_anchors, legacy_anchors);
        assert_eq!(
            dump_relationship_resolutions(&current),
            dump_relationship_resolutions(&legacy)
        );
        assert_eq!(
            dump_anchor_resolutions(&current),
            dump_anchor_resolutions(&legacy)
        );

        // Explicit expected outcomes on the new implementation.
        assert_eq!(current_relationships, 2);
        let relationships: HashMap<(String, i64), (Option<String>, Option<String>)> =
            dump_relationship_resolutions(&current)
                .into_iter()
                .map(|(name, line, target, strategy, _)| ((name, line), (target, strategy)))
                .collect();
        let expect = |name: &str, line: i64| relationships[&(name.to_string(), line)].clone();
        assert_eq!(
            expect("unique_fn", 1),
            (
                Some("s_unique".to_string()),
                Some("global_unique".to_string())
            )
        );
        assert_eq!(
            expect("crossy", 7),
            (
                Some("s_cross_name".to_string()),
                Some("global_unique".to_string())
            )
        );
        assert_eq!(expect("dup_fn", 2), (None, None));
        assert_eq!(expect("imported_fn", 3), (None, None));
        assert_eq!(expect("root_fn", 4), (None, None));
        assert_eq!(expect("unique_fn", 5), (None, None)); // import edge skipped
        assert_eq!(expect("unique_fn", 6), (Some("preset".to_string()), None)); // never overwritten
        assert_eq!(expect("", 8), (None, None));

        assert_eq!(current_anchors, 2);
        let anchors: HashMap<String, Option<String>> =
            dump_anchor_resolutions(&current).into_iter().collect();
        assert_eq!(anchors["a1"], Some("s_unique".to_string()));
        assert_eq!(anchors["a2"], Some("s_qn".to_string())); // qualified-name match
        assert_eq!(anchors["a3"], None); // ambiguous by name
        assert_eq!(anchors["a4"], None); // ambiguous across name + qualified name
        assert_eq!(anchors["a5"], None); // target_file_path already set: untouched
        assert_eq!(anchors["a6"], None); // no target name
        assert_eq!(anchors["a7"], None); // only candidate is an import placeholder

        // Idempotent: a second run resolves nothing and changes nothing.
        let before = dump_relationship_resolutions(&current);
        assert_eq!(
            current.backfill_unresolved_relationship_targets().unwrap(),
            0
        );
        assert_eq!(dump_relationship_resolutions(&current), before);
    }

    /// The composite PRIMARY KEY's autoindex must serve `source_symbol_id`
    /// lookups on its own: a fresh schema carries no duplicate explicit index,
    /// and the query planner picks the autoindex prefix.
    #[test]
    fn relationship_source_lookup_uses_primary_key_autoindex() {
        let store = SymbolStore::in_memory().unwrap();
        let conn = store.conn.lock().unwrap();

        let indexes: Vec<String> = conn
            .prepare("PRAGMA index_list(symbol_relationships)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            !indexes
                .iter()
                .any(|name| name == "idx_symbol_relationships_source"),
            "redundant index present: {indexes:?}"
        );

        let plan = conn
            .prepare(
                "EXPLAIN QUERY PLAN SELECT * FROM symbol_relationships WHERE source_symbol_id = ?1",
            )
            .unwrap()
            .query_map(["symbol-id"], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join(" ");
        assert!(
            plan.contains("sqlite_autoindex_symbol_relationships_1"),
            "source lookup does not use the PK autoindex: {plan}"
        );
    }

    /// Migration v3 removes the legacy duplicate index from a database created
    /// before this version (the base schema used to CREATE it on every open).
    #[test]
    fn migration_v3_drops_legacy_relationship_source_index() {
        let store = SymbolStore::in_memory().unwrap();
        let conn = store.conn.lock().unwrap();
        // Recreate the pre-v3 state: duplicate index present, version rewound.
        conn.execute_batch(
            "CREATE INDEX idx_symbol_relationships_source ON symbol_relationships(source_symbol_id);
             PRAGMA user_version = 2;",
        )
        .unwrap();

        run_migrations(&conn).unwrap();

        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_symbol_relationships_source'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    /// M5.13 — `checkpoint()` must fold the WAL back into the main DB and truncate
    /// the `-wal` sidecar to zero (the 469k-file Firefox index otherwise left a
    /// 560 MB stranded WAL).
    #[test]
    fn checkpoint_truncates_wal() {
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("symbols.db");
        let store = SymbolStore::new(&db_path).unwrap();
        let symbols: Vec<Symbol> = (0..2000)
            .map(|i| create_test_symbol(&format!("fn_{i}"), "src/lib.rs"))
            .collect();
        store.upsert_symbols(&symbols).unwrap();

        let wal_path = dir.path().join("symbols.db-wal");
        store.checkpoint().unwrap();
        let wal_len = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
        assert_eq!(
            wal_len, 0,
            "WAL sidecar should be truncated to 0 after checkpoint, was {wal_len} bytes"
        );
    }

    fn create_test_relationship(
        source_symbol_id: &str,
        file_path: &str,
        target_name: &str,
    ) -> SymbolRelationship {
        SymbolRelationship {
            source_symbol_id: source_symbol_id.to_string(),
            source_file_path: file_path.to_string(),
            target_name: target_name.to_string(),
            target_symbol_id: None,
            relationship_type: SymbolRelationshipType::Call,
            line: 3,
            ..Default::default()
        }
    }

    #[test]
    fn test_create_store() {
        let store = SymbolStore::in_memory().unwrap();
        assert_eq!(store.count().unwrap(), 0);
    }

    /// M2.4 correctness gate: the set-based back-fill resolves a relationship
    /// ONLY when its `target_name` matches exactly one symbol globally. A name
    /// shared by symbols in different files is ambiguous and must stay NULL — a
    /// wrong `target_symbol_id` silently corrupts the knowledge graph.
    #[test]
    fn backfill_resolves_unique_targets_but_never_ambiguous_names() {
        let store = SymbolStore::in_memory().unwrap();

        // "Shared" exists in TWO different files (ambiguous); "OnlyOne" exists
        // in exactly one file (unambiguous).
        store
            .upsert_symbols(&[
                create_test_symbol("Shared", "a.ts"),
                create_test_symbol("Shared", "b.ts"),
                create_test_symbol("OnlyOne", "c.ts"),
                create_test_symbol("caller", "caller.py"),
            ])
            .unwrap();

        let caller_id = "caller.py::caller#function";
        store
            .replace_relationships_for_file(
                "caller.py",
                &[
                    create_test_relationship(caller_id, "caller.py", "Shared"),
                    create_test_relationship(caller_id, "caller.py", "OnlyOne"),
                ],
            )
            .unwrap();

        // Both edges start NULL; exactly one (the unique "OnlyOne") resolves.
        let resolved = store.backfill_unresolved_relationship_targets().unwrap();
        assert_eq!(resolved, 1);

        let conn = store.conn.lock().unwrap();

        // Ambiguous name → still NULL, untagged (no wrong back-fill).
        let (shared_target, shared_strategy): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT target_symbol_id, resolution_strategy
                 FROM symbol_relationships WHERE target_name = 'Shared'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(
            shared_target.is_none(),
            "ambiguous name must NOT be back-filled, got {shared_target:?}"
        );
        assert!(shared_strategy.is_none());

        // Unique name → resolved to the single matching symbol and tagged.
        let (unique_target, unique_strategy, unique_confidence): (
            Option<String>,
            Option<String>,
            Option<f64>,
        ) = conn
            .query_row(
                "SELECT target_symbol_id, resolution_strategy, confidence
                 FROM symbol_relationships WHERE target_name = 'OnlyOne'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(unique_target.as_deref(), Some("c.ts::OnlyOne#function"));
        assert_eq!(unique_strategy.as_deref(), Some("global_unique"));
        assert_eq!(unique_confidence, Some(0.5));
    }

    #[test]
    fn import_provenance_resolves_alias_to_the_declared_module() {
        let store = SymbolStore::in_memory().unwrap();
        let caller = create_test_symbol("Page", "src/app/page.tsx");
        let mut intended = create_test_symbol("createStore", "src/shared/ui.ts");
        intended.symbol_type = SymbolType::Constant;
        let mut duplicate = create_test_symbol("createStore", "src/other/ui.ts");
        duplicate.symbol_type = SymbolType::Constant;

        store
            .upsert_symbols(&[caller.clone(), intended.clone(), duplicate])
            .unwrap();
        store
            .replace_relationships_for_file(
                "src/app/page.tsx",
                &[SymbolRelationship {
                    source_symbol_id: caller.id.clone(),
                    source_file_path: caller.file_path.clone(),
                    target_name: "useStore".to_string(),
                    target_symbol_id: None,
                    relationship_type: SymbolRelationshipType::Call,
                    line: 4,
                    import_path: Some("@/shared/ui".to_string()),
                    imported_name: Some("createStore".to_string()),
                    ..Default::default()
                }],
            )
            .unwrap();

        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 1);
        let (target, strategy, confidence): (Option<String>, Option<String>, Option<f64>) = {
            let conn = store.conn.lock().unwrap();
            conn.query_row(
                "SELECT target_symbol_id, resolution_strategy, confidence
                 FROM symbol_relationships WHERE source_symbol_id = ?1",
                params![caller.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
        };
        assert_eq!(target, Some(intended.id));
        assert_eq!(strategy.as_deref(), Some("import_binding"));
        assert_eq!(confidence, Some(0.9));

        let references = store
            .get_relationship_edges_from_source(&caller.id, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].import_path.as_deref(), Some("@/shared/ui"));
        assert_eq!(references[0].imported_name.as_deref(), Some("createStore"));
    }

    #[test]
    fn javascript_family_calls_never_use_global_unique_fallback() {
        let store = SymbolStore::in_memory().unwrap();
        let caller = create_test_symbol("run", "src/browser.ts");
        let unrelated = create_test_symbol("register", "src/server.rs");
        store
            .upsert_symbols(&[caller.clone(), unrelated])
            .unwrap();
        store
            .replace_relationships_for_file(
                "src/browser.ts",
                &[create_test_relationship(
                    &caller.id,
                    &caller.file_path,
                    "register",
                )],
            )
            .unwrap();

        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 0);
        let target: Option<String> = {
            let conn = store.conn.lock().unwrap();
            conn.query_row(
                "SELECT target_symbol_id FROM symbol_relationships WHERE source_symbol_id = ?1",
                params![caller.id],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(target, None);
    }

    #[test]
    fn import_path_matching_supports_relative_root_alias_and_index_modules() {
        assert!(candidate_path_matches_import(
            "src/app/page.tsx",
            "../shared/ui",
            "src/shared/ui.ts"
        ));
        assert!(candidate_path_matches_import(
            "src/app/page.tsx",
            "@/shared/ui",
            "src/shared/ui.ts"
        ));
        assert!(candidate_path_matches_import(
            "src/app/page.tsx",
            "~/shared/ui",
            "src/shared/ui/index.ts"
        ));
        assert!(!candidate_path_matches_import(
            "src/app/page.tsx",
            "@/shared/ui",
            "src/other/ui.ts"
        ));
    }

    /// The back-fill never overwrites an already-resolved (non-NULL) target and
    /// is idempotent on a second run.
    #[test]
    fn backfill_preserves_existing_targets_and_is_idempotent() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                create_test_symbol("OnlyOne", "c.ts"),
                create_test_symbol("decoy", "d.ts"),
                create_test_symbol("caller", "caller.ts"),
            ])
            .unwrap();

        let caller_id = "caller.ts::caller#function";
        // Pre-resolved to a deliberately "wrong" id: the back-fill must leave it.
        let mut pinned = create_test_relationship(caller_id, "caller.ts", "OnlyOne");
        pinned.target_symbol_id = Some("d.ts::decoy#function".to_string());
        store
            .replace_relationships_for_file("caller.ts", &[pinned])
            .unwrap();

        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 0);
        // Second run is also a no-op.
        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 0);

        let conn = store.conn.lock().unwrap();
        let target: Option<String> = conn
            .query_row(
                "SELECT target_symbol_id FROM symbol_relationships WHERE target_name = 'OnlyOne'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(target.as_deref(), Some("d.ts::decoy#function"));
    }

    // ---- M5.1b GLOBAL receiver-type mining (`receiver_global` / `null_mining`) ----
    //
    // These drive `mine_receiver_type_relationship_targets` directly over a
    // hand-built symbol+edge set so the registry shape, cross-file inheritance,
    // and the precision gates are tested in isolation (no extraction coupling).
    // PRECISION is the prime directive: a wrong, confidently-tagged resolution is
    // the exact bug class these guard against — every negative MUST stay NULL.

    /// A class-like symbol with an explicit qualified name (so simple-name
    /// collisions can be modelled by giving two classes distinct qns).
    fn mining_class(qn: &str, name: &str, file: &str) -> Symbol {
        Symbol {
            id: format!("{file}::{qn}#class"),
            name: name.to_string(),
            qualified_name: qn.to_string(),
            symbol_type: SymbolType::Class,
            file_path: file.to_string(),
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

    /// A method symbol parented to the class whose qn is `class_qn`.
    fn mining_method(method: &str, class_qn: &str, file: &str) -> Symbol {
        Symbol {
            id: format!("{file}::{class_qn}.{method}#method"),
            name: method.to_string(),
            qualified_name: format!("{class_qn}.{method}"),
            symbol_type: SymbolType::Method,
            file_path: file.to_string(),
            range: Range {
                start: Position {
                    line: 2,
                    character: 4,
                },
                end: Position {
                    line: 3,
                    character: 4,
                },
            },
            byte_offset: 0,
            byte_length: 0,
            parent_id: Some(format!("{file}::{class_qn}#class")),
            docstring: None,
            signature: None,
            content_hash: "hash".to_string(),
        }
    }

    /// A `self`/`this`-derived (still-NULL) Call edge: inside a method on
    /// `enclosing_class_qn`, `self.method()` — `recv_type` IS that exact enclosing
    /// class qn and `recv_self` is set, so the GLOBAL miner is eligible to mine it.
    fn mining_self_call(
        source_id: &str,
        file: &str,
        method: &str,
        enclosing_class_qn: &str,
    ) -> SymbolRelationship {
        SymbolRelationship {
            source_symbol_id: source_id.to_string(),
            source_file_path: file.to_string(),
            target_name: method.to_string(),
            target_symbol_id: None,
            relationship_type: SymbolRelationshipType::Call,
            line: 5,
            recv_type: Some(enclosing_class_qn.to_string()),
            recv_self: true,
            ..Default::default()
        }
    }

    /// A param/constructor/annotation-derived (still-NULL) Call edge: the receiver
    /// type is only a SIMPLE NAME that may shadow an imported library type, so
    /// `recv_self` is false and the GLOBAL miner MUST DEFER it (provenance gate).
    fn mining_typed_call(
        source_id: &str,
        file: &str,
        method: &str,
        recv_type: &str,
    ) -> SymbolRelationship {
        SymbolRelationship {
            source_symbol_id: source_id.to_string(),
            source_file_path: file.to_string(),
            target_name: method.to_string(),
            target_symbol_id: None,
            relationship_type: SymbolRelationshipType::Call,
            line: 5,
            recv_type: Some(recv_type.to_string()),
            recv_self: false,
            ..Default::default()
        }
    }

    /// An inheritance (Extends) edge `sub_class_id` → `super_name`. `super_id` is
    /// the RESOLVED target symbol id — the inheritance gate follows ONLY resolved
    /// edges, so pass `None` to model an UNRESOLVED edge (e.g. a library base whose
    /// simple name merely collides with a project class) that must NOT be followed.
    fn mining_extends(
        sub_class_id: &str,
        file: &str,
        super_name: &str,
        super_id: Option<&str>,
    ) -> SymbolRelationship {
        SymbolRelationship {
            source_symbol_id: sub_class_id.to_string(),
            source_file_path: file.to_string(),
            target_name: super_name.to_string(),
            target_symbol_id: super_id.map(|id| id.to_string()),
            relationship_type: SymbolRelationshipType::Extends,
            line: 1,
            ..Default::default()
        }
    }

    /// Read one edge's `(target_symbol_id, resolution_strategy, confidence)`.
    fn edge_resolution(
        store: &SymbolStore,
        source_id: &str,
        target_name: &str,
    ) -> (Option<String>, Option<String>, Option<f64>) {
        let conn = store.conn.lock().unwrap();
        conn.query_row(
            "SELECT target_symbol_id, resolution_strategy, confidence
             FROM symbol_relationships
             WHERE source_symbol_id = ?1 AND target_name = ?2",
            params![source_id, target_name],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
    }

    /// THE CROSS-FILE WIN: `B extends A` (in another file, a RESOLVED inheritance
    /// edge), and inside `B.go` a `self`-typed receiver calls `base`, which is
    /// defined ONLY on `A`. The name `base` is deliberately NON-unique (a decoy
    /// `C.base` exists) so the M2.4 name-only back-fill could never resolve it —
    /// only the supertype walk can. Mining resolves it to `A.base`, tagged
    /// `receiver_type_global` / 0.7, and is idempotent.
    #[test]
    fn receiver_global_self_cross_file_inheritance_resolves_supertype_method() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                mining_class("A", "A", "a.py"),
                mining_method("base", "A", "a.py"),
                mining_class("B", "B", "b.py"),
                mining_method("go", "B", "b.py"),
                // Decoy: makes `base` ambiguous by name so only typing can resolve.
                mining_class("C", "C", "c.py"),
                mining_method("base", "C", "c.py"),
            ])
            .unwrap();

        let b_go = "b.py::B.go#method";
        store
            .replace_relationships_for_file(
                "b.py",
                &[
                    // RESOLVED extends B → A (target pinned to A's class symbol).
                    mining_extends("b.py::B#class", "b.py", "A", Some("a.py::A#class")),
                    // `self.base()` inside B.go — self-typed, recv_type is B's qn.
                    mining_self_call(b_go, "b.py", "base", "B"),
                ],
            )
            .unwrap();

        assert_eq!(
            store.mine_receiver_type_relationship_targets().unwrap(),
            1,
            "self.base() in B must resolve via the B→A supertype walk"
        );

        let (target, strategy, confidence) = edge_resolution(&store, b_go, "base");
        assert_eq!(
            target.as_deref(),
            Some("a.py::A.base#method"),
            "self.base() on a B receiver must resolve to the inherited A.base, not C.base"
        );
        assert_eq!(strategy.as_deref(), Some("receiver_type_global"));
        assert_eq!(confidence, Some(0.7));

        // Idempotent: a second pass turns nothing new (the edge is non-NULL now).
        assert_eq!(store.mine_receiver_type_relationship_targets().unwrap(), 0);
    }

    /// MOST-DERIVED WINS: `B extends A` (resolved), both define `run`; a `self`
    /// receiver inside `B` resolves to `B.run` (the override), deterministically —
    /// never `A.run`.
    #[test]
    fn receiver_global_most_derived_override_beats_inherited() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                mining_class("A", "A", "a.py"),
                mining_method("run", "A", "a.py"),
                mining_class("B", "B", "b.py"),
                mining_method("run", "B", "b.py"),
                mining_method("go", "B", "b.py"),
            ])
            .unwrap();

        let b_go = "b.py::B.go#method";
        store
            .replace_relationships_for_file(
                "b.py",
                &[
                    mining_extends("b.py::B#class", "b.py", "A", Some("a.py::A#class")),
                    mining_self_call(b_go, "b.py", "run", "B"),
                ],
            )
            .unwrap();

        assert_eq!(store.mine_receiver_type_relationship_targets().unwrap(), 1);
        let (target, _, _) = edge_resolution(&store, b_go, "run");
        assert_eq!(
            target.as_deref(),
            Some("b.py::B.run#method"),
            "the most-derived B.run override must win over the inherited A.run"
        );
    }

    /// PROVENANCE GATE (the M5.1b reviewer blocker, isolated): the SAME project
    /// class + method + recv_type NAME resolves ONLY through a `self` receiver, and
    /// NEVER through a param/constructor receiver — because the latter's simple
    /// name could shadow an imported library type. Two `Foo`-receiver calls of
    /// `act`: the self-typed one resolves to `Foo.act`; the param-typed one (even
    /// though `Foo` is an exact project class name) stays NULL.
    #[test]
    fn null_mining_non_self_typed_recv_is_deferred() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                mining_class("Foo", "Foo", "foo.py"),
                mining_method("act", "Foo", "foo.py"),
                create_test_symbol("user", "user.py"),
            ])
            .unwrap();

        let foo_method = "foo.py::Foo.act#method"; // a self call FROM inside Foo
        let user = "user.py::user#function"; // a param-typed call from elsewhere
        store
            .replace_relationships_for_file(
                "foo.py",
                &[mining_self_call(foo_method, "foo.py", "act", "Foo")],
            )
            .unwrap();
        store
            .replace_relationships_for_file(
                "user.py",
                // `def user(f: Foo): f.act()` — recv_type `Foo` from a param/import.
                &[mining_typed_call(user, "user.py", "act", "Foo")],
            )
            .unwrap();

        assert_eq!(
            store.mine_receiver_type_relationship_targets().unwrap(),
            1,
            "only the self-typed edge is eligible; the param-typed edge is deferred"
        );
        assert_eq!(
            edge_resolution(&store, foo_method, "act").0.as_deref(),
            Some("foo.py::Foo.act#method"),
            "the self.act() edge must mine-resolve to Foo.act"
        );
        assert!(
            edge_resolution(&store, user, "act").0.is_none(),
            "a param/constructor recv_type matching a project class name must NOT be mined"
        );
    }

    /// NEGATIVE (a) — REVIEWER'S EXACT SCENARIO, library-typed PARAM collision:
    /// `from pathlib import Path` … `def use(p: Path): return p.compute()`, with a
    /// project `class Path: def compute(self)`. The `p: Path` recv_type is a SIMPLE
    /// NAME inferred from the annotation (not self), so even though it collides
    /// exactly with the project `Path`, mining DEFERS it → the edge stays NULL
    /// (CORRECT — `p` is the library `pathlib.Path`, not the project class).
    #[test]
    fn null_mining_library_typed_param_collision_stays_unresolved() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                mining_class("Path", "Path", "model.py"),
                mining_method("compute", "Path", "model.py"),
                create_test_symbol("use", "user.py"),
            ])
            .unwrap();

        let use_fn = "user.py::use#function";
        store
            .replace_relationships_for_file(
                "user.py",
                &[mining_typed_call(use_fn, "user.py", "compute", "Path")],
            )
            .unwrap();

        assert_eq!(
            store.mine_receiver_type_relationship_targets().unwrap(),
            0,
            "a library-typed `p: Path` param must NOT mine-resolve to project Path.compute"
        );
        let (target, strategy, _) = edge_resolution(&store, use_fn, "compute");
        assert!(target.is_none(), "must stay NULL, got {target:?}");
        assert!(strategy.is_none());
    }

    /// NEGATIVE (b) — library-typed CONSTRUCTOR / local collision: `x = Path();
    /// x.compute()` where the file imported `Path` from a library and a project
    /// `Path` exists. The constructor-bound recv_type is again a simple name (not
    /// self) → mining DEFERS it → NULL.
    #[test]
    fn null_mining_library_typed_constructor_collision_stays_unresolved() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                mining_class("Path", "Path", "model.py"),
                mining_method("compute", "Path", "model.py"),
                create_test_symbol("make", "user.py"),
            ])
            .unwrap();

        let make_fn = "user.py::make#function";
        store
            .replace_relationships_for_file(
                "user.py",
                &[mining_typed_call(make_fn, "user.py", "compute", "Path")],
            )
            .unwrap();

        assert_eq!(
            store.mine_receiver_type_relationship_targets().unwrap(),
            0,
            "a library-typed `x = Path()` local must NOT mine-resolve to project Path.compute"
        );
        assert!(edge_resolution(&store, make_fn, "compute").0.is_none());
    }

    /// NEGATIVE (c) — INHERITANCE GATE, library base whose simple name collides
    /// with a project class: `class Derived(LibBase)` where the `extends` target is
    /// UNRESOLVED (a library base is not an indexed project symbol), while a
    /// coincidental project `class LibBase: def m(self)` exists. `self.m()` inside
    /// `Derived` is self-typed, but the unresolved `extends` edge is NOT followed,
    /// so `Derived` has no project supertype → `m` is not on its chain → NULL.
    #[test]
    fn null_mining_library_base_inheritance_no_wrong_resolution() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                mining_class("Derived", "Derived", "d.py"),
                mining_method("go", "Derived", "d.py"),
                // Coincidental project class sharing the library base's simple name.
                mining_class("LibBase", "LibBase", "proj.py"),
                mining_method("m", "LibBase", "proj.py"),
            ])
            .unwrap();

        let d_go = "d.py::Derived.go#method";
        store
            .replace_relationships_for_file(
                "d.py",
                &[
                    // Extends a LIBRARY base — target did NOT resolve (None).
                    mining_extends("d.py::Derived#class", "d.py", "LibBase", None),
                    mining_self_call(d_go, "d.py", "m", "Derived"),
                ],
            )
            .unwrap();

        assert_eq!(
            store.mine_receiver_type_relationship_targets().unwrap(),
            0,
            "an unresolved library-base extends must NOT pull in the coincidental project LibBase.m"
        );
        assert!(edge_resolution(&store, d_go, "m").0.is_none());
    }

    /// NEGATIVE — LIBRARY / NON-PROJECT METHOD: the receiver IS a project class
    /// (`Widget`, via `self`), but the called method `commit` is not defined on it
    /// or any RESOLVED supertype (it is a library method). 0 candidates on the
    /// chain → NULL.
    #[test]
    fn null_mining_library_method_stays_unresolved() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                mining_class("Widget", "Widget", "w.py"),
                mining_method("spin", "Widget", "w.py"),
            ])
            .unwrap();

        let widget_spin = "w.py::Widget.spin#method";
        store
            .replace_relationships_for_file(
                "w.py",
                // `self.commit()` inside Widget — `commit` is not a Widget method.
                &[mining_self_call(widget_spin, "w.py", "commit", "Widget")],
            )
            .unwrap();

        assert_eq!(
            store.mine_receiver_type_relationship_targets().unwrap(),
            0,
            "a method absent from the receiver class + chain must never resolve"
        );
        assert!(edge_resolution(&store, widget_spin, "commit").0.is_none());
    }

    /// NEGATIVE — AMBIGUOUS (class, method): a diamond where `B` implements BOTH
    /// `A1` and `A2` (both RESOLVED), each defining `m`, and `B` does not. A `self`
    /// receiver inside `B` reaches a most-derived defining level with TWO unrelated
    /// candidates → NULL.
    #[test]
    fn null_mining_ambiguous_class_method_stays_unresolved() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                mining_class("A1", "A1", "a1.py"),
                mining_method("m", "A1", "a1.py"),
                mining_class("A2", "A2", "a2.py"),
                mining_method("m", "A2", "a2.py"),
                mining_class("B", "B", "b.py"),
                mining_method("go", "B", "b.py"),
            ])
            .unwrap();

        let b_go = "b.py::B.go#method";
        store
            .replace_relationships_for_file(
                "b.py",
                &[
                    mining_extends("b.py::B#class", "b.py", "A1", Some("a1.py::A1#class")),
                    mining_extends("b.py::B#class", "b.py", "A2", Some("a2.py::A2#class")),
                    mining_self_call(b_go, "b.py", "m", "B"),
                ],
            )
            .unwrap();

        assert_eq!(
            store.mine_receiver_type_relationship_targets().unwrap(),
            0,
            "two equally-derived A1.m / A2.m candidates must stay ambiguous"
        );
        assert!(edge_resolution(&store, b_go, "m").0.is_none());
    }

    /// REGRESSION GUARD: mining only ever flips NULL→value. An already-resolved
    /// edge (even a self-typed one) is left byte-for-byte untouched, and a
    /// recv_type-less NULL edge is ignored entirely.
    #[test]
    fn null_mining_preserves_already_resolved_and_ignores_plain_edges() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                mining_class("A", "A", "a.py"),
                mining_method("run", "A", "a.py"),
                mining_class("B", "B", "b.py"),
                mining_method("go", "B", "b.py"),
            ])
            .unwrap();

        let b_go = "b.py::B.go#method";
        // (1) A self-typed edge PINNED to a deliberately "wrong" target.
        let mut pinned = mining_self_call(b_go, "b.py", "run", "B");
        pinned.target_symbol_id = Some("a.py::A.run#method".to_string());
        // (2) A recv_type-less (plain) NULL call edge — mining must ignore it.
        let plain = SymbolRelationship {
            source_symbol_id: b_go.to_string(),
            source_file_path: "b.py".to_string(),
            target_name: "helper".to_string(),
            target_symbol_id: None,
            relationship_type: SymbolRelationshipType::Call,
            line: 6,
            ..Default::default()
        };
        store
            .replace_relationships_for_file(
                "b.py",
                &[
                    mining_extends("b.py::B#class", "b.py", "A", Some("a.py::A#class")),
                    pinned,
                    plain,
                ],
            )
            .unwrap();

        assert_eq!(
            store.mine_receiver_type_relationship_targets().unwrap(),
            0,
            "mining must not touch already-resolved or recv_type-less edges"
        );
        // The pinned edge is unchanged and stays untagged by mining.
        let (target, strategy, _) = edge_resolution(&store, b_go, "run");
        assert_eq!(target.as_deref(), Some("a.py::A.run#method"));
        assert!(strategy.is_none());
        // The plain edge is still NULL.
        assert!(edge_resolution(&store, b_go, "helper").0.is_none());
    }

    fn coverage_symbol(
        id: &str,
        name: &str,
        file_path: &str,
        symbol_type: SymbolType,
        signature: Option<&str>,
    ) -> Symbol {
        Symbol {
            id: id.to_string(),
            name: name.to_string(),
            qualified_name: name.to_string(),
            symbol_type,
            file_path: file_path.to_string(),
            range: Range {
                start: Position {
                    line: 1,
                    character: 0,
                },
                end: Position {
                    line: 2,
                    character: 0,
                },
            },
            byte_offset: 0,
            byte_length: 0,
            parent_id: None,
            docstring: None,
            signature: signature.map(|s| s.to_string()),
            content_hash: "hash".to_string(),
        }
    }

    fn coverage_count(counts: &[(String, u64)], key: &str) -> u64 {
        counts
            .iter()
            .find(|(label, _)| label == key)
            .map(|(_, count)| *count)
            // `.expect` proves the zero rows are *present* (back-filled), not just defaulted.
            .unwrap_or_else(|| panic!("expected a back-filled row for `{key}`"))
    }

    #[test]
    fn test_coverage_histogram_backfills_zeros() {
        let store = SymbolStore::in_memory().unwrap();

        // Rust: 3 functions (2 with signatures), 1 method (with signature), 1 struct.
        // function+method total = 4, with non-empty signature = 3 -> pct = 0.75.
        let rust_file = "src/lib.rs";
        store
            .upsert_symbols(&[
                coverage_symbol(
                    "r1",
                    "func_a",
                    rust_file,
                    SymbolType::Function,
                    Some("(x: i32) -> i32"),
                ),
                coverage_symbol(
                    "r2",
                    "func_b",
                    rust_file,
                    SymbolType::Function,
                    Some("() -> ()"),
                ),
                coverage_symbol("r3", "func_c", rust_file, SymbolType::Function, None),
                coverage_symbol(
                    "r4",
                    "method_a",
                    rust_file,
                    SymbolType::Method,
                    Some("(&self)"),
                ),
                coverage_symbol("r5", "StructX", rust_file, SymbolType::Struct, None),
            ])
            .unwrap();

        // One Rust relationship of a single kind (implements); all others must be 0.
        store
            .replace_relationships_for_file(
                rust_file,
                &[SymbolRelationship {
                    source_symbol_id: "r5".to_string(),
                    source_file_path: rust_file.to_string(),
                    target_name: "Renderable".to_string(),
                    target_symbol_id: None,
                    relationship_type: SymbolRelationshipType::Implements,
                    line: 1,
                    ..Default::default()
                }],
            )
            .unwrap();

        // TypeScript: a single Class, no functions/methods (pct must be 0.0).
        let ts_file = "src/app.ts";
        store
            .upsert_symbols(&[coverage_symbol(
                "t1",
                "AppService",
                ts_file,
                SymbolType::Class,
                None,
            )])
            .unwrap();

        let histogram = store.coverage_histogram().unwrap();
        assert_eq!(histogram.per_language.len(), 2);

        let rust = histogram
            .per_language
            .iter()
            .find(|coverage| coverage.language == "Rust")
            .expect("Rust language row present");
        let typescript = histogram
            .per_language
            .iter()
            .find(|coverage| coverage.language == "TypeScript")
            .expect("TypeScript language row present");

        // Present kinds counted correctly.
        assert_eq!(coverage_count(&rust.symbol_counts, "function"), 3);
        assert_eq!(coverage_count(&rust.symbol_counts, "method"), 1);
        assert_eq!(coverage_count(&rust.symbol_counts, "struct"), 1);

        // Absent kinds appear as explicit 0 rows (the load-bearing back-fill).
        assert_eq!(coverage_count(&rust.symbol_counts, "class"), 0);
        assert_eq!(coverage_count(&rust.symbol_counts, "trait"), 0);
        assert_eq!(coverage_count(&rust.symbol_counts, "enum"), 0);
        assert_eq!(coverage_count(&rust.symbol_counts, "css_selector"), 0);
        // Every SymbolType variant is represented exactly once.
        assert_eq!(rust.symbol_counts.len(), ALL_SYMBOL_TYPES.len());

        // Relationship back-fill: implements observed, everything else 0.
        assert_eq!(coverage_count(&rust.relationship_counts, "implements"), 1);
        assert_eq!(coverage_count(&rust.relationship_counts, "call"), 0);
        assert_eq!(coverage_count(&rust.relationship_counts, "import"), 0);
        assert_eq!(rust.relationship_counts.len(), ALL_RELATIONSHIP_TYPES.len());

        // Signature percentage: 3 of 4 Function/Method symbols carry a signature.
        assert!((rust.function_method_signature_pct - 0.75).abs() < 1e-9);

        // Second language shows zeros for the first language's kinds, and vice versa.
        assert_eq!(coverage_count(&typescript.symbol_counts, "class"), 1);
        assert_eq!(coverage_count(&typescript.symbol_counts, "struct"), 0);
        assert_eq!(coverage_count(&typescript.symbol_counts, "function"), 0);
        assert_eq!(typescript.symbol_counts.len(), ALL_SYMBOL_TYPES.len());
        // No Function/Method symbols -> pct defaults to 0.0 (no divide-by-zero).
        assert_eq!(typescript.function_method_signature_pct, 0.0);
    }

    #[test]
    fn test_upsert_and_get() {
        let store = SymbolStore::in_memory().unwrap();
        let symbol = create_test_symbol("authenticate", "auth.ts");

        store.upsert_symbols(&[symbol.clone()]).unwrap();

        let retrieved = store.get_symbol(&symbol.id).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "authenticate");
    }

    #[test]
    fn test_get_symbols_in_file() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("func1", "test.ts");
        let sym2 = create_test_symbol("func2", "test.ts");
        let sym3 = create_test_symbol("other", "other.ts");

        store.upsert_symbols(&[sym1, sym2, sym3]).unwrap();

        let symbols = store.get_symbols_in_file("test.ts").unwrap();
        assert_eq!(symbols.len(), 2);
    }

    #[test]
    fn test_search_by_name_like() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("authenticate", "auth.ts");
        let sym2 = create_test_symbol("authorize", "auth.ts");
        let sym3 = create_test_symbol("validate", "valid.ts");

        store.upsert_symbols(&[sym1, sym2, sym3]).unwrap();

        let results = store.search_by_name_like("auth", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_by_name_like_returns_exact_camel_case_symbol() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("GitCommitMessage", "git.ts");
        let sym2 = create_test_symbol("GitCommitMessageEditor", "editor.ts");

        store.upsert_symbols(&[sym1, sym2]).unwrap();

        let results = store.search_by_name_like("GitCommitMessage", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "GitCommitMessage");
    }

    #[test]
    fn test_search_by_name_like_matches_multi_word_queries() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("authenticateUser", "auth.ts");
        let sym2 = create_test_symbol("UserService", "service.ts");
        let sym3 = create_test_symbol("validateToken", "valid.ts");

        store.upsert_symbols(&[sym1, sym2, sym3]).unwrap();

        let results = store.search_by_name_like("auth user", 10).unwrap();
        assert!(results
            .iter()
            .any(|symbol| symbol.name == "authenticateUser"));
        assert!(results.iter().any(|symbol| symbol.name == "UserService"));
    }

    #[test]
    fn test_search_by_name_uses_fts_identifier_terms() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("--accent-ai", "style.css");
        let sym2 = create_test_symbol(".chat-message", "style.css");
        let sym3 = create_test_symbol("UserService", "service.ts");

        store.upsert_symbols(&[sym1, sym2, sym3]).unwrap();

        let accent_results = store.search_by_name("--accent", 10).unwrap();
        assert!(accent_results
            .iter()
            .any(|symbol| symbol.name == "--accent-ai"));

        let selector_results = store.search_by_name("chat message", 10).unwrap();
        assert!(selector_results
            .iter()
            .any(|symbol| symbol.name == ".chat-message"));
    }

    #[test]
    fn test_search_by_name_fts_indexes_qualified_names_and_signatures() {
        let store = SymbolStore::in_memory().unwrap();
        let mut symbol = create_test_symbol("findUser", "service.ts");
        symbol.qualified_name = "accounts::findUser".to_string();
        symbol.signature = Some("(id: UserId) -> AccountDto".to_string());
        store.upsert_symbols(&[symbol.clone()]).unwrap();

        let by_qualified = store.search_by_name("accounts", 10).unwrap();
        assert_eq!(by_qualified.len(), 1);
        assert_eq!(by_qualified[0].id, symbol.id);

        let by_signature = store.search_by_name("AccountDto", 10).unwrap();
        assert_eq!(by_signature.len(), 1);
        assert_eq!(by_signature[0].id, symbol.id);
    }

    #[test]
    fn test_delete_file_symbols() {
        let store = SymbolStore::in_memory().unwrap();
        let sym1 = create_test_symbol("func1", "test.ts");
        let sym2 = create_test_symbol("func2", "test.ts");

        store.upsert_symbols(&[sym1, sym2]).unwrap();
        assert_eq!(store.count().unwrap(), 2);

        store.delete_file_symbols("test.ts").unwrap();
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn test_clear_generated_index_data_preserves_user_authored_tables() {
        let store = SymbolStore::in_memory().unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "CREATE TABLE user_project_notes (id TEXT PRIMARY KEY, body TEXT NOT NULL)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO user_project_notes (id, body) VALUES ('adr-1', 'keep me')",
                [],
            )
            .unwrap();
        }

        let symbol = create_test_symbol("func1", "test.ts");
        store.upsert_symbols(std::slice::from_ref(&symbol)).unwrap();
        store
            .replace_relationships_for_file(
                "test.ts",
                &[create_test_relationship(&symbol.id, "test.ts", "helper")],
            )
            .unwrap();
        store
            .replace_semantic_anchors_for_file(
                "test.ts",
                &[SemanticAnchor {
                    id: "test.ts::anchor:route:1:0:abc".to_string(),
                    file_path: "test.ts".to_string(),
                    kind: "route".to_string(),
                    value: "/settings".to_string(),
                    line: 1,
                    character: 0,
                    preview: "/settings".to_string(),
                    confidence: 0.9,
                    owner_symbol_id: Some(symbol.id.clone()),
                    target_file_path: None,
                    target_name: None,
                    target_symbol_id: None,
                }],
            )
            .unwrap();
        store.mark_file_indexed("test.ts", "hash", 1).unwrap();

        store.clear_generated_index_data().unwrap();

        assert_eq!(store.count().unwrap(), 0);
        assert!(store.indexed_file_record("test.ts").unwrap().is_none());
        assert!(store
            .get_semantic_anchors_in_file("test.ts", 10)
            .unwrap()
            .is_empty());
        assert!(store
            .get_relationship_targets(&symbol.id, SymbolRelationshipType::Call, 10)
            .unwrap()
            .is_empty());

        let preserved_body = {
            let conn = store.conn.lock().unwrap();
            conn.query_row(
                "SELECT body FROM user_project_notes WHERE id = 'adr-1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        assert_eq!(preserved_body, "keep me");
    }

    #[test]
    fn test_generated_index_table_inventory_stays_intentional() {
        assert_eq!(
            GENERATED_INDEX_TABLES,
            &[
                "symbol_relationships",
                "semantic_anchors",
                "symbols",
                "indexed_files"
            ]
        );
        assert_eq!(
            GENERATED_INDEX_CLEAR_STATEMENTS.len(),
            GENERATED_INDEX_TABLES.len()
        );
    }

    /// Column names of `table`, in declaration order, via `PRAGMA table_info`.
    fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    const NEW_RELATIONSHIP_COLUMNS: [&str; 5] = [
        "resolution_strategy",
        "confidence",
        "metadata_json",
        "import_path",
        "imported_name",
    ];
    const NEW_SEMANTIC_ANCHOR_COLUMNS: [&str; 4] = [
        "owner_symbol_id",
        "target_file_path",
        "target_name",
        "target_symbol_id",
    ];

    #[test]
    fn migration_forward_from_empty_reaches_latest_with_writable_columns() {
        // A fresh in-memory store runs `create_schema` -> `run_migrations`.
        let store = SymbolStore::in_memory().unwrap();
        let conn = store.conn.lock().unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);

        let columns = table_columns(&conn, "symbol_relationships");
        for new_column in NEW_RELATIONSHIP_COLUMNS {
            assert!(
                columns.iter().any(|column| column == new_column),
                "expected migrated column `{new_column}`"
            );
        }
        let anchor_columns = table_columns(&conn, "semantic_anchors");
        for new_column in NEW_SEMANTIC_ANCHOR_COLUMNS {
            assert!(
                anchor_columns.iter().any(|column| column == new_column),
                "expected migrated semantic-anchor column `{new_column}`"
            );
        }

        // The new columns are writable + queryable.
        conn.execute(
            "INSERT INTO symbol_relationships
             (source_symbol_id, source_file_path, target_name, target_symbol_id,
              relationship_type, line, resolution_strategy, confidence, metadata_json)
             VALUES ('s1', 'f.rs', 'helper', 's2', 'call', 7,
                     'unique_global_name', 0.95, '{\"source\":\"backfill\"}')",
            [],
        )
        .unwrap();

        let (strategy, confidence, metadata): (String, f64, String) = conn
            .query_row(
                "SELECT resolution_strategy, confidence, metadata_json
                 FROM symbol_relationships WHERE source_symbol_id = 's1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(strategy, "unique_global_name");
        assert_eq!(confidence, 0.95);
        assert_eq!(metadata, "{\"source\":\"backfill\"}");
    }

    #[test]
    fn migration_forward_from_current_pre_migration_db() {
        // Simulate a DB created BEFORE M2.3: the base `symbol_relationships`
        // table, `user_version = 0`, none of the new columns present.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE symbols (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                qualified_name TEXT NOT NULL DEFAULT '',
                signature TEXT,
                docstring TEXT
            );
            CREATE TABLE symbol_relationships (
                source_symbol_id TEXT NOT NULL,
                source_file_path TEXT NOT NULL,
                target_name TEXT NOT NULL,
                target_symbol_id TEXT,
                relationship_type TEXT NOT NULL,
                line INTEGER NOT NULL,
                PRIMARY KEY (source_symbol_id, target_name, relationship_type, line)
            );",
        )
        .unwrap();

        let start_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(start_version, 0);
        let before = table_columns(&conn, "symbol_relationships");
        for new_column in NEW_RELATIONSHIP_COLUMNS {
            assert!(!before.iter().any(|column| column == new_column));
        }

        run_migrations(&conn).unwrap();

        let migrated_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(migrated_version, LATEST_SCHEMA_VERSION);
        let after = table_columns(&conn, "symbol_relationships");
        for new_column in NEW_RELATIONSHIP_COLUMNS {
            assert!(
                after.iter().any(|column| column == new_column),
                "expected migrated column `{new_column}`"
            );
        }
        let anchor_columns = table_columns(&conn, "semantic_anchors");
        for new_column in NEW_SEMANTIC_ANCHOR_COLUMNS {
            assert!(anchor_columns.iter().any(|column| column == new_column));
        }

        // Running again is a no-op: version stays put and no columns are added.
        run_migrations(&conn).unwrap();
        let rerun_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rerun_version, LATEST_SCHEMA_VERSION);
        assert_eq!(
            after.len(),
            table_columns(&conn, "symbol_relationships").len()
        );
    }

    #[test]
    fn migration_idempotency_no_duplicate_columns() {
        // `create_schema` already migrated once; run it twice more.
        let store = SymbolStore::in_memory().unwrap();
        let conn = store.conn.lock().unwrap();

        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);

        let columns = table_columns(&conn, "symbol_relationships");
        for new_column in NEW_RELATIONSHIP_COLUMNS {
            let occurrences = columns
                .iter()
                .filter(|column| *column == new_column)
                .count();
            assert_eq!(occurrences, 1, "column `{new_column}` was duplicated");
        }
        let anchor_columns = table_columns(&conn, "semantic_anchors");
        for new_column in NEW_SEMANTIC_ANCHOR_COLUMNS {
            let occurrences = anchor_columns
                .iter()
                .filter(|column| *column == new_column)
                .count();
            assert_eq!(occurrences, 1, "column `{new_column}` was duplicated");
        }
    }

    #[test]
    fn test_replace_and_get_relationship_targets() {
        let store = SymbolStore::in_memory().unwrap();
        let symbol = create_test_symbol("caller", "test.ts");
        store.upsert_symbols(&[symbol.clone()]).unwrap();

        store
            .replace_relationships_for_file(
                "test.ts",
                &[
                    create_test_relationship(&symbol.id, "test.ts", "helperOne"),
                    create_test_relationship(&symbol.id, "test.ts", "helperTwo"),
                ],
            )
            .unwrap();

        let targets = store
            .get_relationship_targets(&symbol.id, SymbolRelationshipType::Call, 10)
            .unwrap();

        assert_eq!(targets.len(), 2);
        assert!(targets.iter().any(|target| target == "helperOne"));
        assert!(targets.iter().any(|target| target == "helperTwo"));
    }

    #[test]
    fn test_find_references_to_target() {
        let store = SymbolStore::in_memory().unwrap();
        let caller = create_test_symbol("caller", "main.ts");
        let helper = create_test_symbol("helper", "utils.ts");
        store.upsert_symbols(&[caller.clone(), helper]).unwrap();

        store
            .replace_relationships_for_file(
                "main.ts",
                &[create_test_relationship(&caller.id, "main.ts", "helper")],
            )
            .unwrap();

        let references = store
            .find_references_to_target("helper", SymbolRelationshipType::Call, 10)
            .unwrap();

        assert_eq!(references.len(), 1);
        assert_eq!(references[0].source_symbol.id, caller.id);
        assert_eq!(references[0].target_name, "helper");
        assert!(references[0].target_symbol_id.is_none());
        assert!(references[0].target_symbol.is_none());
        assert_eq!(
            references[0].relationship_type,
            SymbolRelationshipType::Call
        );
    }

    #[test]
    fn test_find_references_to_symbol_id_and_get_outgoing_edges() {
        let store = SymbolStore::in_memory().unwrap();
        let caller = create_test_symbol("caller", "main.ts");
        let helper = create_test_symbol("helper", "utils.ts");
        store
            .upsert_symbols(&[caller.clone(), helper.clone()])
            .unwrap();

        store
            .replace_relationships_for_file(
                "main.ts",
                &[SymbolRelationship {
                    source_symbol_id: caller.id.clone(),
                    source_file_path: "main.ts".to_string(),
                    target_name: helper.name.clone(),
                    target_symbol_id: Some(helper.id.clone()),
                    relationship_type: SymbolRelationshipType::Call,
                    line: 3,
                    recv_type: Some("Helper".to_string()),
                    recv_self: true,
                    import_path: None,
                    imported_name: None,
                    resolution_strategy: Some("receiver_type".to_string()),
                    confidence: Some(0.8),
                    receiver_kind: None,
                }],
            )
            .unwrap();

        let incoming = store
            .find_references_to_symbol_id(&helper.id, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].source_symbol.id, caller.id);
        assert_eq!(
            incoming[0].target_symbol_id.as_deref(),
            Some(helper.id.as_str())
        );
        assert_eq!(
            incoming[0]
                .target_symbol
                .as_ref()
                .map(|symbol| symbol.id.as_str()),
            Some(helper.id.as_str())
        );
        assert_eq!(
            incoming[0].resolution_strategy.as_deref(),
            Some("receiver_type")
        );
        assert_eq!(incoming[0].resolution_confidence, Some(0.8));
        assert_eq!(incoming[0].receiver_type.as_deref(), Some("Helper"));
        assert!(incoming[0].receiver_is_self);

        let outgoing = store
            .get_relationship_edges_from_source(&caller.id, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].source_symbol.id, caller.id);
        assert_eq!(
            outgoing[0].target_symbol_id.as_deref(),
            Some(helper.id.as_str())
        );
        assert_eq!(
            outgoing[0]
                .target_symbol
                .as_ref()
                .map(|symbol| symbol.id.as_str()),
            Some(helper.id.as_str())
        );
        assert_eq!(
            outgoing[0].resolution_strategy.as_deref(),
            Some("receiver_type")
        );
        assert_eq!(outgoing[0].resolution_confidence, Some(0.8));
        assert_eq!(
            outgoing[0].observation_kind,
            RelationshipObservationKind::SyntaxExtracted
        );
    }

    #[test]
    fn test_relationship_integrity_stats_counts_missing_edges() {
        let store = SymbolStore::in_memory().unwrap();

        store
            .replace_relationships_for_file(
                "main.ts",
                &[
                    SymbolRelationship {
                        source_symbol_id: "missing-source".to_string(),
                        source_file_path: "main.ts".to_string(),
                        target_name: "helper".to_string(),
                        target_symbol_id: Some("missing-target".to_string()),
                        relationship_type: SymbolRelationshipType::Call,
                        line: 3,
                        ..Default::default()
                    },
                    SymbolRelationship {
                        source_symbol_id: "missing-source".to_string(),
                        source_file_path: "main.ts".to_string(),
                        target_name: "dynamicCall".to_string(),
                        target_symbol_id: None,
                        relationship_type: SymbolRelationshipType::Call,
                        line: 4,
                        ..Default::default()
                    },
                ],
            )
            .unwrap();

        let stats = store.relationship_integrity_stats().unwrap();

        assert_eq!(stats.total_relationships, 2);
        assert_eq!(stats.resolved_relationships, 1);
        assert_eq!(stats.unresolved_symbol_relationships, 1);
        assert_eq!(stats.missing_source_symbols, 2);
        assert_eq!(stats.missing_target_symbols, 1);
        assert!(stats.by_type.iter().any(|stats| {
            stats.relationship_type == "call"
                && stats.total_relationships == 2
                && stats.resolved_relationships == 1
                && stats.unresolved_symbol_relationships == 1
        }));
        assert!(stats.top_unresolved_targets.iter().any(|target| {
            target.relationship_type == "call"
                && target.target_name == "dynamicCall"
                && target.count == 1
                && target.example_source_file == "main.ts"
        }));
    }

    #[test]
    fn test_delete_file_symbols_removes_relationships() {
        let store = SymbolStore::in_memory().unwrap();
        let symbol = create_test_symbol("caller", "test.ts");
        store.upsert_symbols(&[symbol.clone()]).unwrap();
        store
            .replace_relationships_for_file(
                "test.ts",
                &[create_test_relationship(&symbol.id, "test.ts", "helperOne")],
            )
            .unwrap();

        store.delete_file_symbols("test.ts").unwrap();

        let targets = store
            .get_relationship_targets(&symbol.id, SymbolRelationshipType::Call, 10)
            .unwrap();
        assert!(targets.is_empty());
    }

    #[test]
    fn test_get_file_relationship_targets() {
        let store = SymbolStore::in_memory().unwrap();
        let symbol = create_test_symbol("caller", "test.ts");
        store.upsert_symbols(&[symbol.clone()]).unwrap();
        store
            .replace_relationships_for_file(
                "test.ts",
                &[
                    SymbolRelationship {
                        source_symbol_id: format!("{}::import1#import", symbol.file_path),
                        source_file_path: "test.ts".to_string(),
                        target_name: "./utils".to_string(),
                        target_symbol_id: None,
                        relationship_type: SymbolRelationshipType::Import,
                        line: 1,
                        ..Default::default()
                    },
                    SymbolRelationship {
                        source_symbol_id: format!("{}::import2#import", symbol.file_path),
                        source_file_path: "test.ts".to_string(),
                        target_name: "./helpers".to_string(),
                        target_symbol_id: None,
                        relationship_type: SymbolRelationshipType::Import,
                        line: 2,
                        ..Default::default()
                    },
                ],
            )
            .unwrap();

        let targets = store
            .get_file_relationship_targets("test.ts", SymbolRelationshipType::Import, 10)
            .unwrap();

        assert_eq!(targets.len(), 2);
        assert!(targets.iter().any(|target| target == "./utils"));
        assert!(targets.iter().any(|target| target == "./helpers"));
    }

    #[test]
    fn test_file_indexing_tracking() {
        let store = SymbolStore::in_memory().unwrap();

        // Initially needs reindex
        assert!(store.needs_reindex("test.ts", "abc123").unwrap());

        // Mark as indexed
        store.mark_file_indexed("test.ts", "abc123", 5).unwrap();

        // Same hash, no reindex needed
        assert!(!store.needs_reindex("test.ts", "abc123").unwrap());

        // Different hash, needs reindex
        assert!(store.needs_reindex("test.ts", "def456").unwrap());
    }

    #[test]
    fn test_file_indexing_metadata_tracking() {
        let store = SymbolStore::in_memory().unwrap();

        assert_eq!(
            store
                .needs_reindex_for_metadata("test.ts", 128, 1_234)
                .unwrap(),
            Some(true)
        );

        store
            .mark_file_indexed_with_metadata(
                "test.ts",
                "abc123",
                5,
                Some(128),
                Some(12),
                Some(1_234),
            )
            .unwrap();

        let record = store.indexed_file_record("test.ts").unwrap().unwrap();
        assert_eq!(record.file_size, Some(128));
        assert_eq!(record.line_count, Some(12));
        assert_eq!(record.extractor_version, None);

        store
            .mark_file_indexed_with_metadata_and_extractor_version(
                "test.ts",
                "abc123",
                5,
                Some(128),
                Some(12),
                Some(1_234),
                Some(7),
            )
            .unwrap();
        let record = store.indexed_file_record("test.ts").unwrap().unwrap();
        assert_eq!(record.extractor_version, Some(7));

        assert_eq!(
            store
                .needs_reindex_for_metadata("test.ts", 128, 1_234)
                .unwrap(),
            Some(false)
        );
        assert_eq!(
            store
                .needs_reindex_for_metadata("test.ts", 129, 1_234)
                .unwrap(),
            Some(true)
        );
        assert_eq!(
            store
                .needs_reindex_for_metadata("test.ts", 128, 1_235)
                .unwrap(),
            Some(true)
        );
    }

    // ---- Track D — relationship-kind-aware global resolution ----
    //
    // The back-fill may only resolve a name to a symbol KIND the relationship
    // can legally reference (`RELATIONSHIP_TARGET_KIND_ALLOWLIST`), and
    // uniqueness is judged inside that kind class. A confidently-wrong target
    // (a heading satisfying a call) is the exact bug class these guard against.

    /// A still-NULL relationship of an arbitrary kind
    /// (`create_test_relationship` is Call-only).
    fn kind_relationship(
        source_id: &str,
        file: &str,
        target: &str,
        relationship_type: SymbolRelationshipType,
    ) -> SymbolRelationship {
        SymbolRelationship {
            source_symbol_id: source_id.to_string(),
            source_file_path: file.to_string(),
            target_name: target.to_string(),
            target_symbol_id: None,
            relationship_type,
            line: 3,
            ..Default::default()
        }
    }

    /// One edge's `(target_symbol_id, resolution_strategy, confidence)`,
    /// keyed by relationship kind so same-target edges of different kinds
    /// stay distinguishable.
    fn edge_resolution_for_kind(
        store: &SymbolStore,
        source_id: &str,
        target_name: &str,
        relationship_type: SymbolRelationshipType,
    ) -> (Option<String>, Option<String>, Option<f64>) {
        let conn = store.conn.lock().unwrap();
        conn.query_row(
            "SELECT target_symbol_id, resolution_strategy, confidence
             FROM symbol_relationships
             WHERE source_symbol_id = ?1 AND target_name = ?2 AND relationship_type = ?3",
            params![source_id, target_name, relationship_type.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
    }

    /// Track D core scenario: a `call` observation of a name shared by a
    /// METHOD and a Markdown HEADING resolves to the callable — the heading is
    /// not a legal call target, so it neither wins nor makes the name
    /// ambiguous. The `usage` observation of the same text stays NULL: neither
    /// a method nor a heading is a legal value target.
    #[test]
    fn kind_aware_backfill_call_resolves_to_method_despite_same_named_heading() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                coverage_symbol(
                    "m1",
                    "ResolveModelWithFallbacks",
                    "src/models.py",
                    SymbolType::Method,
                    None,
                ),
                coverage_symbol(
                    "h1",
                    "ResolveModelWithFallbacks",
                    "docs/guide.md",
                    SymbolType::Heading,
                    None,
                ),
                create_test_symbol("caller", "caller.py"),
            ])
            .unwrap();
        let caller_id = "caller.py::caller#function";
        store
            .replace_relationships_for_file(
                "caller.py",
                &[
                    kind_relationship(
                        caller_id,
                        "caller.py",
                        "ResolveModelWithFallbacks",
                        SymbolRelationshipType::Call,
                    ),
                    kind_relationship(
                        caller_id,
                        "caller.py",
                        "ResolveModelWithFallbacks",
                        SymbolRelationshipType::Usage,
                    ),
                ],
            )
            .unwrap();

        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 1);

        let (target, strategy, confidence) = edge_resolution_for_kind(
            &store,
            caller_id,
            "ResolveModelWithFallbacks",
            SymbolRelationshipType::Call,
        );
        assert_eq!(
            target.as_deref(),
            Some("m1"),
            "the call must resolve to the method — the heading must not poison uniqueness"
        );
        assert_eq!(strategy.as_deref(), Some("global_unique"));
        assert_eq!(confidence, Some(0.5));

        let (usage_target, usage_strategy, _) = edge_resolution_for_kind(
            &store,
            caller_id,
            "ResolveModelWithFallbacks",
            SymbolRelationshipType::Usage,
        );
        assert!(
            usage_target.is_none(),
            "no legal value candidate exists: a method/heading must not satisfy `usage`"
        );
        assert!(usage_strategy.is_none());
    }

    /// Release gate: a heading is in NO allowlist, so a name that only exists
    /// as a Markdown heading never resolves any relationship kind.
    #[test]
    fn kind_aware_backfill_heading_is_never_a_target() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                coverage_symbol(
                    "h1",
                    "SetupGuide",
                    "docs/setup.md",
                    SymbolType::Heading,
                    None,
                ),
                create_test_symbol("caller", "caller.py"),
            ])
            .unwrap();
        let caller_id = "caller.py::caller#function";
        store
            .replace_relationships_for_file(
                "caller.py",
                &[
                    kind_relationship(
                        caller_id,
                        "caller.py",
                        "SetupGuide",
                        SymbolRelationshipType::Call,
                    ),
                    kind_relationship(
                        caller_id,
                        "caller.py",
                        "SetupGuide",
                        SymbolRelationshipType::Usage,
                    ),
                    kind_relationship(
                        caller_id,
                        "caller.py",
                        "SetupGuide",
                        SymbolRelationshipType::UsesType,
                    ),
                ],
            )
            .unwrap();

        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 0);
        for relationship_type in [
            SymbolRelationshipType::Call,
            SymbolRelationshipType::Usage,
            SymbolRelationshipType::UsesType,
        ] {
            let (target, strategy, _) =
                edge_resolution_for_kind(&store, caller_id, "SetupGuide", relationship_type);
            assert!(
                target.is_none(),
                "{relationship_type} resolved to a heading"
            );
            assert!(strategy.is_none());
        }
    }

    /// Uniqueness is judged INSIDE the kind class: two legal value candidates
    /// (a constant and a variable) keep `usage` ambiguous, while a unique
    /// value candidate resolves. A `call` to the value-only name finds zero
    /// legal candidates and also stays NULL.
    #[test]
    fn kind_aware_backfill_ambiguous_value_candidates_stay_unresolved() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                coverage_symbol("c1", "LIMIT", "a.rs", SymbolType::Constant, None),
                coverage_symbol("v1", "LIMIT", "b.rs", SymbolType::Variable, None),
                coverage_symbol("c2", "TIMEOUT", "a.rs", SymbolType::Constant, None),
                create_test_symbol("caller", "caller.rs"),
            ])
            .unwrap();
        let caller_id = "caller.rs::caller#function";
        store
            .replace_relationships_for_file(
                "caller.rs",
                &[
                    kind_relationship(
                        caller_id,
                        "caller.rs",
                        "LIMIT",
                        SymbolRelationshipType::Usage,
                    ),
                    kind_relationship(
                        caller_id,
                        "caller.rs",
                        "LIMIT",
                        SymbolRelationshipType::Call,
                    ),
                    kind_relationship(
                        caller_id,
                        "caller.rs",
                        "TIMEOUT",
                        SymbolRelationshipType::Usage,
                    ),
                ],
            )
            .unwrap();

        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 1);

        let (ambiguous, _, _) =
            edge_resolution_for_kind(&store, caller_id, "LIMIT", SymbolRelationshipType::Usage);
        assert!(
            ambiguous.is_none(),
            "two legal value candidates must stay unresolved, got {ambiguous:?}"
        );
        let (call_target, _, _) =
            edge_resolution_for_kind(&store, caller_id, "LIMIT", SymbolRelationshipType::Call);
        assert!(
            call_target.is_none(),
            "a constant/variable is not a legal call target"
        );
        let (unique, strategy, confidence) =
            edge_resolution_for_kind(&store, caller_id, "TIMEOUT", SymbolRelationshipType::Usage);
        assert_eq!(unique.as_deref(), Some("c2"));
        assert_eq!(strategy.as_deref(), Some("global_unique"));
        assert_eq!(confidence, Some(0.5));
    }

    /// `import` stays excluded and the kinds without an allowlist row
    /// (`export`, `reads_env`) are never back-filled — even when a unique
    /// same-named symbol exists.
    #[test]
    fn kind_aware_backfill_leaves_import_export_reads_env_untouched() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                create_test_symbol("envHelper", "util.py"),
                create_test_symbol("caller", "caller.py"),
            ])
            .unwrap();
        let caller_id = "caller.py::caller#function";
        store
            .replace_relationships_for_file(
                "caller.py",
                &[
                    kind_relationship(
                        caller_id,
                        "caller.py",
                        "envHelper",
                        SymbolRelationshipType::Import,
                    ),
                    kind_relationship(
                        caller_id,
                        "caller.py",
                        "envHelper",
                        SymbolRelationshipType::Export,
                    ),
                    kind_relationship(
                        caller_id,
                        "caller.py",
                        "envHelper",
                        SymbolRelationshipType::ReadsEnv,
                    ),
                    kind_relationship(
                        caller_id,
                        "caller.py",
                        "envHelper",
                        SymbolRelationshipType::Call,
                    ),
                ],
            )
            .unwrap();

        // Only the call resolves; the excluded kinds stay untouched.
        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 1);
        for relationship_type in [
            SymbolRelationshipType::Import,
            SymbolRelationshipType::Export,
            SymbolRelationshipType::ReadsEnv,
        ] {
            let (target, strategy, _) =
                edge_resolution_for_kind(&store, caller_id, "envHelper", relationship_type);
            assert!(
                target.is_none(),
                "{relationship_type} must never be back-filled"
            );
            assert!(strategy.is_none());
        }
        let (call_target, _, _) =
            edge_resolution_for_kind(&store, caller_id, "envHelper", SymbolRelationshipType::Call);
        assert_eq!(call_target.as_deref(), Some("util.py::envHelper#function"));
    }

    // ---- Track C — Go implicit interface implementation mining ----
    //
    // These drive `mine_go_interface_implementations` over hand-built symbol
    // and edge sets matching the Track C extractor contract (interface method
    // specs as Method children with `(params) result` signatures; concrete
    // methods linked from their receiver type by `contains` edges). Every
    // deliberate exclusion is asserted — a confidently-wrong `implements`
    // edge is the bug class these guard against.

    /// A Go symbol with explicit kind, file, line, signature, and parent.
    fn go_symbol(
        id: &str,
        name: &str,
        symbol_type: SymbolType,
        file: &str,
        line: u32,
        signature: Option<&str>,
        parent_id: Option<&str>,
    ) -> Symbol {
        Symbol {
            id: id.to_string(),
            name: name.to_string(),
            qualified_name: name.to_string(),
            symbol_type,
            file_path: file.to_string(),
            range: Range {
                start: Position { line, character: 0 },
                end: Position {
                    line: line + 2,
                    character: 0,
                },
            },
            byte_offset: 0,
            byte_length: 0,
            parent_id: parent_id.map(str::to_string),
            docstring: None,
            signature: signature.map(str::to_string),
            content_hash: "hash".to_string(),
        }
    }

    /// Raw relationship insert so tests control `metadata_json` exactly (the
    /// public write path only serializes recv_type provenance).
    fn insert_edge_raw(
        store: &SymbolStore,
        source_id: &str,
        file: &str,
        target_name: &str,
        target_id: Option<&str>,
        relationship_type: &str,
        line: u32,
        metadata_json: Option<&str>,
    ) {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO symbol_relationships
             (source_symbol_id, source_file_path, target_name, target_symbol_id,
              relationship_type, line, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                source_id,
                file,
                target_name,
                target_id,
                relationship_type,
                line,
                metadata_json
            ],
        )
        .unwrap();
    }

    /// All mined `go_implicit_interface` edges as
    /// `(source_symbol_id, target_name, target_symbol_id, line, metadata_json)`.
    fn mined_go_edges(
        store: &SymbolStore,
    ) -> Vec<(String, String, Option<String>, i64, Option<String>)> {
        let conn = store.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT source_symbol_id, target_name, target_symbol_id, line, metadata_json
                 FROM symbol_relationships
                 WHERE resolution_strategy = 'go_implicit_interface'
                 ORDER BY source_symbol_id, target_name",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap();
        rows.collect::<Result<Vec<_>, _>>().unwrap()
    }

    /// The handoff's Fixture 3 core: `Cache` embeds `Base` (RESOLVED extends),
    /// `Memory` implements both, `Incomplete` only `Base`. Concrete methods
    /// carry NAMED params; the interface specs are unnamed — the canonical
    /// signature must bridge them.
    fn seed_go_cache_fixture(store: &SymbolStore) {
        store
            .upsert_symbols(&[
                go_symbol(
                    "i_base",
                    "Base",
                    SymbolType::Interface,
                    "cache/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "i_base_ping",
                    "Ping",
                    SymbolType::Method,
                    "cache/iface.go",
                    2,
                    Some("(context.Context) error"),
                    Some("i_base"),
                ),
                go_symbol(
                    "i_cache",
                    "Cache",
                    SymbolType::Interface,
                    "cache/iface.go",
                    5,
                    None,
                    None,
                ),
                go_symbol(
                    "i_cache_set",
                    "Set",
                    SymbolType::Method,
                    "cache/iface.go",
                    7,
                    Some("(string, []byte) error"),
                    Some("i_cache"),
                ),
                go_symbol(
                    "t_memory",
                    "Memory",
                    SymbolType::Struct,
                    "cache/memory.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_memory_ping",
                    "Ping",
                    SymbolType::Method,
                    "cache/memory.go",
                    3,
                    Some("(ctx context.Context) error"),
                    None,
                ),
                go_symbol(
                    "m_memory_set",
                    "Set",
                    SymbolType::Method,
                    "cache/memory.go",
                    6,
                    Some("(k string, v []byte) error"),
                    None,
                ),
                go_symbol(
                    "t_incomplete",
                    "Incomplete",
                    SymbolType::Struct,
                    "cache/incomplete.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_incomplete_ping",
                    "Ping",
                    SymbolType::Method,
                    "cache/incomplete.go",
                    3,
                    Some("(ctx context.Context) error"),
                    None,
                ),
            ])
            .unwrap();
        // Cache embeds Base — a RESOLVED extends edge.
        insert_edge_raw(
            store,
            "i_cache",
            "cache/iface.go",
            "Base",
            Some("i_base"),
            "extends",
            6,
            None,
        );
        // Receiver containment; absent metadata = value receiver.
        insert_edge_raw(
            store,
            "t_memory",
            "cache/memory.go",
            "Ping",
            Some("m_memory_ping"),
            "contains",
            3,
            None,
        );
        insert_edge_raw(
            store,
            "t_memory",
            "cache/memory.go",
            "Set",
            Some("m_memory_set"),
            "contains",
            6,
            None,
        );
        insert_edge_raw(
            store,
            "t_incomplete",
            "cache/incomplete.go",
            "Ping",
            Some("m_incomplete_ping"),
            "contains",
            3,
            None,
        );
    }

    /// Complete + incomplete implementations with embedded-interface
    /// inheritance, plus the audit trail and idempotent recompute.
    #[test]
    fn go_mining_complete_and_incomplete_with_embedded_inheritance() {
        let store = SymbolStore::in_memory().unwrap();
        seed_go_cache_fixture(&store);

        assert_eq!(store.mine_go_interface_implementations().unwrap(), 3);
        let edges = mined_go_edges(&store);
        let keys: Vec<(String, String)> = edges
            .iter()
            .map(|(source, target, ..)| (source.clone(), target.clone()))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("t_incomplete".to_string(), "Base".to_string()),
                ("t_memory".to_string(), "Base".to_string()),
                ("t_memory".to_string(), "Cache".to_string()),
            ],
            "Memory implements Base+Cache; Incomplete implements Base ONLY"
        );
        for (_, target_name, target_id, line, metadata) in &edges {
            assert!(
                metadata.is_none(),
                "value-set satisfaction carries no metadata"
            );
            assert_eq!(*line, 1, "edge line is the type declaration line");
            match target_name.as_str() {
                "Base" => assert_eq!(target_id.as_deref(), Some("i_base")),
                "Cache" => assert_eq!(target_id.as_deref(), Some("i_cache")),
                other => panic!("unexpected interface target {other}"),
            }
        }
        {
            let conn = store.conn.lock().unwrap();
            let (strategy, confidence): (String, f64) = conn
                .query_row(
                    "SELECT resolution_strategy, confidence FROM symbol_relationships
                     WHERE source_symbol_id = 't_memory' AND target_name = 'Cache'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(strategy, "go_implicit_interface");
            assert_eq!(confidence, 0.75);
        }

        // Idempotent recompute: the same derived set, no duplicates.
        assert_eq!(store.mine_go_interface_implementations().unwrap(), 3);
        assert_eq!(mined_go_edges(&store).len(), 3);
    }

    /// Package visibility: an unexported spec (`load`) is satisfiable only by
    /// a type in the interface's own package directory.
    #[test]
    fn go_mining_unexported_method_requires_same_package() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                go_symbol(
                    "i_store",
                    "Store",
                    SymbolType::Interface,
                    "pkga/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "i_store_load",
                    "load",
                    SymbolType::Method,
                    "pkga/iface.go",
                    2,
                    Some("() error"),
                    Some("i_store"),
                ),
                go_symbol(
                    "t_same",
                    "SamePkg",
                    SymbolType::Struct,
                    "pkga/impl.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_same_load",
                    "load",
                    SymbolType::Method,
                    "pkga/impl.go",
                    3,
                    Some("() error"),
                    None,
                ),
                go_symbol(
                    "t_other",
                    "OtherPkg",
                    SymbolType::Struct,
                    "pkgb/impl.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_other_load",
                    "load",
                    SymbolType::Method,
                    "pkgb/impl.go",
                    3,
                    Some("() error"),
                    None,
                ),
            ])
            .unwrap();
        insert_edge_raw(
            &store,
            "t_same",
            "pkga/impl.go",
            "load",
            Some("m_same_load"),
            "contains",
            3,
            None,
        );
        insert_edge_raw(
            &store,
            "t_other",
            "pkgb/impl.go",
            "load",
            Some("m_other_load"),
            "contains",
            3,
            None,
        );

        assert_eq!(store.mine_go_interface_implementations().unwrap(), 1);
        let edges = mined_go_edges(&store);
        assert_eq!(edges.len(), 1);
        assert_eq!(
            edges[0].0, "t_same",
            "an unexported spec is only satisfiable from the interface's own package"
        );
    }

    /// Pointer/value method sets: pointer-only satisfaction is emitted WITH
    /// the `receiver_set` marker; value-receiver and untagged (= value,
    /// conservative-inclusive) methods satisfy plainly.
    #[test]
    fn go_mining_pointer_only_satisfaction_is_tagged() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                go_symbol(
                    "i_closer",
                    "Closer",
                    SymbolType::Interface,
                    "io/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "i_closer_close",
                    "Close",
                    SymbolType::Method,
                    "io/iface.go",
                    2,
                    Some("() error"),
                    Some("i_closer"),
                ),
                go_symbol(
                    "t_ptr",
                    "PtrCloser",
                    SymbolType::Struct,
                    "io/ptr.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_ptr_close",
                    "Close",
                    SymbolType::Method,
                    "io/ptr.go",
                    3,
                    Some("() error"),
                    None,
                ),
                go_symbol(
                    "t_val",
                    "ValCloser",
                    SymbolType::Struct,
                    "io/val.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_val_close",
                    "Close",
                    SymbolType::Method,
                    "io/val.go",
                    3,
                    Some("() error"),
                    None,
                ),
                go_symbol(
                    "t_untagged",
                    "UntaggedCloser",
                    SymbolType::Struct,
                    "io/untagged.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_untagged_close",
                    "Close",
                    SymbolType::Method,
                    "io/untagged.go",
                    3,
                    Some("() error"),
                    None,
                ),
            ])
            .unwrap();
        insert_edge_raw(
            &store,
            "t_ptr",
            "io/ptr.go",
            "Close",
            Some("m_ptr_close"),
            "contains",
            3,
            Some(r#"{"receiver":"pointer"}"#),
        );
        insert_edge_raw(
            &store,
            "t_val",
            "io/val.go",
            "Close",
            Some("m_val_close"),
            "contains",
            3,
            Some(r#"{"receiver":"value"}"#),
        );
        insert_edge_raw(
            &store,
            "t_untagged",
            "io/untagged.go",
            "Close",
            Some("m_untagged_close"),
            "contains",
            3,
            None,
        );

        assert_eq!(store.mine_go_interface_implementations().unwrap(), 3);
        let edges = mined_go_edges(&store);
        let metadata_by_source: HashMap<String, Option<String>> = edges
            .into_iter()
            .map(|(source, _, _, _, metadata)| (source, metadata))
            .collect();
        assert_eq!(
            metadata_by_source["t_ptr"].as_deref(),
            Some(r#"{"receiver_set":"pointer"}"#),
            "pointer-only satisfaction must be tagged"
        );
        assert_eq!(metadata_by_source["t_val"], None);
        assert_eq!(
            metadata_by_source["t_untagged"], None,
            "absent receiver metadata is treated as a value receiver"
        );
    }

    /// Test-only fakes are ordinary Go types — `_test.go` files are retained.
    #[test]
    fn go_mining_retains_test_only_fakes() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                go_symbol(
                    "i_pinger",
                    "Pinger",
                    SymbolType::Interface,
                    "cache/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "i_pinger_ping",
                    "Ping",
                    SymbolType::Method,
                    "cache/iface.go",
                    2,
                    Some("() error"),
                    Some("i_pinger"),
                ),
                go_symbol(
                    "t_fake",
                    "FakePinger",
                    SymbolType::Struct,
                    "cache/fake_test.go",
                    4,
                    None,
                    None,
                ),
                go_symbol(
                    "m_fake_ping",
                    "Ping",
                    SymbolType::Method,
                    "cache/fake_test.go",
                    6,
                    Some("() error"),
                    None,
                ),
            ])
            .unwrap();
        insert_edge_raw(
            &store,
            "t_fake",
            "cache/fake_test.go",
            "Ping",
            Some("m_fake_ping"),
            "contains",
            6,
            None,
        );

        assert_eq!(store.mine_go_interface_implementations().unwrap(), 1);
        let edges = mined_go_edges(&store);
        assert_eq!(edges[0].0, "t_fake");
        assert_eq!(edges[0].1, "Pinger");
        assert_eq!(
            edges[0].3, 4,
            "edge line is the fake's type declaration line"
        );
    }

    /// The empty interface matches EVERY type — deriving that cross-product
    /// would be pure graph noise, so spec-less interfaces are skipped.
    #[test]
    fn go_mining_skips_empty_interfaces() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                go_symbol(
                    "i_any",
                    "Any",
                    SymbolType::Interface,
                    "pkg/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "t_thing",
                    "Thing",
                    SymbolType::Struct,
                    "pkg/thing.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_thing_run",
                    "Run",
                    SymbolType::Method,
                    "pkg/thing.go",
                    3,
                    Some("() error"),
                    None,
                ),
            ])
            .unwrap();
        insert_edge_raw(
            &store,
            "t_thing",
            "pkg/thing.go",
            "Run",
            Some("m_thing_run"),
            "contains",
            3,
            None,
        );

        assert_eq!(store.mine_go_interface_implementations().unwrap(), 0);
        assert!(mined_go_edges(&store).is_empty());
    }

    /// An interface embedding an UNRESOLVED target (e.g. a library
    /// `io.Reader`) has an unknowable requirement set — emitting from the
    /// partial set would be a confident false positive, so it is skipped.
    #[test]
    fn go_mining_skips_interfaces_with_unresolved_embeds() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                go_symbol(
                    "i_sub",
                    "Sub",
                    SymbolType::Interface,
                    "pkg/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "i_sub_ping",
                    "Ping",
                    SymbolType::Method,
                    "pkg/iface.go",
                    3,
                    Some("() error"),
                    Some("i_sub"),
                ),
                go_symbol(
                    "t_impl",
                    "Impl",
                    SymbolType::Struct,
                    "pkg/impl.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_impl_ping",
                    "Ping",
                    SymbolType::Method,
                    "pkg/impl.go",
                    3,
                    Some("() error"),
                    None,
                ),
            ])
            .unwrap();
        // Sub embeds a library interface the index cannot see: extends target
        // is NULL (unresolved) — Sub's true requirements are unknowable.
        insert_edge_raw(
            &store,
            "i_sub",
            "pkg/iface.go",
            "LibIface",
            None,
            "extends",
            2,
            None,
        );
        insert_edge_raw(
            &store,
            "t_impl",
            "pkg/impl.go",
            "Ping",
            Some("m_impl_ping"),
            "contains",
            3,
            None,
        );

        assert_eq!(
            store.mine_go_interface_implementations().unwrap(),
            0,
            "a partial requirement set must never emit implements edges"
        );
        assert!(mined_go_edges(&store).is_empty());
    }

    /// The receiver-independent canonical signature: names and whitespace are
    /// stripped, types kept; grouped params expand; receivers are skipped;
    /// the method name participates in identity.
    #[test]
    fn canonical_go_signature_strips_names_and_receivers() {
        assert_eq!(
            canonical_go_method_signature("Ping", "(ctx context.Context) error"),
            canonical_go_method_signature("Ping", "(context.Context) error")
        );
        // Grouped params expand: `(a, b string)` == `(x string, y string)`.
        assert_eq!(
            canonical_go_method_signature("Cmp", "(a, b string) int"),
            canonical_go_method_signature("Cmp", "(x string, y string) int")
        );
        // A leading receiver group is skipped.
        assert_eq!(
            canonical_go_method_signature("Ping", "(m Memory) Ping(ctx context.Context) error"),
            canonical_go_method_signature("Ping", "(context.Context) error")
        );
        // Parenthesized single result == bare result.
        assert_eq!(
            canonical_go_method_signature("Do", "() (error)"),
            canonical_go_method_signature("Do", "() error")
        );
        // `chan`/`func` keyword types are never mistaken for names/receivers.
        assert_eq!(
            canonical_go_method_signature("Feed", "(chan int) func(int) error"),
            canonical_go_method_signature("Feed", "(c chan int) func(int) error")
        );
        // Multi-value named results normalize like params.
        assert_eq!(
            canonical_go_method_signature("Read", "(p []byte) (n int, err error)"),
            canonical_go_method_signature("Read", "([]byte) (int, error)")
        );
        // The method NAME participates in identity.
        assert_ne!(
            canonical_go_method_signature("Ping", "() error"),
            canonical_go_method_signature("Pong", "() error")
        );
        // Different parameter types stay different.
        assert_ne!(
            canonical_go_method_signature("Set", "(string, []byte) error"),
            canonical_go_method_signature("Set", "(string, string) error")
        );
    }

    // ---- Track H — semantic-anchor query modes ----

    /// Raw anchor insert so several files' anchors can accumulate without the
    /// per-file replace semantics of `replace_semantic_anchors_for_file`.
    fn insert_anchor_raw(store: &SymbolStore, id: &str, file: &str, value: &str, preview: &str) {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO semantic_anchors
             (id, file_path, kind, value, line, character, preview, confidence, indexed_at)
             VALUES (?1, ?2, 'section_label', ?3, 10, 0, ?4, 0.9, 0)",
            params![id, file, value, preview],
        )
        .unwrap();
    }

    /// A reversed-word-order query misses in Phrase mode but hits in AllTerms
    /// mode with full coverage — the false-empty fix Track H exists for.
    #[test]
    fn anchor_all_terms_hits_where_phrase_misses() {
        let store = SymbolStore::in_memory().unwrap();
        insert_anchor_raw(
            &store,
            "an1",
            "components/header.tsx",
            "Mobile navigation",
            "{/* Mobile navigation */}",
        );

        let phrase = store
            .search_semantic_anchors_mode("navigation mobile", None, 10, AnchorQueryMode::Phrase)
            .unwrap();
        assert!(phrase.results.is_empty());
        assert!(
            !phrase.empty_query,
            "a legitimate no-match is NOT an empty query"
        );
        assert_eq!(phrase.mode, AnchorQueryMode::Phrase);

        let all = store
            .search_semantic_anchors_mode("navigation mobile", None, 10, AnchorQueryMode::AllTerms)
            .unwrap();
        assert_eq!(all.results.len(), 1);
        assert_eq!(all.results[0].anchor.id, "an1");
        assert_eq!(all.results[0].matched_terms, Some(2));
        assert_eq!(all.results[0].total_terms, Some(2));
        assert_eq!(all.total_terms, 2);
        assert_eq!(all.mode, AnchorQueryMode::AllTerms);

        // AllTerms is still AND: a query with one non-matching term misses.
        let miss = store
            .search_semantic_anchors_mode(
                "navigation zzz_absent",
                None,
                10,
                AnchorQueryMode::AllTerms,
            )
            .unwrap();
        assert!(miss.results.is_empty());
        assert!(!miss.empty_query);
    }

    /// AnyTerms counts per-row coverage and ranks full-coverage rows above
    /// partial ones (same confidence, so coverage decides).
    #[test]
    fn anchor_any_terms_counts_and_ranks_by_coverage() {
        let store = SymbolStore::in_memory().unwrap();
        insert_anchor_raw(&store, "an_full", "a.tsx", "Mobile navigation icons", "p");
        insert_anchor_raw(&store, "an_partial", "b.tsx", "Desktop navigation", "p");

        let any = store
            .search_semantic_anchors_mode(
                "mobile navigation icons",
                None,
                10,
                AnchorQueryMode::AnyTerms,
            )
            .unwrap();
        assert_eq!(any.total_terms, 3);
        assert_eq!(any.results.len(), 2);
        assert_eq!(any.results[0].anchor.id, "an_full");
        assert_eq!(any.results[0].matched_terms, Some(3));
        assert_eq!(any.results[0].total_terms, Some(3));
        assert_eq!(any.results[1].anchor.id, "an_partial");
        assert_eq!(any.results[1].matched_terms, Some(1));
        assert!(
            any.results[0].score > any.results[1].score,
            "full coverage must outrank partial coverage"
        );
    }

    /// An empty (or whitespace-only) query is FLAGGED, in every mode — never
    /// silently identical to a legitimate no-match.
    #[test]
    fn anchor_empty_query_is_flagged_never_a_silent_no_match() {
        let store = SymbolStore::in_memory().unwrap();
        insert_anchor_raw(&store, "an1", "a.tsx", "Right side actions", "p");

        for mode in [
            AnchorQueryMode::Phrase,
            AnchorQueryMode::AllTerms,
            AnchorQueryMode::AnyTerms,
        ] {
            let outcome = store
                .search_semantic_anchors_mode("   ", None, 10, mode)
                .unwrap();
            assert!(outcome.empty_query, "{mode:?} must flag the empty query");
            assert!(outcome.results.is_empty());
            assert_eq!(outcome.total_terms, 0);
        }

        let miss = store
            .search_semantic_anchors_mode("zzz_not_here", None, 10, AnchorQueryMode::Phrase)
            .unwrap();
        assert!(!miss.empty_query);
        assert!(miss.results.is_empty());
    }

    /// The legacy entry point keeps its exact Phrase behavior and shape; the
    /// new per-result term fields stay `None` there.
    #[test]
    fn anchor_legacy_search_delegates_to_phrase_mode() {
        let store = SymbolStore::in_memory().unwrap();
        insert_anchor_raw(&store, "an1", "a.tsx", "Mobile navigation", "p");

        let results = store
            .search_semantic_anchors("Mobile navigation", None, 10)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].anchor.id, "an1");
        assert_eq!(results[0].matched_terms, None);
        assert_eq!(results[0].total_terms, None);

        assert!(store
            .search_semantic_anchors("", None, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn anchor_query_mode_parse_accepts_wire_names_only() {
        assert_eq!(
            AnchorQueryMode::parse("phrase"),
            Some(AnchorQueryMode::Phrase)
        );
        assert_eq!(
            AnchorQueryMode::parse("all_terms"),
            Some(AnchorQueryMode::AllTerms)
        );
        assert_eq!(
            AnchorQueryMode::parse("any_terms"),
            Some(AnchorQueryMode::AnyTerms)
        );
        assert_eq!(AnchorQueryMode::parse("fuzzy"), None);
        assert_eq!(AnchorQueryMode::parse(""), None);
    }

    /// Review finding G — LIKE metacharacters in a query term are LITERAL:
    /// `foo_bar` must not wildcard-match a `foo-bar` value, and
    /// `matched_terms` must never exceed what Rust literally verified.
    #[test]
    fn anchor_all_terms_underscore_is_literal_not_wildcard() {
        let store = SymbolStore::in_memory().unwrap();
        insert_anchor_raw(&store, "an_underscore", "a.css", "token foo_bar", "p");
        insert_anchor_raw(&store, "an_dash", "b.css", "token foo-bar", "p");

        let all = store
            .search_semantic_anchors_mode("foo_bar", None, 10, AnchorQueryMode::AllTerms)
            .unwrap();
        assert_eq!(all.results.len(), 1, "the dash variant must NOT match");
        assert_eq!(all.results[0].anchor.id, "an_underscore");
        assert_eq!(all.results[0].matched_terms, Some(1));

        // AnyTerms on the dash row: only the literal `token` term matched —
        // the old SQL-counted path would have overclaimed 2.
        let any = store
            .search_semantic_anchors_mode("foo_bar token", None, 10, AnchorQueryMode::AnyTerms)
            .unwrap();
        let dash = any
            .results
            .iter()
            .find(|result| result.anchor.id == "an_dash")
            .expect("dash row matches via its `token` term");
        assert_eq!(dash.matched_terms, Some(1));
        let underscore = any
            .results
            .iter()
            .find(|result| result.anchor.id == "an_underscore")
            .expect("underscore row matches both terms");
        assert_eq!(underscore.matched_terms, Some(2));
    }

    /// Review finding H — SQLite LIKE case-folds ASCII ONLY, so accented
    /// queries used to false-empty against values differing only in the case
    /// of a non-ASCII letter. Rust's Unicode-lowercased verification (fed by
    /// wildcard-degraded candidate patterns) must match both directions.
    #[test]
    fn anchor_accented_terms_match_across_unicode_case() {
        let store = SymbolStore::in_memory().unwrap();
        insert_anchor_raw(&store, "an_upper", "a.tsx", "CAFÉ menu", "p");

        for mode in [
            AnchorQueryMode::Phrase,
            AnchorQueryMode::AllTerms,
            AnchorQueryMode::AnyTerms,
        ] {
            let outcome = store
                .search_semantic_anchors_mode("café", None, 10, mode)
                .unwrap();
            assert_eq!(
                outcome.results.len(),
                1,
                "{mode:?} must match CAFÉ with query café"
            );
            assert_eq!(outcome.results[0].anchor.id, "an_upper");
        }

        // The reverse direction: uppercase accented query, lowercase value.
        insert_anchor_raw(&store, "an_lower", "b.tsx", "menú día", "p");
        let all = store
            .search_semantic_anchors_mode("MENÚ DÍA", None, 10, AnchorQueryMode::AllTerms)
            .unwrap();
        assert_eq!(all.results.len(), 1);
        assert_eq!(all.results[0].anchor.id, "an_lower");
        assert_eq!(all.results[0].matched_terms, Some(2));
    }

    /// Review finding E — the `usage` allowlist must include
    /// `css_custom_property`: a `var(--token)` usage edge targeting a
    /// globally-unique design token in ANOTHER stylesheet resolved under the
    /// old kind-blind back-fill and must keep resolving.
    #[test]
    fn usage_backfill_resolves_css_custom_property_targets() {
        let store = SymbolStore::in_memory().unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute_batch(
                r#"
                INSERT INTO symbols (id, name, qualified_name, symbol_type, file_path,
                                     start_line, start_char, end_line, end_char, indexed_at)
                VALUES
                    ('s_token', '--accent', 'tokens.css:--accent', 'css_custom_property', 'tokens.css', 3, 0, 3, 0, 0);

                INSERT INTO symbol_relationships
                    (source_symbol_id, source_file_path, target_name, target_symbol_id, relationship_type, line)
                VALUES
                    ('r_button', 'button.css', '--accent', NULL, 'usage', 12);
                "#,
            )
            .unwrap();
        }

        assert_eq!(store.backfill_unresolved_relationship_targets().unwrap(), 1);
        let conn = store.conn.lock().unwrap();
        let (target, strategy): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT target_symbol_id, resolution_strategy FROM symbol_relationships
                 WHERE source_symbol_id = 'r_button'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(target.as_deref(), Some("s_token"));
        assert_eq!(strategy.as_deref(), Some("global_unique"));
    }

    /// Review finding D — the `symbol_relationships` PK
    /// `(source_symbol_id, target_name, relationship_type, line)` cannot hold
    /// two same-NAMED interfaces satisfied by one type; the whole colliding
    /// PK group must be skipped (false negative over a silent clobber) and
    /// the return value must count the rows actually inserted.
    #[test]
    fn go_mining_same_named_interface_collision_skips_group_and_counts_inserts() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                go_symbol(
                    "i_store_a",
                    "Store",
                    SymbolType::Interface,
                    "pkga/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "i_store_a_ping",
                    "Ping",
                    SymbolType::Method,
                    "pkga/iface.go",
                    2,
                    Some("() error"),
                    Some("i_store_a"),
                ),
                go_symbol(
                    "i_store_b",
                    "Store",
                    SymbolType::Interface,
                    "pkgb/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "i_store_b_ping",
                    "Ping",
                    SymbolType::Method,
                    "pkgb/iface.go",
                    2,
                    Some("() error"),
                    Some("i_store_b"),
                ),
                // Control pair: DISTINCT names — both edges must be emitted.
                go_symbol(
                    "i_alpha",
                    "Alpha",
                    SymbolType::Interface,
                    "pkgd/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "i_alpha_ping",
                    "Ping",
                    SymbolType::Method,
                    "pkgd/iface.go",
                    2,
                    Some("() error"),
                    Some("i_alpha"),
                ),
                go_symbol(
                    "i_beta",
                    "Beta",
                    SymbolType::Interface,
                    "pkge/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "i_beta_ping",
                    "Ping",
                    SymbolType::Method,
                    "pkge/iface.go",
                    2,
                    Some("() error"),
                    Some("i_beta"),
                ),
                go_symbol(
                    "t_impl",
                    "Impl",
                    SymbolType::Struct,
                    "pkgc/impl.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_impl_ping",
                    "Ping",
                    SymbolType::Method,
                    "pkgc/impl.go",
                    3,
                    Some("() error"),
                    None,
                ),
            ])
            .unwrap();
        insert_edge_raw(
            &store,
            "t_impl",
            "pkgc/impl.go",
            "Ping",
            Some("m_impl_ping"),
            "contains",
            3,
            None,
        );

        // Impl satisfies all four interfaces (exported, package-portable
        // `Ping() error`), but the two named `Store` collide on the PK
        // `(t_impl, 'Store', 'implements', 1)` — the schema cannot represent
        // both, so BOTH are skipped and the count reflects only real inserts.
        assert_eq!(store.mine_go_interface_implementations().unwrap(), 2);
        let edges = mined_go_edges(&store);
        let keys: Vec<(String, String, Option<String>)> = edges
            .iter()
            .map(|(source, target, target_id, ..)| {
                (source.clone(), target.clone(), target_id.clone())
            })
            .collect();
        assert_eq!(
            keys,
            vec![
                (
                    "t_impl".to_string(),
                    "Alpha".to_string(),
                    Some("i_alpha".to_string())
                ),
                (
                    "t_impl".to_string(),
                    "Beta".to_string(),
                    Some("i_beta".to_string())
                ),
            ],
            "zero implements edges for the colliding (type, 'Store') pair; \
             the distinct-name control pair emits both"
        );
    }

    /// Review finding F — canonical Go signatures are compared as TEXT, so a
    /// bare non-universe identifier (`Config`) only proves a match inside the
    /// declaring package. Cross-package satisfaction requires every type in
    /// the signature to be package-portable; same-package comparison is
    /// unchanged.
    #[test]
    fn go_mining_cross_package_requires_package_portable_signature() {
        let store = SymbolStore::in_memory().unwrap();
        store
            .upsert_symbols(&[
                go_symbol(
                    "i_applier",
                    "Applier",
                    SymbolType::Interface,
                    "pkga/iface.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "i_applier_apply",
                    "Apply",
                    SymbolType::Method,
                    "pkga/iface.go",
                    2,
                    Some("(Config) error"),
                    Some("i_applier"),
                ),
                go_symbol(
                    "i_pinger",
                    "Pinger",
                    SymbolType::Interface,
                    "pkga/iface.go",
                    5,
                    None,
                    None,
                ),
                go_symbol(
                    "i_pinger_ping",
                    "Ping",
                    SymbolType::Method,
                    "pkga/iface.go",
                    6,
                    Some("(string) error"),
                    Some("i_pinger"),
                ),
                // Same-package implementer: its bare `Config` IS pkga's.
                go_symbol(
                    "t_local",
                    "LocalApplier",
                    SymbolType::Struct,
                    "pkga/impl.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_local_apply",
                    "Apply",
                    SymbolType::Method,
                    "pkga/impl.go",
                    3,
                    Some("(c Config) error"),
                    None,
                ),
                // Cross-package implementer: ITS `Config` is pkgb's DIFFERENT
                // type — the canonical strings match textually only.
                go_symbol(
                    "t_remote",
                    "RemoteApplier",
                    SymbolType::Struct,
                    "pkgb/impl.go",
                    1,
                    None,
                    None,
                ),
                go_symbol(
                    "m_remote_apply",
                    "Apply",
                    SymbolType::Method,
                    "pkgb/impl.go",
                    3,
                    Some("(c Config) error"),
                    None,
                ),
                go_symbol(
                    "m_remote_ping",
                    "Ping",
                    SymbolType::Method,
                    "pkgb/impl.go",
                    6,
                    Some("(s string) error"),
                    None,
                ),
            ])
            .unwrap();
        insert_edge_raw(
            &store,
            "t_local",
            "pkga/impl.go",
            "Apply",
            Some("m_local_apply"),
            "contains",
            3,
            None,
        );
        insert_edge_raw(
            &store,
            "t_remote",
            "pkgb/impl.go",
            "Apply",
            Some("m_remote_apply"),
            "contains",
            3,
            None,
        );
        insert_edge_raw(
            &store,
            "t_remote",
            "pkgb/impl.go",
            "Ping",
            Some("m_remote_ping"),
            "contains",
            6,
            None,
        );

        assert_eq!(store.mine_go_interface_implementations().unwrap(), 2);
        let edges = mined_go_edges(&store);
        let keys: Vec<(String, String)> = edges
            .iter()
            .map(|(source, target, ..)| (source.clone(), target.clone()))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("t_local".to_string(), "Applier".to_string()),
                ("t_remote".to_string(), "Pinger".to_string()),
            ],
            "same-package non-portable matches; cross-package builtin-only \
             matches; cross-package `Config` must NOT emit an edge"
        );
    }

    /// The package-portability classifier itself, over signatures built by
    /// `canonical_go_method_signature` (the exact strings satisfaction sees).
    #[test]
    fn go_package_portable_signature_classification() {
        let canonical = canonical_go_method_signature;
        // Universe/builtin, qualified, and composites thereof are portable.
        assert!(canonical_go_signature_is_package_portable(&canonical(
            "Ping", "() error"
        )));
        assert!(canonical_go_signature_is_package_portable(&canonical(
            "Get",
            "(k string, v []byte) (int, error)"
        )));
        assert!(canonical_go_signature_is_package_portable(&canonical(
            "Watch",
            "(ctx context.Context, m map[string][]int64) error"
        )));
        assert!(canonical_go_signature_is_package_portable(&canonical(
            "Feed",
            "(c chan int, p *string, rest ...float64) error"
        )));
        assert!(canonical_go_signature_is_package_portable(&canonical(
            "Fixed",
            "(b [4]byte) error"
        )));
        // Bare non-universe identifiers resolve per-package: NOT portable.
        assert!(!canonical_go_signature_is_package_portable(&canonical(
            "Apply",
            "(Config) error"
        )));
        assert!(!canonical_go_signature_is_package_portable(&canonical(
            "Load",
            "(b [n]byte) error"
        )));
        assert!(!canonical_go_signature_is_package_portable(&canonical(
            "Merge",
            "(m map[string]Config) error"
        )));
    }
}
