use crate::indexer::types::ProjectIndex;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const SOFT_TARGET_BYTES: usize = 8 * 1024;
const HARD_LIMIT_BYTES: usize = 12 * 1024;

#[derive(Debug, Clone, Copy)]
struct RenderConfig {
    max_depth: usize,
    max_dirs_per_level: usize,
    max_files_per_level: usize,
    max_entry_points: usize,
}

#[derive(Debug, Default)]
struct TreeNode {
    dirs: BTreeMap<String, TreeNode>,
    files: Vec<String>,
}

pub fn generate_project_index_min(index: &ProjectIndex) -> String {
    let root = &index.root;

    // Render from richest to tightest view to stay around 4-8KB when possible.
    let configs = [
        RenderConfig {
            max_depth: 5,
            max_dirs_per_level: 24,
            max_files_per_level: 36,
            max_entry_points: 30,
        },
        RenderConfig {
            max_depth: 4,
            max_dirs_per_level: 16,
            max_files_per_level: 22,
            max_entry_points: 24,
        },
        RenderConfig {
            max_depth: 3,
            max_dirs_per_level: 10,
            max_files_per_level: 14,
            max_entry_points: 16,
        },
        RenderConfig {
            max_depth: 2,
            max_dirs_per_level: 8,
            max_files_per_level: 10,
            max_entry_points: 12,
        },
    ];

    let mut candidate = String::new();
    for cfg in configs {
        let rendered = render_with_config(index, root, cfg);
        candidate = rendered;

        if candidate.len() <= SOFT_TARGET_BYTES {
            break;
        }
    }

    truncate_to_limit(candidate, HARD_LIMIT_BYTES)
}

fn render_with_config(index: &ProjectIndex, root: &Path, cfg: RenderConfig) -> String {
    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let language_summary = summarize_languages(index);
    let ecosystem_hints = detect_ecosystem_hints(root);
    let tree = render_directory_tree(index, root, cfg);
    let entry_points = detect_entry_points(index, root, cfg.max_entry_points);

    let mut out = String::new();
    out.push_str(&format!("# Project Index (Minimal): {}\n\n", project_name));

    out.push_str("## Identity + Stack\n\n");
    out.push_str(&format!("- Project: {}\n", project_name));
    out.push_str(&format!("- Primary Languages: {}\n", language_summary));
    out.push_str(&format!("- Ecosystem Hints: {}\n\n", ecosystem_hints));

    out.push_str("## Directory/File Tree\n\n```");
    out.push('\n');
    out.push_str(&tree);
    out.push_str("```\n\n");

    out.push_str("## Entry Points\n\n");
    if entry_points.is_empty() {
        out.push_str("- (none inferred)\n");
    } else {
        for path in entry_points {
            out.push_str("- ");
            out.push_str(&path);
            out.push('\n');
        }
    }

    out
}

fn summarize_languages(index: &ProjectIndex) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();

    for metadata in index.files.values() {
        *counts.entry(metadata.language.as_str()).or_insert(0) += 1;
    }

    if counts.is_empty() {
        return "unknown".to_string();
    }

    let mut sorted: Vec<(&str, usize)> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    sorted
        .into_iter()
        .take(4)
        .map(|(lang, count)| format!("{} ({})", lang, count))
        .collect::<Vec<_>>()
        .join(", ")
}

fn detect_ecosystem_hints(root: &Path) -> String {
    let markers: [(&str, &str); 12] = [
        ("go.mod", "Go modules"),
        ("package.json", "Node.js"),
        ("bun.lock", "Bun"),
        ("bun.lockb", "Bun"),
        ("pnpm-lock.yaml", "pnpm"),
        ("Cargo.toml", "Rust/Cargo"),
        ("pyproject.toml", "Python"),
        ("requirements.txt", "Python"),
        ("Gemfile", "Ruby"),
        ("composer.json", "PHP/Composer"),
        ("pom.xml", "Java/Maven"),
        ("build.gradle", "Java/Gradle"),
    ];

    let mut found = Vec::new();
    for (file, label) in markers {
        if root.join(file).exists() {
            found.push(label);
        }
    }

    found.sort_unstable();
    found.dedup();

    if found.is_empty() {
        "none detected".to_string()
    } else {
        found.join(", ")
    }
}

fn render_directory_tree(index: &ProjectIndex, root: &Path, cfg: RenderConfig) -> String {
    let mut root_node = TreeNode::default();
    let mut rel_paths: Vec<PathBuf> = index
        .files
        .keys()
        .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
        .collect();

    rel_paths.sort_unstable();

    for rel in rel_paths {
        let mut node = &mut root_node;

        if let Some(parent) = rel.parent() {
            for dir in parent.components() {
                let dir_name = dir.as_os_str().to_string_lossy().to_string();
                node = node.dirs.entry(dir_name).or_default();
            }
        }

        if let Some(file_name) = rel.file_name().and_then(|f| f.to_str()) {
            node.files.push(file_name.to_string());
        }
    }

    sort_tree_files(&mut root_node);

    let mut lines = Vec::new();
    render_tree_recursive(&root_node, 0, cfg, String::new(), &mut lines, true);

    if lines.is_empty() {
        "(no indexed source files)\n".to_string()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn sort_tree_files(node: &mut TreeNode) {
    node.files.sort_unstable();
    for child in node.dirs.values_mut() {
        sort_tree_files(child);
    }
}

fn render_tree_recursive(
    node: &TreeNode,
    depth: usize,
    cfg: RenderConfig,
    indent: String,
    lines: &mut Vec<String>,
    is_root: bool,
) {
    if depth > cfg.max_depth {
        return;
    }

    let mut dir_entries: Vec<(&str, &TreeNode)> = node
        .dirs
        .iter()
        .map(|(name, child)| (name.as_str(), child))
        .collect();
    dir_entries.sort_by(|a, b| a.0.cmp(b.0));

    let visible_dirs = dir_entries.len().min(cfg.max_dirs_per_level);
    for (name, child) in dir_entries.into_iter().take(visible_dirs) {
        lines.push(format!("{}{}/", indent, name));
        render_tree_recursive(child, depth + 1, cfg, format!("{}  ", indent), lines, false);
    }

    if node.dirs.len() > visible_dirs {
        lines.push(format!(
            "{}... (+{} more dirs)",
            indent,
            node.dirs.len() - visible_dirs
        ));
    }

    let visible_files = node.files.len().min(cfg.max_files_per_level);
    for file in node.files.iter().take(visible_files) {
        lines.push(format!("{}{}", indent, file));
    }

    if node.files.len() > visible_files {
        lines.push(format!(
            "{}... (+{} more files)",
            indent,
            node.files.len() - visible_files
        ));
    }

    if is_root && node.dirs.is_empty() && node.files.is_empty() {
        lines.push("(empty)".to_string());
    }
}

fn detect_entry_points(index: &ProjectIndex, root: &Path, max_items: usize) -> Vec<String> {
    let mut candidates: Vec<String> = index
        .files
        .keys()
        .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
        .filter_map(|rel| rel.to_str().map(|s| s.replace('\\', "/")))
        .filter(|rel| is_entry_point_path(rel))
        .collect();

    candidates.sort_unstable();
    candidates.dedup();
    candidates.truncate(max_items);
    candidates
}

fn is_entry_point_path(rel: &str) -> bool {
    let lower = rel.to_lowercase();

    let explicit_matches = [
        "main.go",
        "main.rs",
        "main.py",
        "main.ts",
        "main.tsx",
        "main.js",
        "server.ts",
        "server.js",
        "app.ts",
        "app.js",
    ];

    if explicit_matches.iter().any(|name| lower.ends_with(name)) {
        return true;
    }

    if lower.starts_with("cmd/") && lower.ends_with("/main.go") {
        return true;
    }

    let keywords = [
        "handler",
        "handlers",
        "route",
        "router",
        "websocket",
        "chat",
    ];
    keywords.iter().any(|kw| lower.contains(kw))
}

fn truncate_to_limit(mut text: String, byte_limit: usize) -> String {
    if text.len() <= byte_limit {
        return text;
    }

    let suffix = "\n\n[Truncated to 12KB hard cap]\n";
    let keep_bytes = byte_limit.saturating_sub(suffix.len());

    let mut cut_at = keep_bytes.min(text.len());
    while cut_at > 0 && !text.is_char_boundary(cut_at) {
        cut_at -= 1;
    }

    text.truncate(cut_at);
    text.push_str(suffix);
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::builder::index_workspace;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn generates_min_index_without_file_previews() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        fs::write(temp_dir.path().join("package.json"), "{\"name\":\"demo\"}").unwrap();
        fs::write(
            temp_dir.path().join("src/main.ts"),
            "export function main() { return 1; }\n",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/handlers.ts"),
            "export const handler = () => {};\n",
        )
        .unwrap();

        let index = index_workspace(temp_dir.path()).unwrap();
        let output = generate_project_index_min(&index);

        assert!(output.contains("# Project Index (Minimal):"));
        assert!(output.contains("## Identity + Stack"));
        assert!(output.contains("## Directory/File Tree"));
        assert!(output.contains("## Entry Points"));
        assert!(!output.contains("```typescript\nexport"));
    }

    #[test]
    fn enforces_hard_cap() {
        let temp_dir = TempDir::new().unwrap();

        for i in 0..800 {
            let dir = temp_dir.path().join(format!("src/module_{}/sub", i));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(format!("file_{}.ts", i)), "export const x = 1;\n").unwrap();
        }

        let index = index_workspace(temp_dir.path()).unwrap();
        let output = generate_project_index_min(&index);

        assert!(output.len() <= HARD_LIMIT_BYTES);
    }
}
