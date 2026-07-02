//! Persistent WebSocket Connection Manager
//!
//! Manages a single, reusable WebSocket connection to zcoderd.
//! Provides automatic reconnection and connection sharing across all operations.

use crate::blade_ws_client::{BladeWsClient, BladeWsEvent, ToolResult, WorkspaceInfo};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, RwLock};

const DISCONNECT_FLUSH_WINDOW: Duration = Duration::from_millis(500);

/// Connection state
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

#[derive(Debug, Clone)]
struct AuthState {
    user_id: String,
    server_version: String,
}

/// Manages a persistent WebSocket connection to zcoderd
pub struct WsConnectionManager {
    blade_url: RwLock<String>,
    api_key: RwLock<String>,
    client: Mutex<Option<BladeWsClient>>,
    state: RwLock<ConnectionState>,
    event_subscribers: Mutex<Vec<mpsc::UnboundedSender<BladeWsEvent>>>,
    session_id: RwLock<Option<String>>,
    auth_state: RwLock<Option<AuthState>>,
    storage_mode: RwLock<Option<String>>,
}

impl WsConnectionManager {
    /// Create a new connection manager
    pub fn new(blade_url: String, api_key: String) -> Self {
        Self {
            blade_url: RwLock::new(blade_url),
            api_key: RwLock::new(api_key),
            client: Mutex::new(None),
            state: RwLock::new(ConnectionState::Disconnected),
            event_subscribers: Mutex::new(Vec::new()),
            session_id: RwLock::new(None),
            auth_state: RwLock::new(None),
            storage_mode: RwLock::new(None),
        }
    }

    /// Update credentials (e.g., when user changes API key in settings)
    pub async fn update_credentials(&self, blade_url: String, api_key: String) {
        {
            let mut url = self.blade_url.write().await;
            *url = blade_url;
        }
        {
            let mut key = self.api_key.write().await;
            *key = api_key;
        }
        // Force reconnect with new credentials
        self.disconnect().await;
    }

    /// Get current session ID
    pub async fn get_session_id(&self) -> Option<String> {
        self.session_id.read().await.clone()
    }

    /// Set session ID (called when server assigns one)
    pub async fn set_session_id(&self, session_id: Option<String>) {
        let mut sid = self.session_id.write().await;
        *sid = session_id;
    }

    /// Ensure connection is established, connecting if necessary
    /// Returns a receiver for events from this connection
    pub async fn ensure_connected(
        self: &Arc<Self>,
    ) -> Result<mpsc::UnboundedReceiver<BladeWsEvent>, String> {
        let current_client = {
            let client_lock = self.client.lock().await;
            client_lock.as_ref().cloned()
        };
        let has_client = current_client.is_some();

        let state = self.state.read().await.clone();
        let should_return_to_primary = if matches!(state, ConnectionState::Connected) {
            if let Some(client) = current_client {
                client.connected_role().await
                    == Some(crate::blade_endpoint::BladeEndpointRole::Backup)
                    && crate::blade_endpoint::preferred_managed_role().await
                        == crate::blade_endpoint::BladeEndpointRole::Primary
            } else {
                false
            }
        } else {
            false
        };

        if !has_client || matches!(state, ConnectionState::Disconnected) || should_return_to_primary
        {
            self.connect().await?;
        }

        Ok(self.add_subscriber().await)
    }

    /// Connect to the WebSocket server
    async fn connect(self: &Arc<Self>) -> Result<(), String> {
        // Set state to connecting
        {
            let mut state = self.state.write().await;
            *state = ConnectionState::Connecting;
        }

        // Ensure any previous client is closed before opening a new connection.
        // This prevents orphaned websocket tasks when reconnecting repeatedly.
        let previous_session_id = self.get_session_id().await;
        let previous_client = {
            let mut client_lock = self.client.lock().await;
            client_lock.take()
        };
        if let Some(client) = previous_client {
            if let Err(error) = client
                .close_with_session_disconnect(previous_session_id, DISCONNECT_FLUSH_WINDOW)
                .await
            {
                eprintln!(
                    "[WS MANAGER] Failed to send disconnect before reconnect: {}",
                    error
                );
            }
        }

        {
            let mut auth_state = self.auth_state.write().await;
            *auth_state = None;
        }

        {
            let mut session_id = self.session_id.write().await;
            *session_id = None;
        }

        {
            let mut storage_mode = self.storage_mode.write().await;
            *storage_mode = None;
        }

        let blade_url = self.blade_url.read().await.clone();
        let api_key = self.api_key.read().await.clone();

        eprintln!("[WS MANAGER] Connecting to {}", blade_url);

        let client = BladeWsClient::new(blade_url, api_key);

        match client.connect(None).await {
            Ok(event_rx) => {
                // Store the client
                {
                    let mut client_lock = self.client.lock().await;
                    *client_lock = Some(client.clone());
                }

                eprintln!("[WS MANAGER] Connected successfully");

                let manager = Arc::clone(self);
                tokio::spawn(async move {
                    manager.run_event_fanout(event_rx).await;
                });

                Ok(())
            }
            Err(e) => {
                // Reset state
                {
                    let mut state = self.state.write().await;
                    *state = ConnectionState::Disconnected;
                }
                Err(e)
            }
        }
    }

    async fn add_subscriber(&self) -> mpsc::UnboundedReceiver<BladeWsEvent> {
        let (tx, rx) = mpsc::unbounded_channel();

        if let Some(auth_state) = self.auth_state.read().await.clone() {
            let _ = tx.send(BladeWsEvent::Connected {
                user_id: auth_state.user_id,
                server_version: auth_state.server_version,
            });
        }

        let mut subscribers = self.event_subscribers.lock().await;
        subscribers.push(tx);
        rx
    }

    async fn broadcast_event(&self, event: BladeWsEvent) {
        let mut subscribers = self.event_subscribers.lock().await;
        subscribers.retain(|subscriber| subscriber.send(event.clone()).is_ok());
    }

    async fn run_event_fanout(
        self: Arc<Self>,
        mut event_rx: mpsc::UnboundedReceiver<BladeWsEvent>,
    ) {
        while let Some(event) = event_rx.recv().await {
            match &event {
                BladeWsEvent::Connected {
                    user_id,
                    server_version,
                } => {
                    {
                        let mut auth_state = self.auth_state.write().await;
                        *auth_state = Some(AuthState {
                            user_id: user_id.clone(),
                            server_version: server_version.clone(),
                        });
                    }
                    {
                        let mut state = self.state.write().await;
                        *state = ConnectionState::Connected;
                    }
                }
                BladeWsEvent::Session { session_id, .. } => {
                    let mut current_session = self.session_id.write().await;
                    *current_session = Some(session_id.clone());
                }
                BladeWsEvent::Disconnected => {
                    {
                        let mut state = self.state.write().await;
                        *state = ConnectionState::Disconnected;
                    }
                    {
                        let mut auth_state = self.auth_state.write().await;
                        *auth_state = None;
                    }
                    {
                        let mut session_id = self.session_id.write().await;
                        *session_id = None;
                    }
                    {
                        let mut storage_mode = self.storage_mode.write().await;
                        *storage_mode = None;
                    }
                    {
                        let mut client_lock = self.client.lock().await;
                        client_lock.take();
                    }
                }
                _ => {}
            }

            self.broadcast_event(event).await;
        }

        {
            let mut state = self.state.write().await;
            *state = ConnectionState::Disconnected;
        }
        {
            let mut auth_state = self.auth_state.write().await;
            *auth_state = None;
        }
        {
            let mut session_id = self.session_id.write().await;
            *session_id = None;
        }
        {
            let mut storage_mode = self.storage_mode.write().await;
            *storage_mode = None;
        }
        {
            let mut client_lock = self.client.lock().await;
            client_lock.take();
        }
    }

    async fn wait_for_authenticated(
        &self,
        event_rx: &mut mpsc::UnboundedReceiver<BladeWsEvent>,
    ) -> Result<(), String> {
        if self.auth_state.read().await.is_some() {
            return Ok(());
        }

        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match event_rx.recv().await {
                    Some(BladeWsEvent::Connected { .. }) => return Ok(()),
                    Some(BladeWsEvent::Error { message, .. }) => {
                        return Err(format!("Authentication failed: {}", message));
                    }
                    Some(BladeWsEvent::Disconnected) => {
                        return Err("WebSocket disconnected before authentication".to_string());
                    }
                    Some(_) => {}
                    None => return Err("WebSocket closed before authentication".to_string()),
                }
            }
        })
        .await
        .map_err(|_| "WebSocket authentication timed out".to_string())?
    }

    async fn get_client(&self) -> Result<BladeWsClient, String> {
        let client_lock = self.client.lock().await;
        client_lock
            .as_ref()
            .cloned()
            .ok_or_else(|| "Not connected".to_string())
    }

    pub async fn request_history_list(
        self: &Arc<Self>,
        project_id: String,
    ) -> Result<Vec<crate::blade_protocol::ConversationSummary>, String> {
        let mut event_rx = self.ensure_connected().await?;
        self.wait_for_authenticated(&mut event_rx).await?;

        let request_id = format!("history-list-{}", uuid::Uuid::new_v4());
        let client = self.get_client().await?;
        client
            .send_history_list_request(request_id.clone(), project_id)
            .await?;

        tokio::time::timeout(Duration::from_secs(15), async move {
            loop {
                match event_rx.recv().await {
                    Some(BladeWsEvent::HistoryListResponse {
                        request_id: response_id,
                        conversations,
                    }) if response_id == request_id => return Ok(conversations),
                    Some(BladeWsEvent::Error {
                        request_id: error_request_id,
                        message,
                        ..
                    }) if error_request_id
                        .as_deref()
                        .is_none_or(|value| value == request_id) =>
                    {
                        return Err(message);
                    }
                    Some(BladeWsEvent::Disconnected) => {
                        return Err(
                            "WebSocket disconnected while waiting for history list response"
                                .to_string(),
                        );
                    }
                    Some(_) => {}
                    None => {
                        return Err(
                            "WebSocket closed while waiting for history list response".to_string()
                        );
                    }
                }
            }
        })
        .await
        .map_err(|_| "Timed out waiting for history list response".to_string())?
    }

    pub async fn request_history_detail(
        self: &Arc<Self>,
        session_id: String,
    ) -> Result<crate::blade_protocol::FullConversation, String> {
        let mut event_rx = self.ensure_connected().await?;
        self.wait_for_authenticated(&mut event_rx).await?;

        let request_id = format!("history-detail-{}", uuid::Uuid::new_v4());
        let client = self.get_client().await?;
        client
            .send_history_detail_request(request_id.clone(), session_id)
            .await?;

        tokio::time::timeout(Duration::from_secs(15), async move {
            loop {
                match event_rx.recv().await {
                    Some(BladeWsEvent::HistoryDetailResponse {
                        request_id: response_id,
                        conversation,
                    }) if response_id == request_id => return Ok(conversation),
                    Some(BladeWsEvent::Error {
                        request_id: error_request_id,
                        message,
                        ..
                    }) if error_request_id
                        .as_deref()
                        .is_none_or(|value| value == request_id) =>
                    {
                        return Err(message);
                    }
                    Some(BladeWsEvent::Disconnected) => {
                        return Err(
                            "WebSocket disconnected while waiting for history detail response"
                                .to_string(),
                        );
                    }
                    Some(_) => {}
                    None => {
                        return Err("WebSocket closed while waiting for history detail response"
                            .to_string());
                    }
                }
            }
        })
        .await
        .map_err(|_| "Timed out waiting for history detail response".to_string())?
    }

    pub async fn request_zlp(self: &Arc<Self>, payload: Value) -> Result<Value, String> {
        let mut event_rx = self.ensure_connected().await?;
        self.wait_for_authenticated(&mut event_rx).await?;

        let request_id = format!("zlp-{}", uuid::Uuid::new_v4());
        let client = self.get_client().await?;
        client.send_zlp_request(request_id.clone(), payload).await?;

        tokio::time::timeout(Duration::from_secs(15), async move {
            loop {
                match event_rx.recv().await {
                    Some(BladeWsEvent::ZlpResponse {
                        request_id: response_id,
                        response,
                    }) if response_id == request_id => return Ok(response),
                    Some(BladeWsEvent::Error {
                        request_id: error_request_id,
                        message,
                        ..
                    }) if error_request_id
                        .as_deref()
                        .is_none_or(|value| value == request_id) =>
                    {
                        return Err(message);
                    }
                    Some(BladeWsEvent::Disconnected) => {
                        return Err(
                            "WebSocket disconnected while waiting for ZLP response".to_string()
                        );
                    }
                    Some(_) => {}
                    None => {
                        return Err("WebSocket closed while waiting for ZLP response".to_string());
                    }
                }
            }
        })
        .await
        .map_err(|_| "Timed out waiting for ZLP response".to_string())?
    }

    /// Send a chat message with storage mode and an explicit request id.
    pub async fn send_message_with_storage_mode_and_id(
        &self,
        request_id: String,
        session_id: Option<String>,
        model_id: String,
        message: String,
        images: Option<Vec<crate::protocol::ChatImage>>,
        workspace: Option<WorkspaceInfo>,
        storage_mode: Option<String>,
        mode: Option<String>,
        local_conversation_state: Option<crate::blade_ws_client::LocalConversationState>,
        tools: Option<Vec<Value>>,
        tool_choice: Option<Value>,
        parallel_tool_calls: Option<bool>,
        tag: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<(), String> {
        {
            let mut current_storage_mode = self.storage_mode.write().await;
            *current_storage_mode = storage_mode.clone();
        }

        let client = self.get_client().await?;
        client
            .send_message_with_storage_mode_and_id(
                request_id,
                session_id,
                model_id,
                message,
                images,
                workspace,
                storage_mode,
                mode,
                local_conversation_state,
                tools,
                tool_choice,
                parallel_tool_calls,
                tag,
                tags,
            )
            .await
    }

    /// Send a tool result
    pub async fn send_tool_result(
        &self,
        session_id: String,
        tool_call_id: String,
        result: ToolResult,
        model_id: Option<String>,
    ) -> Result<(), String> {
        let client_lock = self.client.lock().await;
        let client = client_lock.as_ref().ok_or("Not connected")?;
        client
            .send_tool_result(session_id, tool_call_id, result, model_id)
            .await
    }

    pub async fn send_approval_response(
        &self,
        session_id: String,
        approval_id: String,
        tool_call_id: String,
        approved: bool,
    ) -> Result<(), String> {
        let client_lock = self.client.lock().await;
        let client = client_lock.as_ref().ok_or("Not connected")?;
        client
            .send_approval_response(session_id, approval_id, tool_call_id, approved)
            .await
    }

    pub async fn send_cancel_request(
        &self,
        request_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<(), String> {
        let client_lock = self.client.lock().await;
        let client = client_lock.as_ref().ok_or("Not connected")?;
        client.send_cancel_request(request_id, session_id).await
    }

    /// Send conversation context
    pub async fn send_conversation_context(
        &self,
        request_id: String,
        session_id: String,
        messages: Vec<serde_json::Value>,
    ) -> Result<(), String> {
        let client_lock = self.client.lock().await;
        let client = client_lock.as_ref().ok_or("Not connected")?;
        client
            .send_conversation_context(request_id, session_id, messages)
            .await
    }

    pub async fn send_context_pack_response(
        &self,
        request_id: String,
        response_type: String,
        payload: crate::blade_protocol::ContextPackPayload,
    ) -> Result<(), String> {
        let client_lock = self.client.lock().await;
        let client = client_lock.as_ref().ok_or("Not connected")?;
        client
            .send_context_pack_response(request_id, response_type, payload)
            .await
    }

    /// Disconnect the WebSocket
    pub async fn disconnect(&self) {
        {
            let mut state = self.state.write().await;
            *state = ConnectionState::Disconnected;
        }

        // Close the client first, then drop it.
        let client_to_close = {
            let mut client_lock = self.client.lock().await;
            client_lock.take()
        };
        if let Some(client) = client_to_close {
            let session_id = self.get_session_id().await;
            if let Err(error) = client
                .close_with_session_disconnect(session_id, DISCONNECT_FLUSH_WINDOW)
                .await
            {
                eprintln!("[WS MANAGER] Failed to send disconnect: {}", error);
            }
        }

        // Clear subscribers
        let mut subscribers = self.event_subscribers.lock().await;
        subscribers.clear();

        {
            let mut auth_state = self.auth_state.write().await;
            *auth_state = None;
        }

        // Clear session
        let mut session = self.session_id.write().await;
        *session = None;

        let mut storage_mode = self.storage_mode.write().await;
        *storage_mode = None;

        eprintln!("[WS MANAGER] Disconnected");
    }

}
