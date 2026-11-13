//! Pasting outside a text field (#132): a link, magnet link or absolute
//! path pasted anywhere but a field asks for its install, as the webview's
//! `InstallIngress` paste listener does. Blitz hands the clipboard only to
//! a focused field, so Ctrl V (⌘ V) outside one ([`read`], from the input
//! layer) asks the window's event loop (`launch::Loop`) to read the
//! clipboard; [`follow`] takes the text when it arrives and tells
//! [`pastes`]' listeners. What [`installation_text`] accepts goes to the
//! install confirmation; nothing starts until it's confirmed.

use dioxus_native::winit::data_transfer::TypeHint;
use dioxus_native::winit::event::WindowEvent;
use dioxus_native::winit::event_loop::{ActiveEventLoop, AsyncRequestSerial};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use tokio::sync::broadcast;

/// Where the text pasted outside a field is told.
pub fn pastes() -> &'static broadcast::Sender<String> {
    static PASTES: OnceLock<broadcast::Sender<String>> = OnceLock::new();
    PASTES.get_or_init(|| broadcast::channel(4).0)
}

/// The clipboard reads asked for, waiting to arrive.
fn pending() -> &'static Mutex<HashSet<AsyncRequestSerial>> {
    static PENDING: OnceLock<Mutex<HashSet<AsyncRequestSerial>>> = OnceLock::new();
    PENDING.get_or_init(Mutex::default)
}

/// Reads the clipboard for a paste outside a text field; its text reaches
/// [`pastes`]. Tests paste what [`tests::set_clipboard`] holds.
pub fn read() {
    #[cfg(test)]
    if let Some(text) = tests::clipboard() {
        let _ = pastes().send(text);
        return;
    }
    crate::launch::ask(crate::launch::Request::Paste);
}

/// Asks the clipboard for its text, on the event loop.
pub fn fetch(event_loop: &dyn ActiveEventLoop) {
    if let Ok(Some(id)) = event_loop.clipboard()
        && let Ok(serial) = event_loop.fetch_data_transfer(id, &TypeHint::Plaintext)
    {
        pending()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(serial);
    }
}

/// Takes the clipboard text [`fetch`] asked for; whether `event` was it.
pub fn follow(event: &WindowEvent) -> bool {
    let WindowEvent::DataTransferReceived { serial, value, .. } = event else {
        return false;
    };
    let mut pending = pending().lock().unwrap_or_else(|e| e.into_inner());
    if !pending.contains(serial) {
        return false;
    }
    match value.try_as_string() {
        // Still arriving: it comes again.
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok(text) => {
            pending.remove(serial);
            let _ = pastes().send(text);
        }
        Err(_) => {
            pending.remove(serial);
        }
    }
    true
}

/// Puts `text` on the clipboard, for the Copy buttons. Unit tests copy
/// nothing: the system clipboard is the display's, which is the
/// developer's desktop when the tests run there (#122).
pub fn copy_text(text: String) -> Result<(), String> {
    #[cfg(test)]
    {
        drop(text);
        Ok(())
    }
    #[cfg(not(test))]
    caskade_app::commands::clipboard::clipboard_write_text(text)
}

/// What a paste can install, as the webview's `installationText`: one
/// http(s) URL without credentials, a magnet link with a BitTorrent info
/// hash, or an absolute path; no line breaks, at most 32 KiB.
pub fn installation_text(value: &str) -> Option<String> {
    let text = value.trim();
    if text.is_empty() || text.len() > 32 * 1024 || text.contains(['\r', '\n', '\0']) {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let url = url::Url::parse(text).ok()?;
        let plain = url.host_str().is_some_and(|host| !host.is_empty())
            && url.username().is_empty()
            && url.password().is_none();
        return plain.then(|| text.to_owned());
    }
    if lower.starts_with("magnet:?") {
        let url = url::Url::parse(text).ok()?;
        let hashed = url.query_pairs().any(|(key, value)| {
            key.eq_ignore_ascii_case("xt")
                && value
                    .to_ascii_lowercase()
                    .strip_prefix("urn:btih:")
                    .and_then(|hash| hash.chars().next())
                    .is_some_and(|first| first.is_ascii_alphanumeric())
        });
        return hashed.then(|| text.to_owned());
    }
    absolute(text).then(|| text.to_owned())
}

/// `/…`, `C:\…` or `C:/…`, or `\\server\…`.
fn absolute(text: &str) -> bool {
    let bytes = text.as_bytes();
    if text.starts_with('/') {
        return true;
    }
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
    {
        return true;
    }
    text.strip_prefix("\\\\")
        .and_then(|rest| rest.split_once('\\'))
        .is_some_and(|(server, _)| !server.is_empty())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    static CLIPBOARD: Mutex<Option<String>> = Mutex::new(None);
    /// What a paste reads in tests; `None` reads nothing.
    pub fn set_clipboard(text: Option<&str>) {
        *caskade_core::lock(&CLIPBOARD) = text.map(str::to_owned);
    }

    pub(super) fn clipboard() -> Option<String> {
        caskade_core::lock(&CLIPBOARD).clone()
    }

    #[test]
    fn accepts_what_the_webview_accepts() {
        for accepted in [
            "https://example.com/game.zip",
            "  HTTP://example.com/a?b=c  ",
            "magnet:?xt=urn:btih:abcdef0123&dn=Game",
            "magnet:?dn=Game&xt=urn:btih:ABCDEF",
            "magnet:?dn=Game&xt=urn%3Abtih%3A0123456789abcdef0123456789abcdef01234567",
            "magnet:?xt=urn:btmh:unsupported&xt=urn%3Abtih%3A0123456789abcdef0123456789abcdef01234567",
            "/home/me/Games/Setup.exe",
            "C:\\Games\\Setup.exe",
            "d:/Games/setup.exe",
            "\\\\server\\share\\game.zip",
        ] {
            assert_eq!(
                installation_text(accepted).as_deref(),
                Some(accepted.trim()),
                "{accepted}"
            );
        }
        for refused in [
            "",
            "just some words",
            "https://user:secret@example.com/game.zip",
            "https://me@example.com/game.zip",
            "https://",
            "magnet:?dn=No%20hash",
            "magnet:?dn=Game#&xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
            "magnet:?dn=xt%3Durn%3Abtih%3A0123456789abcdef0123456789abcdef01234567",
            "ftp://example.com/game.zip",
            "relative/path/game.zip",
            "https://example.com/a\nhttps://example.com/b",
            "\\\\\\share",
        ] {
            assert_eq!(installation_text(refused), None, "{refused:?}");
        }
        let long = format!("https://example.com/{}", "a".repeat(32 * 1024));
        assert_eq!(installation_text(&long), None);
    }
}
