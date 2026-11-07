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
    #[cfg(windows)]
    over_topmost(&window);
    #[cfg(target_os = "linux")]
    present(&window);
    // GTK restores a window asynchronously, and tao drops a focus request
    // for a window it still sees minimized: focus once it is back.
    std::thread::spawn(move || focus(&window));
}

/// How far a raise over another program's topmost window has got.
#[cfg(windows)]
static OVER_TOPMOST: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(NOT_OVER);
#[cfg(windows)]
const NOT_OVER: u8 = 0;
/// Topmost, and waiting for focus. A focus change now is the old one's.
#[cfg(windows)]
const RAISING: u8 = 1;
#[cfg(windows)]
const OVER: u8 = 2;

/// A full-screen client or game is a topmost window on Windows, and an
/// ordinary window stays beneath it however focused: Caskade goes topmost too,
/// until [`watch_focus`] sees it lose focus.
#[cfg(windows)]
fn over_topmost<R: Runtime>(window: &WebviewWindow<R>) {
    match window.set_always_on_top(true) {
        Ok(()) => OVER_TOPMOST.store(RAISING, std::sync::atomic::Ordering::Relaxed),
        Err(error) => tracing::warn!(%error, "couldn't raise over the game"),
    }
}

/// Lets go of topmost when the window that took it loses focus.
#[cfg(windows)]
pub fn watch_focus<R: Runtime>(window: &WebviewWindow<R>) {
    use std::sync::atomic::Ordering::Relaxed;
    let watched = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(true) => {
            let _ = OVER_TOPMOST.compare_exchange(RAISING, OVER, Relaxed, Relaxed);
        }
        tauri::WindowEvent::Focused(false)
            if OVER_TOPMOST
                .compare_exchange(OVER, NOT_OVER, Relaxed, Relaxed)
                .is_ok() =>
        {
            let_go_of_topmost(&watched);
        }
        _ => {}
    });
}

#[cfg(windows)]
fn let_go_of_topmost<R: Runtime>(window: &WebviewWindow<R>) {
    if let Err(error) = window.set_always_on_top(false) {
        tracing::warn!(%error, "Caskade's window stays on top");
    }
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

/// The variables that let a program Caskade starts take focus from this
/// window: a startup id on X11, an activation token on Wayland. A window
/// refused focus demands attention, and GNOME Shell then says "<its name> is
/// ready" in a notification.
#[cfg(target_os = "linux")]
pub fn activation<R: Runtime>(app: &AppHandle<R>) -> Vec<(String, String)> {
    use gtk::gdk;
    use gtk::gio;
    use gtk::prelude::{AppLaunchContextExt as _, Cast as _, WidgetExt as _};
    let Some(window) = app.get_webview_window("main") else {
        return Vec::new();
    };
    let (sent, received) = std::sync::mpsc::channel();
    let target = window.clone();
    let asked = window.run_on_main_thread(move || {
        let id = (|| {
            let context = gdk::Display::default()?.app_launch_context()?;
            let gtk = target.gtk_window().ok()?;
            if let Some(x11) = gtk
                .window()
                .and_then(|gdk| gdk.downcast::<gdkx11::X11Window>().ok())
            {
                context.set_timestamp(gdkx11::functions::x11_get_server_time(&x11));
            }
            let info = gio::AppInfo::create_from_commandline(
                "caskade",
                Some("Caskade"),
                gio::AppInfoCreateFlags::SUPPORTS_STARTUP_NOTIFICATION,
            )
            .ok()?;
            context.startup_notify_id(&info, &[]).map(String::from)
        })();
        let _ = sent.send(id);
    });
    if asked.is_err() {
        return Vec::new();
    }
    let Ok(Some(id)) = received.recv_timeout(Duration::from_secs(1)) else {
        return Vec::new();
    };
    vec![
        ("DESKTOP_STARTUP_ID".to_owned(), id.clone()),
        ("XDG_ACTIVATION_TOKEN".to_owned(), id),
    ]
}

/// Other systems have nothing like it to pass on.
#[cfg(not(target_os = "linux"))]
pub fn activation<R: Runtime>(_: &AppHandle<R>) -> Vec<(String, String)> {
    Vec::new()
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
    let focused = settle(|| window.is_focused().unwrap_or(false));
    // Focus never came, so nothing would end the raise.
    #[cfg(windows)]
    if !focused {
        use std::sync::atomic::Ordering::Relaxed;
        if OVER_TOPMOST
            .compare_exchange(RAISING, NOT_OVER, Relaxed, Relaxed)
            .is_ok()
        {
            let_go_of_topmost(window);
        }
    }
    if !focused {
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
