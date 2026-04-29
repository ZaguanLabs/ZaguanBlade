use crate::screenshot;

#[tauri::command]
pub async fn list_capturable_windows() -> Result<Vec<screenshot::WindowInfo>, String> {
    tokio::task::spawn_blocking(screenshot::list_windows)
        .await
        .map_err(|e| format!("list capturable windows task failed: {}", e))?
}

#[tauri::command]
pub async fn capture_window(window_id: u32) -> Result<screenshot::CaptureResult, String> {
    tokio::task::spawn_blocking(move || screenshot::capture_window(window_id))
        .await
        .map_err(|e| format!("capture window task failed: {}", e))?
}

#[tauri::command]
pub async fn capture_full_screen() -> Result<screenshot::CaptureResult, String> {
    tokio::task::spawn_blocking(screenshot::capture_full_screen)
        .await
        .map_err(|e| format!("capture full screen task failed: {}", e))?
}

#[tauri::command]
pub async fn capture_window_region(
    window_id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<screenshot::CaptureResult, String> {
    tokio::task::spawn_blocking(move || {
        screenshot::capture_window_region(window_id, x, y, width, height)
    })
    .await
    .map_err(|e| format!("capture window region task failed: {}", e))?
}

#[tauri::command]
pub async fn capture_screen_region(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<screenshot::CaptureResult, String> {
    tokio::task::spawn_blocking(move || screenshot::capture_screen_region(x, y, width, height))
        .await
        .map_err(|e| format!("capture screen region task failed: {}", e))?
}
