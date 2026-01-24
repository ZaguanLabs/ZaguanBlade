use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Storage mode for conversation history
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StorageMode {
    #[default]
    Local,
    Server,
}

/// Compression model location
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CompressionModel {
    #[default]
    Remote,
    Local,
}

/// Cache settings for context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cache_size")]
    pub max_size_mb: u32,
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size_mb: 100,
        }
    }
}

/// Storage settings (RFC-002)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSettings {
    #[serde(default)]
    pub mode: StorageMode,
    #[serde(default = "default_true")]
    pub sync_metadata: bool,
    #[serde(default)]
    pub cache: CacheSettings,
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            mode: StorageMode::Local,
            sync_metadata: true,
            cache: CacheSettings::default(),
        }
    }
}

/// Compression settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub model: CompressionModel,
}

impl Default for CompressionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            model: CompressionModel::Remote,
        }
    }
}

/// Context settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSettings {
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub compression: CompressionSettings,
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self {
            max_tokens: 8000,
            compression: CompressionSettings::default(),
        }
    }
}

/// Privacy settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrivacySettings {
    #[serde(default)]
    pub telemetry: bool,
}

/// Editor settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSettings {}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {}
    }
}

/// Per-project settings stored in .zblade/config/settings.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectSettings {
    #[serde(default)]
    pub storage: StorageSettings,
    #[serde(default)]
    pub context: ContextSettings,
    #[serde(default)]
    pub privacy: PrivacySettings,
    #[serde(default)]
    pub editor: EditorSettings,
    /// Whether to allow access to files matched by .gitignore patterns
    /// Default: false (respect .gitignore for security)
    #[serde(default = "default_false")]
    pub allow_gitignored_files: bool,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_cache_size() -> u32 {
    100
}

fn default_max_tokens() -> u32 {
    8000
}

/// Get the .zblade directory path for a project
pub fn get_zblade_dir(project_path: &Path) -> PathBuf {
    project_path.join(".zblade")
}

/// Get the settings file path for a project
pub fn get_settings_path(project_path: &Path) -> PathBuf {
    get_zblade_dir(project_path)
        .join("config")
        .join("settings.json")
}

/// Initialize .zblade directory structure if it doesn't exist
pub fn init_zblade_dir(project_path: &Path) -> Result<(), String> {
    let zblade_dir = get_zblade_dir(project_path);

    // Create directory structure
    let dirs = [
        zblade_dir.join("config"),
        zblade_dir.join("artifacts").join("conversations"),
        zblade_dir.join("artifacts").join("moments"),
        zblade_dir.join("index"),
        zblade_dir.join("cache"),
    ];

    for dir in &dirs {
        fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create directory {:?}: {}", dir, e))?;
    }

    // Create .gitignore in .zblade if it doesn't exist
    let gitignore_path = zblade_dir.join(".gitignore");
    if !gitignore_path.exists() {
        let gitignore_content = r#"# ZaguanBlade local data
# Keep instructions.md tracked, ignore everything else
*
!.gitignore
!instructions.md
"#;
        fs::write(&gitignore_path, gitignore_content)
            .map_err(|e| format!("Failed to create .gitignore: {}", e))?;
    }

    // Create default instructions.md if it doesn't exist
    let instructions_path = zblade_dir.join("instructions.md");
    if !instructions_path.exists() {
        let instructions_content = r#"# Project Instructions

Add project-specific instructions for the AI assistant here.

## Project Overview

<!-- Describe your project briefly -->

## Coding Guidelines

<!-- Add any specific coding conventions or patterns to follow -->

## Important Files

<!-- List key files the AI should be aware of -->
"#;
        fs::write(&instructions_path, instructions_content)
            .map_err(|e| format!("Failed to create instructions.md: {}", e))?;
    }

    Ok(())
}

/// Load project settings from disk
/// Returns error if settings file doesn't exist (first-time setup needed)
pub fn load_project_settings(project_path: &Path) -> Result<ProjectSettings, String> {
    let settings_path = get_settings_path(project_path);

    if !settings_path.exists() {
        return Err("Settings file does not exist".to_string());
    }

    let content = fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))
}

/// Load project settings from disk, returning defaults if not found
/// Use this for internal code that needs settings but doesn't care if they exist
pub fn load_project_settings_or_default(project_path: &Path) -> ProjectSettings {
    load_project_settings(project_path).unwrap_or_default()
}

/// Save project settings to disk
pub fn save_project_settings(
    project_path: &Path,
    settings: &ProjectSettings,
) -> Result<(), String> {
    // Ensure .zblade directory exists
    init_zblade_dir(project_path)?;

    let settings_path = get_settings_path(project_path);

    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    fs::write(&settings_path, json).map_err(|e| format!("Failed to write settings: {}", e))?;

    eprintln!("[SETTINGS] Saved settings to {:?}", settings_path);
    Ok(())
}

/// Check if .zblade directory exists for a project
pub fn has_zblade_dir(project_path: &Path) -> bool {
    get_zblade_dir(project_path).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_settings() {
        let settings = ProjectSettings::default();
        assert_eq!(settings.storage.mode, StorageMode::Local);
        assert!(settings.storage.sync_metadata);
        assert!(settings.storage.cache.enabled);
        assert_eq!(settings.storage.cache.max_size_mb, 100);
        assert_eq!(settings.context.max_tokens, 8000);
        assert!(settings.context.compression.enabled);
        assert_eq!(settings.context.compression.model, CompressionModel::Remote);
        assert!(!settings.privacy.telemetry);
    }

    #[test]
    fn test_settings_serialization() {
        let settings = ProjectSettings::default();
        let json = serde_json::to_string_pretty(&settings).unwrap();
        let restored: ProjectSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.storage.mode, settings.storage.mode);
        assert_eq!(restored.context.max_tokens, settings.context.max_tokens);
    }

    #[test]
    fn test_init_zblade_dir() {
        let temp = tempdir().unwrap();
        let project_path = temp.path();

        init_zblade_dir(project_path).unwrap();

        assert!(get_zblade_dir(project_path).exists());
        assert!(get_zblade_dir(project_path).join("config").exists());
        assert!(get_zblade_dir(project_path)
            .join("artifacts")
            .join("conversations")
            .exists());
        assert!(get_zblade_dir(project_path).join("index").exists());
        assert!(get_zblade_dir(project_path).join(".gitignore").exists());
        assert!(get_zblade_dir(project_path)
            .join("instructions.md")
            .exists());
    }

    #[test]
    fn test_save_and_load_settings() {
        let temp = tempdir().unwrap();
        let project_path = temp.path();

        let mut settings = ProjectSettings::default();
        settings.storage.mode = StorageMode::Server;
        settings.context.max_tokens = 16000;

        save_project_settings(project_path, &settings).unwrap();

        let loaded = load_project_settings(project_path).unwrap();
        assert_eq!(loaded.storage.mode, StorageMode::Server);
        assert_eq!(loaded.context.max_tokens, 16000);
    }
}
