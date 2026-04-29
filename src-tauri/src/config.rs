use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct ApiConfig {
    #[serde(default = "default_blade_url")]
    pub blade_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub ollama_enabled: bool,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    #[serde(default)]
    pub ollama_cloud_enabled: bool,
    #[serde(default)]
    pub ollama_cloud_api_key: String,
    #[serde(default)]
    pub openai_compat_enabled: bool,
    #[serde(default = "default_openai_compat_url")]
    pub openai_compat_url: String,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub markdown_view: String,
    #[serde(default)]
    pub language: String,
}

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct RemoteAiConfig {
    #[serde(default = "default_blade_url")]
    pub blade_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub markdown_view: String,
    #[serde(default)]
    pub language: String,
}

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct LocalAiConfig {
    #[serde(default)]
    pub ollama_enabled: bool,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    #[serde(default)]
    pub ollama_cloud_enabled: bool,
    #[serde(default)]
    pub ollama_cloud_api_key: String,
    #[serde(default)]
    pub openai_compat_enabled: bool,
    #[serde(default = "default_openai_compat_url")]
    pub openai_compat_url: String,
}

impl ApiConfig {
    pub fn remote_config(&self) -> RemoteAiConfig {
        RemoteAiConfig {
            blade_url: self.blade_url.clone(),
            api_key: self.api_key.clone(),
            user_id: self.user_id.clone(),
            theme: self.theme.clone(),
            markdown_view: self.markdown_view.clone(),
            language: self.language.clone(),
        }
    }

    pub fn local_ai_config(&self) -> LocalAiConfig {
        LocalAiConfig {
            ollama_enabled: self.ollama_enabled,
            ollama_url: self.ollama_url.clone(),
            ollama_cloud_enabled: self.ollama_cloud_enabled,
            ollama_cloud_api_key: self.ollama_cloud_api_key.clone(),
            openai_compat_enabled: self.openai_compat_enabled,
            openai_compat_url: self.openai_compat_url.clone(),
        }
    }

    pub fn apply_remote_config(&mut self, remote: &RemoteAiConfig) {
        self.blade_url = remote.blade_url.clone();
        self.api_key = remote.api_key.clone();
        self.user_id = remote.user_id.clone();
        self.theme = remote.theme.clone();
        self.markdown_view = remote.markdown_view.clone();
        self.language = remote.language.clone();
    }

    pub fn apply_local_ai_config(&mut self, local: &LocalAiConfig) {
        self.ollama_enabled = local.ollama_enabled;
        self.ollama_url = local.ollama_url.clone();
        self.ollama_cloud_enabled = local.ollama_cloud_enabled;
        self.ollama_cloud_api_key = local.ollama_cloud_api_key.clone();
        self.openai_compat_enabled = local.openai_compat_enabled;
        self.openai_compat_url = normalize_openai_compat_url(&local.openai_compat_url);
    }
}

fn default_blade_url() -> String {
    // Check environment variable first, then fall back to fidelity
    std::env::var("BLADE_URL").unwrap_or_else(|_| "https://coder.zaguanai.com".to_string())
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

pub fn normalize_openai_compat_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/v1")
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_string()
}

fn default_openai_compat_url() -> String {
    // Use base URL (no version); callers append /v1 paths
    "http://localhost:8080".to_string()
}

pub fn default_global_config_dir() -> PathBuf {
    let Some(dirs) = ProjectDirs::from("com", "zaguan", "zblade") else {
        return Path::new(".").to_path_buf();
    };
    dirs.config_dir().to_path_buf()
}

pub fn default_api_config_path() -> PathBuf {
    default_global_config_dir().join("api.json")
}

pub fn global_prompts_dir() -> PathBuf {
    default_global_config_dir().join("prompts")
}

pub fn ensure_global_prompts_dir() -> Result<(), String> {
    let dir = global_prompts_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())
}

fn prompt_model_name_candidates(model_name: &str) -> Vec<String> {
    let trimmed = model_name.trim();
    let stripped = trimmed
        .strip_prefix("ollama/")
        .or_else(|| trimmed.strip_prefix("openai-compat/"))
        .unwrap_or(trimmed);

    let mut candidates = Vec::new();
    for name in [trimmed, stripped] {
        if name.is_empty() {
            continue;
        }
        push_prompt_candidate(&mut candidates, name);
        if let Some((base, _tag)) = name.split_once(':') {
            push_prompt_candidate(&mut candidates, base);
        }
    }
    candidates
}

fn push_prompt_candidate(candidates: &mut Vec<String>, name: &str) {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return;
    }
    let lower = trimmed.to_ascii_lowercase();
    for candidate in [trimmed.to_string(), lower] {
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
}

fn resolve_prompt_path_for_model(
    prompts_dir: &Path,
    model_name: &str,
) -> Result<Option<PathBuf>, String> {
    let candidates = prompt_model_name_candidates(model_name);
    for candidate in &candidates {
        let path = prompts_dir.join(format!("{}.md", candidate));
        if path.exists() {
            return Ok(Some(path));
        }
    }

    let Ok(entries) = fs::read_dir(prompts_dir) else {
        return Ok(None);
    };
    let candidate_filenames = candidates
        .iter()
        .map(|candidate| format!("{}.md", candidate).to_ascii_lowercase())
        .collect::<Vec<_>>();

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read prompts directory: {}", e))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if candidate_filenames
            .iter()
            .any(|candidate| candidate == &file_name.to_ascii_lowercase())
        {
            return Ok(Some(entry.path()));
        }
    }

    Ok(None)
}

pub fn read_prompt_for_model(model_name: &str) -> Result<Option<String>, String> {
    let Some(path) = resolve_prompt_path_for_model(&global_prompts_dir(), model_name)? else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    eprintln!("[CONFIG] Loading local AI prompt: {}", path.display());
    fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| format!("Failed to read prompt file {}: {}", path.display(), e))
}

pub fn load_api_config(path: &Path) -> ApiConfig {
    let Ok(bytes) = fs::read(path) else {
        return ApiConfig::default();
    };
    serde_json::from_slice::<ApiConfig>(&bytes).unwrap_or_default()
}

pub fn save_api_config(path: &Path, cfg: &ApiConfig) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, json).map_err(|e| e.to_string())
}

pub fn load_remote_ai_config(path: &Path) -> RemoteAiConfig {
    load_api_config(path).remote_config()
}

pub fn save_remote_ai_config(path: &Path, remote: &RemoteAiConfig) -> Result<(), String> {
    let mut cfg = load_api_config(path);
    cfg.apply_remote_config(remote);
    save_api_config(path, &cfg)
}

pub fn load_local_ai_config(path: &Path) -> LocalAiConfig {
    load_api_config(path).local_ai_config()
}

pub fn save_local_ai_config(path: &Path, local: &LocalAiConfig) -> Result<(), String> {
    let mut cfg = load_api_config(path);
    cfg.apply_local_ai_config(local);
    save_api_config(path, &cfg)
}

/// Generate or get user_id from config
/// If user_id doesn't exist, generate one and save it
pub fn get_or_create_user_id(config_path: &Path) -> String {
    let mut config = load_api_config(config_path);

    let mut save_needed = false;

    // First, try to derive from API key if present (ps_live_ or ps_test_)
    if !config.api_key.is_empty() {
        if let Some(start_idx) = config
            .api_key
            .find("ps_live_")
            .or_else(|| config.api_key.find("ps_test_"))
        {
            let prefix_len = 8; // length of "ps_live_" or "ps_test_"
            let hash_start = start_idx + prefix_len;
            if config.api_key.len() >= hash_start + 8 {
                let suffix = &config.api_key[hash_start..hash_start + 8];
                let derived_id = format!("user_{}", suffix);

                // Only update if different
                if config.user_id != derived_id {
                    config.user_id = derived_id;
                    save_needed = true;
                    eprintln!("[CONFIG] Derived user_id from API key: {}", config.user_id);
                }
            }
        }
    }

    // Fallback: If user_id is still empty or invalid (and couldn't be derived), generate a new random one
    if config.user_id.trim().is_empty()
        || (!config.api_key.contains("ps_")
            && !config.user_id.starts_with("user_")
            && config.user_id.len() != 8)
    {
        // Generate a short random suffix using base62 encoding of UUID
        let uuid = uuid::Uuid::new_v4();
        let uuid_bytes = uuid.as_bytes();

        // Take first 6 bytes and encode as base62-like string
        let suffix: String = uuid_bytes[..6]
            .iter()
            .map(|&b| {
                let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
                chars[(b % 62) as usize] as char
            })
            .collect();

        config.user_id = format!("user_{}", suffix);
        save_needed = true;
        eprintln!("[CONFIG] Generated new random user_id: {}", config.user_id);
    }

    // Save the config if changed
    if save_needed {
        if let Err(e) = save_api_config(config_path, &config) {
            eprintln!("[CONFIG] Failed to save user_id: {}", e);
        }
    }

    config.user_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prompt_candidates_include_provider_stripped_and_lowercase_names() {
        let candidates = prompt_model_name_candidates("ollama/laguna-xs.2:Q4_K_M");

        assert!(candidates.contains(&"ollama/laguna-xs.2:Q4_K_M".to_string()));
        assert!(candidates.contains(&"ollama/laguna-xs.2:q4_k_m".to_string()));
        assert!(candidates.contains(&"laguna-xs.2:Q4_K_M".to_string()));
        assert!(candidates.contains(&"laguna-xs.2:q4_k_m".to_string()));
        assert!(candidates.contains(&"laguna-xs.2".to_string()));
    }

    #[test]
    fn resolve_prompt_path_matches_ollama_tag_case_insensitively() {
        let dir = tempdir().expect("tempdir");
        let prompt_path = dir.path().join("laguna-xs.2:q4_K_M.md");
        fs::write(&prompt_path, "Laguna prompt").expect("write prompt");

        let resolved = resolve_prompt_path_for_model(dir.path(), "ollama/laguna-xs.2:Q4_K_M")
            .expect("resolve prompt")
            .expect("prompt should resolve");

        assert_eq!(resolved, prompt_path);
    }

    #[test]
    fn resolve_prompt_path_falls_back_to_base_model_name() {
        let dir = tempdir().expect("tempdir");
        let prompt_path = dir.path().join("laguna-xs.2.md");
        fs::write(&prompt_path, "Base Laguna prompt").expect("write prompt");

        let resolved = resolve_prompt_path_for_model(dir.path(), "ollama/laguna-xs.2:Q4_K_M")
            .expect("resolve prompt")
            .expect("prompt should resolve");

        assert_eq!(resolved, prompt_path);
    }
}
