use crate::app_state::AppState;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
pub fn toggle_devtools(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(debug_assertions)]
        {
            if window.is_devtools_open() {
                window.close_devtools();
            } else {
                window.open_devtools();
            }
        }
        #[cfg(not(debug_assertions))]
        {
            // In production builds with devtools feature enabled
            if window.is_devtools_open() {
                window.close_devtools();
            } else {
                window.open_devtools();
            }
        }
    }
}

#[tauri::command]
pub fn log_frontend(message: String) {
    println!("[FRONTEND] {}", message);
}

// Virtual Buffer Management Commands

#[tauri::command]
pub fn set_virtual_buffer(
    path: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut virtual_buffers = state.virtual_buffers.lock().unwrap();
    virtual_buffers.insert(path.clone(), content);
    println!("[VIRTUAL BUFFER] Set virtual content for: {}", path);
    Ok(())
}

#[tauri::command]
pub fn clear_virtual_buffer(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut virtual_buffers = state.virtual_buffers.lock().unwrap();
    virtual_buffers.remove(&path);
    println!("[VIRTUAL BUFFER] Cleared virtual content for: {}", path);
    Ok(())
}

#[tauri::command]
pub fn has_virtual_buffer(path: String, state: State<'_, AppState>) -> bool {
    let virtual_buffers = state.virtual_buffers.lock().unwrap();
    virtual_buffers.contains_key(&path)
}

#[tauri::command]
pub fn get_virtual_files(state: State<'_, AppState>) -> Vec<String> {
    let virtual_buffers = state.virtual_buffers.lock().unwrap();
    virtual_buffers.keys().cloned().collect()
}
