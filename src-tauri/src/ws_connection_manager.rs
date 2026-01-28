//! Persistent WebSocket Connection Manager
//!
//! Manages a single, reusable WebSocket connection to zcoderd.
//! Provides automatic reconnection and connection sharing across all operations.

use crate::blade_ws_client::{BladeWsClient, BladeWsEvent, WorkspaceInfo, ToolResult};
use tokio::sync::{mpsc, Mutex, RwLock};

/// Connection state
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// Manages a persistent WebSocket connection to zcoderd
pub struct WsConnectionManager {
    blade_url: RwLock<String>,
    api_key: RwLock<String>,
    client: Mutex<Option<BladeWsClient>>,
    state: RwLock<ConnectionState>,
    event_subscribers: Mutex<Vec<mpsc::UnboundedSender<BladeWsEvent>>>,
    session_id: RwLock<Option<String>>,
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

    /// Get current connection state
    pub async fn get_state(&self) -> ConnectionState {
        self.state.read().await.clone()
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
    /// 
    /// Note: Each call creates a new connection. For true connection reuse,
    /// the caller should maintain the event receiver and reuse it.
    pub async fn ensure_connected(&self) -> Result<mpsc::UnboundedReceiver<BladeWsEvent>, String> {
        // Always create a fresh connection for now
        // Future optimization: reuse existing connection if still valid
        self.connect().await
    }

    /// Connect to the WebSocket server
    async fn connect(&self) -> Result<mpsc::UnboundedReceiver<BladeWsEvent>, String> {
        // Set state to connecting
        {
            let mut state = self.state.write().await;
            *state = ConnectionState::Connecting;
        }

        let blade_url = self.blade_url.read().await.clone();
        let api_key = self.api_key.read().await.clone();

        eprintln!("[WS MANAGER] Connecting to {}", blade_url);

        let client = BladeWsClient::new(blade_url, api_key);
        
        match client.connect().await {
            Ok(event_rx) => {
                // Store the client
                {
                    let mut client_lock = self.client.lock().await;
                    *client_lock = Some(client);
                }
                
                // Update state
                {
                    let mut state = self.state.write().await;
                    *state = ConnectionState::Connected;
                }

                eprintln!("[WS MANAGER] Connected successfully");

                // Return the event receiver directly to the caller
                // The caller is responsible for processing events
                Ok(event_rx)
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

    /// Send a chat message using the persistent connection
    pub async fn send_message(
        &self,
        session_id: Option<String>,
        model_id: String,
        message: String,
        workspace: Option<WorkspaceInfo>,
    ) -> Result<(), String> {
        let client_lock = self.client.lock().await;
        let client = client_lock.as_ref().ok_or("Not connected")?;
        client.send_message(session_id, model_id, message, workspace).await
    }

    /// Send a chat message with storage mode
    pub async fn send_message_with_storage_mode(
        &self,
        session_id: Option<String>,
        model_id: String,
        message: String,
        workspace: Option<WorkspaceInfo>,
        storage_mode: Option<String>,
    ) -> Result<(), String> {
        let client_lock = self.client.lock().await;
        let client = client_lock.as_ref().ok_or("Not connected")?;
        client.send_message_with_storage_mode(session_id, model_id, message, workspace, storage_mode).await
    }

    /// Send a tool result
    pub async fn send_tool_result(
        &self,
        session_id: String,
        tool_call_id: String,
        result: ToolResult,
    ) -> Result<(), String> {
        let client_lock = self.client.lock().await;
        let client = client_lock.as_ref().ok_or("Not connected")?;
        client.send_tool_result(session_id, tool_call_id, result).await
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
        client.send_conversation_context(request_id, session_id, messages).await
    }

    /// Disconnect the WebSocket
    pub async fn disconnect(&self) {
        {
            let mut state = self.state.write().await;
            *state = ConnectionState::Disconnected;
        }
        
        // Drop the client - connection will close when dropped
        let mut client_lock = self.client.lock().await;
        *client_lock = None;

        // Clear subscribers
        let mut subscribers = self.event_subscribers.lock().await;
        subscribers.clear();

        // Clear session
        let mut session = self.session_id.write().await;
        *session = None;

        eprintln!("[WS MANAGER] Disconnected");
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        matches!(self.get_state().await, ConnectionState::Connected)
    }
}
