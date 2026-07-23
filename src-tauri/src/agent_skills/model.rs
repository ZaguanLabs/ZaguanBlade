use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    Workspace,
    User,
    Global,
}

impl SkillSource {
    pub(crate) fn scope(self) -> &'static str {
        match self {
            Self::Workspace => "repo",
            Self::User => "user",
            Self::Global => "legacy_user",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillCatalogEntry {
    pub skill_id: String,
    pub source: SkillSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(skip)]
    pub(crate) canonical_path: PathBuf,
    #[serde(skip)]
    pub(crate) file_size: u64,
    #[serde(skip)]
    pub(crate) modified: Option<SystemTime>,
}

impl SkillCatalogEntry {
    pub(crate) fn absolute_path(&self, workspace_root: &Path) -> PathBuf {
        if !self.canonical_path.as_os_str().is_empty() {
            return self.canonical_path.clone();
        }
        let path = Path::new(&self.path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace_root.join(path)
        }
    }

    pub(crate) fn scope(&self) -> &'static str {
        self.source.scope()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadedSkill {
    pub skill_id: String,
    pub name: String,
    pub base_dir: String,
    pub instructions: String,
    pub offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
    pub complete: bool,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDiscoveryDiagnostic {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct SkillCatalog {
    pub skills: Vec<SkillCatalogEntry>,
    pub diagnostics: Vec<SkillDiscoveryDiagnostic>,
    pub truncated: bool,
}
