use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
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
#[allow(dead_code)]
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
    static ref FETCH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::new(());
}

async fn fetch_models_from_server(
    blade_url: &str,
    api_key: &str,
) -> Result<Vec<ModelInfo>, Box<dyn std::error::Error + Send + Sync>> {
    let candidates = crate::blade_endpoint::connection_candidates(blade_url).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let mut last_error = "no Blade endpoints configured".to_string();

    for endpoint in candidates {
        match fetch_models_from_endpoint(&client, &endpoint, api_key).await {
            Ok(models) => {
                crate::blade_endpoint::mark_success(&endpoint).await;
                if endpoint.role == crate::blade_endpoint::BladeEndpointRole::Backup {
                    crate::blade_endpoint::start_primary_monitor(api_key.to_string());
                }
                return Ok(models);
            }
            Err(error) => {
                last_error = error.to_string();
                if error.should_failover {
                    crate::blade_endpoint::mark_failure(&endpoint, api_key).await;
                    continue;
                }
                return Err(error.source);
            }
        }
    }

    Err(boxed_error(last_error))
}

struct EndpointFetchError {
    source: Box<dyn std::error::Error + Send + Sync>,
    should_failover: bool,
}

impl std::fmt::Display for EndpointFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(f)
    }
}

fn boxed_error(message: String) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(std::io::Error::other(message))
}

async fn fetch_models_from_endpoint(
    client: &reqwest::Client,
    endpoint: &crate::blade_endpoint::BladeEndpoint,
    api_key: &str,
) -> Result<Vec<ModelInfo>, EndpointFetchError> {
    let url = format!("{}/v1/blade/models", endpoint.base_url);

    let mut request = client.get(&url);
    if !api_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = request.send().await.map_err(|error| EndpointFetchError {
        should_failover: true,
        source: Box::new(error),
    })?;

    let status = response.status();
    let response_text = response.text().await.map_err(|error| EndpointFetchError {
        should_failover: true,
        source: Box::new(error),
    })?;
    if !status.is_success() {
        return Err(EndpointFetchError {
            should_failover: status.is_server_error(),
            source: boxed_error(format!(
                "Blade model registry error {}: {}",
                status, response_text
            )),
        });
    }

    // Try to deserialize
    let blade_response: BladeModelsResponse = match serde_json::from_str(&response_text) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("[MODEL REGISTRY] Failed to deserialize response: {}", e);
            eprintln!(
                "[MODEL REGISTRY] Raw response preview: {:.1000}",
                response_text
            );
            return Err(EndpointFetchError {
                source: Box::new(e),
                should_failover: false,
            });
        }
    };

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
                provider: Some("zaguan".to_string()),
                reasoning_effort: m.reasoning_effort,
                api_id,
            }
        })
        .collect();

    Ok(models)
}

pub async fn get_models(blade_url: &str, api_key: &str) -> Vec<ModelInfo> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        if let Ok(mut cache) = MODEL_CACHE.lock() {
            *cache = None;
        }
        return Vec::new();
    }

    // 1. Fast path: Check cache first
    if let Ok(cache) = MODEL_CACHE.lock() {
        if let Some(ref cached) = *cache {
            if cached.last_fetch.elapsed() < CACHE_TTL {
                return cached.models.clone();
            }
        }
    }

    // 2. Cache expired or missing - Acquire lock to coordinate fetching
    let _lock = FETCH_LOCK.lock().await;

    // 3. Double-check cache after acquiring lock (in case another thread just finished fetching)
    if let Ok(cache) = MODEL_CACHE.lock() {
        if let Some(ref cached) = *cache {
            if cached.last_fetch.elapsed() < CACHE_TTL {
                return cached.models.clone();
            }
        }
    }

    // 4. Truly need to fetch
    let mut retry_count = 0;
    let max_retries = 3;

    loop {
        match fetch_models_from_server(blade_url, api_key).await {
            Ok(models) => {
                if let Ok(mut cache) = MODEL_CACHE.lock() {
                    *cache = Some(ModelCache {
                        models: models.clone(),
                        last_fetch: Instant::now(),
                    });
                    eprintln!(
                        "[MODEL REGISTRY] Successfully fetched {} models from {}",
                        models.len(),
                        blade_url
                    );
                }
                return models;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count > max_retries {
                    eprintln!(
                        "[MODEL REGISTRY] Failed to fetch models from {} after {} retries: {}",
                        blade_url, max_retries, e
                    );
                    break;
                }

                let delay = Duration::from_millis(500 * (1 << (retry_count - 1)));
                eprintln!(
                    "[MODEL REGISTRY] Fetch failed ({}): {}. Retrying in {:?}...",
                    retry_count, e, delay
                );
                tokio::time::sleep(delay).await;
            }
        }
    }

    // 5. Fallback: If fetch failed but we have expired cache, use it anyway
    if let Ok(cache) = MODEL_CACHE.lock() {
        if let Some(ref cached) = *cache {
            eprintln!("[MODEL REGISTRY] Using EXPIRED cache as fallback");
            return cached.models.clone();
        }
    }

    // 6. Final fallback: empty list
    Vec::new()
}
