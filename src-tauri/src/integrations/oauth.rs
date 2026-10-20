//! OAuth sign-in for Streamable HTTP MCP servers: authorization-server
//! discovery, PKCE with a loopback redirect, keyring persistence, serialized
//! refresh and local sign-out. rmcp drives the protocol; Blade owns every
//! network request, the redirect listener and storage.
//!
//! Tokens are bound to the MCP server URL they were issued for and to the
//! authorization server metadata found at sign-in, so a changed URL never
//! receives them and refreshes always go to the issuing server.
use super::{config::OAuthSettings, credentials::SecretStore, RuntimeError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures_util::StreamExt;
use rmcp::transport::{
    auth::{AuthorizationMetadata, AuthorizationMetadataSource},
    AuthError, AuthorizationManager, AuthorizationRequest, AuthorizationSession,
    CredentialRefreshGuard, CredentialStore, OAuthHttpClient, OAuthHttpClientFuture,
    OAuthHttpRequest, StoredCredentials,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY_BYTES: usize = 1024 * 1024;
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CALLBACK_BYTES: usize = 8 * 1024;
/// Windows Credential Manager holds at most 2560 bytes per entry (UTF-16), so
/// stored state is base64 split across entries well below that.
const CHUNK_CHARS: usize = 1000;
const MAX_CHUNKS: usize = 48;
const MANIFEST: &str = "oauth";

/// Everything persisted for one integration.
#[derive(Serialize, Deserialize)]
struct Persisted {
    /// The MCP server URL the tokens were issued for.
    resource: String,
    metadata: AuthorizationMetadata,
    credentials: StoredCredentials,
}

fn map(error: AuthError) -> RuntimeError {
    match error {
        AuthError::AuthorizationRequired | AuthError::TokenRefreshRejected(_) => {
            RuntimeError::AuthenticationRequired
        }
        AuthError::NoAuthorizationSupport
        | AuthError::MetadataError(_)
        | AuthError::PkceUnsupported => RuntimeError::OAuthUnsupported,
        AuthError::CredentialStoreError(_) => RuntimeError::SecretUnavailable,
        AuthError::HttpError(_) | AuthError::TokenRefreshFailed(_) => {
            RuntimeError::ServerUnreachable
        }
        _ => RuntimeError::AuthorizationFailed,
    }
}

/// HTTPS, or plain HTTP to this machine only.
fn protected(url: &reqwest::Url) -> bool {
    url.scheme() == "https" || (url.scheme() == "http" && super::http::loopback(url))
}

// ---------------------------------------------------------------------------
// Network

/// Every OAuth request rmcp makes goes through here. Redirects are never
/// followed, including for client registration: a redirected POST would carry
/// its body to a destination nobody approved.
struct BladeOAuthHttp {
    client: reqwest::Client,
}

impl BladeOAuthHttp {
    fn new() -> Result<Self, RuntimeError> {
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(HTTP_TIMEOUT)
                .connect_timeout(Duration::from_secs(10))
                .pool_max_idle_per_host(0)
                .user_agent(concat!("ZaguanBlade/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|_| RuntimeError::LaunchFailed)?,
        })
    }

    /// The server's `WWW-Authenticate` challenge, which may name its protected
    /// resource metadata. Nothing is sent but the request line and headers.
    async fn challenge(&self, url: &str) -> Option<String> {
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await
            .ok()?;
        (response.status() == reqwest::StatusCode::UNAUTHORIZED)
            .then(|| {
                response
                    .headers()
                    .get(reqwest::header::WWW_AUTHENTICATE)?
                    .to_str()
                    .ok()
                    .map(str::to_owned)
            })
            .flatten()
    }
}

impl OAuthHttpClient for BladeOAuthHttp {
    fn execute(&self, request: OAuthHttpRequest) -> OAuthHttpClientFuture<'_> {
        Box::pin(async move {
            let request = reqwest::Request::try_from(request.request)?;
            if !protected(request.url()) {
                return Err("OAuth endpoints must use HTTPS".into());
            }
            let response = self.client.execute(request).await?;
            let mut builder = oauth2::http::Response::builder()
                .status(response.status())
                .version(response.version());
            for (name, value) in response.headers() {
                builder = builder.header(name, value);
            }
            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                if chunk.len() > MAX_BODY_BYTES - body.len() {
                    return Err("OAuth response body too large".into());
                }
                body.extend_from_slice(&chunk);
            }
            Ok(builder.body(body)?)
        })
    }
}

// ---------------------------------------------------------------------------
// Storage

fn read(secrets: &dyn SecretStore, integration: Uuid) -> Result<Option<Persisted>, RuntimeError> {
    let Some((generation, count)) = manifest(secrets, integration)? else {
        return Ok(None);
    };
    let mut encoded = String::new();
    for index in 0..count {
        // A missing chunk means a torn write; treat as signed out.
        let Some(chunk) =
            secrets.get_internal(integration, &format!("{MANIFEST}.{generation}.{index}"))?
        else {
            return Ok(None);
        };
        encoded.push_str(&chunk);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| RuntimeError::SecretUnavailable)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| RuntimeError::SecretUnavailable)
}

fn manifest(
    secrets: &dyn SecretStore,
    integration: Uuid,
) -> Result<Option<(String, usize)>, RuntimeError> {
    let Some(manifest) = secrets.get_internal(integration, MANIFEST)? else {
        return Ok(None);
    };
    let mut parts = manifest.split(' ');
    match (
        parts.next(),
        parts.next(),
        parts.next().and_then(|count| count.parse().ok()),
        parts.next(),
    ) {
        (Some("v1"), Some(generation), Some(count), None)
            if count <= MAX_CHUNKS
                && generation.len() == 8
                && generation.bytes().all(|c| c.is_ascii_hexdigit()) =>
        {
            Ok(Some((generation.to_owned(), count)))
        }
        _ => Err(RuntimeError::SecretUnavailable),
    }
}

/// New chunks are written under a fresh generation before the manifest
/// switches to them, so a failed write never leaves a mixed credential.
fn write(
    secrets: &dyn SecretStore,
    integration: Uuid,
    persisted: &Persisted,
) -> Result<(), RuntimeError> {
    let encoded = URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(persisted).map_err(|_| RuntimeError::SecretUnavailable)?);
    let chunks: Vec<_> = encoded.as_bytes().chunks(CHUNK_CHARS).collect();
    if chunks.len() > MAX_CHUNKS {
        return Err(RuntimeError::InvalidSecret);
    }
    let previous = manifest(secrets, integration).ok().flatten();
    let generation = Uuid::new_v4().simple().to_string()[..8].to_owned();
    for (index, chunk) in chunks.iter().enumerate() {
        // Base64 is ASCII, so every chunk is valid UTF-8.
        let chunk = std::str::from_utf8(chunk).map_err(|_| RuntimeError::SecretUnavailable)?;
        secrets.set_internal(
            integration,
            &format!("{MANIFEST}.{generation}.{index}"),
            chunk,
        )?;
    }
    secrets.set_internal(
        integration,
        MANIFEST,
        &format!("v1 {generation} {}", chunks.len()),
    )?;
    if let Some((old, count)) = previous {
        for index in 0..count {
            let _ = secrets.delete_internal(integration, &format!("{MANIFEST}.{old}.{index}"));
        }
    }
    Ok(())
}

fn erase(secrets: &dyn SecretStore, integration: Uuid) -> Result<(), RuntimeError> {
    let previous = manifest(secrets, integration).ok().flatten();
    secrets.delete_internal(integration, MANIFEST)?;
    if let Some((generation, count)) = previous {
        for index in 0..count {
            let _ =
                secrets.delete_internal(integration, &format!("{MANIFEST}.{generation}.{index}"));
        }
    }
    Ok(())
}

/// One refresh at a time per integration, across every connection to it.
static REFRESHING: LazyLock<Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(Default::default);

fn refresh_lock(integration: Uuid) -> Arc<tokio::sync::Mutex<()>> {
    REFRESHING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(integration)
        .or_default()
        .clone()
}

/// rmcp's view of the keyring for one integration and server URL.
struct KeyringCredentials {
    integration: Uuid,
    resource: String,
    metadata: AuthorizationMetadata,
    secrets: Arc<dyn SecretStore>,
}

impl KeyringCredentials {
    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&dyn SecretStore, Uuid) -> Result<T, RuntimeError> + Send + 'static,
    ) -> Result<T, AuthError> {
        let secrets = self.secrets.clone();
        let integration = self.integration;
        tokio::task::spawn_blocking(move || work(secrets.as_ref(), integration))
            .await
            .map_err(|_| AuthError::CredentialStoreError("unavailable".into()))?
            .map_err(|_| AuthError::CredentialStoreError("unavailable".into()))
    }
}

#[async_trait::async_trait]
impl CredentialStore for KeyringCredentials {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let resource = self.resource.clone();
        self.blocking(move |secrets, integration| {
            Ok(read(secrets, integration)?
                .filter(|persisted| persisted.resource == resource)
                .map(|persisted| persisted.credentials))
        })
        .await
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let persisted = Persisted {
            resource: self.resource.clone(),
            metadata: self.metadata.clone(),
            credentials,
        };
        self.blocking(move |secrets, integration| write(secrets, integration, &persisted))
            .await
    }

    async fn clear(&self) -> Result<(), AuthError> {
        self.blocking(erase).await
    }

    /// A revoked refresh token means signing in again; don't keep it around.
    async fn on_refresh_token_rejected(&self, _: &StoredCredentials) -> Result<(), AuthError> {
        self.blocking(erase).await
    }

    async fn acquire_refresh_guard(&self) -> Result<Option<CredentialRefreshGuard>, AuthError> {
        Ok(Some(CredentialRefreshGuard::new(
            refresh_lock(self.integration).lock_owned().await,
        )))
    }
}

/// Whether stored tokens exist for this server URL. Never reads them into the UI.
pub fn signed_in(
    secrets: &dyn SecretStore,
    integration: Uuid,
    url: &str,
) -> Result<bool, RuntimeError> {
    Ok(read(secrets, integration)?.is_some_and(|persisted| {
        persisted.resource == url && persisted.credentials.token_response.is_some()
    }))
}

/// Local sign-out. Serialized with refreshes so one in flight cannot save the
/// tokens back afterwards. The authorization server is not contacted.
pub async fn sign_out(
    secrets: Arc<dyn SecretStore>,
    integration: Uuid,
) -> Result<(), RuntimeError> {
    let _guard = refresh_lock(integration).lock_owned().await;
    tokio::task::spawn_blocking(move || erase(secrets.as_ref(), integration))
        .await
        .map_err(|_| RuntimeError::SecretUnavailable)?
}

/// A manager for a connection, from stored state only: no browser, no prompt.
pub(super) async fn manager(
    secrets: Arc<dyn SecretStore>,
    integration: Uuid,
    url: &str,
) -> Result<AuthorizationManager, RuntimeError> {
    let read_secrets = secrets.clone();
    let persisted = tokio::task::spawn_blocking(move || read(read_secrets.as_ref(), integration))
        .await
        .map_err(|_| RuntimeError::SecretUnavailable)??
        .filter(|persisted| persisted.resource == url)
        .ok_or(RuntimeError::AuthenticationRequired)?;
    let mut manager =
        AuthorizationManager::new_with_oauth_http_client(url, Arc::new(BladeOAuthHttp::new()?))
            .await
            .map_err(map)?;
    manager.set_metadata(persisted.metadata.clone());
    manager.set_credential_store(KeyringCredentials {
        integration,
        resource: url.to_owned(),
        metadata: persisted.metadata,
        secrets,
    });
    if !manager.initialize_from_store().await.map_err(map)? {
        return Err(RuntimeError::AuthenticationRequired);
    }
    Ok(manager)
}

// ---------------------------------------------------------------------------
// Sign-in

/// Text for the page the browser shows after the redirect, already translated.
pub struct CallbackPage {
    pub success: String,
    pub failure: String,
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn page(status: &str, message: &str) -> String {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Zaguán Blade</title>\
         <body style=\"font-family:system-ui,sans-serif;margin:3rem\"><p>{}</p></body>",
        escape(message)
    );
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\n\
         Referrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// What arrived at the redirect URI.
enum Callback {
    /// The full callback URL, for rmcp to validate state, issuer and code.
    Code(String),
    Denied,
    Failed,
}

/// The request target of one bounded HTTP request head, or None.
async fn request_target(stream: &mut tokio::net::TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut buffer = [0u8; 1024];
    let read = async {
        while !head.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).await.ok()?;
            if count == 0 || head.len() + count > MAX_CALLBACK_BYTES {
                return None;
            }
            head.extend_from_slice(&buffer[..count]);
        }
        Some(())
    };
    tokio::time::timeout(CALLBACK_READ_TIMEOUT, read)
        .await
        .ok()??;
    let line = std::str::from_utf8(&head).ok()?.lines().next()?;
    let mut parts = line.split(' ');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("GET"), Some(target), Some(version)) if version.starts_with("HTTP/1.") => {
            Some(target.to_owned())
        }
        _ => None,
    }
}

/// Waits for the browser to return with this sign-in's `state`. Anything else
/// reaching the port (another tab, a stale redirect, a probe) is refused and
/// the wait continues.
async fn await_callback(
    listener: &TcpListener,
    port: u16,
    state: &str,
    texts: &CallbackPage,
) -> Result<(Callback, tokio::net::TcpStream), RuntimeError> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|_| RuntimeError::AuthorizationFailed)?;
        let Some(target) = request_target(&mut stream).await else {
            let _ = stream
                .write_all(page("400 Bad Request", &texts.failure).as_bytes())
                .await;
            continue;
        };
        let url = format!("http://127.0.0.1:{port}{target}");
        let Ok(parsed) = reqwest::Url::parse(&url) else {
            continue;
        };
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        if parsed.path() != "/callback" || query.get("state").map(String::as_str) != Some(state) {
            let _ = stream
                .write_all(page("404 Not Found", &texts.failure).as_bytes())
                .await;
            continue;
        }
        let callback = match query.get("error").map(String::as_str) {
            Some("access_denied") => Callback::Denied,
            Some(_) => Callback::Failed,
            None => Callback::Code(url),
        };
        return Ok((callback, stream));
    }
}

/// Interactive sign-in. `open` shows the authorization page in the user's
/// browser. Existing tokens are replaced only when the new sign-in succeeds.
pub async fn sign_in(
    url: &str,
    settings: &OAuthSettings,
    integration: Uuid,
    secrets: Arc<dyn SecretStore>,
    open: impl FnOnce(&str) -> Result<(), RuntimeError>,
    texts: &CallbackPage,
    cancel: &CancellationToken,
) -> Result<(), RuntimeError> {
    let work = async {
        let parsed = reqwest::Url::parse(url).map_err(|_| RuntimeError::UnsupportedTransport)?;
        if !protected(&parsed) {
            return Err(RuntimeError::OAuthUnsupported);
        }
        let http = BladeOAuthHttp::new()?;
        let challenge = http.challenge(url).await;
        let mut manager = AuthorizationManager::new_with_oauth_http_client(url, Arc::new(http))
            .await
            .map_err(map)?;
        let resolution = manager
            .resolve_metadata_from_challenge(challenge.as_deref())
            .await
            .map_err(map)?;
        // Guessed endpoints on the MCP host are not a published authorization server.
        if resolution.source == AuthorizationMetadataSource::LegacyEndpointFallback {
            return Err(RuntimeError::OAuthUnsupported);
        }
        let metadata = resolution.metadata;
        for endpoint in [&metadata.authorization_endpoint, &metadata.token_endpoint] {
            if !reqwest::Url::parse(endpoint).is_ok_and(|endpoint| protected(&endpoint)) {
                return Err(RuntimeError::OAuthUnsupported);
            }
        }
        manager.set_metadata(metadata.clone());
        manager.set_credential_store(KeyringCredentials {
            integration,
            resource: url.to_owned(),
            metadata,
            secrets,
        });

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|_| RuntimeError::AuthorizationFailed)?;
        let port = listener
            .local_addr()
            .map_err(|_| RuntimeError::AuthorizationFailed)?
            .port();
        let mut request = AuthorizationRequest::new(format!("http://127.0.0.1:{port}/callback"))
            .with_client_name("Zaguán Blade");
        if !settings.scopes.is_empty() {
            request = request.with_scopes(settings.scopes.iter().cloned());
        }
        if let Some(client_id) = &settings.client_id {
            request = request.with_preregistered_client(client_id.clone());
        }
        let session = AuthorizationSession::new(manager, request)
            .await
            .map_err(|(_, error)| map(error))?;
        let authorization = reqwest::Url::parse(session.get_authorization_url())
            .ok()
            .filter(protected)
            .ok_or(RuntimeError::OAuthUnsupported)?;
        let state = authorization
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .ok_or(RuntimeError::AuthorizationFailed)?;
        open(authorization.as_str())?;

        let (callback, mut stream) = await_callback(&listener, port, &state, texts).await?;
        let result = match callback {
            Callback::Code(callback) => session
                .handle_callback_url(&callback)
                .await
                .map(|_| ())
                .map_err(map),
            Callback::Denied => Err(RuntimeError::AuthorizationDenied),
            Callback::Failed => Err(RuntimeError::AuthorizationFailed),
        };
        let response = match result {
            Ok(()) => page("200 OK", &texts.success),
            Err(_) => page("400 Bad Request", &texts.failure),
        };
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        result
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(RuntimeError::Cancelled),
        result = tokio::time::timeout(SIGN_IN_TIMEOUT, work) => result.unwrap_or(Err(RuntimeError::TimedOut)),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use oauth2::TokenResponse;
    use rmcp::transport::auth::OAuthTokenResponse;

    #[derive(Default)]
    pub(crate) struct MemorySecrets(pub Mutex<HashMap<(Uuid, String), String>>);
    impl SecretStore for MemorySecrets {
        fn get(&self, _: Uuid, _: &str) -> Result<Option<String>, RuntimeError> {
            Ok(None)
        }
        fn get_internal(
            &self,
            integration: Uuid,
            key: &str,
        ) -> Result<Option<String>, RuntimeError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&(integration, key.to_owned()))
                .cloned())
        }
        fn set_internal(
            &self,
            integration: Uuid,
            key: &str,
            value: &str,
        ) -> Result<(), RuntimeError> {
            // Mirrors the Windows per-entry bound.
            assert!(value.len() * 2 <= 2560);
            self.0
                .lock()
                .unwrap()
                .insert((integration, key.to_owned()), value.to_owned());
            Ok(())
        }
        fn delete_internal(&self, integration: Uuid, key: &str) -> Result<(), RuntimeError> {
            self.0
                .lock()
                .unwrap()
                .remove(&(integration, key.to_owned()));
            Ok(())
        }
    }

    fn persisted(resource: &str, token: &str) -> Persisted {
        let token: OAuthTokenResponse = serde_json::from_value(
            serde_json::json!({"access_token": token, "token_type": "bearer"}),
        )
        .unwrap();
        let mut metadata = AuthorizationMetadata::default();
        metadata.authorization_endpoint = "https://auth.example.com/authorize".into();
        metadata.token_endpoint = "https://auth.example.com/token".into();
        metadata.issuer = Some("https://auth.example.com".into());
        Persisted {
            resource: resource.into(),
            metadata,
            // No expiry, so a connection uses the token without refreshing.
            credentials: StoredCredentials::new("client".into(), Some(token), vec![], None),
        }
    }

    pub(crate) fn store(secrets: &dyn SecretStore, integration: Uuid, resource: &str, token: &str) {
        write(secrets, integration, &persisted(resource, token)).unwrap();
    }

    #[test]
    fn large_state_is_chunked_and_replaced_atomically() {
        let secrets = MemorySecrets::default();
        let id = Uuid::new_v4();
        assert!(!signed_in(&secrets, id, "https://mcp.example.com").unwrap());
        // A JWT-sized token needs several entries.
        store(&secrets, id, "https://mcp.example.com", &"j".repeat(6000));
        let first = secrets.0.lock().unwrap().len();
        assert!(first > 4);
        assert!(signed_in(&secrets, id, "https://mcp.example.com").unwrap());
        // Tokens never follow a changed server URL.
        assert!(!signed_in(&secrets, id, "https://other.example.com").unwrap());
        store(&secrets, id, "https://mcp.example.com", "short");
        // The previous generation's chunks are gone.
        assert_eq!(secrets.0.lock().unwrap().len(), 2);
        let read = read(&secrets, id).unwrap().unwrap();
        assert_eq!(
            read.credentials
                .token_response
                .unwrap()
                .access_token()
                .secret(),
            "short"
        );
        erase(&secrets, id).unwrap();
        assert!(secrets.0.lock().unwrap().is_empty());
        assert!(write(
            &secrets,
            id,
            &persisted("https://mcp.example.com", &"x".repeat(60_000))
        )
        .is_err());
    }

    #[test]
    fn torn_or_foreign_state_reads_as_signed_out() {
        let secrets = MemorySecrets::default();
        let id = Uuid::new_v4();
        store(&secrets, id, "https://mcp.example.com", &"j".repeat(3000));
        let chunk = secrets
            .0
            .lock()
            .unwrap()
            .keys()
            .find(|(_, key)| key.ends_with(".1"))
            .cloned()
            .unwrap();
        secrets.0.lock().unwrap().remove(&chunk);
        assert!(!signed_in(&secrets, id, "https://mcp.example.com").unwrap());
        secrets.set_internal(id, MANIFEST, "v2 nonsense").unwrap();
        assert_eq!(
            signed_in(&secrets, id, "https://mcp.example.com"),
            Err(RuntimeError::SecretUnavailable)
        );
    }

    /// A locked keyring is an error the user can act on, never "signed out":
    /// that would offer a fresh sign-in and overwrite working tokens.
    #[tokio::test]
    async fn a_locked_keyring_is_not_mistaken_for_signed_out() {
        struct Locked;
        impl SecretStore for Locked {
            fn get(&self, _: Uuid, _: &str) -> Result<Option<String>, RuntimeError> {
                Err(RuntimeError::SecretUnavailable)
            }
        }
        let id = Uuid::new_v4();
        let url = "https://mcp.example.com";
        assert_eq!(
            signed_in(&Locked, id, url),
            Err(RuntimeError::SecretUnavailable)
        );
        assert_eq!(
            write(&Locked, id, &persisted(url, "token")),
            Err(RuntimeError::SecretUnavailable)
        );
        let locked: Arc<dyn SecretStore> = Arc::new(Locked);
        assert_eq!(
            sign_out(locked.clone(), id).await,
            Err(RuntimeError::SecretUnavailable)
        );
        assert!(matches!(
            manager(locked, id, url).await,
            Err(RuntimeError::SecretUnavailable)
        ));
    }

    #[test]
    fn callback_pages_escape_server_independent_text() {
        let response = page("200 OK", "<script>alert(1)</script> & done");
        assert!(response.contains("&lt;script&gt;"));
        assert!(response.contains("default-src 'none'"));
        assert!(!response.contains("<script>"));
    }

    #[tokio::test]
    async fn callback_listener_ignores_requests_without_this_sign_in_state() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let texts = CallbackPage {
            success: "ok".into(),
            failure: "no".into(),
        };
        let client = async {
            for target in [
                "/callback?code=x&state=forged",
                "/other?state=s1",
                "/callback?code=c&state=s1",
            ] {
                let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .unwrap();
                stream
                    .write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                if target.ends_with("s1") && target.starts_with("/callback") {
                    break;
                }
                let mut response = String::new();
                stream.read_to_string(&mut response).await.unwrap();
                assert!(response.starts_with("HTTP/1.1 404"));
            }
        };
        let (result, _) = tokio::join!(await_callback(&listener, port, "s1", &texts), client);
        let (Callback::Code(url), _) = result.unwrap() else {
            panic!()
        };
        assert_eq!(
            url,
            format!("http://127.0.0.1:{port}/callback?code=c&state=s1")
        );
    }

    #[test]
    fn oauth_destinations_must_be_protected() {
        let url = |value: &str| reqwest::Url::parse(value).unwrap();
        assert!(protected(&url("https://auth.example.com/token")));
        assert!(protected(&url("http://127.0.0.1:9000/token")));
        assert!(!protected(&url("http://auth.example.com/token")));
        assert!(!protected(&url("ftp://auth.example.com/token")));
    }
}
