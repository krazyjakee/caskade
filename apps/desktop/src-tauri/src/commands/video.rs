//! Games' videos (trailers from IGDB) play in the video window
//! (`video_player.rs`): this executable, started again. One plays at a
//! time; another replaces it.
use crate::{commands::console, main_window, video_player};
use caskade_core::launch::{FULLSCREEN_ARG, GEOMETRY_ARG};
use std::{
    process::{Child, Command, Stdio},
    sync::Mutex,
};
use tauri::{AppHandle, Runtime, State};

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
    if !video_player::is_video_id(&video_id) {
        return Err(format!("{video_id} isn't a YouTube video"));
    }
    let program = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(program);
    command.args([
        video_player::VIDEO_ARG,
        &video_id,
        video_player::TITLE_ARG,
        &title,
    ]);
    if console::is_on(&app) {
        command.arg(FULLSCREEN_ARG);
    } else if let Some(placement) = main_window::placement(&app) {
        command.args([GEOMETRY_ARG, &placement.arg()]);
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
