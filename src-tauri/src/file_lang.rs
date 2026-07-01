//! Extension-based file/language helpers.
//!
//! Extracted from the removed `indexer` subsystem (M5.20) — these two functions
//! are the only pieces of it still used (by `context_pack`). Everything else in
//! that project-indexer module was superseded by the symbol index and deleted.

use std::path::PathBuf;

pub static CODE_EXTENSIONS: &[&str] = &[
    "go", "rs", "py", "js", "ts", "tsx", "jsx", "astro", "java", "c", "cpp", "h", "hpp", "cs",
    "rb", "php", "swift", "kt", "scala", "vue", "svelte", "sql", "sh", "bash", "zsh", "yaml",
    "yml", "toml", "json", "xml", "html", "css", "scss", "less", "md",
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
            "astro" => "astro",
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
