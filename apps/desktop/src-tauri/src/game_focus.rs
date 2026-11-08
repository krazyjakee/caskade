//! Making way for a game. When a game starts, the
//! main window does what `core.general.onLaunch` says (stay, minimize or
//! hide to the tray), so the window manager focuses the game. Console mode
//! leaves the window where it is: the webview fades to black
//! (`GameCurtain`), and the window manager gives the new game window focus
//! over it. When the last game exits, Caskade takes focus back.
//!
//! A web game in desktop mode opens over Caskade's window,
//! so Caskade stays where it is, under it.
//!
//! A stream from another device (#264) shows its client's own window
//! before the stream's: Caskade's window stays on top of it until the
//! stream is up, then gets out of the way in either mode, since the
//! stream's window is already there underneath. GNOME is the exception,
//! see [`refuses_focus_under_a_cover`].
//!
//! Hiding needs the tray icon to come back from: without one (GNOME without
//! the AppIndicator extension, no libappindicator), the window minimizes
//! instead.

use caskade_core::{
    bus::{Bus, Event},
    db::LibraryStore,
    launch::LaunchService,
    model::LaunchSpec,
    settings::SettingsService,
};
use std::collections::BTreeSet;
use tauri::{AppHandle, Manager, Runtime};

/// What to do with the main window when a game starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Making {
    Stay,
    Minimize,
    Hide,
    /// Console mode: the window stays fullscreen behind the game, black.
    Fade,
}

/// Console mode fades whatever `on_launch` says; a web game, whose window
/// opens over Caskade's, leaves it; `tray` hides only to a tray icon that
/// is up.
pub fn making(console: bool, web: bool, on_launch: &str, tray_shown: bool) -> Making {
    match on_launch {
        _ if console => Making::Fade,
        _ if web => Making::Stay,
        "stay" => Making::Stay,
        "tray" if tray_shown => Making::Hide,
        _ => Making::Minimize,
    }
}

/// What to do with the main window once a stream is up: its window is
/// beneath Caskade's, so Caskade always goes, to the tray when that's the
/// setting outside console mode.
pub fn making_for_stream(console: bool, on_launch: &str, tray_shown: bool) -> Making {
    match making(console, false, on_launch, tray_shown) {
        Making::Hide => Making::Hide,
        _ => Making::Minimize,
    }
}

/// Mutter refuses focus to a window that opens under an always-on-top one and
/// marks it as wanting attention, and GNOME Shell then names the client in a
/// notification ("<client> is ready"). There Caskade stays an ordinary window:
/// the client's first window is one pixel, it has Caskade's startup id, and
/// the stream's opens over Caskade.
fn refuses_focus_under_a_cover(desktop: &str) -> bool {
    desktop
        .split(':')
        .any(|name| name.eq_ignore_ascii_case("gnome"))
}

fn desktop() -> String {
    std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()
}

/// Replaces the sessions followed with the ones running now, after missed
/// events. True when none is left: the window should come back.
fn reconcile(running: &mut BTreeSet<String>, now: impl IntoIterator<Item = String>) -> bool {
    *running = now.into_iter().collect();
    running.is_empty()
}

/// Follows game sessions until the bus is gone.
pub fn follow<R: Runtime>(app: AppHandle<R>, bus: &Bus, settings: SettingsService) {
    let mut stream = bus.stream();
    tauri::async_runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        let mut running = BTreeSet::new();
        // Caskade made way for a game, and takes focus back after it.
        let mut moved = false;
        // Streams starting, whose client's window Caskade covers.
        let mut covering = BTreeSet::new();
        loop {
            match stream.recv().await {
                Ok(Event::StreamStarting { session_id, .. }) => {
                    let Some(window) = app.get_webview_window("main") else {
                        continue;
                    };
                    covering.insert(session_id);
                    if !refuses_focus_under_a_cover(&desktop())
                        && let Err(error) = window.set_always_on_top(true)
                    {
                        tracing::warn!(%error, "couldn't cover the stream's start");
                    }
                }
                Ok(Event::GameStarted(session)) => {
                    let streamed = covering.remove(&session.session_id);
                    running.insert(session.session_id);
                    let Some(window) = app.get_webview_window("main") else {
                        continue;
                    };
                    if streamed && let Err(error) = window.set_always_on_top(false) {
                        tracing::warn!(%error, "Caskade's window stays on top");
                    }
                    let tray_shown = app
                        .try_state::<crate::tray::Tray>()
                        .is_some_and(|tray| tray.shown());
                    let console = settings.flag("core.console", "active").await;
                    let on_launch = on_launch(&settings).await;
                    let way = if streamed {
                        making_for_stream(console, &on_launch, tray_shown)
                    } else {
                        making(
                            console,
                            web_game(&app, &session.entry_id).await,
                            &on_launch,
                            tray_shown,
                        )
                    };
                    let done = match way {
                        Making::Stay => continue,
                        Making::Fade => Ok(()),
                        Making::Minimize => window.minimize(),
                        Making::Hide => window.hide(),
                    };
                    match done {
                        Ok(()) => moved = true,
                        Err(error) => tracing::warn!(%error, "couldn't make way for the game"),
                    }
                }
                Ok(Event::GameExited(session)) => {
                    // A stream that didn't start: its client is gone.
                    if covering.remove(&session.session_id)
                        && let Some(window) = app.get_webview_window("main")
                        && let Err(error) = window.set_always_on_top(false)
                    {
                        tracing::warn!(%error, "Caskade's window stays on top");
                    }
                    if running.remove(&session.session_id) && running.is_empty() && moved {
                        moved = false;
                        crate::main_window::raise(&app);
                    }
                }
                // Exits may be among the missed events: ask the launch
                // service what still runs, so the window isn't left away.
                Err(RecvError::Lagged(missed)) => {
                    let Some(launch) = app.try_state::<LaunchService>() else {
                        continue;
                    };
                    match launch.running_sessions().await {
                        Ok(now) => {
                            let now = now.into_iter().map(|ticket| ticket.session_id);
                            if reconcile(&mut running, now) && moved {
                                tracing::debug!(missed, "no game left after missed events");
                                moved = false;
                                crate::main_window::raise(&app);
                            }
                        }
                        Err(error) => tracing::warn!(%error, "running games unavailable"),
                    }
                }
                Ok(_) => {}
                Err(RecvError::Closed) => break,
            }
        }
    });
}

/// Whether the entry plays in the game window.
async fn web_game<R: Runtime>(app: &AppHandle<R>, entry_id: &str) -> bool {
    let Some(store) = app.try_state::<LibraryStore>() else {
        return false;
    };
    matches!(
        store.get_entry(entry_id).await,
        Ok(Some(entry)) if matches!(entry.launch, Some(LaunchSpec::Web { .. }))
    )
}

async fn on_launch(settings: &SettingsService) -> String {
    settings
        .get("core.general")
        .await
        .ok()
        .and_then(|section| Some(section.value.get("onLaunch")?.as_str()?.to_owned()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::refuses_focus_under_a_cover;

    #[test]
    fn only_gnome_refuses_focus_under_a_cover() {
        assert!(refuses_focus_under_a_cover("ubuntu:GNOME"));
        assert!(refuses_focus_under_a_cover("GNOME"));
        assert!(!refuses_focus_under_a_cover("X-Cinnamon"));
        assert!(!refuses_focus_under_a_cover("KDE"));
        assert!(!refuses_focus_under_a_cover(""));
    }

    use super::*;

    #[test]
    fn console_mode_always_fades_and_desktop_follows_the_setting() {
        assert_eq!(making(true, false, "stay", true), Making::Fade);
        assert_eq!(making(true, false, "tray", true), Making::Fade);
        assert_eq!(making(false, false, "stay", true), Making::Stay);
        assert_eq!(making(false, false, "minimize", true), Making::Minimize);
        assert_eq!(making(false, false, "tray", true), Making::Hide);
    }

    #[test]
    fn without_a_tray_icon_tray_minimizes_instead_of_hiding() {
        assert_eq!(making(false, false, "tray", false), Making::Minimize);
        assert_eq!(making(false, false, "stay", false), Making::Stay);
        assert_eq!(making(true, false, "tray", false), Making::Fade);
    }

    #[test]
    fn a_web_game_opens_over_caskade_in_desktop_mode() {
        assert_eq!(making(false, true, "minimize", true), Making::Stay);
        assert_eq!(making(false, true, "tray", true), Making::Stay);
        assert_eq!(making(true, true, "minimize", true), Making::Fade);
    }

    #[test]
    fn a_stream_always_makes_way_for_its_window() {
        assert_eq!(making_for_stream(true, "stay", true), Making::Minimize);
        assert_eq!(making_for_stream(false, "stay", true), Making::Minimize);
        assert_eq!(making_for_stream(false, "minimize", true), Making::Minimize);
        assert_eq!(making_for_stream(false, "tray", true), Making::Hide);
        assert_eq!(making_for_stream(false, "tray", false), Making::Minimize);
        assert_eq!(making_for_stream(true, "tray", true), Making::Minimize);
    }

    #[test]
    fn missed_events_follow_what_still_runs() {
        let mut running = BTreeSet::from(["a".to_owned(), "b".to_owned()]);
        assert!(!reconcile(&mut running, ["b".to_owned()]));
        assert_eq!(running, BTreeSet::from(["b".to_owned()]));
        assert!(reconcile(&mut running, []));
        assert!(running.is_empty());
    }
}
