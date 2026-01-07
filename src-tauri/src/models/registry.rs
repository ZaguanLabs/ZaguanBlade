use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ZCODERD_URL: &str = "http://10.0.0.1:8880";
const CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_id: Option<String>,
}

#[derive(Deserialize)]
struct BladeModelsResponse {
    models: Vec<BladeModel>,
}

#[derive(Deserialize)]
struct BladeModel {
    id: String,
    name: String,
    description: String,
    #[serde(default)]
    reasoning_effort: Option<String>,
    // Ignore extra fields from zcoderd that we don't need
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
    #[serde(default)]
    context_window: Option<u32>,
    #[serde(default)]
    supports_streaming: Option<bool>,
    #[serde(default)]
    supports_reasoning_effort: Option<bool>,
    #[serde(default)]
    prompt_template: Option<String>,
}

struct ModelCache {
    models: Vec<ModelInfo>,
    last_fetch: Instant,
}

lazy_static::lazy_static! {
    static ref MODEL_CACHE: Arc<Mutex<Option<ModelCache>>> = Arc::new(Mutex::new(None));
}

async fn fetch_models_from_server() -> Result<Vec<ModelInfo>, Box<dyn std::error::Error>> {
    let url = format!("{}/v1/blade/models", ZCODERD_URL);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let response = client.get(&url).send().await?;
    let blade_response: BladeModelsResponse = response.json().await?;

    let models = blade_response
        .models
        .into_iter()
        .map(|m| {
            // Generate unique ID for models with reasoning effort
            let (id, api_id) = if let Some(ref effort) = m.reasoning_effort {
                let unique_id = format!("{}-{}", m.id, effort);
                (unique_id, Some(m.id.clone()))
            } else {
                (m.id.clone(), None)
            };

            ModelInfo {
                id,
                name: m.name,
                description: m.description,
                reasoning_effort: m.reasoning_effort,
                api_id,
            }
        })
        .collect();

    Ok(models)
}

pub async fn get_models() -> Vec<ModelInfo> {
    // Try to use cached models if still valid
    if let Ok(cache) = MODEL_CACHE.lock() {
        if let Some(ref cached) = *cache {
            if cached.last_fetch.elapsed() < CACHE_TTL {
                return cached.models.clone();
            }
        }
    }

    // Cache expired or missing - fetch from server
    match fetch_models_from_server().await {
        Ok(models) => {
            if let Ok(mut cache) = MODEL_CACHE.lock() {
                *cache = Some(ModelCache {
                    models: models.clone(),
                    last_fetch: Instant::now(),
                });
                eprintln!(
                    "[MODEL REGISTRY] Successfully fetched {} models from zcoderd",
                    models.len()
                );
            }
            models
        }
        Err(e) => {
            eprintln!(
                "[MODEL REGISTRY] Failed to fetch models from zcoderd: {}",
                e
            );
            // Return empty list on failure - let the UI handle it or retry
            Vec::new()
        }
    }
}
