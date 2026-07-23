use std::fs;
use std::path::Path;

use super::discovery::discover_available_skills;
#[cfg(test)]
use super::discovery::discover_available_skills_with_global_root;
use super::model::{LoadedSkill, SkillCatalogEntry};

const MAX_SKILL_FILE_BYTES: usize = 40 * 1024;
const MAX_INSTRUCTION_CHUNK_BYTES: usize = 12 * 1024;

pub fn load_skill(workspace_root: &Path, skill_id: &str) -> Result<LoadedSkill, String> {
    load_skill_chunk(workspace_root, skill_id, 0)
}

pub fn load_skill_chunk(
    workspace_root: &Path,
    skill_id: &str,
    offset: usize,
) -> Result<LoadedSkill, String> {
    load_from_entries(
        workspace_root,
        discover_available_skills(workspace_root),
        skill_id,
        offset,
    )
}

#[cfg(test)]
pub(crate) fn load_skill_with_global_root(
    workspace_root: &Path,
    global_skills_root: &Path,
    skill_id: &str,
) -> Result<LoadedSkill, String> {
    load_from_entries(
        workspace_root,
        discover_available_skills_with_global_root(workspace_root, global_skills_root),
        skill_id,
        0,
    )
}

pub fn load_workspace_skill(workspace_root: &Path, skill_id: &str) -> Result<LoadedSkill, String> {
    load_skill(workspace_root, skill_id)
}

fn load_from_entries(
    workspace_root: &Path,
    entries: Vec<SkillCatalogEntry>,
    skill_id: &str,
    offset: usize,
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
    if offset > content.len() {
        return Err(format!(
            "offset {offset} exceeds skill instruction size {}",
            content.len()
        ));
    }
    if !content.is_char_boundary(offset) {
        return Err(format!("offset {offset} is not a UTF-8 boundary"));
    }
    let end = instruction_chunk_end(content, offset);
    let complete = end == content.len();
    let base_dir = base_dir_for_loaded_skill(&path);
    Ok(LoadedSkill {
        skill_id: entry.skill_id.clone(),
        name: entry.name.clone().unwrap_or_default(),
        base_dir,
        instructions: content[offset..end].to_string(),
        offset,
        next_offset: (!complete).then_some(end),
        complete,
        note: "Relative paths in this skill are relative to base_dir. Use read_file or read_many_files to inspect referenced resources only when needed.".to_string(),
    })
}

fn base_dir_for_loaded_skill(path: &Path) -> String {
    path.parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string())
}

fn read_bounded_to_string(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("failed to stat skill: {error}"))?;
    if metadata.len() > MAX_SKILL_FILE_BYTES as u64 {
        return Err(format!(
            "skill exceeds the {MAX_SKILL_FILE_BYTES}-byte limit; move detailed material into referenced files"
        ));
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

fn instruction_chunk_end(content: &str, offset: usize) -> usize {
    let mut end = offset
        .saturating_add(MAX_INSTRUCTION_CHUNK_BYTES)
        .min(content.len());
    while end > offset && !content.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_body_after_crlf_frontmatter() {
        let raw = "---\r\nname: example\r\ndescription: Helpful\r\n---\r\nBody";

        assert_eq!(body_after_frontmatter(raw).unwrap(), "Body");
    }

    #[test]
    fn instruction_chunks_end_on_utf8_boundaries() {
        let content = format!("{}é-tail", "a".repeat(MAX_INSTRUCTION_CHUNK_BYTES - 1));
        let first_end = instruction_chunk_end(&content, 0);

        assert_eq!(first_end, MAX_INSTRUCTION_CHUNK_BYTES - 1);
        assert!(content.is_char_boundary(first_end));
        assert_eq!(
            &content[first_end..instruction_chunk_end(&content, first_end)],
            "é-tail"
        );
    }
}
