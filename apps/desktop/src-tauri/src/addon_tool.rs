//! The catalogue's maintainer tool (#184, `caskade-catalogue`): build
//! packages and a catalogue from a source file, and check a staged or
//! published catalogue with the checks an install makes. Core's
//! `addons::author` packs and reads; this adds what the app does with a
//! package: the plugin host's preview and a start in a scratch host, and
//! the theme service's check. See `docs/plugins/publishing.md`.

use caskade_addons::{AddonKind, author};
use caskade_core::{
    db::LibraryStore,
    themes::{ThemeService, ThemeSource},
};
use caskade_plugin_host::{HostOptions, Limits, PluginHost, PluginServices, PluginStatus};
use std::path::Path;

/// Checks `location` (a path or an address) as `author::check` does, then
/// each plugin release with the plugin host (its manifest, that its
/// API and permissions are the ones the catalogue shows, and that it
/// starts) and each theme release with the theme service. Scratch files
/// go under `work`.
pub async fn check(
    location: &str,
    public_key: Option<&str>,
    work: &Path,
) -> Result<author::Checked, String> {
    let mut checked = author::check(location, public_key, work).await?;
    let builtins = ThemeService::new(work.join("no-themes"));
    for (index, checked_release) in checked.releases.clone().iter().enumerate() {
        let author::CheckedRelease {
            entry,
            release,
            root,
        } = checked_release;
        let at = format!("{} {}", entry.id, release.version);
        let problems = match entry.kind {
            AddonKind::Plugin => {
                check_plugin(release, root, &work.join(format!("host-{index}"))).await
            }
            AddonKind::Theme => {
                let mut problems: Vec<String> = ThemeService::check_folder(root)
                    .into_iter()
                    .map(|problem| format!("{}: {}", problem.file, problem.message))
                    .collect();
                if builtins
                    .list()
                    .iter()
                    .any(|theme| theme.id == entry.id && theme.source == ThemeSource::Builtin)
                {
                    problems.push("a built-in theme has this id, so installs refuse it".to_owned());
                }
                problems
            }
        };
        checked.problems.extend(
            problems
                .into_iter()
                .map(|problem| format!("{at}: {problem}")),
        );
    }
    Ok(checked)
}

async fn check_plugin(
    release: &caskade_addons::CatalogueRelease,
    root: &Path,
    scratch: &Path,
) -> Vec<String> {
    let mut problems = Vec::new();
    let manifest: serde_json::Value = std::fs::read(root.join("plugin.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let api = manifest.get("api").and_then(|api| api.as_str());
    if api.map(str::trim) != release.api.as_deref().map(str::trim) {
        problems.push(format!(
            "the catalogue's API range isn't the manifest's ({})",
            api.unwrap_or("none")
        ));
    }
    let mut asked = author::permission_words(&manifest);
    let mut shown = release.permissions.clone();
    asked.sort();
    shown.sort();
    if asked != shown {
        problems.push(format!(
            "the catalogue shows permissions [{}], the manifest asks for [{}]",
            shown.join(", "),
            asked.join(", ")
        ));
    }
    if let Err(problem) = start(root, scratch).await {
        problems.push(problem);
    }
    problems
}

/// Installs the package into a scratch host, enables it as a reviewed
/// install does and starts its backend.
async fn start(root: &Path, scratch: &Path) -> Result<(), String> {
    let store = LibraryStore::in_memory().await.map_err(|e| e.to_string())?;
    let plugins = scratch.join("plugins");
    let host = PluginHost::new(
        PluginServices {
            store,
            bus: None,
            settings: None,
        },
        HostOptions {
            folders: vec![plugins.clone()],
            data: scratch.join("data"),
            limits: Limits::default(),
        },
    )
    .map_err(|e| e.to_string())?;
    let preview = host.preview_folder(root).await.map_err(|e| e.to_string())?;
    if preview.plugin.status == PluginStatus::Invalid || !preview.plugin.errors.is_empty() {
        return Err(preview
            .plugin
            .errors
            .iter()
            .map(|error| format!("{}: {}", error.key, error.message))
            .collect::<Vec<_>>()
            .join("; "));
    }
    let id = preview.plugin.id.clone();
    copy(root, &plugins.join(&id)).map_err(|e| e.to_string())?;
    host.discover().await.map_err(|e| e.to_string())?;
    host.set_enabled(&id, true)
        .await
        .map_err(|e| e.to_string())?;
    if preview.plugin.has_backend {
        host.activate(&id)
            .await
            .map_err(|e| format!("it didn't start: {e}"))?;
        let _ = host.deactivate(&id).await;
    }
    Ok(())
}

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
