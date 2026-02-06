use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureResult {
    pub data: String,
    pub width: u32,
    pub height: u32,
    pub mime_type: String,
}

fn auto_crop_black(image: image::DynamicImage) -> image::DynamicImage {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return image;
    }
    let rgba = image.to_rgba8();
    let is_black = |x: u32, y: u32| -> bool {
        let p = rgba.get_pixel(x, y);
        p[0] == 0 && p[1] == 0 && p[2] == 0
    };
    // Find right boundary (scan from right)
    let mut right = w;
    'outer_r: for x in (0..w).rev() {
        for y in (0..h).step_by(4) {
            if !is_black(x, y) {
                right = (x + 1).min(w);
                break 'outer_r;
            }
        }
    }
    // Find bottom boundary (scan from bottom)
    let mut bottom = h;
    'outer_b: for y in (0..h).rev() {
        for x in (0..right).step_by(4) {
            if !is_black(x, y) {
                bottom = (y + 1).min(h);
                break 'outer_b;
            }
        }
    }
    // Only crop if we'd remove a significant black border (>10% of image)
    if right < w * 9 / 10 || bottom < h * 9 / 10 {
        // Ensure minimum size
        let crop_w = right.max(1);
        let crop_h = bottom.max(1);
        return image.crop_imm(0, 0, crop_w, crop_h);
    }
    image
}

fn encode_png(image: image::DynamicImage) -> Result<CaptureResult, String> {
    let (width, height) = image.dimensions();
    let mut png_bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(CaptureResult {
        data: BASE64.encode(png_bytes),
        width,
        height,
        mime_type: "image/png".to_string(),
    })
}

pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    let windows = xcap::Window::all().map_err(|e| e.to_string())?;
    Ok(windows
        .into_iter()
        .filter(|w| {
            !w.is_minimized()
                && w.width() > 50
                && w.height() > 50
                && !(w.title().is_empty() && w.app_name().is_empty())
        })
        .map(|window| WindowInfo {
            id: window.id(),
            title: window.title().to_string(),
            app_name: window.app_name().to_string(),
            x: window.x(),
            y: window.y(),
            width: window.width(),
            height: window.height(),
        })
        .collect())
}

pub fn capture_window(window_id: u32) -> Result<CaptureResult, String> {
    let windows = xcap::Window::all().map_err(|e| e.to_string())?;
    let window = windows
        .into_iter()
        .find(|w| w.id() == window_id)
        .ok_or_else(|| "Window not found".to_string())?;
    let image = window.capture_image().map_err(|e| e.to_string())?;
    let dyn_image = auto_crop_black(image::DynamicImage::ImageRgba8(image));
    encode_png(dyn_image)
}

pub fn capture_full_screen() -> Result<CaptureResult, String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let monitor = monitors
        .into_iter()
        .next()
        .ok_or_else(|| "No monitors available".to_string())?;
    let image = monitor.capture_image().map_err(|e| e.to_string())?;
    encode_png(image::DynamicImage::ImageRgba8(image))
}

pub fn capture_window_region(window_id: u32, x: u32, y: u32, width: u32, height: u32) -> Result<CaptureResult, String> {
    let windows = xcap::Window::all().map_err(|e| e.to_string())?;
    let window = windows
        .into_iter()
        .find(|w| w.id() == window_id)
        .ok_or_else(|| "Window not found".to_string())?;
    let image = window.capture_image().map_err(|e| e.to_string())?;
    let dyn_image = auto_crop_black(image::DynamicImage::ImageRgba8(image));
    let (img_width, img_height) = dyn_image.dimensions();
    let safe_x = x.min(img_width.saturating_sub(1));
    let safe_y = y.min(img_height.saturating_sub(1));
    let safe_width = width.min(img_width.saturating_sub(safe_x));
    let safe_height = height.min(img_height.saturating_sub(safe_y));
    let cropped = dyn_image.crop_imm(safe_x, safe_y, safe_width, safe_height);
    encode_png(cropped)
}

pub fn capture_screen_region(x: u32, y: u32, width: u32, height: u32) -> Result<CaptureResult, String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let monitor = monitors
        .into_iter()
        .next()
        .ok_or_else(|| "No monitors available".to_string())?;
    let image = monitor.capture_image().map_err(|e| e.to_string())?;
    let dyn_image = image::DynamicImage::ImageRgba8(image);
    let (img_width, img_height) = dyn_image.dimensions();
    let max_x = img_width.saturating_sub(1);
    let max_y = img_height.saturating_sub(1);
    let safe_x = x.min(max_x);
    let safe_y = y.min(max_y);
    let safe_width = width.min(img_width.saturating_sub(safe_x));
    let safe_height = height.min(img_height.saturating_sub(safe_y));
    let cropped = dyn_image.crop_imm(safe_x, safe_y, safe_width, safe_height);
    encode_png(cropped)
}
