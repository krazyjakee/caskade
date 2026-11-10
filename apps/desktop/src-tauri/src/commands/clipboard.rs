//! The clipboard, for what a pad presses (#247): the on-screen keyboard's
//! Paste and the Copy buttons. A webview reads and writes the clipboard only
//! in answer to a click or a key, and a pad's press reaches the page as an
//! event from the core, so these go through `arboard` (text only).
use caskade_app::commands::clipboard as app;

/// The clipboard's text; empty when it holds none or can't be read.
#[tauri::command]
#[specta::specta]
pub fn clipboard_read_text() -> String {
    app::clipboard_read_text()
}

/// Puts `text` on the clipboard.
#[tauri::command]
#[specta::specta]
pub fn clipboard_write_text(text: String) -> Result<(), String> {
    app::clipboard_write_text(text)
}
