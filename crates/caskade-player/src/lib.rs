//! Caskade's own windows for web games (`web_player`) and videos
//! (`video_player`): tao and wry with no IPC, each in a process of its own,
//! which an app shell starts by running its executable again with
//! `--web-game` or `--video`. The Tauri app and the native app both
//! dispatch those arguments here before they start, so neither shell's
//! event loop has to host a webview. `recovery` is what every Caskade
//! webview does when its processes fail.

pub mod recovery;
pub mod video_player;
pub mod web_player;
pub mod webkit;

use std::ffi::OsString;

/// The window a start's arguments ask for instead of the app.
pub enum Window {
    /// `--web-game <url>`: a web game's window.
    WebGame(web_player::Options),
    /// `--video <id>`: a game's video.
    Video(video_player::Options),
}

impl Window {
    /// The window `args` ask for, if any.
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Option<Self> {
        let args: Vec<_> = args.into_iter().collect();
        web_player::Options::from_args(args.clone())
            .map(Self::WebGame)
            .or_else(|| video_player::Options::from_args(args).map(Self::Video))
    }

    /// Runs the window and ends the process with its code.
    pub fn run(self) -> ! {
        let (outcome, failed) = match self {
            Self::WebGame(options) => (web_player::run(options), "the game window couldn't open"),
            Self::Video(options) => (video_player::run(options), "the video window couldn't open"),
        };
        match outcome {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                tracing::error!(%error, "{failed}");
                std::process::exit(1)
            }
        }
    }
}
