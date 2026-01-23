use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Thread-safe wrapper around a .gitignore matcher
#[derive(Clone)]
pub struct GitignoreFilter {
    inner: Arc<RwLock<Option<Gitignore>>>,
    workspace_root: PathBuf,
}

impl GitignoreFilter {
    /// Create a new GitignoreFilter for the given workspace root.
    /// Loads the .gitignore file if it exists.
    pub fn new(workspace_root: &Path) -> Self {
        let gitignore_path = workspace_root.join(".gitignore");
        
        let gitignore = if gitignore_path.exists() {
            let mut builder = GitignoreBuilder::new(workspace_root);
            // add() returns Option<Error>, not Result
            if let Some(e) = builder.add(&gitignore_path) {
                eprintln!("[GITIGNORE] Failed to load .gitignore: {}", e);
                None
            } else {
                Some(builder.build().unwrap_or_else(|e| {
                    eprintln!("[GITIGNORE] Failed to build gitignore matcher: {}", e);
                    // Return empty gitignore on error (fail-open)
                    GitignoreBuilder::new(workspace_root).build().unwrap()
                }))
            }
        } else {
            eprintln!("[GITIGNORE] No .gitignore found at {}", gitignore_path.display());
            None
        };

        Self {
            inner: Arc::new(RwLock::new(gitignore)),
            workspace_root: workspace_root.to_path_buf(),
        }
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
