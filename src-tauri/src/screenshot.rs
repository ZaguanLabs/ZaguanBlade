use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Cursor;
use std::process::Command;

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

/// Get the current X11 desktop number via EWMH _NET_CURRENT_DESKTOP.
/// Returns None on non-X11 or if xprop is unavailable (graceful fallback).
fn get_current_desktop() -> Option<u32> {
    let output = Command::new("xprop")
        .args(["-root", "_NET_CURRENT_DESKTOP"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // Output format: "_NET_CURRENT_DESKTOP(CARDINAL) = 2"
    let text = String::from_utf8_lossy(&output.stdout);
    text.split('=').nth(1)?.trim().parse::<u32>().ok()
}

/// Get the set of X11 window IDs on the current desktop via EWMH.
/// Queries _NET_CLIENT_LIST for all managed windows, then checks each
/// window's _NET_WM_DESKTOP. Returns None if EWMH is unavailable
/// (graceful fallback — all windows shown).
fn get_current_desktop_window_ids() -> Option<HashSet<u32>> {
    let current_desktop = get_current_desktop()?;

    // Get all managed window IDs from root
    let output = Command::new("xprop")
        .args(["-root", "-notype", "_NET_CLIENT_LIST"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // Output: "_NET_CLIENT_LIST = 0xb00003, 0xb00005, 0xf00001, ..."
    let text = String::from_utf8_lossy(&output.stdout);
    let ids_str = text.split('=').nth(1)?;

    let mut on_current = HashSet::new();
    for id_token in ids_str.split(',') {
        let id_token = id_token.trim().trim_start_matches("0x");
        let wid = u32::from_str_radix(id_token, 16).ok();
        let Some(wid) = wid else { continue };

        // Query this window's desktop
        let prop = Command::new("xprop")
            .args(["-id", &format!("0x{:x}", wid), "-notype", "_NET_WM_DESKTOP"])
            .output()
            .ok();
        if let Some(prop) = prop {
            if prop.status.success() {
                let prop_text = String::from_utf8_lossy(&prop.stdout);
                // "_NET_WM_DESKTOP = 2"  or  "_NET_WM_DESKTOP = 0xFFFFFFFF" (sticky)
                if let Some(val_str) = prop_text.split('=').nth(1) {
                    let val_str = val_str.trim();
                    // 0xFFFFFFFF means "sticky" (visible on all desktops)
                    let desktop = if val_str.starts_with("0x") {
                        u32::from_str_radix(val_str.trim_start_matches("0x"), 16).ok()
                    } else {
                        val_str.parse::<u32>().ok()
                    };
                    if let Some(d) = desktop {
                        if d == current_desktop || d == 0xFFFFFFFF {
                            on_current.insert(wid);
                        }
                    }
                }
            }
        }
    }
    Some(on_current)
}

pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    let windows = xcap::Window::all().map_err(|e| e.to_string())?;
    // On X11 with virtual desktops, filter to current workspace only.
    // Falls back to showing all windows if EWMH is unavailable.
    let desktop_filter = get_current_desktop_window_ids();

    Ok(windows
        .into_iter()
        .filter(|w| {
            !w.is_minimized().unwrap_or(true)
                && w.width().unwrap_or(0) > 50
                && w.height().unwrap_or(0) > 50
                && !(w.title().unwrap_or_default().is_empty() && w.app_name().unwrap_or_default().is_empty())
        })
        .filter_map(|window| {
            let id = window.id().ok()?;
            // Filter by current desktop if EWMH info is available
            if let Some(ref allowed) = desktop_filter {
                if !allowed.contains(&id) {
                    return None;
                }
            }
            Some(WindowInfo {
                id,
                title: window.title().unwrap_or_default(),
                app_name: window.app_name().unwrap_or_default(),
                x: window.x().unwrap_or(0),
                y: window.y().unwrap_or(0),
                width: window.width().ok()?,
                height: window.height().ok()?,
            })
        })
        .collect())
}

pub fn capture_window(window_id: u32) -> Result<CaptureResult, String> {
    let windows = xcap::Window::all().map_err(|e| e.to_string())?;
    let window = windows
        .into_iter()
        .find(|w| w.id().ok() == Some(window_id))
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
        .find(|w| w.id().ok() == Some(window_id))
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
