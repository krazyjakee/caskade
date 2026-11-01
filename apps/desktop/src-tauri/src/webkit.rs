//! WebKitGTK paints every frame on the CPU on NVIDIA's proprietary driver
//! under X11: it turns its DMA-BUF renderer off for that driver, and X11 has
//! no other GPU path. A fullscreen console window then drops to ~15 fps at
//! 1440p. Its GPU renderer does work there when frames come back through
//! shared memory (GBM buffers fail), so Caskade asks for that, unless the
//! user picked a WebKit renderer themselves.
//!
//! WebKit reads these variables once, and `set_var` is unsafe, so the app
//! restarts itself with them set: same process id, same arguments.
//!
//! Where WebKit still paints on the CPU, the page is told
//! (`data-paint="cpu"`), and ambient loops pause.

use std::ffi::OsString;

/// WebKit's variables that pick a renderer. Any of them set is the user's
/// choice, ours included after the restart.
const CHOICES: [&str; 5] = [
    "WEBKIT_DISABLE_DMABUF_RENDERER",
    "WEBKIT_FORCE_DMABUF_RENDERER",
    "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
    "WEBKIT_DISABLE_COMPOSITING_MODE",
    "WEBKIT_FORCE_COMPOSITING_MODE",
];

/// The GPU renderer, with frames handed over in shared memory.
const GPU_OVER_SHM: [(&str, &str); 2] = [
    ("WEBKIT_FORCE_DMABUF_RENDERER", "1"),
    ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"),
];

/// The variables to start WebKit with, given the environment and whether
/// NVIDIA's proprietary driver is loaded.
pub fn overrides(
    var: impl Fn(&str) -> Option<OsString>,
    nvidia: bool,
) -> &'static [(&'static str, &'static str)] {
    if !nvidia || CHOICES.iter().any(|name| var(name).is_some()) || !on_x11(&var) {
        return &[];
    }
    &GPU_OVER_SHM
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

/// Restarts the app with WebKit's GPU renderer where WebKit would paint on
/// the CPU. Returns only when nothing needs changing or the restart failed.
#[cfg(target_os = "linux")]
pub fn prefer_gpu() {
    use std::os::unix::process::CommandExt;

    let nvidia = std::path::Path::new("/proc/driver/nvidia/version").exists();
    let overrides = overrides(|name| std::env::var_os(name), nvidia);
    if overrides.is_empty() {
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            tracing::warn!(%error, "can't restart with WebKit's GPU renderer");
            return;
        }
    };
    tracing::info!("NVIDIA on X11: restarting with WebKit's GPU renderer");
    let mut args = std::env::args_os();
    let mut command = std::process::Command::new(exe);
    if let Some(name) = args.next() {
        command.arg0(name);
    }
    let error = command.args(args).envs(overrides.iter().copied()).exec();
    tracing::warn!(%error, "can't restart with WebKit's GPU renderer");
}

#[cfg(not(target_os = "linux"))]
pub fn prefer_gpu() {}

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
        assert_eq!(overrides(env(&[("DISPLAY", ":0")]), true), GPU_OVER_SHM);
    }

    #[test]
    fn other_drivers_keep_webkits_choice() {
        assert!(overrides(env(&[("DISPLAY", ":0")]), false).is_empty());
    }

    #[test]
    fn a_users_renderer_choice_wins() {
        for name in CHOICES {
            assert!(overrides(env(&[("DISPLAY", ":0"), (name, "1")]), true).is_empty());
        }
    }

    #[test]
    fn wayland_keeps_webkits_choice() {
        let wayland = env(&[("DISPLAY", ":0"), ("WAYLAND_DISPLAY", "wayland-0")]);
        assert!(overrides(wayland, true).is_empty());
        let preferred = env(&[
            ("DISPLAY", ":0"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("GDK_BACKEND", "wayland,x11"),
        ]);
        assert!(overrides(preferred, true).is_empty());
    }

    #[test]
    fn x11_first_in_gdk_backend_counts_as_x11() {
        let forced = env(&[
            ("DISPLAY", ":0"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("GDK_BACKEND", "x11"),
        ]);
        assert_eq!(overrides(forced, true), GPU_OVER_SHM);
    }

    #[test]
    fn no_display_keeps_webkits_choice() {
        assert!(overrides(env(&[]), true).is_empty());
    }
}
