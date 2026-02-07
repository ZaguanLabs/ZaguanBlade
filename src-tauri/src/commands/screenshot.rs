use crate::screenshot;

#[tauri::command]
pub fn list_capturable_windows() -> Result<Vec<screenshot::WindowInfo>, String> {
    screenshot::list_windows()
}

#[tauri::command]
pub fn capture_window(window_id: u32) -> Result<screenshot::CaptureResult, String> {
    screenshot::capture_window(window_id)
}

#[tauri::command]
pub fn capture_full_screen() -> Result<screenshot::CaptureResult, String> {
    screenshot::capture_full_screen()
}

#[tauri::command]
pub fn capture_window_region(
    window_id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<screenshot::CaptureResult, String> {
    screenshot::capture_window_region(window_id, x, y, width, height)
}

#[tauri::command]
pub fn capture_screen_region(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<screenshot::CaptureResult, String> {
    screenshot::capture_screen_region(x, y, width, height)
}
