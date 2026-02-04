use crate::models::registry::ModelInfo;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Deserialize)]
struct OpenAIModelsResponse {
    data: Vec<OpenAIModel>,
}

#[derive(Deserialize)]
struct OpenAIModel {
    id: String,
    #[serde(default)]
    owned_by: Option<String>,
}

struct ModelCache {
    models: Vec<ModelInfo>,
    last_fetch: Instant,
}

lazy_static::lazy_static! {
    static ref MODEL_CACHE: Arc<Mutex<Option<ModelCache>>> = Arc::new(Mutex::new(None));
    static ref FETCH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::new(());
}

async fn fetch_models_from_server(
    server_url: &str,
) -> Result<Vec<ModelInfo>, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}/v1/models", server_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    // CRITICAL: No API key - these are keyless local servers
    let response = client.get(&url).send().await?;
    let response_text = response.text().await?;

    let models_response: OpenAIModelsResponse = match serde_json::from_str(&response_text) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("[OPENAI-COMPAT MODELS] Failed to deserialize response: {}", e);
            eprintln!(
                "[OPENAI-COMPAT MODELS] Raw response preview: {:.1000}",
                response_text
            );
            return Err(Box::new(e));
        }
    };

    let models = models_response
        .data
        .into_iter()
        .map(|m| {
            let description = m
                .owned_by
                .map(|owner| format!("OpenAI-compatible ({})", owner))
                .unwrap_or_else(|| "OpenAI-compatible".to_string());

            ModelInfo {
                id: format!("openai-compat/{}", m.id),
                name: m.id,
                description,
                provider: Some("openai-compat".to_string()),
                reasoning_effort: None,
                api_id: None,
            }
        })
        .collect();

    Ok(models)
}

pub async fn get_models(server_url: &str) -> Vec<ModelInfo> {
    if let Ok(cache) = MODEL_CACHE.lock() {
        if let Some(ref cached) = *cache {
            if cached.last_fetch.elapsed() < CACHE_TTL {
                return cached.models.clone();
            }
        }
    }

    let _lock = FETCH_LOCK.lock().await;

    if let Ok(cache) = MODEL_CACHE.lock() {
        if let Some(ref cached) = *cache {
            if cached.last_fetch.elapsed() < CACHE_TTL {
                return cached.models.clone();
            }
        }
    }

    let mut retry_count = 0;
    let max_retries = 2;

    loop {
        match fetch_models_from_server(server_url).await {
            Ok(models) => {
                if let Ok(mut cache) = MODEL_CACHE.lock() {
                    *cache = Some(ModelCache {
                        models: models.clone(),
                        last_fetch: Instant::now(),
                    });
                    eprintln!(
                        "[OPENAI-COMPAT MODELS] Successfully fetched {} models from {}",
                        models.len(),
                        server_url
                    );
                }
                return models;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count > max_retries {
                    eprintln!(
                        "[OPENAI-COMPAT MODELS] Failed to fetch models from {} after {} retries: {}",
                        server_url, max_retries, e
                    );
                    break;
                }

                let delay = Duration::from_millis(500 * (1 << (retry_count - 1)));
                eprintln!(
                    "[OPENAI-COMPAT MODELS] Fetch failed ({}): {}. Retrying in {:?}...",
                    retry_count, e, delay
                );
                tokio::time::sleep(delay).await;
            }
        }
    }

    if let Ok(cache) = MODEL_CACHE.lock() {
        if let Some(ref cached) = *cache {
            eprintln!("[OPENAI-COMPAT MODELS] Using EXPIRED cache as fallback");
            return cached.models.clone();
        }
    }

    Vec::new()
}

pub async fn test_connection(server_url: &str) -> Result<(), String> {
    let url = format!("{}/v1/models", server_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    // CRITICAL: No API key - these are keyless local servers
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to OpenAI-compatible server: {}", e))?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        Err(format!("Server returned {}: {}", status, text))
    }
}

pub fn clear_cache() {
    if let Ok(mut cache) = MODEL_CACHE.lock() {
        *cache = None;
    }
}
