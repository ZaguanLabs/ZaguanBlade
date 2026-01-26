use crate::indexer::builder::index_workspace;
use crate::indexer::cache::{load_cache, save_cache};
use crate::indexer::handler::{handle_get_full_context, GetFullContextOptions};
use crate::indexer::types::{ProjectIndex, detect_language};
use crate::indexer::preview::get_or_load_preview;
use crate::indexer::watcher::IndexWatcher;
use chrono::Utc;
use std::fs;
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

        let manager = Self {
            index,
            _watcher: Arc::new(watcher),
        };

        // Write project_index.md immediately after indexing
        if let Err(e) = manager.write_project_index_sync() {
            eprintln!("[Indexer] Failed to write project_index.md: {}", e);
        } else {
            eprintln!("[Indexer] Wrote .zblade/context/project_index.md");
        }

        Ok(manager)
    }

    /// Write project_index.md synchronously (for use after initial indexing)
    fn write_project_index_sync(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let max_files = 100;
        let preview_lines = 50;

        let (root, file_count, tree_render, file_paths) = {
            let idx = self.index.read().unwrap();
            
            let root = idx.root.clone();
            let file_count = idx.file_count();
            let tree_render = idx.tree.render(3);
            let file_paths: Vec<std::path::PathBuf> = idx.files.keys()
                .take(max_files)
                .cloned()
                .collect();
            
            (root, file_count, tree_render, file_paths)
        };
        
        let mut output = String::new();
        
        output.push_str(&format!("# Project Index: {}\n\n", 
            root.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
        ));
        output.push_str(&format!("**Generated**: {}\n", Utc::now().format("%Y-%m-%d %H:%M:%S UTC")));
        output.push_str(&format!("**Total Files**: {}\n\n", file_count));
        
        output.push_str("## Directory Structure\n\n```\n");
        output.push_str(&tree_render);
        output.push_str("```\n\n");
        
        output.push_str("## File Previews\n\n");
        
        for path in file_paths {
            match get_or_load_preview(&self.index, &path, preview_lines) {
                Ok(preview) => {
                    let relative_path = path.strip_prefix(&root)
                        .unwrap_or(&path)
                        .display();
                    
                    output.push_str(&format!("### {}\n\n", relative_path));
                    output.push_str(&format!("```{}\n", detect_language(&path)));
                    output.push_str(&preview);
                    output.push_str("\n```\n\n");
                }
                Err(e) => {
                    eprintln!("Failed to load preview for {:?}: {}", path, e);
                }
            }
        }
        
        let output_dir = root.join(".zblade/context");
        fs::create_dir_all(&output_dir)?;
        
        let output_path = output_dir.join("project_index.md");
        fs::write(&output_path, &output)?;
        
        {
            let mut idx = self.index.write().unwrap();
            idx.mark_clean();
        }
        
        Ok(output_path.to_string_lossy().to_string())
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
