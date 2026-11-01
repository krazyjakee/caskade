//! The COM side: subscribing to `ProcessFailed` and reading its arguments.

use crate::{Kind, ProcessFailure};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED, COREWEBVIEW2_PROCESS_FAILED_REASON,
    COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED, COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED,
    COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY,
    COREWEBVIEW2_PROCESS_FAILED_REASON_PROFILE_DELETED,
    COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED, COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE,
    ICoreWebView2, ICoreWebView2Controller, ICoreWebView2ProcessFailedEventArgs,
    ICoreWebView2ProcessFailedEventArgs2,
};
use webview2_com::{ProcessFailedEventHandler, take_pwstr};
use windows_core::{Interface, PWSTR};

/// Calls `on_failure` on the webview's thread for each of its processes
/// that fails, for as long as the webview lives. Call it on the webview's
/// thread (Tauri's `with_webview`, or wry's event loop).
pub fn watch(
    webview: &ICoreWebView2,
    on_failure: impl Fn(ProcessFailure) + 'static,
) -> windows_core::Result<()> {
    let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            on_failure(failure(&args));
        }
        Ok(())
    }));
    let mut token = 0;
    // SAFETY: a live webview, on its own thread (the caller's contract).
    // WebView2 keeps its own reference to the handler.
    #[allow(unsafe_code)]
    unsafe {
        webview.add_ProcessFailed(&handler, &mut token)
    }
}

/// [`watch`] for the webview a controller hosts (Tauri's `PlatformWebview`).
pub fn watch_controller(
    controller: &ICoreWebView2Controller,
    on_failure: impl Fn(ProcessFailure) + 'static,
) -> windows_core::Result<()> {
    // SAFETY: a live controller, on its webview's thread (as for `watch`).
    #[allow(unsafe_code)]
    let webview = unsafe { controller.CoreWebView2() }?;
    watch(&webview, on_failure)
}

fn failure(args: &ICoreWebView2ProcessFailedEventArgs) -> ProcessFailure {
    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
    // SAFETY: the event's own arguments, inside its handler, written to a
    // local.
    #[allow(unsafe_code)]
    let _ = unsafe { args.ProcessFailedKind(&mut kind) };
    let mut failure = ProcessFailure {
        kind: kind_of(kind),
        reason: "unknown",
        exit_code: None,
        process: None,
    };
    // Runtimes before 1.0.992 give no details.
    let Ok(details) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() else {
        return failure;
    };
    let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
    let mut code = 0;
    let mut description = PWSTR::null();
    // SAFETY: as above. The description is WebView2's allocation, handed
    // over and freed by `take_pwstr` (which takes null too).
    #[allow(unsafe_code)]
    unsafe {
        if details.Reason(&mut reason).is_ok() {
            failure.reason = reason_name(reason);
        }
        if details.ExitCode(&mut code).is_ok() {
            failure.exit_code = Some(code);
        }
        if details.ProcessDescription(&mut description).is_ok() {
            failure.process = Some(take_pwstr(description)).filter(|text| !text.is_empty());
        }
    }
    failure
}

fn kind_of(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> Kind {
    match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => Kind::BrowserExited,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => Kind::RendererExited,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => Kind::RendererUnresponsive,
        COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED => Kind::FrameRendererExited,
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED => Kind::GpuExited,
        COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED => Kind::UtilityExited,
        _ => Kind::Other,
    }
}

fn reason_name(reason: COREWEBVIEW2_PROCESS_FAILED_REASON) -> &'static str {
    match reason {
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE => "unresponsive",
        COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED => "terminated",
        COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED => "crashed",
        COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED => "launch failed",
        COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY => "out of memory",
        COREWEBVIEW2_PROCESS_FAILED_REASON_PROFILE_DELETED => "profile deleted",
        _ => "unexpected",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_failures_recovery_acts_on_are_told_apart() {
        assert_eq!(
            kind_of(COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED),
            Kind::BrowserExited
        );
        assert_eq!(
            kind_of(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED),
            Kind::RendererExited
        );
        assert_eq!(
            kind_of(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE),
            Kind::RendererUnresponsive
        );
        assert_eq!(
            kind_of(COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED),
            Kind::FrameRendererExited
        );
        assert_eq!(kind_of(COREWEBVIEW2_PROCESS_FAILED_KIND(99)), Kind::Other);
    }
}
