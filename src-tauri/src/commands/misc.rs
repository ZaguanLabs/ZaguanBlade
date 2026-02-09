// use crate::app_state::AppState;
// use tauri::{AppHandle, Manager, State};
use tauri::AppHandle;
#[cfg(feature = "devtools")]
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

// Virtual Buffer Management Commands - Removed
