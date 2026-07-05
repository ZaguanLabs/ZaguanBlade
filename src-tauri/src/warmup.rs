use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Instant;

/// Warmup trigger types per Blade Protocol v2.1, extended with the editor
/// triggers from the context-prefetch contract (2026-07-05).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarmupTrigger {
    Launch,
    ModelChange,
    WorkspaceChange,
    SessionResume,
    ActiveFileChange,
    FileSave,
    GuidanceChange,
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
    /// DEPRECATED: superseded by the context bundle — provider cache strategy
    /// is zcoderd's call. Kept until the deployed zcoderd gate stops reading it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_strategy: Option<WarmupCacheStrategy>,
    /// DEPRECATED: see `cache_strategy`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_artifacts: Option<Vec<String>>,
    /// Context-prefetch bundle (workspace / editor_state / active_file_snapshot /
    /// repo_guidance / git_state / limits), flattened to top-level fields per
    /// the zcoderd contract. Absent when Blade lacks editor state to send.
    #[serde(flatten)]
    pub context: Option<crate::warmup_bundle::WarmupContextBundle>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarmupCacheStrategy {
    ExplicitMinimal,
    Opportunistic,
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

/// Rapid-fire sends of a byte-identical bundle inside this window are
/// suppressed locally. Kept short on purpose: provider prompt caches expire in
/// minutes, so an *identical* re-send after this window is still useful
/// re-warming, and zcoderd applies its own provider-side dedupe/throttling.
const BUNDLE_DEDUPE_WINDOW_SECS: u64 = 60;

/// Warmup client for proactive cache warming
pub struct WarmupClient {
    base_url: String,
    api_key: String,
    user_id: String,
    http_client: reqwest::Client,
    last_warmup: Mutex<Option<Instant>>,
    /// (key, sent-at) of the last bundle-carrying warmup, for dedupe.
    last_bundle: Mutex<Option<(u64, Instant)>>,
}

impl WarmupClient {
    pub fn new(base_url: String, api_key: String, user_id: String) -> Self {
        // Warmup requests should complete quickly (< 30s)
        // Use a timeout to prevent hanging
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            base_url,
            api_key,
            user_id,
            http_client,
            last_warmup: Mutex::new(None),
            last_bundle: Mutex::new(None),
        }
    }

    /// Send a warmup request to zcoderd
    /// This is non-blocking and failures are non-fatal
    pub async fn warmup(
        &self,
        session_id: &str,
        model: &str,
        trigger: WarmupTrigger,
        context: Option<crate::warmup_bundle::WarmupContextBundle>,
    ) -> Result<WarmupResponse, String> {
        let provider = detect_provider(model).to_lowercase();
        let skipped = |message: &str| WarmupResponse {
            response_type: "warmup_skipped".to_string(),
            session_id: session_id.to_string(),
            provider: provider.clone(),
            cache_supported: false,
            artifacts_loaded: 0,
            cache_ready: false,
            duration_ms: 0,
            message: Some(message.to_string()),
        };
        if is_local_provider(&provider) {
            return Ok(skipped("Skipped remote warmup for local model"));
        }

        // Editor triggers are debounced in the frontend, but a debounce can't
        // see that the resulting bundle is byte-identical — this can.
        let bundle_key = context
            .as_ref()
            .and_then(|bundle| serde_json::to_string(bundle).ok())
            .map(|json| bundle_dedupe_key(session_id, model, &json));
        if let Some(key) = bundle_key {
            if self.is_recent_duplicate_bundle(key) {
                return Ok(skipped("Skipped identical warmup bundle (recently sent)"));
            }
        }
        let (cache_strategy, preferred_artifacts) = if provider_requires_explicit_minimal(&provider)
        {
            (
                Some(WarmupCacheStrategy::ExplicitMinimal),
                Some(vec!["project_index_min.md".to_string()]),
            )
        } else if provider_prefers_opportunistic_cache(&provider) {
            (Some(WarmupCacheStrategy::Opportunistic), None)
        } else {
            (None, None)
        };

        let request = WarmupRequest {
            request_type: "warmup".to_string(),
            session_id: session_id.to_string(),
            user_id: self.user_id.clone(),
            model: model.to_string(),
            trigger,
            cache_strategy,
            preferred_artifacts,
            context,
        };

        let candidates = crate::blade_endpoint::connection_candidates(&self.base_url).await;
        let mut last_error = "no Blade endpoints configured".to_string();
        let mut data = None;

        for endpoint in candidates {
            let url = format!("{}/v1/blade/warmup", endpoint.base_url);
            let response = match self
                .http_client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&request)
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    last_error =
                        format!("Warmup request failed via {}: {}", endpoint.base_url, error);
                    crate::blade_endpoint::mark_failure(&endpoint, &self.api_key).await;
                    continue;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                last_error = format!(
                    "Warmup error via {}: {}: {}",
                    endpoint.base_url, status, text
                );
                if status.is_server_error() {
                    crate::blade_endpoint::mark_failure(&endpoint, &self.api_key).await;
                    continue;
                }
                return Err(last_error);
            }

            let parsed = response
                .json()
                .await
                .map_err(|e| format!("Failed to parse warmup response: {}", e))?;
            crate::blade_endpoint::mark_success(&endpoint).await;
            if endpoint.role == crate::blade_endpoint::BladeEndpointRole::Backup {
                crate::blade_endpoint::start_primary_monitor(self.api_key.clone());
            }
            data = Some(parsed);
            break;
        }

        let data = data.ok_or(last_error)?;

        // Track last warmup time
        *self.last_warmup.lock().unwrap() = Some(Instant::now());
        if let Some(key) = bundle_key {
            *self.last_bundle.lock().unwrap() = Some((key, Instant::now()));
        }

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

    fn is_recent_duplicate_bundle(&self, key: u64) -> bool {
        self.last_bundle
            .lock()
            .unwrap()
            .is_some_and(|(last_key, sent_at)| {
                last_key == key && sent_at.elapsed().as_secs() < BUNDLE_DEDUPE_WINDOW_SECS
            })
    }
}

/// Dedupe key over everything that makes a warmup distinct EXCEPT the trigger:
/// two triggers producing byte-identical context deserve one send.
fn bundle_dedupe_key(session_id: &str, model: &str, bundle_json: &str) -> u64 {
    let mut keyed = String::with_capacity(session_id.len() + model.len() + bundle_json.len() + 2);
    keyed.push_str(session_id);
    keyed.push('\0');
    keyed.push_str(model);
    keyed.push('\0');
    keyed.push_str(bundle_json);
    crate::stable_hash::stable_hash(keyed.as_bytes())
}

/// Extract provider from model string (e.g., "anthropic/claude-sonnet-4" -> "anthropic")
pub fn detect_provider(model: &str) -> &str {
    model.split('/').next().unwrap_or("unknown")
}

pub fn is_local_provider(provider: &str) -> bool {
    matches!(provider.to_lowercase().as_str(), "ollama" | "openai-compat")
}

/// Providers where zcoderd should explicitly warm only the compact deterministic index.
pub fn provider_requires_explicit_minimal(provider: &str) -> bool {
    matches!(
        provider.to_lowercase().as_str(),
        "anthropic" | "qwen" | "minimax"
    )
}

/// Providers that already optimize caching without forced artifact preload.
pub fn provider_prefers_opportunistic_cache(provider: &str) -> bool {
    matches!(
        provider.to_lowercase().as_str(),
        "openai" | "groq" | "novita"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> WarmupClient {
        WarmupClient::new(
            "http://localhost:0".to_string(),
            "test-key".to_string(),
            "user_test".to_string(),
        )
    }

    #[test]
    fn dedupe_key_varies_with_session_model_and_bundle() {
        let base = bundle_dedupe_key("sess_a", "anthropic/claude-opus-4-8", "{\"a\":1}");
        assert_eq!(
            base,
            bundle_dedupe_key("sess_a", "anthropic/claude-opus-4-8", "{\"a\":1}")
        );
        assert_ne!(
            base,
            bundle_dedupe_key("sess_b", "anthropic/claude-opus-4-8", "{\"a\":1}")
        );
        assert_ne!(base, bundle_dedupe_key("sess_a", "openai/gpt-5", "{\"a\":1}"));
        assert_ne!(
            base,
            bundle_dedupe_key("sess_a", "anthropic/claude-opus-4-8", "{\"a\":2}")
        );
    }

    #[test]
    fn duplicate_bundle_suppressed_only_inside_window() {
        let client = test_client();
        assert!(!client.is_recent_duplicate_bundle(42)); // nothing sent yet

        *client.last_bundle.lock().unwrap() = Some((42, Instant::now()));
        assert!(client.is_recent_duplicate_bundle(42)); // identical, fresh
        assert!(!client.is_recent_duplicate_bundle(43)); // different bundle

        // Identical bundle past the window must be re-sent (cache TTL re-warm).
        let stale = Instant::now() - std::time::Duration::from_secs(BUNDLE_DEDUPE_WINDOW_SECS + 1);
        *client.last_bundle.lock().unwrap() = Some((42, stale));
        assert!(!client.is_recent_duplicate_bundle(42));
    }

    #[test]
    fn request_serializes_bundle_fields_at_top_level() {
        let request = WarmupRequest {
            request_type: "warmup".to_string(),
            session_id: "sess_x".to_string(),
            user_id: "user_x".to_string(),
            model: "anthropic/claude-opus-4-8".to_string(),
            trigger: WarmupTrigger::ModelChange,
            cache_strategy: None,
            preferred_artifacts: None,
            context: None,
        };
        let json: serde_json::Value = serde_json::from_str(&serde_json::to_string(&request).unwrap()).unwrap();
        // No bundle: none of the flattened keys may appear.
        assert!(json.get("workspace").is_none());
        assert_eq!(json["type"], "warmup");
        assert_eq!(json["trigger"], "model_change");
    }
}

