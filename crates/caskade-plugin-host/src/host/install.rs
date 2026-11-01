//! Installing plugin folders: the review before, the copy, and uninstalling.

use super::{Entry, HostError, PluginHost, PluginPreview, PluginSummary, Result, folder_name};
use crate::{locales::PluginLocales, manifest::Manifest, write};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Plugin folders are small; anything bigger is refused.
const INSTALL_BYTES_MAX: u64 = 512 << 20;
const INSTALL_FILES_MAX: usize = 20_000;

/// Copies a plugin folder; symlinks and special files are refused so the
/// copy can't reach outside it.
fn copy_plugin(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut pending = vec![PathBuf::new()];
    let (mut bytes, mut files) = (0u64, 0usize);
    let staging = to.with_extension("installing");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    while let Some(relative) = pending.pop() {
        for entry in std::fs::read_dir(from.join(&relative))? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = relative.join(entry.file_name());
            files += 1;
            if files > INSTALL_FILES_MAX {
                return Err(std::io::Error::other("the plugin has too many files"));
            }
            if kind.is_dir() {
                std::fs::create_dir_all(staging.join(&path))?;
                pending.push(path);
            } else if kind.is_file() {
                bytes += entry.metadata()?.len();
                if bytes > INSTALL_BYTES_MAX {
                    return Err(std::io::Error::other("the plugin is larger than 512 MiB"));
                }
                std::fs::copy(entry.path(), staging.join(&path))?;
            } else {
                return Err(std::io::Error::other(format!(
                    "{} is a link or special file",
                    path.display()
                )));
            }
        }
    }
    std::fs::rename(&staging, to)
}

impl PluginHost {
    /// What installing `folder` would add, without copying or running
    /// anything: the user reviews this before granting (#124).
    pub async fn preview_folder(&self, folder: &Path) -> Result<PluginPreview> {
        let manifest = Manifest::read(folder);
        let id = manifest
            .as_ref()
            .map_or_else(|_| folder_name(folder), |manifest| manifest.id.clone());
        let installed = manifest
            .as_ref()
            .ok()
            .and_then(|manifest| self.manifest(&manifest.id))
            .map(|manifest| manifest.version.to_string());
        let entry = Entry {
            folder: folder.to_path_buf(),
            manifest,
            locales: Arc::new(PluginLocales::read(folder)),
            dev: false,
            enabled: false,
            needs_consent: false,
            live: tokio::sync::Mutex::default(),
        };
        let mut plugin = entry.summary(&id, None, &self.language.current());
        plugin.settings_error = entry
            .manifest
            .as_ref()
            .ok()
            .and_then(crate::settings::check);
        Ok(PluginPreview {
            plugin,
            installed_version: installed,
        })
    }

    /// Copies a reviewed plugin folder into the plugins folder as `<id>`
    /// (replacing an older copy) and enables it with what it asks for.
    pub async fn install_folder(&self, folder: &Path) -> Result<PluginSummary> {
        let manifest = Manifest::read(folder).map_err(|errors| HostError::Invalid {
            id: folder.display().to_string(),
            errors,
        })?;
        let root = self
            .options
            .folders
            .first()
            .ok_or_else(|| HostError::Unknown("no plugins folder".into()))?
            .clone();
        let target = root.join(&manifest.id);
        let source = folder.canonicalize()?;
        if target.exists() && target.canonicalize()? != source {
            self.deactivate(&manifest.id).await.ok();
            std::fs::remove_dir_all(&target)?;
        }
        if !target.exists() {
            let (from, to) = (source.clone(), target.clone());
            caskade_jobs::blocking("Installing plugin", move || copy_plugin(&from, &to))
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))??;
        }
        self.discover().await?;
        self.set_enabled(&manifest.id, true).await
    }

    /// Removes a plugin and everything it left behind: its state and grants,
    /// key-value store, settings and data folder. Its folder is deleted only
    /// when it sits directly inside a plugins folder (an installed copy),
    /// never a folder loaded in place.
    pub async fn uninstall(&self, id: &str) -> Result<()> {
        self.uninstall_with(id, false).await
    }

    /// [`Self::uninstall`], keeping its data when `keep_data` (#182): its
    /// key-value store, settings and data folder stay for a reinstall, and
    /// only its folder and its grant go.
    pub async fn uninstall_with(&self, id: &str, keep_data: bool) -> Result<()> {
        let entry = self.entry(id)?;
        self.deactivate(id).await.ok();
        let folder = entry.folder.canonicalize().unwrap_or(entry.folder.clone());
        let installed = self.options.folders.iter().any(|root| {
            let root = root.canonicalize().unwrap_or(root.clone());
            folder.parent() == Some(root.as_path())
        });
        if installed {
            std::fs::remove_dir_all(&folder)?;
        }
        if keep_data {
            // Not enabled and nothing granted: a reinstall is reviewed again.
            self.services
                .store
                .upsert_plugin_state(&caskade_core::db::PluginRow {
                    id: id.to_owned(),
                    enabled: false,
                    grants: serde_json::Value::Null,
                })
                .await?;
            write(&self.entries).remove(id);
            tracing::info!(plugin = id, installed, "plugin uninstalled; its data kept");
            self.changed(id);
            return Ok(());
        }
        self.services.store.delete_plugin_state(id).await?;
        if let Some(settings) = &self.services.settings {
            settings
                .remove(&crate::settings_namespace(id), true)
                .await
                .map_err(|error| caskade_core::Error::Invalid(error.to_string()))?;
        }
        let data = self.options.data.join(id);
        if data.exists() {
            std::fs::remove_dir_all(&data)?;
        }
        write(&self.entries).remove(id);
        write(&self.settings_errors).remove(id);
        tracing::info!(plugin = id, installed, "plugin uninstalled");
        self.changed(id);
        Ok(())
    }
}
