//! M0.x — Miscellaneous P0 baselines: context-pack assembly and raw git status.
//!
//! `#[ignore]`-gated integration tests. Run with:
//!   `cargo test --release --test bench_misc -- --ignored --nocapture`

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tempfile::TempDir;
use zblade_lib::context_pack::{build_context_pack, ContextPackRequest};
use zblade_lib::language_service::LanguageService;
use zblade_lib::symbol_index::SymbolStore;

fn resolve_corpus() -> PathBuf {
    if let Ok(raw) = std::env::var("BENCH_CORPUS") {
        let path = PathBuf::from(&raw);
        assert!(path.is_absolute(), "BENCH_CORPUS must be absolute");
        assert!(path.is_dir(), "BENCH_CORPUS must be a directory");
        return path;
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cargo manifest parent")
        .join("src")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cargo manifest parent")
        .to_path_buf()
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap_or_else(|e| panic!("failed to create {}: {e}", dst.display()));
    let entries =
        fs::read_dir(src).unwrap_or_else(|e| panic!("failed to read {}: {e}", src.display()));
    for entry in entries {
        let entry = entry.expect("read dir entry");
        let target = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir_recursive(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target)
                .unwrap_or_else(|e| panic!("failed to copy {}: {e}", entry.path().display()));
        }
    }
}

fn find_first_file(root: &Path, ext: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == ext) {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn corpus_file_count(root: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push(path);
                } else {
                    count += 1;
                }
            }
        }
    }
    count
}

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
#[ignore = "context pack bench; run explicitly with --ignored --nocapture"]
fn bench_context_pack() {
    let corpus = resolve_corpus();
    assert!(corpus.is_dir(), "corpus not found: {}", corpus.display());

    let work = TempDir::new().expect("create temp workspace");
    let workspace = work.path().to_path_buf();
    copy_dir_recursive(&corpus, &workspace);

    let db_dir = workspace.join(".zblade").join("index");
    fs::create_dir_all(&db_dir).expect("create .zblade index dir");
    let db_path = db_dir.join("symbols.db");

    let store = Arc::new(SymbolStore::new(&db_path).expect("create symbol store"));
    let service = LanguageService::new(workspace.clone(), Arc::clone(&store))
        .expect("create language service");
    service.index_directory("").expect("cold index corpus root");
    drop(service);
    drop(store);

    let active_file = find_first_file(&workspace, "rs").and_then(|p| {
        p.strip_prefix(&workspace)
            .ok()
            .map(|r| r.to_string_lossy().into_owned())
    });

    let request = ContextPackRequest {
        id: "bench".to_string(),
        query: "symbol".to_string(),
        queries: vec![],
        intent: None,
        max_results: Some(8),
        include_tests: Some(false),
        include_docs: Some(false),
        include_memory: Some(false),
        include_project_index_min: Some(false),
    };

    let start = Instant::now();
    let payload = build_context_pack(&workspace, active_file.as_deref(), &[], &request);
    let wall_us = start.elapsed().as_micros() as u64;

    let payload_json = serde_json::to_string(&payload).expect("serialize payload");
    let files = corpus_file_count(&workspace);

    let report = serde_json::json!({
        "corpus_kind": "interim:src-tauri/src",
        "workspace": workspace.to_string_lossy(),
        "query": request.query,
        "active_file": active_file,
        "files": files,
        "primary_files": payload.primary_files.len(),
        "related_files": payload.related_files.len(),
        "enriched_files": payload.enriched_files.len(),
        "payload_bytes": payload_json.len(),
        "wall_us": wall_us,
        "wall_ms": (wall_us as f64) / 1000.0,
        "peak_rss_kb": peak_rss_kb(),
        "build_timing_ms": payload.timing_ms,
    });

    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    println!("BENCH_JSON {}", serde_json::to_string(&report).unwrap());

    assert!(
        payload.primary_files.len() > 0 || payload.error.is_some(),
        "context pack should return results or an error"
    );
}

#[test]
#[ignore = "git status bench; run explicitly with --ignored --nocapture"]
fn bench_git_status() {
    let repo = repo_root();
    assert!(
        repo.join(".git").is_dir(),
        "{} is not a git repo",
        repo.display()
    );

    const SAMPLES: usize = 5;
    let mut times_us = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        // Intentional blocking probe: this ignored benchmark measures the
        // Git subprocess itself and runs outside the application runtime.
        #[allow(clippy::disallowed_methods, clippy::disallowed_types)]
        let output = std::process::Command::new("git")
            .args([
                "-C",
                repo.to_str().expect("repo root utf-8"),
                "status",
                "--porcelain=v2",
                "-uall",
                "--branch",
            ])
            .output()
            .expect("run git status");
        let us = start.elapsed().as_micros() as u64;
        assert!(
            output.status.success(),
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        times_us.push(us);
    }

    times_us.sort();
    let p50 = times_us[SAMPLES / 2];
    let p95 = times_us[(SAMPLES * 95) / 100];
    let p99 = times_us[(SAMPLES * 99) / 100];
    let min = times_us[0];
    let max = times_us[SAMPLES - 1];

    let report = serde_json::json!({
        "repo": repo.to_string_lossy(),
        "command": "git status --porcelain=v2 -uall --branch",
        "samples": SAMPLES,
        "min_us": min,
        "p50_us": p50,
        "p95_us": p95,
        "p99_us": p99,
        "max_us": max,
        "min_ms": (min as f64) / 1000.0,
        "p50_ms": (p50 as f64) / 1000.0,
        "p95_ms": (p95 as f64) / 1000.0,
        "p99_ms": (p99 as f64) / 1000.0,
        "max_ms": (max as f64) / 1000.0,
        "peak_rss_kb": peak_rss_kb(),
    });

    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    println!("BENCH_JSON {}", serde_json::to_string(&report).unwrap());

    assert!(p50 > 0, "git status should take measurable time");
}
