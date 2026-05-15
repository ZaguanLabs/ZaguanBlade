// use crate::app_state::AppState;
// use tauri::{AppHandle, Manager, State};
use std::collections::HashMap;

use serde_json::Value;
use tauri::AppHandle;
use tauri::Manager;

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
pub fn toggle_devtools(app: AppHandle) {
    #[cfg(feature = "devtools")]
    {
        if let Some(window) = app.get_webview_window("main") {
            if window.is_devtools_open() {
                window.close_devtools();
            } else {
                window.open_devtools();
            }
        }
    }
    #[cfg(not(feature = "devtools"))]
    {
        let _ = app;
    }
}

#[tauri::command]
pub fn log_frontend(message: String) {
    println!("[FRONTEND] {}", message);
}

#[tauri::command]
pub fn frontend_shell_ready(app: AppHandle) {
    crate::startup::ensure_post_ui_startup(&app);
}

#[tauri::command]
pub fn set_window_title(app: AppHandle, title: String) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    window.set_title(&title).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_blade_event_metrics() -> Value {
    crate::blade_event_scheduler::metrics_snapshot()
}

#[tauri::command]
pub fn get_runtime_debug_flags() -> HashMap<String, String> {
    fn read_bool_env(key: &str) -> Option<String> {
        match std::env::var(key)
            .ok()?
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "1" | "true" => Some(String::from("true")),
            "0" | "false" => Some(String::from("false")),
            _ => None,
        }
    }

    [
        ("ZBLADE_DISABLE_TERMINAL", "disableTERMINAL"),
        ("ZBLADE_DISABLE_EDITOR", "disableEDITOR"),
        ("ZBLADE_DISABLE_CHAT", "disableCHAT"),
        ("ZBLADE_BLANK_APP", "blankApp"),
        ("ZBLADE_DISABLE_CORE_STATE_SYNC", "disableCoreStateSync"),
        ("ZBLADE_MINIMAL_LAYOUT", "minimalLayout"),
        ("ZBLADE_DISABLE_CHAT_HOOK", "disableChatHook"),
        ("ZBLADE_DISABLE_GIT_STATUS", "disableGitStatus"),
        (
            "ZBLADE_DISABLE_UNCOMMITTED_CHANGES",
            "disableUncommittedChanges",
        ),
        ("ZBLADE_DISABLE_LAYOUT_EVENTS", "disableLayoutEvents"),
        ("ZBLADE_DISABLE_PROJECT_STATE", "disableProjectState"),
        ("ZBLADE_DISABLE_WARMUP", "disableWarmup"),
        ("ZBLADE_DISABLE_TAB_MANAGER", "disableTabManager"),
        ("ZBLADE_DISABLE_ACTIVITY_BAR", "disableActivityBar"),
        ("ZBLADE_DISABLE_SIDEBAR_OVERLAY", "disableSidebarOverlay"),
        ("ZBLADE_DISABLE_EDITOR_CHROME", "disableEditorChrome"),
        ("ZBLADE_DISABLE_CHAT_CHROME", "disableChatChrome"),
        (
            "ZBLADE_DISABLE_EDITOR_WIDTH_OBSERVER",
            "disableEditorWidthObserver",
        ),
        ("ZBLADE_DEBUG_PERF", "debugPerf"),
    ]
    .into_iter()
    .filter_map(|(env_key, flag_key)| {
        read_bool_env(env_key).map(|value| (String::from(flag_key), value))
    })
    .collect()
}

// Virtual Buffer Management Commands - Removed
