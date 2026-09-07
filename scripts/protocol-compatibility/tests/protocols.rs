//! Wire fixtures deliberately use raw JSON rather than an SDK server: both ends
//! using one SDK could hide incompatible assumptions about the protocol.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, ProtocolVersion};
use rmcp::{ClientHandler, ClientLifecycleMode, ClientServiceExt};
use serde_json::{json, Value};
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

struct Wire {
    input: Lines<BufReader<ReadHalf<DuplexStream>>>,
    output: WriteHalf<DuplexStream>,
}

impl Wire {
    fn new(stream: DuplexStream) -> Self {
        let (input, output) = tokio::io::split(stream);
        Self {
            input: BufReader::new(input).lines(),
            output,
        }
    }

    async fn read(&mut self) -> Value {
        serde_json::from_str(
            &self
                .input
                .next_line()
                .await
                .unwrap()
                .expect("unexpected EOF"),
        )
        .unwrap()
    }

    async fn send(&mut self, message: Value) {
        let mut bytes = serde_json::to_vec(&message).unwrap();
        bytes.push(b'\n');
        self.output.write_all(&bytes).await.unwrap();
        self.output.flush().await.unwrap();
    }

    async fn reply(&mut self, request: &Value, result: Value) {
        self.send(json!({"jsonrpc":"2.0", "id":request["id"], "result":result}))
            .await;
    }

    async fn expect_method(&mut self, method: &str) -> Value {
        let request = self.read().await;
        assert_eq!(request["method"], method, "unexpected message: {request}");
        request
    }

    async fn wait_for_close(&mut self) {
        while self.input.next_line().await.unwrap().is_some() {}
    }
}

#[derive(Clone)]
struct ProbeClient;
impl ClientHandler for ProbeClient {}

async fn mcp_roundtrip(legacy_error: Option<i32>) {
    let (client_transport, server_transport) = tokio::io::duplex(16384);
    let version = if legacy_error.is_some() {
        "2025-11-25"
    } else {
        "2026-07-28"
    };
    let server = async move {
        let mut wire = Wire::new(server_transport);
        let discover = wire.expect_method("server/discover").await;
        if let Some(code) = legacy_error {
            wire.send(json!({"jsonrpc":"2.0", "id":discover["id"],
                "error":{"code":code, "message":"Discovery not supported"}}))
                .await;
            let initialize = wire.expect_method("initialize").await;
            assert_eq!(initialize["params"]["protocolVersion"], version);
            wire.reply(
                &initialize,
                json!({"protocolVersion":version,
                "capabilities":{"tools":{}}, "serverInfo":{"name":"legacy-fixture","version":"1"}}),
            )
            .await;
            wire.expect_method("notifications/initialized").await;
        } else {
            wire.reply(&discover, json!({"resultType":"complete", "supportedVersions":[version],
                "ttlMs":0, "cacheScope":"private", "capabilities":{"tools":{}},
                "_meta":{"io.modelcontextprotocol/serverInfo":{"name":"modern-fixture","version":"1"}}})).await;
        }
        let list = wire.expect_method("tools/list").await;
        let mut list_result = json!({"tools":[{"name":"symbol_search", "description":"Fixture symbol search",
            "inputSchema":{"type":"object"}}]});
        if legacy_error.is_none() {
            list_result["resultType"] = json!("complete");
            list_result["ttlMs"] = json!(0);
            list_result["cacheScope"] = json!("private");
        }
        wire.reply(&list, list_result).await;
        let call = wire.expect_method("tools/call").await;
        assert_eq!(call["params"]["name"], "symbol_search");
        let mut call_result = json!({"content":[{"type":"text","text":"fixture result"}],
            "structuredContent":{"symbols":["fixture_symbol"]}, "isError":false});
        if legacy_error.is_none() {
            call_result["resultType"] = json!("complete");
        }
        wire.reply(&call, call_result).await;
        wire.wait_for_close().await;
    };
    let client = async move {
        let service = ProbeClient
            .serve_with_lifecycle(
                client_transport,
                ClientLifecycleMode::Auto {
                    preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                    legacy_version: Some(ProtocolVersion::V_2025_11_25),
                },
            )
            .await
            .unwrap();
        let expected = if legacy_error.is_some() {
            ProtocolVersion::V_2025_11_25
        } else {
            ProtocolVersion::V_2026_07_28
        };
        assert_eq!(service.peer_info().unwrap().protocol_version, expected);
        let tools = service.list_all_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "symbol_search");
        let result = service
            .call_tool(CallToolRequestParams::new("symbol_search"))
            .await
            .unwrap();
        assert_eq!(
            result.structured_content.unwrap()["symbols"][0],
            "fixture_symbol"
        );
        assert_eq!(result.is_error, Some(false));
        service.cancel().await.unwrap();
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(client, server);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn mcp_modern_discovery_and_structured_tool_results() {
    mcp_roundtrip(None).await;
}

#[tokio::test]
async fn mcp_legacy_fallback_after_unknown_discovery_method() {
    mcp_roundtrip(Some(-32601)).await;
}

#[tokio::test]
async fn mcp_legacy_fallback_after_rejected_discovery_parameters() {
    mcp_roundtrip(Some(-32602)).await;
}

#[tokio::test]
async fn acp_v1_services_permissions_and_updates_during_a_prompt() {
    use agent_client_protocol::schema::v1::{
        ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest,
        RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
        SessionNotification, StopReason, TextContent,
    };
    use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};

    let (client_transport, agent_transport) = tokio::io::duplex(16384);
    let notifications = Arc::new(AtomicUsize::new(0));
    let permissions = Arc::new(AtomicUsize::new(0));
    let received_notifications = notifications.clone();
    let received_permissions = permissions.clone();
    let agent = async move {
        let mut wire = Wire::new(agent_transport);
        let initialize = wire.expect_method("initialize").await;
        assert_eq!(initialize["params"]["protocolVersion"], 1);
        wire.reply(
            &initialize,
            json!({"protocolVersion":1,"agentCapabilities":{},"authMethods":[]}),
        )
        .await;
        let session = wire.expect_method("session/new").await;
        assert_eq!(session["params"]["mcpServers"], json!([]));
        wire.reply(&session, json!({"sessionId":"fixture-session"}))
            .await;
        let prompt = wire.expect_method("session/prompt").await;
        assert_eq!(prompt["params"]["sessionId"], "fixture-session");
        wire.send(json!({"jsonrpc":"2.0","id":99,"method":"session/request_permission",
            "params":{"sessionId":"fixture-session","toolCall":{"toolCallId":"fixture-call","title":"Fixture operation"},
                "options":[{"optionId":"reject-fixture","name":"Reject","kind":"reject_once"}]}})).await;
        let decision = wire.read().await;
        assert_eq!(decision["id"], 99);
        assert_eq!(decision["result"]["outcome"]["outcome"], "cancelled");
        wire.send(json!({"jsonrpc":"2.0","method":"session/update",
            "params":{"sessionId":"fixture-session","update":{"sessionUpdate":"agent_message_chunk",
                "content":{"type":"text","text":"Permission declined."}}}}))
            .await;
        wire.reply(&prompt, json!({"stopReason":"end_turn"})).await;
        wire.wait_for_close().await;
    };
    let client = async move {
        let (input, output) = tokio::io::split(client_transport);
        Client
            .builder()
            .on_receive_notification(
                async move |notification: SessionNotification, _cx| {
                    assert_eq!(notification.session_id.to_string(), "fixture-session");
                    received_notifications.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |_request: RequestPermissionRequest, responder, _cx| {
                    received_permissions.fetch_add(1, Ordering::SeqCst);
                    responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(
                ByteStreams::new(output.compat_write(), input.compat()),
                |connection: ConnectionTo<Agent>| async move {
                    let init = connection
                        .send_request(InitializeRequest::new(
                            agent_client_protocol::schema::ProtocolVersion::V1,
                        ))
                        .block_task()
                        .await?;
                    assert_eq!(
                        init.protocol_version,
                        agent_client_protocol::schema::ProtocolVersion::V1
                    );
                    let session = connection
                        .send_request(NewSessionRequest::new(std::env::current_dir().unwrap()))
                        .block_task()
                        .await?;
                    let prompt = connection
                        .send_request(PromptRequest::new(
                            session.session_id,
                            vec![ContentBlock::Text(TextContent::new(
                                "Exercise permission handling",
                            ))],
                        ))
                        .block_task()
                        .await?;
                    assert_eq!(prompt.stop_reason, StopReason::EndTurn);
                    Ok(())
                },
            )
            .await
            .unwrap();
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(client, agent);
    })
    .await
    .unwrap();
    assert_eq!(permissions.load(Ordering::SeqCst), 1);
    assert_eq!(notifications.load(Ordering::SeqCst), 1);
}
