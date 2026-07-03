use crate::explorer::FileEntry;
use crate::gitignore_filter::GitignoreFilter;
use crate::project_settings;
use crate::tree_sitter::Language;
use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub struct FsDirEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub kind: FsEntryKind,
}

pub trait Fs {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<FsDirEntry>>;
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
}

pub struct RealFs;

impl Fs for RealFs {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<FsDirEntry>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let kind = if file_type.is_dir() {
                FsEntryKind::Directory
            } else if file_type.is_file() {
                FsEntryKind::File
            } else {
                continue;
            };
            entries.push(FsDirEntry {
                path: entry.path(),
                file_name: entry.file_name().to_string_lossy().to_string(),
                kind,
            });
        }
        Ok(entries)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFsNodeKind {
    File,
    Directory,
}

#[derive(Clone, Default)]
pub struct TestFs {
    nodes: Arc<RwLock<BTreeMap<PathBuf, TestFsNodeKind>>>,
}

impl TestFs {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let fs = Self::default();
        fs.add_dir(root);
        fs
    }

    pub fn add_dir(&self, path: impl AsRef<Path>) {
        self.insert_node(path.as_ref(), TestFsNodeKind::Directory);
    }

    pub fn add_file(&self, path: impl AsRef<Path>) {
        self.insert_node(path.as_ref(), TestFsNodeKind::File);
    }

    fn insert_node(&self, path: &Path, kind: TestFsNodeKind) {
        let normalized = normalize_path(path);
        let mut nodes = self.nodes.write().unwrap();
        let mut current = PathBuf::new();
        for component in normalized.components() {
            current.push(component.as_os_str());
            nodes
                .entry(current.clone())
                .or_insert(TestFsNodeKind::Directory);
        }
        nodes.insert(normalized, kind);
    }
}

impl Fs for TestFs {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<FsDirEntry>> {
        let dir = normalize_path(path);
        let nodes = self.nodes.read().unwrap();
        match nodes.get(&dir) {
            Some(TestFsNodeKind::Directory) => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("directory not found: {}", dir.display()),
                ));
            }
        }

        let mut entries = Vec::new();
        for (candidate, kind) in nodes.iter() {
            if candidate == &dir {
                continue;
            }
            if candidate.parent() != Some(dir.as_path()) {
                continue;
            }
            let file_name = candidate
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            let kind = match kind {
                TestFsNodeKind::File => FsEntryKind::File,
                TestFsNodeKind::Directory => FsEntryKind::Directory,
            };
            entries.push(FsDirEntry {
                path: candidate.clone(),
                file_name,
                kind,
            });
        }

        Ok(entries)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(normalize_path(path))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPathMatch {
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct WorktreeEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub hidden: bool,
    pub supported_language: bool,
}

#[derive(Debug, Clone)]
pub struct WorktreeSnapshot {
    workspace_root: PathBuf,
    entries: Vec<WorktreeEntry>,
    children: BTreeMap<String, Vec<usize>>,
}

pub struct WorktreeStore {
    workspace_root: PathBuf,
    snapshot: RwLock<Arc<WorktreeSnapshot>>,
}

pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(component) => normalized.push(component),
        }
    }
    normalized
}

pub fn normalize_rel_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn resolve_path_under_workspace_root(
    workspace_root: &Path,
    path: &Path,
) -> Result<PathBuf, String> {
    let root = std::fs::canonicalize(workspace_root).map_err(|e| {
        format!(
            "Failed to canonicalize workspace root '{}': {}",
            workspace_root.display(),
            e
        )
    })?;
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let normalized = normalize_path(&candidate);
    if !normalized.starts_with(&root) {
        return Err(format!(
            "Path is outside workspace (workspace: {}, path: {})",
            root.display(),
            normalized.display()
        ));
    }
    Ok(normalized)
}

impl WorktreeStore {
    pub fn new(workspace_root: PathBuf) -> Result<Self, String> {
        let workspace_root = std::fs::canonicalize(&workspace_root).unwrap_or(workspace_root);
        let snapshot = Arc::new(WorktreeSnapshot::build(
            &workspace_root,
            allow_gitignored_files(&workspace_root),
        )?);
        Ok(Self {
            workspace_root,
            snapshot: RwLock::new(snapshot),
        })
    }

    pub fn refresh(&self) -> Result<(), String> {
        let snapshot = Arc::new(WorktreeSnapshot::build(
            &self.workspace_root,
            allow_gitignored_files(&self.workspace_root),
        )?);
        *self.snapshot.write().unwrap() = snapshot;
        Ok(())
    }

    pub fn snapshot(&self) -> Arc<WorktreeSnapshot> {
        self.snapshot.read().unwrap().clone()
    }

    pub fn search_paths(&self, query: &str, limit: Option<usize>) -> Vec<SnapshotPathMatch> {
        self.snapshot()
            .search_paths(query, limit.unwrap_or(12).min(50))
    }

    pub fn supported_language_files(&self, scope: &str) -> Vec<String> {
        self.snapshot().supported_language_files(scope)
    }
}

impl WorktreeSnapshot {
    pub fn build(workspace_root: &Path, allow_gitignored_files: bool) -> Result<Self, String> {
        let fs = RealFs;
        let workspace_root = fs.canonicalize(workspace_root).map_err(|e| e.to_string())?;
        let gitignore_filter = if allow_gitignored_files {
            None
        } else {
            Some(GitignoreFilter::new(&workspace_root))
        };
        Self::build_with_fs_internal(&fs, workspace_root, gitignore_filter.as_ref())
    }

    pub fn build_with_fs<F: Fs>(workspace_root: &Path, fs: &F) -> Result<Self, String> {
        let workspace_root = fs.canonicalize(workspace_root).map_err(|e| e.to_string())?;
        Self::build_with_fs_internal(fs, workspace_root, None)
    }

    fn build_with_fs_internal<F: Fs>(
        fs: &F,
        workspace_root: PathBuf,
        gitignore_filter: Option<&GitignoreFilter>,
    ) -> Result<Self, String> {
        let mut entries = Vec::new();
        let mut children = BTreeMap::new();
        children.insert(String::new(), Vec::new());
        Self::visit_dir(
            fs,
            &workspace_root,
            "",
            gitignore_filter,
            &mut entries,
            &mut children,
        )?;
        for indices in children.values_mut() {
            indices.sort_by(|left, right| {
                let left_entry = &entries[*left];
                let right_entry = &entries[*right];
                match (left_entry.is_dir, right_entry.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => left_entry
                        .name
                        .to_lowercase()
                        .cmp(&right_entry.name.to_lowercase()),
                }
            });
        }
        Ok(Self {
            workspace_root,
            entries,
            children,
        })
    }

    fn visit_dir<F: Fs>(
        fs: &F,
        absolute_dir: &Path,
        relative_dir: &str,
        gitignore_filter: Option<&GitignoreFilter>,
        entries: &mut Vec<WorktreeEntry>,
        children: &mut BTreeMap<String, Vec<usize>>,
    ) -> Result<(), String> {
        let mut dir_entries = fs.read_dir(absolute_dir).map_err(|e| e.to_string())?;
        dir_entries.sort_by(|left, right| {
            left.file_name
                .to_lowercase()
                .cmp(&right.file_name.to_lowercase())
        });

        for entry in dir_entries {
            if should_skip_component(&entry.file_name) {
                continue;
            }
            if let Some(filter) = gitignore_filter {
                if filter.should_ignore(&entry.path) {
                    continue;
                }
            }

            let relative_path = if relative_dir.is_empty() {
                entry.file_name.clone()
            } else {
                format!("{}/{}", relative_dir, entry.file_name.as_str())
            };
            let is_dir = matches!(entry.kind, FsEntryKind::Directory);
            let hidden = relative_path
                .split('/')
                .any(|component| component.starts_with('.'));
            let supported_language = !is_dir && is_supported_index_path(&relative_path);
            let index = entries.len();
            entries.push(WorktreeEntry {
                path: relative_path.clone(),
                name: entry.file_name.clone(),
                is_dir,
                hidden,
                supported_language,
            });
            children
                .entry(relative_dir.to_string())
                .or_default()
                .push(index);

            if is_dir {
                children.entry(relative_path.clone()).or_default();
                Self::visit_dir(
                    fs,
                    &entry.path,
                    &relative_path,
                    gitignore_filter,
                    entries,
                    children,
                )?;
            }
        }

        Ok(())
    }

    pub fn list_directory(&self, relative_dir: &str) -> Vec<FileEntry> {
        let key = normalize_query_dir(relative_dir);
        let Some(children) = self.children.get(&key) else {
            return Vec::new();
        };
        children
            .iter()
            .map(|index| {
                let entry = &self.entries[*index];
                FileEntry {
                    name: entry.name.clone(),
                    path: self
                        .workspace_root
                        .join(&entry.path)
                        .to_string_lossy()
                        .to_string(),
                    is_dir: entry.is_dir,
                    children: None,
                }
            })
            .collect()
    }

    pub fn search_paths(&self, query: &str, limit: usize) -> Vec<SnapshotPathMatch> {
        let query = query.trim().replace('\\', "/");
        let mut matches = self
            .entries
            .iter()
            .filter_map(|entry| {
                path_match_rank(&query, &entry.path).map(|rank| {
                    (
                        rank,
                        SnapshotPathMatch {
                            path: entry.path.clone(),
                            is_dir: entry.is_dir,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();

        matches.sort_by(|(left_rank, left_match), (right_rank, right_match)| {
            left_rank
                .cmp(right_rank)
                .then_with(|| left_match.path.cmp(&right_match.path))
        });
        matches.truncate(limit);
        matches.into_iter().map(|(_, entry)| entry).collect()
    }

    pub fn supported_language_files(&self, scope: &str) -> Vec<String> {
        let scope = normalize_query_dir(scope);
        self.entries
            .iter()
            .filter(|entry| {
                !entry.is_dir
                    && entry.supported_language
                    // M6.2 — dotenv files are hidden (dotfiles) but must still be
                    // indexed; every other hidden path stays excluded.
                    && (!entry.hidden || is_dotenv_path(&entry.path))
                    && path_in_scope(&entry.path, &scope)
            })
            .map(|entry| entry.path.clone())
            .collect()
    }
}

fn is_supported_index_path(path: &str) -> bool {
    Language::capability_for_path(path).is_some() || is_translation_resource_path(path)
}

/// M6.2 — a `.env` / `.env.*` file (by basename). These are hidden dotfiles but are
/// indexed as key/value config, so discovery includes them despite the hidden flag.
fn is_dotenv_path(path: &str) -> bool {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    file_name == ".env" || file_name.starts_with(".env.")
}

fn is_translation_resource_path(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_lowercase();
    let extension_supported =
        lower.ends_with(".json") || lower.ends_with(".yaml") || lower.ends_with(".yml");
    if !extension_supported || is_known_non_translation_json_path(&lower) {
        return false;
    }

    let components = lower.split('/').collect::<Vec<_>>();
    let in_translation_dir = components.iter().any(|component| {
        matches!(
            *component,
            "i18n"
                | "intl"
                | "l10n"
                | "lang"
                | "langs"
                | "locale"
                | "locales"
                | "messages"
                | "translations"
                | "dictionaries"
                | "dictionary"
        )
    });
    let file_name = components.last().copied().unwrap_or_default();
    in_translation_dir
        || file_name.contains("translation")
        || file_name.contains("message")
        || is_locale_resource_file_name(file_name)
}

fn is_known_non_translation_json_path(lower_path: &str) -> bool {
    [
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "jsconfig.json",
        "composer.json",
        "deno.json",
        "deno.lock",
        "bun.lockb",
    ]
    .iter()
    .any(|suffix| lower_path.ends_with(suffix))
}

fn is_locale_resource_file_name(file_name: &str) -> bool {
    let stem = file_name
        .strip_suffix(".json")
        .or_else(|| file_name.strip_suffix(".yaml"))
        .or_else(|| file_name.strip_suffix(".yml"))
        .unwrap_or(file_name);
    let parts = stem
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    matches!(parts.as_slice(), [language] if is_locale_part(language, 2, 3))
        || matches!(parts.as_slice(), [language, region]
            if is_locale_part(language, 2, 3) && is_locale_part(region, 2, 4))
}

fn is_locale_part(part: &str, min_len: usize, max_len: usize) -> bool {
    (min_len..=max_len).contains(&part.len()) && part.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn allow_gitignored_files(workspace_root: &Path) -> bool {
    project_settings::load_project_settings_or_default(workspace_root).allow_gitignored_files
}

fn normalize_query_dir(path: &str) -> String {
    if path.is_empty() || path == "." {
        return String::new();
    }
    let normalized = normalize_rel_path(Path::new(path));
    normalized.trim_matches('/').to_string()
}

fn path_in_scope(path: &str, scope: &str) -> bool {
    scope.is_empty() || path == scope || path.starts_with(&format!("{}/", scope))
}

fn should_skip_component(name: &str) -> bool {
    match name {
        ".git" | ".zblade" | "node_modules" | "target" | "dist" | "build" | "__pycache__"
        | ".svn" | "vendor" | ".next" | ".nuxt" | ".venv" | "venv" | ".cargo" | ".rustup" => true,
        _ => false,
    }
}

fn path_match_rank(query: &str, rel_path: &str) -> Option<(u8, usize, usize)> {
    if query.is_empty() {
        return Some((3, rel_path.len(), rel_path.matches('/').count()));
    }

    let query_lower = query.to_lowercase();
    let path_lower = rel_path.to_lowercase();
    let file_name = rel_path
        .rsplit('/')
        .next()
        .unwrap_or(rel_path)
        .to_lowercase();

    if file_name.starts_with(&query_lower) {
        return Some((0, rel_path.len(), rel_path.matches('/').count()));
    }
    if path_lower.starts_with(&query_lower) {
        return Some((1, rel_path.len(), rel_path.matches('/').count()));
    }
    if path_lower.contains(&query_lower) {
        return Some((2, rel_path.len(), rel_path.matches('/').count()));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_snapshot_build_with_fake_fs() {
        let root = PathBuf::from("/workspace");
        let fs = TestFs::new(&root);
        fs.add_dir(root.join("src"));
        fs.add_file(root.join("src/main.rs"));
        fs.add_file(root.join("src/lib.rs"));
        fs.add_dir(root.join("node_modules"));
        fs.add_file(root.join("node_modules/pkg/index.js"));
        fs.add_dir(root.join(".zblade"));
        fs.add_file(root.join(".zblade/config/settings.json"));

        let snapshot = WorktreeSnapshot::build_with_fs(&root, &fs).unwrap();
        let root_entries = snapshot.list_directory("");
        assert_eq!(root_entries.len(), 1);
        assert_eq!(root_entries[0].name, "src");

        let src_entries = snapshot.list_directory("src");
        assert_eq!(src_entries.len(), 2);
        assert!(src_entries.iter().any(|entry| entry.name == "main.rs"));
        assert!(src_entries.iter().any(|entry| entry.name == "lib.rs"));
    }

    #[test]
    fn test_snapshot_search_and_language_scope() {
        let root = PathBuf::from("/workspace");
        let fs = TestFs::new(&root);
        fs.add_dir(root.join("src"));
        fs.add_dir(root.join("tests"));
        fs.add_file(root.join("src/main.rs"));
        fs.add_file(root.join("src/mod.ts"));
        fs.add_file(root.join("tests/main_test.rs"));
        fs.add_file(root.join("README.md"));
        fs.add_file(root.join(".env"));

        let snapshot = WorktreeSnapshot::build_with_fs(&root, &fs).unwrap();
        let matches = snapshot.search_paths("main", 10);
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().any(|entry| entry.path == "src/main.rs"));
        assert!(matches
            .iter()
            .any(|entry| entry.path == "tests/main_test.rs"));

        let language_files = snapshot.supported_language_files("src");
        assert_eq!(language_files.len(), 2);
        assert!(language_files.iter().any(|path| path == "src/main.rs"));
        assert!(language_files.iter().any(|path| path == "src/mod.ts"));
    }

    #[test]
    fn dotenv_files_are_discovered_despite_being_hidden() {
        let root = PathBuf::from("/workspace");
        let fs = TestFs::new(&root);
        fs.add_dir(root.join("src"));
        fs.add_file(root.join("src/main.rs"));
        fs.add_file(root.join(".env"));
        fs.add_file(root.join(".env.local"));
        // A non-dotenv hidden file (still a supported extension) stays excluded.
        fs.add_file(root.join(".secret.toml"));

        let snapshot = WorktreeSnapshot::build_with_fs(&root, &fs).unwrap();
        let files = snapshot.supported_language_files(".");
        assert!(files.iter().any(|path| path == ".env"), "{files:?}");
        assert!(files.iter().any(|path| path == ".env.local"), "{files:?}");
        assert!(files.iter().any(|path| path == "src/main.rs"));
        assert!(
            !files.iter().any(|path| path == ".secret.toml"),
            "non-dotenv hidden files stay excluded: {files:?}"
        );
    }

    #[test]
    fn test_resolve_path_under_workspace_root_rejects_escape() {
        let root = PathBuf::from("/workspace");
        let result = resolve_path_under_workspace_root(&root, Path::new("../etc/passwd"));
        assert!(result.is_err());
    }
}
