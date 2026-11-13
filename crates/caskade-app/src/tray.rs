//! The tray icon's state (#88), the same in every shell: whether it's up,
//! whether a close of the main window hides to it, quits, or waits for the
//! UI's answer (unsaved edits, #58), and the menu's labels. The icon itself
//! is each shell's (`tray.rs` in the Tauri app and the native app).

use caskade_core::settings::{GeneralSettings, SettingsService};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Whether a close hides the main window or lets it close.
#[derive(Debug, Default)]
pub struct Tray {
    /// The icon is up.
    shown: AtomicBool,
    /// `core.general.onClose` is `tray`.
    close_to_tray: AtomicBool,
    /// Something asked to quit: closes go through from now on.
    quitting: AtomicBool,
    /// The webview asks before a close that would quit goes through.
    held: AtomicBool,
    /// Closes held so far, and the last of them the webview heard.
    held_closes: AtomicU64,
    heard: AtomicU64,
}

/// What a close of the main window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Close {
    /// It goes through, or console mode's restore takes it from here.
    Go,
    /// The window hides to the tray.
    Hide,
    /// It waits for the webview; the number names it to [`Tray::unanswered`].
    Held(u64),
}

impl Tray {
    /// The icon is up, so a hidden window can be brought back from it.
    pub fn shown(&self) -> bool {
        self.shown.load(Ordering::Relaxed)
    }

    /// Whether a close of the main window hides it instead.
    pub fn hides_on_close(&self, console: bool) -> bool {
        self.shown()
            && self.close_to_tray.load(Ordering::Relaxed)
            && !self.quitting.load(Ordering::Relaxed)
            && !console
    }

    /// Whether the webview holds closes.
    pub fn held(&self) -> bool {
        self.held.load(Ordering::Relaxed)
    }

    /// Decides a close of the main window, and counts it if it's held.
    pub fn close(&self, console: bool) -> Close {
        if self.hides_on_close(console) {
            Close::Hide
        } else if self.held() {
            Close::Held(self.held_closes.fetch_add(1, Ordering::Relaxed) + 1)
        } else {
            Close::Go
        }
    }

    /// The webview's `close_hold`. `true` while it holds says it heard the
    /// closes so far. `true` after `false` is a fresh hold, which forgets
    /// a quit: that's the answer Stay. `false` keeps it, so the webview's
    /// next close goes through.
    pub fn hold(&self, held: bool) {
        let was = self.held.swap(held, Ordering::Relaxed);
        if !held {
            return;
        }
        if !was {
            self.quitting.store(false, Ordering::Relaxed);
        }
        self.heard
            .store(self.held_closes.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    /// Whether held close number `close` still has no word from the webview.
    pub fn unanswered(&self, close: u64) -> bool {
        self.held() && self.heard.load(Ordering::Relaxed) < close
    }

    /// The icon went up (`true`) or away.
    pub fn set_shown(&self, shown: bool) {
        self.shown.store(shown, Ordering::Relaxed);
    }

    /// Lets the next close of the main window quit.
    pub fn quit(&self) {
        self.quitting.store(true, Ordering::Relaxed);
    }

    /// Follows `core.general`'s close setting.
    pub fn follow(&self, settings: &GeneralSettings) {
        self.close_to_tray
            .store(settings.on_close == "tray", Ordering::Relaxed);
    }
}

impl Tray {
    /// Takes the icon down from the state: whether it was up.
    pub fn take_shown(&self) -> bool {
        self.shown.swap(false, Ordering::Relaxed)
    }
}

/// Hands `apply` `core.general` now and after each change to it, until
/// the settings close. It listens before the first read, so a change in
/// between isn't missed.
pub async fn follow_settings(settings: SettingsService, apply: impl Fn(&GeneralSettings)) {
    use tokio::sync::broadcast::error::RecvError;
    let mut changes = settings.subscribe();
    loop {
        apply(&settings.read_or_default("core.general").await);
        loop {
            match changes.recv().await {
                // Missed changes may have been to `core.general`.
                Ok(changed) if changed.namespace != "core.general" => {}
                Ok(_) | Err(RecvError::Lagged(_)) => break,
                Err(RecvError::Closed) => return,
            }
        }
    }
}

/// The menu item that shows the main window.
pub fn show_label() -> String {
    caskade_core::tr!("Show Caskade")
}

/// The menu item that quits.
pub fn quit_label() -> String {
    caskade_core::tr!("Quit Caskade")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tray(shown: bool, on_close: &str) -> Tray {
        let tray = Tray::default();
        tray.shown.store(shown, Ordering::Relaxed);
        tray.follow(&GeneralSettings {
            on_close: on_close.into(),
            ..GeneralSettings::default()
        });
        tray
    }

    #[test]
    fn a_close_hides_only_to_a_tray_icon_that_is_up() {
        assert!(tray(true, "tray").hides_on_close(false));
        assert!(!tray(true, "quit").hides_on_close(false));
        assert!(!tray(false, "tray").hides_on_close(false));
    }

    #[test]
    fn quitting_and_console_mode_close_for_good() {
        let tray = tray(true, "tray");
        assert!(!tray.hides_on_close(true));
        tray.quit();
        assert!(!tray.hides_on_close(false));
    }

    #[test]
    fn a_held_close_waits_but_hiding_to_the_tray_does_not() {
        let tray = tray(true, "tray");
        tray.hold(true);
        // The webview and its draft stay in the tray.
        assert_eq!(tray.close(false), Close::Hide);
        // A close that would quit waits, in console mode too.
        assert_eq!(tray.close(true), Close::Held(1));
        tray.quit();
        assert_eq!(tray.close(false), Close::Held(2));

        tray.hold(false);
        assert_eq!(tray.close(false), Close::Go);
    }

    #[test]
    fn a_close_nothing_holds_is_not_counted() {
        let tray = tray(false, "quit");
        assert_eq!(tray.close(false), Close::Go);
        tray.hold(true);
        assert_eq!(tray.close(false), Close::Held(1));
    }

    #[test]
    fn only_a_webview_that_answers_keeps_a_held_close_waiting() {
        let tray = tray(false, "quit");
        tray.hold(true);
        let Close::Held(first) = tray.close(false) else {
            panic!("not held");
        };
        assert!(tray.unanswered(first));

        tray.hold(true);
        assert!(!tray.unanswered(first));
        // Another close, say a second Quit, needs an answer of its own, so
        // a webview that stopped since can't strand it.
        let Close::Held(second) = tray.close(false) else {
            panic!("not held");
        };
        assert!(!tray.unanswered(first));
        assert!(tray.unanswered(second));

        tray.hold(false);
        assert!(
            !tray.unanswered(second),
            "released: nothing left to wait for"
        );
    }
}
