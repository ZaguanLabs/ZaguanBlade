use ignore::gitignore::{Gitignore, GitignoreBuilder, Glob};
use ignore::Match;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Thread-safe `.gitignore` matcher with CORRECT per-directory anchoring.
///
/// Each directory's `.gitignore` is compiled on its own, anchored to ITS OWN
/// directory (built lazily and cached), exactly like git. A path is tested
/// against the global gitignore plus the `.gitignore` of every ancestor
/// directory from the workspace root down to the path's parent, with deeper
/// directories winning and `!` negations re-including — matching the behavior of
/// `git check-ignore`.
///
/// This replaces an earlier implementation that eagerly merged EVERY nested
/// `.gitignore` in the tree into a single matcher rooted at the workspace root.
/// The `ignore` crate anchors added patterns to the builder's root, so nested
/// patterns were mis-anchored and over-ignored large repos: e.g. a deep
/// `arch/riscv/kernel/vdso_cfi/.gitignore` containing `*.c` hid EVERY `.c` file
/// in the Linux kernel, and `tools/*/.gitignore` `include/` rules hid the
/// top-level `include/`, leaving discovery with ~1 file. The per-directory model
/// below scopes each pattern to its own directory, and is also lazy (no full
/// tree walk at construction).
#[derive(Clone)]
pub struct GitignoreFilter {
    workspace_root: PathBuf,
    /// Global gitignore (`~/.gitignore_global` or XDG `~/.config/git/ignore`),
    /// anchored at the workspace root and applied (lowest priority) to every
    /// path. `None` when absent.
    global: Option<Arc<Gitignore>>,
    /// Lazily-built, cached per-directory matcher: absolute directory -> its
    /// compiled `.gitignore` (anchored to that directory), or `None` if the
    /// directory has no `.gitignore`.
    per_dir: Arc<RwLock<HashMap<PathBuf, Option<Arc<Gitignore>>>>>,
}

impl GitignoreFilter {
    /// Create a new `GitignoreFilter` for the given workspace root. Per-directory
    /// `.gitignore` files are loaded lazily on first query (and cached), so
    /// construction does no filesystem walk.
    pub fn new(workspace_root: &Path) -> Self {
        let global = Self::find_global_gitignore().and_then(|path| {
            let mut builder = GitignoreBuilder::new(workspace_root);
            // `add` returns `Some(err)` only on a hard error; `None` on success.
            if builder.add(&path).is_none() {
                builder.build().ok().map(Arc::new)
            } else {
                None
            }
        });

        Self {
            workspace_root: workspace_root.to_path_buf(),
            global,
            per_dir: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Find the global gitignore file if it exists.
    fn find_global_gitignore() -> Option<PathBuf> {
        if let Some(home) = dirs::home_dir() {
            let global = home.join(".gitignore_global");
            if global.exists() {
                return Some(global);
            }
            let xdg_ignore = home.join(".config/git/ignore");
            if xdg_ignore.exists() {
                return Some(xdg_ignore);
            }
        }
        None
    }

    /// The compiled `.gitignore` for `dir` (anchored to `dir`), built once and
    /// cached. `None` when the directory has no `.gitignore` (also cached).
    fn gitignore_for_dir(&self, dir: &Path) -> Option<Arc<Gitignore>> {
        if let Some(hit) = self.per_dir.read().unwrap().get(dir) {
            return hit.clone();
        }
        let gi_path = dir.join(".gitignore");
        let built = if gi_path.is_file() {
            let mut builder = GitignoreBuilder::new(dir);
            if builder.add(&gi_path).is_none() {
                builder.build().ok().map(Arc::new)
            } else {
                None
            }
        } else {
            None
        };
        self.per_dir
            .write()
            .unwrap()
            .insert(dir.to_path_buf(), built.clone());
        built
    }

    /// Check whether a path should be ignored according to `.gitignore` rules.
    /// Returns `true` if the path SHOULD be ignored (filtered out). Accepts an
    /// absolute path or one relative to the workspace root.
    pub fn should_ignore(&self, path: &Path) -> bool {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        };

        let Ok(rel_path) = abs_path.strip_prefix(&self.workspace_root) else {
            // Outside the workspace: don't filter.
            return false;
        };
        if rel_path.as_os_str().is_empty() {
            // The workspace root itself.
            return false;
        }

        let is_dir = abs_path.is_dir();
        let components: Vec<_> = rel_path.components().map(|c| c.as_os_str()).collect();

        // Accumulate the decision, lowest-priority first so deeper directories
        // win. For each matcher we use `matched_path_or_any_parents` so that a
        // path inside an ignored ancestor directory (e.g. `build/` ignored,
        // querying `build/out.txt`) is correctly reported as ignored.
        let mut ignored = false;

        // Global gitignore: anchored at the workspace root, applied to the full
        // relative path.
        if let Some(global) = &self.global {
            apply(&mut ignored, global.matched_path_or_any_parents(rel_path, is_dir));
        }

        // Each ancestor directory's own `.gitignore`, from the workspace root
        // (j = 0) down to the path's immediate parent (j = len - 1). The path is
        // matched relative to each ancestor directory.
        let mut ancestor = self.workspace_root.clone();
        for j in 0..components.len() {
            if let Some(gitignore) = self.gitignore_for_dir(&ancestor) {
                let rel_to_ancestor: PathBuf = components[j..].iter().collect();
                apply(
                    &mut ignored,
                    gitignore.matched_path_or_any_parents(&rel_to_ancestor, is_dir),
                );
            }
            ancestor.push(components[j]);
        }

        ignored
    }

    /// Helper to filter a list of paths, removing those that should be ignored.
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

/// Fold one matcher's verdict into the running decision. `Ignore` excludes,
/// `Whitelist` (a `!` negation) re-includes, `None` leaves the prior decision
/// untouched. Called lowest-priority-first so later (deeper) matchers win.
fn apply(state: &mut bool, matched: Match<&Glob>) {
    match matched {
        Match::Ignore(_) => *state = true,
        Match::Whitelist(_) => *state = false,
        Match::None => {}
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

        let gitignore_content = r#"
*.log
.env
node_modules/
build/
"#;
        fs::write(root.join(".gitignore"), gitignore_content).unwrap();

        // Create the test files/dirs. The directory entries must exist because
        // `build/` / `node_modules/` are directory-only patterns.
        fs::write(root.join("test.log"), "").unwrap();
        fs::write(root.join(".env"), "").unwrap();
        fs::write(root.join("test.txt"), "").unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();

        let filter = GitignoreFilter::new(root);

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

        let gitignore_content = r#"
*.log
!important.log
"#;
        fs::write(root.join(".gitignore"), gitignore_content).unwrap();

        let filter = GitignoreFilter::new(root);

        assert!(filter.should_ignore(&root.join("test.log")));
        assert!(!filter.should_ignore(&root.join("important.log")));
    }

    #[test]
    fn test_no_gitignore() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        let filter = GitignoreFilter::new(root);

        assert!(!filter.should_ignore(&root.join("anything.txt")));
        assert!(!filter.should_ignore(&root.join(".env")));
    }

    /// Regression for the Linux-kernel over-ignoring bug: a NESTED `.gitignore`
    /// must only affect ITS OWN subtree, not the whole repo. A deep `*.c` rule
    /// must not hide `.c` files elsewhere, and a nested `include/` rule must not
    /// hide a top-level `include/`.
    #[test]
    fn test_nested_gitignore_does_not_leak_to_root() {
        let temp = tempdir().unwrap();
        let root = temp.path();

        // Root .gitignore ignores nothing relevant here.
        fs::write(root.join(".gitignore"), "*.o\n").unwrap();

        // A deep subdirectory ignores *.c and an include/ dir (kernel-style).
        let deep = root.join("arch/riscv/kernel/vdso_cfi");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join(".gitignore"), "*.c\ninclude/\n").unwrap();

        // Real source elsewhere in the tree.
        fs::create_dir_all(root.join("kernel/sched")).unwrap();
        fs::write(root.join("kernel/sched/core.c"), "").unwrap();
        fs::create_dir_all(root.join("include/linux")).unwrap();
        fs::write(root.join("include/linux/list.h"), "").unwrap();

        // A .c inside the deep dir.
        fs::write(deep.join("vgettimeofday.c"), "").unwrap();

        let filter = GitignoreFilter::new(root);

        // The deep rule must NOT leak to the rest of the repo.
        assert!(!filter.should_ignore(&root.join("kernel/sched/core.c")));
        assert!(!filter.should_ignore(&root.join("kernel")));
        assert!(!filter.should_ignore(&root.join("include")));
        assert!(!filter.should_ignore(&root.join("include/linux/list.h")));

        // But it MUST apply within its own subtree.
        assert!(filter.should_ignore(&deep.join("vgettimeofday.c")));
    }
}
