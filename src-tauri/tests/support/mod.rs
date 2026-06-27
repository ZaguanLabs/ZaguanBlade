//! Shared integration-test support helpers.
//!
//! `index_corpus` builds a **cold** symbol index over a real directory on a
//! brand-new on-disk SQLite database. M0.1 (`bench_cold_index`) uses it for the
//! throughput harness; M0.4 (query-latency) is intended to reuse it so both
//! measure against an identically-built index.

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;
use zblade_lib::language_service::LanguageService;
use zblade_lib::symbol_index::SymbolStore;

/// A cold-indexed corpus.
///
/// The `SymbolStore` was created fresh on an on-disk temp DB and then driven
/// through a full cold index of the corpus root. The `TempDir` is kept alive so
/// the DB file is not deleted while `service`/`store` are still in use.
pub struct IndexedCorpus {
    pub service: LanguageService,
    pub store: Arc<SymbolStore>,
    // Keeps the on-disk temp DB alive for the lifetime of the corpus.
    _db_tmp: TempDir,
}

/// Build a **cold** index over `root`.
///
/// Each call creates a brand-new on-disk SQLite DB in a fresh `TempDir` (so the
/// index is genuinely cold — no warm cache, no pre-existing rows) via
/// `SymbolStore::new`, wires it to a `LanguageService` rooted at `root`, and
/// runs a full index of the root.
///
/// ## Index entry point (reconcile vs index_directory)
///
/// The plan prefers `reconcile_index_with_progress`, but that path does **not**
/// populate the per-stage batch timers the bench needs: in
/// `commit_staged_file_indexes_with_metrics` the metrics are computed and then
/// discarded (reconcile calls `commit_staged_file_indexes`, the
/// metrics-dropping wrapper), and `last_discovery_ms` is never set on that path.
/// The `IndexTimingSnapshot.last_batch_*_ms` / `last_discovery_ms` fields are
/// only written by `index_directory` (service.rs ~2212 and ~2329). Because the
/// M0.1 DoD reconciliation needs those per-stage timers, we drive the cold index
/// through `index_directory("")` — the documented fallback, which indexes the
/// root. For a fresh DB this produces an identical cold index of `root`.
pub fn index_corpus(root: &Path) -> IndexedCorpus {
    let db_tmp = TempDir::new().expect("create temp dir for cold symbol db");
    let db_path = db_tmp.path().join("symbols.db");
    let store = Arc::new(SymbolStore::new(&db_path).expect("create cold symbol store"));
    let service =
        LanguageService::new(root.to_path_buf(), Arc::clone(&store)).expect("create language service");
    service
        .index_directory("")
        .expect("cold index of corpus root");
    IndexedCorpus {
        service,
        store,
        _db_tmp: db_tmp,
    }
}
