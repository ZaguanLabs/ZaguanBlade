use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::{DirEntry, WalkDir};

const AGENTS_SKILLS_DIR: &str = ".agents/skills";
const SKILL_FILE_NAME: &str = "SKILL.md";
const MAX_SCAN_DEPTH: usize = 8;
const MAX_SKILLS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 1024;
const MAX_TRIGGER_COUNT: usize = 32;
const MAX_TRIGGER_CHARS: usize = 160;
const MAX_BODY_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillCatalogEntry {
    pub skill_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadedSkill {
    pub skill_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
    pub base_dir: String,
    pub content: String,
    pub note: String,
}

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    triggers: TriggerList,
    #[serde(default)]
    metadata: SkillFrontmatterMetadata,
}

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatterMetadata {
    #[serde(rename = "short-description")]
    short_description: Option<String>,
}

#[derive(Debug, Default)]
struct ParsedSkillFile {
    frontmatter: SkillFrontmatter,
    body: String,
}

#[derive(Debug, Default)]
struct TriggerList(Vec<String>);

impl<'de> Deserialize<'de> for TriggerList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        let triggers = match value {
            serde_yaml::Value::Sequence(items) => items
                .into_iter()
                .filter_map(|item| match item {
                    serde_yaml::Value::String(value) => Some(value),
                    _ => None,
                })
                .collect(),
            serde_yaml::Value::String(value) => vec![value],
            _ => Vec::new(),
        };
        Ok(Self(triggers))
    }
}

pub fn discover_workspace_skills(workspace_root: &Path) -> Vec<SkillCatalogEntry> {
    let skills_root = workspace_root.join(AGENTS_SKILLS_DIR);
    if !skills_root.is_dir() {
        return Vec::new();
    }

    let mut skill_paths = WalkDir::new(&skills_root)
        .follow_links(false)
        .max_depth(MAX_SCAN_DEPTH)
        .into_iter()
        .filter_entry(should_descend)
        .filter_map(Result::ok)
        .filter(is_skill_file)
        .map(|entry| entry.path().to_path_buf())
        .collect::<Vec<PathBuf>>();
    skill_paths.sort();

    let mut discovered = BTreeMap::<String, SkillCatalogEntry>::new();
    for path in skill_paths {
        if discovered.len() >= MAX_SKILLS {
            break;
        }
        let Some(skill) = catalog_entry_from_path(workspace_root, &path) else {
            continue;
        };
        discovered.entry(skill.skill_id.clone()).or_insert(skill);
    }

    discovered.into_values().collect()
}

pub fn load_workspace_skill(workspace_root: &Path, skill_id: &str) -> Result<LoadedSkill, String> {
    let normalized_id = skill_id.trim();
    if normalized_id.is_empty() {
        return Err("load_skill requires skill_id".to_string());
    }

    for entry in discover_workspace_skills(workspace_root) {
        if entry.skill_id != normalized_id && entry.name.as_deref() != Some(normalized_id) {
            continue;
        }

        let path = workspace_root.join(&entry.path);
        let raw = read_bounded_to_string(&path)?;
        let parsed = parse_skill_file(&raw);
        let base_dir = path
            .parent()
            .and_then(|parent| relative_to_workspace(workspace_root, parent))
            .unwrap_or_else(|| ".".to_string());
        return Ok(LoadedSkill {
            skill_id: entry.skill_id,
            name: entry.name,
            description: entry.description,
            triggers: entry.triggers,
            short_description: entry.short_description,
            base_dir,
            content: parsed.body.trim().to_string(),
            note: "Relative paths in this skill are relative to base_dir. Use read_file or read_many_files to inspect referenced resources only when needed.".to_string(),
        });
    }

    Err(format!("skill not found: {normalized_id}"))
}

pub fn render_available_skills_for_prompt(workspace_root: &Path) -> Option<String> {
    let skills = discover_workspace_skills(workspace_root);
    if skills.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    lines.push("# Available Skills".to_string());
    lines.push(
        "Skills are optional task-specific instructions. Use `load_skill` when a skill clearly matches the user's request. Do not load all skills; if no skill applies, continue normally."
            .to_string(),
    );
    lines.push("<available_skills>".to_string());
    for skill in skills {
        lines.push("  <skill>".to_string());
        lines.push(format!(
            "    <skill_id>{}</skill_id>",
            escape_xml(&skill.skill_id)
        ));
        if let Some(name) = skill.name.as_deref() {
            lines.push(format!("    <name>{}</name>", escape_xml(name)));
        }
        if let Some(description) = skill
            .short_description
            .as_deref()
            .or(skill.description.as_deref())
        {
            lines.push(format!(
                "    <description>{}</description>",
                escape_xml(description)
            ));
        }
        if !skill.triggers.is_empty() {
            lines.push(format!(
                "    <triggers>{}</triggers>",
                escape_xml(&skill.triggers.join(", "))
            ));
        }
        lines.push("  </skill>".to_string());
    }
    lines.push("</available_skills>".to_string());
    lines.push("After loading a skill, follow its instructions for this turn. Referenced files, scripts, and assets are not automatically loaded.".to_string());
    Some(lines.join("\n"))
}

fn catalog_entry_from_path(workspace_root: &Path, path: &Path) -> Option<SkillCatalogEntry> {
    let raw = read_bounded_to_string(path).ok()?;
    let parsed = parse_skill_file(&raw);
    let skill_dir_name = path.parent()?.file_name()?.to_string_lossy().to_string();
    let skill_id = first_non_empty([
        parsed.frontmatter.id.as_deref(),
        parsed.frontmatter.name.as_deref(),
        Some(skill_dir_name.as_str()),
    ])?;
    let path = relative_to_workspace(workspace_root, path)?;

    Some(SkillCatalogEntry {
        skill_id: skill_id.to_string(),
        name: normalize_optional_string(parsed.frontmatter.name),
        description: normalize_optional_string(parsed.frontmatter.description)
            .map(|value| truncate_chars(&value, MAX_DESCRIPTION_CHARS)),
        triggers: bounded_triggers(parsed.frontmatter.triggers.0),
        short_description: normalize_optional_string(parsed.frontmatter.metadata.short_description)
            .map(|value| truncate_chars(&value, MAX_DESCRIPTION_CHARS)),
        path,
    })
}

fn parse_skill_file(raw: &str) -> ParsedSkillFile {
    let Some(after_opening) = raw.strip_prefix("---") else {
        return ParsedSkillFile {
            frontmatter: SkillFrontmatter::default(),
            body: raw.to_string(),
        };
    };

    let after_opening = after_opening.strip_prefix('\n').unwrap_or(after_opening);
    let mut frontmatter = Vec::new();
    let mut body = Vec::new();
    let mut found_closing = false;

    for line in after_opening.lines() {
        if !found_closing && line.trim() == "---" {
            found_closing = true;
            continue;
        }
        if found_closing {
            body.push(line);
        } else {
            frontmatter.push(line);
        }
    }

    if !found_closing {
        return ParsedSkillFile {
            frontmatter: SkillFrontmatter::default(),
            body: raw.to_string(),
        };
    }

    let frontmatter = serde_yaml::from_str::<SkillFrontmatter>(&frontmatter.join("\n"))
        .unwrap_or_else(|_| SkillFrontmatter::default());
    ParsedSkillFile {
        frontmatter,
        body: body.join("\n"),
    }
}

fn read_bounded_to_string(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|err| format!("failed to stat skill: {err}"))?;
    if metadata.len() > MAX_BODY_BYTES as u64 {
        return Err("skill is too large to load".to_string());
    }
    fs::read_to_string(path).map_err(|err| format!("failed to read skill: {err}"))
}

fn should_descend(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    entry
        .file_name()
        .to_str()
        .map(|name| !name.starts_with('.'))
        .unwrap_or(false)
}

fn is_skill_file(entry: &DirEntry) -> bool {
    entry.file_type().is_file() && entry.file_name() == SKILL_FILE_NAME
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn bounded_triggers(triggers: Vec<String>) -> Vec<String> {
    triggers
        .into_iter()
        .filter_map(|value| normalize_optional_string(Some(value)))
        .take(MAX_TRIGGER_COUNT)
        .map(|value| truncate_chars(&value, MAX_TRIGGER_CHARS))
        .collect()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

fn relative_to_workspace(workspace_root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(workspace_root).ok()?;
    Some(if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        normalize_path(relative)
    })
}

fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().expect("test path has parent")).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn missing_skills_directory_returns_empty_catalog() {
        let dir = TempDir::new().unwrap();

        assert!(discover_workspace_skills(dir.path()).is_empty());
        assert!(render_available_skills_for_prompt(dir.path()).is_none());
    }

    #[test]
    fn discovers_skill_with_zcoderd_id_precedence() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join(".agents/skills/example/SKILL.md"),
            "---\nid: explicit-id\nname: display-name\ndescription: Do the thing\ntriggers:\n  - thing\nmetadata:\n  short-description: Short thing\n---\nUse this workflow.",
        );

        let skills = discover_workspace_skills(dir.path());

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].skill_id, "explicit-id");
        assert_eq!(skills[0].name.as_deref(), Some("display-name"));
        assert_eq!(skills[0].short_description.as_deref(), Some("Short thing"));
        assert_eq!(skills[0].triggers, vec!["thing"]);
    }

    #[test]
    fn falls_back_to_name_then_directory_name() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join(".agents/skills/by-name/SKILL.md"),
            "---\nname: named-skill\n---\nBody",
        );
        write(&dir.path().join(".agents/skills/by-dir/SKILL.md"), "Body");

        let skills = discover_workspace_skills(dir.path());
        let ids = skills
            .iter()
            .map(|skill| skill.skill_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["by-dir", "named-skill"]);
    }

    #[test]
    fn load_skill_returns_body_and_base_dir() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join(".agents/skills/example/SKILL.md"),
            "---\nid: example\n---\nRead references/guide.md.",
        );

        let loaded = load_workspace_skill(dir.path(), "example").unwrap();

        assert_eq!(loaded.base_dir, ".agents/skills/example");
        assert_eq!(loaded.content, "Read references/guide.md.");
    }

    #[test]
    fn prompt_renders_metadata_not_body() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join(".agents/skills/example/SKILL.md"),
            "---\nid: example\ndescription: Helpful workflow\n---\nSecret body instructions.",
        );

        let prompt = render_available_skills_for_prompt(dir.path()).unwrap();

        assert!(prompt.contains("<skill_id>example</skill_id>"));
        assert!(prompt.contains("Helpful workflow"));
        assert!(!prompt.contains("Secret body instructions"));
    }
}
