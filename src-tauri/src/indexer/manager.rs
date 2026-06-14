use crate::indexer::builder::index_workspace;
use crate::indexer::cache::{load_cache, save_cache};
use crate::indexer::handler::{handle_get_full_context, GetFullContextOptions};
use crate::indexer::minimal_index::generate_project_index_min;
use crate::indexer::preview::get_or_load_preview;
use crate::indexer::types::{detect_language, ProjectIndex};
use crate::indexer::watcher::IndexWatcher;
use crate::project_settings::load_project_settings_or_default;
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct IndexerManager {
    workspace_root: PathBuf,
    index: Arc<RwLock<ProjectIndex>>,
    _watcher: Arc<Option<IndexWatcher>>,
}

impl IndexerManager {
    pub fn new(workspace_root: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let index = match load_cache(workspace_root) {
            Ok(cached_index) => {
                eprintln!(
                    "[Indexer] Loaded cached index with {} files",
                    cached_index.file_count()
                );
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

        let after_update_index = index.clone();
        let after_update = Arc::new(move || {
            let manager = Self {
                workspace_root: after_update_index
                    .read()
                    .map(|idx| idx.root.clone())
                    .unwrap_or_default(),
                index: after_update_index.clone(),
                _watcher: Arc::new(None),
            };

            if let Err(e) = manager.refresh_project_context_files_sync() {
                eprintln!("[Indexer] Failed to refresh project context files: {}", e);
            }
            if let Err(e) = manager.save_cache() {
                eprintln!("[Indexer] Failed to save refreshed index cache: {}", e);
            }
        });

        let watcher = match IndexWatcher::new(index.clone(), Some(after_update)) {
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
            workspace_root: workspace_root.to_path_buf(),
            index,
            _watcher: Arc::new(watcher),
        };

        if let Err(e) = manager.refresh_project_context_files_sync() {
            eprintln!("[Indexer] Failed to write project context files: {}", e);
        }
        if let Err(e) = manager.save_cache() {
            eprintln!("[Indexer] Failed to save index cache: {}", e);
        }

        Ok(manager)
    }

    fn refresh_project_context_files_sync(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let settings = load_project_settings_or_default(&self.workspace_root);
        if !settings.context.project_index_legacy_enabled {
            return Ok(());
        }

        eprintln!("[Indexer] Generating legacy project index Markdown files");
        self.write_project_index_sync()?;
        self.write_project_index_min_sync()?;
        Ok(())
    }

    /// Write project_index.md synchronously (for use after initial indexing)
    fn write_project_index_sync(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let max_files = 100;
        let preview_lines = 50;

        let (root, file_count, tree_render, file_paths) = {
            let idx = self.index.read().unwrap();

            let root = idx.root.clone();
            let file_count = idx.file_count();
            let tree_render = idx.tree.render(10);
            let file_paths: Vec<std::path::PathBuf> =
                idx.files.keys().take(max_files).cloned().collect();

            (root, file_count, tree_render, file_paths)
        };

        let mut output = String::new();

        output.push_str(&format!(
            "# Project Index: {}\n\n",
            root.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
        ));
        output.push_str(&format!(
            "**Generated**: {}\n",
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));
        output.push_str(&format!("**Total Files**: {}\n\n", file_count));

        output.push_str("## Directory Structure\n\n```\n");
        output.push_str(&tree_render);
        output.push_str("```\n\n");

        output.push_str("## File Previews\n\n");

        for path in file_paths {
            match get_or_load_preview(&self.index, &path, preview_lines) {
                Ok(preview) => {
                    let relative_path = path.strip_prefix(&root).unwrap_or(&path).display();

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

    fn write_project_index_min_sync(
        &self,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let (root, output) = {
            let idx = self.index.read().unwrap();
            (idx.root.clone(), generate_project_index_min(&*idx))
        };

        let output_dir = root.join(".zblade/context");
        fs::create_dir_all(&output_dir)?;

        let output_path = output_dir.join("project_index_min.md");
        fs::write(&output_path, output)?;

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

    pub fn save_cache(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let idx = self.index.read().unwrap();
        save_cache(&*idx)
    }

    pub fn file_count(&self) -> usize {
        let idx = self.index.read().unwrap();
        idx.file_count()
    }

    pub fn matches_workspace(&self, workspace_root: &Path) -> bool {
        self.workspace_root == workspace_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_settings::{save_project_settings, ProjectSettings};

    fn test_manager(workspace_root: &Path) -> IndexerManager {
        IndexerManager {
            workspace_root: workspace_root.to_path_buf(),
            index: Arc::new(RwLock::new(ProjectIndex::new(workspace_root.to_path_buf()))),
            _watcher: Arc::new(None),
        }
    }

    #[test]
    fn refresh_project_context_files_skips_legacy_markdown_by_default() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = test_manager(temp_dir.path());

        manager.refresh_project_context_files_sync().unwrap();

        assert!(!temp_dir
            .path()
            .join(".zblade/context/project_index.md")
            .exists());
        assert!(!temp_dir
            .path()
            .join(".zblade/context/project_index_min.md")
            .exists());
    }

    #[test]
    fn refresh_project_context_files_writes_legacy_markdown_when_enabled() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut settings = ProjectSettings::default();
        settings.context.project_index_legacy_enabled = true;
        save_project_settings(temp_dir.path(), &settings).unwrap();
        let manager = test_manager(temp_dir.path());

        manager.refresh_project_context_files_sync().unwrap();

        assert!(temp_dir
            .path()
            .join(".zblade/context/project_index.md")
            .exists());
        assert!(temp_dir
            .path()
            .join(".zblade/context/project_index_min.md")
            .exists());
    }
}
