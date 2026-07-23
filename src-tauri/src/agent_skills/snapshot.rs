use std::path::Path;
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

use super::discovery::discover_skill_catalog;
use super::model::{SkillCatalog, SkillCatalogEntry, SkillSource};
use crate::blade_ws_client::{HostSkillCatalogEntry, HostSkillsSnapshot};

pub fn build_host_skills_snapshot(workspace_root: Option<&Path>) -> HostSkillsSnapshot {
    snapshot_from_catalog(&discover_skill_catalog(workspace_root))
}

pub(crate) fn snapshot_from_catalog(catalog: &SkillCatalog) -> HostSkillsSnapshot {
    let mut entries = catalog.skills.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.skill_id.cmp(&right.skill_id));

    let digest = snapshot_digest(&entries, catalog.truncated);
    let skills = entries.into_iter().filter_map(wire_catalog_entry).collect();

    HostSkillsSnapshot {
        schema_version: 1,
        digest,
        truncated: catalog.truncated,
        skills,
    }
}

pub(crate) fn stable_host_skill_id(source: SkillSource, canonical_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, source.scope().as_bytes());
    hash_field(&mut hasher, canonical_path.to_string_lossy().as_bytes());
    format!("host:{}", hex_digest(hasher.finalize()))
}

fn snapshot_digest(entries: &[&SkillCatalogEntry], truncated: bool) -> String {
    let mut hasher = Sha256::new();
    if truncated {
        hash_field(&mut hasher, b"truncated");
    }
    for entry in entries {
        hash_field(&mut hasher, entry.skill_id.as_bytes());
        hash_field(
            &mut hasher,
            entry.name.as_deref().unwrap_or_default().as_bytes(),
        );
        hash_field(
            &mut hasher,
            entry.description.as_deref().unwrap_or_default().as_bytes(),
        );
        hash_field(
            &mut hasher,
            entry
                .short_description
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
        for trigger in &entry.triggers {
            hash_field(&mut hasher, trigger.as_bytes());
        }
        hash_field(&mut hasher, entry.scope().as_bytes());
        hash_field(&mut hasher, entry.path.as_bytes());
        hash_field(&mut hasher, entry.file_size.to_string().as_bytes());
        let modified_nanos = entry
            .modified
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        hash_field(&mut hasher, modified_nanos.to_string().as_bytes());
    }
    format!("sha256:{}", hex_digest(hasher.finalize()))
}

fn wire_catalog_entry(entry: &SkillCatalogEntry) -> Option<HostSkillCatalogEntry> {
    Some(HostSkillCatalogEntry {
        skill_id: entry.skill_id.clone(),
        name: entry.name.clone()?,
        description: entry.description.clone()?,
        short_description: entry.short_description.clone(),
        triggers: entry.triggers.clone(),
        scope: entry.scope().to_string(),
        display_locator: Some(entry.path.clone()),
    })
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(value.len().to_be_bytes());
    hasher.update(value);
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_catalog_uses_the_canonical_empty_sha256_digest() {
        let snapshot = snapshot_from_catalog(&SkillCatalog::default());

        assert_eq!(
            snapshot.digest,
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(snapshot.skills.is_empty());
        assert!(!snapshot.truncated);
    }
}
