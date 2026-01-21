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
    pub theme: String,
    pub markdown_view: String,
}

fn default_blade_url() -> String {
    // Check environment variable first, then fall back to fidelity
    std::env::var("BLADE_URL").unwrap_or_else(|_| "https://coder.zaguanai.com".to_string())
}

pub fn default_api_config_path() -> PathBuf {
    let Some(dirs) = ProjectDirs::from("com", "zaguan", "zblade") else {
        return Path::new("ideai-api.json").to_path_buf();
    };
    dirs.config_dir().join("api.json")
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

    // If user_id is empty or invalid, generate a new one
    if config.user_id.trim().is_empty() || !config.user_id.starts_with("user_") {
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
        eprintln!("[CONFIG] Generated new user_id: {}", config.user_id);

        // Save the config with the new user_id
        if let Err(e) = save_api_config(config_path, &config) {
            eprintln!("[CONFIG] Failed to save user_id: {}", e);
        }
    }

    config.user_id
}
