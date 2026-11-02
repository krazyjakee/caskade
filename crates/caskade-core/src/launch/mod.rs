//! Resolve launch specs, intercept commands, spawn and track play sessions.
mod actions;
pub use actions::{
    ActionOption, ReleaseActions, built_in_action, delete_action, entry_actions, game_actions,
    save_action, validate_target, validate_tracking,
};
mod bus;
mod command;
mod emulator;
pub use bus::{LaunchAfter, LaunchBefore};
pub use emulator::{DiscOption, EmulatorLaunches, LaunchChoice, LaunchPrompt, ProfileOption};
mod events;
mod hooks;
mod resolve;
mod runners;
pub use runners::{RunnerPaths, RunnerResolver};
mod service;
mod stop;
mod stream;
mod web;
pub use command::{EmulationLaunch, LaunchCommand, managed_path, uri_command};
pub use events::{LaunchEvent, LaunchTicket, SessionStatus};
pub use hooks::LaunchPreparer;
pub use hooks::{LaunchHooks, NoHooks, NoPluginRunner, PlayGate, PluginRunner};
pub use resolve::{PreparedLaunch, resolve, resolve_with_runners};
pub use service::{GameLease, LaunchConfig, LaunchHandle, LaunchService};
pub use stream::{ClientLine, HostBusy, StreamLauncher, StreamStart};
pub use web::{
    BUNDLE_ARG, CAPTURE_ARG, DATA_DIR_ARG, FULLSCREEN_ARG, GEOMETRY_ARG, SIZE_ARG, WEB_GAME_ARG,
    WebPlayer, WindowPlacement,
};

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("invalid launch: {0}")]
    Invalid(String),
    #[error("game already has a running session")]
    AlreadyRunning,
    #[error("launch was cancelled")]
    Cancelled,
    /// Something the launch runs first stopped it: a plugin's
    /// `launch.before` hook, or the emulator profile's before-command. The
    /// text names which and why (#160).
    #[error("{0}")]
    Refused(String),
    /// Ask-at-launch or the disc picker: launch again with a [`LaunchChoice`].
    #[error("the launch needs a choice")]
    NeedsChoice(Box<LaunchPrompt>),
    /// The signed-in account's age limit holds the game back until an
    /// admin's PIN approves this launch ([`PlayGate`]).
    #[error("{0}")]
    AgeRestricted(Box<crate::accounts::AgeRestriction>),
    /// The streaming device plays another game: Play asks before stopping
    /// it (#264).
    #[error("{device} is playing another game", device = .0.device)]
    HostBusy(Box<HostBusy>),
    #[error("no runner registered for {0}")]
    Unsupported(String),
    #[error("game process did not appear before the launch deadline")]
    NotStarted,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Tracking(#[from] crate::tracking::TrackingError),
    #[error("session store: {0}")]
    Store(String),
}
pub type Result<T> = std::result::Result<T, LaunchError>;

fn store_error(error: impl std::fmt::Display) -> LaunchError {
    LaunchError::Store(error.to_string())
}

#[cfg(all(test, unix))]
mod service_tests;
#[cfg(all(test, unix))]
mod test_support;

#[cfg(all(test, target_os = "linux"))]
mod sqlite_tests;

#[cfg(all(test, target_os = "linux"))]
mod uri_tests;

#[cfg(all(test, target_os = "linux"))]
mod native_tests;

#[cfg(all(test, target_os = "linux"))]
mod bus_tests;

#[cfg(all(test, target_os = "linux"))]
mod gate_tests;

#[cfg(all(test, target_os = "linux"))]
mod actions_launch_tests;

#[cfg(all(test, target_os = "linux"))]
mod scripts_launch_tests;

#[cfg(all(test, target_os = "linux"))]
mod stream_tests;
