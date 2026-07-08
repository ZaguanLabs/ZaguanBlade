//! M0.1 — Throughput bench harness.
//!
//! `#[ignore]`-gated integration tests that drive a **real cold index** over a
//! directory and print a JSON breakdown of where indexing time goes, using the
//! per-stage timers ZBlade already collects (`IndexTimingSnapshot`). They also
//! capture the M0.2 coverage matrix in the same run.
//!
//! Run with:
//!   `cargo test --release --test bench_cold_index -- --ignored --nocapture`
//!
//! Corpus selection:
//!   - `BENCH_CORPUS=/abs/dir` → that directory, labeled `"pinned"`.
//!   - otherwise → this repo's own `src-tauri/src`, labeled
//!     `"interim:src-tauri/src"` (so the harness runs with no network/clone as a
//!     real interim baseline until the pinned corpus lands).

mod support;

use std::path::{Path, PathBuf};
use std::time::Instant;

use tempfile::TempDir;

/// Directories excluded from corpus measurement and from the incremental copy.
const EXCLUDE_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "vendor",
    "target",
    "build",
    "dist",
    "__pycache__",
    ".cache",
];

/// Extensions we will pick from when choosing a file to mutate for the
/// incremental-reindex test.
const SOURCE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "c", "cc", "cpp", "rb",
];

/// DoD reconciliation bound (M0.1): `|wall_ms - Σ(stage_ms)| / wall_ms`.
///
/// The plan's primary bound is 0.10 and it holds on the interim corpus
/// (observed ~0.067–0.078 over repeated cold runs), so we assert it directly.
///
/// FINDING (timer coverage): `parse_extract_ms` is a *sum* across up to 8
/// parallel index workers (`index_directory`, service.rs ~2293) while `wall_ms`
/// is real elapsed time, so the stage sum structurally *over-counts* parse. On
/// this `relationship_enrichment_ms`-dominated Rust corpus that inflation is
/// only ~6–8% of wall, but it scales with the parse fraction — a parse-heavy
/// pinned corpus may exceed 0.10 and need the plan's documented 0.25 fallback.
/// We assert the 0.25 fallback (covers all corpora incl. parse-heavy ones) and
/// keep `within_10pct` as a printed diagnostic so the tighter bound stays visible.
const RECONCILE_BOUND: f64 = 0.25;

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

/// Count files / lines-of-code / bytes over `root`, honoring `EXCLUDE_DIRS`.
fn count_corpus(root: &Path) -> (u64, u64, u64) {
    fn walk(dir: &Path, files: &mut u64, loc: &mut u64, bytes: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                let name = entry.file_name();
                if EXCLUDE_DIRS.contains(&name.to_string_lossy().as_ref()) {
                    continue;
                }
                walk(&entry.path(), files, loc, bytes);
            } else if file_type.is_file() {
                *files += 1;
                if let Ok(meta) = entry.metadata() {
                    *bytes += meta.len();
                }
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    *loc += content.lines().count() as u64;
                }
            }
        }
    }
    let (mut files, mut loc, mut bytes) = (0u64, 0u64, 0u64);
    walk(root, &mut files, &mut loc, &mut bytes);
    (files, loc, bytes)
}

/// Recursively copy `src` into `dst`, honoring `EXCLUDE_DIRS`, so the
/// incremental test can mutate files freely without touching the real repo.
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create copy dir");
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let to = dst.join(&name);
        if file_type.is_dir() {
            if EXCLUDE_DIRS.contains(&name.to_string_lossy().as_ref()) {
                continue;
            }
            copy_tree(&entry.path(), &to);
        } else if file_type.is_file() {
            let _ = std::fs::copy(entry.path(), &to);
        }
    }
}

/// First file under `root` whose extension is in `SOURCE_EXTS`.
fn find_source_file(root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let file_type = entry.file_type().ok()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if !EXCLUDE_DIRS.contains(&name.to_string_lossy().as_ref()) {
                dirs.push(entry.path());
            }
        } else if file_type.is_file() {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                if SOURCE_EXTS.contains(&ext) {
                    return Some(entry.path());
                }
            }
        }
    }
    for dir in dirs {
        if let Some(found) = find_source_file(&dir) {
            return Some(found);
        }
    }
    None
}

/// Peak resident set size (kB) via `/proc/self/status` `VmHWM`. `None` off Linux.
fn peak_rss_kb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                return rest.split_whitespace().next()?.parse::<u64>().ok();
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[test]
#[ignore = "throughput bench; run explicitly with --ignored --nocapture"]
fn bench_cold_index() {
    let (corpus, corpus_kind) = resolve_corpus();
    assert!(corpus.is_dir(), "corpus dir does not exist: {corpus:?}");

    let (files, loc, bytes) = count_corpus(&corpus);

    // Time the whole cold index (fresh DB + service construction + index).
    let start = Instant::now();
    let indexed = support::index_corpus(&corpus);
    let wall_ms = start.elapsed().as_millis() as u64;

    // Per-stage timers ZBlade already collects.
    let timings = indexed.service.index_health_snapshot().timings;
    let discovery_ms = timings.last_discovery_ms.unwrap_or(0);
    let parse_extract_ms = timings.last_batch_parse_extract_ms.unwrap_or(0);
    let relationship_enrichment_ms = timings.last_batch_relationship_enrichment_ms.unwrap_or(0);
    let db_write_ms = timings.last_batch_db_write_ms.unwrap_or(0);
    let cache_update_ms = timings.last_batch_cache_update_ms.unwrap_or(0);

    // Graph counts + coverage matrix from the store.
    let nodes = indexed.store.count().expect("count symbols");
    let rel_stats = indexed
        .store
        .relationship_integrity_stats()
        .expect("relationship integrity stats");
    let edges = rel_stats.total_relationships;
    let unresolved_edges = rel_stats.unresolved_symbol_relationships;
    let coverage = indexed
        .store
        .coverage_histogram()
        .expect("coverage histogram");

    let peak_rss_kb = peak_rss_kb();

    let wall_secs = (wall_ms.max(1) as f64) / 1000.0;
    let files_per_sec = files as f64 / wall_secs;
    let mb_per_sec = (bytes as f64 / 1_048_576.0) / wall_secs;

    let report = serde_json::json!({
        "corpus_kind": corpus_kind,
        "files": files,
        "loc": loc,
        "bytes": bytes,
        "wall_ms": wall_ms,
        "files_per_sec": files_per_sec,
        "mb_per_sec": mb_per_sec,
        "parse_extract_ms": parse_extract_ms,
        "relationship_enrichment_ms": relationship_enrichment_ms,
        "db_write_ms": db_write_ms,
        "cache_update_ms": cache_update_ms,
        "discovery_ms": discovery_ms,
        "nodes": nodes,
        "edges": edges,
        "unresolved_edges": unresolved_edges,
        "peak_rss_kb": peak_rss_kb,
        "coverage_histogram": coverage,
    });

    // Print BEFORE asserting so --nocapture always shows the JSON + reconcile
    // diagnostics even if the reconciliation assertion fails.
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    // Single-line sentinel for machine scraping across multi-repo baseline runs.
    println!("BENCH_JSON {}", serde_json::to_string(&report).unwrap());

    // DoD reconciliation: do the per-stage timers account for wall?
    let stage_sum_ms = discovery_ms
        + parse_extract_ms
        + relationship_enrichment_ms
        + db_write_ms
        + cache_update_ms;
    let discrepancy = (wall_ms as f64 - stage_sum_ms as f64).abs() / (wall_ms.max(1) as f64);
    println!(
        "[reconcile] wall_ms={wall_ms} stage_sum_ms={stage_sum_ms} discrepancy={discrepancy:.4} within_10pct={} within_25pct={} (asserted bound={RECONCILE_BOUND})",
        discrepancy <= 0.10,
        discrepancy <= 0.25
    );

    assert!(files > 0, "corpus must contain files (got 0)");
    assert!(nodes > 0, "cold index must produce symbols (got 0 nodes)");
    assert!(
        discrepancy <= RECONCILE_BOUND,
        "stage timers do not reconcile to wall within {:.0}%: wall_ms={wall_ms}, stage_sum_ms={stage_sum_ms}, discrepancy={discrepancy:.4}",
        RECONCILE_BOUND * 100.0
    );
}

#[test]
#[ignore = "incremental reindex bench; run explicitly with --ignored --nocapture"]
fn bench_incremental_reindex() {
    let (corpus, _corpus_kind) = resolve_corpus();

    // Copy the corpus into a temp tree so we can mutate files freely.
    let work = TempDir::new().expect("create incremental work dir");
    let copy_root = work.path().join("corpus");
    copy_tree(&corpus, &copy_root);

    let cold_start = Instant::now();
    let indexed = support::index_corpus(&copy_root);
    let cold_ms = cold_start.elapsed().as_millis() as u64;

    // Mutate exactly one source file (content + size + mtime all change).
    let target = find_source_file(&copy_root).expect("a source file to touch in the corpus copy");
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&target)
            .expect("open target for append");
        writeln!(file, "\n// bench_cold_index incremental touch").expect("append to target");
    }

    // Time a single-file incremental reindex (only the touched file is stale).
    let inc_start = Instant::now();
    let stats = indexed
        .service
        .index_directory("")
        .expect("incremental reindex");
    let incremental_ms = inc_start.elapsed().as_millis() as u64;

    let report = serde_json::json!({ "incremental_ms": incremental_ms });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    println!(
        "[incremental] cold_ms={cold_ms} files_reindexed={} files_fresh={} touched={}",
        stats.files_reindexed,
        stats.files_fresh,
        target.display()
    );

    assert!(
        stats.files_reindexed >= 1,
        "incremental reindex should reindex at least the touched file (got {})",
        stats.files_reindexed
    );
    assert!(
        stats.files_fresh > stats.files_reindexed,
        "incremental reindex should leave most files fresh (fresh={}, reindexed={})",
        stats.files_fresh,
        stats.files_reindexed
    );
}
