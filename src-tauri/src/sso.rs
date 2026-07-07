use reqwest::header::RETRY_AFTER;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DEFAULT_WEBSITE_URL: &str = "https://zaguanai.com";

#[derive(Clone, Debug, Serialize)]
pub struct SsoProgressEvent {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceStartResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PollResponse {
    pub status: String,
    pub api_key: Option<String>,
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub tier: Option<String>,
    #[allow(dead_code)]
    pub key_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SsoLoginResult {
    pub user_id: String,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ApprovedLogin {
    pub api_key: String,
    pub user_id: String,
    pub email: String,
    pub tier: Option<String>,
}

#[derive(Debug)]
pub enum PollOutcome {
    RateLimited(Duration),
    Status(PollResponse),
}

#[derive(Serialize)]
struct DeviceStartRequest {
    device_name: String,
    platform: String,
    app_version: String,
}

#[derive(Serialize)]
struct DevicePollRequest<'a> {
    device_code: &'a str,
}

pub fn website_base_url() -> String {
    for var in ["ZAGUAN_WEBSITE_URL", "BLADE_WEBSITE_URL"] {
        if let Ok(value) = std::env::var(var) {
            let normalized = normalize_base_url(&value);
            if !normalized.is_empty() {
                return normalized;
            }
        }
    }
    DEFAULT_WEBSITE_URL.to_string()
}

pub async fn start_device_auth(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<DeviceStartResponse, String> {
    let request = DeviceStartRequest {
        device_name: device_name(),
        platform: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let url = format!("{}/api/blade/auth/start", normalize_base_url(base_url));
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|error| format!("Failed to start sign-in: {error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Sign-in start failed ({status}): {body}"));
    }

    response
        .json::<DeviceStartResponse>()
        .await
        .map_err(|error| format!("Failed to read sign-in response: {error}"))
}

pub async fn poll_device_auth(
    client: &reqwest::Client,
    base_url: &str,
    device_code: &str,
) -> Result<PollOutcome, String> {
    let url = format!("{}/api/blade/auth/poll", normalize_base_url(base_url));
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&DevicePollRequest { device_code })
        .send()
        .await
        .map_err(|error| format!("Failed to poll sign-in: {error}"))?;

    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        return Ok(PollOutcome::RateLimited(retry_after(&response)));
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Sign-in poll failed ({status}): {body}"));
    }

    let payload = response
        .json::<PollResponse>()
        .await
        .map_err(|error| format!("Failed to read sign-in poll response: {error}"))?;
    Ok(PollOutcome::Status(payload))
}

pub fn approved_login(response: PollResponse) -> Result<ApprovedLogin, String> {
    let api_key = response
        .api_key
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Approved sign-in did not include an API key.".to_string())?;
    let user_id = response
        .user_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Approved sign-in did not include a user ID.".to_string())?;
    let email = response
        .email
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Approved sign-in did not include an email address.".to_string())?;

    Ok(ApprovedLogin {
        api_key,
        user_id,
        email,
        tier: response.tier.filter(|value| !value.trim().is_empty()),
    })
}

fn retry_after(response: &reqwest::Response) -> Duration {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(5))
}

fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("ZaguanBlade {}", std::env::consts::OS))
}
