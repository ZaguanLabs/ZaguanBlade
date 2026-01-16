use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Instant;

/// Warmup trigger types per Blade Protocol v2.1
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarmupTrigger {
    Launch,
    ModelChange,
    WorkspaceChange,
    SessionResume,
}

/// Warmup request sent to zcoderd
#[derive(Debug, Serialize)]
pub struct WarmupRequest {
    #[serde(rename = "type")]
    pub request_type: String,
    pub session_id: String,
    pub user_id: String,
    pub model: String,
    pub trigger: WarmupTrigger,
}

/// Warmup response from zcoderd
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarmupResponse {
    #[serde(rename = "type")]
    pub response_type: String,
    pub session_id: String,
    pub provider: String,
    pub cache_supported: bool,
    pub artifacts_loaded: i32,
    pub cache_ready: bool,
    pub duration_ms: i64,
    pub message: Option<String>,
}

/// Warmup client for proactive cache warming
pub struct WarmupClient {
    base_url: String,
    api_key: String,
    http_client: reqwest::Client,
    last_warmup: Mutex<Option<Instant>>,
}

impl WarmupClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        // Warmup requests should complete quickly (< 30s)
        // Use a timeout to prevent hanging
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            base_url,
            api_key,
            http_client,
            last_warmup: Mutex::new(None),
        }
    }

    /// Send a warmup request to zcoderd
    /// This is non-blocking and failures are non-fatal
    pub async fn warmup(
        &self,
        session_id: &str,
        model: &str,
        trigger: WarmupTrigger,
    ) -> Result<WarmupResponse, String> {
        let request = WarmupRequest {
            request_type: "warmup".to_string(),
            session_id: session_id.to_string(),
            user_id: "default".to_string(),
            model: model.to_string(),
            trigger,
        };

        let url = format!("{}/v1/blade/warmup", self.base_url);

        eprintln!(
            "[WARMUP] Sending warmup request: session={}, model={}, trigger={:?}",
            session_id, model, request.trigger
        );

        let response = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Warmup request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("Warmup error {}: {}", status, text));
        }

        let data: WarmupResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse warmup response: {}", e))?;

        eprintln!(
            "[WARMUP] Response: type={}, provider={}, artifacts={}, ready={}, duration={}ms",
            data.response_type,
            data.provider,
            data.artifacts_loaded,
            data.cache_ready,
            data.duration_ms
        );

        // Track last warmup time
        *self.last_warmup.lock().unwrap() = Some(Instant::now());

        Ok(data)
    }

    /// Check if we should rewarm based on inactivity
    /// Returns true if more than 5 minutes have passed since last warmup
    pub fn should_rewarm(&self) -> bool {
        let last = self.last_warmup.lock().unwrap();
        match *last {
            Some(instant) => instant.elapsed().as_secs() > 300, // 5 minutes
            None => true,
        }
    }
}

/// Extract provider from model string (e.g., "anthropic/claude-sonnet-4" -> "anthropic")
pub fn detect_provider(model: &str) -> &str {
    model.split('/').next().unwrap_or("unknown")
}

/// Check if a provider supports prompt caching
#[allow(dead_code)]
pub fn provider_supports_cache(provider: &str) -> bool {
    matches!(provider.to_lowercase().as_str(), "anthropic" | "openai")
}

