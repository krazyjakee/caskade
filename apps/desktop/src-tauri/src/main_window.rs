//! Bringing the main window back from outside the webview: the tray, a
//! second `caskade` start, a deep link, the guide button and a game's exit
//! all use [`raise`].

use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

/// How long the window manager gets to restore and focus the window.
const SETTLE: Duration = Duration::from_millis(1500);
const POLL: Duration = Duration::from_millis(25);

/// Shows the main window, from the tray or minimized, and focuses it, even
/// over a running game. Where the window manager refuses focus (Wayland,
/// Windows foreground rules), the window asks for attention instead.
pub fn raise<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(error) = window.show().and_then(|()| window.unminimize()) {
        tracing::warn!(%error, "couldn't raise the main window");
    }
    #[cfg(target_os = "linux")]
    present(&window);
    // GTK restores a window asynchronously, and tao drops a focus request
    // for a window it still sees minimized: focus once it is back.
    std::thread::spawn(move || focus(&window));
}

/// Where the main window is while it's on screen (shown, not minimized):
/// a web game's window opens over it in desktop mode.
pub fn placement<R: Runtime>(app: &AppHandle<R>) -> Option<caskade_core::launch::WindowPlacement> {
    let window = app.get_webview_window("main")?;
    if !window.is_visible().ok()? || window.is_minimized().ok()? {
        return None;
    }
    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    Some(caskade_core::launch::WindowPlacement {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

/// Muffin and Mutter leave a minimized window minimized on GTK's deiconify,
/// and tao won't present a window it sees minimized: present it here, which
/// asks the window manager to activate it (`_NET_ACTIVE_WINDOW`).
///
/// GTK stamps the request with Caskade's last input, the click on Play.
/// Muffin ignores it as stale once the game has had input since, so on X11
/// it carries the server's current time.
#[cfg(target_os = "linux")]
fn present<R: Runtime>(window: &WebviewWindow<R>) {
    use gtk::prelude::{Cast, GtkWindowExt, WidgetExt};
    let target = window.clone();
    let presented = window.run_on_main_thread(move || match target.gtk_window() {
        Ok(gtk) => {
            let now = gtk
                .window()
                .and_then(|gdk| gdk.downcast::<gdkx11::X11Window>().ok())
                .map_or(gtk::gdk::ffi::GDK_CURRENT_TIME as u32, |x11| {
                    gdkx11::functions::x11_get_server_time(&x11)
                });
            gtk.present_with_time(now);
        }
        Err(error) => tracing::warn!(%error, "couldn't present the main window"),
    });
    if let Err(error) = presented {
        tracing::warn!(%error, "couldn't present the main window");
    }
}

fn focus<R: Runtime>(window: &WebviewWindow<R>) {
    settle(|| !window.is_minimized().unwrap_or(false));
    if let Err(error) = window.set_focus() {
        tracing::warn!(%error, "couldn't focus the main window");
    }
    if !settle(|| window.is_focused().unwrap_or(false)) {
        let _ = window.request_user_attention(Some(tauri::UserAttentionType::Critical));
    }
}

/// Waits up to [`SETTLE`] for `done`; false when it never was.
fn settle(done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if done() {
            return true;
        }
        if start.elapsed() >= SETTLE {
            return false;
        }
        std::thread::sleep(POLL);
    }
}
