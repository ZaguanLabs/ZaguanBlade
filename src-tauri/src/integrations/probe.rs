//! Short-lived protocol probes. No tools, prompts, sessions, authentication or
//! editor callbacks are executed. Reader budget also bounds unterminated frames.
use super::{catalog::McpCatalog, mcp_session::McpSession, RuntimeError};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

pub const MAX_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct ProbeResult {
    pub protocol_version: String,
    pub tools: usize,
    pub catalog: Option<McpCatalog>,
    pub resources: bool,
    pub prompts: bool,
    pub authentication_methods: usize,
}

pub async fn mcp(
    integration_id: uuid::Uuid,
    read: impl AsyncRead + Send + Unpin + 'static,
    write: impl AsyncWrite + Send + Unpin + 'static,
) -> Result<ProbeResult, RuntimeError> {
    let session = McpSession::open(read.take(MAX_OUTPUT_BYTES), write).await?;
    let catalog = session.discover(integration_id).await?;
    let result = ProbeResult {
        protocol_version: session.protocol_version.clone(),
        tools: catalog.tools().len(),
        catalog: Some(catalog),
        resources: session.resources,
        prompts: session.prompts,
        authentication_methods: 0,
    };
    session.shutdown().await;
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
                        catalog: None,
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

    async fn mcp_fixture(
        modern: bool,
        repeat_cursor: bool,
        duplicate_name: bool,
    ) -> Result<ProbeResult, RuntimeError> {
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
                        let name = if duplicate_name {
                            "repeated".into()
                        } else {
                            format!("tool{pages}")
                        };
                        let mut result = json!({"tools":[{"name":name, "description":"Fixture tool", "inputSchema":{"type":"object", "additionalProperties":false},
                            "outputSchema":{"type":"object"}, "annotations":{"readOnlyHint":true}}]});
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
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            mcp(uuid::Uuid::nil(), read, write),
        )
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
            let result = mcp_fixture(modern, false, false).await.unwrap();
            assert_eq!(result.tools, 2);
            let catalog = result.catalog.as_ref().unwrap();
            assert_eq!(catalog.tools()[0].definition.name, "tool1");
            assert_eq!(catalog.tools()[1].definition.name, "tool2");
            assert_eq!(
                catalog.tools()[0].definition.input_schema["additionalProperties"],
                json!(false)
            );
            assert!(catalog.tools()[0].definition.output_schema.is_some());
            assert!(catalog.tools()[0].definition.annotations.is_some());
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
            mcp_fixture(false, true, false).await.unwrap_err(),
            RuntimeError::OutputLimit
        );
    }

    #[tokio::test]
    async fn duplicate_names_across_pages_reject_the_entire_catalog() {
        for modern in [false, true] {
            assert_eq!(
                mcp_fixture(modern, false, true).await.unwrap_err(),
                RuntimeError::InvalidCatalog
            );
        }
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
        assert!(tokio::time::timeout(
            std::time::Duration::from_secs(3),
            mcp(uuid::Uuid::nil(), read, write)
        )
        .await
        .unwrap()
        .is_err());
        peer.await.unwrap();
    }
}
