//! Native chooser boundary; mock-runtime contracts inject a picker without
//! opening an OS dialog or granting filesystem/shell access to the webview.
use std::{future::Future, pin::Pin, sync::Arc};
use tauri_plugin_dialog::{DialogExt, FilePath};

type Selection<T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send>>;
pub struct InstallPicker {
    pub files: Arc<dyn Fn() -> Selection<Vec<String>> + Send + Sync>,
    pub folder: Arc<dyn Fn() -> Selection<Option<String>> + Send + Sync>,
}
pub(super) fn native_path(path: FilePath) -> Result<String, String> {
    path.into_path()
        .map_err(|e| e.to_string())?
        .into_os_string()
        .into_string()
        .map_err(|_| caskade_core::tr!("selected path is not UTF-8"))
}
impl InstallPicker {
    pub fn native<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Self {
        let files_app = app.clone();
        Self {
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
                    let (send, receive) = tokio::sync::oneshot::channel();
                    app.dialog()
                        .file()
                        .set_title(caskade_core::tr!("Choose game folder to install"))
                        .pick_folder(move |path| {
                            let _ = send.send(path);
                        });
                    receive
                        .await
                        .map_err(|_| caskade_core::tr!("folder dialog closed without a response"))?
                        .map(native_path)
                        .transpose()
                })
            }),
        }
    }
}
