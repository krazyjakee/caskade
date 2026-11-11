//! The plugin and theme catalogue for Settings → Plugins (#104, moved
//! from #108): the curated add-ons with what each means here, the records
//! (skipped and rejected releases, the update preference), installing,
//! updating and removing through the shared package service, screenshots
//! from the artwork cache, and what the user's other devices have; through
//! the app layer's commands as the webview's `api/addons.ts` does.
//! Changes arrive as `AddonsChanged`-style events (`Key::Addons`).

use std::sync::Arc;

pub use caskade_addons::package::{AddonRecord, AddonRecords, RejectedRelease, UpdatePreference};
pub use caskade_addons::{
    AddonKind, AppMode, CatalogueItem, CatalogueService, CatalogueStatus, ItemState,
};
use caskade_app::commands::addon_offers::{self, DeviceAddon};
use caskade_app::commands::addon_packages::{
    self as packages, AddonFolders, AddonJobs, AddonOutcome, AddonServices,
};
use caskade_app::commands::addons::{self as catalogue, AddonMedia, CatalogueView};
use caskade_app::events::Events;
use caskade_core::{
    artwork::CachedArtwork, bus::Bus, db::LibraryStore, devices::Devices, error::ErrorReport,
    settings::SettingsService, sync::SyncService, themes::ThemeService,
};
use caskade_plugin_host::PluginHost;

type Result<T> = std::result::Result<T, ErrorReport>;

struct Parts {
    catalogue: Option<CatalogueService>,
    plugins: Option<PluginHost>,
    themes: ThemeService,
    settings: SettingsService,
    store: LibraryStore,
    bus: Bus,
    events: Events,
    folders: AddonFolders,
    jobs: AddonJobs,
    media: AddonMedia,
    sync: Option<SyncService>,
    devices: Option<Devices>,
}

/// The catalogue and its packages over one app's services.
#[derive(Clone)]
pub struct Addons(Arc<Parts>);

/// What [`Addons::new`] takes.
pub struct AddonParts {
    pub catalogue: Option<CatalogueService>,
    pub plugins: Option<PluginHost>,
    pub themes: ThemeService,
    pub settings: SettingsService,
    pub store: LibraryStore,
    pub bus: Bus,
    pub events: Events,
    pub folders: AddonFolders,
    pub media: AddonMedia,
    pub sync: Option<SyncService>,
    pub devices: Option<Devices>,
}

impl Addons {
    pub fn new(parts: AddonParts) -> Self {
        Self(Arc::new(Parts {
            catalogue: parts.catalogue,
            plugins: parts.plugins,
            themes: parts.themes,
            settings: parts.settings,
            store: parts.store,
            bus: parts.bus,
            events: parts.events,
            folders: parts.folders,
            jobs: AddonJobs::default(),
            media: parts.media,
            sync: parts.sync,
            devices: parts.devices,
        }))
    }

    /// The running app's catalogue, telling `events`.
    pub fn of(services: &caskade_app::start::Services, events: Events) -> Self {
        Self::new(AddonParts {
            catalogue: services.catalogue.clone(),
            plugins: services.plugins.clone(),
            themes: services.themes.clone(),
            settings: services.settings.clone(),
            store: services.store.clone(),
            bus: services.bus.clone(),
            events,
            folders: AddonFolders {
                plugins: services.addon_folders.plugins.clone(),
                themes: services.addon_folders.themes.clone(),
                work: services.addon_folders.work.clone(),
            },
            media: AddonMedia(
                services
                    .addon_media
                    .as_ref()
                    .and_then(|media| media.0.clone()),
            ),
            sync: Some(services.sync.clone()),
            devices: Some(services.devices.clone()),
        })
    }

    fn services(&self) -> AddonServices<'_> {
        let parts = &self.0;
        AddonServices {
            events: parts.events.clone(),
            catalogue: parts.catalogue.as_ref(),
            plugins: parts.plugins.as_ref(),
            themes: Some(&parts.themes),
            bus: Some(&parts.bus),
            settings: Some(&parts.settings),
        }
    }

    /// The catalogue as it fits this device in `mode`.
    pub async fn catalogue(&self, mode: AppMode) -> Result<CatalogueView> {
        let parts = &self.0;
        catalogue::addons_catalogue(
            parts.catalogue.as_ref(),
            parts.plugins.as_ref(),
            Some(&parts.themes),
            mode,
        )
        .await
    }

    /// Fetches the catalogue again, then reads it.
    pub async fn refresh(&self, mode: AppMode) -> Result<CatalogueView> {
        let parts = &self.0;
        catalogue::addons_catalogue_refresh(
            parts.catalogue.as_ref(),
            parts.plugins.as_ref(),
            Some(&parts.themes),
            mode,
        )
        .await
    }

    pub async fn records(&self) -> Result<AddonRecords> {
        packages::addons_records(&self.0.store).await
    }

    pub async fn set_update_preference(&self, preference: UpdatePreference) -> Result<()> {
        packages::addons_set_update_preference(&self.0.store, preference)
            .await
            .map(|_| ())
    }

    /// Skips `version` of `id`'s updates; `None` offers them again.
    pub async fn skip_update(&self, id: &str, version: Option<String>) -> Result<()> {
        packages::addons_skip_update(&self.0.store, id.to_owned(), version)
            .await
            .map(|_| ())
    }

    /// Installs or updates `id`. A plugin whose permissions need a review
    /// comes back `NeedsReview` until `accept`.
    pub async fn install(
        &self,
        id: &str,
        mode: AppMode,
        accept: bool,
        retry: bool,
    ) -> Result<AddonOutcome> {
        let parts = &self.0;
        packages::addons_install(
            &self.services(),
            &parts.folders,
            &parts.jobs,
            &parts.store,
            id.to_owned(),
            mode,
            accept,
            retry,
        )
        .await
    }

    /// Stops `id`'s download; whether one was running.
    pub fn cancel(&self, id: &str) -> bool {
        packages::addons_install_cancel(&self.0.jobs, id.to_owned())
    }

    pub async fn uninstall(&self, id: &str, keep_data: bool) -> Result<()> {
        let parts = &self.0;
        packages::addons_uninstall(
            &self.services(),
            &parts.folders,
            &parts.store,
            id.to_owned(),
            keep_data,
        )
        .await
    }

    /// Screenshot `index` of `id`, cached; `None` when it can't be had.
    pub async fn media(&self, id: &str, index: u32) -> Result<Option<CachedArtwork>> {
        catalogue::addons_media(self.0.catalogue.as_ref(), &self.0.media, id, index).await
    }

    /// What the user's other devices have and this one doesn't.
    pub async fn from_devices(&self) -> Result<Vec<DeviceAddon>> {
        addon_offers::addons_from_devices(self.0.sync.as_ref(), self.0.devices.as_ref()).await
    }
}

impl PartialEq for Addons {
    /// One catalogue per app: a view's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Addons {
    /// The catalogue service, for tests that give it a catalogue.
    pub fn service(&self) -> Option<&CatalogueService> {
        self.0.catalogue.as_ref()
    }
}
