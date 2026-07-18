//! Fixture 5 — ranking and context cost (Symbols-Index handoff, hard gates).
//!
//! Exercises the deterministic ranking corpus at
//! `tests/fixtures/symbol_tracks/fixture5_ranking`:
//!
//! - `src/toolbar_builder.ts` — small file with the exact expected answer
//!   (`buildNavigationToolbar`);
//! - `src/data_helpers.ts` — large distractor full of repeated weak/generic
//!   terms (get/set/data/item/list/value) plus a few weak-partial `toolbar`
//!   hits;
//! - `docs/toolbar_guide.md` / `docs/venue_listing.md` — relevant and
//!   irrelevant Markdown headings; the toolbar guide's "Navigation toolbar
//!   reference" section is deliberately LONGER than 121 lines so the
//!   120-line doc-range cap is actually exercised (never vacuous);
//! - `src/switcher_panel.ts` — strong hits near BOTH the top
//!   (`LanguageSwitcher`) and bottom (`renderLanguageSwitcher`) with ~240
//!   lines of unrelated filler between (distant-evidence clustering);
//! - `data/huge_payload.json` — a single-line JSON payload of several hundred
//!   KB (tool-result byte-budget case).
//!
//! Every test copies the fixture tree into a fresh `TempDir` workspace and
//! indexes it there by relative path — never the repo checkout itself
//! (`write_symbol_fixture` pattern). Hard release gates run as normal tests;
//! the metric dump (top-3 recall / MRR / bytes) follows the bench convention
//! and is `#[ignore]`-gated:
//!
//!   `cargo test --release --test symbol_ranking -- --ignored --nocapture`

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;
use zblade_lib::blade_protocol::ContextRange;
use zblade_lib::context_pack::{build_context_pack, ContextPackRequest};
use zblade_lib::language_service::LanguageService;
use zblade_lib::symbol_index::search::execute_search;
use zblade_lib::symbol_index::{SearchQuery, SymbolStore};
use zblade_lib::tools::truncate_large_content;

use support::{copy_dir_recursive, index_corpus, IndexedCorpus};

/// Byte cap for a hard tool result (mirrors `tools::MAX_TOOL_RESULT_BYTES`,
/// which is private — hard gate: at most 48 KiB) plus the slack the in-tree
/// truncation tests allow for the truncation banner.
const TOOL_RESULT_BYTE_CAP: usize = 48 * 1024;
const TRUNCATION_SLACK: usize = 512;
/// Range caps (mirror `context_pack::MAX_CONTEXT_RANGE_LINES` /
/// `MAX_DOC_RANGE_LINES` / `MAX_CONTEXT_RANGES`, which are private).
const MAX_RANGE_LINES: u32 = 160;
const MAX_DOC_RANGE_LINES: u32 = 120;
const MAX_RANGES_PER_FILE: usize = 3;

const EXPECTED_ANSWER_FILE: &str = "src/toolbar_builder.ts";
const DISTRACTOR_FILE: &str = "src/data_helpers.ts";
const DISTANT_EVIDENCE_FILE: &str = "src/switcher_panel.ts";
const RELEVANT_DOC: &str = "docs/toolbar_guide.md";
const IRRELEVANT_DOC: &str = "docs/venue_listing.md";
const HUGE_JSON: &str = "data/huge_payload.json";

fn fixture_source_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/symbol_tracks/fixture5_ranking")
}

/// Copy the fixture corpus into a fresh TempDir workspace.
fn fixture_workspace() -> TempDir {
    let workspace = TempDir::new().expect("create fixture workspace");
    copy_dir_recursive(&fixture_source_dir(), workspace.path());
    workspace
}

/// Cold-index a fresh copy of the fixture workspace (index DB in its own
/// separate TempDir, as `index_corpus` does).
fn indexed_fixture() -> (TempDir, IndexedCorpus) {
    let workspace = fixture_workspace();
    let corpus = index_corpus(workspace.path());
    (workspace, corpus)
}

/// Distinct file paths of `query`'s results, best-score-first (result order is
/// already score-descending; the first occurrence of a file is its rank).
fn ranked_files(store: &SymbolStore, query: &str) -> Vec<String> {
    let results = execute_search(store, &SearchQuery::text(query)).expect("execute_search");
    let mut files = Vec::new();
    for result in &results {
        if !files.contains(&result.symbol.file_path) {
            files.push(result.symbol.file_path.clone());
        }
    }
    files
}

fn rank_of(files: &[String], suffix: &str) -> Option<usize> {
    files.iter().position(|file| file.ends_with(suffix))
}

// ---- Release gate: "Expected broad-query file — top 3 when fixture contains
// meaningful matching evidence" -----------------------------------------------

#[test]
fn expected_answer_file_ranks_in_top_three() {
    let (_workspace, corpus) = indexed_fixture();

    for query in ["navigation toolbar", "build navigation toolbar"] {
        let files = ranked_files(&corpus.store, query);
        let rank = rank_of(&files, EXPECTED_ANSWER_FILE);
        assert!(
            rank.is_some_and(|rank| rank < 3),
            "expected {EXPECTED_ANSWER_FILE} in the top 3 files for {query:?}, got: {files:?}"
        );
    }
}

// ---- Release gate: "Weak-frequency distractor — cannot win solely through
// repeated low-confidence hits" ------------------------------------------------

#[test]
fn weak_frequency_distractor_cannot_outrank_expected_answer() {
    let (_workspace, corpus) = indexed_fixture();

    let query = "build navigation toolbar";
    let results = execute_search(&corpus.store, &SearchQuery::text(query)).expect("execute_search");

    let best_expected = results
        .iter()
        .filter(|result| result.symbol.file_path.ends_with(EXPECTED_ANSWER_FILE))
        .map(|result| result.score)
        .fold(0.0f32, f32::max);
    let best_distractor = results
        .iter()
        .filter(|result| result.symbol.file_path.ends_with(DISTRACTOR_FILE))
        .map(|result| result.score)
        .fold(0.0f32, f32::max);

    assert!(
        best_expected > 0.0,
        "expected-answer file must produce a scored hit for {query:?}"
    );
    assert!(
        best_expected > best_distractor,
        "the weak-frequency distractor must not outrank the expected answer: \
         expected {best_expected} vs distractor {best_distractor}"
    );

    // Rank order at the file level must agree: the distractor's dozens of
    // generic get/set/data/item symbols never beat one strong exact answer.
    let files = ranked_files(&corpus.store, query);
    let expected_rank = rank_of(&files, EXPECTED_ANSWER_FILE).expect("expected file ranked");
    if let Some(distractor_rank) = rank_of(&files, DISTRACTOR_FILE) {
        assert!(
            expected_rank < distractor_rank,
            "distractor file outranked the expected answer: {files:?}"
        );
    }

    // Pure-generic queries built from the distractor's repeated terms must not
    // surface the expected-answer file either way — but more importantly the
    // distractor may only match through genuinely covered tokens, never through
    // frequency: every returned distractor hit must carry a positive score
    // (zero-score retrieval noise is filtered before ranking).
    for result in &results {
        assert!(
            result.score > 0.0,
            "execute_search must never return zero-score noise: {:?}",
            result.symbol.qualified_name
        );
    }
}

// ---- Release gate (Track G): "Short-fragment prefix match — impossible when
// either token is under 3 characters" ------------------------------------------

#[test]
fn short_fragment_query_returns_zero_results() {
    let (_workspace, corpus) = indexed_fixture();

    // 2-char fragments of real fixture tokens ("na(vigation)", "to(olbar)",
    // "lg", "sw(itcher)") — prefix/char-overlap matching under 3 chars must
    // admit a truthful miss instead of manufacturing noise.
    for query in ["na to", "to na", "lg sw", "sw pa"] {
        let results =
            execute_search(&corpus.store, &SearchQuery::text(query)).expect("execute_search");
        assert!(
            results.is_empty(),
            "short-fragment query {query:?} must return zero results, got: {:?}",
            results
                .iter()
                .map(|result| (&result.symbol.qualified_name, result.score))
                .collect::<Vec<_>>()
        );
    }
}

// ---- Release gates: "Normal tool result ≤ 24 KiB / hard limit ≤ 48 KiB,
// including single-line payloads" — the tools-layer byte budget over the huge
// single-line JSON ------------------------------------------------------------

#[test]
fn huge_single_line_json_truncates_under_budget_with_head_and_tail() {
    let workspace = fixture_workspace();
    let raw = std::fs::read_to_string(workspace.path().join(HUGE_JSON)).expect("read huge json");

    // Preconditions the fixture must keep: single line, several hundred KB.
    assert_eq!(raw.lines().count(), 1, "fixture must be single-line JSON");
    assert!(
        raw.len() > 300 * 1024,
        "fixture must be several hundred KB, got {} bytes",
        raw.len()
    );

    let truncated = truncate_large_content(&raw);
    assert!(
        truncated.len() <= TOOL_RESULT_BYTE_CAP + TRUNCATION_SLACK,
        "truncated single-line JSON ({} bytes) must stay within {} + {} slack",
        truncated.len(),
        TOOL_RESULT_BYTE_CAP,
        TRUNCATION_SLACK
    );
    assert!(
        truncated.contains("[TRUNCATED:"),
        "truncation banner missing"
    );
    assert!(
        truncated.contains("showing first") && truncated.contains("bytes]"),
        "single-line payload must fall back to byte-based head/tail truncation"
    );
    // Head marker: the result must still begin with the original head bytes...
    assert!(
        truncated.starts_with(&raw[..64]),
        "truncated result must preserve the head of the payload"
    );
    // ...and end with the original tail bytes.
    assert!(
        truncated.ends_with(&raw[raw.len() - 64..]),
        "truncated result must preserve the tail of the payload"
    );
}

// ---- Context-pack gates (pub API `zblade_lib::context_pack::build_context_pack`):
// "Ordinary suggested range ≤ 160 lines", "Markdown range ≤ 120 lines", at most
// 3 disjoint ranges per file ---------------------------------------------------

/// Index the workspace into its own `.zblade/index/symbols.db` — the DB
/// location `build_context_pack` reopens — then drop the indexing service.
fn index_workspace_in_place(root: &Path) {
    let db_path = root.join(".zblade").join("index").join("symbols.db");
    std::fs::create_dir_all(db_path.parent().expect("db parent")).expect("create .zblade/index");
    let store = Arc::new(SymbolStore::new(&db_path).expect("create workspace symbol store"));
    let service = LanguageService::new(root.to_path_buf(), store).expect("create language service");
    service
        .index_directory("")
        .expect("index fixture workspace");
}

fn context_pack_request(query: &str) -> ContextPackRequest {
    ContextPackRequest {
        id: "fixture5".to_string(),
        query: query.to_string(),
        queries: Vec::new(),
        intent: None,
        max_results: Some(8),
        include_tests: Some(false),
        include_docs: Some(true),
        include_memory: Some(false),
        include_project_index_min: Some(false),
    }
}

fn assert_ranges_capped_and_disjoint(path: &str, ranges: &[ContextRange], max_lines: u32) {
    assert!(
        ranges.len() <= MAX_RANGES_PER_FILE,
        "{path}: at most {MAX_RANGES_PER_FILE} suggested ranges, got {ranges:?}"
    );
    for range in ranges {
        assert!(
            range.start_line >= 1 && range.end_line >= range.start_line,
            "{path}: malformed range {range:?}"
        );
        let span = range.end_line - range.start_line + 1;
        assert!(
            span <= max_lines,
            "{path}: suggested range {range:?} spans {span} lines (cap {max_lines})"
        );
    }
    for pair in ranges.windows(2) {
        assert!(
            pair[0].end_line < pair[1].start_line,
            "{path}: suggested ranges must be disjoint and ordered, got {ranges:?}"
        );
    }
}

#[test]
fn context_pack_ranks_expected_file_and_caps_doc_ranges() {
    let workspace = fixture_workspace();
    index_workspace_in_place(workspace.path());

    let pack = build_context_pack(
        workspace.path(),
        None,
        &[],
        &context_pack_request("navigation toolbar"),
    );
    assert!(pack.error.is_none(), "context pack error: {:?}", pack.error);

    // Expected-answer file in the top 3 primary files.
    let primary_rank = pack
        .primary_files
        .iter()
        .position(|file| file.path.ends_with(EXPECTED_ANSWER_FILE));
    assert!(
        primary_rank.is_some_and(|rank| rank < 3),
        "expected {EXPECTED_ANSWER_FILE} in the top 3 primary files, got: {:?}",
        pack.primary_files
            .iter()
            .map(|file| (&file.path, file.score))
            .collect::<Vec<_>>()
    );

    // Relevant Markdown heading surfaces; the irrelevant venue doc does not.
    assert!(
        pack.related_docs
            .iter()
            .any(|doc| doc.path.ends_with(RELEVANT_DOC)),
        "relevant doc {RELEVANT_DOC} missing from related_docs: {:?}",
        pack.related_docs
            .iter()
            .map(|doc| &doc.path)
            .collect::<Vec<_>>()
    );
    assert!(
        !pack
            .related_docs
            .iter()
            .any(|doc| doc.path.ends_with(IRRELEVANT_DOC)),
        "irrelevant doc {IRRELEVANT_DOC} must not appear for a navigation query"
    );

    // Markdown gate: every doc range ≤ 120 lines. The fixture's "Navigation
    // toolbar reference" section is >121 lines by construction (precondition
    // asserted below), so this cap check is exercised for real — a range over
    // that section MUST have been clamped, never emitted at natural length.
    let guide_raw = std::fs::read_to_string(workspace.path().join(RELEVANT_DOC))
        .expect("read relevant fixture doc");
    let heading_line = guide_raw
        .lines()
        .position(|line| line.starts_with("## Navigation toolbar reference"))
        .expect("fixture doc must keep the long reference section")
        as u32
        + 1;
    let next_heading_line = guide_raw
        .lines()
        .enumerate()
        .position(|(index, line)| index as u32 + 1 > heading_line && line.starts_with("## "))
        .expect("long section must be followed by another heading") as u32
        + 1;
    assert!(
        next_heading_line - heading_line > MAX_DOC_RANGE_LINES + 1,
        "fixture precondition: the reference section must span more than {} lines, got {}",
        MAX_DOC_RANGE_LINES + 1,
        next_heading_line - heading_line
    );
    let mut saw_clamped_long_section = false;
    for doc in &pack.related_docs {
        for range in &doc.suggested_ranges {
            let span = range.end_line - range.start_line + 1;
            assert!(
                span <= MAX_DOC_RANGE_LINES,
                "{}: Markdown range {range:?} spans {span} lines (cap {MAX_DOC_RANGE_LINES})",
                doc.path
            );
            if doc.path.ends_with(RELEVANT_DOC) && range.start_line == heading_line {
                // The long section surfaced; its natural extent exceeds the
                // cap, so this range only passes the span check via clamping.
                saw_clamped_long_section = true;
            }
        }
    }
    assert!(
        saw_clamped_long_section,
        "the >121-line reference section must surface as a suggested doc range \
         (otherwise the 120-line cap check is vacuous); related_docs: {:?}",
        pack.related_docs
            .iter()
            .map(|doc| (&doc.path, &doc.suggested_ranges))
            .collect::<Vec<_>>()
    );

    // Ordinary gate: every enriched file's ranges ≤ 3 disjoint, ≤ 160 lines.
    for enriched in &pack.enriched_files {
        assert_ranges_capped_and_disjoint(
            &enriched.path,
            &enriched.suggested_ranges,
            MAX_RANGE_LINES,
        );
    }
}

#[test]
fn context_pack_clusters_distant_evidence_into_capped_disjoint_ranges() {
    let workspace = fixture_workspace();
    index_workspace_in_place(workspace.path());

    let pack = build_context_pack(
        workspace.path(),
        None,
        &[],
        &context_pack_request("language switcher"),
    );
    assert!(pack.error.is_none(), "context pack error: {:?}", pack.error);

    assert!(
        pack.primary_files
            .iter()
            .any(|file| file.path.ends_with(DISTANT_EVIDENCE_FILE)),
        "expected {DISTANT_EVIDENCE_FILE} among primary files, got: {:?}",
        pack.primary_files
            .iter()
            .map(|file| &file.path)
            .collect::<Vec<_>>()
    );

    let enriched = pack
        .enriched_files
        .iter()
        .find(|file| file.path.ends_with(DISTANT_EVIDENCE_FILE))
        .expect("distant-evidence file must be enriched");

    assert_ranges_capped_and_disjoint(&enriched.path, &enriched.suggested_ranges, MAX_RANGE_LINES);

    // Distant evidence: the strong bottom hit (`renderLanguageSwitcher`, ~line
    // 254) and the strong top hit (`LanguageSwitcher`, line 2) must surface as
    // SEPARATE clusters — never one merged ~250-line range.
    assert!(
        enriched.suggested_ranges.len() >= 2,
        "top and bottom evidence must produce at least 2 disjoint ranges, got {:?}",
        enriched.suggested_ranges
    );
    assert!(
        enriched
            .suggested_ranges
            .iter()
            .any(|range| range.start_line <= 30),
        "no suggested range covers the top-of-file evidence: {:?}",
        enriched.suggested_ranges
    );
    assert!(
        enriched
            .suggested_ranges
            .iter()
            .any(|range| range.end_line >= 240),
        "no suggested range covers the bottom-of-file evidence: {:?}",
        enriched.suggested_ranges
    );
}

// ---- Metric dump (bench convention, `#[ignore]`-gated) ------------------------

/// Top-3 recall, MRR, and model-visible-bytes report over the fixture query
/// set. Informational only — the hard gates above stay normal tests.
///
/// Run: `cargo test --release --test symbol_ranking -- --ignored --nocapture`
#[test]
#[ignore]
fn ranking_metrics_report() {
    let (_workspace, corpus) = indexed_fixture();

    let query_set: &[(&str, &str)] = &[
        ("navigation toolbar", EXPECTED_ANSWER_FILE),
        ("build navigation toolbar", EXPECTED_ANSWER_FILE),
        ("language switcher", DISTANT_EVIDENCE_FILE),
        ("render language switcher", DISTANT_EVIDENCE_FILE),
        ("toolbar keyboard access", RELEVANT_DOC),
    ];

    let mut recall_at_3 = 0usize;
    let mut reciprocal_rank_sum = 0.0f64;
    println!("== fixture5 ranking metrics ==");
    for (query, expected) in query_set {
        let files = ranked_files(&corpus.store, query);
        let rank = rank_of(&files, expected);
        let reciprocal = rank.map_or(0.0, |rank| 1.0 / (rank as f64 + 1.0));
        reciprocal_rank_sum += reciprocal;
        if rank.is_some_and(|rank| rank < 3) {
            recall_at_3 += 1;
        }
        println!(
            "query {query:?}: expected {expected}, rank {:?}, rr {reciprocal:.3}, files {files:?}",
            rank.map(|rank| rank + 1)
        );
    }
    println!(
        "top-3 recall: {recall_at_3}/{} = {:.2}",
        query_set.len(),
        recall_at_3 as f64 / query_set.len() as f64
    );
    println!("MRR: {:.3}", reciprocal_rank_sum / query_set.len() as f64);

    let raw = std::fs::read_to_string(fixture_source_dir().join(HUGE_JSON)).expect("read json");
    let truncated = truncate_large_content(&raw);
    println!(
        "huge single-line JSON: raw {} bytes -> model-visible {} bytes (cap {} + {} slack)",
        raw.len(),
        truncated.len(),
        TOOL_RESULT_BYTE_CAP,
        TRUNCATION_SLACK
    );
}
