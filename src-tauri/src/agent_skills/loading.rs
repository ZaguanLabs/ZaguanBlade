use std::fs;
use std::path::Path;

use super::discovery::{discover_available_skills, discover_available_skills_with_global_root};
use super::model::{LoadedSkill, SkillCatalogEntry, SkillSource};

const MAX_BODY_BYTES: usize = 512 * 1024;

pub fn load_skill(workspace_root: &Path, skill_id: &str) -> Result<LoadedSkill, String> {
    load_from_entries(
        workspace_root,
        discover_available_skills(workspace_root),
        skill_id,
    )
}

pub(crate) fn load_skill_with_global_root(
    workspace_root: &Path,
    global_skills_root: &Path,
    skill_id: &str,
) -> Result<LoadedSkill, String> {
    load_from_entries(
        workspace_root,
        discover_available_skills_with_global_root(workspace_root, global_skills_root),
        skill_id,
    )
}

pub fn load_workspace_skill(workspace_root: &Path, skill_id: &str) -> Result<LoadedSkill, String> {
    load_skill(workspace_root, skill_id)
}

fn load_from_entries(
    workspace_root: &Path,
    entries: Vec<SkillCatalogEntry>,
    skill_id: &str,
) -> Result<LoadedSkill, String> {
    let selector = skill_id.trim();
    if selector.is_empty() {
        return Err("load_skill requires skill_id".to_string());
    }

    let entry = if let Some(entry) = entries.iter().find(|entry| entry.skill_id == selector) {
        entry
    } else {
        let mut named = entries
            .iter()
            .filter(|entry| entry.name.as_deref() == Some(selector));
        let Some(entry) = named.next() else {
            return Err(format!("skill not found: {selector}"));
        };
        if named.next().is_some() {
            return Err(format!(
                "skill name is ambiguous; use a skill_id returned by list_skills: {selector}"
            ));
        }
        entry
    };

    let path = entry.absolute_path(workspace_root);
    let raw = read_bounded_to_string(&path)?;
    let content = body_after_frontmatter(&raw)?;
    let base_dir = base_dir_for_loaded_skill(workspace_root, entry, &path);
    Ok(LoadedSkill {
        skill_id: entry.skill_id.clone(),
        source: entry.source,
        name: entry.name.clone(),
        description: entry.description.clone(),
        triggers: entry.triggers.clone(),
        short_description: entry.short_description.clone(),
        base_dir,
        content: content.trim().to_string(),
        note: "Relative paths in this skill are relative to base_dir. Use read_file or read_many_files to inspect referenced resources only when needed.".to_string(),
    })
}

fn base_dir_for_loaded_skill(
    workspace_root: &Path,
    entry: &SkillCatalogEntry,
    path: &Path,
) -> String {
    match entry.source {
        SkillSource::Workspace => Path::new(&entry.path)
            .parent()
            .map(normalize_path)
            .unwrap_or_else(|| {
                path.parent()
                    .and_then(|parent| parent.strip_prefix(workspace_root).ok())
                    .map(normalize_path)
                    .unwrap_or_else(|| ".".to_string())
            }),
        SkillSource::User | SkillSource::Global => path
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string()),
    }
}

fn read_bounded_to_string(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("failed to stat skill: {error}"))?;
    if metadata.len() > MAX_BODY_BYTES as u64 {
        return Err("skill is too large to load".to_string());
    }
    fs::read_to_string(path).map_err(|error| format!("failed to read skill: {error}"))
}

fn body_after_frontmatter(raw: &str) -> Result<&str, String> {
    let mut offset = 0usize;
    for (index, line) in raw.split_inclusive('\n').enumerate() {
        offset += line.len();
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if index == 0 {
            if line != "---" {
                return Err("skill must begin with a --- frontmatter delimiter".to_string());
            }
            continue;
        }
        if line == "---" {
            return Ok(&raw[offset..]);
        }
    }
    Err("skill frontmatter is missing its closing --- delimiter".to_string())
}

fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_body_after_crlf_frontmatter() {
        let raw = "---\r\nname: example\r\ndescription: Helpful\r\n---\r\nBody";

        assert_eq!(body_after_frontmatter(raw).unwrap(), "Body");
    }
}
