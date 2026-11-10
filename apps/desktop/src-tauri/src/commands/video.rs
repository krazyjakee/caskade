//! Games' videos (trailers from IGDB) play in the video window
//! (`video_player.rs`): this executable, started again. One plays at a
//! time; another replaces it.
use crate::{commands::console, main_window};
pub use app::VideoWindow;
use caskade_app::commands::video::{self as app, VideoPlacement};
use tauri::{AppHandle, Runtime, State};

/// Plays the YouTube video `video_id` in the video window, titled `title`:
/// fullscreen in console mode, else over Caskade's window.
#[tauri::command]
#[specta::specta]
pub fn video_play<R: Runtime>(
    app: AppHandle<R>,
    window: State<'_, VideoWindow>,
    video_id: String,
    title: String,
) -> Result<(), String> {
    let placement = if console::is_on(&app) {
        VideoPlacement::Fullscreen
    } else if let Some(placement) = main_window::placement(&app) {
        VideoPlacement::Over(placement.arg())
    } else {
        VideoPlacement::Anywhere
    };
    app::video_play(&window, &video_id, &title, placement)
}
