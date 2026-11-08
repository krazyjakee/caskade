//! WebKitGTK paints every frame on the CPU on NVIDIA's proprietary driver
//! under X11: it turns its DMA-BUF renderer off for that driver, and X11 has
//! no other GPU path. A fullscreen console window then drops to ~15 fps at
//! 1440p. Its GPU renderer does work there when frames come back through
//! shared memory (GBM buffers fail), so Caskade asks for that, unless the
//! user picked a renderer themselves.
//!
//! WebKit reads these variables once, and `set_var` is unsafe, so the app
//! restarts itself with them set: same process id, same arguments.
//!
//! The GPU renderer doesn't work with every driver and WebKit (#72: the
//! page froze on NVIDIA 580 with WebKit 2.50). When the main window's page
//! crashes or hangs under Caskade's pick, [`record_gpu_failure`] notes the
//! driver and WebKit, and the app restarts on WebKit's own renderer. It
//! tries the GPU again once either changes.
//!
//! Where WebKit still paints on the CPU, the page is told
//! (`data-paint="cpu"`), and ambient loops pause.

use std::ffi::OsString;

/// WebKit's variables that pick a renderer. Any of them set is the user's
/// choice, unless [`OURS`] says Caskade set them.
const CHOICES: [&str; 5] = [
    "WEBKIT_DISABLE_DMABUF_RENDERER",
    "WEBKIT_FORCE_DMABUF_RENDERER",
    "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
    "WEBKIT_DISABLE_COMPOSITING_MODE",
    "WEBKIT_FORCE_COMPOSITING_MODE",
];

/// Asks for software OpenGL: set by a software-rendering desktop session
/// (Cinnamon's, for one) or by the user.
const SOFTWARE_GL: &str = "LIBGL_ALWAYS_SOFTWARE";

/// The GPU renderer, with frames handed over in shared memory.
#[cfg(any(target_os = "linux", test))]
const GPU_OVER_SHM: [(&str, &str); 2] = [
    ("WEBKIT_FORCE_DMABUF_RENDERER", "1"),
    ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"),
];

/// Set with [`GPU_OVER_SHM`]: the renderer is Caskade's pick, not the
/// user's.
const OURS: &str = "CASKADE_WEBKIT_GPU";

/// What to do about WebKit's renderer before it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Start as things are.
    Keep,
    /// Restart with the GPU renderer.
    Gpu,
    /// Restart without Caskade's pick: it failed on this driver and WebKit.
    Default,
}

/// The plan, given the environment, whether NVIDIA's proprietary driver is
/// loaded and whether the GPU renderer failed with this driver and WebKit.
pub fn plan(var: impl Fn(&str) -> Option<OsString>, nvidia: bool, failed: bool) -> Plan {
    if var(OURS).is_some() {
        return if failed { Plan::Default } else { Plan::Keep };
    }
    if !nvidia
        || failed
        || CHOICES.iter().any(|name| var(name).is_some())
        || software_gl(&var)
        || !on_x11(&var)
    {
        return Plan::Keep;
    }
    Plan::Gpu
}

/// Whether the session asks for software OpenGL.
fn software_gl(var: &impl Fn(&str) -> Option<OsString>) -> bool {
    var(SOFTWARE_GL).is_some_and(|value| !value.is_empty() && value != "0")
}

/// Whether GTK will open the window on X11: it takes Wayland when there is
/// a Wayland display, unless `GDK_BACKEND` puts X11 first.
fn on_x11(var: &impl Fn(&str) -> Option<OsString>) -> bool {
    let first_backend = var("GDK_BACKEND")
        .and_then(|backend| backend.to_str().map(str::to_owned))
        .and_then(|backend| backend.split(',').next().map(str::trim).map(str::to_owned));
    match first_backend.as_deref() {
        Some("x11") => true,
        Some("wayland") => false,
        _ => var("WAYLAND_DISPLAY").is_none() && var("DISPLAY").is_some(),
    }
}

/// The driver and WebKit in use, as one line: NVIDIA's version line and
/// the WebKit library's path, size and modification time. `None` without
/// NVIDIA's driver.
#[cfg(target_os = "linux")]
fn gpu_stack() -> Option<String> {
    use std::io::BufRead;

    let driver = std::fs::read_to_string("/proc/driver/nvidia/version").ok()?;
    let driver = driver
        .lines()
        .next()?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let maps = std::io::BufReader::new(std::fs::File::open("/proc/self/maps").ok()?);
    let webkit = maps
        .lines()
        .map_while(Result::ok)
        .find_map(|line| {
            let path = line.split_whitespace().nth(5)?;
            let name = path.rsplit('/').next()?;
            name.starts_with("libwebkit2gtk-").then(|| path.to_owned())
        })
        .and_then(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            let modified = meta
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            Some(format!("{path} {} {modified}", meta.len()))
        })
        .unwrap_or_default();
    Some(format!("{driver} | {webkit}"))
}

/// Where a failure of the GPU renderer is kept: the [`gpu_stack`] it
/// failed on.
#[cfg(target_os = "linux")]
fn failure_record() -> Option<std::path::PathBuf> {
    caskade_core::AppPaths::standard()
        .ok()
        .map(|paths| paths.cache.join("webkit-gpu-failed"))
}

/// Restarts the app with WebKit's GPU renderer where WebKit would paint on
/// the CPU, or without it where it failed before. Returns only when nothing
/// needs changing or the restart failed.
#[cfg(target_os = "linux")]
pub fn prefer_gpu() {
    use std::os::unix::process::CommandExt;

    let stack = gpu_stack();
    let failed = stack.as_ref().is_some_and(|stack| {
        failure_record()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .is_some_and(|recorded| recorded.trim() == stack)
    });
    let plan = plan(|name| std::env::var_os(name), stack.is_some(), failed);
    if plan == Plan::Keep {
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            tracing::warn!(%error, "can't restart for WebKit's renderer");
            return;
        }
    };
    let mut args = std::env::args_os();
    let mut command = std::process::Command::new(exe);
    if let Some(name) = args.next() {
        command.arg0(name);
    }
    command.args(args);
    if plan == Plan::Gpu {
        tracing::info!("NVIDIA on X11: restarting with WebKit's GPU renderer");
        command.envs(GPU_OVER_SHM.iter().copied()).env(OURS, "1");
    } else {
        tracing::warn!(
            stack = stack.as_deref().unwrap_or(""),
            "WebKit's GPU renderer failed with this driver and WebKit: restarting with WebKit's own"
        );
        for (name, _) in GPU_OVER_SHM {
            command.env_remove(name);
        }
        command.env_remove(OURS);
    }
    let error = command.exec();
    tracing::warn!(%error, "can't restart for WebKit's renderer");
}

#[cfg(not(target_os = "linux"))]
pub fn prefer_gpu() {}

/// The main window's page crashed or hung. Where Caskade picked WebKit's
/// GPU renderer, notes that it fails with this driver and WebKit, and
/// returns true: the next start uses WebKit's own renderer, so the app
/// should restart rather than reload.
#[cfg(target_os = "linux")]
pub fn record_gpu_failure() -> bool {
    if std::env::var_os(OURS).is_none() {
        return false;
    }
    let (Some(stack), Some(file)) = (gpu_stack(), failure_record()) else {
        return false;
    };
    tracing::warn!(%stack, "the page failed on WebKit's GPU renderer");
    if let Err(error) = std::fs::write(&file, format!("{stack}\n")) {
        tracing::warn!(%error, "couldn't record the GPU renderer's failure");
        return false;
    }
    true
}

#[cfg(not(target_os = "linux"))]
pub fn record_gpu_failure() -> bool {
    false
}

/// Sets `data-paint="cpu"` on the page's `<html>` when WebKit paints on the
/// CPU anyway (no GPU compositing for this display or driver). The
/// `paint-cpu` token set then pauses ambient loops, which would otherwise
/// repaint the whole screen every frame.
#[cfg(target_os = "linux")]
pub fn mark_cpu_paint<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt, WebViewExt};

    let page = webview.clone();
    let result = webview.with_webview(move |platform| {
        let policy = platform
            .inner()
            .settings()
            .map(|settings| settings.hardware_acceleration_policy());
        tracing::debug!(?policy, "WebKit's rendering policy");
        if policy != Some(HardwareAccelerationPolicy::Never) {
            return;
        }
        tracing::info!("WebKit paints on the CPU: ambient motion pauses");
        // Not from inside `with_webview`: on the main thread that still
        // holds the webview's lock, and `eval` waiting for it never returns.
        tauri::async_runtime::spawn(async move {
            if let Err(error) = page.eval("document.documentElement.dataset.paint = 'cpu'") {
                tracing::warn!(%error, "can't mark CPU painting");
            }
        });
    });
    if let Err(error) = result {
        tracing::warn!(%error, "can't read WebKit's rendering policy");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn mark_cpu_paint<R: tauri::Runtime>(_webview: &tauri::Webview<R>) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        move |name| {
            vars.iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, value)| value.into())
        }
    }

    #[test]
    fn nvidia_on_x11_gets_the_gpu_renderer() {
        assert_eq!(plan(env(&[("DISPLAY", ":0")]), true, false), Plan::Gpu);
    }

    #[test]
    fn other_drivers_keep_webkits_choice() {
        assert_eq!(plan(env(&[("DISPLAY", ":0")]), false, false), Plan::Keep);
    }

    #[test]
    fn a_users_renderer_choice_wins() {
        for name in CHOICES {
            assert_eq!(
                plan(env(&[("DISPLAY", ":0"), (name, "1")]), true, false),
                Plan::Keep
            );
            // Even when Caskade's pick failed: the user's choice stays.
            assert_eq!(
                plan(env(&[("DISPLAY", ":0"), (name, "1")]), true, true),
                Plan::Keep
            );
        }
    }

    #[test]
    fn a_software_rendering_session_keeps_webkits_choice() {
        let software = env(&[("DISPLAY", ":0"), (SOFTWARE_GL, "1")]);
        assert_eq!(plan(software, true, false), Plan::Keep);
        let off = env(&[("DISPLAY", ":0"), (SOFTWARE_GL, "0")]);
        assert_eq!(plan(off, true, false), Plan::Gpu);
    }

    #[test]
    fn a_gpu_renderer_that_failed_here_is_not_picked_again() {
        assert_eq!(plan(env(&[("DISPLAY", ":0")]), true, true), Plan::Keep);
    }

    #[test]
    fn caskades_pick_is_dropped_once_it_failed() {
        let mut ours = vec![("DISPLAY", ":0"), (OURS, "1")];
        ours.extend(GPU_OVER_SHM);
        assert_eq!(plan(env(&ours), true, false), Plan::Keep);
        assert_eq!(plan(env(&ours), true, true), Plan::Default);
    }

    #[test]
    fn wayland_keeps_webkits_choice() {
        let wayland = env(&[("DISPLAY", ":0"), ("WAYLAND_DISPLAY", "wayland-0")]);
        assert_eq!(plan(wayland, true, false), Plan::Keep);
        let preferred = env(&[
            ("DISPLAY", ":0"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("GDK_BACKEND", "wayland,x11"),
        ]);
        assert_eq!(plan(preferred, true, false), Plan::Keep);
    }

    #[test]
    fn x11_first_in_gdk_backend_counts_as_x11() {
        let forced = env(&[
            ("DISPLAY", ":0"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("GDK_BACKEND", "x11"),
        ]);
        assert_eq!(plan(forced, true, false), Plan::Gpu);
    }

    #[test]
    fn no_display_keeps_webkits_choice() {
        assert_eq!(plan(env(&[]), true, false), Plan::Keep);
    }
}
