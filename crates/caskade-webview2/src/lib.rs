//! WebView2's `ProcessFailed` event behind a safe API (#3).
//!
//! WebView2 runs a page in processes of its own (browser, renderer, GPU,
//! utilities) and reports each one that fails through `ProcessFailed`.
//! Neither wry nor Tauri listens for it, so a webview that loses its browser
//! process stays white for good. The desktop app's `webview_recovery`
//! decides what to do; this crate only subscribes and describes the failure,
//! keeping the COM calls, which are all `unsafe`, out of the adapter (whose
//! lints forbid unsafe code).
//!
//! Windows only: elsewhere the crate is empty.

#![deny(unsafe_code)]

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{watch, watch_controller};

/// Which process failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The browser process exited: the webview is closed for good.
    BrowserExited,
    /// The main frame's renderer exited.
    RendererExited,
    /// The main frame's renderer stopped answering. WebView2 repeats this
    /// every few seconds while it lasts.
    RendererUnresponsive,
    /// A frame's renderer exited; the page goes on.
    FrameRendererExited,
    /// The GPU process exited; WebView2 starts another.
    GpuExited,
    /// A utility process exited; WebView2 starts another.
    UtilityExited,
    /// Any other process (sandbox helper, plugin, unknown).
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BrowserExited => "browser exited",
            Self::RendererExited => "renderer exited",
            Self::RendererUnresponsive => "renderer unresponsive",
            Self::FrameRendererExited => "frame renderer exited",
            Self::GpuExited => "GPU process exited",
            Self::UtilityExited => "utility process exited",
            Self::Other => "other process exited",
        }
    }
}

/// One `ProcessFailed` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessFailure {
    pub kind: Kind,
    /// Why, as WebView2 puts it ("crashed", "terminated", "out of memory",
    /// …); "unknown" on runtimes without details.
    pub reason: &'static str,
    pub exit_code: Option<i32>,
    /// The failed process's description, where WebView2 gives one.
    pub process: Option<String>,
}
