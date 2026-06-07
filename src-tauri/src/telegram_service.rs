use crate::protocol::ToolCall;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

lazy_static::lazy_static! {
    static ref POLLING_TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
    static ref ACTIVE_INTERACTION: Mutex<Option<ActiveInteraction>> = Mutex::new(None);
}

#[derive(Deserialize, Debug)]
struct UpdateResponse {
    ok: bool,
    result: Vec<Update>,
}

#[derive(Deserialize, Debug)]
struct Update {
    update_id: i64,
    message: Option<Message>,
}

#[derive(Deserialize, Debug)]
struct Message {
    #[allow(dead_code)]
    message_id: i64,
    chat: Chat,
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
}

#[derive(Serialize)]
struct SendMessageReq<'a> {
    chat_id: String,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<String>,
}

#[derive(Deserialize, Debug)]
struct SendMessageResult {
    ok: bool,
    result: Message,
}

#[derive(Serialize)]
struct EditMessageTextReq<'a> {
    chat_id: String,
    message_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<String>,
}

struct ActiveInteraction {
    chat_id: String,
    message_id: Option<i64>,
    accumulated_text: String,
    reasoning_text: String,
    tool_status: Vec<String>,
    last_update_time: std::time::Instant,
    needs_flush: bool,
}

pub async fn start_polling(app_handle: AppHandle, token: String, _bot_username: String) {
    let mut task_guard = POLLING_TASK.lock().await;
    if let Some(task) = task_guard.take() {
        task.abort();
    }

    let handle = tokio::spawn(async move {
        let client = Client::new();
        let mut offset = 0;

        loop {
            let url = format!(
                "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=30",
                token, offset
            );

            match client.get(&url).send().await {
                Ok(resp) => {
                    if let Ok(update_resp) = resp.json::<UpdateResponse>().await {
                        if update_resp.ok {
                            for update in update_resp.result {
                                offset = offset.max(update.update_id + 1);

                                if let Some(msg) = update.message {
                                    handle_message(&app_handle, &client, &token, msg).await;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Telegram poll error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    });

    *task_guard = Some(handle);
}

pub async fn stop_polling() {
    let mut task_guard = POLLING_TASK.lock().await;
    if let Some(task) = task_guard.take() {
        task.abort();
    }
}

async fn handle_message(app_handle: &AppHandle, client: &Client, token: &str, msg: Message) {
    let text = msg.text.unwrap_or_default();
    let chat_id = msg.chat.id.to_string();
    let state = app_handle.state::<crate::app_state::AppState>();

    // Check if we are configured
    if !state.remote_control.is_configured().await {
        return;
    }

    // Only allow private chats
    if msg.chat.chat_type != "private" {
        return;
    }

    // On first interaction, set the chat ID as the paired user
    let status = state.remote_control.get_status().await;
    if status.telegram_chat_id.is_none() {
        state.remote_control.set_chat_id(chat_id.clone()).await;
        let _ = send_message(
            client,
            token,
            &chat_id,
            "Connected to Zaguán Blade! Send me commands to execute.",
        )
        .await;
        // Broadcast an event so frontend updates to "Connected"
        let _ = app_handle.emit(
            "remote_control_status",
            state.remote_control.get_status().await,
        );
    } else if let Some(paired_id) = status.telegram_chat_id {
        if paired_id != chat_id {
            let _ = send_message(
                client,
                token,
                &chat_id,
                "This bot is already paired with another user.",
            )
            .await;
            return;
        }
    }

    if text.starts_with("/start") {
        let _ = send_message(client, token, &chat_id, "Ready to receive commands.").await;
        return;
    }

    if !text.is_empty() {
        // Initialize active Telegram interaction
        let mut int_guard = ACTIVE_INTERACTION.lock().await;
        let msg_id = init_telegram_interaction(&chat_id, token, client).await;

        *int_guard = Some(ActiveInteraction {
            chat_id: chat_id.clone(),
            message_id: msg_id,
            accumulated_text: String::new(),
            reasoning_text: String::new(),
            tool_status: Vec::new(),
            last_update_time: std::time::Instant::now(),
            needs_flush: false,
        });

        // Broadcast an event so frontend can forward the command to the chat orchestrator
        let _ = app_handle.emit("telegram_command", text.clone());
    }
}

async fn init_telegram_interaction(chat_id: &str, token: &str, client: &Client) -> Option<i64> {
    let initial_text = "🤖 <b>Zaguán Blade is working...</b>";
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);

    let req = SendMessageReq {
        chat_id: chat_id.to_string(),
        text: initial_text,
        parse_mode: Some("HTML".to_string()),
    };

    if let Ok(resp) = client.post(&url).json(&req).send().await {
        if let Ok(send_resp) = resp.json::<SendMessageResult>().await {
            if send_resp.ok {
                return Some(send_resp.result.message_id);
            }
        }
    }
    None
}

fn escape_html(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
}

fn format_tool_call(tc: &ToolCall) -> String {
    let name = &tc.function.name;
    let args_str = &tc.function.arguments;

    let detail = if let Ok(val) = serde_json::from_str::<serde_json::Value>(args_str) {
        if let Some(cmd) = val.get("CommandLine").and_then(|v| v.as_str()) {
            format!("CommandLine: <code>{}</code>", escape_html(cmd))
        } else if let Some(path) = val.get("TargetFile").and_then(|v| v.as_str()) {
            format!("File: <code>{}</code>", escape_html(path))
        } else if let Some(path) = val.get("AbsolutePath").and_then(|v| v.as_str()) {
            format!("File: <code>{}</code>", escape_html(path))
        } else {
            escape_html(&args_str.chars().take(60).collect::<String>())
        }
    } else {
        escape_html(&args_str.chars().take(60).collect::<String>())
    };

    let status_icon = match tc.status.as_deref() {
        Some("executing") | Some("running") => "⏳",
        Some("success") | Some("completed") | Some("done") => "✅",
        Some("failed") | Some("error") => "❌",
        _ => "🔧",
    };

    let status_text = tc.status.as_deref().unwrap_or("queued");

    format!(
        "{} <code>{}</code> ({}) - {}",
        status_icon, name, status_text, detail
    )
}

fn render_html(interaction: &ActiveInteraction) -> String {
    let mut parts = Vec::new();

    if !interaction.reasoning_text.is_empty() {
        parts.push(format!(
            "🧠 <b>Thinking:</b>\n<i>{}</i>",
            escape_html(&interaction.reasoning_text)
        ));
    }

    if !interaction.tool_status.is_empty() {
        let mut tools_list = String::new();
        for tool in &interaction.tool_status {
            tools_list.push_str(&format!("• {}\n", tool));
        }
        parts.push(format!("🔧 <b>Tools:</b>\n{}", tools_list));
    }

    if !interaction.accumulated_text.is_empty() {
        parts.push(format!(
            "💬 <b>Response:</b>\n{}",
            escape_html(&interaction.accumulated_text)
        ));
    } else if parts.is_empty() {
        parts.push("🤖 <b>Zaguán Blade is working...</b>".to_string());
    }

    parts.join("\n\n")
}

async fn flush_interaction(client: &Client, token: &str, int: &mut ActiveInteraction, force: bool) {
    let msg_id = match int.message_id {
        Some(id) => id,
        None => return,
    };

    let now = std::time::Instant::now();
    // Throttle: edit at most once every 1200ms unless forced
    if !force && now.duration_since(int.last_update_time) < std::time::Duration::from_millis(1200) {
        int.needs_flush = true;
        return;
    }

    int.last_update_time = now;
    int.needs_flush = false;

    let html = render_html(int);

    let url = format!("https://api.telegram.org/bot{}/editMessageText", token);
    let req = EditMessageTextReq {
        chat_id: int.chat_id.clone(),
        message_id: msg_id,
        text: &html,
        parse_mode: Some("HTML".to_string()),
    };

    let _ = client.post(&url).json(&req).send().await;
}

pub async fn update_telegram_text<R: tauri::Runtime>(app_handle: &AppHandle<R>, chunk: &str) {
    let state = app_handle.state::<crate::app_state::AppState>();
    let token = {
        let config = state.config.lock().unwrap();
        config.telegram_bot_token.clone()
    };
    if token.is_empty() {
        return;
    }

    let mut int_guard = ACTIVE_INTERACTION.lock().await;
    if let Some(int) = int_guard.as_mut() {
        int.accumulated_text.push_str(chunk);
        let client = Client::new();
        flush_interaction(&client, &token, int, false).await;
    }
}

pub async fn update_telegram_reasoning<R: tauri::Runtime>(app_handle: &AppHandle<R>, chunk: &str) {
    let state = app_handle.state::<crate::app_state::AppState>();
    let token = {
        let config = state.config.lock().unwrap();
        config.telegram_bot_token.clone()
    };
    if token.is_empty() {
        return;
    }

    let mut int_guard = ACTIVE_INTERACTION.lock().await;
    if let Some(int) = int_guard.as_mut() {
        int.reasoning_text.push_str(chunk);
        let client = Client::new();
        flush_interaction(&client, &token, int, false).await;
    }
}

pub async fn update_telegram_tools<R: tauri::Runtime>(
    app_handle: &AppHandle<R>,
    tool_calls: &[ToolCall],
) {
    let state = app_handle.state::<crate::app_state::AppState>();
    let token = {
        let config = state.config.lock().unwrap();
        config.telegram_bot_token.clone()
    };
    if token.is_empty() {
        return;
    }

    let mut int_guard = ACTIVE_INTERACTION.lock().await;
    if let Some(int) = int_guard.as_mut() {
        int.tool_status = tool_calls.iter().map(|tc| format_tool_call(tc)).collect();
        let client = Client::new();
        // Force update on tool status changes
        flush_interaction(&client, &token, int, true).await;
    }
}

pub async fn complete_telegram_interaction<R: tauri::Runtime>(
    app_handle: &AppHandle<R>,
    _msg_id: &str,
) -> bool {
    let state = app_handle.state::<crate::app_state::AppState>();
    let token = {
        let config = state.config.lock().unwrap();
        config.telegram_bot_token.clone()
    };
    if token.is_empty() {
        return false;
    }

    let mut int_guard = ACTIVE_INTERACTION.lock().await;
    if let Some(mut int) = int_guard.take() {
        let client = Client::new();
        flush_interaction(&client, &token, &mut int, true).await;
        true
    } else {
        false
    }
}

async fn send_message(
    client: &Client,
    token: &str,
    chat_id: &str,
    text: &str,
) -> Result<(), reqwest::Error> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
    let req = SendMessageReq {
        chat_id: chat_id.to_string(),
        text,
        parse_mode: None,
    };
    client.post(&url).json(&req).send().await?;
    Ok(())
}

pub async fn send_to_telegram<R: tauri::Runtime>(
    app_handle: &AppHandle<R>,
    text: &str,
) -> Result<(), String> {
    let state = app_handle.state::<crate::app_state::AppState>();

    if !state.remote_control.is_configured().await {
        return Ok(());
    }

    let status = state.remote_control.get_status().await;
    let chat_id = match status.telegram_chat_id {
        Some(id) => id,
        None => return Ok(()),
    };

    let token = {
        let config = state.config.lock().unwrap();
        config.telegram_bot_token.clone()
    };

    if token.is_empty() {
        return Ok(());
    }

    let client = Client::new();
    let max_len = 4000;
    let chars: Vec<char> = text.chars().collect();
    for chunk in chars.chunks(max_len) {
        let chunk_str: String = chunk.iter().collect();
        let _ = send_message(&client, &token, &chat_id, &chunk_str).await;
    }

    Ok(())
}
