//! Console mode (#148): whether Caskade starts in it, and remembering the
//! mode chosen with the toggle. The window's part (fullscreen, and the
//! desktop window put back) is the shell's.
//!
//! Caskade starts in console mode with `--console`, with `CASKADE_CONSOLE=1`
//! (`0` keeps it out), or by the `core.console` setting: `auto` follows
//! gamescope (Steam Deck game mode), then the mode it was last left in.

use caskade_core::settings::{ConsoleSettings, SettingsService};

/// How Caskade was started, for the first mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Startup {
    /// `--console` on the command line.
    pub flag: bool,
    /// `CASKADE_CONSOLE`: `1` or `0` overrides the setting.
    pub forced: Option<bool>,
    /// Running inside gamescope.
    pub gamescope: bool,
}

impl Startup {
    pub fn read(args: &[String]) -> Self {
        Self::from(args, |name| std::env::var(name).ok())
    }

    pub fn from(args: &[String], var: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            flag: wants_console(args),
            forced: match var("CASKADE_CONSOLE").as_deref().map(str::trim) {
                Some("1" | "true") => Some(true),
                Some("0" | "false") => Some(false),
                _ => None,
            },
            gamescope: in_gamescope(var),
        }
    }

    /// Whether to start in console mode.
    pub fn console(&self, settings: &ConsoleSettings) -> bool {
        if self.flag {
            return true;
        }
        if let Some(forced) = self.forced {
            return forced;
        }
        match settings.start.as_str() {
            "console" => true,
            "desktop" => false,
            _ => self.gamescope || settings.active,
        }
    }
}

/// `--console` among `args` (the first one is the program).
pub fn wants_console(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| arg == "--console")
}

/// gamescope sets `GAMESCOPE_WAYLAND_DISPLAY` for its clients; Steam's game
/// mode also sets `SteamGamepadUI`.
pub fn in_gamescope(var: impl Fn(&str) -> Option<String>) -> bool {
    var("GAMESCOPE_WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
        || var("XDG_CURRENT_DESKTOP").is_some_and(|value| value.eq_ignore_ascii_case("gamescope"))
        || var("SteamGamepadUI").as_deref() == Some("1")
}

/// The console mode settings section.
pub const SETTINGS: &str = "core.console";

/// Remembers the mode chosen with the toggle, for `auto` starts.
pub async fn remember(service: &SettingsService, active: bool) -> Result<(), String> {
    let mut value: ConsoleSettings = service.read_or_default(SETTINGS).await;
    if value.active == active {
        return Ok(());
    }
    value.active = active;
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    service
        .set(SETTINGS, value, None)
        .await
        .map(drop)
        .map_err(|error| error.to_string())
}

/// Whether Caskade starts in console mode, by how it was started and the
/// setting.
pub async fn starts_in_console(startup: &Startup, settings: &SettingsService) -> bool {
    let preferences: ConsoleSettings = settings.read_or_default(SETTINGS).await;
    startup.console(&preferences)
}
