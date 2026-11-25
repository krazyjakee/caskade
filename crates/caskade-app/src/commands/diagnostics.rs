//! Diagnostics for Settings → About (#176): opening Caskade's logs folder
//! in the OS file manager, and formatting a privacy-safe system summary for
//! bug reports.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use caskade_core::AppPaths;

use crate::error::{CommandError, ErrorCode, logged};

/// Maximum length of any single sanitized field in the diagnostics summary.
const MAX_FIELD_LEN: usize = 128;

/// Which window renderer is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererMode {
    /// Hardware-accelerated GPU renderer (e.g. `vello_hybrid`).
    Gpu(&'static str),
    /// Software CPU renderer (e.g. `vello_cpu`).
    Cpu(&'static str),
}

impl RendererMode {
    /// Whether the window is drawn on the CPU.
    pub fn is_cpu(self) -> bool {
        matches!(self, Self::Cpu(_))
    }

    /// The renderer backend name (e.g. `vello_hybrid`, `vello_cpu`).
    pub fn backend(self) -> &'static str {
        match self {
            Self::Gpu(name) | Self::Cpu(name) => name,
        }
    }
}

/// Privacy-safe build, OS and renderer facts for bug reports (#176).
/// Contains no filesystem paths, usernames, hostnames, tokens or keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInfo {
    pub version: String,
    pub debug_build: bool,
    pub git_sha: Option<String>,
    pub os: String,
    pub os_version: Option<String>,
    pub arch: String,
    pub renderer: RendererMode,
    pub gpu_adapter: Option<String>,
}

/// Ensures `paths.logs` exists as a directory and returns it, or a clear
/// [`CommandError`] if it cannot be created or is not a directory.
pub(crate) fn prepare_logs_folder(paths: &AppPaths) -> Result<PathBuf, CommandError> {
    let folder = paths.logs.clone();
    if !folder.exists() {
        std::fs::create_dir_all(&folder).map_err(|error| {
            let mut failure = CommandError::from_error(&error);
            failure.summary = caskade_core::tr!(
                "Couldn't create the logs folder (“{folder}”).",
                folder = folder.display()
            );
            logged(failure)
        })?;
    }
    if !folder.is_dir() {
        return Err(logged(CommandError::new(
            ErrorCode::Invalid,
            caskade_core::tr!("The logs folder isn't available on this device."),
        )));
    }
    Ok(folder)
}

/// Opens Caskade's logs folder (`paths.logs`) in the OS file manager (#176).
/// The UI never passes a path; this opens only the app's own logs directory.
pub async fn open_logs_folder(paths: &AppPaths) -> Result<(), CommandError> {
    let folder = prepare_logs_folder(paths)?;
    super::library::open_file_manager([&folder], &folder)
}

/// Formats `info` as a concise, privacy-safe diagnostics block for bug reports.
pub fn format_system_info(info: &SystemInfo) -> String {
    let version = sanitize_field(&info.version).unwrap_or_else(|| "unknown".to_owned());
    let build_kind = if info.debug_build { "Debug" } else { "Release" };
    let build = match info.git_sha.as_deref().and_then(sanitize_sha) {
        Some(sha) => format!("{build_kind} ({sha})"),
        None => build_kind.to_owned(),
    };
    let os = sanitize_field(&info.os).unwrap_or_else(|| "unknown".to_owned());
    let arch = sanitize_field(&info.arch).unwrap_or_else(|| "unknown".to_owned());
    let os_line = match info.os_version.as_deref().and_then(sanitize_field) {
        Some(os_version) => format!("{os} ({arch}) — {os_version}"),
        None => format!("{os} ({arch})"),
    };
    let mode_kind = if info.renderer.is_cpu() { "CPU" } else { "GPU" };
    let backend = info.renderer.backend();
    let renderer_line = match info.gpu_adapter.as_deref().and_then(sanitize_field) {
        Some(adapter) if !info.renderer.is_cpu() => {
            format!("{mode_kind} ({backend}) — {adapter}")
        }
        _ => format!("{mode_kind} ({backend})"),
    };
    format!("Caskade: {version}\nBuild: {build}\nOS: {os_line}\nRenderer: {renderer_line}")
}

/// Cheaply reads a privacy-safe OS version label once per process without
/// spawning subprocesses (`None` when unavailable on this platform).
pub fn detect_os_version() -> Option<String> {
    static OS_VERSION: OnceLock<Option<String>> = OnceLock::new();
    OS_VERSION
        .get_or_init(|| {
            #[cfg(target_os = "linux")]
            {
                std::fs::read_to_string("/etc/os-release")
                    .ok()
                    .and_then(|text| parse_os_release(&text))
            }
            #[cfg(not(target_os = "linux"))]
            {
                None
            }
        })
        .clone()
}

/// Cheaply reads a privacy-safe GPU product name once per process without
/// spawning subprocesses (`None` when unavailable).
pub fn detect_gpu_adapter() -> Option<String> {
    static GPU_ADAPTER: OnceLock<Option<String>> = OnceLock::new();
    GPU_ADAPTER
        .get_or_init(|| {
            #[cfg(target_os = "linux")]
            {
                read_drm_adapter_name(Path::new("/sys/class/drm"))
            }
            #[cfg(not(target_os = "linux"))]
            {
                None
            }
        })
        .clone()
}

/// Parses `PRETTY_NAME` (or `NAME` + `VERSION_ID`) from an `/etc/os-release`
/// file, keeping only clean single-line text.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_os_release(contents: &str) -> Option<String> {
    let mut pretty = None;
    let mut name = None;
    let mut version_id = None;
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unquote_os_release(value.trim());
        match key.trim() {
            "PRETTY_NAME" => pretty = sanitize_field(&value),
            "NAME" => name = sanitize_field(&value),
            "VERSION_ID" => version_id = sanitize_field(&value),
            _ => {}
        }
    }
    pretty.or_else(|| match (name, version_id) {
        (Some(name), Some(version)) => sanitize_field(&format!("{name} {version}")),
        (Some(name), None) => Some(name),
        _ => None,
    })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn unquote_os_release(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value);
    inner.replace("\\\"", "\"").replace("\\\\", "\\")
}

/// Reads the first non-empty `device/product_name` under a Linux DRM directory
/// (`/sys/class/drm`), ignoring connector entries (`card0-DP-1`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn read_drm_adapter_name(drm: &Path) -> Option<String> {
    let entries = std::fs::read_dir(drm).ok()?;
    let mut cards: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let text = name.to_str()?;
            let number = text.strip_prefix("card")?;
            (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then(|| entry.path())
        })
        .collect();
    cards.sort();
    for card in cards {
        if let Ok(product) = std::fs::read_to_string(card.join("device").join("product_name"))
            && let Some(clean) = sanitize_field(&product)
        {
            return Some(clean);
        }
    }
    None
}

fn sanitize_sha(sha: &str) -> Option<String> {
    let trimmed = sha.trim();
    if !trimmed.is_empty() && trimmed.len() <= 16 && trimmed.chars().all(|c| c.is_ascii_hexdigit())
    {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

fn sanitize_field(value: &str) -> Option<String> {
    let cleaned: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(MAX_FIELD_LEN).collect())
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
