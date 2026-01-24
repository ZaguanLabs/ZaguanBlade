use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use walkdir::WalkDir;

/// Thread-safe wrapper around a .gitignore matcher
/// Recursively loads ALL .gitignore files in the workspace
#[derive(Clone)]
pub struct GitignoreFilter {
    inner: Arc<RwLock<Option<Gitignore>>>,
    workspace_root: PathBuf,
}

impl GitignoreFilter {
    /// Create a new GitignoreFilter for the given workspace root.
    /// Recursively loads ALL .gitignore files found in the workspace.
    pub fn new(workspace_root: &Path) -> Self {
        let mut builder = GitignoreBuilder::new(workspace_root);
        let mut gitignore_count = 0;
        
        // First, add the root .gitignore if it exists
        let root_gitignore = workspace_root.join(".gitignore");
        if root_gitignore.exists() {
            if let Some(e) = builder.add(&root_gitignore) {
                eprintln!("[GITIGNORE] Failed to load root .gitignore: {}", e);
            } else {
                gitignore_count += 1;
            }
        }
        
        // Also check for global gitignore (~/.gitignore_global or git config)
        if let Some(global_gitignore) = Self::find_global_gitignore() {
            if let Some(e) = builder.add(&global_gitignore) {
                eprintln!("[GITIGNORE] Failed to load global gitignore: {}", e);
            } else {
                eprintln!("[GITIGNORE] Loaded global gitignore: {}", global_gitignore.display());
                gitignore_count += 1;
            }
        }
        
        // Recursively find all .gitignore files in subdirectories
        // We need to be careful not to descend into directories that are already ignored
        // For simplicity, we'll do a full walk and collect all .gitignore files
        for entry in WalkDir::new(workspace_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                // Skip common large/ignored directories to speed up the walk
                let name = e.file_name().to_string_lossy();
                !matches!(name.as_ref(), 
                    "node_modules" | ".git" | "target" | "dist" | "build" | 
                    ".next" | ".nuxt" | "__pycache__" | ".venv" | "venv" |
                    ".cargo" | ".rustup" | "vendor"
                )
            })
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            
            // Skip the root .gitignore (already added)
            if path == root_gitignore {
                continue;
            }
            
            // Check if this is a .gitignore file
            if path.file_name().map(|n| n == ".gitignore").unwrap_or(false) {
                if let Some(e) = builder.add(path) {
                    eprintln!("[GITIGNORE] Failed to load {}: {}", path.display(), e);
                } else {
                    gitignore_count += 1;
                }
            }
        }
        
        let gitignore = if gitignore_count > 0 {
            eprintln!("[GITIGNORE] Loaded {} .gitignore file(s) from {}", gitignore_count, workspace_root.display());
            Some(builder.build().unwrap_or_else(|e| {
                eprintln!("[GITIGNORE] Failed to build gitignore matcher: {}", e);
                GitignoreBuilder::new(workspace_root).build().unwrap()
            }))
        } else {
            eprintln!("[GITIGNORE] No .gitignore files found in {}", workspace_root.display());
            None
        };

        Self {
            inner: Arc::new(RwLock::new(gitignore)),
            workspace_root: workspace_root.to_path_buf(),
        }
    }
    
    /// Find the global gitignore file if it exists
    fn find_global_gitignore() -> Option<PathBuf> {
        // Check common locations for global gitignore
        if let Some(home) = dirs::home_dir() {
            // Check ~/.gitignore_global (common convention)
            let global = home.join(".gitignore_global");
            if global.exists() {
                return Some(global);
            }
            
            // Check ~/.config/git/ignore (XDG standard)
            let xdg_ignore = home.join(".config/git/ignore");
            if xdg_ignore.exists() {
                return Some(xdg_ignore);
            }
        }
        
        None
    }

    /// Check if a path should be ignored according to .gitignore rules.
    /// Returns true if the path SHOULD be ignored (filtered out).
    /// 
    /// # Arguments
    /// * `path` - The path to check (can be absolute or relative)
    /// 
    /// # Returns
    /// * `true` if the path should be ignored
    /// * `false` if the path should be included
    pub fn should_ignore(&self, path: &Path) -> bool {
        let guard = self.inner.read().unwrap();
        
        // If no gitignore loaded, don't filter anything
        let Some(ref gitignore) = *guard else {
            return false;
        };

        // Convert to absolute path if needed
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        };

        // Get relative path from workspace root for matching
        let rel_path = match abs_path.strip_prefix(&self.workspace_root) {
            Ok(p) => p,
            Err(_) => {
                // Path is outside workspace, don't filter
                return false;
            }
        };

        // Check if gitignore matches this path
        let matched = gitignore.matched(rel_path, abs_path.is_dir());
        
        // The ignore crate returns:
        // - Ignore if the path should be ignored
        // - Whitelist if the path is whitelisted (negated patterns like !important.txt)
        // - None if no pattern matched
        match matched {
            ignore::Match::Ignore(_) => true,  // Should be ignored
            ignore::Match::Whitelist(_) => false, // Explicitly included
            ignore::Match::None => false,      // No match = include
        }
    }

    /// Helper to filter a list of paths, removing those that should be ignored
    pub fn filter_paths<P: AsRef<Path>>(&self, paths: Vec<P>) -> Vec<PathBuf> {
        paths
            .into_iter()
            .filter_map(|p| {
                let path = p.as_ref();
                if self.should_ignore(path) {
                    None
                } else {
                    Some(path.to_path_buf())
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_gitignore_basic() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        // Create .gitignore
        let gitignore_content = r#"
*.log
.env
node_modules/
build/
"#;
        fs::write(root.join(".gitignore"), gitignore_content).unwrap();

        // Create test files/dirs
        fs::write(root.join("test.log"), "").unwrap();
        fs::write(root.join(".env"), "").unwrap();
        fs::write(root.join("test.txt"), "").unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();

        let filter = GitignoreFilter::new(root);

        // Test filtering
        assert!(filter.should_ignore(&root.join("test.log")));
        assert!(filter.should_ignore(&root.join(".env")));
        assert!(filter.should_ignore(&root.join("node_modules")));
        assert!(filter.should_ignore(&root.join("build")));
        
        assert!(!filter.should_ignore(&root.join("test.txt")));
        assert!(!filter.should_ignore(&root.join("src")));
    }

    #[test]
    fn test_gitignore_negation() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        // Create .gitignore with negation
        let gitignore_content = r#"
*.log
!important.log
"#;
        fs::write(root.join(".gitignore"), gitignore_content).unwrap();

        let filter = GitignoreFilter::new(root);

        // test.log should be ignored
        assert!(filter.should_ignore(&root.join("test.log")));
        
        // important.log should NOT be ignored (whitelisted)
        assert!(!filter.should_ignore(&root.join("important.log")));
    }

    #[test]
    fn test_no_gitignore() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        let filter = GitignoreFilter::new(root);

        // Without .gitignore, nothing should be filtered
        assert!(!filter.should_ignore(&root.join("anything.txt")));
        assert!(!filter.should_ignore(&root.join(".env")));
    }
}
