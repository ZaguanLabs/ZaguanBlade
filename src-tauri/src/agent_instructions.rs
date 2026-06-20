use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

const AGENTS_FILE_NAME: &str = "AGENTS.md";
const MAX_AGENT_INSTRUCTIONS_CHARS: usize = 32_000;
const INCLUDE_EXTENSIONS: &[&str] = &["md", "txt", "rst", "adoc"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentInstructions {
    pub content: Option<String>,
    pub files: Vec<String>,
    pub includes: Vec<String>,
    pub truncated: bool,
}

#[derive(Default)]
struct LoadState {
    agent_files: HashSet<PathBuf>,
    include_files: HashSet<PathBuf>,
}

pub fn load_agent_instructions(
    workspace_root: &Path,
    candidate_paths: &[String],
) -> AgentInstructions {
    let Some(canonical_root) = canonical_dir(workspace_root) else {
        return AgentInstructions::default();
    };

    let agent_paths = discover_agent_files(&canonical_root, candidate_paths);
    if agent_paths.is_empty() {
        return AgentInstructions::default();
    }

    let mut state = LoadState::default();
    let mut content = String::new();
    let mut files = Vec::new();
    let mut includes = Vec::new();

    for path in agent_paths {
        let Some(canonical_path) = canonical_file(&path) else {
            continue;
        };
        if !state.agent_files.insert(canonical_path.clone()) {
            continue;
        }

        let Some(raw) = read_text_file(&canonical_path) else {
            continue;
        };
        let relative = display_path(&canonical_root, &canonical_path);
        let expanded = expand_includes(
            &canonical_root,
            &canonical_path,
            &raw,
            &mut state,
            &mut includes,
        );
        if expanded.trim().is_empty() {
            continue;
        }

        files.push(relative.clone());
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str("## ");
        content.push_str(&relative);
        content.push_str("\n\n");
        content.push_str(expanded.trim());
    }

    let (content, truncated) =
        truncate_chars(content.trim().to_string(), MAX_AGENT_INSTRUCTIONS_CHARS);
    AgentInstructions {
        content: (!content.is_empty()).then_some(content),
        files,
        includes,
        truncated,
    }
}

fn discover_agent_files(workspace_root: &Path, candidate_paths: &[String]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    push_agent_file(workspace_root, workspace_root, &mut paths, &mut seen);

    for candidate in candidate_paths {
        let Some(relative) = normalize_candidate_path(workspace_root, candidate) else {
            continue;
        };
        let mut dir = relative
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(PathBuf::new);
        if relative.extension().is_none() {
            dir = relative;
        }

        let mut current = PathBuf::new();
        for component in dir.components() {
            let Component::Normal(part) = component else {
                continue;
            };
            current.push(part);
            push_agent_file(
                workspace_root,
                &workspace_root.join(&current),
                &mut paths,
                &mut seen,
            );
        }
    }

    paths
}

fn push_agent_file(
    workspace_root: &Path,
    dir: &Path,
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    let path = dir.join(AGENTS_FILE_NAME);
    if !path.is_file() {
        return;
    }
    let Some(canonical_path) = canonical_file(&path) else {
        return;
    };
    if !canonical_path.starts_with(workspace_root) || !seen.insert(canonical_path.clone()) {
        return;
    }
    paths.push(canonical_path);
}

fn expand_includes(
    workspace_root: &Path,
    source_path: &Path,
    content: &str,
    state: &mut LoadState,
    includes: &mut Vec<String>,
) -> String {
    include_regex()
        .replace_all(content, |captures: &regex::Captures<'_>| {
            let prefix = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
            let include_ref = captures.get(2).map(|m| m.as_str()).unwrap_or_default();
            let replacement =
                load_include(workspace_root, source_path, include_ref, state, includes)
                    .unwrap_or_default();
            format!("{prefix}{replacement}")
        })
        .into_owned()
}

fn load_include(
    workspace_root: &Path,
    source_path: &Path,
    include_ref: &str,
    state: &mut LoadState,
    includes: &mut Vec<String>,
) -> Option<String> {
    let include_path = resolve_include_path(workspace_root, source_path, include_ref)?;
    let canonical_path = canonical_file(&include_path)?;
    if !canonical_path.starts_with(workspace_root)
        || !state.include_files.insert(canonical_path.clone())
    {
        return None;
    }

    let raw = read_text_file(&canonical_path)?;
    let expanded = expand_includes(workspace_root, &canonical_path, &raw, state, includes);
    if expanded.trim().is_empty() {
        return None;
    }

    let relative = display_path(workspace_root, &canonical_path);
    includes.push(relative.clone());
    Some(format!(
        "\n\n### Included from {}\n\n{}\n",
        relative,
        expanded.trim()
    ))
}

fn resolve_include_path(
    workspace_root: &Path,
    source_path: &Path,
    include_ref: &str,
) -> Option<PathBuf> {
    let ref_path = Path::new(include_ref);
    if !is_allowed_include_extension(ref_path) {
        return None;
    }

    let base_dir = source_path.parent()?;
    let resolved = normalize_relative_path(base_dir.join(ref_path))?;
    if resolved.starts_with(workspace_root) {
        Some(resolved)
    } else {
        None
    }
}

fn normalize_candidate_path(workspace_root: &Path, candidate: &str) -> Option<PathBuf> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = Path::new(trimmed);
    let absolute = if path.is_absolute() {
        normalize_relative_path(path.to_path_buf())?
    } else {
        normalize_relative_path(workspace_root.join(path))?
    };
    let relative = absolute.strip_prefix(workspace_root).ok()?;
    Some(relative.to_path_buf())
}

fn normalize_relative_path(path: PathBuf) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Some(normalized)
}

fn canonical_dir(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok().filter(|path| path.is_dir())
}

fn canonical_file(path: &Path) -> Option<PathBuf> {
    if fs::symlink_metadata(path).ok()?.file_type().is_symlink() {
        return None;
    }
    fs::canonicalize(path).ok().filter(|path| path.is_file())
}

fn read_text_file(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    if content.contains('\0') {
        None
    } else {
        Some(content)
    }
}

fn is_allowed_include_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            INCLUDE_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
        .unwrap_or(false)
}

fn include_regex() -> &'static Regex {
    static INCLUDE_REGEX: OnceLock<Regex> = OnceLock::new();
    INCLUDE_REGEX.get_or_init(|| {
        Regex::new(r"(^|[\s(])@([A-Za-z0-9_./-]+\.(?i:md|txt|rst|adoc))\b")
            .expect("valid AGENTS.md include regex")
    })
}

fn display_path(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn truncate_chars(value: String, max_chars: usize) -> (String, bool) {
    if value.chars().count() <= max_chars {
        return (value, false);
    }
    (value.chars().take(max_chars).collect(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn missing_agents_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_agent_instructions(dir.path(), &[]);

        assert!(loaded.content.is_none());
        assert!(loaded.files.is_empty());
        assert!(loaded.includes.is_empty());
    }

    #[test]
    fn loads_root_and_nested_agents_in_order() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("AGENTS.md"), "root rules");
        write(&dir.path().join("src/AGENTS.md"), "src rules");
        write(&dir.path().join("src/ui/AGENTS.md"), "ui rules");

        let loaded = load_agent_instructions(dir.path(), &["src/ui/Button.tsx".to_string()]);
        let content = loaded.content.unwrap();

        assert_eq!(
            loaded.files,
            vec!["AGENTS.md", "src/AGENTS.md", "src/ui/AGENTS.md"]
        );
        assert!(content.find("root rules").unwrap() < content.find("src rules").unwrap());
        assert!(content.find("src rules").unwrap() < content.find("ui rules").unwrap());
    }

    #[test]
    fn dedupes_shared_ancestors_for_multiple_candidates() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("AGENTS.md"), "root");
        write(&dir.path().join("src/AGENTS.md"), "src");

        let loaded = load_agent_instructions(
            dir.path(),
            &["src/a.ts".to_string(), "src/b.ts".to_string()],
        );

        assert_eq!(loaded.files, vec!["AGENTS.md", "src/AGENTS.md"]);
    }

    #[test]
    fn expands_relative_and_nested_includes() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("AGENTS.md"),
            "Before @docs/workflow.md After",
        );
        write(
            &dir.path().join("docs/workflow.md"),
            "Workflow @details/checks.txt",
        );
        write(&dir.path().join("docs/details/checks.txt"), "Run tests");

        let loaded = load_agent_instructions(dir.path(), &[]);
        let content = loaded.content.unwrap();

        assert!(content.contains("Before"));
        assert!(content.contains("Workflow"));
        assert!(content.contains("Run tests"));
        assert_eq!(
            loaded.includes,
            vec!["docs/details/checks.txt", "docs/workflow.md"]
        );
    }

    #[test]
    fn include_cycles_stop_silently() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("AGENTS.md"), "@a.md");
        write(&dir.path().join("a.md"), "A @b.md");
        write(&dir.path().join("b.md"), "B @a.md");

        let loaded = load_agent_instructions(dir.path(), &[]);
        let content = loaded.content.unwrap();

        assert!(content.contains("A"));
        assert!(content.contains("B"));
        assert_eq!(loaded.includes, vec!["b.md", "a.md"]);
    }

    #[test]
    fn ignores_out_of_workspace_candidates_and_includes() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(&dir.path().join("AGENTS.md"), "root @../outside.md");
        write(&outside.path().join("AGENTS.md"), "outside");

        let loaded = load_agent_instructions(
            dir.path(),
            &[outside.path().join("file.rs").to_string_lossy().to_string()],
        );

        let content = loaded.content.unwrap();
        assert!(content.contains("root"));
        assert!(!content.contains("outside"));
        assert_eq!(loaded.files, vec!["AGENTS.md"]);
        assert!(loaded.includes.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn ignores_symlinked_instruction_files() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("real.md"), "linked");
        std::os::unix::fs::symlink(dir.path().join("real.md"), dir.path().join("AGENTS.md"))
            .unwrap();

        let loaded = load_agent_instructions(dir.path(), &[]);

        assert!(loaded.content.is_none());
        assert!(loaded.files.is_empty());
    }
}
