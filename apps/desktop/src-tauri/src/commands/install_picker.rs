//! Native chooser boundary; mock-runtime contracts inject a picker without
//! opening an OS dialog or granting filesystem/shell access to the webview.
pub use caskade_app::commands::install_picker::InstallPicker;
use std::sync::Arc;
use tauri_plugin_dialog::{DialogExt, FilePath};

fn native_path(path: FilePath) -> Result<String, String> {
    path.into_path()
        .map_err(|e| e.to_string())?
        .into_os_string()
        .into_string()
        .map_err(|_| caskade_core::tr!("selected path is not UTF-8"))
}
/// The folder the user picks in the OS's dialog titled `title`; `None` when
/// they cancel.
pub(super) async fn pick_folder<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    title: String,
) -> Result<Option<String>, String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .pick_folder(move |path| {
            let _ = send.send(path);
        });
    receive
        .await
        .map_err(|_| caskade_core::tr!("The folder chooser closed without an answer."))?
        .map(native_path)
        .transpose()
}

/// The OS's file dialogs, through Tauri's dialog plugin.
pub fn native<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> InstallPicker {
    let files_app = app.clone();
    InstallPicker {
        files: Arc::new(move || {
            let app = files_app.clone();
            Box::pin(async move {
                let (send, receive) = tokio::sync::oneshot::channel();
                app.dialog()
                    .file()
                    .set_title(caskade_core::tr!("Choose games to install"))
                    .pick_files(move |paths| {
                        let _ = send.send(paths);
                    });
                receive
                    .await
                    .map_err(|_| caskade_core::tr!("file dialog closed without a response"))?
                    .unwrap_or_default()
                    .into_iter()
                    .map(native_path)
                    .collect()
            })
        }),
        folder: Arc::new(move || {
            let app = app.clone();
            Box::pin(async move {
                pick_folder(&app, caskade_core::tr!("Choose game folder to install")).await
            })
        }),
    }
}
