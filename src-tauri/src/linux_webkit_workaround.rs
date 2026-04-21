#[cfg(target_os = "linux")]
fn env_truthy(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

#[cfg(target_os = "linux")]
fn session_type() -> Option<String> {
    std::env::var("XDG_SESSION_TYPE")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "linux")]
fn has_proprietary_nvidia_module() -> bool {
    std::path::Path::new("/sys/module/nvidia").exists()
}

#[cfg(target_os = "linux")]
fn primary_gpu_is_nvidia() -> bool {
    let drm_dir = match std::fs::read_dir("/sys/class/drm") {
        Ok(entries) => entries,
        Err(_) => return false,
    };

    for entry in drm_dir.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };

        if !name.starts_with("card") || name.contains('-') {
            continue;
        }

        let vendor = std::fs::read_to_string(path.join("device/vendor")).ok();
        let is_nvidia = matches!(vendor.as_deref().map(str::trim), Some("0x10de"));
        if !is_nvidia {
            continue;
        }

        let boot_vga = std::fs::read_to_string(path.join("device/boot_vga")).ok();
        let boot_display = std::fs::read_to_string(path.join("device/boot_display")).ok();
        let is_primary = matches!(boot_vga.as_deref().map(str::trim), Some("1"))
            || matches!(boot_display.as_deref().map(str::trim), Some("1"));

        if is_primary {
            return true;
        }
    }

    false
}

#[cfg(target_os = "linux")]
fn should_apply_webkit_workaround() -> bool {
    if env_truthy("ZBLADE_SKIP_LINUX_WEBKIT_WORKAROUND") {
        return false;
    }

    if !has_proprietary_nvidia_module() {
        return false;
    }

    primary_gpu_is_nvidia()
}

#[cfg(target_os = "linux")]
fn set_env_if_unset(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        std::env::set_var(key, value);
    }
}

#[cfg(target_os = "linux")]
pub fn apply() {
    if !should_apply_webkit_workaround() {
        return;
    }

    let session = session_type().unwrap_or_else(|| String::from("unknown"));

    if session == "x11" {
        set_env_if_unset("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        set_env_if_unset("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        eprintln!(
            "[LINUX WEBKIT] Applied X11/NVIDIA WebKitGTK workaround: WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1"
        );
        return;
    }

    if session == "wayland" {
        set_env_if_unset("__NV_DISABLE_EXPLICIT_SYNC", "1");
        eprintln!(
            "[LINUX WEBKIT] Applied Wayland/NVIDIA workaround: __NV_DISABLE_EXPLICIT_SYNC=1"
        );
        return;
    }

    set_env_if_unset("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    set_env_if_unset("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    eprintln!(
        "[LINUX WEBKIT] Applied fallback NVIDIA workaround for unknown session type: WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1"
    );
}

#[cfg(not(target_os = "linux"))]
pub fn apply() {}
