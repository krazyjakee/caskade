//! The clipboard, for what a pad presses (#247): the on-screen keyboard's
//! Paste and the Copy buttons. A webview reads and writes the clipboard only
//! in answer to a click or a key, and a pad's press reaches the page as an
//! event from the core, so these go through `arboard` (text only).

use std::sync::{Mutex, OnceLock};

/// One clipboard for the app's lifetime: on X11 and Wayland, text written
/// stays on the clipboard only while its owner lives.
fn clipboard() -> &'static Mutex<Option<arboard::Clipboard>> {
    static CLIPBOARD: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();
    CLIPBOARD.get_or_init(|| Mutex::new(arboard::Clipboard::new().ok()))
}

/// The clipboard's text; empty when it holds none or can't be read.
#[tauri::command]
#[specta::specta]
pub fn clipboard_read_text() -> String {
    clipboard()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .and_then(|clipboard| clipboard.get_text().ok())
        .unwrap_or_default()
}

/// Puts `text` on the clipboard.
#[tauri::command]
#[specta::specta]
pub fn clipboard_write_text(text: String) -> Result<(), String> {
    clipboard()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .ok_or_else(|| caskade_core::tr!("the clipboard isn't available"))?
        .set_text(text)
        .map_err(|_| caskade_core::tr!("the clipboard couldn't be written"))
}
