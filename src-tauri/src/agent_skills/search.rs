use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use super::discovery::discover_skill_catalog;
use super::model::{SkillCatalog, SkillCatalogEntry};

const DEFAULT_LIMIT: usize = 5;
const MAX_LIMIT: usize = 10;
const MAX_DESCRIPTION_CHARS: usize = 320;
const MAX_RESULT_BYTES: usize = 4 * 1024;

#[derive(Clone, Serialize)]
struct ListSkillsResult {
    skills: Vec<ListSkillsEntry>,
    total_matches: usize,
    truncated: bool,
}

#[derive(Clone, Serialize)]
struct ListSkillsEntry {
    skill_id: String,
    name: String,
    description: String,
    scope: String,
}

pub fn list_skills(workspace_root: &Path, query: &str, limit: usize) -> Result<String, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("query is required".to_string());
    }

    let catalog = discover_skill_catalog(Some(workspace_root));
    list_catalog_skills(&catalog, query, limit)
}

fn list_catalog_skills(
    catalog: &SkillCatalog,
    query: &str,
    requested_limit: usize,
) -> Result<String, String> {
    let limit = if requested_limit == 0 {
        DEFAULT_LIMIT
    } else {
        requested_limit.min(MAX_LIMIT)
    };
    let normalized_query = normalize_search_text(query);
    let query_tokens = tokenize_search_text(query);
    let mut matches: Vec<(&SkillCatalogEntry, i32)> = catalog
        .skills
        .iter()
        .filter_map(|skill| {
            let score = score_skill(query, &normalized_query, &query_tokens, skill);
            (score > 0).then_some((skill, score))
        })
        .collect();
    matches.sort_by_key(|(skill, score)| {
        (
            Reverse(*score),
            Reverse(scope_rank(skill.scope())),
            &skill.skill_id,
        )
    });

    let total_matches = matches.len();
    let mut result = ListSkillsResult {
        skills: Vec::with_capacity(limit.min(total_matches)),
        total_matches,
        truncated: false,
    };
    for (skill, _) in matches {
        if result.skills.len() >= limit {
            result.truncated = true;
            break;
        }
        let description = skill
            .short_description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(skill.description.as_deref())
            .unwrap_or_default()
            .trim();
        let entry = ListSkillsEntry {
            skill_id: skill.skill_id.clone(),
            name: skill.name.clone().unwrap_or_default(),
            description: truncate_chars(description, MAX_DESCRIPTION_CHARS),
            scope: skill.scope().to_string(),
        };
        let mut candidate = result.clone();
        candidate.skills.push(entry);
        candidate.truncated = candidate.skills.len() < total_matches;
        let encoded = serde_json::to_string(&candidate)
            .map_err(|error| format!("encode list_skills result: {error}"))?;
        if encoded.len() > MAX_RESULT_BYTES {
            result.truncated = true;
            break;
        }
        result = candidate;
    }
    result.truncated |= result.skills.len() < total_matches;

    let encoded = serde_json::to_string(&result)
        .map_err(|error| format!("encode list_skills result: {error}"))?;
    if encoded.len() > MAX_RESULT_BYTES {
        return Err("list_skills result exceeded internal byte budget".to_string());
    }
    eprintln!(
        "[SKILLS][list] query_bytes={} result_count={} result_bytes={} total_matches={}",
        query.len(),
        result.skills.len(),
        encoded.len(),
        total_matches
    );
    Ok(encoded)
}

fn score_skill(
    raw_query: &str,
    normalized_query: &str,
    query_tokens: &[String],
    skill: &SkillCatalogEntry,
) -> i32 {
    let mut score = 0;
    if raw_query.trim().eq_ignore_ascii_case(&skill.skill_id) {
        score += 10_000;
    }

    let name = skill.name.as_deref().unwrap_or_default();
    if !normalized_query.is_empty() && normalized_query == normalize_search_text(name) {
        score += 8_000;
    }
    let name_tokens = tokenize_search_text(name);
    for query_token in query_tokens {
        for name_token in &name_tokens {
            if query_token == name_token {
                score += 240;
            } else if query_token.chars().count() >= 3 && name_token.starts_with(query_token) {
                score += 120;
            } else if name_token.chars().count() >= 3 && query_token.starts_with(name_token) {
                score += 80;
            }
        }
    }

    for trigger in &skill.triggers {
        let normalized_trigger = normalize_search_text(trigger);
        if !normalized_trigger.is_empty() && normalized_query.contains(&normalized_trigger) {
            score += 500;
        }
        score += token_overlap(query_tokens, &tokenize_search_text(trigger)) * 90;
    }
    score += token_overlap(
        query_tokens,
        &tokenize_search_text(skill.description.as_deref().unwrap_or_default()),
    ) * 30;
    score += token_overlap(
        query_tokens,
        &tokenize_search_text(skill.short_description.as_deref().unwrap_or_default()),
    ) * 45;
    score
}

fn token_overlap(left: &[String], right: &[String]) -> i32 {
    let right: HashSet<&str> = right.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    left.iter()
        .filter(|token| seen.insert(token.as_str()) && right.contains(token.as_str()))
        .count() as i32
}

fn tokenize_search_text(value: &str) -> Vec<String> {
    normalize_search_text(value)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn normalize_search_text(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_space = true;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            normalized.push(character);
            last_was_space = false;
        } else if !last_was_space {
            normalized.push(' ');
            last_was_space = true;
        }
    }
    normalized.trim().to_string()
}

fn scope_rank(scope: &str) -> i32 {
    match scope.trim() {
        "repo" => 3,
        "user" => 2,
        "legacy_user" => 1,
        _ => 0,
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_skills::SkillSource;
    use std::path::PathBuf;

    fn entry(id: &str, name: &str, description: &str, source: SkillSource) -> SkillCatalogEntry {
        SkillCatalogEntry {
            skill_id: id.to_string(),
            source,
            name: Some(name.to_string()),
            description: Some(description.to_string()),
            triggers: Vec::new(),
            short_description: None,
            path: String::new(),
            license: None,
            compatibility: None,
            metadata: Default::default(),
            allowed_tools: Vec::new(),
            canonical_path: PathBuf::new(),
            file_size: 0,
            modified: None,
        }
    }

    #[test]
    fn exact_id_wins_and_duplicate_names_are_preserved() {
        let catalog = SkillCatalog {
            skills: vec![
                entry("host:b", "review", "Review code", SkillSource::User),
                entry("host:a", "review", "Review code", SkillSource::Workspace),
            ],
            ..Default::default()
        };

        let by_name: serde_json::Value =
            serde_json::from_str(&list_catalog_skills(&catalog, "review", 10).unwrap()).unwrap();
        assert_eq!(by_name["skills"].as_array().unwrap().len(), 2);
        assert_eq!(by_name["skills"][0]["skill_id"], "host:a");

        let by_id: serde_json::Value =
            serde_json::from_str(&list_catalog_skills(&catalog, "host:b", 10).unwrap()).unwrap();
        assert_eq!(by_id["skills"][0]["skill_id"], "host:b");
    }

    #[test]
    fn result_enforces_count_description_and_byte_budgets() {
        let catalog = SkillCatalog {
            skills: (0..30)
                .map(|index| {
                    entry(
                        &format!("host:{index:02}"),
                        &format!("review-{index}"),
                        &format!("review {}", "ø".repeat(1_000)),
                        SkillSource::User,
                    )
                })
                .collect(),
            ..Default::default()
        };

        let encoded = list_catalog_skills(&catalog, "review", 100).unwrap();
        let result: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert!(encoded.len() <= MAX_RESULT_BYTES);
        assert!(result["skills"].as_array().unwrap().len() <= MAX_LIMIT);
        assert!(result["truncated"].as_bool().unwrap());
        assert!(
            result["skills"][0]["description"]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= MAX_DESCRIPTION_CHARS
        );
    }
}
