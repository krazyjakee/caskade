//! Bringing a window's page back when the webview loses its processes
//! (#3). WebView2 runs each page in processes of its own, and they can go
//! away under the app: a crash, running out of memory, a GPU driver reset,
//! or an installer or Windows update closing them. Without this the window
//! stays white for good.
//!
//! - The page's renderer exited, or hung for [`HANG_GRACE`]: reload the
//!   page. The core still has everything, so the UI comes back where it was.
//! - The WebView2 browser process exited: the webview is dead and can't be
//!   rebuilt in place, so the main window restarts Caskade.
//! - Reloads that keep failing ([`RELOADS`] within [`RELOAD_WINDOW`]) turn
//!   into a restart. Restarts are counted across processes, in a file
//!   ([`Restarts`]): past [`RESTARTS`] within [`RESTART_WINDOW`] a native
//!   dialog asks to restart or quit instead of looping.
//!
//! The game and video windows (`web_player`, `video_player`) follow the
//! same [`Policy`], reloading or closing themselves.
//!
//! WebView2 reports failures on Windows; WebKitGTK reports its web process
//! exiting or going unresponsive on Linux. A main-window failure on the GPU
//! renderer Caskade picked (`webkit.rs`) restarts on WebKit's own instead
//! of reloading (#72).

// Elsewhere nothing reports a failure, so recovery never runs.
#![cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

/// Renderer failures a page survives by reloading, within [`RELOAD_WINDOW`].
pub const RELOADS: usize = 3;
pub const RELOAD_WINDOW: Duration = Duration::from_secs(120);
/// How long a renderer stays unresponsive before the page reloads: a long
/// task (a big library rendering, a heavy game) gets this long to finish.
pub const HANG_GRACE: Duration = Duration::from_secs(20);
/// Unresponsive reports further apart than this start a new hang.
const HANG_GAP: Duration = Duration::from_secs(30);
/// Restarts Caskade makes on its own within [`RESTART_WINDOW`].
pub const RESTARTS: usize = 2;
pub const RESTART_WINDOW: Duration = Duration::from_secs(600);

/// What went wrong with a page's processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The page's renderer exited.
    RendererExited,
    /// The page's renderer stopped answering.
    RendererUnresponsive,
    /// The browser process exited: the webview is gone.
    BrowserExited,
    /// A process the webview recovers from by itself (GPU, utility, a
    /// frame's renderer).
    Other,
}

/// What to do about a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing yet: the webview recovers by itself, or a hang may pass.
    Wait,
    /// Reload the page.
    Reload,
    /// The page can't come back in this webview.
    Restart,
}

/// Decides each failure's [`Step`], one window's worth.
#[derive(Debug, Default)]
pub struct Policy {
    reloads: Vec<Instant>,
    /// When the current hang was first reported, and last.
    hang: Option<(Instant, Instant)>,
}

impl Policy {
    pub fn next(&mut self, failure: Failure, now: Instant) -> Step {
        match failure {
            Failure::Other => Step::Wait,
            Failure::BrowserExited => Step::Restart,
            Failure::RendererExited => self.reload(now),
            Failure::RendererUnresponsive => {
                let since = match self.hang {
                    Some((since, last)) if now.saturating_duration_since(last) <= HANG_GAP => since,
                    _ => now,
                };
                self.hang = Some((since, now));
                if now.saturating_duration_since(since) < HANG_GRACE {
                    Step::Wait
                } else {
                    self.reload(now)
                }
            }
        }
    }

    fn reload(&mut self, now: Instant) -> Step {
        self.hang = None;
        self.reloads
            .retain(|at| now.saturating_duration_since(*at) < RELOAD_WINDOW);
        if self.reloads.len() >= RELOADS {
            return Step::Restart;
        }
        self.reloads.push(now);
        Step::Reload
    }
}

/// Caskade's own restarts after losing its webview, kept in a file so a
/// webview that dies at every start doesn't restart forever.
#[derive(Debug, Clone)]
pub struct Restarts {
    file: PathBuf,
}

impl Restarts {
    pub fn new(file: PathBuf) -> Self {
        Self { file }
    }

    /// Records a restart at `now` and says whether it may go ahead.
    pub fn claim(&self, now: SystemTime) -> bool {
        let now = millis(now);
        let mut recent = recent(
            &std::fs::read_to_string(&self.file).unwrap_or_default(),
            now,
        );
        if recent.len() >= RESTARTS {
            return false;
        }
        recent.push(now);
        self.write(&recent)
    }

    /// Records a restart the user asked for: it always goes ahead.
    pub fn record(&self, now: SystemTime) {
        let now = millis(now);
        let mut recent = recent(
            &std::fs::read_to_string(&self.file).unwrap_or_default(),
            now,
        );
        recent.push(now);
        let _ = self.write(&recent);
    }

    fn write(&self, times: &[u64]) -> bool {
        let text: String = times.iter().map(|at| format!("{at}\n")).collect();
        if let Err(error) = std::fs::write(&self.file, text) {
            tracing::warn!(%error, "couldn't record the webview restart");
            return false;
        }
        true
    }
}

/// The restarts in `text` (one Unix time in milliseconds per line) within
/// [`RESTART_WINDOW`] of `now`.
fn recent(text: &str, now: u64) -> Vec<u64> {
    let window = RESTART_WINDOW.as_millis() as u64;
    text.lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .filter(|at| *at <= now && now - at < window)
        .collect()
}

fn millis(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

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

/// The [`Failure`] a WebView2 process failure is.
pub fn failure_of(kind: caskade_webview2::Kind) -> Failure {
    use caskade_webview2::Kind;
    match kind {
        Kind::RendererExited => Failure::RendererExited,
        Kind::RendererUnresponsive => Failure::RendererUnresponsive,
        Kind::BrowserExited => Failure::BrowserExited,
        Kind::FrameRendererExited | Kind::GpuExited | Kind::UtilityExited | Kind::Other => {
            Failure::Other
        }
    }
}

/// Logs a webview process failure in `window`, with what WebView2 says.
pub fn log(window: &str, failure: &caskade_webview2::ProcessFailure) {
    tracing::error!(
        window,
        pid = std::process::id(),
        kind = failure.kind.as_str(),
        reason = failure.reason,
        exit_code = failure.exit_code,
        process = failure.process.as_deref().unwrap_or(""),
        "a webview process failed"
    );
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
#[cfg(target_os = "linux")]
fn attach_webkit(
    view: &webkit2gtk::WebView,
    notify: impl Fn(caskade_webview2::ProcessFailure) + 'static,
) {
    use webkit2gtk::WebViewExt;
    use webkit2gtk::glib::object::ObjectExt;
    let notify = std::rc::Rc::new(notify);
    let exited = notify.clone();
    view.connect_web_process_terminated(move |view, reason| {
        exited(caskade_webview2::ProcessFailure {
            kind: caskade_webview2::Kind::RendererExited,
            reason: "WebKit web process terminated",
            exit_code: None,
            process: Some(format!("{reason:?}")),
            url: view.uri().map(|uri| uri.to_string()),
        });
    });
    let hung = move |view: &webkit2gtk::WebView| {
        notify(caskade_webview2::ProcessFailure {
            kind: caskade_webview2::Kind::RendererUnresponsive,
            reason: "WebKit web process unresponsive",
            exit_code: None,
            process: None,
            url: view.uri().map(|uri| uri.to_string()),
        });
    };
    let hung = std::rc::Rc::new(hung);
    view.connect_is_web_process_responsive_notify(move |view| {
        if view.is_web_process_responsive() {
            return;
        }
        hung(view);
        // WebKit says so once, and the policy waits for a hang that lasts:
        // say it again if it still hangs past the grace.
        let view = view.downgrade();
        let again = hung.clone();
        let past_grace = HANG_GRACE + Duration::from_secs(1);
        webkit2gtk::glib::timeout_add_local_once(past_grace, move || {
            if let Some(view) = view.upgrade()
                && !view.is_web_process_responsive()
            {
                again(&view);
            }
        });
    });
}
pub fn watch_player(
    view: &wry::WebView,
    notify: impl Fn(caskade_webview2::ProcessFailure) + 'static,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        use wry::WebViewExtWindows;
        caskade_webview2::watch(&view.webview(), notify).map_err(|error| error.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        use wry::WebViewExtUnix;
        attach_webkit(&view.webview(), notify);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = (view, notify);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    #[test]
    fn an_unwritable_budget_stops_automatic_restarts() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!Restarts::new(dir.path().to_path_buf()).claim(SystemTime::now()));
    }

    #[test]
    fn recovery_routes_keep_query_and_fragment_and_stay_local() {
        assert_eq!(
            route("http://tauri.localhost/library?filter=installed#x"),
            Some("/library?filter=installed#x".into())
        );
        assert_eq!(route("javascript:alert(1)"), None);
        assert_eq!(route("tauri://localhost//external.example/path"), None);
    }

    #[test]
    fn a_renderer_that_exits_reloads_the_page() {
        let mut policy = Policy::default();
        assert_eq!(
            policy.next(Failure::RendererExited, Instant::now()),
            Step::Reload
        );
    }

    #[test]
    fn a_lost_browser_process_restarts() {
        let mut policy = Policy::default();
        assert_eq!(
            policy.next(Failure::BrowserExited, Instant::now()),
            Step::Restart
        );
    }

    #[test]
    fn processes_the_webview_restarts_itself_are_left_alone() {
        use caskade_webview2::Kind;
        let mut policy = Policy::default();
        for kind in [
            Kind::FrameRendererExited,
            Kind::GpuExited,
            Kind::UtilityExited,
            Kind::Other,
        ] {
            assert_eq!(policy.next(failure_of(kind), Instant::now()), Step::Wait);
        }
    }

    #[test]
    fn reloads_that_keep_failing_turn_into_a_restart() {
        let start = Instant::now();
        let mut policy = Policy::default();
        for second in 0..RELOADS as u64 {
            assert_eq!(
                policy.next(Failure::RendererExited, at(start, second)),
                Step::Reload
            );
        }
        assert_eq!(
            policy.next(Failure::RendererExited, at(start, 10)),
            Step::Restart
        );
        // Long after, a failure is a new one.
        assert_eq!(
            policy.next(
                Failure::RendererExited,
                at(start, 10 + RELOAD_WINDOW.as_secs())
            ),
            Step::Reload
        );
    }

    #[test]
    fn a_hang_gets_time_to_pass_before_the_page_reloads() {
        let start = Instant::now();
        let mut policy = Policy::default();
        let hung = Failure::RendererUnresponsive;
        assert_eq!(policy.next(hung, at(start, 0)), Step::Wait);
        assert_eq!(policy.next(hung, at(start, 10)), Step::Wait);
        assert_eq!(policy.next(hung, at(start, 20)), Step::Reload);
        // The reload starts the clock again.
        assert_eq!(policy.next(hung, at(start, 25)), Step::Wait);
    }

    #[test]
    fn hangs_far_apart_are_separate() {
        let start = Instant::now();
        let mut policy = Policy::default();
        let hung = Failure::RendererUnresponsive;
        assert_eq!(policy.next(hung, at(start, 0)), Step::Wait);
        assert_eq!(policy.next(hung, at(start, 15)), Step::Wait);
        // More than HANG_GAP without a report: that hang passed.
        assert_eq!(policy.next(hung, at(start, 50)), Step::Wait);
        assert_eq!(policy.next(hung, at(start, 65)), Step::Wait);
        assert_eq!(policy.next(hung, at(start, 70)), Step::Reload);
    }

    #[test]
    fn restarts_stop_after_a_few_and_count_across_starts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("webview-restarts");
        let now = SystemTime::now();
        for _ in 0..RESTARTS {
            // A new process each time: only the file remembers.
            assert!(Restarts::new(file.clone()).claim(now));
        }
        assert!(!Restarts::new(file.clone()).claim(now));
        assert!(Restarts::new(file).claim(now + RESTART_WINDOW));
    }

    #[test]
    fn a_restart_the_user_asks_for_always_counts() {
        let dir = tempfile::tempdir().unwrap();
        let restarts = Restarts::new(dir.path().join("webview-restarts"));
        let now = SystemTime::now();
        for _ in 0..RESTARTS {
            restarts.record(now);
        }
        assert!(!restarts.claim(now));
    }

    #[test]
    fn unreadable_or_future_records_are_ignored() {
        let now = 1_000_000_000;
        let text = format!("junk\n{}\n{}\n", now - 1000, now + 5000);
        assert_eq!(recent(&text, now), vec![now - 1000]);
    }
}
