//! Games' videos (trailers from IGDB) play in the video window: this
//! executable, started again with [`VIDEO_ARG`]. One plays at a time;
//! another replaces it.
use caskade_core::launch::{FULLSCREEN_ARG, GEOMETRY_ARG};
use std::{
    process::{Child, Command, Stdio},
    sync::Mutex,
};

/// The video-window argument the executable dispatches on, before the app
/// starts.
pub const VIDEO_ARG: &str = "--video";
/// The window's title: the game's, and the video's name.
pub const TITLE_ARG: &str = "--title";

/// A YouTube video id: 11 letters, digits, `-` or `_`.
pub fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The video window open now, if any.
#[derive(Default)]
pub struct VideoWindow(Mutex<Option<Child>>);

impl VideoWindow {
    /// Closes the open video window, if any.
    fn close(&self) {
        let mut open = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(mut child) = open.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Where the video window goes: fullscreen (console mode), or over the
/// app's window at `geometry` (`WindowPlacement::arg`).
pub enum VideoPlacement {
    Fullscreen,
    Over(String),
    Anywhere,
}

/// Plays the YouTube video `video_id` in the video window, titled `title`,
/// placed as `placement` says.
pub fn video_play(
    window: &VideoWindow,
    video_id: &str,
    title: &str,
    placement: VideoPlacement,
) -> Result<(), String> {
    if !is_video_id(video_id) {
        return Err(format!("{video_id} isn't a YouTube video"));
    }
    let program = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(program);
    command.args([VIDEO_ARG, video_id, TITLE_ARG, title]);
    match placement {
        VideoPlacement::Fullscreen => {
            command.arg(FULLSCREEN_ARG);
        }
        VideoPlacement::Over(geometry) => {
            command.args([GEOMETRY_ARG, &geometry]);
        }
        VideoPlacement::Anywhere => {}
    }
    // The window closes when its stdin does: when Caskade exits.
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    window.close();
    let child = command.spawn().map_err(|error| error.to_string())?;
    *window
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(child);
    Ok(())
}
