//! Files dragged onto the main window (#99): the library installs what's
//! dropped on it (the webview's `useLibraryDrop`). winit hands the window's
//! drag events to the app's own handler (`launch::Loop`) first: [`follow`]
//! accepts a drag of files, asks for them when they're dropped, and tells
//! [`drops`]' listeners.

use dioxus_native::winit::data_transfer::TypeHint;
use dioxus_native::winit::event::WindowEvent;
use dioxus_native::winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, DndAction};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tokio::sync::broadcast;

/// A drag over the window.
#[derive(Clone, Debug, PartialEq)]
pub enum DropEvent {
    /// Files are being dragged over it.
    Over,
    /// The drag left, or was cancelled.
    Left,
    /// These files were dropped.
    Dropped(Vec<PathBuf>),
}

/// Where drags over the main window are told.
pub fn drops() -> &'static broadcast::Sender<DropEvent> {
    static DROPS: OnceLock<broadcast::Sender<DropEvent>> = OnceLock::new();
    DROPS.get_or_init(|| broadcast::channel(16).0)
}

/// The file lists asked for, waiting to arrive.
fn pending() -> &'static Mutex<HashSet<AsyncRequestSerial>> {
    static PENDING: OnceLock<Mutex<HashSet<AsyncRequestSerial>>> = OnceLock::new();
    PENDING.get_or_init(Mutex::default)
}

/// Local files among `uris` (`file:///…`, as a file manager drags them).
pub fn paths(uris: &[String]) -> Vec<PathBuf> {
    uris.iter()
        .map(|uri| uri.trim())
        .filter(|uri| !uri.is_empty() && !uri.starts_with('#'))
        .filter_map(|uri| url::Url::parse(uri).ok()?.to_file_path().ok())
        .collect()
}

/// Follows the main window's drag events; whether `event` was one.
pub fn follow(event_loop: &dyn ActiveEventLoop, event: &WindowEvent) -> bool {
    let send = |event: DropEvent| {
        let _ = drops().send(event);
    };
    match event {
        WindowEvent::DragEntered { id, .. } => {
            let _ = event_loop.set_valid_dnd_actions(*id, &[DndAction::Copy]);
            send(DropEvent::Over);
            true
        }
        WindowEvent::DragPosition { id, .. } => {
            let _ = event_loop.set_valid_dnd_actions(*id, &[DndAction::Copy]);
            true
        }
        WindowEvent::DragLeft { .. } => {
            send(DropEvent::Left);
            true
        }
        WindowEvent::DragDropped { id, .. } => {
            send(DropEvent::Left);
            if let Ok(serial) = event_loop.fetch_data_transfer(*id, &TypeHint::UriList) {
                pending()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(serial);
            }
            true
        }
        WindowEvent::DataTransferReceived { serial, value, .. } => {
            let ours = pending()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(serial);
            if !ours {
                return false;
            }
            if let Ok(uris) = value.try_as_uris() {
                let paths = paths(&uris);
                if !paths.is_empty() {
                    send(DropEvent::Dropped(paths));
                }
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_uris_become_local_paths() {
        let uris = [
            "file:///home/me/Games/Setup%20Game.exe".to_owned(),
            "# a comment line".to_owned(),
            "https://example.com/game.zip".to_owned(),
            String::new(),
        ];
        assert_eq!(
            paths(&uris),
            [PathBuf::from("/home/me/Games/Setup Game.exe")]
        );
    }
}
