//! File Indexer with automatic reindexing
//!
//! Provides file watching and automatic reindexing for
//! keeping the symbol index up to date.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::tree_sitter::Language;

/// Events emitted during indexing
#[derive(Debug, Clone)]
pub enum IndexEvent {
    /// Started indexing a file
    FileStarted { path: String },
    /// Finished indexing a file
    FileCompleted { path: String, symbols: usize },
    /// Failed to index a file
    FileFailed { path: String, error: String },
    /// Workspace indexing progress
    Progress { completed: usize, total: usize },
    /// Workspace indexing complete
    WorkspaceCompleted {
        files: usize,
        symbols: usize,
        duration_ms: u64,
    },
}

/// File indexer for managing workspace indexing
pub struct FileIndexer {
    /// Workspace root
    workspace_root: PathBuf,
    /// Files currently being indexed
    in_progress: RwLock<HashSet<String>>,
    /// Debounce duration for file changes
    debounce_duration: Duration,
    /// Last change times per file
    last_changes: RwLock<std::collections::HashMap<String, Instant>>,
}

impl FileIndexer {
    /// Create a new file indexer
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            in_progress: RwLock::new(HashSet::new()),
            debounce_duration: Duration::from_millis(300),
            last_changes: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Set debounce duration
    pub fn with_debounce(mut self, duration: Duration) -> Self {
        self.debounce_duration = duration;
        self
    }

    /// Get list of all supported files in workspace
    pub fn discover_files(&self) -> Vec<String> {
        let mut files = Vec::new();
        self.discover_files_recursive(&self.workspace_root, "", &mut files);
        files
    }

    fn discover_files_recursive(&self, base: &Path, relative: &str, files: &mut Vec<String>) {
        let current_dir = if relative.is_empty() {
            base.to_path_buf()
        } else {
            base.join(relative)
        };

        let entries = match std::fs::read_dir(&current_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            // Skip ignored directories
            if Self::should_ignore(name) {
                continue;
            }

            let rel_path = if relative.is_empty() {
                name.to_string()
            } else {
                format!("{}/{}", relative, name)
            };

            if path.is_dir() {
                self.discover_files_recursive(base, &rel_path, files);
            } else if path.is_file() && Language::from_path(&rel_path).is_some() {
                files.push(rel_path);
            }
        }
    }

    /// Check if a file/directory should be ignored
    fn should_ignore(name: &str) -> bool {
        name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name == "dist"
            || name == "build"
            || name == "__pycache__"
            || name == ".git"
            || name == ".svn"
            || name == "vendor"
    }

    /// Check if a path is a supported language file
    pub fn is_supported(&self, path: &str) -> bool {
        Language::from_path(path).is_some()
    }

    /// Check if a file change should be processed (debouncing)
    pub fn should_process(&self, path: &str) -> bool {
        let mut changes = self.last_changes.write().unwrap();
        let now = Instant::now();

        if let Some(last) = changes.get(path) {
            if now.duration_since(*last) < self.debounce_duration {
                return false;
            }
        }

        changes.insert(path.to_string(), now);
        true
    }

    /// Mark a file as being indexed
    pub fn start_indexing(&self, path: &str) -> bool {
        let mut in_progress = self.in_progress.write().unwrap();
        if in_progress.contains(path) {
            false
        } else {
            in_progress.insert(path.to_string());
            true
        }
    }

    /// Mark a file as done indexing
    pub fn finish_indexing(&self, path: &str) {
        let mut in_progress = self.in_progress.write().unwrap();
        in_progress.remove(path);
    }

    /// Check if a file is currently being indexed
    pub fn is_indexing(&self, path: &str) -> bool {
        self.in_progress.read().unwrap().contains(path)
    }

    /// Get relative path from absolute path
    pub fn to_relative(&self, abs_path: &Path) -> Option<String> {
        abs_path
            .strip_prefix(&self.workspace_root)
            .ok()
            .and_then(|p| p.to_str())
            .map(|s| s.replace('\\', "/"))
    }

    /// Get absolute path from relative
    pub fn to_absolute(&self, rel_path: &str) -> PathBuf {
        self.workspace_root.join(rel_path)
    }

    /// Estimate total files for progress tracking
    pub fn count_files(&self) -> usize {
        self.discover_files().len()
    }
}

/// Statistics from a batch indexing operation
#[derive(Debug, Clone, Default)]
pub struct BatchIndexStats {
    pub started_at: Option<Instant>,
    pub files_discovered: usize,
    pub files_indexed: usize,
    pub files_failed: usize,
    pub symbols_total: usize,
}

impl BatchIndexStats {
    pub fn new(files_discovered: usize) -> Self {
        Self {
            started_at: Some(Instant::now()),
            files_discovered,
            ..Default::default()
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started_at
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn progress_percent(&self) -> f32 {
        if self.files_discovered == 0 {
            0.0
        } else {
            (self.files_indexed + self.files_failed) as f32 / self.files_discovered as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();

        // Create some test files
        fs::write(dir.path().join("main.ts"), "function main() {}").unwrap();
        fs::write(dir.path().join("utils.ts"), "function helper() {}").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/index.ts"), "export * from './main';").unwrap();

        // Create some files that should be ignored
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        fs::write(dir.path().join("node_modules/package.ts"), "").unwrap();

        dir
    }

    #[test]
    fn test_discover_files() {
        let dir = create_test_workspace();
        let indexer = FileIndexer::new(dir.path().to_path_buf());

        let files = indexer.discover_files();

        assert_eq!(files.len(), 3);
        assert!(files.iter().any(|f| f == "main.ts"));
        assert!(files.iter().any(|f| f == "utils.ts"));
        assert!(files.iter().any(|f| f == "src/index.ts"));

        // Should not include node_modules
        assert!(!files.iter().any(|f| f.contains("node_modules")));
    }

    #[test]
    fn test_is_supported() {
        let dir = TempDir::new().unwrap();
        let indexer = FileIndexer::new(dir.path().to_path_buf());

        assert!(indexer.is_supported("main.ts"));
        assert!(indexer.is_supported("app.tsx"));
        assert!(indexer.is_supported("script.js"));
        assert!(indexer.is_supported("main.py"));
        assert!(indexer.is_supported("lib.rs"));
        assert!(!indexer.is_supported("data.json"));
        assert!(!indexer.is_supported("readme.md"));
    }

    #[test]
    fn test_debouncing() {
        let dir = TempDir::new().unwrap();
        let indexer =
            FileIndexer::new(dir.path().to_path_buf()).with_debounce(Duration::from_millis(100));

        // First call should process
        assert!(indexer.should_process("test.ts"));

        // Immediate second call should be debounced
        assert!(!indexer.should_process("test.ts"));

        // Different file should process
        assert!(indexer.should_process("other.ts"));
    }

    #[test]
    fn test_indexing_lock() {
        let dir = TempDir::new().unwrap();
        let indexer = FileIndexer::new(dir.path().to_path_buf());

        // First call should acquire lock
        assert!(indexer.start_indexing("test.ts"));
        assert!(indexer.is_indexing("test.ts"));

        // Second call should fail
        assert!(!indexer.start_indexing("test.ts"));

        // After finishing, can start again
        indexer.finish_indexing("test.ts");
        assert!(!indexer.is_indexing("test.ts"));
        assert!(indexer.start_indexing("test.ts"));
    }
}
