//! Streamable HTTP MCP endpoints. Requests carry only explicitly configured
//! headers, never follow redirects, and every response body is bounded before
//! the SDK parses it. Transport recovery never re-sends an ordinary request.
use super::{
    config::{ConfigValue, OAuthSettings},
    credentials::SecretStore,
    mcp_session::{Signals, MAX_FRAME_BYTES},
    RuntimeError,
};
use bytes::Bytes;
use futures_util::{stream::BoxStream, Stream, StreamExt};
use reqwest::{
    header::{HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE, WWW_AUTHENTICATE},
    StatusCode,
};
use rmcp::{
    model::{ClientJsonRpcMessage, ClientRequest, ErrorData, JsonRpcMessage, ServerJsonRpcMessage},
    transport::{
        common::client_side_sse::SseRetryPolicy,
        streamable_http_client::{
            AuthRequiredError, InsufficientScopeError, SseError, StreamableHttpClient,
            StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
            StreamableHttpError, StreamableHttpPostResponse,
        },
    },
};
use sse_stream::{Sse, SseStream};
use std::{
    collections::{BTreeMap, HashMap},
    io,
    net::IpAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};
use uuid::Uuid;

const EVENT_STREAM: &str = "text/event-stream";
const JSON: &str = "application/json";
const SESSION_HEADER: &str = "Mcp-Session-Id";
const LAST_EVENT_HEADER: &str = "Last-Event-Id";

/// Names the transport owns. Configured headers cannot replace them; the
/// protocol headers it adds itself are applied after this check.
pub(super) fn reserved_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "accept"
            | "content-type"
            | "content-length"
            | "host"
            | "connection"
            | "transfer-encoding"
            | "mcp-session-id"
            | "mcp-protocol-version"
            | "mcp-method"
            | "mcp-name"
            | "last-event-id"
    ) || name.starts_with("mcp-param-")
}

pub struct PreparedHttp {
    pub url: String,
    headers: HashMap<HeaderName, HeaderValue>,
    pub header_names: Vec<String>,
    /// Plain HTTP to a host other than this machine.
    pub insecure: bool,
    pub fingerprint: String,
    /// Present when the server is reached with the user's OAuth sign-in.
    pub(super) oauth: Option<OAuthBinding>,
}

pub(super) struct OAuthBinding {
    pub integration: Uuid,
    pub secrets: Arc<dyn SecretStore>,
}

impl PreparedHttp {
    pub fn resolve(
        url: &str,
        headers: &BTreeMap<String, ConfigValue>,
        oauth: Option<&OAuthSettings>,
        integration: Uuid,
        secrets: &Arc<dyn SecretStore>,
    ) -> Result<Self, RuntimeError> {
        let parsed = reqwest::Url::parse(url).map_err(|_| RuntimeError::UnsupportedTransport)?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
            return Err(RuntimeError::UnsupportedTransport);
        }
        let insecure = parsed.scheme() == "http" && !loopback(&parsed);
        // Bind approval to the URL plus every resolved header value, including
        // secrets. Only header names are returned to the webview.
        let mut bytes = Vec::new();
        let mut push = |value: &[u8]| {
            bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
            bytes.extend_from_slice(value);
        };
        push(url.as_bytes());
        let mut resolved = HashMap::new();
        let mut header_names = Vec::new();
        for (name, value) in headers {
            if reserved_header(name) {
                return Err(RuntimeError::UnsupportedTransport);
            }
            let header =
                HeaderName::from_bytes(name.as_bytes()).map_err(|_| RuntimeError::ConfigChanged)?;
            let mut value = match value {
                ConfigValue::Text { value } => {
                    HeaderValue::from_str(value).map_err(|_| RuntimeError::ConfigChanged)?
                }
                ConfigValue::Secret { name } => {
                    let secret = secrets
                        .get(integration, name)?
                        .ok_or(RuntimeError::SecretMissing)?;
                    if secret.len() > 8192 {
                        return Err(RuntimeError::InvalidSecret);
                    }
                    HeaderValue::from_str(&secret).map_err(|_| RuntimeError::InvalidSecret)?
                }
            };
            value.set_sensitive(true);
            push(header.as_str().as_bytes());
            push(value.as_bytes());
            header_names.push(name.clone());
            resolved.insert(header, value);
        }
        // Approval binds to the sign-in configuration, not to tokens, which
        // rotate on every refresh. Tokens are bound to this URL in storage.
        let oauth = match oauth {
            None => None,
            Some(settings) => {
                if insecure || resolved.contains_key(&reqwest::header::AUTHORIZATION) {
                    return Err(RuntimeError::OAuthUnsupported);
                }
                if !super::oauth::signed_in(secrets.as_ref(), integration, url)? {
                    return Err(RuntimeError::AuthenticationRequired);
                }
                push(b"oauth");
                push(settings.client_id.as_deref().unwrap_or_default().as_bytes());
                for scope in &settings.scopes {
                    push(scope.as_bytes());
                }
                Some(OAuthBinding {
                    integration,
                    secrets: secrets.clone(),
                })
            }
        };
        Ok(Self {
            url: url.to_owned(),
            headers: resolved,
            header_names,
            insecure,
            fingerprint: super::store::fingerprint(&bytes),
            oauth,
        })
    }

    /// Header names and values, secrets resolved, for forwarding to an ACP
    /// agent. Sorted by name so the order is stable.
    pub(super) fn header_pairs(&self) -> Vec<(String, String)> {
        let mut pairs: Vec<_> = self
            .headers
            .iter()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
            })
            .collect();
        pairs.sort();
        pairs
    }

    pub(super) fn transport(
        &self,
        signals: Arc<Signals>,
        budget: usize,
    ) -> Result<StreamableHttpClientTransport<BoundedHttpClient>, RuntimeError> {
        let (client, config) = self.parts(signals, budget)?;
        Ok(StreamableHttpClientTransport::with_client(client, config))
    }

    /// The bounded client and transport configuration, for callers that wrap
    /// the client (OAuth adds the bearer token and one refresh on a 401).
    pub(super) fn parts(
        &self,
        signals: Arc<Signals>,
        budget: usize,
    ) -> Result<(BoundedHttpClient, StreamableHttpClientTransportConfig), RuntimeError> {
        let client = reqwest::Client::builder()
            // A redirect target never receives configured headers or credentials.
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("ZaguanBlade/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| RuntimeError::LaunchFailed)?;
        let mut config = StreamableHttpClientTransportConfig::with_uri(self.url.as_str())
            .custom_headers(self.headers.clone())
            .max_concurrent_requests(4)
            .max_sse_event_size(MAX_FRAME_BYTES)
            // Replaying a POST after session expiry could repeat a tool call.
            .reinit_on_expired_session(false);
        // Resuming an event stream re-attaches to it; it never re-sends a POST.
        config.retry_config = Arc::new(ResumeTwice);
        Ok((
            BoundedHttpClient {
                client,
                signals,
                remaining: Arc::new(AtomicUsize::new(budget)),
            },
            config,
        ))
    }
}

pub(super) fn loopback(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            host == "localhost" || host.ends_with(".localhost")
        }
    }
}

#[derive(Debug)]
struct ResumeTwice;
impl SseRetryPolicy for ResumeTwice {
    fn retry(&self, attempts: usize) -> Option<Duration> {
        (attempts < 2).then_some(Duration::from_secs(1))
    }
}

type HttpError = StreamableHttpError<reqwest::Error>;

/// The SDK's own reqwest client reads JSON bodies without a size limit. This
/// one charges every received byte to a per-connection budget as well.
#[derive(Clone)]
pub struct BoundedHttpClient {
    client: reqwest::Client,
    signals: Arc<Signals>,
    remaining: Arc<AtomicUsize>,
}

fn limit_error() -> HttpError {
    StreamableHttpError::Io(io::Error::new(
        io::ErrorKind::InvalidData,
        "response limit exceeded",
    ))
}

impl BoundedHttpClient {
    fn charge(&self, bytes: usize) -> bool {
        let charged = self
            .remaining
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |left| {
                left.checked_sub(bytes)
            })
            .is_ok();
        if !charged {
            self.signals.exceeded();
        }
        charged
    }

    fn send_error(&self, error: reqwest::Error) -> HttpError {
        if error.is_connect() || error.is_timeout() {
            self.signals.unreachable();
        }
        StreamableHttpError::Client(error)
    }

    fn request(
        &self,
        mut request: reqwest::RequestBuilder,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<reqwest::RequestBuilder, HttpError> {
        if let Some(token) = auth {
            request = request.bearer_auth(token);
        }
        for (name, value) in headers {
            if matches!(name.as_str(), "accept" | "mcp-session-id" | "last-event-id") {
                return Err(StreamableHttpError::ReservedHeaderConflict(
                    name.to_string(),
                ));
            }
            request = request.header(name, value);
        }
        Ok(request)
    }

    fn check_auth(&self, response: &reqwest::Response) -> Result<(), HttpError> {
        let challenge = || {
            response
                .headers()
                .get(WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned()
        };
        match response.status() {
            StatusCode::UNAUTHORIZED => {
                self.signals.auth_required();
                Err(StreamableHttpError::AuthRequired(AuthRequiredError::new(
                    challenge(),
                )))
            }
            StatusCode::FORBIDDEN => {
                self.signals.auth_required();
                Err(StreamableHttpError::InsufficientScope(
                    InsufficientScopeError::new(challenge(), None),
                ))
            }
            _ => Ok(()),
        }
    }

    async fn body(&self, mut response: reqwest::Response) -> Result<Vec<u8>, HttpError> {
        if response
            .content_length()
            .is_some_and(|length| length > MAX_FRAME_BYTES as u64)
        {
            self.signals.exceeded();
            return Err(limit_error());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(StreamableHttpError::Client)?
        {
            if body.len() + chunk.len() > MAX_FRAME_BYTES || !self.charge(chunk.len()) {
                self.signals.exceeded();
                return Err(limit_error());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    fn events(&self, response: reqwest::Response) -> BoxStream<'static, Result<Sse, SseError>> {
        SseStream::from_bytes_stream(BoundedEvents {
            inner: response.bytes_stream().boxed(),
            client: self.clone(),
            line: 0,
            event: 0,
            after_cr: false,
            failed: false,
        })
        .boxed()
    }
}

fn content_type(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(CONTENT_TYPE)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
}

fn awaits_no_reply(message: &ClientJsonRpcMessage) -> bool {
    matches!(
        message,
        ClientJsonRpcMessage::Notification(_)
            | ClientJsonRpcMessage::Response(_)
            | ClientJsonRpcMessage::Error(_)
    )
}

/// Legacy servers often reject `server/discover` with a plain 4xx. Correlate
/// it as a JSON-RPC error so the SDK falls back to `initialize`, as the SDK's
/// own reqwest client does.
fn legacy_discover(
    message: &ClientJsonRpcMessage,
    session_attached: bool,
    status: StatusCode,
    body: &[u8],
) -> Option<StreamableHttpPostResponse> {
    if session_attached
        || !status.is_client_error()
        || matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
    {
        return None;
    }
    let ClientJsonRpcMessage::Request(request) = message else {
        return None;
    };
    if !matches!(request.request, ClientRequest::DiscoverRequest(_)) {
        return None;
    }
    let error = match serde_json::from_slice::<ServerJsonRpcMessage>(body) {
        Ok(ServerJsonRpcMessage::Error(error)) => error.error,
        _ => {
            ErrorData::invalid_request(format!("server/discover rejected with HTTP {status}"), None)
        }
    };
    Some(StreamableHttpPostResponse::Json(
        ServerJsonRpcMessage::error(error, Some(request.id.clone())),
        None,
    ))
}

impl StreamableHttpClient for BoundedHttpClient {
    type Error = reqwest::Error;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, HttpError> {
        let mut request = self.request(
            self.client
                .post(uri.as_ref())
                .header(ACCEPT, format!("{EVENT_STREAM}, {JSON}")),
            auth,
            headers,
        )?;
        let session_attached = session_id.is_some();
        if let Some(session_id) = session_id {
            request = request.header(SESSION_HEADER, session_id.as_ref());
        }
        let response = request
            .json(&message)
            .send()
            .await
            .map_err(|error| self.send_error(error))?;
        self.check_auth(&response)?;
        let status = response.status();
        if matches!(status, StatusCode::ACCEPTED | StatusCode::NO_CONTENT) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if status == StatusCode::NOT_FOUND && session_attached {
            return Err(StreamableHttpError::SessionExpired);
        }
        let content_type = content_type(&response);
        let session = response
            .headers()
            .get(SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        // Some servers answer notifications with an empty 200.
        if status.is_success() && response.content_length() == Some(0) && awaits_no_reply(&message)
        {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        let is_json = content_type
            .as_deref()
            .is_some_and(|value| value.starts_with(JSON));
        if !status.is_success() {
            let body = self.body(response).await?;
            if let Some(response) = legacy_discover(&message, session_attached, status, &body) {
                return Ok(response);
            }
            if is_json {
                if let Ok(error @ JsonRpcMessage::Error(_)) =
                    serde_json::from_slice::<ServerJsonRpcMessage>(&body)
                {
                    return Ok(StreamableHttpPostResponse::Json(error, session));
                }
            }
            // Never echo response bodies; they may reflect request contents.
            return Err(StreamableHttpError::UnexpectedServerResponse(
                format!("HTTP {status}").into(),
            ));
        }
        match content_type.as_deref() {
            Some(value) if value.starts_with(EVENT_STREAM) => Ok(StreamableHttpPostResponse::Sse(
                self.events(response),
                session,
            )),
            Some(_) if is_json => {
                let body = self.body(response).await?;
                match serde_json::from_slice::<ServerJsonRpcMessage>(&body) {
                    Ok(parsed) => Ok(StreamableHttpPostResponse::Json(parsed, session)),
                    Err(_) if awaits_no_reply(&message) => Ok(StreamableHttpPostResponse::Accepted),
                    Err(error) => Err(StreamableHttpError::Deserialize(error)),
                }
            }
            other => Err(StreamableHttpError::UnexpectedContentType(
                other.map(str::to_owned),
            )),
        }
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), HttpError> {
        let response = self
            .request(self.client.delete(uri.as_ref()), auth, headers)?
            .header(SESSION_HEADER, session_id.as_ref())
            .send()
            .await
            .map_err(|error| self.send_error(error))?;
        if response.status() == StatusCode::METHOD_NOT_ALLOWED {
            return Ok(());
        }
        response
            .error_for_status()
            .map(|_| ())
            .map_err(StreamableHttpError::Client)
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, HttpError> {
        let mut request = self.request(
            self.client
                .get(uri.as_ref())
                .header(ACCEPT, format!("{EVENT_STREAM}, {JSON}")),
            auth,
            headers,
        )?;
        if let Some(session_id) = session_id {
            request = request.header(SESSION_HEADER, session_id.as_ref());
        }
        if let Some(last_event_id) = last_event_id {
            request = request.header(LAST_EVENT_HEADER, last_event_id);
        }
        let response = request
            .send()
            .await
            .map_err(|error| self.send_error(error))?;
        if response.status() == StatusCode::METHOD_NOT_ALLOWED {
            return Err(StreamableHttpError::ServerDoesNotSupportSse);
        }
        self.check_auth(&response)?;
        let response = response
            .error_for_status()
            .map_err(StreamableHttpError::Client)?;
        match content_type(&response) {
            Some(value) if value.starts_with(EVENT_STREAM) || value.starts_with(JSON) => {
                Ok(self.events(response))
            }
            other => Err(StreamableHttpError::UnexpectedContentType(other)),
        }
    }
}

/// Charges the connection budget and bounds one event (bytes up to a blank
/// line) before the SSE parser can buffer it. CR, LF and CRLF end lines.
struct BoundedEvents {
    inner: BoxStream<'static, reqwest::Result<Bytes>>,
    client: BoundedHttpClient,
    line: usize,
    event: usize,
    after_cr: bool,
    failed: bool,
}

impl Stream for BoundedEvents {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.failed {
            return Poll::Ready(None);
        }
        let chunk = match self.inner.poll_next_unpin(cx) {
            Poll::Ready(Some(Ok(chunk))) => chunk,
            Poll::Ready(Some(Err(error))) => {
                return Poll::Ready(Some(Err(io::Error::other(error))));
            }
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };
        if !self.client.charge(chunk.len()) {
            self.failed = true;
            return Poll::Ready(Some(Err(io::ErrorKind::InvalidData.into())));
        }
        for &byte in chunk.iter() {
            let ends_line = byte == b'\r' || (byte == b'\n' && !self.after_cr);
            self.after_cr = byte == b'\r';
            if byte == b'\n' && !ends_line {
                continue;
            }
            if ends_line {
                if self.line == 0 {
                    self.event = 0;
                }
                self.line = 0;
                continue;
            }
            self.line += 1;
            self.event += 1;
            if self.event > MAX_FRAME_BYTES {
                self.failed = true;
                self.client.signals.exceeded();
                return Poll::Ready(Some(Err(io::ErrorKind::InvalidData.into())));
            }
        }
        Poll::Ready(Some(Ok(chunk)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document_service::DocumentService,
        integrations::{
            config::{ConnectionConfig, IntegrationConfig, IntegrationDefinition, McpTransport},
            connections::ConnectionPhase,
            mcp_session::McpSession,
            permissions::{CallContext, CallTarget},
            runtime::IntegrationRuntime,
            store::IntegrationStore,
        },
    };
    use serde_json::{json, Value};
    use std::sync::{atomic::AtomicBool, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_util::sync::CancellationToken;

    #[derive(Clone)]
    struct Seen {
        method: String,
        target: String,
        headers: HashMap<String, String>,
        body: Value,
        raw: String,
    }
    type Log = Arc<Mutex<Vec<Seen>>>;

    /// Minimal HTTP/1.1 peer: one request per connection, then close.
    async fn server(reply: impl Fn(&Seen) -> Vec<u8> + Send + Sync + 'static) -> (String, Log) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let log: Log = Default::default();
        let (reply, seen) = (Arc::new(reply), log.clone());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (reply, seen) = (reply.clone(), seen.clone());
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0; 8192];
                    let end = loop {
                        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                            break end;
                        }
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                        }
                    };
                    let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
                    let mut lines = head.split("\r\n");
                    let mut line = lines.next().unwrap().split(' ');
                    let method = line.next().unwrap().to_owned();
                    let target = line.next().unwrap_or_default().to_owned();
                    let headers: HashMap<_, _> = lines
                        .filter_map(|line| line.split_once(':'))
                        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
                        .collect();
                    let length = headers
                        .get("content-length")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0);
                    let mut body = buffer[end + 4..].to_vec();
                    while body.len() < length {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => body.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let request = Seen {
                        method,
                        target,
                        headers,
                        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                        raw: String::from_utf8_lossy(&body).into_owned(),
                    };
                    let response = reply(&request);
                    seen.lock().unwrap().push(request);
                    let _ = socket.write_all(&response).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (url, log)
    }

    fn http(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (name, value) in headers {
            response.push_str(&format!("{name}: {value}\r\n"));
        }
        response.push_str("\r\n");
        let mut response = response.into_bytes();
        response.extend_from_slice(body);
        response
    }
    fn reply(seen: &Seen, result: Value, sse: bool, session: Option<&str>) -> Vec<u8> {
        let message = json!({"jsonrpc":"2.0","id":seen.body["id"],"result":result}).to_string();
        let mut headers = vec![("Content-Type", if sse { EVENT_STREAM } else { JSON })];
        if let Some(session) = session {
            headers.push((SESSION_HEADER, session));
        }
        if sse {
            http(
                "200 OK",
                &headers,
                format!("event: message\r\ndata: {message}\r\n\r\n").as_bytes(),
            )
        } else {
            http("200 OK", &headers, message.as_bytes())
        }
    }

    /// Modern servers answer `server/discover` with JSON; legacy servers reject
    /// it with a plain 400, issue a session and answer requests over SSE.
    fn mcp(
        legacy: bool,
        call: impl Fn(&Seen) -> Option<Vec<u8>> + Send + Sync + 'static,
    ) -> impl Fn(&Seen) -> Vec<u8> + Send + Sync + 'static {
        move |seen| {
            match seen.method.as_str() {
                "GET" => return http("405 Method Not Allowed", &[], b""),
                "DELETE" => return http("200 OK", &[], b""),
                _ => {}
            }
            let modern = |mut result: Value| {
                if !legacy {
                    result["resultType"] = json!("complete");
                    result["ttlMs"] = json!(0);
                    result["cacheScope"] = json!("private");
                }
                result
            };
            match seen.body["method"].as_str().unwrap_or_default() {
                "server/discover" if legacy => {
                    http("400 Bad Request", &[("Content-Type", "text/plain")], b"unknown method")
                }
                "server/discover" => reply(
                    seen,
                    json!({"resultType":"complete","supportedVersions":["2026-07-28"],"ttlMs":0,
                        "cacheScope":"private","capabilities":{"tools":{}}}),
                    false,
                    None,
                ),
                "initialize" => reply(
                    seen,
                    json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},
                        "serverInfo":{"name":"fixture","version":"1"}}),
                    false,
                    Some("legacy-session"),
                ),
                method if method.starts_with("notifications/") => http("202 Accepted", &[], b""),
                "tools/list" => reply(
                    seen,
                    modern(json!({"tools":[{"name":"search","inputSchema":{"type":"object"}}]})),
                    legacy,
                    None,
                ),
                "tools/call" => call(seen).unwrap_or_else(|| {
                    reply(
                        seen,
                        modern(json!({"structuredContent":{"arguments":seen.body["params"]["arguments"]},"isError":false})),
                        legacy,
                        None,
                    )
                }),
                other => panic!("unexpected request {other}"),
            }
        }
    }

    #[derive(Default)]
    struct Secrets(Mutex<HashMap<(Uuid, String), String>>);
    impl SecretStore for Secrets {
        fn get(&self, id: Uuid, name: &str) -> Result<Option<String>, RuntimeError> {
            Ok(self.0.lock().unwrap().get(&(id, name.into())).cloned())
        }
    }

    fn headers() -> BTreeMap<String, ConfigValue> {
        BTreeMap::from([
            (
                "Authorization".into(),
                ConfigValue::Secret {
                    name: "token".into(),
                },
            ),
            (
                "X-Client".into(),
                ConfigValue::Text {
                    value: "blade".into(),
                },
            ),
        ])
    }

    fn prepared(url: &str) -> PreparedHttp {
        resolve(url, &BTreeMap::new(), Uuid::nil(), &Default::default()).unwrap()
    }

    fn resolve(
        url: &str,
        headers: &BTreeMap<String, ConfigValue>,
        id: Uuid,
        secrets: &Arc<Secrets>,
    ) -> Result<PreparedHttp, RuntimeError> {
        let secrets: Arc<dyn SecretStore> = secrets.clone();
        PreparedHttp::resolve(url, headers, None, id, &secrets)
    }

    fn methods(log: &Log, method: &str) -> usize {
        log.lock()
            .unwrap()
            .iter()
            .filter(|seen| seen.body["method"] == method)
            .count()
    }

    #[tokio::test]
    async fn probe_connection_and_approved_call_work_over_modern_and_legacy_http() {
        for legacy in [false, true] {
            let (url, log) = server(mcp(legacy, |_| None)).await;
            let root = tempfile::tempdir().unwrap();
            let id = Uuid::new_v4();
            let secrets = Arc::new(Secrets::default());
            secrets
                .0
                .lock()
                .unwrap()
                .insert((id, "token".into()), "Bearer fixture-secret".into());
            let config = IntegrationConfig {
                entries: vec![IntegrationDefinition {
                    id,
                    name: "Remote".into(),
                    enabled: true,
                    connection: ConnectionConfig::Mcp {
                        transport: McpTransport::StreamableHttp {
                            url: url.clone(),
                            headers: headers(),
                            oauth: None,
                        },
                    },
                }],
                ..Default::default()
            };
            let revision = IntegrationStore::new(root.path().into())
                .save("missing", config)
                .unwrap()
                .revision;
            let runtime = Arc::new(IntegrationRuntime::new(root.path().into(), secrets));
            let documents = Arc::new(DocumentService::new(root.path().into()));

            let review = runtime.prepare(id, revision.clone(), &documents).unwrap();
            let shown = serde_json::to_value(&review).unwrap();
            assert_eq!(shown["kind"], "http");
            assert_eq!(shown["url"], url.as_str());
            assert_eq!(shown["header_names"], json!(["Authorization", "X-Client"]));
            assert_eq!(shown["insecure"], false);
            assert!(!shown.to_string().contains("fixture-secret"));
            let probe = runtime
                .run(review.ticket_id, documents.clone())
                .await
                .unwrap();
            assert_eq!(probe.tools, 1);
            assert_eq!(
                probe.protocol_version,
                if legacy { "2025-11-25" } else { "2026-07-28" }
            );

            let live = runtime
                .prepare_connection(id, revision, &documents)
                .unwrap();
            runtime
                .connect(live.ticket_id, documents.clone())
                .await
                .unwrap();
            let status = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let status = runtime.connections.status(id, &documents).unwrap().unwrap();
                    if status.phase == ConnectionPhase::Connected {
                        return status;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            let catalog = runtime
                .connections
                .catalog(status.connection_id, &documents)
                .unwrap();
            let context = CallContext::new(
                &documents,
                Uuid::new_v4(),
                Uuid::new_v4(),
                "call".into(),
                CancellationToken::new(),
            )
            .unwrap();
            let call = runtime
                .prepare_tool_call(
                    &documents,
                    &context,
                    CallTarget {
                        connection_id: status.connection_id,
                        alias: catalog.tools()[0].alias.clone(),
                        catalog_revision: catalog.revision().into(),
                    },
                    json!({"query":"approved"}),
                )
                .unwrap();
            runtime
                .decide_tool_call(&context, call.request_id, true)
                .unwrap();
            let result = runtime
                .execute_tool_call(documents.clone(), &context, call.request_id)
                .await
                .unwrap();
            assert_eq!(
                result.result["structuredContent"]["arguments"]["query"],
                "approved"
            );
            runtime
                .connections
                .disconnect(status.connection_id, &documents)
                .unwrap();
            runtime.shutdown().await;

            let log = log.lock().unwrap();
            let posts: Vec<_> = log.iter().filter(|seen| seen.method == "POST").collect();
            assert!(posts.iter().all(|seen| seen.headers["authorization"]
                == "Bearer fixture-secret"
                && seen.headers["x-client"] == "blade"));
            assert_eq!(
                posts
                    .iter()
                    .filter(|seen| seen.body["method"] == "tools/call")
                    .count(),
                1
            );
            if legacy {
                assert!(log.iter().any(|seen| seen.method == "DELETE"
                    && seen.headers.get("mcp-session-id").map(String::as_str)
                        == Some("legacy-session")));
            }
        }
    }

    #[tokio::test]
    async fn oversized_json_and_sse_responses_exhaust_the_bound_not_memory() {
        for sse in [false, true] {
            let (url, _) = server(move |seen| {
                if seen.body["method"] == "server/discover" {
                    let body = format!("{{\"padding\":\"{}\"}}", "x".repeat(MAX_FRAME_BYTES));
                    let content = if sse {
                        format!("data: {body}\n\n")
                    } else {
                        body
                    };
                    http(
                        "200 OK",
                        &[("Content-Type", if sse { EVENT_STREAM } else { JSON })],
                        content.as_bytes(),
                    )
                } else {
                    http("202 Accepted", &[], b"")
                }
            })
            .await;
            assert_eq!(
                McpSession::open_http(&prepared(&url), 64 * MAX_FRAME_BYTES)
                    .await
                    .err(),
                Some(RuntimeError::OutputLimit)
            );
        }
        // The connection budget also covers responses that are individually small.
        let (url, _) = server(mcp(false, |_| None)).await;
        assert_eq!(
            McpSession::open_http(&prepared(&url), 64).await.err(),
            Some(RuntimeError::OutputLimit)
        );
    }

    #[tokio::test]
    async fn authentication_unreachable_and_redirect_failures_are_distinct_and_safe() {
        let (url, _) =
            server(|_| http("401 Unauthorized", &[("WWW-Authenticate", "Bearer")], b"")).await;
        assert_eq!(
            McpSession::open_http(&prepared(&url), MAX_SESSION)
                .await
                .err(),
            Some(RuntimeError::AuthenticationRequired)
        );
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = closed.local_addr().unwrap();
        drop(closed);
        assert_eq!(
            McpSession::open_http(&prepared(&format!("http://{address}/mcp")), MAX_SESSION)
                .await
                .err(),
            Some(RuntimeError::ServerUnreachable)
        );
        let (target, redirected) = server(mcp(false, |_| None)).await;
        let (url, _) = server(move |_| {
            http(
                "307 Temporary Redirect",
                &[("Location", target.as_str())],
                b"",
            )
        })
        .await;
        let secrets = Arc::new(Secrets::default());
        secrets
            .0
            .lock()
            .unwrap()
            .insert((Uuid::nil(), "token".into()), "Bearer x".into());
        let with_secret = resolve(&url, &headers(), Uuid::nil(), &secrets).unwrap();
        assert_eq!(
            McpSession::open_http(&with_secret, MAX_SESSION).await.err(),
            Some(RuntimeError::ProtocolFailed)
        );
        assert!(redirected.lock().unwrap().is_empty());
    }
    const MAX_SESSION: usize = super::super::mcp_session::MAX_SESSION_BYTES;

    #[tokio::test]
    async fn oauth_sign_in_connects_refreshes_a_rejected_token_once_and_signs_out() {
        #[derive(Default)]
        struct Authority {
            revoked: Vec<String>,
            refreshes: usize,
        }
        let authority = Arc::new(Mutex::new(Authority::default()));
        let origin = Arc::new(Mutex::new(String::new()));
        let inner = mcp(false, |_| None);
        let (url, log) = server({
            let (authority, origin) = (authority.clone(), origin.clone());
            move |seen| {
                let origin = origin.lock().unwrap().clone();
                let json = |body: Value| {
                    http("200 OK", &[("Content-Type", JSON)], body.to_string().as_bytes())
                };
                match seen.target.split('?').next().unwrap_or_default() {
                    "/.well-known/oauth-protected-resource/mcp" => json(json!({
                        "resource": format!("{origin}/mcp"), "authorization_servers": [origin]})),
                    "/.well-known/oauth-authorization-server" => json(json!({
                        "issuer": origin,
                        "authorization_endpoint": format!("{origin}/authorize"),
                        "token_endpoint": format!("{origin}/token"),
                        "registration_endpoint": format!("{origin}/register"),
                        "code_challenge_methods_supported": ["S256"],
                        "response_types_supported": ["code"]})),
                    "/register" => http(
                        "201 Created",
                        &[("Content-Type", JSON)],
                        json!({"client_id": "dynamic-client",
                            "redirect_uris": seen.body["redirect_uris"]})
                        .to_string()
                        .as_bytes(),
                    ),
                    "/token" => {
                        let token = if seen.raw.contains("grant_type=refresh_token") {
                            authority.lock().unwrap().refreshes += 1;
                            "at-2"
                        } else {
                            assert!(seen.raw.contains("code=granted"));
                            assert!(seen.raw.contains("code_verifier="));
                            "at-1"
                        };
                        json(json!({"access_token": token, "token_type": "Bearer",
                            "expires_in": 3600, "refresh_token": "rt"}))
                    }
                    "/mcp" => {
                        let token = seen
                            .headers
                            .get("authorization")
                            .and_then(|value| value.strip_prefix("Bearer "));
                        match token {
                            Some(token)
                                if token.starts_with("at-")
                                    && !authority.lock().unwrap().revoked.iter().any(|t| t == token) =>
                            {
                                inner(seen)
                            }
                            _ => {
                                let challenge = format!(
                                    "Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\""
                                );
                                http("401 Unauthorized", &[("WWW-Authenticate", &challenge)], b"")
                            }
                        }
                    }
                    other => http("404 Not Found", &[], other.as_bytes()),
                }
            }
        })
        .await;
        *origin.lock().unwrap() = url.trim_end_matches("/mcp").to_owned();

        let root = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let config = IntegrationConfig {
            entries: vec![IntegrationDefinition {
                id,
                name: "Remote".into(),
                enabled: true,
                connection: ConnectionConfig::Mcp {
                    transport: McpTransport::StreamableHttp {
                        url: url.clone(),
                        headers: BTreeMap::new(),
                        oauth: Some(Default::default()),
                    },
                },
            }],
            ..Default::default()
        };
        let revision = IntegrationStore::new(root.path().into())
            .save("missing", config)
            .unwrap()
            .revision;
        let secrets = Arc::new(crate::integrations::oauth::tests::MemorySecrets::default());
        let runtime = Arc::new(IntegrationRuntime::new(root.path().into(), secrets.clone()));
        let documents = Arc::new(DocumentService::new(root.path().into()));
        assert_eq!(
            runtime
                .prepare_connection(id, revision.clone(), &documents)
                .err(),
            Some(RuntimeError::AuthenticationRequired)
        );
        let texts = || crate::integrations::oauth::CallbackPage {
            success: "Signed in".into(),
            failure: "Not signed in".into(),
        };
        // The browser: follow the redirect the authorization server would send.
        let browser = |reply: &'static str| {
            let (page, shown) = tokio::sync::oneshot::channel();
            let open = move |authorization: &str| {
                let authorization = reqwest::Url::parse(authorization).unwrap();
                let query: HashMap<String, String> =
                    authorization.query_pairs().into_owned().collect();
                assert_eq!(query["code_challenge_method"], "S256");
                assert_eq!(query["client_id"], "dynamic-client");
                assert!(query["redirect_uri"].starts_with("http://127.0.0.1:"));
                let redirect =
                    format!("{}?{reply}&state={}", query["redirect_uri"], query["state"]);
                tokio::spawn(async move {
                    let _ = page.send(reqwest::get(redirect).await.unwrap().text().await.unwrap());
                });
                Ok(())
            };
            (open, shown)
        };

        let (open, shown) = browser("error=access_denied");
        assert_eq!(
            runtime.sign_in(id, revision.clone(), open, texts()).await,
            Err(RuntimeError::AuthorizationDenied)
        );
        assert!(shown.await.unwrap().contains("Not signed in"));
        assert!(!runtime.signed_in(id).unwrap());

        let (open, shown) = browser("code=granted");
        runtime
            .sign_in(id, revision.clone(), open, texts())
            .await
            .unwrap();
        assert!(shown.await.unwrap().contains("Signed in"));
        assert!(runtime.signed_in(id).unwrap());
        // Tokens never reach configuration or the review shown to the user.
        let review = runtime
            .prepare_connection(id, revision.clone(), &documents)
            .unwrap();
        let shown = serde_json::to_value(&review).unwrap();
        assert_eq!(shown["oauth"], true);
        assert!(!shown.to_string().contains("at-1"));
        assert!(
            !std::fs::read_to_string(root.path().join("integrations.json"))
                .unwrap()
                .contains("at-1")
        );

        runtime
            .connect(review.ticket_id, documents.clone())
            .await
            .unwrap();
        let connected = |refreshes: usize| {
            let (runtime, documents, authority) =
                (runtime.clone(), documents.clone(), authority.clone());
            async move {
                tokio::time::timeout(Duration::from_secs(10), async {
                    loop {
                        let status = runtime.connections.status(id, &documents).unwrap().unwrap();
                        if status.phase == ConnectionPhase::Connected
                            && authority.lock().unwrap().refreshes == refreshes
                        {
                            return status;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap()
            }
        };
        let status = connected(0).await;

        // The server revokes the token: one refresh, one retry, and the
        // catalog refresh succeeds with the new token.
        authority.lock().unwrap().revoked.push("at-1".into());
        runtime
            .connections
            .refresh(status.connection_id, &documents)
            .unwrap();
        connected(1).await;
        let lists = |token: &str| {
            log.lock()
                .unwrap()
                .iter()
                .filter(|seen| {
                    seen.body["method"] == "tools/list"
                        && seen.headers.get("authorization").map(String::as_str)
                            == Some(&format!("Bearer {token}"))
                })
                .count()
        };
        assert_eq!(
            lists("at-1"),
            2,
            "initial discovery, then the rejected refresh"
        );
        assert_eq!(lists("at-2"), 1);

        runtime.sign_out(id).await.unwrap();
        assert!(!runtime.signed_in(id).unwrap());
        assert!(secrets.0.lock().unwrap().is_empty());
        assert_eq!(
            runtime.prepare_connection(id, revision, &documents).err(),
            Some(RuntimeError::AuthenticationRequired)
        );
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn an_expired_session_ends_the_call_without_replaying_it() {
        let (url, log) = server(mcp(true, |_| Some(http("404 Not Found", &[], b"")))).await;
        let session = McpSession::open_http(&prepared(&url), MAX_SESSION)
            .await
            .unwrap();
        let sent = AtomicBool::new(false);
        let result = session
            .call(
                "search".into(),
                Default::default(),
                &CancellationToken::new(),
                &sent,
            )
            .await;
        assert!(result.is_err());
        assert!(
            sent.load(Ordering::Acquire),
            "a sent call has an uncertain outcome"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(methods(&log, "tools/call"), 1);
        assert_eq!(methods(&log, "initialize"), 1);
    }

    #[test]
    fn resolution_binds_secrets_and_flags_plain_http_to_other_hosts() {
        let secrets = Arc::new(Secrets::default());
        let id = Uuid::new_v4();
        assert_eq!(
            resolve("https://example.com/mcp", &headers(), id, &secrets).err(),
            Some(RuntimeError::SecretMissing)
        );
        secrets
            .0
            .lock()
            .unwrap()
            .insert((id, "token".into()), "Bearer one".into());
        let first = resolve("https://example.com/mcp", &headers(), id, &secrets).unwrap();
        secrets
            .0
            .lock()
            .unwrap()
            .insert((id, "token".into()), "Bearer two".into());
        let second = resolve("https://example.com/mcp", &headers(), id, &secrets).unwrap();
        assert_ne!(first.fingerprint, second.fingerprint);
        assert!(!first.insecure);
        secrets
            .0
            .lock()
            .unwrap()
            .insert((id, "token".into()), "Bearer\r\nInjected: yes".into());
        assert_eq!(
            resolve("https://example.com/mcp", &headers(), id, &secrets).err(),
            Some(RuntimeError::InvalidSecret)
        );
        for (url, insecure) in [
            ("http://example.com/mcp", true),
            ("http://192.168.1.4:8080/mcp", true),
            ("http://localhost:8080/mcp", false),
            ("http://127.0.0.1/mcp", false),
            ("http://[::1]:9000/mcp", false),
            ("http://tool.localhost/mcp", false),
        ] {
            assert_eq!(prepared(url).insecure, insecure, "{url}");
        }
        for name in [
            "Accept",
            "Mcp-Session-Id",
            "MCP-Protocol-Version",
            "Mcp-Param-Query",
            "Host",
        ] {
            assert!(reserved_header(name), "{name}");
        }
        assert!(!reserved_header("X-Api-Key"));
    }

    #[tokio::test]
    async fn event_bound_tracks_blank_lines_across_chunks() {
        let client = BoundedHttpClient {
            client: reqwest::Client::new(),
            signals: Default::default(),
            remaining: Arc::new(AtomicUsize::new(usize::MAX)),
        };
        let events = |chunks: Vec<Vec<u8>>| {
            SseStream::from_bytes_stream(BoundedEvents {
                inner: futures_util::stream::iter(chunks.into_iter().map(|c| Ok(Bytes::from(c))))
                    .boxed(),
                client: client.clone(),
                line: 0,
                event: 0,
                after_cr: false,
                failed: false,
            })
        };
        let half = "x".repeat(MAX_FRAME_BYTES / 2);
        // Two events, each below the bound, split across CRLF boundaries.
        let mut stream = events(vec![
            format!("data: {half}\r").into_bytes(),
            format!("\n\r\ndata: {half}\n\n").into_bytes(),
        ]);
        assert!(stream.next().await.unwrap().is_ok());
        assert!(stream.next().await.unwrap().is_ok());
        assert!(stream.next().await.is_none());
        // One event spread over many lines still counts as one event.
        let mut stream = events(vec![
            format!("data: {half}\n").into_bytes(),
            format!("data: {half}\n").into_bytes(),
            b"data: x\n\n".to_vec(),
        ]);
        assert!(stream.next().await.unwrap().is_err());
        assert_eq!(client.signals.error(), RuntimeError::OutputLimit);
    }
}
