//! The main window's side of WebKitGTK's renderer choice: the choice
//! itself is `caskade_player::webkit`'s, which every Caskade webview
//! process makes; here the page is told when WebKit paints on the CPU
//! (`data-paint="cpu"`), so ambient loops pause.

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
