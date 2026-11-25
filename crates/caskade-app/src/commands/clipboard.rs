//! The clipboard, for Copy buttons (#247): text only, through `arboard`.

use std::sync::{Mutex, OnceLock};
/// One clipboard for the app's lifetime: on X11 and Wayland, text written
/// stays on the clipboard only while its owner lives.
fn clipboard() -> &'static Mutex<Option<arboard::Clipboard>> {
    static CLIPBOARD: OnceLock<Mutex<Option<arboard::Clipboard>>> = OnceLock::new();
    CLIPBOARD.get_or_init(|| Mutex::new(arboard::Clipboard::new().ok()))
}

/// Puts `text` on the clipboard.
pub fn clipboard_write_text(text: String) -> Result<(), String> {
    clipboard()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .ok_or_else(|| caskade_core::tr!("the clipboard isn't available"))?
        .set_text(text)
        .map_err(|_| caskade_core::tr!("the clipboard couldn't be written"))
}
