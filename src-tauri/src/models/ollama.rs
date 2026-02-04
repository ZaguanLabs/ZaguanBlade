use crate::models::registry::ModelInfo;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelTag>,
}

#[derive(Deserialize)]
struct OllamaModelTag {
    name: String,
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
    ollama_url: &str,
) -> Result<Vec<ModelInfo>, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}/api/tags", ollama_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let response = client.get(&url).send().await?;
    let response_text = response.text().await?;

    let tags_response: OllamaTagsResponse = match serde_json::from_str(&response_text) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("[OLLAMA MODELS] Failed to deserialize response: {}", e);
            eprintln!(
                "[OLLAMA MODELS] Raw response preview: {:.1000}",
                response_text
            );
            return Err(Box::new(e));
        }
    };

    let models = tags_response
        .models
        .into_iter()
        .map(|m| ModelInfo {
            id: format!("ollama/{}", m.name),
            name: m.name,
            description: "Ollama".to_string(),
            provider: Some("ollama".to_string()),
            reasoning_effort: None,
            api_id: None,
        })
        .collect();

    Ok(models)
}

pub async fn get_models(ollama_url: &str) -> Vec<ModelInfo> {
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
        match fetch_models_from_server(ollama_url).await {
            Ok(models) => {
                if let Ok(mut cache) = MODEL_CACHE.lock() {
                    *cache = Some(ModelCache {
                        models: models.clone(),
                        last_fetch: Instant::now(),
                    });
                    eprintln!(
                        "[OLLAMA MODELS] Successfully fetched {} models from {}",
                        models.len(),
                        ollama_url
                    );
                }
                return models;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count > max_retries {
                    eprintln!(
                        "[OLLAMA MODELS] Failed to fetch models from {} after {} retries: {}",
                        ollama_url, max_retries, e
                    );
                    break;
                }

                let delay = Duration::from_millis(500 * (1 << (retry_count - 1)));
                eprintln!(
                    "[OLLAMA MODELS] Fetch failed ({}): {}. Retrying in {:?}...",
                    retry_count, e, delay
                );
                tokio::time::sleep(delay).await;
            }
        }
    }

    if let Ok(cache) = MODEL_CACHE.lock() {
        if let Some(ref cached) = *cache {
            eprintln!("[OLLAMA MODELS] Using EXPIRED cache as fallback");
            return cached.models.clone();
        }
    }

    Vec::new()
}

pub async fn test_connection(ollama_url: &str) -> Result<(), String> {
    let url = format!("{}/api/tags", ollama_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to Ollama: {}", e))?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        Err(format!("Ollama returned {}: {}", status, text))
    }
}

pub fn clear_cache() {
    if let Ok(mut cache) = MODEL_CACHE.lock() {
        *cache = None;
    }
}
