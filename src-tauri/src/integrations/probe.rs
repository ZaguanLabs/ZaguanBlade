//! Short-lived protocol probes. No tools, prompts, sessions, authentication or
//! editor callbacks are executed. Reader budget also bounds unterminated frames.
use super::RuntimeError;
use rmcp::model::{PaginatedRequestParams, ProtocolVersion};
use rmcp::{ClientHandler, ClientLifecycleMode, ClientServiceExt};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

pub const MAX_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct ProbeResult {
    pub protocol_version: String,
    pub tools: usize,
    pub resources: bool,
    pub prompts: bool,
    pub authentication_methods: usize,
}

struct ProbeClient;
impl ClientHandler for ProbeClient {}

pub async fn mcp(
    read: impl AsyncRead + Send + Unpin + 'static,
    write: impl AsyncWrite + Send + Unpin + 'static,
) -> Result<ProbeResult, RuntimeError> {
    let service = ProbeClient
        .serve_with_lifecycle(
            (read.take(MAX_OUTPUT_BYTES), write),
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                legacy_version: Some(ProtocolVersion::V_2025_11_25),
            },
        )
        .await
        .map_err(|_| RuntimeError::ProtocolFailed)?;
    let info = service.peer_info().ok_or(RuntimeError::ProtocolFailed)?;
    if ![ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25]
        .contains(&info.protocol_version)
    {
        return Err(RuntimeError::UnsupportedVersion);
    }
    let mut result = ProbeResult {
        protocol_version: info.protocol_version.to_string(),
        tools: 0,
        resources: info.capabilities.resources.is_some(),
        prompts: info.capabilities.prompts.is_some(),
        authentication_methods: 0,
    };
    if info.capabilities.tools.is_some() {
        let mut cursor = None;
        let mut seen = std::collections::HashSet::new();
        for page in 0..16 {
            let response = service
                .list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor)))
                .await
                .map_err(|_| RuntimeError::ProtocolFailed)?;
            result.tools += response.tools.len();
            if result.tools > 512 {
                return Err(RuntimeError::OutputLimit);
            }
            cursor = response.next_cursor;
            let Some(next) = cursor.as_ref() else { break };
            if page == 15 || !seen.insert(next.clone()) {
                return Err(RuntimeError::OutputLimit);
            }
        }
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), service.cancel()).await;
    Ok(result)
}

pub async fn acp(
    read: impl AsyncRead + Send + Unpin + 'static,
    write: impl AsyncWrite + Send + Unpin + 'static,
) -> Result<ProbeResult, RuntimeError> {
    use agent_client_protocol::schema::{
        v1::{
            InitializeRequest, RequestPermissionOutcome, RequestPermissionRequest,
            RequestPermissionResponse,
        },
        ProtocolVersion,
    };
    use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};
    let (sender, receiver) = tokio::sync::oneshot::channel();
    Client
        .builder()
        .on_receive_request(
            async move |_request: RequestPermissionRequest, responder, _connection| {
                responder.respond(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Cancelled,
                ))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(
            ByteStreams::new(write.compat_write(), read.take(MAX_OUTPUT_BYTES).compat()),
            |connection: ConnectionTo<Agent>| async move {
                let response = connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                let result = if response.protocol_version != ProtocolVersion::V1 {
                    Err(RuntimeError::UnsupportedVersion)
                } else {
                    Ok(ProbeResult {
                        protocol_version: response.protocol_version.to_string(),
                        tools: 0,
                        resources: false,
                        prompts: false,
                        authentication_methods: response.auth_methods.len(),
                    })
                };
                let _ = sender.send(result);
                Ok(())
            },
        )
        .await
        .map_err(|_| RuntimeError::ProtocolFailed)?;
    receiver.await.map_err(|_| RuntimeError::ProtocolFailed)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    async fn mcp_fixture(modern: bool, repeat_cursor: bool) -> Result<ProbeResult, RuntimeError> {
        let (client, server) = tokio::io::duplex(16384);
        let (read, write) = tokio::io::split(client);
        let peer = tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(server);
            let mut lines = BufReader::new(read).lines();
            let mut pages = 0;
            while let Some(line) = lines.next_line().await.unwrap() {
                let request: Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap();
                if method == "notifications/initialized" {
                    continue;
                }
                let result = match method {
                    "server/discover" if modern => {
                        json!({"resultType":"complete", "supportedVersions":["2026-07-28"], "ttlMs":0,"cacheScope":"private", "capabilities":{"tools":{},"resources":{}}})
                    }
                    "server/discover" => {
                        let response = json!({"jsonrpc":"2.0", "id":request["id"], "error":{"code":-32601,"message":"Not supported"}});
                        write
                            .write_all(format!("{response}\n").as_bytes())
                            .await
                            .unwrap();
                        continue;
                    }
                    "initialize" => {
                        json!({"protocolVersion":"2025-11-25", "capabilities":{"tools":{},"resources":{}},"serverInfo":{"name":"fixture","version":"1"}})
                    }
                    "tools/list" => {
                        pages += 1;
                        if pages == 2 {
                            assert_eq!(request["params"]["cursor"], "next");
                        }
                        let mut result = json!({"tools":[{"name":format!("tool{pages}"), "inputSchema":{"type":"object"}}]});
                        if repeat_cursor || pages == 1 {
                            result["nextCursor"] = json!("next");
                        }
                        if modern {
                            result["resultType"] = json!("complete");
                            result["ttlMs"] = json!(0);
                            result["cacheScope"] = json!("private");
                        }
                        result
                    }
                    other => panic!("probe must not call tools or start sessions: {other}"),
                };
                let response = json!({"jsonrpc":"2.0", "id":request["id"], "result":result});
                write
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), mcp(read, write))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), peer)
            .await
            .unwrap()
            .unwrap();
        result
    }

    #[tokio::test]
    async fn modern_and_legacy_discovery_paginate_without_executing_tools() {
        for modern in [false, true] {
            let result = mcp_fixture(modern, false).await.unwrap();
            assert_eq!(result.tools, 2);
            assert!(result.resources);
            assert!(!result.prompts);
            assert_eq!(
                result.protocol_version,
                if modern { "2026-07-28" } else { "2025-11-25" }
            );
        }
    }

    #[tokio::test]
    async fn repeated_pagination_cursor_is_bounded() {
        assert_eq!(
            mcp_fixture(false, true).await.unwrap_err(),
            RuntimeError::OutputLimit
        );
    }

    #[tokio::test]
    async fn acp_only_initializes_and_rejects_unsupported_versions() {
        for version in [1, 99] {
            let (client, server) = tokio::io::duplex(16384);
            let (read, write) = tokio::io::split(client);
            let peer = tokio::spawn(async move {
                let (read, mut write) = tokio::io::split(server);
                let mut lines = BufReader::new(read).lines();
                let request: Value =
                    serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
                assert_eq!(request["method"], "initialize");
                assert_eq!(request["params"]["protocolVersion"], 1);
                let caps = &request["params"]["clientCapabilities"];
                assert_ne!(caps["terminal"], json!(true));
                assert_ne!(caps["fs"]["readTextFile"], json!(true));
                assert_ne!(caps["fs"]["writeTextFile"], json!(true));
                let permission = json!({"jsonrpc":"2.0","id":"permission","method":"session/request_permission",
                    "params":{"sessionId":"unsolicited-session","toolCall":{"toolCallId":"call","title":"Unrequested operation"},
                    "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]}});
                write
                    .write_all(format!("{permission}\n").as_bytes())
                    .await
                    .unwrap();
                let decision: Value =
                    serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
                assert_eq!(decision["id"], "permission");
                assert_eq!(decision["result"]["outcome"]["outcome"], "cancelled");
                let response = json!({"jsonrpc":"2.0", "id":request["id"], "result":{"protocolVersion":version,"agentCapabilities":{},"authMethods":[]}});
                write
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .unwrap();
                assert!(lines.next_line().await.unwrap().is_none());
            });
            let result = tokio::time::timeout(std::time::Duration::from_secs(3), acp(read, write))
                .await
                .unwrap();
            if version == 1 {
                assert_eq!(result.unwrap().protocol_version, "1");
            } else {
                assert_eq!(result.unwrap_err(), RuntimeError::UnsupportedVersion);
            }
            peer.await.unwrap();
        }
    }

    #[tokio::test]
    async fn unterminated_output_is_bounded() {
        let (client, mut server) = tokio::io::duplex(16384);
        let (read, write) = tokio::io::split(client);
        let peer = tokio::spawn(async move {
            let _ = server
                .write_all(&vec![b'x'; MAX_OUTPUT_BYTES as usize + 1])
                .await;
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(3), mcp(read, write))
                .await
                .unwrap()
                .is_err()
        );
        peer.await.unwrap();
    }
}
