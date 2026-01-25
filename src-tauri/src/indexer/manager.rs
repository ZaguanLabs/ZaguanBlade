use crate::indexer::builder::index_workspace;
use crate::indexer::cache::{load_cache, save_cache};
use crate::indexer::handler::{handle_get_full_context, GetFullContextOptions};
use crate::indexer::types::ProjectIndex;
use crate::indexer::watcher::IndexWatcher;
use std::path::Path;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct IndexerManager {
    index: Arc<RwLock<ProjectIndex>>,
    _watcher: Arc<Option<IndexWatcher>>,
}

impl IndexerManager {
    pub fn new(workspace_root: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let index = match load_cache(workspace_root) {
            Ok(cached_index) => {
                eprintln!("[Indexer] Loaded cached index with {} files", cached_index.file_count());
                cached_index
            }
            Err(_) => {
                eprintln!("[Indexer] Building fresh index for {:?}", workspace_root);
                let fresh_index = index_workspace(workspace_root)?;
                eprintln!("[Indexer] Indexed {} files", fresh_index.file_count());
                fresh_index
            }
        };

        let index = Arc::new(RwLock::new(index));

        let watcher = match IndexWatcher::new(index.clone()) {
            Ok(w) => {
                eprintln!("[Indexer] File watcher started");
                Some(w)
            }
            Err(e) => {
                eprintln!("[Indexer] Failed to start file watcher: {}", e);
                None
            }
        };

        Ok(Self {
            index,
            _watcher: Arc::new(watcher),
        })
    }

    pub async fn get_full_context(
        &self,
        max_files: Option<usize>,
        preview_lines: Option<usize>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let options = GetFullContextOptions {
            max_files: max_files.unwrap_or(100),
            preview_lines: preview_lines.unwrap_or(50),
        };

        handle_get_full_context(&self.index, options).await
    }

    pub fn save_cache(&self) -> Result<(), Box<dyn std::error::Error>> {
        let idx = self.index.read().unwrap();
        save_cache(&*idx)
    }

    pub fn file_count(&self) -> usize {
        let idx = self.index.read().unwrap();
        idx.file_count()
    }
}
