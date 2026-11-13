//! Bringing a window's page back when the webview loses its processes
//! (#3). WebView2 runs each page in processes of its own, and they can go
//! away under the app: a crash, running out of memory, a GPU driver reset,
//! or an installer or Windows update closing them. Without this the window
//! stays white for good.
//!
//! - The page's renderer exited, or hung for [`HANG_GRACE`]: reload the
//!   page. The core still has everything, so the UI comes back where it was.
//! - The WebView2 browser process exited: the webview is dead and can't be
//!   rebuilt in place, so the window closes.
//! - Reloads that keep failing ([`RELOADS`] within [`RELOAD_WINDOW`]) close
//!   it too.
//!
//! The game and video windows (`web_player`, `video_player`) follow this
//! [`Policy`]. [`Restarts`] counts Caskade's own restarts across processes,
//! in a file: the native app restarts after a crash, and past [`RESTARTS`]
//! within [`RESTART_WINDOW`] a native dialog asks to restart or quit
//! instead of looping.
//!
//! WebView2 reports failures on Windows; WebKitGTK reports its web process
//! exiting or going unresponsive on Linux. A main-window failure on the GPU
//! renderer Caskade picked ([`crate::webkit`]) restarts on WebKit's own
//! instead of reloading (#72).

// Elsewhere nothing reports a failure, so recovery never runs.
#![cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]

use caskade_webview2::Kind;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Renderer failures a page survives by reloading, within [`RELOAD_WINDOW`].
const RELOADS: usize = 3;
const RELOAD_WINDOW: Duration = Duration::from_secs(120);
/// How long a renderer stays unresponsive before the page reloads: a long
/// task (a big library rendering, a heavy game) gets this long to finish.
const HANG_GRACE: Duration = Duration::from_secs(20);
/// Unresponsive reports further apart than this start a new hang.
const HANG_GAP: Duration = Duration::from_secs(30);
/// Restarts Caskade makes on its own within [`RESTART_WINDOW`].
pub const RESTARTS: usize = 2;
const RESTART_WINDOW: Duration = Duration::from_secs(600);

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
    pub fn next(&mut self, failure: Kind, now: Instant) -> Step {
        match failure {
            // The webview starts these again by itself.
            Kind::FrameRendererExited | Kind::GpuExited | Kind::UtilityExited | Kind::Other => {
                Step::Wait
            }
            Kind::BrowserExited => Step::Restart,
            Kind::RendererExited => self.reload(now),
            Kind::RendererUnresponsive => {
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
        let mut recent = self.recent(now);
        if recent.len() >= RESTARTS {
            return false;
        }
        recent.push(now);
        self.write(&recent)
    }

    /// Records a restart the user asked for: it always goes ahead.
    pub fn record(&self, now: SystemTime) {
        let now = millis(now);
        let mut recent = self.recent(now);
        recent.push(now);
        let _ = self.write(&recent);
    }

    fn recent(&self, now: u64) -> Vec<u64> {
        recent(
            &std::fs::read_to_string(&self.file).unwrap_or_default(),
            now,
        )
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

#[cfg(target_os = "linux")]
pub fn attach_webkit(
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
    fn a_renderer_that_exits_reloads_the_page() {
        let mut policy = Policy::default();
        assert_eq!(
            policy.next(Kind::RendererExited, Instant::now()),
            Step::Reload
        );
    }

    #[test]
    fn a_lost_browser_process_restarts() {
        let mut policy = Policy::default();
        assert_eq!(
            policy.next(Kind::BrowserExited, Instant::now()),
            Step::Restart
        );
    }

    #[test]
    fn processes_the_webview_restarts_itself_are_left_alone() {
        let mut policy = Policy::default();
        for kind in [
            Kind::FrameRendererExited,
            Kind::GpuExited,
            Kind::UtilityExited,
            Kind::Other,
        ] {
            assert_eq!(policy.next(kind, Instant::now()), Step::Wait);
        }
    }

    #[test]
    fn reloads_that_keep_failing_turn_into_a_restart() {
        let start = Instant::now();
        let mut policy = Policy::default();
        for second in 0..RELOADS as u64 {
            assert_eq!(
                policy.next(Kind::RendererExited, at(start, second)),
                Step::Reload
            );
        }
        assert_eq!(
            policy.next(Kind::RendererExited, at(start, 10)),
            Step::Restart
        );
        // Long after, a failure is a new one.
        assert_eq!(
            policy.next(
                Kind::RendererExited,
                at(start, 10 + RELOAD_WINDOW.as_secs())
            ),
            Step::Reload
        );
    }

    #[test]
    fn a_hang_gets_time_to_pass_before_the_page_reloads() {
        let start = Instant::now();
        let mut policy = Policy::default();
        let hung = Kind::RendererUnresponsive;
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
        let hung = Kind::RendererUnresponsive;
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
