//! Diagnostic: find the single file whose extraction spins at 100% CPU.
//!
//! Walks a corpus (BENCH_CORPUS=/abs/dir), indexes each file on a detached
//! watchdog thread, and reports the first file that exceeds the timeout — the
//! pathological file that stalls the real cold index.
//!
//!   BENCH_CORPUS=/home/stig/dev/ideas/firefox \
//!     cargo test --release --test find_hang find_hang -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// File currently being indexed (set by the main thread before each spawn).
/// A panic hook reads this so a `panic=abort` crash still names the culprit.
static CURRENT: Mutex<String> = Mutex::new(String::new());

use zblade_lib::language_service::LanguageService;
use zblade_lib::symbol_index::SymbolStore;

fn collect_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == "node_modules" || name == ".zblade" {
            continue;
        }
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            dirs.push(path);
        } else if ft.is_file() {
            out.push(path);
        }
    }
    dirs.sort();
    for d in dirs {
        collect_files(&d, out);
    }
}

#[test]
#[ignore]
fn find_hang() {
    let corpus = std::env::var("BENCH_CORPUS").expect("set BENCH_CORPUS=/abs/dir");
    let root = PathBuf::from(&corpus);
    assert!(root.is_dir(), "BENCH_CORPUS is not a directory: {corpus}");

    std::panic::set_hook(Box::new(|info| {
        let current = CURRENT.lock().map(|g| g.clone()).unwrap_or_default();
        eprintln!("\n[find_hang] *** PANIC while indexing: {current}");
        eprintln!("[find_hang] *** {info}");
    }));

    let store = Arc::new(SymbolStore::in_memory().expect("in-memory store"));
    let service =
        Arc::new(LanguageService::new(root.clone(), Arc::clone(&store)).expect("language service"));

    let mut files = Vec::new();
    collect_files(&root, &mut files);
    eprintln!("[find_hang] {} files to scan under {corpus}", files.len());

    let timeout = Duration::from_secs(
        std::env::var("HANG_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8),
    );

    let mut slowest: Vec<(u128, String)> = Vec::new();
    for (i, abs) in files.iter().enumerate() {
        let rel = abs
            .strip_prefix(&root)
            .unwrap_or(abs)
            .to_string_lossy()
            .to_string();

        *CURRENT.lock().unwrap() = rel.clone();
        let (tx, rx) = mpsc::channel();
        let svc = Arc::clone(&service);
        let rel_thread = rel.clone();
        std::thread::spawn(move || {
            let start = Instant::now();
            let _ = svc.index_file(&rel_thread);
            let _ = tx.send(start.elapsed());
        });

        match rx.recv_timeout(timeout) {
            Ok(elapsed) => {
                let ms = elapsed.as_millis();
                if ms > 250 {
                    slowest.push((ms, rel.clone()));
                    eprintln!("[find_hang] SLOW {ms} ms  {rel}");
                }
            }
            Err(_) => {
                eprintln!("\n[find_hang] *** HANG: file exceeded {timeout:?} ***");
                eprintln!("[find_hang] *** {rel}");
                eprintln!("[find_hang] *** (was file #{i} of {})", files.len());
                eprintln!("\n[find_hang] slowest completed files so far:");
                slowest.sort_by(|a, b| b.0.cmp(&a.0));
                for (ms, p) in slowest.iter().take(20) {
                    eprintln!("    {ms:>7} ms  {p}");
                }
                // Leak the hung thread; the process is about to exit.
                return;
            }
        }

        if i % 5000 == 0 && i > 0 {
            eprintln!("[find_hang] ...{i}/{} scanned", files.len());
        }
    }

    eprintln!("\n[find_hang] NO HANG. Slowest files:");
    slowest.sort_by(|a, b| b.0.cmp(&a.0));
    for (ms, p) in slowest.iter().take(30) {
        eprintln!("    {ms:>7} ms  {p}");
    }
}
