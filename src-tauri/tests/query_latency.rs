//! M0.4 — In-process query timing (p50/p95/p99).
//!
//! An `#[ignore]`-gated integration test that measures search latency for the
//! two search paths **separately**, timing only the in-process rusqlite call
//! (no process spawn, no Tauri command layer):
//!
//!   - **FTS** — `SymbolStore::search_by_name` (`symbols_fts MATCH` + `bm25`
//!     ranking). This is the primary BM25 path.
//!   - **LIKE** — `SymbolStore::search_by_name_like` (the exact/prefix/substring
//!     `LIKE` fallback the public search uses when FTS under-fills).
//!
//! Both are public methods on `SymbolStore` with identical signatures, so the
//! two paths are exercised deterministically and independently (no flag, no
//! query trick required — see the report). The public `execute_search` entry
//! auto-merges the two (`search_symbol_candidates` in `search.rs`); this harness
//! deliberately calls the lower-level methods so each path is timed on its own.
//!
//! Run with:
//!   `cargo test --release --test query_latency -- --ignored --nocapture`
//!
//! Corpus selection (mirrors `bench_cold_index`):
//!   - `BENCH_CORPUS=/abs/dir` → that directory, labeled `"pinned"`.
//!   - otherwise → this repo's own `src-tauri/src`, labeled
//!     `"interim:src-tauri/src"` (runs with no network/clone as a real interim
//!     baseline until the pinned corpus lands).

mod support;

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Searches per query that contribute timing samples (after warm-up).
const N_PER_QUERY: usize = 100;
/// Untimed warm-up iterations per query (prime caches / page-ins).
const WARMUP: usize = 5;
/// Result limit passed to each search call (typical interactive limit).
const SEARCH_LIMIT: usize = 50;
/// Number of query terms to sample (the plan's "~5 representative terms").
const MAX_QUERIES: usize = 5;

/// Candidate identifier fragments; the test keeps the first `MAX_QUERIES` that
/// actually return FTS hits over the corpus, so every timed query exercises a
/// real result path.
const CANDIDATE_TERMS: &[&str] = &[
    "symbol", "index", "search", "store", "service", "query", "language", "parse", "file", "result",
];

/// Resolve the corpus directory and its label.
fn resolve_corpus() -> (PathBuf, String) {
    if let Ok(raw) = std::env::var("BENCH_CORPUS") {
        let path = PathBuf::from(&raw);
        assert!(
            path.is_absolute(),
            "BENCH_CORPUS must be an absolute path, got {raw:?}"
        );
        assert!(
            path.is_dir(),
            "BENCH_CORPUS must be a directory, got {raw:?}"
        );
        return (path, "pinned".to_string());
    }
    let default = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    (default, "interim:src-tauri/src".to_string())
}

/// Nearest-rank percentile over microsecond samples. Sorts `samples` in place;
/// `q` is a fraction in `[0.0, 1.0]` (e.g. `0.50` for p50). Returns `0` for an
/// empty input.
fn percentile(samples: &mut Vec<u128>, q: f64) -> u128 {
    if samples.is_empty() {
        return 0;
    }
    samples.sort_unstable();
    let idx = ((samples.len() - 1) as f64 * q).round() as usize;
    samples[idx.min(samples.len() - 1)]
}

/// Run `f`, returning the elapsed wall time in microseconds.
fn time_us(mut f: impl FnMut()) -> u128 {
    let start = Instant::now();
    f();
    start.elapsed().as_micros()
}

#[test]
#[ignore = "latency baseline; run explicitly with --ignored --nocapture"]
fn query_latency_baseline() {
    let (corpus, corpus_kind) = resolve_corpus();
    let indexed = support::index_corpus(&corpus);
    let store = indexed.store.as_ref();

    // Keep only terms that exercise the FTS path with a non-empty result set, so
    // the timings reflect real query work rather than empty-result short paths.
    let queries: Vec<&'static str> = CANDIDATE_TERMS
        .iter()
        .copied()
        .filter(|term| {
            store
                .search_by_name(term, SEARCH_LIMIT)
                .map(|hits| !hits.is_empty())
                .unwrap_or(false)
        })
        .take(MAX_QUERIES)
        .collect();
    assert!(
        !queries.is_empty(),
        "no candidate term returned FTS results over corpus {corpus_kind:?}"
    );

    let mut fts_samples: Vec<u128> = Vec::with_capacity(queries.len() * N_PER_QUERY);
    let mut like_samples: Vec<u128> = Vec::with_capacity(queries.len() * N_PER_QUERY);

    for term in &queries {
        // Warm up both paths (untimed).
        for _ in 0..WARMUP {
            black_box(store.search_by_name(term, SEARCH_LIMIT).unwrap());
            black_box(store.search_by_name_like(term, SEARCH_LIMIT).unwrap());
        }

        // FTS / BM25 path.
        for _ in 0..N_PER_QUERY {
            let us = time_us(|| {
                black_box(store.search_by_name(term, SEARCH_LIMIT).unwrap());
            });
            fts_samples.push(us);
        }

        // LIKE fallback path.
        for _ in 0..N_PER_QUERY {
            let us = time_us(|| {
                black_box(store.search_by_name_like(term, SEARCH_LIMIT).unwrap());
            });
            like_samples.push(us);
        }
    }

    let expected = queries.len() * N_PER_QUERY;
    assert_eq!(fts_samples.len(), expected, "FTS sample count mismatch");
    assert_eq!(like_samples.len(), expected, "LIKE sample count mismatch");

    let fts_p50 = percentile(&mut fts_samples, 0.50);
    let fts_p95 = percentile(&mut fts_samples, 0.95);
    let fts_p99 = percentile(&mut fts_samples, 0.99);
    let like_p50 = percentile(&mut like_samples, 0.50);
    let like_p95 = percentile(&mut like_samples, 0.95);
    let like_p99 = percentile(&mut like_samples, 0.99);

    let report = serde_json::json!({
        "corpus_kind": corpus_kind,
        "n_per_query": N_PER_QUERY,
        "queries": queries,
        "fts": {
            "p50_us": fts_p50 as u64,
            "p95_us": fts_p95 as u64,
            "p99_us": fts_p99 as u64,
            "samples": fts_samples.len(),
        },
        "like": {
            "p50_us": like_p50 as u64,
            "p95_us": like_p95 as u64,
            "p99_us": like_p99 as u64,
            "samples": like_samples.len(),
        },
        "note": "FTS = SymbolStore::search_by_name (symbols_fts MATCH + bm25); \
                 LIKE = SymbolStore::search_by_name_like. Separate public methods, \
                 timed independently around the in-process rusqlite call.",
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());

    // Sanity assertions: every expected sample was collected and the in-process
    // call took measurable time on both paths (N>0, non-degenerate timing).
    assert_eq!(
        fts_samples.len() + like_samples.len(),
        2 * expected,
        "unexpected total sample count"
    );
    assert!(fts_p50 > 0, "FTS p50 should be > 0us (got {fts_p50})");
    assert!(like_p50 > 0, "LIKE p50 should be > 0us (got {like_p50})");
}
