use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "default_blade_url")]
    pub blade_url: String,
    #[serde(default)]
    pub api_key: String,
    pub theme: String,
    pub markdown_view: String,
    #[serde(default)]
    pub selected_model: Option<String>,
}

fn default_blade_url() -> String {
    // Check environment variable first, then fall back to fidelity
    std::env::var("BLADE_URL").unwrap_or_else(|_| "http://10.0.0.1:8880".to_string())
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
