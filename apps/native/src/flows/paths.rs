//! Choosing files and folders for a flow, as the webview's `PathPicker`:
//! the OS's own dialog (#88) in desktop mode, the in-app `FolderBrowser`
//! in console mode, where a pad has to drive it. A
//! [`PathPickerProvider`] around the views says which; outside one, the
//! OS's dialog.

use dioxus_native::prelude::*;
use tokio::sync::oneshot;

use super::folder_browser::FolderBrowser;

/// What a picker chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    File,
    Folder,
}

/// What to ask for.
#[derive(Clone, Debug, PartialEq)]
pub struct PathRequest {
    pub kind: PickKind,
    /// The dialog's title: what is being picked.
    pub title: String,
    /// File mode: the extensions that may be picked (without the dot);
    /// other files show dimmed. Every file without any.
    pub extensions: Vec<String>,
    /// Where to start; home without it.
    pub start_at: Option<String>,
}

impl PathRequest {
    pub fn new(kind: PickKind, title: String) -> Self {
        Self {
            kind,
            title,
            extensions: Vec::new(),
            start_at: None,
        }
    }
}

/// One path chosen in the OS's dialog for `request`; `None` when it's
/// cancelled. The dialog runs off the UI's thread, as `dialogs` needs.
pub async fn pick_native(request: PathRequest) -> Option<String> {
    caskade_app::runtime::spawn(async move {
        tokio::task::spawn_blocking(move || match request.kind {
            PickKind::File => crate::dialogs::pick_file(&request.title, None),
            PickKind::Folder => crate::dialogs::pick_folder(&request.title),
        })
        .await
        .ok()
        .flatten()
    })
    .await
    .ok()
    .flatten()
    .map(|path| path.to_string_lossy().into_owned())
}

/// The request the in-app browser shows, and who waits for its answer.
struct Pending {
    id: u64,
    request: PathRequest,
    answer: oneshot::Sender<Option<String>>,
}

/// The picker for this mode: a cheap handle to copy into handlers.
#[derive(Clone, Copy)]
pub struct PathPicker {
    /// Console mode, where `pick` shows the in-app browser.
    in_app: Option<Signal<bool>>,
    pending: Option<Signal<Option<Pending>>>,
}

impl PathPicker {
    /// Console mode: `pick` shows the in-app browser. A caller whose OS
    /// dialog takes several paths takes one from the browser instead.
    pub fn in_app(&self) -> bool {
        self.in_app.is_some_and(|in_app| *in_app.peek())
    }

    /// A path, or `None` when cancelled. In the browser, a second request
    /// replaces the first, which is cancelled.
    pub async fn pick(self, request: PathRequest) -> Option<String> {
        match self.pending {
            Some(mut pending) if self.in_app() => {
                let (answer, wait) = oneshot::channel();
                if let Some(earlier) = pending.write().take() {
                    let _ = earlier.answer.send(None);
                }
                pending.set(Some(Pending {
                    id: next_id(),
                    request,
                    answer,
                }));
                wait.await.ok().flatten()
            }
            _ => pick_native(request).await,
        }
    }
}

fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The picker for this mode; outside a [`PathPickerProvider`], the OS's.
pub fn use_path_picker() -> PathPicker {
    try_use_context::<PathPicker>().unwrap_or(PathPicker {
        in_app: None,
        pending: None,
    })
}

/// Gives the views below a picker: the in-app browser when `in_app`
/// (console mode), else the OS's dialogs.
#[component]
pub fn PathPickerProvider(in_app: bool, children: Element) -> Element {
    let mut mode = use_signal(|| in_app);
    if *mode.peek() != in_app {
        mode.set(in_app);
    }
    let mut pending = use_signal(|| None::<Pending>);
    use_context_provider(|| PathPicker {
        in_app: Some(mode),
        pending: Some(pending),
    });
    // A picker still open when this goes is cancelled.
    use_drop(move || {
        if let Some(open) = pending.write().take() {
            let _ = open.answer.send(None);
        }
    });
    let shown = pending
        .read()
        .as_ref()
        .map(|open| (open.id, open.request.clone()));
    rsx! {
        {children}
        if let Some((id, request)) = shown {
            FolderBrowser {
                key: "{id}",
                request,
                on_close: move |path: Option<String>| {
                    if let Some(open) = pending.write().take() {
                        let _ = open.answer.send(path);
                    }
                },
            }
        }
    }
}

/// A path as people write it: Windows' `\\?\` prefixes dropped, as the
/// webview's `formatPath`.
pub fn format_path(path: &str) -> String {
    let unc = path
        .get(..8)
        .is_some_and(|start| start.eq_ignore_ascii_case("\\\\?\\UNC\\"));
    if unc {
        return format!("\\\\{}", &path[8..]);
    }
    let bytes = path.as_bytes();
    let drive = bytes.len() > 6
        && path.starts_with("\\\\?\\")
        && bytes[4].is_ascii_alphabetic()
        && bytes[5] == b':'
        && bytes[6] == b'\\';
    if drive {
        return path[4..].to_owned();
    }
    path.to_owned()
}

#[cfg(test)]
mod tests {
    use super::format_path;

    #[test]
    fn paths_lose_windows_long_path_prefixes_as_in_the_webview() {
        assert_eq!(format_path("\\\\?\\C:\\Games\\a.exe"), "C:\\Games\\a.exe");
        assert_eq!(format_path("\\\\?\\UNC\\nas\\games"), "\\\\nas\\games");
        assert_eq!(format_path("\\\\?\\unc\\nas\\games"), "\\\\nas\\games");
        assert_eq!(format_path("/home/me/game.sh"), "/home/me/game.sh");
        assert_eq!(format_path("\\\\?\\Volume{x}\\a"), "\\\\?\\Volume{x}\\a");
    }
}
