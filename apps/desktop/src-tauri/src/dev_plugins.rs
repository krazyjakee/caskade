//! Dev plugins (#131): in debug builds, `caskade dev <folder>` (also from a
//! second launch, forwarded by single-instance) and Settings → Plugins "Load
//! unpacked" load a plugin folder in place, without review, and watch it.
//! Release builds ignore both (CSK-06).
use caskade_plugin_host::{PluginHost, dev};
use std::path::{Path, PathBuf};
use tauri::Manager;

/// The watched dev folders; dropping a watch stops it.
#[derive(Default)]
pub struct DevPlugins(tokio::sync::Mutex<Vec<dev::DevWatch>>);

/// `caskade dev <folder>`: the folder, relative to `cwd`. Only as the first
/// argument, and never in release builds: a dev plugin skips the review.
pub fn folder_from_args(args: &[String], cwd: &Path) -> Option<PathBuf> {
    if !cfg!(debug_assertions) || args.get(1)? != "dev" {
        return None;
    }
    let folder = PathBuf::from(args.get(2)?);
    Some(if folder.is_absolute() {
        folder
    } else {
        cwd.join(folder)
    })
}

/// Loads `folder` as a dev plugin unless it is already watched.
pub async fn load<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    folder: &Path,
) -> Result<(), String> {
    // A theme folder is watched as a dev theme instead (#140).
    if crate::commands::themes::is_theme_folder(folder) {
        return crate::commands::themes::load_dev(app, folder).map(|_| ());
    }
    let (Some(host), Some(bus), Some(dev_plugins)) = (
        app.try_state::<PluginHost>(),
        app.try_state::<caskade_core::bus::Bus>(),
        app.try_state::<DevPlugins>(),
    ) else {
        return Err(caskade_core::tr!("plugins are not ready"));
    };
    let folder = folder
        .canonicalize()
        .map_err(|error| format!("{}: {error}", folder.display()))?;
    let mut watches = dev_plugins.0.lock().await;
    if watches.iter().any(|watch| watch.folder() == folder) {
        return Ok(());
    }
    let watch = dev::watch(
        host.inner().clone(),
        bus.inner().clone(),
        &folder,
        dev::DEBOUNCE,
    )
    .await
    .map_err(|error| error.to_string())?;
    watches.push(watch);
    Ok(())
}

/// [`load`] on the async runtime, logging a failure.
pub fn load_in_background<R: tauri::Runtime>(app: &tauri::AppHandle<R>, folder: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = load(&app, &folder).await {
            tracing::warn!(%error, "could not load the dev plugin");
        }
    });
}

/// Settings → Plugins "Load unpacked": debug builds only.
#[tauri::command]
#[specta::specta]
pub async fn plugin_load_unpacked(app: tauri::AppHandle, folder: String) -> Result<(), String> {
    if !cfg!(debug_assertions) {
        return Err(caskade_core::tr!(
            "Load unpacked is only available in development builds"
        ));
    }
    load(&app, Path::new(&folder)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(debug_assertions)]
    #[test]
    fn dev_argument_names_a_folder_relative_to_the_working_directory() {
        let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        let cwd = Path::new("/work");
        assert_eq!(
            folder_from_args(&args(&["caskade", "dev", "my-plugin"]), cwd),
            Some(PathBuf::from("/work/my-plugin"))
        );
        assert_eq!(
            folder_from_args(&args(&["caskade", "dev", "/abs/plugin"]), cwd),
            Some(PathBuf::from("/abs/plugin"))
        );
        assert_eq!(folder_from_args(&args(&["caskade", "dev"]), cwd), None);
        assert_eq!(folder_from_args(&args(&["caskade"]), cwd), None);
    }

    #[test]
    fn dev_counts_only_as_the_first_argument() {
        let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        let cwd = Path::new("/work");
        for list in [
            &["caskade", "--console", "dev", "/tmp/evil"][..],
            &["caskade", "caskade://install?path=/x", "dev", "/tmp/evil"],
            &["dev", "/tmp/evil"],
        ] {
            assert_eq!(folder_from_args(&args(list), cwd), None, "{list:?}");
        }
    }
}
