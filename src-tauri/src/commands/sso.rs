use crate::app_state::AppState;
use crate::config;
use crate::sso::{self, ApprovedLogin, PollOutcome, SsoLoginResult, SsoProgressEvent};
use std::time::Duration;
use tauri::{Emitter, State};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::oneshot;
use tokio::time::{sleep, Instant};

#[tauri::command]
pub async fn start_sso_login(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<SsoLoginResult, String> {
    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    {
        let mut active = state
            .sso_cancel
            .lock()
            .map_err(|error| format!("Failed to lock sign-in state: {error}"))?;
        if active.is_some() {
            return Err("A Zaguán sign-in is already in progress.".to_string());
        }
        *active = Some(cancel_tx);
    }

    let result = run_sso_login(&app, &state, &mut cancel_rx).await;

    if let Ok(mut active) = state.sso_cancel.lock() {
        *active = None;
    }

    if let Err(message) = &result {
        emit_progress(
            &app,
            SsoProgressEvent {
                status: "error".to_string(),
                user_code: None,
                verification_uri: None,
                message: Some(message.clone()),
                email: None,
                tier: None,
            },
        );
    }

    result
}

#[tauri::command]
pub fn cancel_sso_login(state: State<'_, AppState>) -> Result<(), String> {
    let cancel = state
        .sso_cancel
        .lock()
        .map_err(|error| format!("Failed to lock sign-in state: {error}"))?
        .take();
    if let Some(cancel) = cancel {
        let _ = cancel.send(());
    }
    Ok(())
}

#[tauri::command]
pub async fn sign_out_zaguan(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Ok(mut active) = state.sso_cancel.lock() {
        if let Some(cancel) = active.take() {
            let _ = cancel.send(());
        }
    }

    let remote = {
        let mut cfg = state
            .config
            .lock()
            .map_err(|error| format!("Failed to lock settings: {error}"))?;
        cfg.api_key.clear();
        cfg.user_id.clear();
        cfg.user_email.clear();
        cfg.tier.clear();
        cfg.remote_config()
    };

    let path = config::default_api_config_path();
    let remote_to_save = remote.clone();
    tokio::task::spawn_blocking(move || config::save_remote_ai_config(&path, &remote_to_save))
        .await
        .map_err(|error| format!("save remote ai settings task failed: {error}"))??;

    state
        .warmup_client
        .update_credentials(config::default_blade_url(), String::new(), String::new());
    state
        .ws_connection
        .update_credentials(config::default_blade_url(), String::new())
        .await;

    if let Ok(mut user_id) = state.user_id.lock() {
        *user_id = None;
    }

    let _ = app.emit("remote-settings-changed", ());
    Ok(())
}

async fn run_sso_login(
    app: &tauri::AppHandle,
    state: &AppState,
    cancel_rx: &mut oneshot::Receiver<()>,
) -> Result<SsoLoginResult, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Failed to initialize sign-in client: {error}"))?;
    let website_base_url = sso::website_base_url();

    emit_progress(
        app,
        SsoProgressEvent {
            status: "starting".to_string(),
            user_code: None,
            verification_uri: None,
            message: None,
            email: None,
            tier: None,
        },
    );

    let start = sso::start_device_auth(&client, &website_base_url).await?;
    emit_progress(
        app,
        SsoProgressEvent {
            status: "verification_required".to_string(),
            user_code: Some(start.user_code.clone()),
            verification_uri: Some(start.verification_uri_complete.clone()),
            message: None,
            email: None,
            tier: None,
        },
    );

    if let Err(error) = app
        .opener()
        .open_url(start.verification_uri_complete.clone(), None::<&str>)
    {
        eprintln!("[SSO] Failed to open verification URL: {error}");
    }

    let deadline = Instant::now() + Duration::from_secs(start.expires_in.max(1));
    let mut poll_delay = Duration::from_secs(start.interval.max(1));

    loop {
        if Instant::now() >= deadline {
            return Err("Zaguán sign-in expired. Start a new sign-in request.".to_string());
        }

        tokio::select! {
            _ = &mut *cancel_rx => {
                emit_progress(
                    app,
                    SsoProgressEvent {
                        status: "cancelled".to_string(),
                        user_code: None,
                        verification_uri: None,
                        message: Some("Sign-in cancelled.".to_string()),
                        email: None,
                        tier: None,
                    },
                );
                return Err("Zaguán sign-in cancelled.".to_string());
            }
            _ = sleep(poll_delay) => {}
        }

        match sso::poll_device_auth(&client, &website_base_url, &start.device_code).await? {
            PollOutcome::RateLimited(retry_after) => {
                poll_delay = retry_after.max(Duration::from_secs(1));
                emit_progress(
                    app,
                    SsoProgressEvent {
                        status: "rate_limited".to_string(),
                        user_code: Some(start.user_code.clone()),
                        verification_uri: Some(start.verification_uri.clone()),
                        message: Some(format!(
                            "Waiting {} seconds before retrying.",
                            poll_delay.as_secs()
                        )),
                        email: None,
                        tier: None,
                    },
                );
            }
            PollOutcome::Status(response) => match response.status.as_str() {
                "pending" => {
                    poll_delay = Duration::from_secs(start.interval.max(1));
                    emit_progress(
                        app,
                        SsoProgressEvent {
                            status: "pending".to_string(),
                            user_code: Some(start.user_code.clone()),
                            verification_uri: Some(start.verification_uri.clone()),
                            message: None,
                            email: None,
                            tier: None,
                        },
                    );
                }
                "pending_subscription" => {
                    poll_delay = Duration::from_secs(start.interval.max(1));
                    emit_progress(
                        app,
                        SsoProgressEvent {
                            status: "pending_subscription".to_string(),
                            user_code: Some(start.user_code.clone()),
                            verification_uri: Some(start.verification_uri_complete.clone()),
                            message: Some("Finish subscription checkout in the browser, then approve this device.".to_string()),
                            email: None,
                            tier: None,
                        },
                    );
                }
                "approved" => {
                    let approved = sso::approved_login(response)?;
                    let result = persist_approved_login(state, approved.clone()).await?;
                    emit_progress(
                        app,
                        SsoProgressEvent {
                            status: "approved".to_string(),
                            user_code: None,
                            verification_uri: None,
                            message: None,
                            email: Some(result.email.clone()),
                            tier: result.tier.clone(),
                        },
                    );
                    let _ = app.emit("remote-settings-changed", ());
                    return Ok(result);
                }
                "denied" => return Err("Zaguán sign-in was denied in the browser.".to_string()),
                "expired" => {
                    return Err("Zaguán sign-in expired. Start a new sign-in request.".to_string())
                }
                "consumed" => {
                    return Err(
                        "This Zaguán sign-in was already used. Start a new request.".to_string()
                    )
                }
                other => return Err(format!("Unexpected Zaguán sign-in status: {other}")),
            },
        }
    }
}

async fn persist_approved_login(
    state: &AppState,
    approved: ApprovedLogin,
) -> Result<SsoLoginResult, String> {
    let remote = {
        let mut cfg = state
            .config
            .lock()
            .map_err(|error| format!("Failed to lock settings: {error}"))?;
        cfg.api_key = approved.api_key.clone();
        cfg.user_id = approved.user_id.clone();
        cfg.user_email = approved.email.clone();
        cfg.tier = approved.tier.clone().unwrap_or_default();
        cfg.remote_config()
    };

    let path = config::default_api_config_path();
    let remote_to_save = remote.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(error) = config::ensure_global_prompts_dir() {
            eprintln!("[CONFIG] Failed to ensure global prompts directory: {error}");
        }
        config::save_remote_ai_config(&path, &remote_to_save)
    })
    .await
    .map_err(|error| format!("save remote ai settings task failed: {error}"))??;

    state.warmup_client.update_credentials(
        config::default_blade_url(),
        approved.api_key.clone(),
        approved.user_id.clone(),
    );
    state
        .ws_connection
        .update_credentials(config::default_blade_url(), approved.api_key)
        .await;

    if let Ok(mut user_id) = state.user_id.lock() {
        *user_id = Some(approved.user_id.clone());
    }

    Ok(SsoLoginResult {
        user_id: approved.user_id,
        email: approved.email,
        tier: approved.tier,
    })
}

fn emit_progress(app: &tauri::AppHandle, payload: SsoProgressEvent) {
    let _ = app.emit("sso-login-progress", payload);
}
