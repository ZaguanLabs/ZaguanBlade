use lazy_static::lazy_static;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::RwLock;

pub const PRIMARY_BLADE_URL: &str = "https://coder-api.zblade.dev";
pub const BACKUP_BLADE_URL: &str = "https://coder-direct.zblade.dev";
const LEGACY_BLADE_URL: &str = "https://coder.zaguanai.com";
const PRIMARY_PROBE_INTERVAL: Duration = Duration::from_secs(30);
const PRIMARY_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BladeEndpointRole {
    Primary,
    Backup,
    Custom,
}

#[derive(Debug, Clone)]
pub struct BladeEndpoint {
    pub base_url: String,
    pub role: BladeEndpointRole,
}

lazy_static! {
    static ref PREFERRED_MANAGED_ROLE: RwLock<BladeEndpointRole> =
        RwLock::new(BladeEndpointRole::Primary);
}

static PRIMARY_MONITOR_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn default_blade_url() -> String {
    std::env::var("BLADE_URL").unwrap_or_else(|_| PRIMARY_BLADE_URL.to_string())
}

pub fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

pub fn is_managed_blade_url(url: &str) -> bool {
    matches!(
        normalize_base_url(url).as_str(),
        PRIMARY_BLADE_URL | BACKUP_BLADE_URL | LEGACY_BLADE_URL
    )
}

pub async fn connection_candidates(configured_url: &str) -> Vec<BladeEndpoint> {
    let normalized = normalize_base_url(configured_url);
    if !normalized.is_empty() && !is_managed_blade_url(&normalized) {
        return vec![BladeEndpoint {
            base_url: normalized,
            role: BladeEndpointRole::Custom,
        }];
    }

    match *PREFERRED_MANAGED_ROLE.read().await {
        BladeEndpointRole::Backup => vec![
            BladeEndpoint {
                base_url: BACKUP_BLADE_URL.to_string(),
                role: BladeEndpointRole::Backup,
            },
            BladeEndpoint {
                base_url: PRIMARY_BLADE_URL.to_string(),
                role: BladeEndpointRole::Primary,
            },
        ],
        _ => vec![
            BladeEndpoint {
                base_url: PRIMARY_BLADE_URL.to_string(),
                role: BladeEndpointRole::Primary,
            },
            BladeEndpoint {
                base_url: BACKUP_BLADE_URL.to_string(),
                role: BladeEndpointRole::Backup,
            },
        ],
    }
}

pub async fn preferred_managed_role() -> BladeEndpointRole {
    *PREFERRED_MANAGED_ROLE.read().await
}

pub async fn mark_success(endpoint: &BladeEndpoint) {
    if endpoint.role == BladeEndpointRole::Primary {
        let mut preferred = PREFERRED_MANAGED_ROLE.write().await;
        *preferred = BladeEndpointRole::Primary;
    }
}

pub async fn mark_failure(endpoint: &BladeEndpoint, api_key: &str) {
    if endpoint.role != BladeEndpointRole::Primary {
        return;
    }

    {
        let mut preferred = PREFERRED_MANAGED_ROLE.write().await;
        *preferred = BladeEndpointRole::Backup;
    }

    start_primary_monitor(api_key.to_string());
}

pub fn start_primary_monitor(api_key: String) {
    if PRIMARY_MONITOR_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(PRIMARY_PROBE_INTERVAL).await;
            if primary_is_reachable(&api_key).await {
                let mut preferred = PREFERRED_MANAGED_ROLE.write().await;
                *preferred = BladeEndpointRole::Primary;
                PRIMARY_MONITOR_RUNNING.store(false, Ordering::SeqCst);
                eprintln!("[BLADE ENDPOINT] Primary endpoint is reachable again");
                return;
            }
        }
    });
}

pub fn to_websocket_url(base_url: &str) -> String {
    normalize_base_url(base_url)
        .replace("http://", "ws://")
        .replace("https://", "wss://")
}

async fn primary_is_reachable(api_key: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(PRIMARY_PROBE_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            eprintln!(
                "[BLADE ENDPOINT] Failed to create primary probe client: {}",
                error
            );
            return false;
        }
    };

    let mut request = client.get(format!("{}/v1/blade/models", PRIMARY_BLADE_URL));
    if !api_key.trim().is_empty() {
        request = request.header("Authorization", format!("Bearer {}", api_key.trim()));
    }

    match request.send().await {
        Ok(response) => !response.status().is_server_error(),
        Err(_) => false,
    }
}
