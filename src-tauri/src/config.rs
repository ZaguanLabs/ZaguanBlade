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
    pub openai_compat_enabled: bool,
    #[serde(default = "default_openai_compat_url")]
    pub openai_compat_url: String,
    pub theme: String,
    pub markdown_view: String,
}

fn default_blade_url() -> String {
    // Check environment variable first, then fall back to fidelity
    std::env::var("BLADE_URL").unwrap_or_else(|_| "https://coder.zaguanai.com".to_string())
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
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

pub fn read_prompt_for_model(model_name: &str) -> Result<Option<String>, String> {
    let filename = format!("{}.md", model_name);
    let path = global_prompts_dir().join(filename);
    if !path.exists() {
        return Ok(None);
    }
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
