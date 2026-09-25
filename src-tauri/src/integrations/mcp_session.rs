//! Owned stdio MCP session. Discovery is not tool invocation authorization.
use super::{
    catalog::{CatalogBuilder, McpCatalog},
    RuntimeError,
};
use rmcp::{
    model::{
        CallToolRequest, CallToolRequestParams, CallToolResult, ClientRequest,
        PaginatedRequestParams, ProtocolVersion, ServerResult,
    },
    service::{PeerRequestOptions, RunningService},
    ClientHandler, ClientLifecycleMode, ClientServiceExt, RoleClient,
};
use std::{
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
const MAX_SESSION_BYTES: usize = 64 * 1024 * 1024;

pub struct McpClient;
impl ClientHandler for McpClient {}

pub struct McpSession {
    service: RunningService<RoleClient, McpClient>,
    exceeded: Arc<AtomicBool>,
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
        let exceeded = Arc::new(AtomicBool::new(false));
        let read = BoundedReader {
            inner: read,
            frame: 0,
            total: 0,
            exceeded: exceeded.clone(),
        };
        let service = McpClient
            .serve_with_lifecycle(
                (read, write),
                ClientLifecycleMode::Auto {
                    preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                    legacy_version: Some(ProtocolVersion::V_2025_11_25),
                },
            )
            .await
            .map_err(|_| protocol_error(&exceeded))?;
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
            exceeded,
        })
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
                    .map_err(|_| protocol_error(&self.exceeded))?;
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
        protocol_error(&self.exceeded)
    }

    pub async fn shutdown(mut self) {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), self.service.close()).await;
    }
}

fn protocol_error(exceeded: &AtomicBool) -> RuntimeError {
    if exceeded.load(Ordering::Acquire) {
        RuntimeError::OutputLimit
    } else {
        RuntimeError::ProtocolFailed
    }
}

/// Enforce bounds before the SDK can grow an unterminated JSON frame. A lifetime
/// budget also bounds unsolicited traffic; exceeding it closes, never restarts.
struct BoundedReader<R> {
    inner: R,
    frame: usize,
    total: usize,
    exceeded: Arc<AtomicBool>,
}
impl<R: AsyncRead + Unpin> AsyncRead for BoundedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.exceeded.load(Ordering::Acquire) {
            return Poll::Ready(Err(io::ErrorKind::InvalidData.into()));
        }
        let start = buffer.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buffer) {
            Poll::Ready(Ok(())) => {
                for byte in &buffer.filled()[start..] {
                    self.total += 1;
                    self.frame += 1;
                    if self.frame > MAX_FRAME_BYTES || self.total > MAX_SESSION_BYTES {
                        self.exceeded.store(true, Ordering::Release);
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
        let exceeded = Arc::new(AtomicBool::new(false));
        let mut reader = BoundedReader {
            inner: &b"abc\ndef\n"[..],
            frame: 0,
            total: MAX_SESSION_BYTES - 4,
            exceeded: exceeded.clone(),
        };
        assert!(reader.read_to_end(&mut Vec::new()).await.is_err());
        assert!(exceeded.load(Ordering::Acquire));
        let exceeded = Arc::new(AtomicBool::new(false));
        let mut reader = BoundedReader {
            inner: &b"abc\ndef\n"[..],
            frame: MAX_FRAME_BYTES - 4,
            total: 0,
            exceeded: exceeded.clone(),
        };
        let mut output = Vec::new();
        reader.read_to_end(&mut output).await.unwrap();
        assert_eq!(output, b"abc\ndef\n");
        assert!(!exceeded.load(Ordering::Acquire));
        let mut reader = BoundedReader {
            inner: &b"abcde"[..],
            frame: MAX_FRAME_BYTES - 4,
            total: 0,
            exceeded,
        };
        assert!(reader.read_to_end(&mut Vec::new()).await.is_err());
    }
}
