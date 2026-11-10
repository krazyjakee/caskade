//! The main window's side of webview recovery (#3): the policy and the
//! restart budget are `caskade_player::recovery`'s; here the main window
//! reloads, restarts Caskade, or asks when restarts keep failing.

pub use caskade_player::recovery::*;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};
/// The main window's recovery state.
struct Recovery {
    policy: Mutex<Policy>,
    restarts: Restarts,
    /// The give-up dialog is open.
    asking: AtomicBool,
    restarting: AtomicBool,
    route: Mutex<Option<String>>,
}

/// Watches the main window's webview and brings it back when its
/// processes fail. `restarts` is the file [`Restarts`] keeps.
pub fn watch<R: Runtime>(window: &WebviewWindow<R>, restarts: PathBuf) {
    window.app_handle().manage(Recovery {
        policy: Mutex::default(),
        restarts: Restarts::new(restarts),
        asking: AtomicBool::new(false),
        restarting: AtomicBool::new(false),
        route: Mutex::new(None),
    });
    #[cfg(windows)]
    {
        let app = window.app_handle().clone();
        let attached = window.with_webview(move |platform| {
            let result =
                caskade_webview2::watch_controller(&platform.controller(), move |failed| {
                    log("main window", &failed);
                    // Off WebView2's callback: a restart waits for the event
                    // loop, and the dialog blocks.
                    let app = app.clone();
                    let failure = failure_of(failed.kind);
                    std::thread::spawn(move || recover(&app, failure, failed.url));
                });
            if let Err(error) = result {
                tracing::warn!(%error, "can't watch the webview's processes");
            }
        });
        if let Err(error) = attached {
            tracing::warn!(%error, "can't watch the webview's processes");
        }
    }
    #[cfg(target_os = "linux")]
    {
        let app = window.app_handle().clone();
        if let Err(error) = window.with_webview(move |platform| {
            attach_webkit(&platform.inner(), move |failed| {
                log("main window", &failed);
                let app = app.clone();
                let failure = failure_of(failed.kind);
                std::thread::spawn(move || recover(&app, failure, failed.url));
            });
        }) {
            tracing::warn!(%error, "can't watch WebKit's web process");
        }
    }
}

fn recover<R: Runtime>(app: &AppHandle<R>, failure: Failure, url: Option<String>) {
    let state = app.state::<Recovery>();
    if state.asking.load(Ordering::Relaxed) || state.restarting.load(Ordering::Relaxed) {
        return;
    }
    if let Some(url) = url.and_then(|url| route(&url)) {
        *state
            .route
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(url);
    }
    let step = state
        .policy
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .next(failure, Instant::now());
    // The page failed on the GPU renderer Caskade picked: restart on
    // WebKit's own rather than reload into the same failure.
    let step = if step != Step::Wait && crate::webkit::record_gpu_failure() {
        Step::Restart
    } else {
        step
    };
    match step {
        Step::Wait => {}
        Step::Reload => {
            tracing::warn!("reloading the main window's page");
            let reloaded = app.get_webview_window("main").map(|window| window.reload());
            if !matches!(reloaded, Some(Ok(()))) {
                tracing::error!("the main window's page couldn't reload");
                restart_or_ask(app, &state);
            }
        }
        Step::Restart => restart_or_ask(app, &state),
    }
}

fn restart_or_ask<R: Runtime>(app: &AppHandle<R>, state: &Recovery) {
    if state.restarting.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(route) = state
        .route
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        && let Some(path) = route_path()
        && let Err(error) = std::fs::write(path, route)
    {
        tracing::warn!(%error, "couldn't keep the route for webview recovery");
    }
    if state.restarts.claim(SystemTime::now()) {
        tracing::warn!("restarting Caskade to bring its window back");
        restart(app);
        return;
    }
    if state.asking.swap(true, Ordering::Relaxed) {
        return;
    }
    tracing::error!("the main window keeps failing: asking to restart or quit");
    ask(app, state.restarts.clone());
}

/// The last resort: a native dialog, since the page can't show one.
fn ask<R: Runtime>(app: &AppHandle<R>, restarts: Restarts) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let handle = app.clone();
    let logs = caskade_core::AppPaths::standard()
        .map(|paths| paths.logs.display().to_string())
        .unwrap_or_else(|_| "logs".into());
    app.dialog()
        .message(caskade_core::tr!(
            "Caskade's window stopped working and couldn't be brought back on its own. Restart Caskade to try again, or quit.\n\nDiagnostic logs: {logs}",
            logs = logs
        ))
        .title("Caskade")
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCancelCustom(
            caskade_core::tr!("Restart Caskade"),
            caskade_core::tr!("Quit"),
        ))
        .show(move |again| {
            if again {
                restarts.record(SystemTime::now());
                restart(&handle);
            } else {
                handle.exit(1);
            }
        });
}

fn route_path() -> Option<PathBuf> {
    caskade_core::AppPaths::standard()
        .ok()
        .map(|paths| paths.cache.join("webview-route"))
}
fn route(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https" | "tauri")
        || !url.path().starts_with('/')
        || url.path().starts_with("//")
        || url.path().contains('\\')
    {
        return None;
    }
    Some(format!(
        "{}{}{}",
        url.path(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default(),
        url.fragment().map(|f| format!("#{f}")).unwrap_or_default()
    ))
}
pub fn take_route() -> Option<String> {
    let path = route_path()?;
    let saved = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(path);
    // Validate as a relative path in an inert origin, including backslashes.
    let parsed = url::Url::parse(&format!("tauri://localhost{saved}")).ok()?;
    if parsed.host_str() != Some("localhost") {
        return None;
    }
    route(parsed.as_str())
}
fn restart<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        use tauri_plugin_window_state::AppHandleExt;
        if !crate::commands::console::is_on(&handle)
            && let Err(error) = handle.save_window_state(crate::window_state::flags())
        {
            tracing::warn!(%error, "couldn't save window before webview recovery");
        }
        handle.request_restart();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_routes_keep_query_and_fragment_and_stay_local() {
        assert_eq!(
            route("http://tauri.localhost/library?filter=installed#x"),
            Some("/library?filter=installed#x".into())
        );
        assert_eq!(route("javascript:alert(1)"), None);
        assert_eq!(route("tauri://localhost//external.example/path"), None);
    }
}
