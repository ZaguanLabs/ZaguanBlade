use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectIndex {
    pub root: PathBuf,
    pub generated_at: DateTime<Utc>,
    pub tree: DirectoryTree,
    pub files: HashMap<PathBuf, FileMetadata>,
    #[serde(skip)]
    pub previews: HashMap<PathBuf, CachedPreview>,
    pub dirty: bool,
}

impl ProjectIndex {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            generated_at: Utc::now(),
            tree: DirectoryTree::default(),
            files: HashMap::new(),
            previews: HashMap::new(),
            dirty: false,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn update_file(&mut self, path: PathBuf, metadata: FileMetadata) {
        self.files.insert(path.clone(), metadata);
        self.previews.remove(&path);
        self.mark_dirty();
    }

    pub fn remove_file(&mut self, path: &PathBuf) {
        self.files.remove(path);
        self.previews.remove(path);
        self.mark_dirty();
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
    pub language: String,
    pub line_count: usize,
}

impl FileMetadata {
    pub fn from_path(path: &PathBuf) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        let size = metadata.len();
        let modified = metadata.modified()?;
        let language = detect_language(path);
        let line_count = count_lines(path)?;

        Ok(Self {
            path: path.clone(),
            size,
            modified,
            language,
            line_count,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CachedPreview {
    pub lines: Vec<String>,
    pub cached_at: SystemTime,
    pub file_modified: SystemTime,
}

impl CachedPreview {
    pub fn new(lines: Vec<String>, file_modified: SystemTime) -> Self {
        Self {
            lines,
            cached_at: SystemTime::now(),
            file_modified,
        }
    }

    pub fn is_valid(&self, current_modified: SystemTime) -> bool {
        self.file_modified == current_modified
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DirectoryTree {
    pub name: String,
    pub children: Vec<DirectoryTree>,
    pub files: Vec<String>,
}

impl DirectoryTree {
    pub fn new(name: String) -> Self {
        Self {
            name,
            children: Vec::new(),
            files: Vec::new(),
        }
    }

    pub fn add_file(&mut self, name: String) {
        self.files.push(name);
    }

    pub fn add_child(&mut self, child: DirectoryTree) {
        self.children.push(child);
    }

    pub fn render(&self, max_depth: usize) -> String {
        let mut output = String::new();
        self.render_recursive(&mut output, 0, max_depth, "");
        output
    }

    fn render_recursive(&self, output: &mut String, depth: usize, max_depth: usize, prefix: &str) {
        if depth > max_depth {
            return;
        }

        if depth > 0 {
            output.push_str(&format!("{}{}/\n", prefix, self.name));
        }

        let child_prefix = if depth == 0 {
            String::new()
        } else {
            format!("{}  ", prefix)
        };

        for file in &self.files {
            output.push_str(&format!("{}{}\n", child_prefix, file));
        }

        for child in &self.children {
            child.render_recursive(output, depth + 1, max_depth, &child_prefix);
        }
    }
}

pub static CODE_EXTENSIONS: &[&str] = &[
    "go", "rs", "py", "js", "ts", "tsx", "jsx",
    "java", "c", "cpp", "h", "hpp", "cs",
    "rb", "php", "swift", "kt", "scala",
    "vue", "svelte", "sql", "sh", "bash", "zsh",
    "yaml", "yml", "toml", "json", "xml",
    "html", "css", "scss", "less", "md",
];

pub static SKIP_DIRS: &[&str] = &[
    "node_modules", "vendor", "dist", "build",
    "__pycache__", ".git", ".zblade", "target",
    ".next", ".nuxt", ".output", "coverage",
];

pub fn is_code_file(path: &PathBuf) -> bool {
    if let Some(ext) = path.extension() {
        if let Some(ext_str) = ext.to_str() {
            return CODE_EXTENSIONS.contains(&ext_str);
        }
    }
    false
}

pub fn detect_language(path: &PathBuf) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| match ext {
            "rs" => "rust",
            "py" => "python",
            "js" => "javascript",
            "ts" => "typescript",
            "tsx" => "typescript",
            "jsx" => "javascript",
            "go" => "go",
            "java" => "java",
            "c" => "c",
            "cpp" | "cc" | "cxx" => "cpp",
            "h" | "hpp" => "cpp",
            "cs" => "csharp",
            "rb" => "ruby",
            "php" => "php",
            "swift" => "swift",
            "kt" => "kotlin",
            "scala" => "scala",
            "vue" => "vue",
            "svelte" => "svelte",
            "sql" => "sql",
            "sh" | "bash" | "zsh" => "bash",
            "yaml" | "yml" => "yaml",
            "toml" => "toml",
            "json" => "json",
            "xml" => "xml",
            "html" => "html",
            "css" => "css",
            "scss" => "scss",
            "less" => "less",
            "md" => "markdown",
            _ => "text",
        })
        .unwrap_or("text")
        .to_string()
}

fn count_lines(path: &PathBuf) -> std::io::Result<usize> {
    let content = std::fs::read_to_string(path)?;
    Ok(content.lines().count())
}
