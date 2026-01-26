use serde::Serialize;
use std::env;
use std::process::Command;

/// Environment information sent to zcoderd during authentication
#[derive(Debug, Clone, Serialize, Default)]
pub struct EnvironmentInfo {
    /// Operating system: linux, darwin, windows
    pub os: String,
    /// OS version string (e.g., "Ubuntu 22.04", "macOS 14.2")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    /// CPU architecture: amd64, arm64
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// Default shell path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// System package manager: apt, brew, dnf, pacman, winget, choco
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<String>,
    /// User's home directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_dir: Option<String>,
    /// Current working directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// Docker is available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_docker: Option<bool>,
    /// Git is available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_git: Option<bool>,
    /// Node.js version if installed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_version: Option<String>,
    /// Python version if installed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python_version: Option<String>,
    /// Go version if installed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub go_version: Option<String>,
    /// Rust version if installed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_version: Option<String>,
    /// Editor/IDE name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
}

impl EnvironmentInfo {
    /// Collect environment information from the system
    pub fn collect() -> Self {
        let os = detect_os();
        let arch = detect_arch();
        let os_version = detect_os_version();
        let shell = detect_shell();
        let package_manager = detect_package_manager();
        let home_dir = dirs::home_dir().map(|p| p.to_string_lossy().to_string());
        let working_dir = env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string());

        // Check for available tools
        let has_git = check_command_exists("git");
        let has_docker = check_command_exists("docker");

        // Get versions (these are quick operations)
        let node_version = get_version("node", &["-v"]);
        let python_version = get_version("python3", &["--version"])
            .or_else(|| get_version("python", &["--version"]));
        let go_version = get_version("go", &["version"]);
        let rust_version = get_version("rustc", &["--version"]);

        EnvironmentInfo {
            os,
            os_version,
            arch,
            shell,
            package_manager,
            home_dir,
            working_dir,
            has_docker: Some(has_docker),
            has_git: Some(has_git),
            node_version,
            python_version,
            go_version,
            rust_version,
            editor: Some("zblade".to_string()),
        }
    }
}

/// Detect the operating system
fn detect_os() -> String {
    #[cfg(target_os = "linux")]
    {
        "linux".to_string()
    }
    #[cfg(target_os = "macos")]
    {
        "darwin".to_string()
    }
    #[cfg(target_os = "windows")]
    {
        "windows".to_string()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        env::consts::OS.to_string()
    }
}

/// Detect CPU architecture
fn detect_arch() -> Option<String> {
    let arch = env::consts::ARCH;
    let normalized = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => arch,
    };
    Some(normalized.to_string())
}

/// Detect OS version
fn detect_os_version() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        // Try to read /etc/os-release
        if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
            for line in content.lines() {
                if line.starts_with("PRETTY_NAME=") {
                    let value = line.trim_start_matches("PRETTY_NAME=");
                    let value = value.trim_matches('"');
                    return Some(value.to_string());
                }
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        // Use sw_vers to get macOS version
        if let Ok(output) = Command::new("sw_vers").arg("-productVersion").output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Some(format!("macOS {}", version));
            }
        }
        None
    }
    #[cfg(target_os = "windows")]
    {
        // Use systeminfo or registry
        if let Ok(output) = Command::new("cmd")
            .args(["/C", "ver"])
            .output()
        {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Some(version);
            }
        }
        None
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Detect the default shell
fn detect_shell() -> Option<String> {
    #[cfg(unix)]
    {
        env::var("SHELL").ok()
    }
    #[cfg(windows)]
    {
        env::var("COMSPEC").ok().or_else(|| Some("powershell".to_string()))
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// Detect the system package manager
fn detect_package_manager() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if check_command_exists("brew") {
            return Some("brew".to_string());
        }
        None
    }
    #[cfg(target_os = "windows")]
    {
        if check_command_exists("winget") {
            return Some("winget".to_string());
        }
        if check_command_exists("choco") {
            return Some("choco".to_string());
        }
        None
    }
    #[cfg(target_os = "linux")]
    {
        // Check common package managers in order of popularity
        if check_command_exists("apt-get") {
            return Some("apt".to_string());
        }
        if check_command_exists("dnf") {
            return Some("dnf".to_string());
        }
        if check_command_exists("yum") {
            return Some("yum".to_string());
        }
        if check_command_exists("pacman") {
            return Some("pacman".to_string());
        }
        if check_command_exists("zypper") {
            return Some("zypper".to_string());
        }
        if check_command_exists("apk") {
            return Some("apk".to_string());
        }
        None
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Check if a command exists in PATH
fn check_command_exists(cmd: &str) -> bool {
    #[cfg(unix)]
    {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        Command::new("where")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// Get version string from a command
fn get_version<I, S>(cmd: &str, args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new(cmd).args(args).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let version_str = stdout.trim();

    // Extract version number from common formats
    // "v20.10.0" -> "20.10.0"
    // "Python 3.11.4" -> "3.11.4"
    // "go version go1.22.0 linux/amd64" -> "1.22.0"
    // "rustc 1.75.0 (82e1608df 2023-12-21)" -> "1.75.0"

    let version = extract_version(version_str)?;
    Some(version)
}

/// Extract version number from various output formats
fn extract_version(s: &str) -> Option<String> {
    // Common patterns:
    // v20.10.0
    // Python 3.11.4
    // go version go1.22.0 linux/amd64
    // rustc 1.75.0 (82e1608df 2023-12-21)
    // node v20.10.0

    // Try to find a version pattern like X.Y.Z or vX.Y.Z
    let re = regex::Regex::new(r"v?(\d+\.\d+(?:\.\d+)?)").ok()?;
    let caps = re.captures(s)?;
    caps.get(1).map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_os() {
        let os = detect_os();
        assert!(!os.is_empty());
        #[cfg(target_os = "linux")]
        assert_eq!(os, "linux");
        #[cfg(target_os = "macos")]
        assert_eq!(os, "darwin");
        #[cfg(target_os = "windows")]
        assert_eq!(os, "windows");
    }

    #[test]
    fn test_detect_arch() {
        let arch = detect_arch();
        assert!(arch.is_some());
        let arch = arch.unwrap();
        assert!(arch == "amd64" || arch == "arm64" || !arch.is_empty());
    }

    #[test]
    fn test_extract_version() {
        assert_eq!(extract_version("v20.10.0"), Some("20.10.0".to_string()));
        assert_eq!(
            extract_version("Python 3.11.4"),
            Some("3.11.4".to_string())
        );
        assert_eq!(
            extract_version("go version go1.22.0 linux/amd64"),
            Some("1.22.0".to_string())
        );
        assert_eq!(
            extract_version("rustc 1.75.0 (82e1608df 2023-12-21)"),
            Some("1.75.0".to_string())
        );
        assert_eq!(
            extract_version("node v20.10.0"),
            Some("20.10.0".to_string())
        );
    }

    #[test]
    fn test_collect_environment() {
        let env = EnvironmentInfo::collect();
        assert!(!env.os.is_empty());
        assert!(env.arch.is_some());
        assert!(env.home_dir.is_some());
    }
}
