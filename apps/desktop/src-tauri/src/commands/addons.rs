//! The add-on catalogue (#181): its entries with what each means here
//! (installed, update, compatible), its status, a refresh, and its images
//! through the artwork cache, so nothing in the catalogue runs in the
//! webview. Plugins and themes only.
use crate::error::{CommandError, report};
use caskade_addons::{
    AddonKind, AddonOs, AppMode, CatalogueItem, CatalogueService, CatalogueStatus, HostFacts,
    InstalledAddon, evaluate,
};
use caskade_core::{
    artwork::{ArtworkCache, CachedArtwork},
    themes::{ThemeService, ThemeSource},
};
use caskade_plugin_host::PluginHost;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{Manager, Runtime, State};

/// The catalogue as Settings shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueView {
    pub status: CatalogueStatus,
    pub items: Vec<CatalogueItem>,
}

/// The cache the catalogue's images go through; `None` without one.
pub struct AddonMedia(pub Option<ArtworkCache>);

/// The catalogue client: `CASKADE_CATALOGUE_URL` and
/// `CASKADE_CATALOGUE_PUBKEY` point a run at another catalogue (a local
/// server while developing), else the curated origin checked against the
/// release key the updater uses.
pub fn service(
    app: &tauri::AppHandle<impl Runtime>,
    dir: std::path::PathBuf,
) -> Option<CatalogueService> {
    let origin = std::env::var("CASKADE_CATALOGUE_URL")
        .ok()
        .unwrap_or_else(|| caskade_addons::DEFAULT_ORIGIN.to_owned());
    let key = std::env::var("CASKADE_CATALOGUE_PUBKEY").ok().or_else(|| {
        app.config()
            .plugins
            .0
            .get("updater")
            .and_then(|updater| updater.get("pubkey"))
            .and_then(|key| key.as_str())
            .map(str::to_owned)
    });
    CatalogueService::new(dir, &origin, key.as_deref())
        .inspect_err(|error| tracing::error!(%error, "the add-on catalogue is unavailable"))
        .ok()
}

/// Whether the catalogue is served from this machine (a local server):
/// then its images may come from there too.
pub fn local_origin(service: &CatalogueService) -> bool {
    service.origin().host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    })
}

/// What is installed: plugins by manifest id and version, and themes
/// (built-in and installed; a plugin's bundled themes are the plugin's).
pub(crate) async fn installed<R: Runtime>(app: &tauri::AppHandle<R>) -> Vec<InstalledAddon> {
    let mut installed = Vec::new();
    if let Some(host) = app.try_state::<PluginHost>() {
        installed.extend(
            host.list()
                .await
                .into_iter()
                .filter(|plugin| !plugin.dev)
                .map(|plugin| InstalledAddon {
                    id: plugin.id,
                    kind: AddonKind::Plugin,
                    version: plugin.version,
                }),
        );
    }
    if let Some(themes) = app.try_state::<ThemeService>() {
        installed.extend(
            themes
                .list()
                .into_iter()
                .filter(|theme| matches!(theme.source, ThemeSource::Builtin | ThemeSource::User))
                .map(|theme| InstalledAddon {
                    id: theme.id,
                    kind: AddonKind::Theme,
                    version: Some(theme.version),
                }),
        );
    }
    installed
}

pub(crate) fn host_facts(mode: AppMode) -> HostFacts {
    HostFacts {
        plugin_api: caskade_plugin_host::manifest::api_supported,
        plugin_api_version: caskade_plugin_host::HOST_API.to_string(),
        os: AddonOs::current(),
        mode,
    }
}

fn catalogue_service<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<State<'_, CatalogueService>, CommandError> {
    app.try_state::<CatalogueService>().ok_or_else(|| {
        report(caskade_core::Error::Invalid(caskade_core::tr!(
            "the add-on catalogue is unavailable"
        )))
    })
}

/// The catalogue as last fetched (offline too), with what each entry means
/// here in `mode`. Fetches nothing.
#[tauri::command]
#[specta::specta]
pub async fn addons_catalogue<R: Runtime>(
    app: tauri::AppHandle<R>,
    mode: AppMode,
) -> Result<CatalogueView, CommandError> {
    let service = catalogue_service(&app)?;
    let items = evaluate(
        &service.catalogue(),
        &installed(&app).await,
        &host_facts(mode),
    );
    Ok(CatalogueView {
        status: service.status(),
        items,
    })
}

/// Fetches the catalogue again; on failure the last good one stays and
/// the status says why.
#[tauri::command]
#[specta::specta]
pub async fn addons_catalogue_refresh<R: Runtime>(
    app: tauri::AppHandle<R>,
    mode: AppMode,
) -> Result<CatalogueView, CommandError> {
    let service = catalogue_service(&app)?.inner().clone();
    service.refresh().await;
    addons_catalogue(app, mode).await
}

/// One of an entry's screenshots, fetched and re-encoded by the artwork
/// cache (an image, never markup or script). Only the catalogue's own
/// images can be asked for; `null` when it isn't there or can't load.
#[tauri::command]
#[specta::specta]
pub async fn addons_media<R: Runtime>(
    app: tauri::AppHandle<R>,
    media: State<'_, AddonMedia>,
    id: String,
    index: u32,
) -> Result<Option<CachedArtwork>, CommandError> {
    let service = catalogue_service(&app)?;
    let Some(cache) = &media.0 else {
        return Ok(None);
    };
    let catalogue = service.catalogue();
    let Some(image) = catalogue
        .entry(&id)
        .and_then(|entry| entry.screenshots.get(index as usize))
    else {
        return Ok(None);
    };
    match cache.download_metadata(&image.url).await {
        Ok(cached) => Ok(Some(cached)),
        Err(error) => {
            tracing::debug!(%error, addon = %id, "catalogue image not loaded");
            Ok(None)
        }
    }
}

#[cfg(test)]
#[path = "addons_tests.rs"]
mod tests;
