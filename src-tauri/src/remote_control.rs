//! Remote Control Service
//!
//! Manages Telegram-based remote control of Zaguán Blade.
//! Handles pairing lifecycle, remote command execution,
//! and output batching for Telegram rate limiting.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Remote control pairing/connection state
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteControlState {
    /// Not paired with any Telegram bot
    Disconnected,
    /// Bot token configured and verified
    Connected {
        bot_username: String,
        telegram_chat_id: Option<String>,
        connected_at: i64,
    },
}

/// Status payload sent to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteControlStatus {
    pub state: String,
    pub paired: bool,
    pub telegram_chat_id: Option<String>,
    pub connected_at: Option<i64>,
    pub bot_username: Option<String>,
}

/// Manages the remote control lifecycle
pub struct RemoteControlService {
    state: Arc<RwLock<RemoteControlState>>,
}

impl RemoteControlService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(RemoteControlState::Disconnected)),
        }
    }

    /// Get current remote control status for the frontend
    pub async fn get_status(&self) -> RemoteControlStatus {
        let state = self.state.read().await;
        match &*state {
            RemoteControlState::Disconnected => RemoteControlStatus {
                state: "disconnected".to_string(),
                paired: false,
                telegram_chat_id: None,
                connected_at: None,
                bot_username: None,
            },
            RemoteControlState::Connected {
                bot_username,
                telegram_chat_id,
                connected_at,
            } => RemoteControlStatus {
                state: "connected".to_string(),
                paired: telegram_chat_id.is_some(),
                telegram_chat_id: telegram_chat_id.clone(),
                connected_at: Some(*connected_at),
                bot_username: Some(bot_username.clone()),
            },
        }
    }

    /// Set connected state when a bot token is verified
    pub async fn set_configured(&self, bot_username: String) {
        let mut state = self.state.write().await;
        // Keep existing chat_id if we were already connected
        let existing_chat_id = match &*state {
            RemoteControlState::Connected {
                telegram_chat_id, ..
            } => telegram_chat_id.clone(),
            _ => None,
        };

        *state = RemoteControlState::Connected {
            bot_username,
            telegram_chat_id: existing_chat_id,
            connected_at: chrono::Utc::now().timestamp(),
        };
    }

    /// Set paired chat id
    pub async fn set_chat_id(&self, chat_id: String) {
        let mut state = self.state.write().await;
        if let RemoteControlState::Connected {
            telegram_chat_id,
            connected_at,
            ..
        } = &mut *state
        {
            *telegram_chat_id = Some(chat_id);
            *connected_at = chrono::Utc::now().timestamp();
        }
    }

    /// Disconnect / unpair
    pub async fn disconnect(&self) {
        let mut state = self.state.write().await;
        *state = RemoteControlState::Disconnected;
    }

    /// Check if currently configured
    pub async fn is_configured(&self) -> bool {
        matches!(
            &*self.state.read().await,
            RemoteControlState::Connected { .. }
        )
    }
}
