//! The library error dialog's Restore from backup… (#192); staging and
//! applying restores are the app layer's (`caskade_app::recovery`).

pub use caskade_app::recovery::*;
use caskade_core::AppPaths;

/// From the library error dialog: asks for a backup, stages it and
/// restarts into it. Quits if none is chosen or it can't be staged.
pub fn restore_from_dialog<R: tauri::Runtime>(app: &tauri::AppHandle<R>, paths: AppPaths) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let handle = app.clone();
    app.dialog()
        .file()
        .set_title(caskade_core::tr!("Choose a Caskade backup"))
        .add_filter(caskade_core::tr!("Caskade backups"), &["zip"])
        .pick_file(move |file| {
            let Some(archive) = file.and_then(|file| file.into_path().ok()) else {
                handle.exit(1);
                return;
            };
            caskade_app::runtime::spawn(async move {
                match stage(&paths, &archive).await {
                    Ok(()) => handle.restart(),
                    Err(error) => {
                        let quit = handle.clone();
                        handle
                            .dialog()
                            .message(caskade_core::tr!(
                                "This backup can't be restored.\n\n{error}",
                                error = error
                            ))
                            .title("Caskade")
                            .kind(MessageDialogKind::Error)
                            .show(move |_| quit.exit(1));
                    }
                }
            });
        });
}
