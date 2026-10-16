//! Owned MCP session over stdio or Streamable HTTP. Discovery is not tool
//! invocation authorization.
use super::{
    catalog::{CatalogBuilder, McpCatalog},
    http::PreparedHttp,
    RuntimeError,
};
use rmcp::{
    model::{
        CallToolRequest, CallToolRequestParams, CallToolResult, ClientRequest,
        PaginatedRequestParams, ProtocolVersion, ServerNotification, ServerResult,
        SubscriptionFilter,
    },
    service::{NotificationContext, PeerRequestOptions, RunningService},
    ClientCacheConfig, ClientHandler, ClientLifecycleMode, ClientServiceExt, RoleClient,
};
use std::{
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::Notify,
};

pub(super) const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_SESSION_BYTES: usize = 64 * 1024 * 1024;

/// Transport failures the SDK reduces to an opaque error, recorded where they
/// are observed so the UI can show an actionable reason.
#[derive(Default)]
pub struct Signals {
    exceeded: AtomicBool,
    auth_required: AtomicBool,
    unreachable: AtomicBool,
}
impl Signals {
    pub(super) fn exceeded(&self) {
        self.exceeded.store(true, Ordering::Release);
    }
    pub(super) fn auth_required(&self) {
        self.auth_required.store(true, Ordering::Release);
    }
    pub(super) fn unreachable(&self) {
        self.unreachable.store(true, Ordering::Release);
    }
    pub(super) fn error(&self) -> RuntimeError {
        if self.exceeded.load(Ordering::Acquire) {
            RuntimeError::OutputLimit
        } else if self.auth_required.load(Ordering::Acquire) {
            RuntimeError::AuthenticationRequired
        } else if self.unreachable.load(Ordering::Acquire) {
            RuntimeError::ServerUnreachable
        } else {
            RuntimeError::ProtocolFailed
        }
    }
}

/// Legacy servers deliver change notices here; 2026-07-28 notices arrive on
/// the subscription stream instead (see [`McpSession::watch_tool_changes`]).
pub struct McpClient {
    tools_changed: Arc<Notify>,
}
impl ClientHandler for McpClient {
    async fn on_tool_list_changed(&self, _: NotificationContext<RoleClient>) {
        self.tools_changed.notify_one();
    }
}

pub struct McpSession {
    service: RunningService<RoleClient, McpClient>,
    signals: Arc<Signals>,
    tools_changed: Arc<Notify>,
    pub protocol_version: String,
    pub resources: bool,
    pub prompts: bool,
}

impl McpSession {
    /// Exactly one wire request. MRTR/task results are unsupported; never use the
    /// SDK convenience loop that could silently issue another tools/call.
    pub(super) async fn call(
        &self,
        name: String,
        arguments: serde_json::Map<String, serde_json::Value>,
        cancel: &tokio_util::sync::CancellationToken,
        sent: &AtomicBool,
    ) -> Result<CallToolResult, RuntimeError> {
        if cancel.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
        let request = ClientRequest::CallToolRequest(CallToolRequest::new(
            CallToolRequestParams::new(name).with_arguments(arguments),
        ));
        // Once submission begins, failure cannot establish absence of effects.
        sent.store(true, Ordering::Release);
        let mut handle = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(RuntimeError::Cancelled),
            result = tokio::time::timeout_at(deadline, self.service.peer().send_request_with_option(request, PeerRequestOptions::no_options())) => {
                result.map_err(|_| RuntimeError::TimedOut)?.map_err(|_| self.close_error())?
            }
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(RuntimeError::Cancelled),
            _ = tokio::time::sleep_until(deadline) => Err(RuntimeError::TimedOut),
            result = &mut handle.rx => result.map_err(|_| self.close_error()).and_then(|result| result.map_err(|_| self.close_error())),
        };
        if matches!(
            result,
            Err(RuntimeError::Cancelled | RuntimeError::TimedOut)
        ) {
            // Best effort protocol cancellation, followed by actor-owned teardown.
            let _ =
                tokio::time::timeout(std::time::Duration::from_millis(250), handle.cancel(None))
                    .await;
        }
        match result? {
            ServerResult::CallToolResult(result) => Ok(result),
            ServerResult::InputRequiredResult(_) | ServerResult::CreateTaskResult(_) => {
                Err(RuntimeError::UnsupportedResult)
            }
            _ => Err(RuntimeError::ProtocolFailed),
        }
    }

    pub async fn open(
        read: impl AsyncRead + Send + Unpin + 'static,
        write: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Result<Self, RuntimeError> {
        let signals = Arc::new(Signals::default());
        let read = BoundedReader {
            inner: read,
            frame: 0,
            total: 0,
            signals: signals.clone(),
        };
        Self::serve((read, write), signals).await
    }

    /// `budget` bounds all response bytes received over this session's lifetime.
    pub async fn open_http(prepared: &PreparedHttp, budget: usize) -> Result<Self, RuntimeError> {
        let signals = Arc::new(Signals::default());
        let transport = prepared.transport(signals.clone(), budget)?;
        Self::serve(transport, signals).await
    }

    async fn serve<T, E, A>(transport: T, signals: Arc<Signals>) -> Result<Self, RuntimeError>
    where
        T: rmcp::transport::IntoTransport<RoleClient, E, A>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let tools_changed = Arc::new(Notify::new());
        let service = McpClient {
            tools_changed: tools_changed.clone(),
        }
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                legacy_version: Some(ProtocolVersion::V_2025_11_25),
            },
        )
        .await
        .map_err(|_| signals.error())?;
        // The SDK cache serves expired lists when a re-fetch fails, which would
        // make a dead server look healthy. Discovery is always live.
        service
            .peer()
            .set_response_cache_config(ClientCacheConfig::disabled())
            .await;
        let info = service.peer_info().ok_or(RuntimeError::ProtocolFailed)?;
        if ![ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25]
            .contains(&info.protocol_version)
        {
            return Err(RuntimeError::UnsupportedVersion);
        }
        Ok(Self {
            protocol_version: info.protocol_version.to_string(),
            resources: info.capabilities.resources.is_some(),
            prompts: info.capabilities.prompts.is_some(),
            service,
            signals,
            tools_changed,
        })
    }

    /// Resolves when the server has declared its tool list changed. Notices
    /// that arrive while nobody waits are retained (coalesced to one).
    pub async fn tools_changed(&self) {
        self.tools_changed.notified().await;
    }

    /// Holds a 2026-07-28 `subscriptions/listen` stream for tool list changes
    /// and never returns. After any end a notice may have been missed, so it
    /// reports a change before re-subscribing. Legacy sessions just wait.
    pub async fn watch_tool_changes(&self) {
        let modern = self.protocol_version == ProtocolVersion::V_2026_07_28.to_string();
        let declared = self.service.peer_info().is_some_and(|info| {
            info.capabilities
                .tools
                .as_ref()
                .is_some_and(|tools| tools.list_changed == Some(true))
        });
        if modern && declared {
            let filter = SubscriptionFilter::builder().tools_list_changed().build();
            let mut failures: u64 = 0;
            while failures < 3 {
                let started = tokio::time::Instant::now();
                let subscription = tokio::time::timeout(
                    Duration::from_secs(10),
                    self.service.peer().listen_with_capacity(
                        filter.clone(),
                        std::num::NonZeroUsize::new(8).expect("non-zero capacity"),
                    ),
                )
                .await;
                if let Ok(Ok(mut subscription)) = subscription {
                    if subscription.acknowledged().tools_list_changed != Some(true) {
                        break;
                    }
                    while let Ok(Some(notice)) = subscription.next().await {
                        if matches!(notice, ServerNotification::ToolListChangedNotification(_)) {
                            self.tools_changed.notify_one();
                        }
                    }
                    self.tools_changed.notify_one();
                }
                if self.is_closed() {
                    break;
                }
                failures = if started.elapsed() >= Duration::from_secs(60) {
                    0
                } else {
                    failures + 1
                };
                tokio::time::sleep(Duration::from_secs(5 * failures.max(1))).await;
            }
        }
        std::future::pending().await
    }

    pub async fn discover(&self, integration: uuid::Uuid) -> Result<McpCatalog, RuntimeError> {
        let mut catalog = CatalogBuilder::new(integration);
        if self
            .service
            .peer_info()
            .is_some_and(|info| info.capabilities.tools.is_some())
        {
            let mut cursor = None;
            let mut seen = std::collections::HashSet::new();
            for page in 0..16 {
                let response = self
                    .service
                    .list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor)))
                    .await
                    .map_err(|_| self.signals.error())?;
                for tool in response.tools {
                    catalog.push(tool)?;
                }
                cursor = response.next_cursor;
                let Some(next) = cursor.as_ref() else { break };
                if page == 15 || !seen.insert(next.clone()) {
                    return Err(RuntimeError::OutputLimit);
                }
            }
        }
        catalog.finish()
    }

    pub fn is_closed(&self) -> bool {
        self.service.is_closed() || self.service.peer().is_transport_closed()
    }

    pub fn close_error(&self) -> RuntimeError {
        self.signals.error()
    }

    pub async fn shutdown(mut self) {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), self.service.close()).await;
    }
}

/// Enforce bounds before the SDK can grow an unterminated JSON frame. A lifetime
/// budget also bounds unsolicited traffic; exceeding it closes, never restarts.
struct BoundedReader<R> {
    inner: R,
    frame: usize,
    total: usize,
    signals: Arc<Signals>,
}
impl<R: AsyncRead + Unpin> AsyncRead for BoundedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.signals.exceeded.load(Ordering::Acquire) {
            return Poll::Ready(Err(io::ErrorKind::InvalidData.into()));
        }
        let start = buffer.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buffer) {
            Poll::Ready(Ok(())) => {
                for byte in &buffer.filled()[start..] {
                    self.total += 1;
                    self.frame += 1;
                    if self.frame > MAX_FRAME_BYTES || self.total > MAX_SESSION_BYTES {
                        self.signals.exceeded();
                        // AsyncRead must not advance the caller's filled bytes on error.
                        buffer.set_filled(start);
                        return Poll::Ready(Err(io::ErrorKind::InvalidData.into()));
                    }
                    if *byte == b'\n' {
                        self.frame = 0;
                    }
                }
                Poll::Ready(Ok(()))
            }
            result => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    #[tokio::test]
    async fn frame_and_lifetime_limits_are_independent_and_fail_closed() {
        let signals = Arc::new(Signals::default());
        let mut reader = BoundedReader {
            inner: &b"abc\ndef\n"[..],
            frame: 0,
            total: MAX_SESSION_BYTES - 4,
            signals: signals.clone(),
        };
        assert!(reader.read_to_end(&mut Vec::new()).await.is_err());
        assert_eq!(signals.error(), RuntimeError::OutputLimit);
        let signals = Arc::new(Signals::default());
        let mut reader = BoundedReader {
            inner: &b"abc\ndef\n"[..],
            frame: MAX_FRAME_BYTES - 4,
            total: 0,
            signals: signals.clone(),
        };
        let mut output = Vec::new();
        reader.read_to_end(&mut output).await.unwrap();
        assert_eq!(output, b"abc\ndef\n");
        assert_eq!(signals.error(), RuntimeError::ProtocolFailed);
        let mut reader = BoundedReader {
            inner: &b"abcde"[..],
            frame: MAX_FRAME_BYTES - 4,
            total: 0,
            signals,
        };
        assert!(reader.read_to_end(&mut Vec::new()).await.is_err());
    }
}
