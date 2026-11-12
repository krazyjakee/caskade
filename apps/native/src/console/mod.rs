//! Console mode (#107; the webview's `shell/console/mode.tsx`): the same app
//! and data with a different shell, for a TV and a pad. Caskade starts in
//! it with `--console`, `CASKADE_CONSOLE=1`, inside gamescope (a Steam Deck
//! in game mode) or by the `core.console` setting
//! (`caskade_app::commands::console`). The mode is `data-mode` on the
//! frame's root, so the `mode-console` token set (TV text sizes) applies;
//! console mode makes the window fullscreen, and leaving it puts the
//! desktop window back. The toggle remembers the choice; a second
//! `caskade --console` switches without remembering.

use dioxus_native::prelude::*;

mod hint_bar;
#[cfg(test)]
mod tests;

pub use hint_bar::{HintBar, glyph, use_family};

/// Which shell the window shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Desktop,
    Console,
}

impl Mode {
    /// As `data-mode` and the token sets name it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Console => "console",
        }
    }
}

/// The mode now; `None` until the start has been decided.
static MODE: GlobalSignal<Option<Mode>> = Signal::global(|| None);

/// [`MODE`] for code outside the UI (the event loop's close, plugin runs).
static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether console mode is on, from any thread.
pub fn active() -> bool {
    ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

fn store(mode: Mode) {
    *MODE.write() = Some(mode);
    ACTIVE.store(mode == Mode::Console, std::sync::atomic::Ordering::Relaxed);
}

/// The mode the window is in (desktop until the start is decided).
pub fn mode() -> Mode {
    MODE().unwrap_or_default()
}

pub fn is_console() -> bool {
    mode() == Mode::Console
}

/// Switches to `next`: the window goes fullscreen (or back), the input
/// layer takes it, and with `keep` the choice is remembered for the next
/// start. The other shell replaces the view, so Settings' unsaved edits are
/// asked about first.
pub fn set_mode(next: Mode, keep: bool) {
    if MODE.peek().is_some_and(|current| current == next) {
        return;
    }
    crate::settings::unsaved::after_unsaved_changes(
        crate::settings::unsaved::Leaving::Mode,
        move || switch(next, keep),
    );
}

fn switch(next: Mode, keep: bool) {
    if MODE.peek().is_some_and(|current| current == next) {
        return;
    }
    store(next);
    crate::launch::ask(crate::launch::Request::Fullscreen(next == Mode::Console));
    if keep && let Some(source) = try_consume_context::<crate::data::DataSource>() {
        let settings = source.settings.clone();
        spawn_forever(async move {
            let saved = caskade_app::runtime::spawn(async move {
                settings.remember_console(next == Mode::Console).await
            })
            .await;
            if let Ok(Err(error)) = saved {
                tracing::warn!(%error, "console mode not remembered");
            }
        });
    }
}

/// Asks for console mode from outside the UI (a second `caskade --console`,
/// from the app's runtime): the window's root switches when it hears it.
/// Not remembered.
pub fn request_console() {
    let _ = requests().send(Mode::Console);
}

fn requests() -> &'static tokio::sync::broadcast::Sender<Mode> {
    static REQUESTS: std::sync::OnceLock<tokio::sync::broadcast::Sender<Mode>> =
        std::sync::OnceLock::new();
    REQUESTS.get_or_init(|| tokio::sync::broadcast::channel(4).0)
}

/// The other mode, remembered: the toggle and the View button.
pub fn toggle() {
    set_mode(
        if is_console() {
            Mode::Desktop
        } else {
            Mode::Console
        },
        true,
    );
}

use dioxus_native::prelude::dioxus_core::spawn_forever;

/// The main window gaining (`true`) or losing focus, as the event loop sees
/// it.
pub fn focus_changes() -> &'static tokio::sync::broadcast::Sender<bool> {
    static FOCUS: std::sync::OnceLock<tokio::sync::broadcast::Sender<bool>> =
        std::sync::OnceLock::new();
    FOCUS.get_or_init(|| tokio::sync::broadcast::channel(8).0)
}

/// Decides the mode to start in before `children` show, so the desktop
/// shell never flashes before a console start, and keeps the input layer
/// and `data-mode` in step with it.
#[component]
pub fn ModeRoot(children: Element) -> Element {
    let input = crate::input::use_input();
    let settings = try_use_context::<crate::data::DataSource>().map(|source| source.settings);
    use_hook(|| {
        let mut asked = requests().subscribe();
        spawn(async move {
            while let Ok(mode) = asked.recv().await {
                set_mode(mode, false);
            }
        });
    });
    use_hook(move || {
        if MODE.peek().is_some() {
            return;
        }
        // Tests open many windows over one fixture library, and a saved
        // "Start in" (Settings → General) would carry from one to the next:
        // they start on the desktop and switch with View.
        let Some(settings) = settings.filter(|_| !cfg!(test)) else {
            store(Mode::Desktop);
            return;
        };
        spawn(async move {
            let startup = caskade_app::commands::console::Startup::read(
                &std::env::args().collect::<Vec<_>>(),
            );
            let console =
                caskade_app::runtime::spawn(
                    async move { settings.starts_in_console(&startup).await },
                )
                .await
                .unwrap_or(false);
            let start = if console {
                Mode::Console
            } else {
                Mode::Desktop
            };
            store(start);
            if console {
                crate::launch::ask(crate::launch::Request::Fullscreen(true));
            }
        });
    });
    let current = MODE();
    use_effect(move || input.set_console(MODE() == Some(Mode::Console)));
    let Some(current) = current else {
        return rsx! {};
    };
    rsx! {
        div { "data-mode": current.name(), class: "contents", {children} }
    }
}
