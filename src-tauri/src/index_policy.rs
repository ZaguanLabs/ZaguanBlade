//! Shared policy and cooperative lifetimes for every built-in index instance,
//! including temporary context services. No policy read opens the symbol store.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
};

pub const DISABLED: &str =
    "symbols_index_disabled: use rg, get_workspace_structure, read_file_range and text patches";
pub const STOPPING: &str = "symbols_index_stopping";

#[derive(Default)]
pub struct IndexLifetime {
    cancelled: AtomicBool,
    active: AtomicUsize,
    background_refresh_started: AtomicBool,
}

pub struct IndexWork<'a>(&'a IndexLifetime);
impl Drop for IndexWork<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}
impl IndexLifetime {
    /// A service generation needs at most one startup/settings reconciliation.
    pub fn claim_background_refresh(&self) -> bool {
        !self.is_cancelled() && !self.background_refresh_started.swap(true, Ordering::AcqRel)
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
    pub fn check(&self) -> Result<(), crate::language_service::LanguageError> {
        if self.is_cancelled() {
            Err(crate::language_service::LanguageError::NotSupported(
                DISABLED.into(),
            ))
        } else {
            Ok(())
        }
    }
    pub fn enter(&self) -> Result<IndexWork<'_>, crate::language_service::LanguageError> {
        self.check()?;
        self.active.fetch_add(1, Ordering::AcqRel);
        let work = IndexWork(self);
        self.check()?;
        Ok(work)
    }
}

type Registry = HashMap<PathBuf, Vec<Weak<IndexLifetime>>>;
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}
fn key(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

pub fn global_enabled() -> Result<bool, String> {
    // Unit tests must not depend on or mutate a user's actual global settings.
    #[cfg(test)]
    {
        Ok(true)
    }
    #[cfg(not(test))]
    {
        crate::integrations::store::IntegrationStore::new(crate::config::default_global_config_dir())
        .load().map(|snapshot| snapshot.config.symbols_index_enabled)
        .map_err(|_| "symbols_index_policy_unavailable".into())
    }
}
pub fn workspace_override(root: &Path) -> Result<Option<bool>, String> {
    let path = crate::project_settings::get_settings_path(root);
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("symbols_index_policy_unavailable".into()),
    };
    let settings: crate::project_settings::ProjectSettings =
        serde_json::from_str(&content).map_err(|_| "symbols_index_policy_unavailable")?;
    Ok(settings.integrations.symbols_index_enabled)
}
pub fn enabled(root: &Path) -> Result<bool, String> {
    Ok(workspace_override(root)?.unwrap_or(global_enabled()?))
}

pub fn register(root: &Path) -> Result<Arc<IndexLifetime>, String> {
    let mut registry = registry().lock().map_err(|_| STOPPING)?;
    let entries = registry.entry(key(root)).or_default();
    entries.retain(|entry| entry.strong_count() > 0);
    if !enabled(root)? {
        for life in entries.iter().filter_map(Weak::upgrade) {
            life.cancel();
        }
        return Err(DISABLED.into());
    }
    if entries
        .iter()
        .filter_map(Weak::upgrade)
        .any(|life| life.is_cancelled() && life.active() > 0)
    {
        return Err(STOPPING.into());
    }
    let life = Arc::new(IndexLifetime::default());
    entries.push(Arc::downgrade(&life));
    Ok(life)
}

/// Reconcile persisted policy before acknowledging a Settings save. Registered
/// temporary services share cancellation with AppState-owned services.
pub fn refresh() {
    if let Ok(mut registry) = registry().lock() {
        registry.retain(|root, entries| {
            entries.retain(|entry| entry.strong_count() > 0);
            if !enabled(root).unwrap_or(false) {
                for life in entries.iter().filter_map(Weak::upgrade) {
                    life.cancel();
                }
            }
            !entries.is_empty()
        });
    }
}

pub fn cancel_workspace(root: &Path) {
    if let Ok(registry) = registry().lock() {
        if let Some(entries) = registry.get(&key(root)) {
            for life in entries.iter().filter_map(Weak::upgrade) {
                life.cancel();
            }
        }
    }
}

pub fn stopping(root: &Path) -> bool {
    registry()
        .lock()
        .map(|registry| {
            registry.get(&key(root)).is_some_and(|entries| {
                entries
                    .iter()
                    .filter_map(Weak::upgrade)
                    .any(|life| life.is_cancelled() && life.active() > 0)
            })
        })
        .unwrap_or(true)
}

/// Mixed structural tools are omitted until they have an explicit file-only
/// contract. Parser-independent reading, searching and text editing stay usable.
pub fn requires_index(name: &str) -> bool {
    name.starts_with("symbol_")
        || matches!(
            name,
            "semantic_anchor_search"
                | "edit_impact"
                | "fast_context"
                | "codebase_investigator"
                | "get_project_index_overview"
                | "get_project_index_chunk"
        )
}
pub fn filter_tools(tools: Vec<serde_json::Value>, enabled: bool) -> Vec<serde_json::Value> {
    if enabled {
        return tools;
    }
    tools
        .into_iter()
        .filter(|tool| {
            !requires_index(
                tool["function"]["name"]
                    .as_str()
                    .or_else(|| tool["name"].as_str())
                    .unwrap_or(""),
            )
        })
        .collect()
}

/// Bounded file-based context, independent of symbol parsing and persistence.
/// Paths are hints, not structural evidence; no source content is read here.
pub fn file_context_paths(
    root: &Path,
    query: &str,
    active: Option<&str>,
    open: &[String],
    limit: usize,
) -> Vec<String> {
    let mut ranked = HashMap::<String, usize>::new();
    let canonical_root = key(root);
    for path in active.into_iter().chain(open.iter().map(String::as_str)) {
        let absolute = root.join(path);
        if let Ok(canonical) = absolute.canonicalize() {
            if canonical.is_file() {
                if let Ok(relative) = canonical.strip_prefix(&canonical_root) {
                    ranked.insert(relative.to_string_lossy().replace('\\', "/"), 100);
                }
            }
        }
    }
    let tokens: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|part| part.len() >= 3)
        .take(12)
        .map(str::to_lowercase)
        .collect();
    let walker = ignore::WalkBuilder::new(root)
        .require_git(false)
        .hidden(true)
        .filter_entry(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some("node_modules" | "target" | "dist" | "build" | "vendor" | ".git" | ".zblade")
            )
        })
        .build();
    for entry in walker.take(10_000).filter_map(Result::ok) {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let path = relative.to_string_lossy().replace('\\', "/");
        let lower = path.to_lowercase();
        let score = tokens
            .iter()
            .filter(|token| lower.contains(token.as_str()))
            .count();
        if score > 0 {
            ranked.entry(path).or_insert(score);
        }
    }
    let mut ranked: Vec<_> = ranked.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked
        .into_iter()
        .take(limit.min(20))
        .map(|(path, _)| path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set(root: &Path, enabled: bool) {
        let mut settings = crate::project_settings::ProjectSettings::default();
        settings.integrations.symbols_index_enabled = Some(enabled);
        crate::project_settings::save_project_settings(root, &settings).unwrap();
    }
    #[test]
    fn disabled_context_and_stale_tools_never_create_a_database() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("service.rs"), "fn kept() {}\n").unwrap();
        set(root.path(), false);
        assert!(!enabled(root.path()).unwrap());
        assert!(crate::context_pack::language_service_for_workspace(root.path()).is_err());
        let result =
            crate::tools::execute_tool(root.path(), "symbol_search", r#"{"query":"kept"}"#);
        assert!(result.error.is_some());
        assert_eq!(
            file_context_paths(root.path(), "service", None, &[], 5),
            vec!["service.rs"]
        );
        let mut request = crate::context_pack::ContextPackRequest {
            id: "file-context".into(),
            query: "service".into(),
            queries: Vec::new(),
            intent: None,
            max_results: Some(5),
            include_tests: None,
            include_docs: None,
            include_memory: None,
            include_project_index_min: None,
        };
        let payload = crate::context_pack::build_context_pack(root.path(), None, &[], &request);
        assert!(payload.error.is_none());
        assert_eq!(payload.primary_files[0].path, "service.rs");
        assert!(matches!(
            payload.index_health.unwrap().status,
            crate::language_service::IndexHealthStatus::Disabled
        ));
        request.query.clear();
        assert!(
            crate::context_pack::build_context_pack(root.path(), None, &[], &request)
                .error
                .is_some()
        );
        assert!(!root.path().join(".zblade/index/symbols.db").exists());
    }
    #[test]
    fn stopping_counts_owned_work_and_reenable_waits_for_it() {
        let root = tempfile::tempdir().unwrap();
        let life = register(root.path()).unwrap();
        let work = life.enter().unwrap();
        set(root.path(), false);
        refresh();
        assert!(life.check().is_err());
        assert!(stopping(root.path()));
        set(root.path(), true);
        assert!(register(root.path()).is_err());
        drop(work);
        assert!(!stopping(root.path()));
        assert!(register(root.path()).is_ok());
    }
    #[test]
    fn invalid_policy_fails_closed_and_missing_policy_defaults_enabled() {
        let root = tempfile::tempdir().unwrap();
        assert!(enabled(root.path()).unwrap());
        set(root.path(), false);
        std::fs::write(
            crate::project_settings::get_settings_path(root.path()),
            "broken",
        )
        .unwrap();
        assert!(enabled(root.path()).is_err());
        assert!(register(root.path()).is_err());
    }
    #[test]
    fn disabled_catalog_retains_plain_file_tools() {
        let tools = filter_tools(crate::ai_workflow::get_tool_definitions(), false);
        let names: Vec<_> = tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"read_file_range"));
        assert!(names.contains(&"apply_patch"));
        assert!(names.contains(&"rg"));
        assert!(!names.iter().any(|name| requires_index(name)));
    }
    #[test]
    fn disable_during_reconciliation_preserves_live_documents_and_cached_database() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("service.rs"), "fn before() {}\n").unwrap();
        let documents = Arc::new(crate::document_service::DocumentService::new(
            root.path().into(),
        ));
        let db = root.path().join("symbols.db");
        let store = Arc::new(crate::symbol_index::store::SymbolStore::new(&db).unwrap());
        let service =
            crate::language_service::LanguageService::with_documents(documents.clone(), store)
                .unwrap();
        service.index_file("service.rs").unwrap();
        let result = service.reconcile_index_with_progress(|_| {
            set(root.path(), false);
            refresh();
        });
        assert!(result.is_err());
        assert_eq!(service.lifetime.active(), 0);
        assert!(service.index_file("service.rs").is_err());
        assert!(service.get_file_symbols("service.rs").is_err());
        documents
            .sync("service.rs", Some(2), "fn after() {}\n")
            .unwrap();
        assert_eq!(
            documents.read("service.rs").unwrap().content(),
            "fn after() {}\n"
        );
        assert!(db.exists());
        assert!(!documents.cancellation().is_cancelled());
        set(root.path(), true);
        let next = crate::context_pack::language_service_for_workspace(root.path()).unwrap();
        next.index_file("service.rs").unwrap();
    }

    #[test]
    fn concurrent_background_requests_start_once_per_generation() {
        let life = Arc::new(IndexLifetime::default());
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let life = life.clone();
                std::thread::spawn(move || life.claim_background_refresh())
            })
            .collect();
        assert_eq!(
            threads
                .into_iter()
                .filter_map(|thread| thread.join().ok())
                .filter(|claimed| *claimed)
                .count(),
            1
        );
        let cancelled = IndexLifetime::default();
        cancelled.cancel();
        assert!(!cancelled.claim_background_refresh());
    }

    #[test]
    fn workspace_retirement_cancels_temporary_context_services_too() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let a = register(first.path()).unwrap();
        let b = register(second.path()).unwrap();
        cancel_workspace(first.path());
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled());
    }
}
