use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;

const MAX_FRONTMATTER_BYTES: usize = 16 * 1024;
const MAX_NAME_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 1024;
const MAX_TRIGGER_COUNT: usize = 32;
const MAX_TRIGGER_CHARS: usize = 160;

#[derive(Debug)]
pub(crate) struct CatalogMetadata {
    pub name: String,
    pub description: String,
    pub short_description: Option<String>,
    pub triggers: Vec<String>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub allowed_tools: Vec<String>,
    pub recovered_name: bool,
}

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    license: Option<String>,
    compatibility: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, serde_yaml::Value>,
    #[serde(default)]
    triggers: StringOrList,
    #[serde(default, rename = "allowed-tools")]
    allowed_tools: StringOrList,
}

#[derive(Debug, Default)]
struct StringOrList(Vec<String>);

impl<'de> Deserialize<'de> for StringOrList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        let values = match value {
            serde_yaml::Value::String(value) => vec![value],
            serde_yaml::Value::Sequence(items) => items
                .into_iter()
                .filter_map(|item| match item {
                    serde_yaml::Value::String(value) => Some(value),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok(Self(values))
    }
}

pub(crate) fn read_catalog_metadata(
    path: &Path,
    directory_name: &str,
) -> Result<CatalogMetadata, String> {
    let file = File::open(path).map_err(|error| format!("open frontmatter: {error}"))?;
    let yaml = read_frontmatter(BufReader::new(file))?;
    let parsed = serde_yaml::from_str::<SkillFrontmatter>(&yaml)
        .map_err(|error| format!("parse frontmatter YAML: {error}"))?;

    let declared_name = normalize_optional(parsed.name);
    let recovered_name = declared_name.is_none();
    let name = declared_name.unwrap_or_else(|| directory_name.to_string());
    validate_name(&name)?;
    if name != directory_name {
        return Err(format!(
            "skill name {name:?} must match parent directory {directory_name:?}"
        ));
    }

    let description = normalize_optional(parsed.description)
        .ok_or_else(|| "skill description is required".to_string())?;
    let description_chars = description.chars().count();
    if description_chars > MAX_DESCRIPTION_CHARS {
        return Err(format!(
            "skill description exceeds {MAX_DESCRIPTION_CHARS} characters"
        ));
    }

    let mut metadata = BTreeMap::new();
    for (key, value) in parsed.metadata {
        if let serde_yaml::Value::String(value) = value {
            if let Some(value) = normalize_optional(Some(value)) {
                metadata.insert(key, value);
            }
        }
    }
    let short_description = metadata
        .get("short-description")
        .cloned()
        .map(|value| truncate_chars(&value, MAX_DESCRIPTION_CHARS));

    Ok(CatalogMetadata {
        name,
        description,
        short_description,
        triggers: bounded_values(parsed.triggers.0, MAX_TRIGGER_COUNT, MAX_TRIGGER_CHARS),
        license: normalize_optional(parsed.license),
        compatibility: normalize_optional(parsed.compatibility),
        metadata,
        allowed_tools: parsed
            .allowed_tools
            .0
            .into_iter()
            .flat_map(|value| {
                value
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect(),
        recovered_name,
    })
}

fn read_frontmatter(mut reader: impl BufRead) -> Result<String, String> {
    let mut line = String::new();
    let opening_bytes = reader
        .read_line(&mut line)
        .map_err(|error| format!("read opening delimiter: {error}"))?;
    if opening_bytes == 0 || trim_line_ending(&line) != "---" {
        return Err("skill must begin with a --- frontmatter delimiter".to_string());
    }

    let mut bytes_read = opening_bytes;
    let mut yaml = String::new();
    loop {
        line.clear();
        let line_bytes = reader
            .read_line(&mut line)
            .map_err(|error| format!("read frontmatter: {error}"))?;
        if line_bytes == 0 {
            return Err("skill frontmatter is missing its closing --- delimiter".to_string());
        }
        bytes_read = bytes_read.saturating_add(line_bytes);
        if bytes_read > MAX_FRONTMATTER_BYTES {
            return Err(format!(
                "skill frontmatter exceeds {MAX_FRONTMATTER_BYTES} bytes"
            ));
        }
        if trim_line_ending(&line) == "---" {
            return Ok(yaml);
        }
        yaml.push_str(&line);
    }
}

fn trim_line_ending(line: &str) -> &str {
    line.strip_suffix("\r\n")
        .or_else(|| line.strip_suffix('\n'))
        .unwrap_or(line)
}

fn validate_name(name: &str) -> Result<(), String> {
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS {
        return Err(format!(
            "skill name must contain 1 to {MAX_NAME_CHARS} characters"
        ));
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Err(format!("invalid skill name {name:?}"));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(format!("invalid skill name {name:?}"));
    }
    Ok(())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn bounded_values(values: Vec<String>, max_count: usize, max_chars: usize) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| normalize_optional(Some(value)))
        .take(max_count)
        .map(|value| truncate_chars(&value, max_chars))
        .collect()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frontmatter_reader_stops_at_closing_delimiter() {
        let raw = b"---\nname: example\ndescription: Helpful\n---\nbody: [not yaml";
        let yaml = read_frontmatter(Cursor::new(raw)).unwrap();

        assert_eq!(yaml, "name: example\ndescription: Helpful\n");
    }

    #[test]
    fn rejects_frontmatter_over_limit_without_reading_a_body() {
        let yaml = "x".repeat(MAX_FRONTMATTER_BYTES);
        let raw = format!("---\ndescription: {yaml}\n---\nbody");

        let error = read_frontmatter(Cursor::new(raw)).unwrap_err();

        assert!(error.contains("frontmatter exceeds"));
    }
}
